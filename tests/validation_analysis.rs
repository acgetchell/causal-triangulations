//! Volume fixing, circular-loop measurement, chain artifacts, and MCMC diagnostic adapters.

use approx::assert_relative_eq;
use causal_triangulations::prelude::action::{ActionConfig, VolumeFixing, VolumeFixingError};
use causal_triangulations::prelude::observables::*;
use causal_triangulations::prelude::simulation::{
    CdtMcmcCheckpoint, CdtMoveFamilyDistribution, CdtProposal, CdtTarget, Measurement,
    MetropolisAlgorithm, MetropolisConfig, MoveType, SimulationResultsBackend,
};
use causal_triangulations::{
    AnalysisArtifactOperation, CdtAnalysisError, CdtConfig, CdtError, CdtTopology,
    CheckpointResumeFailure, DiagnosticPolicyField, MeasurementCountField, SpatialLoopError,
    TriangulationQuery, VALIDATION_CHAIN_SCHEMA_VERSION, ValidationChainData, run_simulation,
};
use markov_chain_monte_carlo::{
    DelayedProposal, DiagnosticTiming, EssEstimate, EssEstimator, Target,
};
use num_traits::ToPrimitive;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};
use std::{
    assert_matches,
    f64::consts::LN_2,
    io::{self, Write},
    time::Duration,
};

/// Models an output destination that refuses a write before a complete artifact exists.
struct Unwritable;

impl Write for Unwritable {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::PermissionDenied.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn policy() -> CdtDiagnosticPolicy {
    CdtDiagnosticPolicy {
        maximum_lag: 128,
        minimum_completed_blocks: 16,
        stabilization_levels: 3,
        binning_relative_tolerance: 0.2,
        require_stable_binning: false,
        minimum_chains: 8,
        minimum_ess: 100.0,
        maximum_mean_mcse: 20.0,
        quantiles: [0.05, 0.5, 0.95]
            .into_iter()
            .map(|probability| QuantileRequirement {
                probability,
                maximum_mcse: 30.0,
            })
            .collect(),
    }
}

/// Synthetic profile transport isolates CDT observable selection from generic estimator math.
fn synthetic_chain(seed: u64, lengths: &[u32]) -> ValidationChain {
    let config = CdtConfig {
        topology: CdtTopology::Toroidal,
        simulate: true,
        seed: Some(seed),
        steps: u32::try_from(lengths.len()).expect("bounded fixture"),
        thermalization_steps: 1,
        measurement_frequency: 1,
        ..CdtConfig::new(16, 4)
    };
    let action = config
        .clone()
        .into_validated()
        .expect("synthetic configuration")
        .to_action_config();
    let measurements = lengths
        .iter()
        .enumerate()
        .map(|(row, &length)| {
            let profile = vec![length, 4, 4, 4];
            let vertices: u32 = profile.iter().sum();
            let triangles = 2 * vertices;
            let edges = 3 * vertices;
            let physical =
                action.physical_action(vertices as usize, edges as usize, triangles as usize);
            let slab = (0..4)
                .map(|slice| profile[slice] + profile[(slice + 1) % 4])
                .collect();
            Measurement::try_new(
                u32::try_from(row + 1).expect("bounded row"),
                physical,
                vertices,
                edges,
                triangles,
            )
            .expect("synthetic measurement")
            .try_with_slab_triangle_profile(slab)
            .expect("slab profile")
            .try_with_spatial_loop_lengths(profile)
            .expect("loop profile")
        })
        .collect();
    ValidationChain::try_new(ValidationChainData {
        schema_version: VALIDATION_CHAIN_SCHEMA_VERSION,
        crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        spatial_topology: "S1".to_owned(),
        temporal_boundary: "periodic".to_owned(),
        minimum_loop_length: 3,
        action_convention: "-kappa_0*N0-kappa_2*N2+lambda_edge*N1".to_owned(),
        run_id: "synthetic-adapter-fixture".to_owned(),
        chain_id: format!("chain-{seed}"),
        configuration: config,
        measurements,
    })
    .expect("complete synthetic chain")
}

#[test]
fn chain_json_preserves_typed_measurement_failures() {
    let chain = synthetic_chain(7, &[4, 4]);
    let valid = serde_json::to_value(chain.data()).expect("valid transport");
    for (key, expected_field) in [
        ("vertices", MeasurementCountField::Vertices),
        ("edges", MeasurementCountField::Edges),
        ("triangles", MeasurementCountField::Triangles),
    ] {
        let mut invalid = valid.clone();
        invalid["measurements"][0][key] = json!(0);
        let bytes = serde_json::to_vec(&invalid).expect("syntactically valid JSON");
        assert_matches!(
            ValidationChain::read_json(bytes.as_slice()),
            Err(CdtError::InvalidMeasurementCount { field, provided_value: 0 })
                if field == expected_field
        );
    }
    let mut invalid = valid;
    invalid["measurements"][0]["slab_triangle_profile"] = json!([33, 0, 0, 0]);
    let bytes = serde_json::to_vec(&invalid).expect("syntactically valid JSON");
    assert_matches!(
        ValidationChain::read_json(bytes.as_slice()),
        Err(CdtError::InvalidMeasurementSlabTriangleProfile {
            step: 1,
            profile_total: 33,
            triangles: 32,
        })
    );
    assert_matches!(
        ValidationChain::read_json(b"{\"measurements\":[".as_slice()),
        Err(CdtError::Analysis(CdtAnalysisError::Serialization {
            operation: AnalysisArtifactOperation::Read,
            ..
        }))
    );
}

fn independent_lengths(seed: u64, count: usize) -> Vec<u32> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..count).map(|_| rng.random_range(3..=1002)).collect()
}

#[test]
fn quadratic_bias_is_opt_in_validated_and_independently_recomputed() {
    assert!(ActionConfig::default().volume_fixing().is_none());
    let fixing = VolumeFixing::quadratic_triangles(32, 0.25).expect("valid bias");
    for (count, expected) in [(28, 4.0), (30, 1.0), (32, 0.0), (34, 1.0), (36, 4.0)] {
        assert_relative_eq!(fixing.action(count), expected, epsilon = 1e-14);
    }
    for target in [0, 1, 31, u32::MAX] {
        assert_matches!(
            VolumeFixing::quadratic_triangles(target, 0.1),
            Err(VolumeFixingError::InvalidTarget { .. })
        );
    }
    for strength in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, f64::MAX] {
        assert_matches!(
            VolumeFixing::quadratic_triangles(32, strength),
            Err(VolumeFixingError::InvalidStrength { .. })
        );
    }
    assert_relative_eq!(
        VolumeFixing::quadratic_triangles(32, 0.0)
            .expect("zero explicit strength")
            .action(36),
        0.0
    );
    let encoded = serde_json::to_string(&fixing).expect("encode bias");
    let restored: VolumeFixing = serde_json::from_str(&encoded).expect("validated decoding");
    assert_eq!(restored.target(), fixing.target());
    assert_relative_eq!(restored.strength(), fixing.strength());
    assert!(
        serde_json::from_value::<VolumeFixing>(
            json!({"mode":"quadratic-triangles","target":31,"strength":0.1})
        )
        .is_err()
    );
}

