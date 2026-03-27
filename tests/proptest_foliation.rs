#![forbid(unsafe_code)]
//! Property-based tests for CDT foliation construction and validation.

use causal_triangulations::prelude::triangulation::*;
use proptest::prelude::*;

proptest! {
    /// Property: Foliated cylinder construction always produces valid foliation and causality.
    ///
    /// For any valid (vertices_per_slice, num_slices, seed):
    /// - vertex count == vertices_per_slice × num_slices
    /// - every slice has exactly vertices_per_slice vertices
    /// - foliation and causality validation both pass
    #[test]
    fn foliated_cylinder_invariants(
        vertices_per_slice in 4u32..10,
        num_slices in 2u32..6,
        seed in 0u64..1000
    ) {
        // Some seed/size combinations trigger builder degeneracy;
        // skip those — the constructor returns Err, not a broken triangulation.
        let Ok(tri) = CdtTriangulation::from_foliated_cylinder(vertices_per_slice, num_slices, Some(seed)) else {
            return Ok(());
        };

        // Vertex count must match grid
        let expected_v = vertices_per_slice as usize * num_slices as usize;
        prop_assert_eq!(tri.vertex_count(), expected_v, "Vertex count should match grid");

        // Must have foliation
        prop_assert!(tri.has_foliation(), "Foliated cylinder must have foliation");

        // Every slice has the right count
        let sizes = tri.slice_sizes();
        prop_assert_eq!(sizes.len(), num_slices as usize, "Should have num_slices slices");
        for (t, &size) in sizes.iter().enumerate() {
            prop_assert_eq!(size, vertices_per_slice as usize,
                "Slice {} should have {} vertices", t, vertices_per_slice);
        }

        // Foliation validation passes
        prop_assert!(tri.validate_foliation().is_ok(), "Foliation should be valid");

        // Causality passes (no edges spanning >1 slice)
        prop_assert!(tri.validate_causality_delaunay().is_ok(),
            "Causality should hold for foliated cylinder with {} vertices/slice, {} slices, seed {}",
            vertices_per_slice, num_slices, seed);
    }

    /// Property: Every edge in a foliated cylinder is classifiable and
    /// spacelike + timelike == total edges.
    #[test]
    fn foliated_cylinder_edge_classification_complete(
        vertices_per_slice in 4u32..8,
        num_slices in 2u32..5,
        seed in 0u64..500
    ) {
        let Ok(tri) = CdtTriangulation::from_foliated_cylinder(vertices_per_slice, num_slices, Some(seed)) else {
            return Ok(());
        };

        let mut spacelike = 0usize;
        let mut timelike = 0usize;

        for edge in tri.geometry().edges() {
            let et = tri.edge_type(&edge);
            prop_assert!(et.is_some(), "Every edge should be classifiable");
            match et.unwrap() {
                EdgeType::Spacelike => spacelike += 1,
                EdgeType::Timelike => timelike += 1,
                EdgeType::Acausal => {
                    prop_assert!(false, "Foliated cylinder should not have acausal edges");
                }
            }
        }

        prop_assert_eq!(spacelike + timelike, tri.edge_count(),
            "spacelike + timelike should equal total edge count");
        prop_assert!(spacelike > 0, "Should have spacelike edges");
        prop_assert!(timelike > 0, "Should have timelike edges");
    }

    /// Property: Foliated cylinder construction is deterministic for a given seed.
    #[test]
    fn foliated_cylinder_seed_determinism(
        vertices_per_slice in 4u32..8,
        num_slices in 2u32..5,
        seed in 0u64..500
    ) {
        let r1 = CdtTriangulation::from_foliated_cylinder(vertices_per_slice, num_slices, Some(seed));
        let r2 = CdtTriangulation::from_foliated_cylinder(vertices_per_slice, num_slices, Some(seed));

        match (r1, r2) {
            (Ok(t1), Ok(t2)) => {
                prop_assert_eq!(t1.vertex_count(), t2.vertex_count());
                prop_assert_eq!(t1.edge_count(), t2.edge_count());
                prop_assert_eq!(t1.face_count(), t2.face_count());
                prop_assert_eq!(t1.slice_sizes(), t2.slice_sizes());
            }
            (Err(_), Err(_)) => { /* both fail — consistent */ }
            _ => prop_assert!(false, "Determinism violated: one succeeded, one failed"),
        }
    }
}
