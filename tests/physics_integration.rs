#![forbid(unsafe_code)]

//! End-to-end physics integration tests for 1+1 CDT simulations.

use causal_triangulations::{
    CdtConfig, CdtTopology, TestConfig, ValidatedCdtConfig, run_simulation,
};

/// Enables real simulation mode on canned test configs with a deterministic seed.
const fn simulated_config(mut config: CdtConfig, seed: u64) -> CdtConfig {
    config.simulate = true;
    config.seed = Some(seed);
    config
}

/// Verifies that an end-to-end run mutates geometry and preserves final invariants.
fn assert_physics_pipeline(config: CdtConfig) {
    let config = config
        .into_validated()
        .expect("physics integration config should validate");
    let results = run_simulation(&config).expect("physics integration run should succeed");

    let acceptance_rate = results.acceptance_rate();
    assert!(
        acceptance_rate > 0.05,
        "acceptance rate too low: {acceptance_rate}"
    );
    assert!(
        acceptance_rate < 0.99,
        "acceptance rate suspiciously high: {acceptance_rate}"
    );

    let first_action = results
        .measurements()
        .first()
        .expect("simulation should record measurements")
        .action;
    assert!(
        results
            .measurements()
            .iter()
            .any(|measurement| (measurement.action - first_action).abs() > 1e-6),
        "action never changed"
    );

    results
        .triangulation()
        .validate()
        .expect("triangulation invalid after simulation");

    let profile = results.average_volume_profile();
    assert_eq!(
        profile.len(),
        usize::try_from(config.timeslices()).expect("timeslices should fit usize"),
        "volume profile should cover every time slice"
    );
    assert!(
        profile
            .iter()
            .take(occupied_time_slabs(&config))
            .all(|&volume| volume > 0.0),
        "empty time slice detected: {profile:?}"
    );

    let stats = results.move_stats();
    assert_eq!(
        stats.total_attempted(),
        u64::from(config.to_metropolis_config().steps().get()),
        "one move proposal should be recorded for each configured simulation step"
    );
    assert!(stats.total_acceptance_rate() > 0.0);
}

/// Counts slabs expected to have volume in the measured CDT profile.
fn occupied_time_slabs(config: &ValidatedCdtConfig) -> usize {
    let slabs = match config.topology() {
        CdtTopology::OpenBoundary => config.timeslices().saturating_sub(1),
        CdtTopology::Toroidal => config.timeslices(),
    };
    usize::try_from(slabs).expect("time slab count should fit usize")
}

#[test]
fn small_cdt_simulation_has_nontrivial_physics_signal() {
    let config = simulated_config(TestConfig::small(), 42);

    assert_physics_pipeline(config);
}

#[cfg(feature = "slow-tests")]
#[test]
fn medium_cdt_simulation_has_nontrivial_physics_signal() {
    let config = simulated_config(TestConfig::medium(), 42);

    assert_physics_pipeline(config);
}