#[test]
fn action_configuration_json_preserves_every_floating_point_bit() {
    let original = fixed_action();
    let encoded = serde_json::to_string(&original).expect("encode action");
    let restored: ActionConfig = serde_json::from_str(&encoded).expect("decode action");
    assert_eq!(
        original.cosmological_constant().to_bits(),
        restored.cosmological_constant().to_bits()
    );
    assert_eq!(
        original
            .volume_fixing()
            .expect("explicit bias")
            .strength()
            .to_bits(),
        restored
            .volume_fixing()
            .expect("restored bias")
            .strength()
            .to_bits()
    );
}

#[test]
fn production_fixed_and_bare_chains_feed_the_same_dependence_adapter() {
    for fixing in [None, fixed_action().volume_fixing()] {
        let chains: Vec<_> = (0..2)
            .map(|chain| {
                let profile = if chain == 0 {
                    vec![4, 4, 4, 4]
                } else {
                    vec![3, 5, 4, 4]
                };
                let configuration = CdtConfig {
                    topology: CdtTopology::Toroidal,
                    spatial_vertex_profile: Some(profile),
                    volume_fixing: fixing,
                    simulate: true,
                    seed: Some(131 + chain),
                    steps: 32,
                    thermalization_steps: 4,
                    measurement_frequency: 1,
                    ..CdtConfig::new(16, 4)
                }
                .into_validated()
                .expect("production fixture configuration");
                let results = run_simulation(&configuration).expect("production sampler");
                ValidationChain::from_results(
                    "production-fixtures",
                    &format!("chain-{chain}"),
                    &configuration,
                    &results,
                )
                .expect("public artifact")
            })
            .collect();
        for observable in [
            CdtDiagnosticObservable::Triangles,
            CdtDiagnosticObservable::LoopLength { slice: 0 },
            CdtDiagnosticObservable::TargetProfileIndicator {
                target: 32,
                profile: vec![4, 4, 4, 4],
            },
        ] {
            let report = analyze_cdt_chains(&chains, observable, &policy(), None)
                .expect("same public adapter");
            assert_eq!(report.dependence.len(), 2);
            assert!(
                report
                    .dependence
                    .iter()
                    .all(|chain| chain.sample_count == 29)
            );
            assert_ne!(report.verdict, CdtDiagnosticVerdict::SufficientInformation);
        }
    }
}

#[test]
fn fixing_configuration_rejects_open_and_unreachable_ensembles() {
    let fixing = Some(VolumeFixing::quadratic_triangles(32, 0.1).expect("bias"));
    assert_matches!(
        CdtConfig {
            volume_fixing: fixing,
            ..CdtConfig::new(16, 4)
        }
        .into_validated(),
        Err(CdtError::VolumeFixing(
            VolumeFixingError::UnsupportedTopology { .. }
        ))
    );
    assert_matches!(
        CdtConfig {
            topology: CdtTopology::Toroidal,
            volume_fixing: Some(
                VolumeFixing::quadratic_triangles(22, 0.1).expect("positive even target")
            ),
            ..CdtConfig::new(16, 4)
        }
        .into_validated(),
        Err(CdtError::VolumeFixing(
            VolumeFixingError::TargetBelowMinimum { minimum: 24, .. }
        ))
    );
}

#[test]
#[cfg(feature = "cli")]
fn fixing_cli_requires_paired_parameters_and_preserves_target() {
    assert!(CdtConfig::try_from_args(["cdt", "--target-triangles", "32"]).is_err());
    assert!(CdtConfig::try_from_args(["cdt", "--volume-fixing-strength", "0.1"]).is_err());
    let config = CdtConfig::try_from_args([
        "cdt",
        "--topology",
        "toroidal",
        "--vertices",
        "16",
        "--timeslices",
        "4",
        "--target-triangles",
        "32",
        "--volume-fixing-strength",
        "0.1",
    ])
    .expect("paired CLI arguments")
    .into_validated()
    .expect("periodic bias config");
    assert_eq!(
        config
            .to_action_config()
            .volume_fixing()
            .map(|fixing| fixing.target().get()),
        Some(32)
    );
}

#[test]
fn spatial_loop_lengths_agree_with_independent_spacelike_edge_incidence() {
    for profile in [&[4, 4, 4, 4][..], &[3, 5, 4, 4][..]] {
        let state = CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(profile)
            .expect("periodic fixture");
        let mut links = vec![0u32; profile.len()];
        for edge in state.geometry().edges() {
            let (left, right) = state
                .geometry()
                .edge_endpoints(&edge)
                .expect("live endpoints");
            let left = state
                .time_label(&left)
                .expect("left label")
                .expect("foliation");
            let right = state
                .time_label(&right)
                .expect("right label")
                .expect("foliation");
            if left == right {
                links[left as usize] += 1;
            }
        }
        assert_eq!(
            spatial_loop_lengths(&state).expect("closed observable"),
            links
        );
        assert_eq!(links, profile);
        assert_eq!(
            2 * links.iter().map(|&length| u64::from(length)).sum::<u64>(),
            state.face_count() as u64
        );
        assert_ne!(
            links,
            state
                .slab_triangle_profile()
                .expect("different slab observable")
        );
    }
    let open = CdtTriangulation::from_cdt_strip(4, 4).expect("open fixture");
    assert_matches!(
        spatial_loop_lengths(&open),
        Err(CdtError::SpatialLoop(SpatialLoopError::UnsupportedTopology))
    );
}

#[test]
fn volume_fixed_target_has_physical_bias_and_temperature_terms() {
    let state = CdtTriangulation::from_toroidal_cdt(4, 4).expect("periodic state");
    let fixing = VolumeFixing::quadratic_triangles(40, 0.25).expect("bias");
    let action = ActionConfig::new(0.2, 0.3, 0.4)
        .expect("physical action")
        .with_volume_fixing(Some(fixing))
        .expect("combined action");
    let target = CdtTarget::new(action.clone(), 2.0).expect("finite target");
    // (-1/5)*16 - (3/10)*32 + (2/5)*48 = 32/5 independently of production FMA.
    let physical = 6.4;
    let bias = 0.25 * 64.0;
    assert_relative_eq!(
        target.log_prob(&state),
        -(physical + bias) / 2.0,
        epsilon = 1e-12
    );
    assert_relative_eq!(
        action.physical_action(16, 48, 32),
        physical,
        epsilon = 1e-12
    );
    let open = CdtTriangulation::from_cdt_strip(4, 4).expect("open state");
    assert!(target.log_prob(&open).is_infinite() && target.log_prob(&open).is_sign_negative());
}

fn fixed_action() -> ActionConfig {
    ActionConfig::default()
        .with_volume_fixing(Some(
            VolumeFixing::quadratic_triangles(32, 0.1).expect("bias"),
        ))
        .expect("combined action")
}

