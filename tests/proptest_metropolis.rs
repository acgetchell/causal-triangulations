//! Property-based tests for CDT Metropolis integration.

use causal_triangulations::cdt::action::ActionConfig;
use causal_triangulations::cdt::metropolis::CdtTarget;
use causal_triangulations::cdt::triangulation::CdtTriangulation;
use markov_chain_monte_carlo::Target;
use proptest::prelude::*;

/// Shared triangulation created once (fixed seed, cheap).
fn test_triangulation() -> causal_triangulations::geometry::CdtTriangulation2D {
    CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Fixed-seed triangulation")
}

proptest! {
    /// `CdtTarget::log_prob` must always equal `-action / temperature` and be
    /// finite for any valid coupling constants and positive temperature.
    #[test]
    fn log_prob_equals_negative_action_over_temperature(
        coupling_0 in -10.0f64..10.0,
        coupling_2 in -10.0f64..10.0,
        cosmological_constant in -5.0f64..5.0,
        temperature in 0.01f64..100.0,
    ) {
        let tri = test_triangulation();
        let action_config = ActionConfig::new(coupling_0, coupling_2, cosmological_constant);

        let target = CdtTarget::new(action_config.clone(), temperature);

        let log_prob = target.log_prob(&tri);

        // Must be finite
        prop_assert!(
            log_prob.is_finite(),
            "log_prob should be finite, got {} (κ₀={}, κ₂={}, λ={}, T={})",
            log_prob, coupling_0, coupling_2, cosmological_constant, temperature,
        );

        // Must equal -action / T
        let v = u32::try_from(tri.vertex_count()).unwrap();
        let e = u32::try_from(tri.edge_count()).unwrap();
        let f = u32::try_from(tri.face_count()).unwrap();
        let action = action_config.calculate_action(v, e, f);
        let expected = -action / temperature;

        prop_assert!(
            (log_prob - expected).abs() < 1e-12,
            "log_prob {:.15} != -action/T {:.15}",
            log_prob, expected,
        );
    }
}
