//! Comprehensive integration tests for CDT-RS.
//!
//! This module contains integration tests that verify the complete CDT simulation
//! workflows, topology preservation, error handling, and consistency between components.

use causal_triangulations::cdt::action::ActionConfig;
use causal_triangulations::cdt::metropolis::{MetropolisAlgorithm, MetropolisConfig};
use causal_triangulations::cdt::triangulation::CdtTriangulation;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_complete_cdt_simulation_workflow() {
        // Test full CDT simulation pipeline
        let triangulation = CdtTriangulation::from_random_points(8, 2, 2)
            .expect("Failed to create initial triangulation");

        let config = MetropolisConfig::new(1.0, 50, 10, 5);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        // Run simulation
        let results = algorithm
            .run(triangulation)
            .expect("Simulation should succeed");

        // Verify results
        assert!(!results.steps.is_empty(), "Simulation should produce steps");
        assert!(
            !results.measurements.is_empty(),
            "Simulation should produce measurements"
        );
        assert!(
            results.acceptance_rate() >= 0.0,
            "Acceptance rate should be non-negative"
        );
        assert!(
            results.acceptance_rate() <= 1.0,
            "Acceptance rate should not exceed 1.0"
        );
        assert!(
            results.average_action().is_finite(),
            "Average action should be finite"
        );
    }

    #[test]
    fn test_edge_counting_consistency() {
        // Test that edge counting is consistent using a fixed seed for deterministic results
        // Seed 13 produces a deterministic planar triangulation with boundary (χ = 1)
        const TRIANGULATION_SEED: u64 = 13;

        let triangulation = CdtTriangulation::from_seeded_points(7, 3, 2, TRIANGULATION_SEED)
            .expect("Failed to create triangulation with fixed seed");

        let edge_count = triangulation.edge_count();
        assert!(edge_count > 0, "Should have positive edge count");

        // Edge count should be consistent with Euler's formula
        let v = triangulation.vertex_count();
        let e = edge_count;
        let f = triangulation.face_count();

        // For a manifold with boundary (typical planar triangulation), Euler's formula V - E + F = 1
        let euler =
            i32::try_from(v).unwrap() - i32::try_from(e).unwrap() + i32::try_from(f).unwrap();
        assert_eq!(
            euler, 1,
            "Euler characteristic should be 1 for planar triangulation with boundary, got {euler} (V={v}, E={e}, F={f})"
        );
    }

    #[test]
    fn test_topology_invariants() {
        // Use fixed seed for deterministic topology testing
        // Seed 29 produces a planar triangulation with boundary (χ = 1)
        const TRIANGULATION_SEED: u64 = 29;

        let triangulation = CdtTriangulation::from_seeded_points(6, 1, 2, TRIANGULATION_SEED)
            .expect("Failed to create triangulation with fixed seed");

        let v = i32::try_from(triangulation.vertex_count()).unwrap_or(i32::MAX);
        let e = i32::try_from(triangulation.edge_count()).unwrap_or(i32::MAX);
        let f = i32::try_from(triangulation.face_count()).unwrap_or(i32::MAX);

        // Verify Euler's formula for manifolds with boundary (typical 2D triangulation)
        let euler = v - e + f;
        assert_eq!(
            euler, 1,
            "Euler characteristic should be 1 for planar triangulation with boundary, got {euler} (V={v}, E={e}, F={f})"
        );

        // Verify all counts are positive
        assert!(v > 0, "Must have positive vertex count");
        assert!(e > 0, "Must have positive edge count");
        assert!(f > 0, "Must have positive face count");
    }

    #[test]
    fn test_enhanced_caching_behavior() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 1, 2).expect("Failed to create triangulation");

        // Test cache population
        let initial_count = triangulation.edge_count();
        let cached_count = triangulation.edge_count(); // Should use cache
        assert_eq!(initial_count, cached_count);

        // Test that cache is invalidated on mutation
        {
            let _mut_ref = triangulation.geometry_mut();
        }

        let recalculated_count = triangulation.edge_count();
        assert_eq!(
            initial_count, recalculated_count,
            "Results should be consistent after cache invalidation"
        );
    }

    #[test]
    fn test_error_handling_robustness() {
        // Test parameter validation with enhanced error context
        let result = CdtTriangulation::from_random_points(2, 1, 2);
        assert!(result.is_err(), "Should reject < 3 vertices");

        let result = CdtTriangulation::from_random_points(5, 1, 3);
        assert!(result.is_err(), "Should reject non-2D");

        // Test successful minimum case
        let min_triangulation = CdtTriangulation::from_random_points(3, 1, 2);
        assert!(
            min_triangulation.is_ok(),
            "Minimum valid parameters should succeed"
        );
    }

    #[test]
    fn test_action_calculation_consistency() {
        let triangulation =
            CdtTriangulation::from_random_points(4, 1, 2).expect("Failed to create triangulation");

        let config = ActionConfig::default();
        let vertices = u32::try_from(triangulation.vertex_count()).unwrap_or_default();
        let edges = u32::try_from(triangulation.edge_count()).unwrap_or_default();
        let faces = u32::try_from(triangulation.face_count()).unwrap_or_default();

        let action = config.calculate_action(vertices, edges, faces);

        // Action should be finite and non-NaN
        assert!(
            action.is_finite(),
            "Action calculation must produce finite results"
        );

        // For default config (κ₀=1.0, κ₂=1.0, λ=0.1): S = -V - F + 0.1*E
        let expected = 0.1f64.mul_add(f64::from(edges), -f64::from(vertices) - f64::from(faces));
        assert!(
            (action - expected).abs() < f64::EPSILON,
            "Action formula should match expected calculation"
        );
    }

    #[test]
    fn test_simulation_reproducibility() {
        // Test that simulations with same parameters produce consistent results structure
        let triangulation1 = CdtTriangulation::from_random_points(5, 1, 2)
            .expect("Failed to create first triangulation");
        let triangulation2 = CdtTriangulation::from_random_points(5, 1, 2)
            .expect("Failed to create second triangulation");

        let config = MetropolisConfig::new(1.0, 10, 2, 2);
        let action_config = ActionConfig::default();

        let algorithm1 = MetropolisAlgorithm::new(config.clone(), action_config.clone());
        let algorithm2 = MetropolisAlgorithm::new(config, action_config);

        let results1 = algorithm1
            .run(triangulation1)
            .expect("Run 1 should succeed");
        let results2 = algorithm2
            .run(triangulation2)
            .expect("Run 2 should succeed");

        // Results should have same structure (though values may differ due to randomness)
        assert_eq!(
            results1.steps.len(),
            results2.steps.len(),
            "Should have same number of steps"
        );
        assert_eq!(
            results1.measurements.len(),
            results2.measurements.len(),
            "Should have same number of measurements"
        );

        // Both should produce valid results
        assert!(results1.acceptance_rate().is_finite() && results1.acceptance_rate() >= 0.0);
        assert!(results2.acceptance_rate().is_finite() && results2.acceptance_rate() >= 0.0);
    }

    #[test]
    fn test_memory_efficiency() {
        // Test that large triangulations can be created and processed efficiently
        let triangulation = CdtTriangulation::from_random_points(20, 1, 2)
            .expect("Failed to create large triangulation");

        // Verify reasonable scaling of components
        let vertices = triangulation.vertex_count();
        let edges = triangulation.edge_count();
        let faces = triangulation.face_count();

        assert!(
            (3..=20).contains(&vertices),
            "Should have reasonable number of vertices (3-20), got {vertices}. Random point generation may create duplicates."
        );
        assert!(edges > vertices, "Should have more edges than vertices");
        assert!(faces > 0, "Should have positive face count");

        // Test that edge counting is efficient (doesn't hang)
        let start = std::time::Instant::now();
        let _ = triangulation.edge_count();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 1000,
            "Edge counting should complete quickly"
        );
    }
}