#[test]
fn biased_concrete_proposal_preserves_physical_fixing_and_hastings_ratio_algebra() {
    // This checks the declared ratio algebra, not independently enumerated reverse
    // support. The original counterexample remains in tests/regressions.rs (#316).
    let state = CdtTriangulation::from_toroidal_cdt(4, 4).expect("periodic flux fixture");
    let action = ActionConfig::new(0.125, -0.0625, 0.25)
        .expect("binary-rational physical action")
        .with_volume_fixing(Some(
            VolumeFixing::quadratic_triangles(32, 0.25).expect("bias"),
        ))
        .expect("combined action");
    let distribution =
        CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 5.0, 2.0]).expect("unequal families");
    let mut proposal = CdtProposal::new(action.clone())
        .with_seed(19)
        .with_policy(distribution);
    let mut rng = StdRng::seed_from_u64(71);
    let mut selected = None;
    for _ in 0..256 {
        if let Some(plan) = proposal
            .propose_plan(&state, &mut rng)
            .expect("proposal planning")
            && plan.move_type() == MoveType::Move13Add
        {
            selected = Some(plan);
            break;
        }
    }
    let plan = selected.expect("fixture must offer a concrete insertion");
    assert!(plan.reverse_site_count() > 0);
    let forward = distribution.probability(MoveType::Move13Add)
        / f64::from(u32::try_from(plan.forward_site_count()).expect("bounded support"));
    let reverse = distribution.probability(MoveType::Move31Remove)
        / f64::from(u32::try_from(plan.reverse_site_count()).expect("bounded support"));
    assert_relative_eq!(plan.delta_action(), 1.75, epsilon = 1e-14);
    assert_relative_eq!(
        plan.log_proposal_ratio(),
        (reverse / forward).ln(),
        epsilon = 1e-14
    );
    let temperature = 2.0;
    let before_weight = (-12.0_f64 / temperature).exp();
    let after_weight = (-13.75_f64 / temperature).exp();
    let log_alpha = -1.75 / temperature + (reverse / forward).ln();
    let forward_flux = before_weight * forward * log_alpha.min(0.0).exp();
    let reverse_flux = after_weight * reverse * (-log_alpha).min(0.0).exp();
    assert_relative_eq!(forward_flux, reverse_flux, epsilon = 1e-14);
    let mut after = state;
    proposal
        .commit(&mut after, plan, &mut rng)
        .expect("commit supported plan");
    assert_eq!(
        (after.vertex_count(), after.edge_count(), after.face_count()),
        (17, 51, 34)
    );
    let target = CdtTarget::new(action, temperature).expect("finite target");
    assert_relative_eq!(
        target.log_prob(&after),
        -13.75 / temperature,
        epsilon = 1e-14
    );
}

#[test]
fn unavailable_folded_component_and_insufficient_information_remain_inconclusive() {
    let chains: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &[3, 5].repeat(512)))
        .collect();
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("symmetric two-point data");
    assert!(report.rank_normalized_rhat.value().is_some());
    assert!(report.folded_rhat.value().is_none());
    assert!(report.combined_rhat.value().is_none());
    assert_eq!(report.verdict, CdtDiagnosticVerdict::Inconclusive);
    let independent: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &independent_lengths(seed, 129)))
        .collect();
    let mut required = policy();
    required.minimum_ess = 1.0e12;
    let report = analyze_cdt_chains(
        &independent,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &required,
        None,
    )
    .expect("insufficient information report");
    assert_eq!(report.verdict, CdtDiagnosticVerdict::Inconclusive);
    required = policy();
    required.require_stable_binning = true;
    required.minimum_completed_blocks = usize::MAX;
    let report = analyze_cdt_chains(
        &independent,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &required,
        None,
    )
    .expect("unresolved binning report");
    assert!(
        report
            .dependence
            .iter()
            .all(|chain| chain.selected_binning_level.value().is_none())
    );
    assert_eq!(report.verdict, CdtDiagnosticVerdict::Inconclusive);
}

#[test]
fn blocked_errors_and_mean_mcse_scale_in_original_units_and_exclude_incomplete_blocks() {
    let original: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &independent_lengths(seed, 129)))
        .collect();
    let scaled: Vec<_> = (0..8)
        .map(|seed| {
            let values: Vec<_> = independent_lengths(seed, 129)
                .into_iter()
                .map(|value| 3 + 10 * (value - 3))
                .collect();
            synthetic_chain(seed, &values)
        })
        .collect();
    let before = analyze_cdt_chains(
        &original,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("original units");
    let after = analyze_cdt_chains(
        &scaled,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("scaled units");
    assert_relative_eq!(
        after.mean_mcse.value().expect("scaled MCSE").standard_error,
        10.0 * before
            .mean_mcse
            .value()
            .expect("original MCSE")
            .standard_error,
        epsilon = 1e-10
    );
    assert_eq!(before.dependence.len(), 8, "one diagnostic per input chain");
    assert_eq!(after.dependence.len(), 8, "one diagnostic per scaled chain");
    for (before, after) in before.dependence.iter().zip(&after.dependence) {
        let before = before
            .binning
            .value()
            .expect("binning levels")
            .iter()
            .find(|level| level.block_size == 8)
            .expect("eight-draw blocks");
        let after = after
            .binning
            .value()
            .expect("scaled binning")
            .iter()
            .find(|level| level.block_size == 8)
            .expect("eight-draw blocks");
        assert_eq!(before.completed_blocks, 16);
        assert_eq!(after.completed_blocks, 16);
        assert_relative_eq!(
            after.blocked_mean_error.expect("scaled blocked error"),
            10.0 * before.blocked_mean_error.expect("original blocked error"),
            epsilon = 1e-10
        );
    }
}

fn checkpoint_without_time(checkpoint: &CdtMcmcCheckpoint) -> Value {
    let mut value: Value =
        serde_json::from_str(&checkpoint.to_json().expect("checkpoint JSON")).expect("JSON object");
    value
        .as_object_mut()
        .expect("checkpoint object")
        .remove("elapsed_time");
    value
}

/// Reports the first differing field while retaining whole-checkpoint comparison.
fn assert_checkpoint_json_matches(left: &Value, right: &Value, path: &str) {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            assert_eq!(
                left.keys().collect::<Vec<_>>(),
                right.keys().collect::<Vec<_>>(),
                "keys at {path}"
            );
            for (key, value) in left {
                assert_checkpoint_json_matches(value, &right[key], &format!("{path}/{key}"));
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            assert_eq!(left.len(), right.len(), "length at {path}");
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                assert_checkpoint_json_matches(left, right, &format!("{path}/{index}"));
            }
        }
        _ => assert_eq!(left, right, "value at {path}"),
    }
}

type VertexSignature = (Option<u32>, Vec<u64>);
type FaceSignature = (Option<i32>, Vec<VertexSignature>);

