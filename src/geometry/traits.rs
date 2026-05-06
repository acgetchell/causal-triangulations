#![forbid(unsafe_code)]

//! Core geometry traits for CDT abstraction.
//!
//! This module defines the trait-based interface that completely isolates
//! CDT algorithms from specific geometry implementations.

use crate::util::saturating_usize_to_i32;
use num_traits::Float;
use std::error::Error as StdError;
use std::fmt::Debug;
use std::hash::Hash;

/// Core numeric trait for coordinates in geometric calculations.
///
/// `num_traits::Float` already implies `Copy`, `Clone`, `PartialEq`, and `PartialOrd`.
pub trait CoordinateScalar: Debug + Float {}

impl<T: Debug + Float> CoordinateScalar for T {}

/// Handle types for geometry entities - completely opaque to prevent coupling
pub trait GeometryHandle: Clone + Eq + Hash + Debug {}

// Blanket implementation for any type satisfying the constraints
impl<T: Clone + Eq + Hash + Debug> GeometryHandle for T {}

/// Core geometry backend trait - completely abstracted from implementation details.
pub trait GeometryBackend {
    /// Coordinate type used by this backend
    type Coordinate: CoordinateScalar;
    /// Opaque handle type for vertices
    type VertexHandle: GeometryHandle;
    /// Opaque handle type for edges
    type EdgeHandle: GeometryHandle;
    /// Opaque handle type for faces
    type FaceHandle: GeometryHandle;
    /// Error type for backend operations
    type Error: StdError;

    /// Backend identifier for debugging
    fn backend_name(&self) -> &'static str;
}

/// Read-only triangulation operations
pub trait TriangulationQuery: GeometryBackend {
    /// Get the number of vertices in the triangulation
    fn vertex_count(&self) -> usize;

    /// Get the number of edges in the triangulation
    fn edge_count(&self) -> usize;

    /// Get the number of faces in the triangulation
    fn face_count(&self) -> usize;

    /// Get the dimensionality of the triangulation
    fn dimension(&self) -> usize;

    /// Iterate over all vertices in the triangulation.
    ///
    /// Implementations return a concrete iterator so repeated topology scans do
    /// not require heap allocation or dynamic dispatch.
    fn vertices(&self) -> impl Iterator<Item = Self::VertexHandle> + '_;

    /// Iterate over all edges in the triangulation.
    ///
    /// Implementations return a concrete iterator so repeated topology scans do
    /// not require heap allocation or dynamic dispatch.
    fn edges(&self) -> impl Iterator<Item = Self::EdgeHandle> + '_;

    /// Iterate over all faces in the triangulation.
    ///
    /// Implementations return a concrete iterator so repeated topology scans do
    /// not require heap allocation or dynamic dispatch.
    fn faces(&self) -> impl Iterator<Item = Self::FaceHandle> + '_;

