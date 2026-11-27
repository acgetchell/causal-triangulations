#![forbid(unsafe_code)]

//! Regression tests for previously observed CDT failures.

use approx::assert_relative_eq;
use causal_triangulations::prelude::geometry::{DelaunayBackend2D, build_delaunay2_with_data};
use causal_triangulations::prelude::simulation::{
    CdtMoveFamilyDistribution, CdtProposal, DelayedProposal, MoveType,
};
use causal_triangulations::{
    ActionConfig, CdtError, CdtMcmcCheckpoint, CdtMoveFamilyPolicyError, CdtResult, CdtTarget,
    CdtTriangulation, CdtTriangulation2D, CheckpointResumeFailure, ConfigurationSetting,
    ErgodicsSystem, FoliationError, Measurement, MetropolisAlgorithm, MetropolisConfig,
    MonteCarloStep, MoveResult, SimulationEvent, SpatialLoopError, TriangulationQuery,
    VolumeFixing, average_dual_return_probability_curve,
};
use la_stack::Interval;
use rand::{SeedableRng, rngs::StdRng};
use serde_json::{Value, json};
use std::assert_matches;
use std::num::{NonZeroU32, NonZeroUsize};

/// Captures coordinate bits without depending on backend handle identities.
fn coordinate_bits(triangulation: &CdtTriangulation2D) -> Vec<[u64; 2]> {
    triangulation
        .geometry()
        .vertices()
        .map(|vertex| {
            let coordinates = triangulation
                .geometry()
                .vertex_coordinates(&vertex)
                .expect("fixture vertex coordinates should resolve");
            [coordinates[0].to_bits(), coordinates[1].to_bits()]
        })
        .collect()
}

#[test]
fn issue_316_original_removal_has_no_periodic_barycentric_reverse() {
    // Preserve the original constructor, seeds, deleted coordinate and valid
    // endpoints of #316, including a nonempty reverse-family site set.
    let before = CdtTriangulation2D::from_toroidal_cdt(4, 4).expect("original 4x4 torus");
    before.validate().expect("valid source endpoint");
    assert_eq!(before.slice_sizes(), &[4, 4, 4, 4]);
    let mut proposal = CdtProposal::new(ActionConfig::default())
        .with_seed(1)
        .with_policy(
            CdtMoveFamilyDistribution::from_weights([0.0, 1.0, 1.0, 0.0])
                .expect("original reversible volume-family weights"),
        );
    let mut rng = StdRng::seed_from_u64(17);
    let plan = proposal
        .propose_plan(&before, &mut rng)
        .expect("original removal should plan")
        .expect("original removal has a valid destination");
    assert_eq!(plan.move_type(), MoveType::Move31Remove);
    assert_eq!(plan.forward_site_count(), 2);
    assert_eq!(plan.reverse_site_count(), 15);
    let (forward, reverse) = plan.transition_probabilities();
    assert!(forward > 0.0);
    assert_eq!(reverse, 0.0);
    assert!(
        plan.log_proposal_ratio().is_infinite() && plan.log_proposal_ratio().is_sign_negative()
    );

    // Commit the draft directly to examine the geometric endpoint, independently
    // of whether a corrected Metropolis rule accepts this represented transition.
    let before_coordinates = coordinate_bits(&before);
    let mut after = before;
    proposal
        .commit(&mut after, plan, &mut rng)
        .expect("original removal should realize successfully");
    after.validate().expect("valid destination endpoint");
    assert_eq!(after.slice_sizes(), &[4, 4, 3, 4]);
    let after_coordinates = coordinate_bits(&after);
    assert_eq!(before_coordinates.len(), after_coordinates.len() + 1);
    assert!(
        after_coordinates
            .iter()
            .all(|c| before_coordinates.contains(c))
    );
    let removed = before_coordinates
        .iter()
        .filter(|c| !after_coordinates.contains(c))
        .collect::<Vec<_>>();
    assert_eq!(removed.len(), 1);
    let expected = [3.005_476_804_546_397_3_f64, 1.970_856_741_530_444_5_f64];
    assert_eq!(*removed[0], expected.map(f64::to_bits));

    // Independent necessary condition for ANY periodic lift of ANY destination
    // face: 3*c[d] - sum(vertices[d]) = period[d]*k[d], k[d] an integer.
    // Checking every face is stronger than inspecting only offered insertions;
    // this calculation does not use the production barycenter or site visitor.
    let minimum_residue = after
        .geometry()
        .faces()
        .map(|face| {
            let mut sum = [0.0; 2];
            let mut bounds = [Interval::ZERO; 2];
            for vertex in after.geometry().face_vertices(&face).expect("live face") {
                let coordinates = after
                    .geometry()
                    .vertex_coordinates(&vertex)
                    .expect("live face vertex");
                sum[0] += coordinates[0];
                sum[1] += coordinates[1];
                for (bound, &coordinate) in bounds.iter_mut().zip(coordinates) {
                    *bound = bound
                        .try_add(&Interval::point(coordinate).unwrap())
                        .unwrap();
                }
            }
            // A reverse would require an integer lift on both axes. Even the
            // outward-rounded quotient excludes every integer on at least one.
            assert!((0..2).any(|axis| {
                let q = Interval::point(expected[axis])
                    .unwrap()
                    .try_mul(&Interval::point(3.0).unwrap())
                    .unwrap()
                    .try_add(&bounds[axis].negate())
                    .unwrap()
                    .try_div(&Interval::point(4.0).unwrap())
                    .unwrap();
                q.lower().ceil() > q.upper().floor()
            }));
            (0..2)
                .map(|d| {
                    #[expect(
                        clippy::suboptimal_flops,
                        reason = "preserve the original issue #316 probe's IEEE operation order"
                    )]
                    let quotient = (3.0 * expected[d] - sum[d]) / 4.0;
                    (quotient - quotient.round()).abs()
                })
                .fold(0.0_f64, f64::max)
        })
        .fold(f64::INFINITY, f64::min);
    assert_relative_eq!(minimum_residue, 0.013_167_135_089_362_29, epsilon = 1e-14);
    assert!(
        minimum_residue > 1e-10,
        "rounding cannot restore reverse support"
    );
}