/// Ignores fresh random UUIDs while retaining bit-exact coordinates, labels and connectivity.
fn canonical_geometry(
    state: &causal_triangulations::CdtTriangulation2D,
) -> (Vec<VertexSignature>, Vec<FaceSignature>) {
    let geometry = state.geometry();
    let signature = |vertex| {
        (
            state.time_label(&vertex).expect("live label"),
            geometry
                .vertex_coordinates(&vertex)
                .expect("live coordinates")
                .iter()
                .copied()
                .map(f64::to_bits)
                .collect(),
        )
    };
    let mut vertices: Vec<_> = geometry.vertices().map(signature).collect();
    vertices.sort();
    let mut faces: Vec<_> = geometry
        .faces()
        .map(|face| {
            let mut vertices: Vec<_> = geometry
                .face_vertices(&face)
                .expect("live face")
                .map(signature)
                .collect();
            let first = vertices
                .iter()
                .enumerate()
                .min_by_key(|(_, vertex)| *vertex)
                .map(|(index, _)| index)
                .expect("triangle vertices");
            vertices.rotate_left(first);
            (
                geometry
                    .simplex_data(&face)
                    .expect("live simplex classification"),
                vertices,
            )
        })
        .collect();
    faces.sort();
    (vertices, faces)
}

#[test]
fn fixed_checkpoint_restores_exact_trajectory() {
    let state = CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(&[3, 5, 4, 4])
        .expect("nonuniform start");
    let action = fixed_action();
    let config = |steps| {
        MetropolisConfig::new(1.0, steps, 4, 2)
            .expect("sampling schedule")
            .with_seed(41)
    };
    let full = MetropolisAlgorithm::new(config(32), action.clone())
        .run_to_checkpoint(state.clone())
        .expect("full fixed run");
    let prefix = MetropolisAlgorithm::new(config(16), action.clone())
        .run_to_checkpoint(state)
        .expect("prefix run");
    let mut downgraded: Value =
        serde_json::from_str(&prefix.to_json().expect("complete checkpoint")).expect("valid JSON");
    assert_eq!(downgraded["format_version"], 3);
    downgraded["format_version"] = json!(2);
    assert_matches!(
        CdtMcmcCheckpoint::from_json(&downgraded.to_string()).map(drop),
        Err(CdtError::UnsupportedCheckpointVersion {
            encountered: 2,
            supported: 3
        })
    );
    let restored = CdtMcmcCheckpoint::from_json(&prefix.to_json().expect("encode prefix"))
        .expect("restore prefix");
    let in_memory = MetropolisAlgorithm::new(config(16), action.clone())
        .resume_to_checkpoint(prefix.clone())
        .expect("in-memory continuation");
    assert_checkpoint_json_matches(
        &serde_json::to_value(full.steps()).expect("full steps"),
        &serde_json::to_value(in_memory.steps()).expect("in-memory steps"),
        "/in-memory-steps",
    );
    assert_checkpoint_json_matches(
        &checkpoint_without_time(&prefix),
        &checkpoint_without_time(&restored),
        "/restored-prefix",
    );
    assert_checkpoint_json_matches(
        &serde_json::to_value(&full.steps()[..16]).expect("full prefix steps"),
        &serde_json::to_value(prefix.steps()).expect("prefix steps"),
        "/prefix-steps",
    );
    let resumed = MetropolisAlgorithm::new(config(16), action)
        .resume_to_checkpoint(restored)
        .expect("exact continuation");
    assert_checkpoint_json_matches(
        &serde_json::to_value(full.steps()).expect("full steps"),
        &serde_json::to_value(resumed.steps()).expect("resumed steps"),
        "/steps",
    );
    assert_eq!(
        canonical_geometry(full.triangulation()),
        canonical_geometry(resumed.triangulation())
    );
    // Delaunay make_uuid() uses UUIDv4 independently of the sampler RNG. The
    // prefix above must restore byte-for-byte; new entities after that prefix
    // have fresh opaque IDs. Compare their canonical geometry separately.
    let mut full_json = checkpoint_without_time(&full);
    let mut resumed_json = checkpoint_without_time(&resumed);
    full_json["triangulation"]["geometry"]["triangulation"]["tds"] = Value::Null;
    resumed_json["triangulation"]["geometry"]["triangulation"]["tds"] = Value::Null;
    assert_checkpoint_json_matches(&full_json, &resumed_json, "");
    for measurement in resumed.measurements() {
        assert_relative_eq!(
            measurement.action(),
            measurement.physical_action() + measurement.fixing_action(),
            epsilon = 1e-11
        );
        assert_eq!(
            2 * measurement
                .spatial_loop_lengths()
                .expect("ring profile")
                .iter()
                .sum::<u32>(),
            measurement.triangles().get()
        );
    }
}

#[test]
fn fixed_checkpoint_rejects_changed_bias_and_forged_components() {
    let state = CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(&[3, 5, 4, 4])
        .expect("nonuniform start");
    let config = MetropolisConfig::new(1.0, 16, 4, 2)
        .expect("schedule")
        .with_seed(41);
    let prefix = MetropolisAlgorithm::new(config.clone(), fixed_action())
        .run_to_checkpoint(state)
        .expect("fixed prefix");
    for fixing in [
        None,
        Some(VolumeFixing::quadratic_triangles(34, 0.1).expect("changed target")),
        Some(VolumeFixing::quadratic_triangles(32, 0.2).expect("changed strength")),
        Some(VolumeFixing::quadratic_triangles(32, 0.1_f64.next_up()).expect("one-ULP change")),
    ] {
        let changed_action = ActionConfig::default()
            .with_volume_fixing(fixing)
            .expect("valid action");
        assert_matches!(
            MetropolisAlgorithm::new(config.clone(), changed_action)
                .resume_to_checkpoint(prefix.clone())
                .map(drop),
            Err(CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::IncompatibleActionConfiguration,
            })
        );
    }
    let mut corrupted: Value =
        serde_json::from_str(&prefix.to_json().expect("complete checkpoint")).expect("valid JSON");
    corrupted["measurements"][0]["fixing_action"] = json!(100.0);
    assert_matches!(
        CdtMcmcCheckpoint::from_json(&corrupted.to_string()).map(drop),
        Err(CdtError::InvalidMeasurementActionComponents { .. })
    );
}

