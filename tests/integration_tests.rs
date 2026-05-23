#![forbid(unsafe_code)]

//! Comprehensive integration tests for CDT-RS.
//!
//! This module contains integration tests that verify the complete CDT simulation
//! workflows, topology preservation, error handling, and consistency between components.

use approx::{abs_diff_eq, assert_relative_eq};
use causal_triangulations::prelude::action::ActionConfig;
use causal_triangulations::prelude::simulation::{MetropolisAlgorithm, MetropolisConfig};
use causal_triangulations::prelude::triangulation::{
    CdtTopology, CdtTriangulation, TriangulationQuery,
};
use std::time::Instant;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_complete_cdt_simulation_workflow_runs_moves() {
        // Test full CDT simulation pipeline
        let triangulation =
            CdtTriangulation::from_cdt_strip(8, 4).expect("Failed to create initial triangulation");
        triangulation
            .validate_foliation()
            .expect("initial strip foliation is valid");
        triangulation
            .validate_causality()
            .expect("initial strip causality is valid");

        let config = MetropolisConfig::new(1.0, 10, 5, 2).with_seed(42);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        let results = algorithm
            .run(triangulation)
            .expect("simulation should execute real move loop");
        assert_eq!(results.steps().len(), 10);
        assert!(
            results.acceptance_rate() > 0.0,
            "real move loop should accept at least one move"
        );
        assert!(!results.measurements().is_empty());
        assert!(results.average_action().is_finite());
        assert!(
            results.steps().iter().any(|step| {
                step.action_after.is_some_and(|action_after| {
                    !abs_diff_eq!(action_after, step.action_before, epsilon = f64::EPSILON)
                })
            }),
            "accepted moves should change the action over time"
        );
        assert!(
            results
                .triangulation()
                .geometry()
                .triangulation()
                .tds()
                .is_valid()
                .is_ok(),
            "final triangulation should remain structurally valid"
        );
    }

    #[test]
    fn test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology() {
        const STEPS: u32 = 80;

        let triangulation = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        assert_eq!(triangulation.metadata().topology, CdtTopology::Toroidal);
        assert_eq!(triangulation.geometry().euler_characteristic(), 0);
        let initial_profile = triangulation.volume_profile();
        triangulation
            .validate_topology()
            .expect("initial toroidal topology is valid");
        triangulation
            .validate_foliation()
            .expect("initial toroidal foliation is valid");

        let config = MetropolisConfig::new(1.0, STEPS, 0, 10).with_seed(105);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let results = algorithm
            .run(triangulation)
            .expect("toroidal simulation should preserve move invariants");

        assert_eq!(results.steps().len(), STEPS as usize);
        assert_eq!(results.move_stats().total_attempted(), u64::from(STEPS));
        assert!(
            results.move_stats().total_accepted() > 0,
            "periodic toroidal simulation should accept at least one move"
        );
        assert!(
            results.move_stats().moves_13_accepted > 0,
            "periodic toroidal simulation should accept at least one volume-increasing move"
        );
        assert_ne!(
            results.triangulation().volume_profile(),
            initial_profile,
            "periodic toroidal volume moves should change the final volume profile"
        );
        assert_eq!(
            results.triangulation().metadata().topology,
            CdtTopology::Toroidal
        );
        assert_eq!(results.triangulation().geometry().euler_characteristic(), 0);
        results
            .triangulation()
            .validate_topology()
            .expect("final toroidal topology remains valid");
        results
            .triangulation()
            .validate_foliation()
            .expect("final toroidal foliation remains closed S1 rings");
        results
            .triangulation()
            .validate_causality()
            .expect("final toroidal causality remains valid");
        results
            .triangulation()
            .validate_simplex_classification()
            .expect("final toroidal simplex classification remains valid");
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
        triangulation.refresh_cache();
        let initial_count = triangulation.edge_count();
        let cached_count = triangulation.edge_count(); // Should use cache
        assert_eq!(initial_count, cached_count);

        // Test that cache is invalidated by a safe metadata mutation.
        triangulation
            .set_time_slices(2)
            .expect("open-boundary time-slice metadata can be widened");

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
        assert_relative_eq!(action, expected, epsilon = f64::EPSILON);
    }

    #[test]
    fn test_seeded_simulation_reproducibility() {
        // Test that seeded simulation inputs consistently produce the same move trace.
        let triangulation1 =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Failed to create first triangulation");
        let triangulation2 =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Failed to create second triangulation");

        let config = MetropolisConfig::new(1.0, 10, 2, 2).with_seed(123);
        let action_config = ActionConfig::default();

        let algorithm1 = MetropolisAlgorithm::new(config.clone(), action_config.clone());
        let algorithm2 = MetropolisAlgorithm::new(config, action_config);

        let results1 = algorithm1
            .run(triangulation1)
            .expect("Run 1 should succeed");
        let results2 = algorithm2
            .run(triangulation2)
            .expect("Run 2 should succeed");

        assert_eq!(results1.steps().len(), results2.steps().len());
        for (left, right) in results1.steps().iter().zip(results2.steps().iter()) {
            assert_eq!(left.move_type, right.move_type);
            assert_eq!(left.accepted, right.accepted);
        }
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
        let start = Instant::now();
        let _ = triangulation.edge_count();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 1000,
            "Edge counting should complete quickly"
        );
    }
}
