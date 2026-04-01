#![forbid(unsafe_code)]
//! Property-based tests for CDT foliation construction and validation.

use causal_triangulations::prelude::geometry::DelaunayBackend2D;
use causal_triangulations::prelude::triangulation::*;
use causal_triangulations::{CdtError, CdtResult};
use proptest::prelude::*;

fn assert_cdt_strip_known_failure(result: CdtResult<CdtTriangulation<DelaunayBackend2D>>) {
    match result {
        Err(CdtError::ValidationFailed { check, detail }) => {
            assert!(
                check == "cdt_construction" && detail.contains("not yet implemented"),
                "Expected explicit strip-construction placeholder failure, got check={check:?}, detail={detail}"
            );
        }
        Ok(_) => panic!("Expected explicit strip-construction rejection"),
        Err(other) => panic!("Expected ValidationFailed, got {other:?}"),
    }
}

#[test]
fn cdt_strip_known_limitation_regression_guard() {
    assert_cdt_strip_known_failure(CdtTriangulation::from_cdt_strip(5, 3));
}

proptest! {
    /// Property: Explicit CDT strip construction always produces valid foliation and causality.
    ///
    /// For any valid (vertices_per_slice, num_slices, seed):
    /// - vertex count == vertices_per_slice × num_slices
    /// - every slice has exactly vertices_per_slice vertices
    /// - foliation and causality validation both pass
    ///
    /// TODO(#57): Re-enable this as an active invariant when explicit CDT strip
    /// construction is available (blocked on delaunay/293).
    #[test]
    #[ignore = "TODO(#57): blocked on delaunay/293 explicit strip construction"]
    fn cdt_strip_invariants(
        vertices_per_slice in 4u32..10,
        num_slices in 2u32..6,
        seed in 0u64..1000
    ) {
        let _ = seed; // deterministic seed reserved for future explicit-strip implementation
        let tri = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("TODO(#57): expected to pass once explicit strip construction lands");

        // Vertex count must match grid
        let expected_v = vertices_per_slice as usize * num_slices as usize;
        prop_assert_eq!(tri.vertex_count(), expected_v, "Vertex count should match grid");

        // Must have foliation
        prop_assert!(tri.has_foliation(), "CDT strip must have foliation");

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

    /// Property: Every edge in an explicit CDT strip is classifiable and
    /// spacelike + timelike == total edges.
    ///
    /// TODO(#57): Re-enable this as an active invariant when explicit CDT strip
    /// construction is available (blocked on delaunay/293).
    #[test]
    #[ignore = "TODO(#57): blocked on delaunay/293 explicit strip construction"]
    fn cdt_strip_edge_classification_complete(
        vertices_per_slice in 4u32..8,
        num_slices in 2u32..5,
        seed in 0u64..500
    ) {
        let _ = seed; // deterministic seed reserved for future explicit-strip implementation
        let tri = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("TODO(#57): expected to pass once explicit strip construction lands");

        let mut spacelike = 0usize;
        let mut timelike = 0usize;

        for edge in tri.geometry().edges() {
            let et = tri.edge_type(&edge);
            prop_assert!(et.is_some(), "Every edge should be classifiable");
            match et.unwrap() {
                EdgeType::Spacelike => spacelike += 1,
                EdgeType::Timelike => timelike += 1,
                EdgeType::Acausal => {
                    prop_assert!(false, "CDT strip should not have acausal edges");
                }
            }
        }

        prop_assert_eq!(spacelike + timelike, tri.edge_count(),
            "spacelike + timelike should equal total edge count");
        prop_assert!(spacelike > 0, "Should have spacelike edges");
        prop_assert!(timelike > 0, "Should have timelike edges");
    }

    /// Property: Explicit CDT strip construction is deterministic for fixed inputs.
    ///
    /// TODO(#57): Re-enable this as an active invariant when explicit CDT strip
    /// construction is available (blocked on delaunay/293).
    #[test]
    #[ignore = "TODO(#57): blocked on delaunay/293 explicit strip construction"]
    fn cdt_strip_determinism(
        vertices_per_slice in 4u32..8,
        num_slices in 2u32..5,
        seed in 0u64..500
    ) {
        let _ = seed; // deterministic seed reserved for future explicit-strip implementation
        let t1 = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("TODO(#57): expected to pass once explicit strip construction lands");
        let t2 = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("TODO(#57): expected to pass once explicit strip construction lands");

        prop_assert_eq!(t1.vertex_count(), t2.vertex_count());
        prop_assert_eq!(t1.edge_count(), t2.edge_count());
        prop_assert_eq!(t1.face_count(), t2.face_count());
        prop_assert_eq!(t1.slice_sizes(), t2.slice_sizes());
    }
}