#[test]
fn review_regression_restore_rejects_forged_loops_through_unchanged_volume_spans() {
    for steps in [4, 5] {
        let state = CdtTriangulation::from_toroidal_cdt(4, 4).expect("flat torus");
        let policy = CdtMoveFamilyDistribution::from_weights([1.0, 0.0, 0.0, 0.0])
            .expect("edge flips preserve every slice size");
        let checkpoint = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, steps, 0, 2)
                .expect("step-zero and two-step measurement cadence")
                .with_seed(7),
            ActionConfig::default(),
        )
        .with_policy(policy)
        .run_to_checkpoint(state)
        .expect("volume-preserving chain");
        assert_eq!(checkpoint.triangulation().slice_sizes(), &[4, 4, 4, 4]);
        assert_eq!(
            checkpoint
                .measurements()
                .iter()
                .map(Measurement::step)
                .collect::<Vec<_>>(),
            [0, 2, 4]
        );
        let checkpoint_json: Value =
            serde_json::from_str(&checkpoint.to_json().expect("checkpoint JSON"))
                .expect("checkpoint transport");
        CdtMcmcCheckpoint::from_json(&checkpoint_json.to_string())
            .expect("valid profiles must round trip");
        let results = checkpoint.into_results().expect("validated results");
        let results_json = serde_json::to_value(&results).expect("results transport");
        serde_json::from_value::<SimulationResultsBackend>(results_json.clone())
            .expect("valid result profiles must round trip");

        for index in 0..3 {
            // Both profiles have N2=32 and slab counts [8, 8, 8, 8]. Only
            // canonical slice sizes distinguish the forgery from [4, 4, 4, 4].
            let mut forged_checkpoint = checkpoint_json.clone();
            forged_checkpoint["measurements"][index]["spatial_loop_lengths"] = json!([3, 5, 3, 5]);
            assert_matches!(
                CdtMcmcCheckpoint::from_json(&forged_checkpoint.to_string()).map(drop),
                Err(CdtError::SpatialLoop(SpatialLoopError::InvalidProfile { profile, triangles: 32 }))
                    if profile == [3, 5, 3, 5]
            );

            let mut forged_results = results_json.clone();
            forged_results["measurements"][index]["spatial_loop_lengths"] = json!([3, 5, 3, 5]);
            let error = serde_json::from_value::<SimulationResultsBackend>(forged_results)
                .expect_err("unchanged-volume measurements must match canonical slice sizes");
            assert!(
                error.to_string().contains("invalid loop profile"),
                "unexpected result restore error: {error}"
            );
        }
    }
}

#[test]
fn released_shape_run_exports_complete_reproducible_chain_json() {
    let config = CdtConfig {
        topology: CdtTopology::Toroidal,
        spatial_vertex_profile: Some(vec![3, 5, 4, 4]),
        simulate: true,
        seed: Some(72),
        steps: 24,
        thermalization_steps: 4,
        measurement_frequency: 2,
        volume_fixing: fixed_action().volume_fixing(),
        ..CdtConfig::new(16, 4)
    }
    .into_validated()
    .expect("run config");
    let results = run_simulation(&config).expect("volume-fixed run");
    let chain = ValidationChain::from_results("fixture-run", "nonuniform-72", &config, &results)
        .expect("chain metadata and measurements");
    let mut detached = config.config().clone();
    detached.coupling_0 = 1.0;
    assert_matches!(
        ValidationChain::from_results(
            "fixture-run", "detached", &detached.into_validated().expect("valid separate action"), &results,
        ),
        Err(CdtError::Analysis(CdtAnalysisError::IncompatibleCondition { chain }))
            if chain == "detached"
    );
    let mut bytes = Vec::new();
    chain.write_json(&mut bytes).expect("write stable JSON");
    let restored = ValidationChain::read_json(bytes.as_slice()).expect("external JSON consumer");
    assert_eq!(
        restored.data().measurements.len(),
        results.measurements().len()
    );
    assert_eq!(
        restored.data().configuration.spatial_vertex_profile,
        Some(vec![3, 5, 4, 4])
    );
    for measurement in restored.at_exact_triangles(32) {
        assert_eq!(measurement.triangles().get(), 32);
    }
    let mut bad = restored.data().clone();
    bad.measurements.remove(2);
    assert_matches!(
        ValidationChain::try_new(bad),
        Err(CdtError::Analysis(CdtAnalysisError::Cadence { .. }))
    );
    let mut bad = restored.data().clone();
    bad.measurements.pop();
    assert_matches!(
        ValidationChain::try_new(bad),
        Err(CdtError::Analysis(CdtAnalysisError::Cadence { .. }))
    );
    let mut bad = restored.data().clone();
    bad.schema_version += 1;
    assert_matches!(
        ValidationChain::try_new(bad),
        Err(CdtError::Analysis(CdtAnalysisError::Schema { .. }))
    );
    assert!(ValidationChain::read_json(&bytes[..bytes.len() / 2]).is_err());
}

#[test]
fn independent_chain_diagnostics_preserve_upstream_identity_counts_and_units() {
    let chains: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &independent_lengths(seed, 1025)))
        .collect();
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("independent analysis");
    let raw: Vec<Vec<f64>> = chains
        .iter()
        .map(|chain| {
            chain
                .data()
                .measurements
                .iter()
                .map(|measurement| {
                    f64::from(measurement.spatial_loop_lengths().expect("loop profile")[0])
                })
                .collect()
        })
        .collect();
    let slices: Vec<_> = raw.iter().map(Vec::as_slice).collect();
    let upstream = EssEstimate::estimate(&slices, EssEstimator::Mean).expect("raw mean ESS");
    let mean = report.mean_ess.value().expect("mean ESS");
    assert_relative_eq!(mean.effective_samples, upstream.value());
    assert_eq!(mean.original_samples, 8 * 1025);
    assert_eq!(mean.retained_samples, 8 * 1024);
    assert_eq!(mean.estimator, "Mean");
    assert_eq!(report.observable_units, "spacelike-links");
    assert!(
        report
            .combined_rhat
            .value()
            .is_some_and(|&value| value < 1.01)
    );
    assert_eq!(report.verdict, CdtDiagnosticVerdict::SufficientInformation);
    assert!(report.mean_ess_per_second.value().is_none());
    let mut json = Vec::new();
    report
        .write_json(&mut json)
        .expect("diagnostic report export");
    let value: Value = serde_json::from_slice(&json).expect("external report parser");
    assert_eq!(value["mean_ess"]["status"], "available");
    assert_eq!(value["mean_ess_per_second"]["status"], "unavailable");
}

