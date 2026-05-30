#![forbid(unsafe_code)]

//! Regression tests for previously observed CDT failures.

use causal_triangulations::{
    ActionConfig, CdtTriangulation, ErgodicsSystem, MetropolisAlgorithm, MetropolisConfig,
    MoveResult,
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

    let metropolis_config = MetropolisConfig::new(1.0, 20, 0, 5).with_seed(7);
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
fn proposal_site_cache_does_not_reuse_sites_for_replaced_triangulation_instances() {
    // Regression for causal-triangulations#148: proposal-site cache identity
    // must distinguish fresh triangulation instances, even when a new state
    // occupies the same local variable and has the same public modification
    // count as the cached state.
    let mut system = ErgodicsSystem::with_seed(11);
    let mut triangulation =
        CdtTriangulation::from_toroidal_cdt(4, 3).expect("toroidal fixture should build");
    assert_eq!(triangulation.metadata().modification_count, 0);

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
    assert_eq!(triangulation.metadata().modification_count, 0);

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
