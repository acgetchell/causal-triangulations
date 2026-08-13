#![forbid(unsafe_code)]
//! Property-based tests for CDT Metropolis integration.

use approx::relative_eq;
use causal_triangulations::prelude::action::ActionConfig;
use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveResult};
use causal_triangulations::prelude::simulation::{CdtTarget, Target};
use causal_triangulations::prelude::triangulation::{CdtTriangulation, CdtTriangulation2D};
use proptest::prelude::*;

#[path = "common/proptest_config.rs"]
mod proptest_config;

use proptest_config::with_default_cases;

/// Shared triangulation created once (fixed seed, cheap).
fn test_triangulation() -> CdtTriangulation2D {
    CdtTriangulation::from_cdt_strip(4, 3).expect("regular open-boundary strip")
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
        let action_config = ActionConfig::new(coupling_0, coupling_2, cosmological_constant)
            .expect("generated couplings are finite");

        let target = CdtTarget::new(action_config.clone(), temperature)
            .expect("generated action config and temperature are valid");

        let log_prob = target.log_prob(&tri);

        // Must be finite
        prop_assert!(
            log_prob.is_finite(),
            "log_prob should be finite, got {} (κ₀={}, κ₂={}, λ={}, T={})",
            log_prob, coupling_0, coupling_2, cosmological_constant, temperature,
        );

        // Must equal -action / T
        let v = tri.vertex_count();
        let e = tri.edge_count();
        let f = tri.face_count();
        let action = action_config.calculate_action(v, e, f);
        let expected = -action / temperature;

        prop_assert!(
            relative_eq!(log_prob, expected, epsilon = 1e-12),
            "log_prob {:.15} != -action/T {:.15}",
            log_prob, expected,
        );
    }
}

proptest! {
    #![proptest_config(with_default_cases(8))]

    /// Random local-move sequences must preserve the full evolved CDT invariant
    /// contract for representative open-boundary and toroidal fixtures.
    #[test]
    fn random_move_sequences_preserve_cdt_invariants(
        seed in 0_u64..10_000,
        attempts in 1_usize..16,
        toroidal in any::<bool>(),
    ) {
        let mut triangulation = if toroidal {
            CdtTriangulation::from_toroidal_cdt(4, 3)
                .expect("valid toroidal property-test fixture should build")
        } else {
            CdtTriangulation::from_cdt_strip(4, 3)
                .expect("valid strip property-test fixture should build")
        };
        let mut moves = ErgodicsSystem::with_seed(seed);

        for attempt in 0..attempts {
            let result = moves.attempt_random_move(&mut triangulation);
            prop_assert!(
                !matches!(result, MoveResult::HardFailure(_)),
                "attempt {attempt} produced a hard move failure: {result:?}",
            );
            triangulation
                .validate()
                .unwrap_or_else(|err| panic!("attempt {attempt} left invalid CDT state after {result:?}: {err}"));
        }
    }
}