#[test]
fn correlated_chains_have_lower_information_and_larger_mean_error() {
    let independent: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &independent_lengths(seed, 4096)))
        .collect();
    let correlated: Vec<_> = (0..8)
        .map(|seed| {
            let mut rng = StdRng::seed_from_u64(seed + 17);
            let mut current: f64 = 0.0;
            let lengths: Vec<_> = (0..4096)
                .map(|_| {
                    current = 0.8_f64.mul_add(current, rng.random_range(-100.0..100.0));
                    (current + 1000.0)
                        .round()
                        .to_u32()
                        .expect("positive bounded AR fixture")
                })
                .collect();
            synthetic_chain(seed + 100, &lengths)
        })
        .collect();
    let iid = analyze_cdt_chains(
        &independent,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("iid diagnostics");
    let ar = analyze_cdt_chains(
        &correlated,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("AR diagnostics");
    // AR(1) phi=.8 has tau=(1+phi)/(1-phi)=9. Quantization is tiny relative to spread.
    let times: Vec<_> = ar
        .dependence
        .iter()
        .map(|chain| *chain.integrated_time.value().expect("AR integrated time"))
        .collect();
    let average_time = times.iter().sum::<f64>() / 8.0;
    assert!((6.0..12.0).contains(&average_time), "AR tau={average_time}");
    assert!(
        ar.mean_ess.value().expect("AR ESS").effective_samples
            < iid.mean_ess.value().expect("iid ESS").effective_samples / 3.0
    );
    // Normalize each original-unit MCSE by its observed spread before comparing chains with different marginal scales.
    let ar_relative_error = ar.mean_mcse.value().expect("AR mean MCSE").standard_error / 100.0;
    let iid_relative_error =
        iid.mean_mcse.value().expect("iid mean MCSE").standard_error / (1000.0 / 12.0_f64.sqrt());
    assert!(ar_relative_error > 2.0 * iid_relative_error);
    assert!(
        ar.dependence
            .iter()
            .all(|chain| chain.binning.value().is_some_and(|levels| levels
                .iter()
                .any(|level| level.block_size == 8 && level.completed_blocks == 512)))
    );
}

#[test]
fn separated_locations_and_scales_cannot_receive_a_passing_verdict() {
    let locations: Vec<_> = (0..8)
        .map(|seed| {
            let values: Vec<_> = independent_lengths(seed, 1024)
                .into_iter()
                .map(|value| value + u32::try_from(seed * 1000).expect("bounded shift"))
                .collect();
            synthetic_chain(seed, &values)
        })
        .collect();
    let report = analyze_cdt_chains(
        &locations,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("location report");
    assert_eq!(report.verdict, CdtDiagnosticVerdict::NotConverged);
    let scale: Vec<_> = (0..8)
        .map(|seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            let width = if seed < 4 { 10 } else { 1000 };
            let values: Vec<u32> = (0..1024)
                .map(|_| rng.random_range(2000 - width..=2000 + width))
                .collect();
            synthetic_chain(seed, &values)
        })
        .collect();
    let report = analyze_cdt_chains(
        &scale,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("scale report");
    assert!(
        report
            .rank_normalized_rhat
            .value()
            .is_some_and(|&value| value < 1.01)
    );
    assert!(
        report
            .folded_rhat
            .value()
            .is_some_and(|&value| value > 1.01)
    );
    assert_eq!(report.verdict, CdtDiagnosticVerdict::NotConverged);
}

#[test]
fn constant_short_and_discrete_traces_preserve_unavailable_reasons() {
    for values in [vec![4; 128], vec![4, 5, 6]] {
        let chains: Vec<_> = (0..8).map(|seed| synthetic_chain(seed, &values)).collect();
        let report = analyze_cdt_chains(
            &chains,
            CdtDiagnosticObservable::LoopLength { slice: 0 },
            &policy(),
            None,
        )
        .expect("valid but unresolved traces");
        assert!(report.combined_rhat.value().is_none());
        assert_eq!(report.verdict, CdtDiagnosticVerdict::Inconclusive);
    }
    let chains: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &[3, 3, 3, 4].repeat(256)))
        .collect();
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy(),
        None,
    )
    .expect("tied discrete draws");
    assert_matches!(
        report.quantiles[0].mcse,
        DiagnosticEstimate::Unavailable { .. }
    );
    assert_ne!(report.verdict, CdtDiagnosticVerdict::SufficientInformation);
}

#[test]
fn metadata_policy_and_timing_do_not_silently_change_the_workload() {
    let chains: Vec<_> = (0..8)
        .map(|seed| synthetic_chain(seed, &independent_lengths(seed, 129)))
        .collect();
    assert!(
        analyze_cdt_chains(
            &chains[..1],
            CdtDiagnosticObservable::Triangles,
            &policy(),
            None
        )
        .is_err()
    );
    assert_matches!(
        analyze_cdt_chains(
            &[chains[0].clone(), chains[0].clone()],
            CdtDiagnosticObservable::Triangles,
            &policy(),
            None
        ),
        Err(CdtError::Analysis(CdtAnalysisError::DuplicateChain { .. }))
    );
    let mut changed = chains[1].data().clone();
    changed.configuration.temperature = 2.0;
    let changed = ValidationChain::try_new(changed).expect("valid separate condition");
    assert_matches!(
        analyze_cdt_chains(
            &[chains[0].clone(), changed],
            CdtDiagnosticObservable::Triangles,
            &policy(),
            None
        ),
        Err(CdtError::Analysis(
            CdtAnalysisError::IncompatibleCondition { .. }
        ))
    );
    let mut invalid = policy();
    invalid.minimum_ess = f64::NAN;
    assert_matches!(
        analyze_cdt_chains(&chains, CdtDiagnosticObservable::Triangles, &invalid, None),
        Err(CdtError::Analysis(CdtAnalysisError::Policy { .. }))
    );
    let timing =
        DiagnosticTiming::try_new(Duration::from_secs(2), 8, 129).expect("matching timing scope");
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::Triangles,
        &policy(),
        Some(&timing),
    )
    .expect("timed report");
    assert_relative_eq!(
        *report
            .mean_ess_per_second
            .value()
            .expect("matched ESS rate"),
        report.mean_ess.value().expect("mean ESS").effective_samples / 2.0
    );
    let wrong =
        DiagnosticTiming::try_new(Duration::from_secs(2), 8, 128).expect("different workload");
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::Triangles,
        &policy(),
        Some(&wrong),
    )
    .expect("explicit unavailable timing");
    assert!(report.mean_ess_per_second.value().is_none());
}

#[test]
fn artifact_metadata_and_serde_conversions_preserve_the_validated_boundary() {
    let chain = synthetic_chain(17, &[4, 5, 4, 5]);
    let expected = serde_json::to_value(chain.data()).expect("raw transport");
    let decoded: ValidationChain = serde_json::from_value(
        serde_json::to_value(&chain).expect("validated chain serialization"),
    )
    .expect("validated chain deserialization");
    let transported: ValidationChainData = decoded.into();
    assert_eq!(
        serde_json::to_value(&transported).expect("transport"),
        expected
    );
    ValidationChain::try_from(transported).expect("raw conversion reparses invariants");

    for field in ["run_id", "chain_id", "crate_version", "seed"] {
        let mut bad = expected.clone();
        if field == "seed" {
            bad["configuration"][field] = Value::Null;
        } else {
            bad[field] = json!("");
        }
        let data = serde_json::from_value(bad.clone()).expect("raw metadata transport");
        assert_matches!(
            ValidationChain::try_new(data),
            Err(CdtError::Analysis(CdtAnalysisError::MissingIdentity))
        );
        serde_json::from_value::<ValidationChain>(bad)
            .expect_err("Serde must not bypass identity validation");
    }
    for (field, value) in [
        ("spatial_topology", json!("S2")),
        ("temporal_boundary", json!("open")),
        ("minimum_loop_length", json!(2)),
        ("action_convention", json!("lambda*N2")),
    ] {
        let mut bad = expected.clone();
        bad[field] = value;
        assert_matches!(
            ValidationChain::try_new(serde_json::from_value(bad).expect("raw conventions")),
            Err(CdtError::Analysis(CdtAnalysisError::IncompatibleCondition { chain }))
                if chain == "chain-17"
        );
    }
    let mut open = chain.data().clone();
    open.configuration.topology = CdtTopology::OpenBoundary;
    assert_matches!(
        ValidationChain::try_new(open),
        Err(CdtError::Analysis(CdtAnalysisError::UnsupportedTopology))
    );
}

