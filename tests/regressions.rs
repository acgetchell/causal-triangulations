#![forbid(unsafe_code)]

//! Regression tests for previously observed CDT failures.

use approx::assert_relative_eq;
use causal_triangulations::prelude::simulation::{
    CdtMoveFamilyDistribution, CdtProposal, DelayedProposal, MoveType,
};
use causal_triangulations::{
    ActionConfig, CdtTriangulation, CdtTriangulation2D, ErgodicsSystem, MetropolisAlgorithm,
    MetropolisConfig, MonteCarloStep, MoveResult, SimulationEvent, TriangulationQuery,
};
use rand::{SeedableRng, rngs::StdRng};
use std::assert_matches;

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
    // endpoints of #316. This is counterexample evidence, NOT a claim that the
    // sampler is corrected: its family/site ratio currently misses this absent
    // reverse transition. The combinatorial correction must retain this fixture
    // and separately prove its state/measure and actual transition probabilities.
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

    // Commit the draft directly to examine the geometric endpoint, independently
    // of whether a corrected Metropolis rule accepts this represented transition.
    let mut after = before.clone();
    proposal
        .commit(&mut after, plan, &mut rng)
        .expect("original removal should realize successfully");
    after.validate().expect("valid destination endpoint");
    assert_eq!(after.slice_sizes(), &[4, 4, 3, 4]);
    let before_coordinates = coordinate_bits(&before);
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
            for vertex in after.geometry().face_vertices(&face).expect("live face") {
                let coordinates = after
                    .geometry()
                    .vertex_coordinates(&vertex)
                    .expect("live face vertex");
                sum[0] += coordinates[0];
                sum[1] += coordinates[1];
            }
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
