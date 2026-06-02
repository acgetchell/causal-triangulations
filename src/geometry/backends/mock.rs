#![forbid(unsafe_code)]

//! Mock geometry backend for testing.
//!
//! This backend provides a simple, controllable implementation for unit testing
//! CDT algorithms without requiring actual triangulation computations.

use crate::geometry::traits::{
    EdgeAdjacentFaces, EdgeAdjacentFacesResult, FlipResult, GeometryBackend, SubdivisionResult,
    TriangulationMut, TriangulationQuery,
};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroUsize;

/// Mock backend for testing
#[derive(Debug, Clone)]
pub struct MockBackend {
    vertices: HashMap<usize, Vec<f64>>,
    edges: HashMap<usize, (usize, usize)>,
    faces: HashMap<usize, Vec<usize>>,
    dimension: usize,
    next_vertex_id: usize,
    next_edge_id: usize,
    next_face_id: usize,
}

/// Mock vertex handle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MockVertexHandle(usize);

/// Mock edge handle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MockEdgeHandle(usize);

/// Mock face handle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MockFaceHandle(usize);

/// Mock backend operation category used in typed errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MockOperation {
    /// Insert a vertex.
    InsertVertex,
    /// Move an existing vertex.
    MoveVertex,
    /// Subdivide an existing face.
    SubdivideFace,
}

impl fmt::Display for MockOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsertVertex => formatter.write_str("insert_vertex"),
            Self::MoveVertex => formatter.write_str("move_vertex"),
            Self::SubdivideFace => formatter.write_str("subdivide_face"),
        }
    }
}

/// Mock backend storage target for reservation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MockStorageTarget {
    /// Vertex storage.
    Vertices,
    /// Face storage.
    Faces,
    /// Edge storage.
    Edges,
}

impl fmt::Display for MockStorageTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vertices => formatter.write_str("reserve_capacity(vertices)"),
            Self::Faces => formatter.write_str("reserve_capacity(faces)"),
            Self::Edges => formatter.write_str("reserve_capacity(edges)"),
        }
    }
}

/// Mock edge-flip precondition that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MockNonFlippableReason {
    /// The edge is not shared by exactly two faces.
    EdgeSharedByTwoFaces,
    /// One or both adjacent faces are not triangles.
    AdjacentFacesMustBeTriangles,
    /// An adjacent triangle does not contain an opposite vertex.
    MissingOppositeVertex,
    /// The two opposite vertices are the same.
    OppositeVerticesMustBeDistinct,
    /// The replacement edge already exists.
    ReplacementEdgeAlreadyExists,
}

impl fmt::Display for MockNonFlippableReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EdgeSharedByTwoFaces => {
                formatter.write_str("edge must be shared by exactly two faces")
            }
            Self::AdjacentFacesMustBeTriangles => {
                formatter.write_str("adjacent faces must be triangles")
            }
            Self::MissingOppositeVertex => {
                formatter.write_str("adjacent triangle is missing an opposite vertex")
            }
            Self::OppositeVerticesMustBeDistinct => {
                formatter.write_str("opposite vertices must be distinct")
            }
            Self::ReplacementEdgeAlreadyExists => {
                formatter.write_str("replacement edge already exists")
            }
        }
    }
}

/// Mock backend errors
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MockError {
    /// Invalid vertex handle provided
    #[error("Invalid vertex handle: {0}")]
    Vertex(usize),

    /// Invalid edge handle provided
    #[error("Invalid edge handle: {0}")]
    Edge(usize),

    /// Invalid face handle provided
    #[error("Invalid face handle: {0}")]
    Face(usize),

    /// Storage reservation failed.
    #[error("Reservation failed for {operation} requesting {requested_capacity} slots: {detail}")]
    ReservationFailed {
        /// Operation being attempted.
        operation: MockStorageTarget,
        /// Capacity requested for the targeted storage map.
        requested_capacity: usize,
        /// Allocation failure detail.
        detail: String,
    },

    /// Coordinate arity does not match the mock backend dimension.
    #[error("Invalid coordinate dimension for {operation}: got {actual}, expected {expected}")]
    InvalidCoordinateDimension {
        /// Mutation operation being attempted.
        operation: MockOperation,
        /// Expected coordinate arity.
        expected: usize,
        /// Actual coordinate arity supplied by the caller.
        actual: usize,
    },

    /// An edge exists, but its local topology cannot be flipped.
    #[error(
        "Edge {edge} cannot be flipped: found {adjacent_faces} adjacent faces, expected 2 ({reason})"
    )]
    NonFlippableEdge {
        /// Edge handle that was requested.
        edge: usize,
        /// Number of faces incident to the edge.
        adjacent_faces: usize,
        /// Additional reason the edge cannot be flipped.
        reason: MockNonFlippableReason,
    },

    /// A face exists, but its local topology cannot be subdivided.
    #[error("Face {face} cannot be subdivided: found {vertex_count} vertices, expected {expected}")]
    NonSubdividableFace {
        /// Face handle that was requested.
        face: usize,
        /// Number of vertices currently stored for the face.
        vertex_count: usize,
        /// Expected local topology.
        expected: &'static str,
    },
}