#[test]
fn artifact_rows_reject_detached_counts_profiles_and_action_conventions() {
    let chain = synthetic_chain(17, &[4, 5, 4, 5]);
    let transport = serde_json::to_value(chain.data()).expect("raw transport");
    for (field, value, vertices, edges) in [("vertices", 17, 17, 48), ("edges", 49, 16, 49)] {
        let mut bad = transport.clone();
        bad["measurements"][0][field] = json!(value);
        assert_matches!(
            ValidationChain::try_new(serde_json::from_value(bad).expect("raw counts")),
            Err(CdtError::Analysis(CdtAnalysisError::Counts {
                chain, row: 0, vertices: actual_vertices, edges: actual_edges, triangles: 32,
            })) if chain == "chain-17" && actual_vertices == vertices && actual_edges == edges
        );
    }
    for (field, value) in [
        ("spatial_loop_lengths", Value::Null),
        ("spatial_loop_lengths", json!([3, 3, 3, 3, 4])),
        ("slab_triangle_profile", json!([8, 8, 8])),
        ("slab_triangle_profile", json!([7, 9, 8, 8])),
    ] {
        let mut bad = transport.clone();
        bad["measurements"][0][field] = value;
        assert_matches!(
            ValidationChain::try_new(serde_json::from_value(bad).expect("raw profile")),
            Err(CdtError::Analysis(CdtAnalysisError::Profile { chain, row: 0 }))
                if chain == "chain-17"
        );
    }
    let mut different_action = chain.data().clone();
    different_action.configuration.cosmological_constant = 0.25;
    assert_matches!(
        ValidationChain::try_new(different_action),
        Err(CdtError::Analysis(CdtAnalysisError::IncompatibleCondition { chain }))
            if chain == "chain-17"
    );
}

#[test]
fn diagnostic_policy_rejects_invalid_fields_before_analyzing_samples() {
    let chains = [
        synthetic_chain(1, &[4, 5, 4, 5]),
        synthetic_chain(2, &[5, 4, 5, 4]),
    ];
    let check = |invalid: CdtDiagnosticPolicy, expected| {
        assert_matches!(
            analyze_cdt_chains(&chains, CdtDiagnosticObservable::Triangles, &invalid, None),
            Err(CdtError::Analysis(CdtAnalysisError::Policy { field })) if field == expected
        );
    };
    check(
        CdtDiagnosticPolicy {
            maximum_lag: 0,
            ..policy()
        },
        DiagnosticPolicyField::MaximumLag,
    );
    check(
        CdtDiagnosticPolicy {
            minimum_completed_blocks: 1,
            ..policy()
        },
        DiagnosticPolicyField::MinimumCompletedBlocks,
    );
    check(
        CdtDiagnosticPolicy {
            stabilization_levels: 1,
            ..policy()
        },
        DiagnosticPolicyField::StabilizationLevels,
    );
    check(
        CdtDiagnosticPolicy {
            minimum_chains: 1,
            ..policy()
        },
        DiagnosticPolicyField::MinimumChains,
    );
    for value in [f64::NAN, f64::INFINITY, -1.0] {
        check(
            CdtDiagnosticPolicy {
                binning_relative_tolerance: value,
                ..policy()
            },
            DiagnosticPolicyField::BinningRelativeTolerance,
        );
    }
    for value in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        check(
            CdtDiagnosticPolicy {
                minimum_ess: value,
                ..policy()
            },
            DiagnosticPolicyField::MinimumEss,
        );
        check(
            CdtDiagnosticPolicy {
                maximum_mean_mcse: value,
                ..policy()
            },
            DiagnosticPolicyField::MaximumMeanMcse,
        );
        check(
            CdtDiagnosticPolicy {
                quantiles: vec![QuantileRequirement {
                    probability: 0.5,
                    maximum_mcse: value,
                }],
                ..policy()
            },
            DiagnosticPolicyField::Quantiles,
        );
    }
    for probability in [f64::NAN, f64::INFINITY, -0.1, 0.0, 1.0] {
        check(
            CdtDiagnosticPolicy {
                quantiles: vec![QuantileRequirement {
                    probability,
                    maximum_mcse: 1.0,
                }],
                ..policy()
            },
            DiagnosticPolicyField::Quantiles,
        );
    }
    let exact_plateau = CdtDiagnosticPolicy {
        binning_relative_tolerance: 0.0,
        quantiles: Vec::new(),
        ..policy()
    };
    analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::Triangles,
        &exact_plateau,
        None,
    )
    .expect("zero plateau tolerance and supplementary quantiles are valid declarations");
}

#[test]
fn observable_selection_preserves_values_and_original_units() {
    let chains: Vec<_> = (1..=2)
        .map(|seed| {
            let mut data = synthetic_chain(seed, &[3, 4, 5, 6]).data().clone();
            data.configuration.volume_fixing =
                Some(VolumeFixing::quadratic_triangles(32, 0.5).expect("explicit bias"));
            data.measurements = [3_u32, 4, 5, 6]
                .into_iter()
                .enumerate()
                .map(|(row, length)| {
                    let vertices = length + 12;
                    let triangles = 2 * vertices;
                    let physical = f64::from(triangles) * LN_2;
                    let difference = f64::from(triangles) - 32.0;
                    let fixing = 0.5 * difference * difference;
                    Measurement::try_new(
                        u32::try_from(row + 1).expect("bounded row"),
                        physical + fixing,
                        vertices,
                        3 * vertices,
                        triangles,
                    )
                    .expect("known counts and action")
                    .try_with_action_components(physical, fixing)
                    .expect("separate terms")
                    .try_with_spatial_loop_lengths(vec![length, 4, 4, 4])
                    .expect("ring lengths")
                    .try_with_slab_triangle_profile(vec![length + 4, 8, 8, length + 4])
                    .expect("slab counts")
                })
                .collect();
            ValidationChain::try_new(data).expect("known biased fixture")
        })
        .collect();
    let physical_mean = 33.0 * LN_2;
    for (observable, units, expected) in [
        (
            CdtDiagnosticObservable::CombinedAction,
            "action",
            physical_mean + 3.0,
        ),
        (
            CdtDiagnosticObservable::PhysicalAction,
            "action",
            physical_mean,
        ),
        (CdtDiagnosticObservable::FixingAction, "action", 3.0),
        (
            CdtDiagnosticObservable::MaximumLoopLength,
            "spacelike-links",
            4.75,
        ),
        (
            CdtDiagnosticObservable::LoopProduct { left: 0, right: 1 },
            "spacelike-links-squared",
            18.0,
        ),
        (
            CdtDiagnosticObservable::ProfileIndicator {
                profile: vec![3, 4, 4, 4],
            },
            "indicator",
            0.25,
        ),
        (
            CdtDiagnosticObservable::TargetVolumeIndicator { target: 30 },
            "indicator",
            0.25,
        ),
    ] {
        let report =
            analyze_cdt_chains(&chains, observable, &policy(), None).expect("observable analysis");
        assert_eq!(report.observable_units, units);
        for dependence in &report.dependence {
            assert_relative_eq!(
                dependence.binning.value().expect("completed blocks")[0].mean,
                expected,
                epsilon = 1e-12
            );
        }
    }
    for observable in [
        CdtDiagnosticObservable::LoopLength { slice: 4 },
        CdtDiagnosticObservable::LoopProduct { left: 0, right: 4 },
        CdtDiagnosticObservable::ProfileIndicator {
            profile: vec![3, 4, 4],
        },
        CdtDiagnosticObservable::ProfileIndicator {
            profile: vec![2, 4, 4, 4],
        },
        CdtDiagnosticObservable::TargetProfileIndicator {
            target: 32,
            profile: vec![3, 4, 4, 4],
        },
    ] {
        assert_matches!(
            analyze_cdt_chains(&chains, observable, &policy(), None),
            Err(CdtError::Analysis(CdtAnalysisError::Observable))
        );
    }
}

