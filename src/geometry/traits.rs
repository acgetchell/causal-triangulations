//! Core geometry traits for CDT abstraction.
//!
//! This module defines the trait-based interface that completely isolates
//! CDT algorithms from specific geometry implementations.

use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

/// Core numeric trait for coordinates in geometric calculations.
///
/// `num_traits::Float` already implies `Copy`, `Clone`, `PartialEq`, and `PartialOrd`.
pub trait CoordinateScalar: Debug + 'static + num_traits::Float {}

impl<T> CoordinateScalar for T where T: Debug + 'static + num_traits::Float {}

/// Handle types for geometry entities - completely opaque to prevent coupling
pub trait GeometryHandle: Clone + Eq + Hash + Debug {}

// Blanket implementation for any type satisfying the constraints
impl<T> GeometryHandle for T where T: Clone + Eq + Hash + Debug {}

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
    type Error: std::error::Error + 'static;

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

    /// Iterate over all vertices in the triangulation
    fn vertices(&self) -> Box<dyn Iterator<Item = Self::VertexHandle> + '_>;

    /// Iterate over all edges in the triangulation
    fn edges(&self) -> Box<dyn Iterator<Item = Self::EdgeHandle> + '_>;

    /// Iterate over all faces in the triangulation
    fn faces(&self) -> Box<dyn Iterator<Item = Self::FaceHandle> + '_>;

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
        let v = crate::util::saturating_usize_to_i32(self.vertex_count());
        let e = crate::util::saturating_usize_to_i32(self.edge_count());
        let f = crate::util::saturating_usize_to_i32(self.face_count());
        v - e + f
    }
}

/// Results from edge flip operations
#[derive(Debug, Clone)]
pub struct FlipResult<V, E, F> {
    /// The new edge created by the flip
    pub new_edge: E,
    /// Faces affected by the flip operation
    pub affected_faces: Vec<F>,
    _phantom: PhantomData<V>,
}

impl<V, E, F> FlipResult<V, E, F> {
    /// Create a new flip result
    pub const fn new(new_edge: E, affected_faces: Vec<F>) -> Self {
        Self {
            new_edge,
            affected_faces,
            _phantom: PhantomData,
        }
    }
}

/// Results from face subdivision operations
#[derive(Debug, Clone)]
pub struct SubdivisionResult<V, E, F> {
    /// The new vertex created at the subdivision point
    pub new_vertex: V,
    /// New faces created by subdividing the original face
    pub new_faces: Vec<F>,
    /// The face that was subdivided (now removed)
    pub removed_face: F,
    _phantom: PhantomData<E>,
}

impl<V, E, F> SubdivisionResult<V, E, F> {
    /// Create a new subdivision result
    pub const fn new(new_vertex: V, new_faces: Vec<F>, removed_face: F) -> Self {
        Self {
            new_vertex,
            new_faces,
            removed_face,
            _phantom: PhantomData,
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
    #[allow(clippy::type_complexity)]
    fn flip_edge(
        &mut self,
        edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::VertexHandle, Self::EdgeHandle, Self::FaceHandle>, Self::Error>;

    /// Check if an edge can be flipped
    fn can_flip_edge(&self, edge: &Self::EdgeHandle) -> bool;

    /// Subdivide a face by adding a vertex at the given point
    ///
    /// # Errors
    /// Returns error if the face cannot be subdivided
    #[allow(clippy::type_complexity)]
    fn subdivide_face(
        &mut self,
        face: Self::FaceHandle,
        point: &[Self::Coordinate],
    ) -> Result<
        SubdivisionResult<Self::VertexHandle, Self::EdgeHandle, Self::FaceHandle>,
        Self::Error,
    >;

    /// Clear all elements from the triangulation
    fn clear(&mut self);

    /// Reserve capacity for vertices and faces
    fn reserve_capacity(&mut self, vertices: usize, faces: usize);
}
