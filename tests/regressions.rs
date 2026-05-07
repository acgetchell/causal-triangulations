#![forbid(unsafe_code)]

//! Regression tests for previously observed CDT failures.

use approx::{assert_relative_eq, relative_eq};
use causal_triangulations::{
    ActionConfig, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    estimate_hausdorff_dimension, estimate_spectral_dimension,
};

/// Asserts that optional floating-point observables remain unchanged while
/// still using tolerant comparisons for concrete numeric estimates.
fn assert_observable_unchanged(name: &str, before: Option<f64>, after: Option<f64>, message: &str) {
    match (before, after) {
        (Some(before), Some(after)) => {
            assert!(
                relative_eq!(before, after, epsilon = f64::EPSILON),
                "{name}: {message}; before={before:?}, after={after:?}"
            );
        }
        (None, None) => {}
        (before, after) => panic!("{name}: {message}; before={before:?}, after={after:?}"),
    }
}

#[test]
fn toroidal_observables_remain_static_until_periodic_move_support_lands() {
    // Regression for causal-triangulations#122, which is blocked on
    // delaunay#337: periodic toroidal candidate moves are currently rejected
    // by the backend before they can mutate geometry. The observables example
    // exposed this because initial and final Hausdorff/spectral estimates were
    // identical. Once periodic mutation support lands, this test should flip
    // to expect accepted moves and changed final observables.
    let triangulation =
        CdtTriangulation::from_toroidal_cdt(8, 8).expect("observables fixture should build");
    let initial_counts = (
        triangulation.vertex_count(),
        triangulation.edge_count(),
        triangulation.face_count(),
    );
    let initial_profile = triangulation.volume_profile();
    let initial_hausdorff = estimate_hausdorff_dimension(&triangulation);
    let initial_spectral = estimate_spectral_dimension(&triangulation);

    let metropolis_config = MetropolisConfig::new(1.0, 80, 20, 10).with_seed(7);
    let results =
        MetropolisAlgorithm::new(metropolis_config, ActionConfig::default()).run(triangulation);
    let results = results.expect("toroidal observables regression run should complete");

    assert_eq!(results.move_stats.total_attempted(), 80);
    assert_eq!(
        results.move_stats.total_accepted(),
        0,
        "delaunay#337 / causal-triangulations#122 currently block successful periodic toroidal moves"
    );
    assert_relative_eq!(results.acceptance_rate(), 0.0, epsilon = f64::EPSILON);
    assert_eq!(
        (
            results.triangulation.vertex_count(),
            results.triangulation.edge_count(),
            results.triangulation.face_count(),
        ),
        initial_counts,
        "toroidal geometry should stay unchanged until periodic move support lands"
    );
    assert_eq!(results.triangulation.volume_profile(), initial_profile);
    assert_observable_unchanged(
        "Hausdorff dimension",
        initial_hausdorff,
        results.hausdorff_dimension_estimate(),
        "observables example should expose unchanged toroidal estimate while blocked",
    );
    assert_observable_unchanged(
        "spectral dimension",
        initial_spectral,
        results.spectral_dimension_estimate(),
        "observables example should expose unchanged toroidal estimate while blocked",
    );
    for (slice, fluctuation) in results.volume_fluctuations().into_iter().enumerate() {
        assert!(
            relative_eq!(fluctuation, 0.0, epsilon = f64::EPSILON),
            "volume fluctuation for slice {slice} should stay zero while periodic moves are blocked; got {fluctuation}"
        );
    }
}