#[test]
fn artifact_output_failures_keep_the_write_operation_and_error() {
    let chains = [
        synthetic_chain(1, &[4, 5, 4, 5]),
        synthetic_chain(2, &[5, 4, 5, 4]),
    ];
    let report = analyze_cdt_chains(&chains, CdtDiagnosticObservable::Triangles, &policy(), None)
        .expect("diagnostic report");
    for result in [
        chains[0].write_json(Unwritable),
        report.write_json(Unwritable),
    ] {
        assert_matches!(
            result,
            Err(CdtError::Analysis(CdtAnalysisError::Serialization {
                operation: AnalysisArtifactOperation::Write, detail,
            })) if !detail.is_empty()
        );
    }
}

#[test]
fn overflowing_second_moments_remain_unavailable_instead_of_passing_diagnostics() {
    let chains: Vec<_> = (1..=2)
        .map(|seed| {
            let mut transport =
                serde_json::to_value(synthetic_chain(seed, &[3, 4, 5, 6, 3, 4, 5, 6]).data())
                    .expect("raw chain");
            transport["configuration"]["cosmological_constant"] = json!(1e200);
            for measurement in transport["measurements"].as_array_mut().expect("rows") {
                let edges = measurement["edges"].as_u64().expect("small integer count");
                let action = edges.to_f64().expect("exact count conversion") * 1e200;
                measurement["action"] = json!(action);
                measurement["physical_action"] = json!(action);
            }
            ValidationChain::try_new(serde_json::from_value(transport).expect("finite transport"))
                .expect("finite actions remain valid even when their second moments overflow")
        })
        .collect();
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::PhysicalAction,
        &policy(),
        None,
    )
    .expect("estimator failure remains a report outcome");
    for dependence in &report.dependence {
        assert_matches!(&dependence.binning, DiagnosticEstimate::Unavailable { reason } if !reason.is_empty());
        assert_matches!(&dependence.selected_binning_level, DiagnosticEstimate::Unavailable { reason } if !reason.is_empty());
    }
    assert_eq!(report.verdict, CdtDiagnosticVerdict::Inconclusive);
    report
        .write_json(Vec::new())
        .expect("unavailable estimates remain serializable");
}

#[test]
fn measurement_builders_reject_invalid_ring_profiles_and_detached_bare_action() {
    let measurement = Measurement::try_new(7, 12.0, 16, 48, 32).expect("measurement");
    assert_matches!(
        measurement.clone().try_with_action_components(11.0, 0.0),
        Err(CdtError::CheckpointResumeFailed {
            failure: CheckpointResumeFailure::MeasurementActionStateMismatch { step: 7, .. }
        })
    );
    for profile in [vec![], vec![8, 8], vec![2, 5, 4, 5], vec![4, 4, 4, 5]] {
        assert_matches!(
            measurement.clone().try_with_spatial_loop_lengths(profile.clone()),
            Err(CdtError::SpatialLoop(SpatialLoopError::InvalidProfile { profile: actual, triangles: 32 }))
                if actual == profile
        );
        let mut encoded = serde_json::to_value(&measurement).expect("measurement transport");
        encoded["spatial_loop_lengths"] = json!(profile);
        serde_json::from_value::<Measurement>(encoded)
            .expect_err("deserialization must revalidate the ring profile");
    }
}

#[test]
fn combined_action_range_rejects_individually_finite_but_overflowing_terms() {
    let maximum_count = usize::MAX.to_f64().expect("representable count bound");
    let extent = maximum_count + 32.0;
    let physical = ActionConfig::new(0.0, 0.0, (0.75 * f64::MAX) / maximum_count)
        .expect("finite physical range");
    let fixing = VolumeFixing::quadratic_triangles(32, ((0.75 * f64::MAX) / extent) / extent)
        .expect("finite bias range");
    assert!(physical.calculate_action(0, usize::MAX, 0).is_finite());
    assert!(fixing.action(usize::MAX).is_finite());
    assert_matches!(
        physical.with_volume_fixing(Some(fixing)),
        Err(CdtError::VolumeFixing(
            VolumeFixingError::CombinedActionOverflow
        ))
    );
}

#[test]
fn volume_fixing_runner_and_checkpoint_restore_enforce_ensemble_boundaries() {
    let schedule = MetropolisConfig::new(1.0, 1, 0, 1)
        .expect("one-step schedule")
        .with_seed(7);
    let fixing = VolumeFixing::quadratic_triangles(32, 0.0).expect("zero-strength explicit bias");
    let action = ActionConfig::default()
        .with_volume_fixing(Some(fixing))
        .expect("finite action");
    let open = CdtTriangulation::from_cdt_strip(4, 4).expect("open fixture");
    assert_matches!(
        MetropolisAlgorithm::new(schedule.clone(), action)
            .run(open.clone())
            .map(drop),
        Err(CdtError::VolumeFixing(
            VolumeFixingError::UnsupportedTopology {
                topology: CdtTopology::OpenBoundary,
            }
        ))
    );
    let unreachable = ActionConfig::default()
        .with_volume_fixing(Some(
            VolumeFixing::quadratic_triangles(22, 0.0).expect("even target"),
        ))
        .expect("finite action");
    assert_matches!(
        MetropolisAlgorithm::new(schedule.clone(), unreachable)
            .run(CdtTriangulation::from_toroidal_cdt(4, 4).expect("torus"))
            .map(drop),
        Err(CdtError::VolumeFixing(
            VolumeFixingError::TargetBelowMinimum {
                target: 22,
                minimum: 24
            }
        ))
    );

    let bare = MetropolisAlgorithm::new(schedule, ActionConfig::default())
        .run_to_checkpoint(open)
        .expect("bare open chains remain supported");
    let mut encoded: Value =
        serde_json::from_str(&bare.to_json().expect("checkpoint")).expect("transport");
    encoded["format_version"] = json!(3);
    encoded["action_config"]["volume_fixing"] =
        serde_json::to_value(fixing).expect("bias transport");
    assert_matches!(
        CdtMcmcCheckpoint::from_json(&encoded.to_string()).map(drop),
        Err(CdtError::VolumeFixing(
            VolumeFixingError::UnsupportedTopology {
                topology: CdtTopology::OpenBoundary,
            }
        ))
    );
}
