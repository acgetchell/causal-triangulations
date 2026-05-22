#![forbid(unsafe_code)]
//! Property-based tests for CDT foliation construction and validation.

use causal_triangulations::prelude::geometry::{DelaunayBackend2D, GeometryBackend};
use causal_triangulations::prelude::triangulation::*;
use proptest::prelude::*;

#[derive(Debug, PartialEq, Eq)]
struct StripFingerprint {
    vertices: Vec<VertexFingerprint>,
    faces: Vec<Vec<VertexFingerprint>>,
    slice_sizes: Vec<usize>,
}

type VertexFingerprint = (Vec<u64>, Option<u32>);

/// Captures a vertex by exact coordinate bits and time label for deterministic comparisons.
fn vertex_fingerprint(
    tri: &CdtTriangulation2D,
    vertex: &<DelaunayBackend2D as GeometryBackend>::VertexHandle,
) -> VertexFingerprint {
    let coords = tri
        .geometry()
        .vertex_coordinates(vertex)
        .expect("strip vertex coordinates should resolve")
        .into_iter()
        .map(f64::to_bits)
        .collect();
    (coords, tri.time_label(vertex))
}

/// Builds a canonical strip mesh fingerprint that is stable across handle allocation order.
fn strip_fingerprint(tri: &CdtTriangulation2D) -> StripFingerprint {
    let mut vertices = tri
        .geometry()
        .vertices()
        .map(|vertex| vertex_fingerprint(tri, &vertex))
        .collect::<Vec<_>>();
    vertices.sort();

    let mut faces = tri
        .geometry()
        .faces()
        .map(|face| {
            let mut face_vertices = tri
                .geometry()
                .face_vertices(&face)
                .expect("strip face vertices should resolve")
                .into_iter()
                .map(|vertex| vertex_fingerprint(tri, &vertex))
                .collect::<Vec<_>>();
            face_vertices.sort();
            face_vertices
        })
        .collect::<Vec<_>>();
    faces.sort();

    StripFingerprint {
        vertices,
        faces,
        slice_sizes: tri.slice_sizes().to_vec(),
    }
}

#[test]
fn cdt_strip_builds_delaunay_mesh() {
    let tri = CdtTriangulation::from_cdt_strip(5, 3).expect("Delaunay CDT strip should build");
    assert_eq!(tri.vertex_count(), 15);
    assert_eq!(tri.face_count(), 16);
    tri.geometry()
        .validate_delaunay()
        .expect("Delaunay CDT strip should pass upstream Level 1-4 validation");
    tri.validate_topology()
        .expect("Delaunay CDT strip topology should validate");
    tri.validate_foliation()
        .expect("Delaunay CDT strip foliation should validate");
    tri.validate_causality_delaunay()
        .expect("Delaunay CDT strip causality should validate");
    tri.validate_simplex_classification()
        .expect("Delaunay CDT strip simplices should classify");
}