#[test]
fn toroidal_observables_run_accepts_periodic_moves_after_offset_support() {
    // Regression for causal-triangulations#122 after delaunay#337: periodic
    // toroidal candidate moves should no longer be blocked by backend offset
    // handling, and the observables workflow should report accepted moves while
    // preserving the toroidal CDT invariants.
    let triangulation =
        CdtTriangulation::from_toroidal_cdt(4, 3).expect("observables fixture should build");

    let metropolis_config = MetropolisConfig::new(1.0, 20, 0, 5)
        .expect("regression Metropolis config should be valid")
        .with_seed(7);
    let results =
        MetropolisAlgorithm::new(metropolis_config, ActionConfig::default()).run(triangulation);
    let results = results.expect("toroidal observables regression run should complete");

    assert_eq!(results.move_stats().total_attempted(), 20);
    assert!(
        results.move_stats().total_accepted() > 0,
        "periodic toroidal runs should accept moves after delaunay offset-aware flips"
    );
    assert!(
        results.steps().iter().any(MonteCarloStep::accepted),
        "observables workflow should expose at least one accepted periodic move"
    );
    assert!(
        results.acceptance_rate() > 0.0,
        "accepted periodic moves should produce a nonzero acceptance rate"
    );
    results
        .triangulation()
        .validate()
        .expect("accepted periodic moves should preserve evolved toroidal CDT invariants");
    assert!(
        results
            .all_scale_effective_hausdorff_slope()
            .expect("final triangulation adjacency should be readable")
            .is_some(),
        "observables workflow should still report a Hausdorff estimate"
    );
    assert!(
        results
            .short_time_effective_spectral_dimension()
            .expect("final triangulation adjacency should be readable")
            .is_some(),
        "observables workflow should still report a spectral estimate"
    );
}