impl MockBackend {
    /// Create a new mock backend with a nonzero coordinate dimension.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use std::num::NonZeroUsize;
    ///
    /// let n = 2;
    /// let Some(dimension) = NonZeroUsize::new(n) else {
    ///     return;
    /// };
    /// let backend = MockBackend::new(dimension);
    /// assert_eq!(backend.dimension(), 2);
    /// assert_eq!(backend.vertex_count(), 0);
    /// ```
    #[must_use]
    pub fn new(dimension: NonZeroUsize) -> Self {
        Self {
            vertices: HashMap::new(),
            edges: HashMap::new(),
            faces: HashMap::new(),
            dimension: dimension.get(),
            next_vertex_id: 0,
            next_edge_id: 0,
            next_face_id: 0,
        }
    }

    /// Create an empty two-dimensional mock backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::new_2d();
    /// assert_eq!(backend.dimension(), 2);
    /// ```
    #[must_use]
    pub fn new_2d() -> Self {
        Self {
            vertices: HashMap::new(),
            edges: HashMap::new(),
            faces: HashMap::new(),
            dimension: 2,
            next_vertex_id: 0,
            next_edge_id: 0,
            next_face_id: 0,
        }
    }

    /// Create a simple triangle for testing
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// assert_eq!(backend.vertex_count(), 3);
    /// assert_eq!(backend.edge_count(), 3);
    /// assert_eq!(backend.face_count(), 1);
    /// ```
    #[must_use]
    pub fn create_triangle() -> Self {
        let mut backend = Self::new_2d();

        // Add three vertices
        backend.vertices.insert(0, vec![0.0, 0.0]);
        backend.vertices.insert(1, vec![1.0, 0.0]);
        backend.vertices.insert(2, vec![0.5, 1.0]);
        backend.next_vertex_id = 3;

        // Add three edges
        backend.edges.insert(0, (0, 1));
        backend.edges.insert(1, (1, 2));
        backend.edges.insert(2, (2, 0));
        backend.next_edge_id = 3;

        // Add one face
        backend.faces.insert(0, vec![0, 1, 2]);
        backend.next_face_id = 1;

        backend
    }

    /// Centralizes coordinate arity checks so mock mutations fail before state changes.
    const fn validate_coordinate_dimension(
        &self,
        operation: MockOperation,
        coords: &[f64],
    ) -> Result<(), MockError> {
        if coords.len() == self.dimension {
            return Ok(());
        }

        Err(MockError::InvalidCoordinateDimension {
            operation,
            expected: self.dimension,
            actual: coords.len(),
        })
    }

    /// Detects duplicate undirected edges before local topology mutations insert new edges.
    fn edge_exists_between(&self, v0: usize, v1: usize) -> bool {
        self.edges
            .values()
            .any(|&(a, b)| (a == v0 && b == v1) || (a == v1 && b == v0))
    }

    /// Resolves edge incidence from face storage so flip validation uses the same local topology it mutates.
    fn adjacent_face_ids_for_edge(&self, v0: usize, v1: usize) -> Vec<usize> {
        let mut face_ids: Vec<_> = self
            .faces
            .iter()
            .filter_map(|(&face_id, vertices)| {
                (vertices.contains(&v0) && vertices.contains(&v1)).then_some(face_id)
            })
            .collect();
        face_ids.sort_unstable();
        face_ids
    }

    /// Allocates edge IDs through one path so generated IDs stay monotonic after subdivisions.
    const fn next_missing_edge_id(&mut self) -> usize {
        let id = self.next_edge_id;
        self.next_edge_id += 1;
        id
    }

