#![forbid(unsafe_code)]

//! Regression tests for previously observed CDT failures.

use causal_triangulations::{
    ActionConfig, CdtTriangulation, ErgodicsSystem, MetropolisAlgorithm, MetropolisConfig,
    MoveResult, SimulationEvent,
};
use std::assert_matches;

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
        results.steps().iter().any(|step| step.accepted),
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
        results.hausdorff_dimension_estimate().is_some(),
        "observables workflow should still report a Hausdorff estimate"
    );
    assert!(
        results.spectral_dimension_estimate().is_some(),
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
    let history = results.triangulation().metadata().simulation_history();

    let mut accepted_events = 0_u64;
    for event in history {
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
    assert_eq!(
        second_result,
        MoveResult::Success,
        "fresh strip state should rebuild its face-subdivision sites instead of reusing stale toroidal insertion sites"
    );
    triangulation
        .validate()
        .expect("fresh strip move should preserve CDT invariants");
}