#[test]
fn accepted_move_history_keeps_matching_attempt_after_planned_proposal_handoff() {
    // Regression for causal-triangulations#153: accepted planned-proposal steps
    // replace the live triangulation with the committed chain state. The final
    // state must retain the MoveAttempted event for each accepted move, not only
    // the MoveAccepted event recorded after the replacement.
    let triangulation =
        CdtTriangulation::from_toroidal_cdt(4, 3).expect("history fixture should build");
    let metropolis_config = MetropolisConfig::new(1.0, 20, 0, 5)
        .expect("history regression Metropolis config should be valid")
        .with_seed(7);

    let results =
        MetropolisAlgorithm::new(metropolis_config, ActionConfig::default()).run(triangulation);
    let results = results.expect("history regression run should complete");
    let history = results.simulation_history().collect::<Vec<_>>();

    let mut accepted_events = 0_u64;
    for event in &history {
        let SimulationEvent::MoveAccepted {
            move_type, step, ..
        } = event
        else {
            continue;
        };
        accepted_events = accepted_events.saturating_add(1);
        assert!(
            history.iter().any(|candidate| matches!(
                candidate,
                SimulationEvent::MoveAttempted {
                    move_type: attempted_move,
                    step: attempted_step,
                } if attempted_move == move_type && attempted_step == step
            )),
            "accepted move {move_type:?} at step {step} should retain its matching attempt event"
        );
    }

    assert!(
        accepted_events > 0,
        "deterministic history regression should accept at least one move"
    );
}

#[test]
fn accepted_step_telemetry_keeps_action_delta_consistent_after_planned_proposal_handoff() {
    // Regression for causal-triangulations#153: accepted public telemetry must
    // keep reconstructed action_after and delta_action in sync after the
    // planned-proposal sampler hands the committed state back to CDT.
    let triangulation =
        CdtTriangulation::from_toroidal_cdt(4, 3).expect("telemetry fixture should build");
    let metropolis_config = MetropolisConfig::new(1.0, 20, 0, 5)
        .expect("telemetry regression Metropolis config should be valid")
        .with_seed(7);

    let results =
        MetropolisAlgorithm::new(metropolis_config, ActionConfig::default()).run(triangulation);
    let results = results.expect("telemetry regression run should complete");

    let mut accepted_steps = 0_u64;
    for step in results.steps() {
        if step.accepted() {
            accepted_steps = accepted_steps.saturating_add(1);
            let action_after = step
                .action_after()
                .expect("accepted steps should expose action_after");
            let delta_action = step
                .delta_action()
                .expect("accepted steps should expose delta_action");
            assert_relative_eq!(
                delta_action,
                action_after - step.action_before(),
                epsilon = 1e-12
            );
        } else {
            assert!(
                step.action_after().is_none(),
                "rejected steps should not expose action_after"
            );
        }
    }

    assert!(
        accepted_steps > 0,
        "deterministic telemetry regression should accept at least one move"
    );
}

#[test]
fn proposal_site_cache_does_not_reuse_sites_for_replaced_triangulation_instances() {
    // Regression for causal-triangulations#148: proposal-site cache identity
    // must distinguish fresh triangulation instances, even when a new state
    // occupies the same local variable and has the same public modification
    // count as the cached state.
    let mut system = ErgodicsSystem::with_seed(11);
    let mut triangulation =
        CdtTriangulation::from_toroidal_cdt(4, 3).expect("toroidal fixture should build");
    assert_eq!(triangulation.metadata().modification_count(), 0);

    let first_result = system.attempt_13_move(&mut triangulation);
    assert_matches!(
        first_result,
        MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation,
        "initial toroidal move should not hit a hard proposal failure"
    );
    triangulation
        .validate()
        .expect("initial cached triangulation should remain valid");

    triangulation = CdtTriangulation::from_cdt_strip(4, 3).expect("strip fixture should build");
    assert_eq!(triangulation.metadata().modification_count(), 0);

    let second_result = system.attempt_13_move(&mut triangulation);
    assert_matches!(
        second_result,
        MoveResult::Success | MoveResult::GeometricViolation,
        "fresh strip state should rebuild face-subdivision sites instead of surfacing stale toroidal-site hard failures"
    );
    triangulation
        .validate()
        .expect("fresh strip move should preserve CDT invariants");
}