proptest! {
    /// Property: public point constructors reject zero time slices as invalid
    /// CDT metadata instead of creating triangulations whose validators can
    /// silently skip time-slice checks.
    #[test]
    fn seeded_point_constructor_rejects_zero_time_slices(
        vertices in 3u32..20,
        seed in 0u64..1000,
    ) {
        let result = CdtTriangulation::from_seeded_points(vertices, 0, 2, seed);
        match result {
            Err(CdtError::InvalidTriangulationMetadata {
                field,
                provided_value,
                expected,
                ..
            }) => {
                prop_assert_eq!(field, "timeslices");
                prop_assert_eq!(provided_value, "0");
                prop_assert_eq!(expected, "≥ 1");
            }
            other => prop_assert!(
                false,
                "expected invalid timeslices metadata error, got {other:?}"
            ),
        }
    }

    /// Property: valid metadata survives construction unchanged.
    #[test]
    fn seeded_point_constructor_preserves_valid_metadata(
        vertices in 3u32..20,
        time_slices in 1u32..8,
        seed in 0u64..1000,
    ) {
        let tri = CdtTriangulation::from_seeded_points(vertices, time_slices, 2, seed)
            .expect("valid seeded construction should preserve metadata");

        prop_assert_eq!(tri.time_slices(), time_slices);
        prop_assert_eq!(tri.dimension(), 2);
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    /// Property: periodic toroidal construction preserves Delaunay, topological,
    /// and foliation invariants for small generated N×T meshes.
    #[test]
    fn toroidal_cdt_static_invariants(
        vertices_per_slice in 3u32..8,
        num_slices in 3u32..8,
    ) {
        let tri = CdtTriangulation::from_toroidal_cdt(vertices_per_slice, num_slices)
            .expect("valid toroidal CDT should build");

        let expected_vertices = vertices_per_slice as usize * num_slices as usize;
        prop_assert_eq!(tri.vertex_count(), expected_vertices);
        prop_assert_eq!(tri.face_count(), 2 * expected_vertices);
        prop_assert_eq!(tri.edge_count(), 3 * expected_vertices);
        prop_assert!(tri.has_foliation());
        let expected_slice_sizes = vec![vertices_per_slice as usize; num_slices as usize];
        prop_assert_eq!(tri.slice_sizes(), expected_slice_sizes.as_slice());
        prop_assert!(tri.geometry().validate_delaunay().is_ok(),
            "toroidal CDT must pass upstream Level 1-4 Delaunay validation");
        prop_assert!(tri.validate_topology().is_ok());
        prop_assert!(tri.validate_foliation().is_ok());
        prop_assert!(tri.validate_causality().is_ok());
        prop_assert!(tri.validate_simplex_classification().is_ok());
    }
}

proptest! {
    /// Property: Delaunay CDT strip construction always produces valid foliation and causality.
    ///
    /// For any valid (vertices_per_slice, num_slices):
    /// - vertex count == vertices_per_slice × num_slices
    /// - every slice has exactly vertices_per_slice vertices
    /// - foliation and causality validation both pass
    ///
    #[test]
    fn cdt_strip_invariants(
        vertices_per_slice in 4u32..10,
        num_slices in 2u32..6,
    ) {
        let tri = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("valid Delaunay strip construction should pass");

        // Vertex count must match grid
        let expected_v = vertices_per_slice as usize * num_slices as usize;
        prop_assert_eq!(tri.vertex_count(), expected_v, "Vertex count should match grid");
        let expected_f = 2 * (vertices_per_slice as usize - 1) * (num_slices as usize - 1);
        prop_assert_eq!(tri.face_count(), expected_f, "Face count should match split quads");

        // Must have foliation
        prop_assert!(tri.has_foliation(), "CDT strip must have foliation");
        prop_assert!(tri.geometry().validate_delaunay().is_ok(),
            "CDT strip must pass upstream Level 1-4 Delaunay validation");

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
            "Causality should hold for Delaunay CDT strip with {} vertices/slice, {} slices",
            vertices_per_slice, num_slices);
        prop_assert!(tri.validate_simplex_classification().is_ok(),
            "Every Delaunay strip face should classify as Up or Down");
    }

    /// Property: Every edge in a Delaunay CDT strip is classifiable and
    /// spacelike + timelike == total edges.
    ///
    #[test]
    fn cdt_strip_edge_classification_complete(
        vertices_per_slice in 4u32..8,
        num_slices in 2u32..5,
    ) {
        let tri = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("valid Delaunay strip construction should pass");

        let mut spacelike = 0usize;
        let mut timelike = 0usize;

        for edge in tri.geometry().edges() {
            match tri.edge_type(&edge) {
                Some(EdgeType::Spacelike) => spacelike += 1,
                Some(EdgeType::Timelike) => timelike += 1,
                Some(EdgeType::Acausal) => {
                    prop_assert!(false, "CDT strip should not have acausal edges");
                }
                None => {
                    prop_assert!(false, "Every edge should be classifiable");
                }
            }
        }

        prop_assert_eq!(spacelike + timelike, tri.edge_count(),
            "spacelike + timelike should equal total edge count");
        prop_assert!(spacelike > 0, "Should have spacelike edges");
        prop_assert!(timelike > 0, "Should have timelike edges");
    }

    /// Property: Delaunay CDT strip construction is deterministic for fixed inputs.
    ///
    #[test]
    fn cdt_strip_determinism(
        vertices_per_slice in 4u32..8,
        num_slices in 2u32..5,
    ) {
        let t1 = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("valid Delaunay strip construction should pass");
        let t2 = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("valid Delaunay strip construction should pass");

        prop_assert_eq!(strip_fingerprint(&t1), strip_fingerprint(&t2));
    }
}
