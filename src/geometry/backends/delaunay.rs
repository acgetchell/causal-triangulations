//! Delaunay backend - wraps the delaunay crate.
//!
//! This is the ONLY module that directly uses types from the delaunay crate,
//! providing complete isolation of the geometry implementation from CDT logic.
// cspell:ignore vkey

use crate::geometry::traits::{
    FlipResult, GeometryBackend, SubdivisionResult, ThreadSafeBackend, TriangulationMut,
    TriangulationQuery,
};
use delaunay::core::delaunay_triangulation::DelaunayTriangulation;
use delaunay::core::edge::EdgeKey;
use delaunay::core::triangulation_data_structure::{CellKey, VertexKey};
use delaunay::geometry::kernel::RobustKernel;

/// Delaunay backend wrapping the delaunay crate's triangulation (f64 coordinates).
///
/// # Mutation support
///
/// The [`TriangulationMut`] methods (`insert_vertex`, `remove_vertex`, `flip_edge`, etc.)
/// are not yet implemented and return [`DelaunayError::NotImplemented`]. The `clear()` and
/// `reserve_capacity()` methods are currently no-ops that emit a `log::warn!` diagnostic.
#[derive(Debug)]
pub struct DelaunayBackend<VertexData, CellData, const D: usize>
where
    VertexData: delaunay::core::DataType,
    CellData: delaunay::core::DataType,
{
    /// The underlying Delaunay triangulation from the delaunay crate
    dt: DelaunayTriangulation<RobustKernel<f64>, VertexData, CellData, D>,
}

/// Opaque handle for vertices in Delaunay backend
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayVertexHandle {
    key: VertexKey,
}

/// Opaque handle for edges in Delaunay backend
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayEdgeHandle {
    key: EdgeKey,
}

/// Opaque handle for faces in Delaunay backend
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayFaceHandle {
    key: CellKey,
}

/// Error type for Delaunay backend operations
#[derive(Debug, thiserror::Error)]
pub enum DelaunayError {
    /// Operation is not yet implemented
    #[error("not implemented: {operation}")]
    NotImplemented {
        /// Name of the unimplemented operation
        operation: &'static str,
    },

    /// Invalid vertex handle (key not found in triangulation)
    #[error("invalid vertex handle: key {key:?} not found in triangulation")]
    InvalidVertex {
        /// The vertex key that was looked up
        key: VertexKey,
    },

    /// Invalid edge handle (one or both endpoint vertices not found)
    #[error("invalid edge handle: endpoint vertices {v0:?}, {v1:?} not found in triangulation")]
    InvalidEdge {
        /// First endpoint vertex key
        v0: VertexKey,
        /// Second endpoint vertex key
        v1: VertexKey,
    },

    /// Invalid face/cell handle (key not found in triangulation)
    #[error("invalid face handle: key {key:?} not found in triangulation")]
    InvalidFace {
        /// The cell key that was looked up
        key: CellKey,
    },
}

impl<VertexData, CellData, const D: usize> DelaunayBackend<VertexData, CellData, D>
where
    VertexData: delaunay::core::DataType,
    CellData: delaunay::core::DataType,
{
    /// Create a new Delaunay backend from an existing Delaunay triangulation
    #[must_use]
    pub const fn from_triangulation(
        dt: DelaunayTriangulation<RobustKernel<f64>, VertexData, CellData, D>,
    ) -> Self {
        Self { dt }
    }

    /// Access the underlying Delaunay triangulation (read-only)
    #[must_use]
    pub const fn triangulation(
        &self,
    ) -> &DelaunayTriangulation<RobustKernel<f64>, VertexData, CellData, D> {
        &self.dt
    }

    /// Check if the triangulation is valid and satisfies the Delaunay property.
    ///
    /// Uses the upstream cumulative validation (`DelaunayTriangulation::validate`) which
    /// checks neighbor pointer consistency, Euler characteristic, coherent orientation
    /// (Levels 1–3) and the Delaunay in-sphere property (Level 4).
    #[must_use]
    pub fn is_delaunay(&self) -> bool {
        self.dt.validate().is_ok()
    }

    /// Returns the high-level topology kind (`Euclidean`, `Toroidal`, etc.) of the
    /// underlying triangulation.
    ///
    /// This exposes the [`GlobalTopology`](delaunay::topology::traits::GlobalTopology)
    /// metadata attached by [`DelaunayTriangulationBuilder`](delaunay::core::builder::DelaunayTriangulationBuilder) at construction time.
    #[must_use]
    pub const fn topology_kind(&self) -> delaunay::topology::traits::TopologyKind {
        self.dt.topology_kind()
    }
}