#[test]
fn open_boundary_visualization_seed_preserves_spatial_interval_slices() {
    // Regression for causal-triangulations#191: the README visualization seed
    // exposed accepted open-boundary proposals whose spacelike subgraph branched
    // within a single time slice. Such candidates should be rejected and rolled
    // back, leaving every spatial slice as one interval. The same seed also
    // evolves beyond its initialization drawing, so combinatorially valid paths
    // must not be rejected merely because stale backend x-order no longer
    // matches the abstract spatial interval.
    let triangulation =
        CdtTriangulation::from_cdt_strip(24, 7).expect("visualization fixture should build");
    let metropolis_config = MetropolisConfig::new(1.0, 160, 0, 1)
        .expect("visualization Metropolis config should be valid")
        .with_seed(20_260_612);
    let action_config =
        ActionConfig::new(0.0, 0.0, 0.9).expect("visualization action config should be valid");

    let results = MetropolisAlgorithm::new(metropolis_config, action_config)
        .run(triangulation)
        .expect("visualization regression run should complete");

    assert_eq!(
        results.move_stats().total_hard_failures(),
        0,
        "invalid open-interval candidates should be rejected, not surfaced as hard failures"
    );
    results
        .triangulation()
        .validate()
        .expect("final visualization triangulation should preserve open-boundary CDT invariants");
}

#[test]
fn resumed_scalar_trace_keeps_checkpoint_seed_not_fresh_resume_seed() {
    // Regression for causal-triangulations#164: checkpoint continuation ignores
    // the fresh algorithm seed and resumes from serialized RNG state. Scalar
    // trace metadata must therefore keep the checkpoint/run seed, not the seed
    // on the temporary resume driver.
    let action_config = ActionConfig::default();
    let prefix_config = MetropolisConfig::new(1.0, 4, 0, 1)
        .expect("prefix Metropolis config should be valid")
        .with_seed(19);
    let resume_config = MetropolisConfig::new(1.0, 6, 0, 1)
        .expect("resume Metropolis config should be valid")
        .with_seed(999);
    let prefix = MetropolisAlgorithm::new(prefix_config, action_config.clone())
        .run_to_checkpoint(
            CdtTriangulation::from_cdt_strip(4, 3).expect("strip fixture should build"),
        )
        .expect("prefix checkpoint should build");

    let checkpoint = MetropolisAlgorithm::new(resume_config, action_config)
        .resume_to_checkpoint(prefix)
        .expect("resume with a different fresh seed should still validate");
    let results = checkpoint
        .into_results()
        .expect("checkpoint results should validate");
    let trace = results.scalar_trace().expect("scalar trace should export");
    let seed_low_index = trace
        .observable_names()
        .iter()
        .position(|name| name == "seed_low_u32")
        .expect("trace should include seed_low_u32");
    let seed_high_index = trace
        .observable_names()
        .iter()
        .position(|name| name == "seed_high_u32")
        .expect("trace should include seed_high_u32");
    let seed_present_index = trace
        .observable_names()
        .iter()
        .position(|name| name == "seed_present")
        .expect("trace should include seed_present");

    assert_eq!(trace.len(), 10);
    for record in trace.records() {
        let values = record.observable_values();
        assert_relative_eq!(values[seed_low_index], 19.0);
        assert_relative_eq!(values[seed_high_index], 0.0);
        assert_relative_eq!(values[seed_present_index], 1.0);
    }
}

#[test]
fn coordinate_bands_preserve_a_vertex_just_below_the_third_boundary() -> CdtResult<()> {
    // Multiplication by 2^70 preserves all dyadic ratios. At the small scale
    // the original epsilon check collapsed every vertex into the first band.
    for unit in [1e-20, 1e-20 * 2.0_f64.powi(70)] {
        let points = [
            ([0.0, 0.0], 0),
            ([unit, unit], 0),
            ([-unit, 2.0 * unit], 0),
            ([unit, 3.0 * unit], 0),
            ([0.0, 4.0 * unit], 0),
        ];
        let backend = DelaunayBackend2D::from_triangulation(build_delaunay2_with_data(&points)?)
            .expect("five distinct planar vertices");
        let mut mesh = CdtTriangulation::try_new(backend, 4, 2)?;
        mesh.assign_foliation_by_y(NonZeroU32::new(4).unwrap())?;
        // The exact index of 3*unit is 3 - 1/6646139978924579.
        assert_eq!(mesh.slice_sizes(), &[1, 1, 2, 1]);
        assert_matches!(
            mesh.assign_foliation_by_y(NonZeroU32::MAX),
            Err(CdtError::Foliation(FoliationError::EmptySlice { slice: 1 }))
        );
        assert_eq!(mesh.slice_sizes(), &[1, 1, 2, 1]);
    }
    Ok(())
}