    /// Get the coordinates of a vertex
    ///
    /// # Errors
    /// Returns error if the vertex handle is invalid
    fn vertex_coordinates(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::Coordinate>, Self::Error>;

    /// Get the vertices that form a face
    ///
    /// # Errors
    /// Returns error if the face handle is invalid
    fn face_vertices(
        &self,
        face: &Self::FaceHandle,
    ) -> Result<Vec<Self::VertexHandle>, Self::Error>;

    /// Get the two vertices that form an edge.
    ///
    /// Returns `Some((v0, v1))` when endpoint resolution succeeds, or `None`
    /// if the edge handle is invalid or endpoints are unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// let edge = backend.edges().next().expect("triangle should have an edge");
    /// let endpoints = backend.edge_endpoints(&edge);
    /// assert!(endpoints.is_some());
    /// ```
    fn edge_endpoints(
        &self,
        edge: &Self::EdgeHandle,
    ) -> Option<(Self::VertexHandle, Self::VertexHandle)>;

    /// Get the two faces adjacent to an edge and their opposite vertices.
    ///
    /// Returns `Ok(Some(_))` for an interior edge shared by exactly two
    /// triangular faces, `Ok(None)` for boundary or otherwise non-flippable
    /// local topology, and `Err` when the edge handle itself is invalid.
    ///
    /// # Errors
    /// Returns error if the edge handle is invalid.
    fn edge_adjacent_faces(
        &self,
        edge: &Self::EdgeHandle,
    ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error>;

    /// Get all faces adjacent to a vertex
    ///
    /// # Errors
    /// Returns error if the vertex handle is invalid
    fn adjacent_faces(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error>;

    /// Get all edges incident to a vertex
    ///
    /// # Errors
    /// Returns error if the vertex handle is invalid
    fn incident_edges(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::EdgeHandle>, Self::Error>;

    /// Get all faces neighboring a given face
    ///
    /// # Errors
    /// Returns error if the face handle is invalid
    fn face_neighbors(&self, face: &Self::FaceHandle)
    -> Result<Vec<Self::FaceHandle>, Self::Error>;

    /// Check if the triangulation is valid
    fn is_valid(&self) -> bool;

    /// Calculate the Euler characteristic (V - E + F)
    fn euler_characteristic(&self) -> i32 {
        let v = saturating_usize_to_i32(self.vertex_count());
        let e = saturating_usize_to_i32(self.edge_count());
        let f = saturating_usize_to_i32(self.face_count());
        v.saturating_sub(e).saturating_add(f)
    }
}

/// Results from edge flip operations
#[derive(Debug, Clone)]
pub struct FlipResult<E, F> {
    /// The new edge created by the flip
    pub new_edge: E,
    /// Faces affected by the flip operation
    pub affected_faces: Vec<F>,
}

impl<E, F> FlipResult<E, F> {
    /// Create a new flip result
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::traits::FlipResult;
    ///
    /// let result = FlipResult::new("new edge", vec!["left face", "right face"]);
    /// assert_eq!(result.new_edge, "new edge");
    /// assert_eq!(result.affected_faces.len(), 2);
    /// ```
    pub const fn new(new_edge: E, affected_faces: Vec<F>) -> Self {
        Self {
            new_edge,
            affected_faces,
        }
    }
}

/// Local topology adjacent to one edge in a 2D triangulation.
#[derive(Debug, Clone)]
pub struct EdgeAdjacentFaces<V, F> {
    /// The two endpoints of the queried edge.
    pub endpoints: (V, V),
    /// The two faces sharing the edge.
    pub faces: (F, F),
    /// The vertex opposite the edge in each adjacent face.
    pub opposite_vertices: (V, V),
}

/// Result type for querying the local faces adjacent to an edge.
pub type EdgeAdjacentFacesResult<V, F, E> = Result<Option<EdgeAdjacentFaces<V, F>>, E>;

impl<V, F> EdgeAdjacentFaces<V, F> {
    /// Create a local edge-adjacency record.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::traits::EdgeAdjacentFaces;
    ///
    /// let adjacency = EdgeAdjacentFaces::new(("a", "b"), ("left", "right"), ("c", "d"));
    /// assert_eq!(adjacency.endpoints, ("a", "b"));
    /// ```
    pub const fn new(endpoints: (V, V), faces: (F, F), opposite_vertices: (V, V)) -> Self {
        Self {
            endpoints,
            faces,
            opposite_vertices,
        }
    }
}

/// Results from face subdivision operations
#[derive(Debug, Clone)]
pub struct SubdivisionResult<V, F> {
    /// The new vertex created at the subdivision point
    pub new_vertex: V,
    /// New faces created by subdividing the original face
    pub new_faces: Vec<F>,
    /// The face that was subdivided (now removed)
    pub removed_face: F,
}

impl<V, F> SubdivisionResult<V, F> {
    /// Create a new subdivision result
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::traits::SubdivisionResult;
    ///
    /// let result = SubdivisionResult::new("new vertex", vec!["f0", "f1", "f2"], "old face");
    /// assert_eq!(result.new_vertex, "new vertex");
    /// assert_eq!(result.removed_face, "old face");
    /// ```
    pub const fn new(new_vertex: V, new_faces: Vec<F>, removed_face: F) -> Self {
        Self {
            new_vertex,
            new_faces,
            removed_face,
        }
    }
}

/// Mutable triangulation operations
pub trait TriangulationMut: TriangulationQuery {
    /// Insert a new vertex at the given coordinates
    ///
    /// # Errors
    /// Returns error if the vertex cannot be inserted
    fn insert_vertex(
        &mut self,
        coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error>;

    /// Remove a vertex from the triangulation
    ///
    /// # Errors
    /// Returns error if the vertex cannot be removed
    fn remove_vertex(
        &mut self,
        vertex: Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error>;

    /// Move a vertex to new coordinates
    ///
    /// # Errors
    /// Returns error if the vertex cannot be moved
    fn move_vertex(
        &mut self,
        vertex: Self::VertexHandle,
        new_coords: &[Self::Coordinate],
    ) -> Result<(), Self::Error>;

    /// Flip an edge in the triangulation
    ///
    /// # Errors
    /// Returns error if the edge cannot be flipped
    fn flip_edge(
        &mut self,
        edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::EdgeHandle, Self::FaceHandle>, Self::Error>;

    /// Check if an edge can be flipped
    fn can_flip_edge(&self, edge: &Self::EdgeHandle) -> bool;

    /// Subdivide a face by adding a vertex at the given point
    ///
    /// # Errors
    /// Returns error if the face cannot be subdivided
    fn subdivide_face(
        &mut self,
        face: Self::FaceHandle,
        point: &[Self::Coordinate],
    ) -> Result<SubdivisionResult<Self::VertexHandle, Self::FaceHandle>, Self::Error>;

    /// Remove every vertex, edge, and face from the triangulation.
    ///
    /// A successful call leaves the backend in an empty, reusable state. Backends
    /// whose storage model cannot be cleared in place should report that through
    /// their associated error type instead of silently leaving geometry behind.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot clear its current storage or
    /// when the operation is unsupported by that backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::{
    ///     MockBackend, MockError, TriangulationMut, TriangulationQuery,
    /// };
    ///
    /// fn main() -> Result<(), MockError> {
    ///     let mut backend = MockBackend::create_triangle();
    ///     assert!(backend.vertex_count() > 0);
    ///
    ///     backend.clear()?;
    ///
    ///     assert_eq!(backend.vertex_count(), 0);
    ///     assert_eq!(backend.edge_count(), 0);
    ///     assert_eq!(backend.face_count(), 0);
    ///     Ok(())
    /// }
    /// ```
    fn clear(&mut self) -> Result<(), Self::Error>;

    /// Reserve storage capacity for at least `vertices` vertices and `faces` faces.
    ///
    /// Implementations may reserve additional derived storage, such as edge or
    /// adjacency buffers. Backends that cannot expose a meaningful reservation
    /// operation should report that through their associated error type instead
    /// of treating the request as a successful no-op.
    ///
    /// # Errors
    ///
    /// Returns an error when allocation fails, the backend cannot honor the
    /// requested capacity, or the operation is unsupported by that backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::{
    ///     MockBackend, MockError, TriangulationMut, TriangulationQuery,
    /// };
    ///
    /// fn main() -> Result<(), MockError> {
    ///     let mut backend = MockBackend::create_triangle();
    ///     let counts = (backend.vertex_count(), backend.edge_count(), backend.face_count());
    ///
    ///     backend.reserve_capacity(16, 8)?;
    ///
    ///     assert_eq!(counts, (backend.vertex_count(), backend.edge_count(), backend.face_count()));
    ///     Ok(())
    /// }
    /// ```
    fn reserve_capacity(&mut self, vertices: usize, faces: usize) -> Result<(), Self::Error>;
}
