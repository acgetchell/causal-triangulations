#![forbid(unsafe_code)]

//! Core geometry traits for CDT abstraction.
//!
//! This module defines the trait-based interface that completely isolates
//! CDT algorithms from specific geometry implementations.

use std::error::Error as StdError;

/// Core geometry backend trait - completely abstracted from implementation details.
///
/// Coordinate and handle types are deliberately unconstrained here. Read-only
/// count and topology consumers do not need numeric coordinates or cloneable,
/// hashable handles; algorithms add those capabilities only where they use them.
pub trait GeometryBackend {
    /// Coordinate type used by this backend.
    type Coordinate;
    /// Opaque handle type for vertices.
    type VertexHandle;
    /// Opaque handle type for edges.
    type EdgeHandle;
    /// Opaque handle type for faces.
    type FaceHandle;
    /// Error type for backend operations
    type Error: StdError;

    /// Backend identifier for debugging
    fn backend_name(&self) -> &'static str;
}

/// Read-only queries over a geometry backend's triangulation.
///
/// Generic CDT and analysis code can depend on this trait to inspect counts,
/// connectivity, and coordinates without naming a concrete backend.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::testing::*;
///
/// fn simplex_counts(backend: &impl TriangulationQuery) -> (usize, usize, usize) {
///     (
///         backend.vertex_count(),
///         backend.edge_count(),
///         backend.face_count(),
///     )
/// }
///
/// let backend = MockBackend::create_triangle();
/// assert_eq!(simplex_counts(&backend), (3, 3, 1));
/// ```
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

    /// Returns the coordinate vector for a vertex handle.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the vertex handle is invalid or when the
    /// backend cannot resolve the coordinate payload for that vertex.
    fn vertex_coordinates(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::Coordinate>, Self::Error>;

    /// Returns the vertices that form a face.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the face handle is invalid or when the
    /// backend cannot resolve the face connectivity.
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
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// assert!(backend
    ///     .edges()
    ///     .next()
    ///     .is_some_and(|edge| backend.edge_endpoints(&edge).is_some()));
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
    ///
    /// Returns the backend error when the edge handle is invalid. Boundary edges
    /// and non-flippable local topology should return `Ok(None)`.
    fn edge_adjacent_faces(
        &self,
        edge: &Self::EdgeHandle,
    ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error>;

    /// Get all faces adjacent to a vertex.
    ///
    /// Backend implementations may build an adjacency index for this query.
    /// The Delaunay backend caches that index between topology mutations, so the
    /// first query after a mutation is `O(F)` in the number of finite faces and
    /// later queries reuse the same snapshot.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the vertex handle is invalid or adjacency
    /// information cannot be resolved.
    fn adjacent_faces(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error>;

    /// Get all edges incident to a vertex
    ///
    /// # Errors
    ///
    /// Returns the backend error when the vertex handle is invalid or incident
    /// edge information cannot be resolved.
    fn incident_edges(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::EdgeHandle>, Self::Error>;

    /// Get all faces neighboring a given face
    ///
    /// # Errors
    ///
    /// Returns the backend error when the face handle is invalid or neighboring
    /// face information cannot be resolved.
    fn face_neighbors(&self, face: &Self::FaceHandle)
    -> Result<Vec<Self::FaceHandle>, Self::Error>;

    /// Check if the triangulation is valid
    fn is_valid(&self) -> bool;

    /// Calculates the Euler characteristic `V - E + F`.
    ///
    /// Counts are widened before arithmetic so large triangulations cannot
    /// silently clamp the topological invariant used by CDT validation.
    fn euler_characteristic(&self) -> i128 {
        let vertices = self.vertex_count() as i128;
        let edges = self.edge_count() as i128;
        let faces = self.face_count() as i128;
        vertices - edges + faces
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
    /// use causal_triangulations::prelude::geometry::FlipResult;
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
    /// use causal_triangulations::prelude::geometry::EdgeAdjacentFaces;
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
    /// use causal_triangulations::prelude::geometry::SubdivisionResult;
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

/// Topology and coordinate mutations supported by a geometry backend.
///
/// Implementations return backend-specific errors and preserve their structural
/// invariants across successful mutations.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::testing::*;
///
/// fn insert_origin<B>(backend: &mut B) -> Result<B::VertexHandle, B::Error>
/// where
///     B: TriangulationMut<Coordinate = f64>,
/// {
///     backend.reserve_capacity(1, 0)?;
///     backend.insert_vertex(&[0.0, 0.0])
/// }
///
/// let mut backend = MockBackend::new_2d();
/// let _vertex = insert_origin(&mut backend)?;
/// assert_eq!(backend.vertex_count(), 1);
/// # Ok::<(), MockError>(())
/// ```
pub trait TriangulationMut: TriangulationQuery {
    /// Insert a new vertex at the given coordinates
    ///
    /// # Errors
    ///
    /// Returns the backend error when coordinate arity or values are invalid,
    /// allocation fails, or the insertion would violate backend invariants.
    fn insert_vertex(
        &mut self,
        coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error>;

    /// Removes a vertex and lets the backend retriangulate the surrounding cavity.
    ///
    /// This generic lifecycle operation reports only success or failure. A backend
    /// may implement it with a specialized inverse k=1 move when applicable, but
    /// callers must query and validate the resulting topology instead of assuming
    /// replacement-face or cavity metadata. If callers need such evidence in the
    /// future, it should be exposed through a dedicated structured result type.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the vertex handle is invalid, local
    /// topology is not removable, or the removal would violate backend
    /// invariants.
    fn remove_vertex(&mut self, vertex: Self::VertexHandle) -> Result<(), Self::Error>;

    /// Move a vertex to new coordinates
    ///
    /// # Errors
    ///
    /// Returns the backend error when the vertex handle is invalid, coordinate
    /// arity or values are invalid, or the move would violate backend invariants.
    fn move_vertex(
        &mut self,
        vertex: Self::VertexHandle,
        new_coords: &[Self::Coordinate],
    ) -> Result<(), Self::Error>;

    /// Flip an edge in the triangulation
    ///
    /// # Errors
    ///
    /// Returns the backend error when the edge handle is invalid, the edge is not
    /// flippable in the backend's local topology, or the flip would violate
    /// backend invariants.
    fn flip_edge(
        &mut self,
        edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::EdgeHandle, Self::FaceHandle>, Self::Error>;

    /// Checks whether an edge passes the backend's deterministic flip preflight.
    ///
    /// A `true` result means the corresponding [`Self::flip_edge`] call will not
    /// fail for deterministic structural or topological reasons on the same
    /// triangulation state. Allocation or failures outside that preflight may
    /// still prevent the mutation from committing.
    fn can_flip_edge(&self, edge: &Self::EdgeHandle) -> bool;

    /// Checks whether a face subdivision passes the backend's deterministic preflight.
    ///
    /// Backends without an immutable subdivision validator may conservatively
    /// return `false`. A `true` result has the same preflight relationship to
    /// [`Self::subdivide_face`] as [`Self::can_flip_edge`] has to
    /// [`Self::flip_edge`].
    fn can_subdivide_face(&self, _face: &Self::FaceHandle, _point: &[Self::Coordinate]) -> bool {
        false
    }

    /// Checks whether a vertex supports an exact local inverse k=1 collapse.
    ///
    /// This is narrower than [`Self::remove_vertex`], which may support generic
    /// cavity retriangulation. Backends without an immutable local-collapse
    /// validator may conservatively return `false`.
    fn can_collapse_vertex(&self, _vertex: &Self::VertexHandle) -> bool {
        false
    }

    /// Subdivide a face by adding a vertex at the given point
    ///
    /// # Errors
    ///
    /// Returns the backend error when the face handle is invalid, the point has
    /// invalid coordinate arity or values, allocation fails, or subdivision would
    /// violate backend invariants.
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
    /// use causal_triangulations::prelude::testing::*;
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
    /// use causal_triangulations::prelude::testing::*;
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