#[cfg(target_pointer_width = "64")]
#[test]
fn finite_action_range_preserves_the_last_safe_coupling() {
    let large = f64::MAX / 2.0_f64.powi(64);
    let small = (large - large.next_down()) * 0.375;
    // Each small addend disappears separately. Together they overflow at usize::MAX.
    assert_eq!((large + small + small).to_bits(), large.to_bits());
    assert_matches!(
        ActionConfig::new(-large, -small, small),
        Err(CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::ActionCouplings,
            ..
        })
    );
    // The bias stays below 2^128. Even at MAX, adding it leaves the rounded
    // action finite, although an interval bounding the real sum needs +infinity.
    let safe = ActionConfig::new(-large, 0.0, 0.0)
        .unwrap()
        .with_volume_fixing(Some(VolumeFixing::quadratic_triangles(32, 1.0).unwrap()))
        .unwrap();
    assert_eq!(
        safe.calculate_action(usize::MAX, 0, usize::MAX).to_bits(),
        f64::MAX.to_bits()
    );
    assert!(CdtTarget::new(safe, 1.0).is_ok());
    let near_limit = ActionConfig::new(-large, -large * 2.0_f64.powi(-65), 0.0).unwrap();
    assert_eq!(
        near_limit
            .physical_action(usize::MAX, 0, usize::MAX)
            .to_bits(),
        f64::MAX.to_bits()
    );
}

#[test]
fn proposal_weights_allocate_the_last_atom_by_exact_remainder() {
    let policy = CdtMoveFamilyDistribution::from_weights([1.0, 1.0, 1.0, 7.0]).unwrap();
    // 2^53 mod 10 = 2. The fourth family has remainder 4/10 and the others 2/10.
    // Every draw mass, including the full grid, is exactly representable in f64.
    let grid = 2.0_f64.powi(53);
    let base = 900_719_925_474_099.0;
    assert_eq!(
        policy.probabilities().map(|p| (p * grid).to_bits()),
        [base, base, base, 6_305_039_478_318_695.0_f64].map(f64::to_bits)
    );
}

#[test]
fn proposal_weight_total_overflow_is_independent_of_family_order() {
    // The exact total is MAX + 3/4 ulp, which rounds to infinity. Adding each
    // small weight directly to MAX used to discard both increments.
    let small = (f64::MAX - f64::MAX.next_down()) * 0.375;
    for weights in [[f64::MAX, small, small, 0.0], [small, small, f64::MAX, 0.0]] {
        assert_matches!(CdtMoveFamilyDistribution::from_weights(weights),
            Err(CdtMoveFamilyPolicyError::NonFiniteTotalWeight { total_weight }) if total_weight.is_infinite());
    }
    // One increment rounds back to MAX. Both positive families must keep support.
    let near = CdtMoveFamilyDistribution::from_weights([f64::MAX, small, 0.0, 0.0]).unwrap();
    assert!(near.probabilities()[1] > 0.0);
}

#[test]
fn measurement_action_tolerance_rejects_a_subnormal_beyond_the_limit() -> CdtResult<()> {
    let limit = 8.0 * f64::EPSILON;
    let measurement = Measurement::try_new(3, limit, 3, 3, 1)?;
    measurement.clone().try_with_action_components(0.0, 0.0)?;
    // The exact difference exceeds the inclusive tolerance by the smallest
    // subnormal. Floating subtraction alone rounds that difference back to the limit.
    let physical = -f64::from_bits(1);
    assert_matches!(measurement.try_with_action_components(physical, 0.0),
        Err(CdtError::CheckpointResumeFailed { failure: CheckpointResumeFailure::MeasurementActionStateMismatch {
            step: 3, actual, expected }}) if actual.to_bits() == limit.to_bits() && expected.to_bits() == physical.to_bits());
    Ok(())
}