    /// Allocates face IDs through one path so generated IDs stay monotonic after subdivisions.
    const fn next_missing_face_id(&mut self) -> usize {
        let id = self.next_face_id;
        self.next_face_id += 1;
        id
    }

    /// Validates the exact local edge-flip preconditions before `flip_edge` mutates maps.
    fn edge_flip_opposites(
        &self,
        edge: usize,
        v0: usize,
        v1: usize,
        adjacent_faces: &[usize],
    ) -> Result<(usize, usize), MockError> {
        if adjacent_faces.len() != 2 {
            return Err(MockError::NonFlippableEdge {
                edge,
                adjacent_faces: adjacent_faces.len(),
                reason: MockNonFlippableReason::EdgeSharedByTwoFaces,
            });
        }

        let mut opposites = Vec::with_capacity(2);
        for face_id in adjacent_faces {
            let vertices = self.faces.get(face_id).ok_or(MockError::Face(*face_id))?;
            if vertices.len() != 3 {
                return Err(MockError::NonFlippableEdge {
                    edge,
                    adjacent_faces: adjacent_faces.len(),
                    reason: MockNonFlippableReason::AdjacentFacesMustBeTriangles,
                });
            }
            let opposite = vertices
                .iter()
                .copied()
                .find(|&vertex| vertex != v0 && vertex != v1)
                .ok_or(MockError::NonFlippableEdge {
                    edge,
                    adjacent_faces: adjacent_faces.len(),
                    reason: MockNonFlippableReason::MissingOppositeVertex,
                })?;
            opposites.push(opposite);
        }

        let first = opposites[0];
        let second = opposites[1];

        if first == second {
            return Err(MockError::NonFlippableEdge {
                edge,
                adjacent_faces: adjacent_faces.len(),
                reason: MockNonFlippableReason::OppositeVerticesMustBeDistinct,
            });
        }

        if self.edge_exists_between(first, second) {
            return Err(MockError::NonFlippableEdge {
                edge,
                adjacent_faces: adjacent_faces.len(),
                reason: MockNonFlippableReason::ReplacementEdgeAlreadyExists,
            });
        }

        Ok((first, second))
    }
}

impl GeometryBackend for MockBackend {
    type Coordinate = f64;
    type VertexHandle = MockVertexHandle;
    type EdgeHandle = MockEdgeHandle;
    type FaceHandle = MockFaceHandle;
    type Error = MockError;

    fn backend_name(&self) -> &'static str {
        "mock"
    }
}

impl TriangulationQuery for MockBackend {
    fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    fn edge_count(&self) -> usize {
        self.edges.len()
    }

    fn face_count(&self) -> usize {
        self.faces.len()
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn vertices(&self) -> impl Iterator<Item = Self::VertexHandle> + '_ {
        self.vertices.keys().map(|&id| MockVertexHandle(id))
    }