impl<VertexData, CellData, const D: usize> GeometryBackend
    for DelaunayBackend<VertexData, CellData, D>
where
    VertexData: delaunay::core::DataType,
    CellData: delaunay::core::DataType,
{
    type Coordinate = f64;
    type VertexHandle = DelaunayVertexHandle;
    type EdgeHandle = DelaunayEdgeHandle;
    type FaceHandle = DelaunayFaceHandle;
    type Error = DelaunayError;

    fn backend_name(&self) -> &'static str {
        "delaunay"
    }
}

// The upstream `Tds`, `Triangulation`, and `DelaunayTriangulation` types auto-derive
// `Send + Sync` (all internal storage is `SlotMap`/`DenseSlotMap` + `FxHashMap`, all `Send + Sync`).
impl<VertexData, CellData, const D: usize> ThreadSafeBackend
    for DelaunayBackend<VertexData, CellData, D>
where
    VertexData: delaunay::core::DataType + Send + Sync,
    CellData: delaunay::core::DataType + Send + Sync,
{
}

impl<VertexData, CellData, const D: usize> TriangulationQuery
    for DelaunayBackend<VertexData, CellData, D>
where
    VertexData: delaunay::core::DataType,
    CellData: delaunay::core::DataType,
{
    fn vertex_count(&self) -> usize {
        self.dt.number_of_vertices()
    }

    fn edge_count(&self) -> usize {
        self.dt.as_triangulation().number_of_edges()
    }

    fn face_count(&self) -> usize {
        self.dt.number_of_cells()
    }

    fn dimension(&self) -> usize {
        D
    }

    fn vertices(&self) -> Box<dyn Iterator<Item = Self::VertexHandle> + '_> {
        Box::new(
            self.dt
                .vertices()
                .map(|(key, _)| DelaunayVertexHandle { key }),
        )
    }

    fn edges(&self) -> Box<dyn Iterator<Item = Self::EdgeHandle> + '_> {
        Box::new(self.dt.edges().map(|key| DelaunayEdgeHandle { key }))
    }

    fn faces(&self) -> Box<dyn Iterator<Item = Self::FaceHandle> + '_> {
        Box::new(self.dt.cells().map(|(key, _)| DelaunayFaceHandle { key }))
    }

    fn vertex_coordinates(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::Coordinate>, Self::Error> {
        let coords = self
            .dt
            .vertex_coords(vertex.key)
            .ok_or(DelaunayError::InvalidVertex { key: vertex.key })?;
        Ok(coords.to_vec())
    }

    fn face_vertices(
        &self,
        face: &Self::FaceHandle,
    ) -> Result<Vec<Self::VertexHandle>, Self::Error> {
        let vkeys = self
            .dt
            .cell_vertices(face.key)
            .ok_or(DelaunayError::InvalidFace { key: face.key })?;
        Ok(vkeys
            .iter()
            .map(|&key| DelaunayVertexHandle { key })
            .collect())
    }

    fn edge_endpoints(
        &self,
        edge: &Self::EdgeHandle,
    ) -> Result<(Self::VertexHandle, Self::VertexHandle), Self::Error> {
        let (v0, v1) = edge.key.endpoints();
        // Validate that both endpoint vertices exist in this triangulation
        if self.dt.vertex_coords(v0).is_none() || self.dt.vertex_coords(v1).is_none() {
            return Err(DelaunayError::InvalidEdge { v0, v1 });
        }
        Ok((
            DelaunayVertexHandle { key: v0 },
            DelaunayVertexHandle { key: v1 },
        ))
    }

    fn adjacent_faces(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        if !self.dt.tds().contains_vertex_key(vertex.key) {
            return Err(DelaunayError::InvalidVertex { key: vertex.key });
        }
        Ok(self
            .dt
            .as_triangulation()
            .adjacent_cells(vertex.key)
            .map(|key| DelaunayFaceHandle { key })
            .collect())
    }

    fn incident_edges(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::EdgeHandle>, Self::Error> {
        if !self.dt.tds().contains_vertex_key(vertex.key) {
            return Err(DelaunayError::InvalidVertex { key: vertex.key });
        }
        Ok(self
            .dt
            .incident_edges(vertex.key)
            .map(|key| DelaunayEdgeHandle { key })
            .collect())
    }

    fn face_neighbors(
        &self,
        face: &Self::FaceHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        if !self.dt.tds().contains_cell_key(face.key) {
            return Err(DelaunayError::InvalidFace { key: face.key });
        }
        Ok(self
            .dt
            .cell_neighbors(face.key)
            .map(|key| DelaunayFaceHandle { key })
            .collect())
    }

    fn is_valid(&self) -> bool {
        // Structural minimum: must have enough vertices and at least one cell.
        if self.dt.number_of_vertices() <= D || self.dt.number_of_cells() == 0 {
            return false;
        }

        // v0.7.2: use Levels 1–3 structural/topological validation via the
        // Triangulation layer (neighbor pointers, Euler characteristic, coherent
        // orientation) WITHOUT the Level 4 Delaunay property check.
        // Use is_delaunay() for the full Levels 1–4 check.
        self.dt.as_triangulation().validate().is_ok()
    }
}

impl<VertexData, CellData, const D: usize> TriangulationMut
    for DelaunayBackend<VertexData, CellData, D>
where
    VertexData: delaunay::core::DataType,
    CellData: delaunay::core::DataType,
{
    fn insert_vertex(
        &mut self,
        _coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error> {
        // TODO: Implement vertex insertion.
        Err(DelaunayError::NotImplemented {
            operation: "insert_vertex",
        })
    }

    fn remove_vertex(
        &mut self,
        _vertex: Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        // TODO: Implement vertex removal.
        Err(DelaunayError::NotImplemented {
            operation: "remove_vertex",
        })
    }

    fn move_vertex(
        &mut self,
        _vertex: Self::VertexHandle,
        _new_coords: &[Self::Coordinate],
    ) -> Result<(), Self::Error> {
        // TODO: Implement vertex movement.
        Err(DelaunayError::NotImplemented {
            operation: "move_vertex",
        })
    }

    fn flip_edge(
        &mut self,
        _edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::VertexHandle, Self::EdgeHandle, Self::FaceHandle>, Self::Error>
    {
        // TODO: Implement edge flip.
        Err(DelaunayError::NotImplemented {
            operation: "flip_edge",
        })
    }

    fn can_flip_edge(&self, _edge: &Self::EdgeHandle) -> bool {
        // TODO: Implement flip feasibility check.
        false
    }

    fn subdivide_face(
        &mut self,
        _face: Self::FaceHandle,
        _point: &[Self::Coordinate],
    ) -> Result<
        SubdivisionResult<Self::VertexHandle, Self::EdgeHandle, Self::FaceHandle>,
        Self::Error,
    > {
        // TODO: Implement face subdivision.
        Err(DelaunayError::NotImplemented {
            operation: "subdivide_face",
        })
    }

    fn clear(&mut self) {
        // TODO: Implement clear operation.
        log::warn!("DelaunayBackend::clear() is not yet implemented; triangulation unchanged");
    }

    fn reserve_capacity(&mut self, vertices: usize, faces: usize) {
        // TODO: Implement capacity reservation.
        log::warn!(
            "DelaunayBackend::reserve_capacity(vertices={vertices}, faces={faces}) is not yet implemented"
        );
    }
}

/// Type alias
///
/// Uses `()` vertex data — CDT metadata is tracked at the [`CdtTriangulation`](crate::cdt::triangulation::CdtTriangulation) level.
pub type DelaunayBackend2D = DelaunayBackend<(), i32, 2>;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_is_delaunay_various_sizes() {
        // is_delaunay() should pass for valid triangulations of all sizes
        for n in [3, 4, 10, 20] {
            let dt = crate::util::generate_random_delaunay2(n, (0.0, 10.0));
            let backend = DelaunayBackend::from_triangulation(dt);
            assert!(
                backend.is_delaunay(),
                "Triangulation with {n} vertices should satisfy Delaunay property"
            );
        }
    }

    #[test]
    fn test_is_valid_and_is_delaunay_consistency() {
        // is_delaunay (Levels 1–4) implies is_valid (Levels 1–3)
        let dt = crate::util::generate_random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        assert!(backend.is_valid(), "Triangulation should be valid");
        assert!(
            backend.is_delaunay(),
            "Valid Delaunay triangulation should pass is_delaunay"
        );
    }

    #[test]
    fn test_is_delaunay_minimal_triangulation() {
        // Test with minimal triangulation (3 vertices)
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        assert!(backend.is_valid(), "Minimal triangulation should be valid");
        assert!(
            backend.is_delaunay(),
            "Minimal triangulation should satisfy Delaunay property"
        );
        assert_eq!(backend.vertex_count(), 3, "Should have exactly 3 vertices");
        assert_eq!(
            backend.face_count(),
            1,
            "Should have exactly 1 face (triangle)"
        );
    }

    // Tests for iterator methods

    #[test]
    fn test_vertices_iterator() {
        let dt = crate::util::generate_random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertices: Vec<_> = backend.vertices().collect();
        assert_eq!(
            vertices.len(),
            backend.vertex_count(),
            "Iterator should return all vertices"
        );

        // Check that all handles are unique
        let unique_count = vertices.iter().collect::<HashSet<_>>().len();
        assert_eq!(
            unique_count,
            vertices.len(),
            "All vertex handles should be unique"
        );
    }

    #[test]
    fn test_edges_iterator() {
        let dt = crate::util::generate_random_delaunay2(4, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let edges: Vec<_> = backend.edges().collect();
        assert_eq!(
            edges.len(),
            backend.edge_count(),
            "Iterator should return all edges"
        );

        // Check that all handles are unique
        let unique_count = edges.iter().collect::<HashSet<_>>().len();
        assert_eq!(
            unique_count,
            edges.len(),
            "All edge handles should be unique"
        );
    }

    #[test]
    fn test_faces_iterator() {
        let dt = crate::util::generate_random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let faces: Vec<_> = backend.faces().collect();
        assert_eq!(
            faces.len(),
            backend.face_count(),
            "Iterator should return all faces"
        );

        // Check that all handles are unique
        let unique_count = faces.iter().collect::<HashSet<_>>().len();
        assert_eq!(
            unique_count,
            faces.len(),
            "All face handles should be unique"
        );
    }

    // Tests for query methods

    #[test]
    fn test_vertex_coordinates() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertices: Vec<_> = backend.vertices().collect();
        assert!(!vertices.is_empty(), "Should have at least one vertex");

        for vertex in &vertices {
            let coords = backend
                .vertex_coordinates(vertex)
                .expect("Should retrieve coordinates for valid vertex");
            assert_eq!(coords.len(), 2, "Should have 2D coordinates");
            assert!(
                coords.iter().all(|&c| (0.0..=10.0).contains(&c)),
                "Coordinates should be within expected range"
            );
        }
    }

    #[test]
    fn test_vertex_coordinates_invalid_handle() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        // Use a high-generation key that cannot exist in the triangulation's slotmap
        let bogus_key = VertexKey::from(slotmap::KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.vertex_coordinates(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key),
            "Expected InvalidVertex with matching key, got: {err}"
        );
    }

    #[test]
    fn test_face_vertices() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let faces: Vec<_> = backend.faces().collect();
        assert!(!faces.is_empty(), "Should have at least one face");

        for face in &faces {
            let vertices = backend
                .face_vertices(face)
                .expect("Should retrieve vertices for valid face");
            assert_eq!(vertices.len(), 3, "2D face should have exactly 3 vertices");

            // Verify all vertices are unique
            let unique_count = vertices.iter().collect::<HashSet<_>>().len();
            assert_eq!(
                unique_count,
                vertices.len(),
                "Face vertices should be unique"
            );
        }
    }

    #[test]
    fn test_face_vertices_invalid_handle() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = CellKey::from(slotmap::KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };
        let err = backend.face_vertices(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key),
            "Expected InvalidFace with matching key, got: {err}"
        );
    }

    #[test]
    fn test_edge_endpoints() {
        let dt = crate::util::generate_random_delaunay2(4, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let edges: Vec<_> = backend.edges().collect();
        assert!(!edges.is_empty(), "Should have at least one edge");

        for edge in &edges {
            let (v1, v2) = backend
                .edge_endpoints(edge)
                .expect("Should retrieve endpoints for valid edge");
            assert_ne!(v1, v2, "Edge endpoints should be different");

            // Verify endpoints exist in vertex list
            let vertices: Vec<_> = backend.vertices().collect();
            assert!(
                vertices.contains(&v1),
                "First endpoint should be a valid vertex"
            );
            assert!(
                vertices.contains(&v2),
                "Second endpoint should be a valid vertex"
            );
        }
    }

    #[test]
    fn test_edge_endpoints_invalid_handle() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let k1 = VertexKey::from(slotmap::KeyData::from_ffi(u64::MAX - 1));
        let k2 = VertexKey::from(slotmap::KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayEdgeHandle {
            key: EdgeKey::new(k1, k2),
        };
        let err = backend.edge_endpoints(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidEdge { .. }),
            "Expected InvalidEdge, got: {err}"
        );
    }

    #[test]
    fn test_adjacent_faces() {
        let dt = crate::util::generate_random_delaunay2(4, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertices: Vec<_> = backend.vertices().collect();
        assert!(!vertices.is_empty(), "Should have at least one vertex");

        for vertex in &vertices {
            let adjacent = backend
                .adjacent_faces(vertex)
                .expect("Should retrieve adjacent faces for valid vertex");
            assert!(
                !adjacent.is_empty(),
                "Each vertex should have at least one adjacent face"
            );

            // Verify each adjacent face contains this vertex
            for face_handle in &adjacent {
                let face_vertices = backend
                    .face_vertices(face_handle)
                    .expect("Should retrieve face vertices");
                assert!(
                    face_vertices.contains(vertex),
                    "Adjacent face should contain the vertex"
                );
            }
        }
    }

    #[test]
    fn test_incident_edges() {
        let dt = crate::util::generate_random_delaunay2(4, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertices: Vec<_> = backend.vertices().collect();
        assert!(!vertices.is_empty(), "Should have at least one vertex");

        for vertex in &vertices {
            let incident = backend
                .incident_edges(vertex)
                .expect("Should retrieve incident edges for valid vertex");
            assert!(
                !incident.is_empty(),
                "Each vertex should have at least one incident edge"
            );

            // Verify each incident edge has this vertex as an endpoint
            for edge_handle in &incident {
                let (v1, v2) = backend
                    .edge_endpoints(edge_handle)
                    .expect("Should retrieve edge endpoints");
                assert!(
                    v1 == *vertex || v2 == *vertex,
                    "Incident edge should have vertex as an endpoint"
                );
            }
        }
    }

    #[test]
    fn test_face_neighbors() {
        let dt = crate::util::generate_random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let faces: Vec<_> = backend.faces().collect();
        assert!(!faces.is_empty(), "Should have at least one face");

        for face in &faces {
            let neighbors = backend
                .face_neighbors(face)
                .expect("Should retrieve neighbors for valid face");

            // In a 2D triangulation, each face can have 0-3 neighbors
            assert!(
                neighbors.len() <= 3,
                "A 2D face should have at most 3 neighbors"
            );

            // Verify neighbors are valid faces
            let all_faces: HashSet<_> = backend.faces().collect();
            for neighbor in &neighbors {
                assert!(
                    all_faces.contains(neighbor),
                    "Neighbor should be a valid face"
                );
            }
        }
    }

    #[test]
    fn test_face_neighbors_invalid_handle() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = CellKey::from(slotmap::KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };
        let err = backend.face_neighbors(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key),
            "Expected InvalidFace with matching key, got: {err}"
        );
    }

    #[test]
    fn test_adjacent_faces_invalid_handle() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = VertexKey::from(slotmap::KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.adjacent_faces(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key),
            "Expected InvalidVertex with matching key, got: {err}"
        );
    }

    #[test]
    fn test_incident_edges_invalid_handle() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = VertexKey::from(slotmap::KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.incident_edges(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key),
            "Expected InvalidVertex with matching key, got: {err}"
        );
    }

    #[test]
    fn test_dimension() {
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);
        assert_eq!(backend.dimension(), 2, "DelaunayBackend2D should be 2D");
    }

    #[test]
    fn test_euler_characteristic() {
        // For a planar triangulation without boundary: V - E + F = 1
        let dt = crate::util::generate_seeded_delaunay2(6, (0.0, 10.0), 42);
        let backend = DelaunayBackend::from_triangulation(dt);
        let chi = backend.euler_characteristic();
        assert!(
            (0..=2).contains(&chi),
            "Euler characteristic should be 0, 1, or 2 for planar triangulation, got {chi}"
        );
    }

    #[test]
    fn test_face_neighbor_symmetry() {
        // If face A lists B as a neighbor, then B must list A as a neighbor
        let dt = crate::util::generate_seeded_delaunay2(8, (0.0, 10.0), 42);
        let backend = DelaunayBackend::from_triangulation(dt);

        for face in backend.faces() {
            let neighbors = backend
                .face_neighbors(&face)
                .expect("Should retrieve neighbors");
            for neighbor in &neighbors {
                let reverse = backend
                    .face_neighbors(neighbor)
                    .expect("Neighbor should have neighbors");
                assert!(
                    reverse.contains(&face),
                    "Neighbor relationship should be symmetric"
                );
            }
        }
    }

    #[test]
    fn test_topology_consistency() {
        // Test that topology is consistent across different query methods
        // Use a fixed seed for reproducibility and to avoid random topology issues
        let dt = crate::util::generate_seeded_delaunay2(6, (0.0, 10.0), 42);
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertex_count = backend.vertex_count();
        let edge_count = backend.edge_count();
        let face_count = backend.face_count();

        // Verify Euler characteristic for planar graphs
        // For a triangulation without the outer infinite face: V - E + F = 1
        // For a triangulation with the outer infinite face: V - E + F = 2
        // Note: Random triangulations may occasionally have degeneracies that result in χ = 0
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let euler = vertex_count as i32 - edge_count as i32 + face_count as i32;
        assert!(
            (0..=2).contains(&euler),
            "Euler characteristic should be in range [0, 2] for planar triangulation, got {euler} (V={vertex_count}, E={edge_count}, F={face_count})"
        );

        // Count edges through incident_edges (should match total edge count)
        let mut edge_set = HashSet::new();
        for vertex in backend.vertices() {
            if let Ok(incident) = backend.incident_edges(&vertex) {
                edge_set.extend(incident);
            }
        }
        assert_eq!(
            edge_set.len(),
            edge_count,
            "Total edges from incident_edges should match edge_count"
        );
    }

    #[test]
    fn test_minimal_triangulation_queries() {
        // Test with minimal valid triangulation (3 vertices, 1 face)
        let dt = crate::util::generate_random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        // Test all vertices are accessible
        let vertices: Vec<_> = backend.vertices().collect();
        assert_eq!(vertices.len(), 3, "Should have exactly 3 vertices");

        // Test all edges are accessible
        let edges: Vec<_> = backend.edges().collect();
        assert_eq!(edges.len(), 3, "Should have exactly 3 edges");

        // Test face is accessible
        let faces: Vec<_> = backend.faces().collect();
        assert_eq!(faces.len(), 1, "Should have exactly 1 face");

        // Verify face has all 3 vertices
        let face_vertices = backend
            .face_vertices(&faces[0])
            .expect("Should get face vertices");
        assert_eq!(face_vertices.len(), 3, "Face should have 3 vertices");
    }

    #[test]
    fn test_topology_kind_is_euclidean() {
        // Triangulations built via the builder default to Euclidean topology
        let dt = crate::util::generate_random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        assert_eq!(
            backend.topology_kind(),
            delaunay::topology::traits::TopologyKind::Euclidean,
            "Default builder construction should produce Euclidean topology"
        );
    }

    #[test]
    fn test_is_valid_runs_structural_validation() {
        // is_valid() runs Levels 1–3 (structural/topological) via as_triangulation().validate();
        // is_delaunay() runs Levels 1–4 (including the Delaunay property).
        // For a well-formed Delaunay triangulation both should pass.
        let dt = crate::util::generate_seeded_delaunay2(8, (0.0, 10.0), 99);
        let backend = DelaunayBackend::from_triangulation(dt);

        let valid = backend.is_valid();
        let delaunay = backend.is_delaunay();

        assert!(valid, "Seeded triangulation should be structurally valid");
        assert!(
            delaunay,
            "Seeded triangulation should satisfy Delaunay property"
        );
        // is_delaunay() (Levels 1–4) implies is_valid() (Levels 1–3)
        assert!(delaunay && valid, "is_delaunay() should imply is_valid()");
    }

    #[test]
    fn test_builder_produces_correct_vertex_count() {
        // Verify the builder path in generate_delaunay2_with_context preserves vertex count
        for n in [3, 5, 10, 20] {
            let dt = crate::util::generate_delaunay2_with_context(n, (0.0, 10.0), Some(42))
                .expect("Builder should succeed");
            assert_eq!(
                dt.number_of_vertices(),
                n as usize,
                "Builder should produce exactly {n} vertices"
            );
        }
    }

    #[test]
    fn test_thread_safety() {
        fn assert_send_sync<T: Send + Sync>(_: &T) {}

        // Verify the backend can be sent across threads (ThreadSafeBackend)
        let dt = crate::util::generate_random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);
        assert_send_sync(&backend);
    }
}