#[cfg(target_pointer_width = "64")]
#[test]
fn quadratic_volume_bias_subtracts_the_integer_target_before_rounding() {
    let fixing = VolumeFixing::quadratic_triangles(2, 1.0).unwrap();
    // (2^53 + 1) - 2 = 2^53 - 1 is exactly representable. Converting the
    // triangle count first used to lose one triangle before the subtraction.
    let triangles = (1_usize << 53) + 1;
    let expected = 2.0_f64.powi(106) - 2.0_f64.powi(54);
    assert_eq!(fixing.action(triangles).to_bits(), expected.to_bits());
    // (2^53 + 1)^2 also retains its middle term even though the displacement
    // itself no longer fits exactly in f64.
    let expected = 2.0_f64.powi(106) + 2.0_f64.powi(54);
    assert_eq!(fixing.action(triangles + 2).to_bits(), expected.to_bits());
}

#[test]
fn quadratic_volume_bias_rounds_the_integer_square_once() {
    let fixing = VolumeFixing::quadratic_triangles(32, 0.1).unwrap();
    // The exact coefficient is the binary64 value of 0.1. Multiplying it by
    // 36 rounds to 3.6; two successive multiplies by six round upward.
    assert_eq!(fixing.action(38).to_bits(), 3.6_f64.to_bits());
    assert_eq!(fixing.action(26).to_bits(), 3.6_f64.to_bits());
}

#[cfg(target_pointer_width = "64")]
#[test]
fn target_rejects_overflow_from_scaling_action_components_separately() -> CdtResult<()> {
    // At the count limit, dividing the sum rounds to MAX. Dividing both
    // components first makes the additive target overflow on their final addition.
    let coupling = f64::from_bits(0x43ec_36d9_61b1_9219);
    let strength = f64::from_bits(0x3fed_ec1a_5e4e_6de5);
    let temperature = f64::from_bits(0x07fd_1179_e000_0000);
    let action = ActionConfig::new(-coupling, 0.0, 0.0)?
        .with_volume_fixing(Some(VolumeFixing::quadratic_triangles(2, strength)?))?;
    let physical = action.physical_action(usize::MAX, 0, usize::MAX);
    let bias = action.fixing_action(usize::MAX);
    assert_eq!(
        ((physical + bias) / temperature).to_bits(),
        f64::MAX.to_bits()
    );
    assert!((physical / temperature + bias / temperature).is_infinite());
    assert!(CdtTarget::new(action.clone(), temperature.next_up()).is_ok());
    assert_matches!(
        CdtTarget::new(action, temperature).map(drop),
        Err(CdtError::InvalidSimulationConfiguration {
            setting: ConfigurationSetting::Temperature,
            ..
        })
    );
    Ok(())
}

#[test]
fn one_face_walk_stays_at_its_root_at_every_diffusion_time() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_seeded_points(3, 1, 2, 0x000B_5E2A)?;
    assert_eq!(triangulation.face_count(), 1);
    let curve =
        average_dual_return_probability_curve(&triangulation, NonZeroUsize::new(4).unwrap())?;
    assert_eq!(
        curve.iter().map(|p| p.to_bits()).collect::<Vec<_>>(),
        [1.0_f64.to_bits(); 5]
    );
    Ok(())
}

#[test]
fn checkpoint_rejects_an_altered_early_ring_even_when_every_slab_agrees() -> CdtResult<()> {
    let mesh = CdtTriangulation2D::from_toroidal_cdt(4, 4)?;
    let checkpoint = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 32, 0, 1)?.with_seed(41),
        ActionConfig::new(0.0, 0.0, 0.0)?,
    )
    .run_to_checkpoint(mesh)?;
    assert!(checkpoint.steps().iter().any(|step| step.accepted()
        && matches!(
            step.move_type(),
            MoveType::Move13Add | MoveType::Move31Remove
        )));
    let mut stored: Value = serde_json::from_str(&checkpoint.to_json()?).unwrap();
    let first = &mut stored["measurements"][0];
    assert_eq!(first["spatial_loop_lengths"], json!([4, 4, 4, 4]));
    // Every adjacent pair still sums to eight, including the seam. Only
    // comparison with the reconstructed historical rings can distinguish the records.
    first["spatial_loop_lengths"] = json!([3, 5, 3, 5]);
    assert_matches!(CdtMcmcCheckpoint::from_json(&stored.to_string()).map(drop), Err(CdtError::SpatialLoop(
        SpatialLoopError::InvalidProfile { profile, triangles: 32 })) if profile == [3, 5, 3, 5]);
    Ok(())
}