    fn edges(&self) -> impl Iterator<Item = Self::EdgeHandle> + '_ {
        self.edges.keys().map(|&id| MockEdgeHandle(id))
    }

    fn faces(&self) -> impl Iterator<Item = Self::FaceHandle> + '_ {
        self.faces.keys().map(|&id| MockFaceHandle(id))
    }

    fn vertex_coordinates(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::Coordinate>, Self::Error> {
        self.vertices
            .get(&vertex.0)
            .cloned()
            .ok_or(MockError::Vertex(vertex.0))
    }

    fn face_vertices(
        &self,
        face: &Self::FaceHandle,
    ) -> Result<Vec<Self::VertexHandle>, Self::Error> {
        self.faces
            .get(&face.0)
            .map(|indices| indices.iter().map(|&id| MockVertexHandle(id)).collect())
            .ok_or(MockError::Face(face.0))
    }

    fn edge_endpoints(
        &self,
        edge: &Self::EdgeHandle,
    ) -> Option<(Self::VertexHandle, Self::VertexHandle)> {
        self.edges
            .get(&edge.0)
            .map(|&(v1, v2)| (MockVertexHandle(v1), MockVertexHandle(v2)))
    }

    fn edge_adjacent_faces(
        &self,
        edge: &Self::EdgeHandle,
    ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error> {
        let (v0, v1) = *self.edges.get(&edge.0).ok_or(MockError::Edge(edge.0))?;
        let adjacent_faces = self.adjacent_face_ids_for_edge(v0, v1);
        let Ok((opposite_0, opposite_1)) =
            self.edge_flip_opposites(edge.0, v0, v1, &adjacent_faces)
        else {
            return Ok(None);
        };
        let [face_0, face_1] = adjacent_faces.as_slice() else {
            return Ok(None);
        };

        Ok(Some(EdgeAdjacentFaces::new(
            (MockVertexHandle(v0), MockVertexHandle(v1)),
            (MockFaceHandle(*face_0), MockFaceHandle(*face_1)),
            (MockVertexHandle(opposite_0), MockVertexHandle(opposite_1)),
        )))
    }

    fn adjacent_faces(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        if !self.vertices.contains_key(&vertex.0) {
            return Err(MockError::Vertex(vertex.0));
        }

        let mut faces: Vec<_> = self
            .faces
            .iter()
            .filter_map(|(&face_id, vertices)| {
                vertices
                    .contains(&vertex.0)
                    .then_some(MockFaceHandle(face_id))
            })
            .collect();
        faces.sort_unstable_by_key(|face| face.0);
        Ok(faces)
    }

    fn incident_edges(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::EdgeHandle>, Self::Error> {
        if !self.vertices.contains_key(&vertex.0) {
            return Err(MockError::Vertex(vertex.0));
        }

        let mut edges: Vec<_> = self
            .edges
            .iter()
            .filter_map(|(&edge_id, &(v0, v1))| {
                (v0 == vertex.0 || v1 == vertex.0).then_some(MockEdgeHandle(edge_id))
            })
            .collect();
        edges.sort_unstable_by_key(|edge| edge.0);
        Ok(edges)
    }

    fn face_neighbors(
        &self,
        face: &Self::FaceHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        if !self.faces.contains_key(&face.0) {
            return Err(MockError::Face(face.0));
        }

        let face_vertices = &self.faces[&face.0];
        let mut neighbors: Vec<_> = self
            .faces
            .iter()
            .filter_map(|(&neighbor_id, neighbor_vertices)| {
                if neighbor_id == face.0 {
                    return None;
                }

                let shared_vertices = face_vertices
                    .iter()
                    .filter(|vertex| neighbor_vertices.contains(vertex))
                    .count();
                (shared_vertices >= 2).then_some(MockFaceHandle(neighbor_id))
            })
            .collect();
        neighbors.sort_unstable_by_key(|neighbor| neighbor.0);
        Ok(neighbors)
    }

    fn is_valid(&self) -> bool {
        !self.vertices.is_empty() && !self.faces.is_empty()
    }
}

impl TriangulationMut for MockBackend {
    fn insert_vertex(
        &mut self,
        coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error> {
        self.validate_coordinate_dimension(MockOperation::InsertVertex, coords)?;
        let id = self.next_vertex_id;
        self.next_vertex_id += 1;
        self.vertices.insert(id, coords.to_vec());
        Ok(MockVertexHandle(id))
    }

    fn remove_vertex(
        &mut self,
        vertex: Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        self.vertices
            .remove(&vertex.0)
            .ok_or(MockError::Vertex(vertex.0))?;
        Ok(Vec::new())
    }

    fn move_vertex(
        &mut self,
        vertex: Self::VertexHandle,
        new_coords: &[Self::Coordinate],
    ) -> Result<(), Self::Error> {
        self.validate_coordinate_dimension(MockOperation::MoveVertex, new_coords)?;
        self.vertices
            .get_mut(&vertex.0)
            .map_or(Err(MockError::Vertex(vertex.0)), |coords| {
                *coords = new_coords.to_vec();
                Ok(())
            })
    }

    fn flip_edge(
        &mut self,
        edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::EdgeHandle, Self::FaceHandle>, Self::Error> {
        let (v0, v1) = *self.edges.get(&edge.0).ok_or(MockError::Edge(edge.0))?;
        let adjacent_faces = self.adjacent_face_ids_for_edge(v0, v1);
        let (opposite_0, opposite_1) = self.edge_flip_opposites(edge.0, v0, v1, &adjacent_faces)?;

        self.edges.insert(edge.0, (opposite_0, opposite_1));
        self.faces
            .insert(adjacent_faces[0], vec![opposite_0, opposite_1, v0]);
        self.faces
            .insert(adjacent_faces[1], vec![opposite_1, opposite_0, v1]);

        Ok(FlipResult::new(
            edge,
            adjacent_faces.into_iter().map(MockFaceHandle).collect(),
        ))
    }

    fn can_flip_edge(&self, edge: &Self::EdgeHandle) -> bool {
        let Some(&(v0, v1)) = self.edges.get(&edge.0) else {
            return false;
        };
        let adjacent_faces = self.adjacent_face_ids_for_edge(v0, v1);
        self.edge_flip_opposites(edge.0, v0, v1, &adjacent_faces)
            .is_ok()
    }

    fn subdivide_face(
        &mut self,
        face: Self::FaceHandle,
        point: &[Self::Coordinate],
    ) -> Result<SubdivisionResult<Self::VertexHandle, Self::FaceHandle>, Self::Error> {
        self.validate_coordinate_dimension(MockOperation::SubdivideFace, point)?;
        let vertices = self
            .faces
            .get(&face.0)
            .cloned()
            .ok_or(MockError::Face(face.0))?;
        if vertices.len() != 3 {
            return Err(MockError::NonSubdividableFace {
                face: face.0,
                vertex_count: vertices.len(),
                expected: "3 vertices",
            });
        }

        let new_vertex = self.insert_vertex(point)?;
        let new_vertex_id = new_vertex.0;
        for &old_vertex in &vertices {
            if !self.edge_exists_between(old_vertex, new_vertex_id) {
                let edge_id = self.next_missing_edge_id();
                self.edges.insert(edge_id, (old_vertex, new_vertex_id));
            }
        }

        self.faces.remove(&face.0);
        let new_faces = [
            vec![vertices[0], vertices[1], new_vertex_id],
            vec![vertices[1], vertices[2], new_vertex_id],
            vec![vertices[2], vertices[0], new_vertex_id],
        ]
        .into_iter()
        .map(|vertices| {
            let face_id = self.next_missing_face_id();
            self.faces.insert(face_id, vertices);
            MockFaceHandle(face_id)
        })
        .collect();

        Ok(SubdivisionResult::new(new_vertex, new_faces, face))
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.vertices.clear();
        self.edges.clear();
        self.faces.clear();
        self.next_vertex_id = 0;
        self.next_edge_id = 0;
        self.next_face_id = 0;
        Ok(())
    }

    fn reserve_capacity(&mut self, vertices: usize, faces: usize) -> Result<(), Self::Error> {
        self.vertices
            .try_reserve(vertices)
            .map_err(|err| MockError::ReservationFailed {
                operation: MockStorageTarget::Vertices,
                requested_capacity: vertices,
                detail: err.to_string(),
            })?;
        self.faces
            .try_reserve(faces)
            .map_err(|err| MockError::ReservationFailed {
                operation: MockStorageTarget::Faces,
                requested_capacity: faces,
                detail: err.to_string(),
            })?;
        let edges = vertices.saturating_add(faces);
        self.edges
            .try_reserve(edges)
            .map_err(|err| MockError::ReservationFailed {
                operation: MockStorageTarget::Edges,
                requested_capacity: edges,
                detail: err.to_string(),
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[test]
    fn mock_operation_display_covers_all_operations() {
        assert_eq!(MockOperation::InsertVertex.to_string(), "insert_vertex");
        assert_eq!(MockOperation::MoveVertex.to_string(), "move_vertex");
        assert_eq!(MockOperation::SubdivideFace.to_string(), "subdivide_face");
    }

    #[test]
    fn mock_storage_target_display_covers_all_targets() {
        assert_eq!(
            MockStorageTarget::Vertices.to_string(),
            "reserve_capacity(vertices)"
        );
        assert_eq!(
            MockStorageTarget::Faces.to_string(),
            "reserve_capacity(faces)"
        );
        assert_eq!(
            MockStorageTarget::Edges.to_string(),
            "reserve_capacity(edges)"
        );
    }

    #[test]
    fn mock_non_flippable_reason_display_covers_all_reasons() {
        let cases = [
            (
                MockNonFlippableReason::EdgeSharedByTwoFaces,
                "edge must be shared by exactly two faces",
            ),
            (
                MockNonFlippableReason::AdjacentFacesMustBeTriangles,
                "adjacent faces must be triangles",
            ),
            (
                MockNonFlippableReason::MissingOppositeVertex,
                "adjacent triangle is missing an opposite vertex",
            ),
            (
                MockNonFlippableReason::OppositeVerticesMustBeDistinct,
                "opposite vertices must be distinct",
            ),
            (
                MockNonFlippableReason::ReplacementEdgeAlreadyExists,
                "replacement edge already exists",
            ),
        ];

        for (reason, expected) in cases {
            assert_eq!(reason.to_string(), expected);
        }
    }

    #[test]
    fn test_mock_backend_creation() {
        let backend = MockBackend::new_2d();
        assert_eq!(backend.backend_name(), "mock");
        assert_eq!(backend.dimension(), 2);
        assert_eq!(backend.vertex_count(), 0);
        assert_eq!(backend.edge_count(), 0);
        assert_eq!(backend.face_count(), 0);
        assert!(!backend.is_valid());
    }

    #[test]
    fn test_mock_triangle() {
        let backend = MockBackend::create_triangle();
        assert_eq!(backend.vertex_count(), 3);
        assert_eq!(backend.edge_count(), 3);
        assert_eq!(backend.face_count(), 1);
        assert!(backend.is_valid());
    }

    #[test]
    fn test_mock_triangle_queries_return_expected_entities() {
        let backend = MockBackend::create_triangle();
        let vertex = MockVertexHandle(0);
        let edge = MockEdgeHandle(0);
        let face = MockFaceHandle(0);

        assert_eq!(
            backend
                .vertex_coordinates(&vertex)
                .expect("valid vertex")
                .len(),
            2
        );
        assert_eq!(backend.face_vertices(&face).expect("valid face").len(), 3);
        assert!(backend.edge_endpoints(&edge).is_some());
        assert_eq!(
            backend
                .adjacent_faces(&vertex)
                .expect("valid vertex adjacency"),
            vec![face.clone()]
        );
        assert_eq!(
            backend
                .incident_edges(&vertex)
                .expect("valid vertex incidence"),
            vec![MockEdgeHandle(0), MockEdgeHandle(2)]
        );
        assert!(
            backend
                .face_neighbors(&face)
                .expect("valid face neighbors")
                .is_empty()
        );
        assert_eq!(backend.euler_characteristic(), 1);
    }

    #[test]
    fn test_mock_backend_rejects_invalid_handles() {
        let mut backend = MockBackend::create_triangle();
        let missing_vertex = MockVertexHandle(99);
        let missing_edge = MockEdgeHandle(99);
        let missing_face = MockFaceHandle(99);

        assert_matches!(
            backend.vertex_coordinates(&missing_vertex),
            Err(MockError::Vertex(99))
        );
        assert_matches!(
            backend.face_vertices(&missing_face),
            Err(MockError::Face(99))
        );
        assert!(backend.edge_endpoints(&missing_edge).is_none());
        assert_matches!(
            backend.adjacent_faces(&missing_vertex),
            Err(MockError::Vertex(99))
        );
        assert_matches!(
            backend.incident_edges(&missing_vertex),
            Err(MockError::Vertex(99))
        );
        assert_matches!(
            backend.face_neighbors(&missing_face),
            Err(MockError::Face(99))
        );
        assert_matches!(
            backend.remove_vertex(missing_vertex.clone()),
            Err(MockError::Vertex(99))
        );
        assert_matches!(
            backend.move_vertex(missing_vertex, &[1.0, 2.0]),
            Err(MockError::Vertex(99))
        );
        assert_matches!(backend.flip_edge(missing_edge), Err(MockError::Edge(99)));
        assert_matches!(
            backend.subdivide_face(missing_face, &[0.25, 0.25]),
            Err(MockError::Face(99))
        );
    }

    #[test]
    fn test_mock_backend_rejects_invalid_coordinate_dimensions() {
        let mut backend = MockBackend::create_triangle();

        assert_matches!(
            backend.insert_vertex(&[1.0]),
            Err(MockError::InvalidCoordinateDimension {
                operation: MockOperation::InsertVertex,
                expected: 2,
                actual: 1,
            })
        );
        assert_matches!(
            backend.move_vertex(MockVertexHandle(0), &[1.0, 2.0, 3.0]),
            Err(MockError::InvalidCoordinateDimension {
                operation: MockOperation::MoveVertex,
                expected: 2,
                actual: 3,
            })
        );
        assert_matches!(
            backend.subdivide_face(MockFaceHandle(0), &[0.25]),
            Err(MockError::InvalidCoordinateDimension {
                operation: MockOperation::SubdivideFace,
                expected: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn test_mock_backend_basic_mutations_update_state() {
        let mut backend = MockBackend::create_triangle();
        backend
            .reserve_capacity(8, 4)
            .expect("mock backend should reserve storage");

        let vertex = backend
            .insert_vertex(&[2.0, 3.0])
            .expect("mock vertex insertion should succeed");
        assert_eq!(
            backend
                .vertex_coordinates(&vertex)
                .expect("inserted vertex should be queryable"),
            vec![2.0, 3.0]
        );

        backend
            .move_vertex(vertex.clone(), &[4.0, 5.0])
            .expect("mock vertex move should succeed");
        assert_eq!(
            backend
                .vertex_coordinates(&vertex)
                .expect("moved vertex should be queryable"),
            vec![4.0, 5.0]
        );

        let edge = backend
            .edges()
            .next()
            .expect("triangle should contain edges");
        assert_matches!(
            backend.flip_edge(edge.clone()),
            Err(MockError::NonFlippableEdge {
                edge: _,
                adjacent_faces: 1,
                reason: MockNonFlippableReason::EdgeSharedByTwoFaces,
            })
        );
        assert!(!backend.can_flip_edge(&edge));
        assert!(!backend.can_flip_edge(&MockEdgeHandle(99)));

        let face = backend
            .faces()
            .next()
            .expect("triangle should contain a face");
        let subdivision = backend
            .subdivide_face(face.clone(), &[0.25, 0.25])
            .expect("mock face subdivision should succeed");
        assert_eq!(subdivision.removed_face, face);
        assert_eq!(subdivision.new_faces.len(), 3);
        assert_eq!(
            backend
                .vertex_coordinates(&subdivision.new_vertex)
                .expect("subdivision vertex should be queryable"),
            vec![0.25, 0.25]
        );
        assert_eq!(backend.vertex_count(), 5);
        assert_eq!(backend.face_count(), 3);

        let removed_faces = backend
            .remove_vertex(vertex)
            .expect("mock vertex removal should succeed");
        assert!(removed_faces.is_empty());

        backend.clear().expect("mock backend should clear storage");
        assert_eq!(backend.vertex_count(), 0);
        assert_eq!(backend.edge_count(), 0);
        assert_eq!(backend.face_count(), 0);
        assert!(!backend.is_valid());
        assert_eq!(backend.vertices().count(), 0);
        assert_eq!(backend.edges().count(), 0);
        assert_eq!(backend.faces().count(), 0);
    }

    #[test]
    fn test_mock_backend_reservation_failure_reports_requested_capacity() {
        let mut backend = MockBackend::create_triangle();

        let result = backend.reserve_capacity(usize::MAX, 0);

        assert_matches!(
            result,
            Err(MockError::ReservationFailed {
                operation: MockStorageTarget::Vertices,
                requested_capacity: usize::MAX,
                detail,
            }) if !detail.is_empty()
        );
    }

    #[test]
    fn test_mock_backend_flips_interior_edge() {
        let mut backend = MockBackend::new_2d();
        backend.vertices.insert(0, vec![0.0, 0.0]);
        backend.vertices.insert(1, vec![1.0, 0.0]);
        backend.vertices.insert(2, vec![1.0, 1.0]);
        backend.vertices.insert(3, vec![0.0, 1.0]);
        backend.next_vertex_id = 4;
        backend.edges.insert(0, (0, 1));
        backend.edges.insert(1, (1, 2));
        backend.edges.insert(2, (2, 3));
        backend.edges.insert(3, (3, 0));
        backend.edges.insert(4, (0, 2));
        backend.next_edge_id = 5;
        backend.faces.insert(0, vec![0, 1, 2]);
        backend.faces.insert(1, vec![0, 2, 3]);
        backend.next_face_id = 2;

        let diagonal = MockEdgeHandle(4);
        assert!(backend.can_flip_edge(&diagonal));

        let flip = backend
            .flip_edge(diagonal.clone())
            .expect("interior diagonal should be flippable");

        assert_eq!(flip.new_edge, diagonal);
        assert_eq!(flip.affected_faces.len(), 2);
        assert_eq!(
            backend.edge_endpoints(&flip.new_edge),
            Some((MockVertexHandle(1), MockVertexHandle(3)))
        );
        assert_eq!(
            backend
                .face_vertices(&MockFaceHandle(0))
                .expect("updated face"),
            vec![
                MockVertexHandle(1),
                MockVertexHandle(3),
                MockVertexHandle(0),
            ]
        );
        assert_eq!(
            backend
                .face_vertices(&MockFaceHandle(1))
                .expect("updated face"),
            vec![
                MockVertexHandle(3),
                MockVertexHandle(1),
                MockVertexHandle(2),
            ]
        );
    }

    #[test]
    fn test_mock_backend_face_neighbors_return_shared_edge_faces() {
        let mut backend = MockBackend::new_2d();
        backend.vertices.insert(0, vec![0.0, 0.0]);
        backend.vertices.insert(1, vec![1.0, 0.0]);
        backend.vertices.insert(2, vec![1.0, 1.0]);
        backend.vertices.insert(3, vec![0.0, 1.0]);
        backend.next_vertex_id = 4;
        backend.faces.insert(0, vec![0, 1, 2]);
        backend.faces.insert(1, vec![0, 2, 3]);
        backend.next_face_id = 2;

        assert_eq!(
            backend
                .face_neighbors(&MockFaceHandle(0))
                .expect("face 0 should be valid"),
            vec![MockFaceHandle(1)]
        );
        assert_eq!(
            backend
                .face_neighbors(&MockFaceHandle(1))
                .expect("face 1 should be valid"),
            vec![MockFaceHandle(0)]
        );
    }

    #[test]
    fn test_mock_backend_rejects_malformed_edge_flips() {
        let mut backend = MockBackend::new_2d();
        backend.vertices.insert(0, vec![0.0, 0.0]);
        backend.vertices.insert(1, vec![1.0, 0.0]);
        backend.vertices.insert(2, vec![1.0, 1.0]);
        backend.vertices.insert(3, vec![0.0, 1.0]);
        backend.next_vertex_id = 4;

        backend.edges.insert(0, (0, 1));
        backend.faces.insert(0, vec![0, 1, 2, 3]);
        backend.faces.insert(1, vec![0, 1, 2]);
        assert_matches!(
            backend.flip_edge(MockEdgeHandle(0)),
            Err(MockError::NonFlippableEdge {
                edge: 0,
                adjacent_faces: 2,
                reason: MockNonFlippableReason::AdjacentFacesMustBeTriangles,
            })
        );

        backend.faces.insert(0, vec![0, 1, 2]);
        backend.faces.insert(1, vec![1, 0, 2]);
        assert_matches!(
            backend.flip_edge(MockEdgeHandle(0)),
            Err(MockError::NonFlippableEdge {
                edge: 0,
                adjacent_faces: 2,
                reason: MockNonFlippableReason::OppositeVerticesMustBeDistinct,
            })
        );

        backend.faces.insert(1, vec![1, 0, 3]);
        backend.edges.insert(1, (2, 3));
        assert_matches!(
            backend.flip_edge(MockEdgeHandle(0)),
            Err(MockError::NonFlippableEdge {
                edge: 0,
                adjacent_faces: 2,
                reason: MockNonFlippableReason::ReplacementEdgeAlreadyExists,
            })
        );
    }

    #[test]
    fn test_mock_backend_rejects_non_triangular_subdivision() {
        let mut backend = MockBackend::new_2d();
        backend.vertices.insert(0, vec![0.0, 0.0]);
        backend.vertices.insert(1, vec![1.0, 0.0]);
        backend.vertices.insert(2, vec![1.0, 1.0]);
        backend.vertices.insert(3, vec![0.0, 1.0]);
        backend.next_vertex_id = 4;
        backend.faces.insert(0, vec![0, 1, 2, 3]);
        backend.next_face_id = 1;

        assert_matches!(
            backend.subdivide_face(MockFaceHandle(0), &[0.5, 0.5]),
            Err(MockError::NonSubdividableFace {
                face: 0,
                vertex_count: 4,
                expected: "3 vertices",
            })
        );
        assert_eq!(backend.vertex_count(), 4);
        assert_eq!(backend.face_count(), 1);
    }
}