#[test]
fn persisted_trajectory_rejects_misattributed_move_families() -> CdtResult<()> {
    use causal_triangulations::{CheckpointMoveCounter, SimulationResultsBackend};

    let sampler = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 32, 0, 1)?.with_seed(41),
        ActionConfig::new(0.0, 0.0, 0.0)?,
    );
    let checkpoint = sampler.run_to_checkpoint(CdtTriangulation2D::from_toroidal_cdt(4, 4)?)?;
    let checkpoint_json = serde_json::to_value(&checkpoint).unwrap();
    let results = checkpoint.into_results()?;
    let results_json = serde_json::to_value(&results).unwrap();
    // A non-accepted attempt can change family without changing the geometry,
    // action, total attempts, or any acceptance count.
    let rejected = results
        .steps()
        .iter()
        .position(|step| !step.accepted())
        .unwrap();
    let original = results.steps()[rejected].move_type();
    let replacement = MoveType::REVERSIBLE_1P1
        .into_iter()
        .find(|family| *family != original)
        .unwrap();
    let accepted = results
        .steps()
        .iter()
        .position(|step| step.accepted() && step.move_type() != original)
        .unwrap();
    for swap in [false, true] {
        let tamper = |mut stored: Value| {
            let rows = stored["scalar_trace_rows"].as_array_mut().unwrap();
            if swap {
                // Swapping the family labels also preserves every family's
                // attempt count. Only the per-family acceptance counts change.
                rows[rejected]["move_type"] = rows[accepted]["move_type"].clone();
                rows[accepted]["move_type"] = json!(original);
            } else {
                rows[rejected]["move_type"] = json!(replacement);
            }
            stored
        };
        let checkpoint_error =
            CdtMcmcCheckpoint::from_json(&tamper(checkpoint_json.clone()).to_string())
                .map(drop)
                .unwrap_err();
        let results_error =
            serde_json::from_value::<SimulationResultsBackend>(tamper(results_json.clone()))
                .unwrap_err();
        let expected_counter = if swap {
            CheckpointMoveCounter::Accepted
        } else {
            CheckpointMoveCounter::Attempted
        };
        assert_matches!(&checkpoint_error, CdtError::CheckpointResumeFailed {
            failure: CheckpointResumeFailure::ScalarTraceMoveCountMismatch { counter, actual, expected, .. }
        } if *counter == expected_counter && actual.abs_diff(*expected) == 1);
        assert_eq!(results_error.to_string(), checkpoint_error.to_string());
    }
    Ok(())
}

#[test]
fn constant_observations_preserve_their_exact_action_and_slab_means() -> CdtResult<()> {
    let sampler = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 3, 1, 1)?.with_seed(7),
        ActionConfig::new(-1.0, 0.0, 0.0)?,
    )
    .with_policy(CdtMoveFamilyDistribution::from_weights([
        1.0, 0.0, 0.0, 0.0,
    ])?);
    let results = sampler.run(CdtTriangulation::from_toroidal_cdt(5, 3)?)?;
    assert_eq!(results.measurements().len(), 3);
    for row in results.measurements() {
        assert_eq!(row.action().to_bits(), 15.0_f64.to_bits());
        assert_eq!(row.slab_triangle_profile(), &[10, 10, 10]);
    }
    assert_eq!(results.average_action().to_bits(), 15.0_f64.to_bits());
    assert_eq!(
        results
            .average_slab_triangle_profile()
            .iter()
            .map(|x| x.to_bits())
            .collect::<Vec<_>>(),
        [10.0_f64.to_bits(); 3]
    );
    assert_eq!(
        results
            .slab_triangle_fluctuations()
            .iter()
            .map(|x| x.to_bits())
            .collect::<Vec<_>>(),
        [0.0_f64.to_bits(); 3]
    );
    Ok(())
}

#[test]
fn binning_plateau_respects_the_exact_fractional_threshold() -> CdtResult<()> {
    use causal_triangulations::{
        CdtConfig, CdtDiagnosticObservable, CdtDiagnosticPolicy, CdtTopology,
        VALIDATION_CHAIN_SCHEMA_VERSION, ValidationChain, ValidationChainData, analyze_cdt_chains,
    };
    // Original measured profiles at steps 1 through 16, retaining every
    // repeated observation. Proposal changes must leave this numerical
    // regression's input intact.
    let seed_41 = [
        (5, [4, 4, 5, 4]),
        (1, [5, 4, 5, 4]),
        (5, [5, 4, 4, 4]),
        (5, [6, 4, 4, 4]),
    ];
    let seed_7 = [
        (3, [4, 4, 4, 4]),
        (3, [4, 4, 5, 4]),
        (2, [4, 4, 6, 4]),
        (8, [4, 4, 7, 4]),
    ];
    let mut chains = Vec::new();
    for (seed, runs) in [(41, seed_41), (7, seed_7)] {
        let config = CdtConfig {
            topology: CdtTopology::Toroidal,
            simulate: true,
            seed: Some(seed),
            steps: 16,
            thermalization_steps: 1,
            measurement_frequency: 1,
            ..CdtConfig::new(16, 4)
        };
        let action = config.clone().into_validated()?.to_action_config();
        let measurements = (1..)
            .zip(
                runs.into_iter()
                    .flat_map(|(count, profile)| std::iter::repeat_n(profile, count)),
            )
            .map(|(step, profile)| {
                let vertices: u32 = profile.iter().sum();
                let edges = 3 * vertices;
                let triangles = 2 * vertices;
                let physical =
                    action.physical_action(vertices as usize, edges as usize, triangles as usize);
                let slab = profile
                    .into_iter()
                    .zip(profile.into_iter().cycle().skip(1))
                    .map(|(left, right)| left + right)
                    .collect();
                Measurement::try_new(step, physical, vertices, edges, triangles)?
                    .try_with_slab_triangle_profile(slab)?
                    .try_with_spatial_loop_lengths(profile.to_vec())
            })
            .collect::<CdtResult<Vec<_>>>()?;
        chains.push(ValidationChain::try_new(ValidationChainData {
            schema_version: VALIDATION_CHAIN_SCHEMA_VERSION,
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
            spatial_topology: "S1".to_owned(),
            temporal_boundary: "periodic".to_owned(),
            minimum_loop_length: 3,
            action_convention: "-kappa_0*N0-kappa_2*N2+lambda_edge*N1".to_owned(),
            run_id: "binning-regression".to_owned(),
            chain_id: format!("chain-{seed}"),
            configuration: config,
            measurements,
        })?);
    }
    // For seed 41, the relative change between the two eligible error levels is
    // exactly 285490319773395 / 1411390226616019 for the reported binary64s.
    // These adjacent tolerances straddle it. The smaller one's rounded product
    // with the larger error used to equal the difference and certify a false plateau.
    let lower = 0.202_275_964_782_534_32_f64;
    for (tolerance, selected) in [(lower, None), (lower.next_up(), Some(&1))] {
        let policy = CdtDiagnosticPolicy {
            maximum_lag: 4,
            minimum_completed_blocks: 8,
            stabilization_levels: 2,
            binning_relative_tolerance: tolerance,
            require_stable_binning: true,
            minimum_chains: 2,
            minimum_ess: 1.0,
            maximum_mean_mcse: 10.0,
            quantiles: Vec::new(),
        };
        let report =
            analyze_cdt_chains(&chains, CdtDiagnosticObservable::Triangles, &policy, None)?;
        let chain = &report.dependence[0];
        assert_eq!(chain.chain_id, "chain-41");
        let levels = chain.binning.value().unwrap();
        assert_eq!(
            levels[0].blocked_mean_error.unwrap().to_bits(),
            0.25_f64.to_bits()
        );
        assert_eq!(
            levels[1].blocked_mean_error.unwrap().to_bits(),
            0.313_391_585_264_004_35_f64.to_bits()
        );
        assert_eq!(chain.selected_binning_level.value(), selected);
    }
    Ok(())
}
