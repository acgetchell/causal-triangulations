#![forbid(unsafe_code)]

//! Delaunay backend - wraps the delaunay crate.
//!
//! Together with `src/geometry/generators.rs`, this module is one of only
//! two places that directly import from the `delaunay` crate.  All modules
//! outside `src/geometry/` access Delaunay functionality through the trait
//! abstractions and handle types defined here
//! (see `docs/dev/rust.md § Geometry Backend Isolation`).
// cspell:ignore vkey

use crate::geometry::traits::{
    EdgeAdjacentFaces, EdgeAdjacentFacesResult, FlipResult, GeometryBackend, SubdivisionResult,
    TriangulationMut, TriangulationQuery,
};
use delaunay::core::DataType;
use delaunay::core::edge::EdgeKey;
use delaunay::core::facet::FacetHandle;
use delaunay::core::tds::{CellKey, VertexKey};
use delaunay::core::vertex::Vertex;
use delaunay::geometry::kernel::AdaptiveKernel;
use delaunay::geometry::point::Point;
use delaunay::geometry::traits::coordinate::Coordinate;
use delaunay::prelude::VertexBuilder;
use delaunay::prelude::triangulation::flips::BistellarFlips;
use delaunay::topology::traits::{GlobalTopology, TopologyKind};
use delaunay::triangulation::DelaunayTriangulation;
use std::collections::HashMap;

type DelaunayKernel = AdaptiveKernel<f64>;
type RawTriangulation<VertexData, CellData, const D: usize> =
    DelaunayTriangulation<DelaunayKernel, VertexData, CellData, D>;
type RawVertex<VertexData, const D: usize> = Vertex<f64, VertexData, D>;

/// Delaunay backend wrapping the delaunay crate's triangulation (f64 coordinates).
///
/// # Mutation support
///
/// The [`TriangulationMut`] methods (`insert_vertex`, `remove_vertex`, `flip_edge`, etc.)
/// are backed by the upstream Delaunay edit API where possible. `move_vertex()` is not yet
/// implemented and returns [`DelaunayError::NotImplemented`]. The `clear()` and
/// `reserve_capacity()` methods are currently no-ops that emit a `log::warn!` diagnostic.
#[derive(Debug, Clone)]
pub struct DelaunayBackend<VertexData: DataType, CellData: DataType, const D: usize> {
    /// The underlying Delaunay triangulation from the delaunay crate
    dt: RawTriangulation<VertexData, CellData, D>,
    /// Interior 2D edge to one incident facet suitable for k=2 local queries.
    interior_facets_by_edge: HashMap<EdgeKey, FacetHandle>,
}

/// Opaque handle for vertices in Delaunay backend
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayVertexHandle {
    key: VertexKey,
}

impl DelaunayVertexHandle {
    /// Returns the underlying slotmap key for use in secondary maps.
    #[must_use]
    pub(crate) const fn vertex_key(&self) -> VertexKey {
        self.key
    }

    /// Creates a handle from a raw vertex key (crate-internal).
    #[must_use]
    #[expect(dead_code, reason = "needed by ergodic moves (#55)")]
    pub(crate) const fn from_key(key: VertexKey) -> Self {
        Self { key }
    }
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

impl DelaunayFaceHandle {
    /// Returns the underlying cell key for payload lookups and updates by key.
    #[must_use]
    pub(crate) const fn cell_key(&self) -> CellKey {
        self.key
    }
}

/// Error type for Delaunay backend operations
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

    /// Invalid edge handle (the keyed edge is not present in the triangulation).
    #[error("invalid edge handle: edge {v0:?} -- {v1:?} not found in triangulation")]
    InvalidEdge {
        /// First endpoint vertex key
        v0: VertexKey,
        /// Second endpoint vertex key
        v1: VertexKey,
    },

    /// The edge exists but is not a valid bistellar k=2 flip target.
    #[error("non-flippable edge {v0:?} -- {v1:?}: {reason}")]
    NonFlippableEdge {
        /// First endpoint vertex key.
        v0: VertexKey,
        /// Second endpoint vertex key.
        v1: VertexKey,
        /// Why the live edge cannot be flipped.
        reason: &'static str,
    },

    /// Invalid face/cell handle (key not found in triangulation)
    #[error("invalid face handle: key {key:?} not found in triangulation")]
    InvalidFace {
        /// The cell key that was looked up
        key: CellKey,
    },

    /// Coordinate slice length does not match the backend dimension.
    #[error("coordinate dimension mismatch: got {actual}, expected {expected}")]
    CoordinateDimensionMismatch {
        /// Actual number of coordinates supplied.
        actual: usize,
        /// Expected coordinate count.
        expected: usize,
    },

    /// Vertex construction failed before a backend mutation could be attempted.
    #[error("failed to build vertex for {operation}: {detail}")]
    VertexBuildFailed {
        /// Backend operation that needed a new vertex.
        operation: &'static str,
        /// Underlying builder diagnostic.
        detail: String,
    },

    /// A Delaunay insertion failed after the vertex was built.
    #[error("{operation} insertion failed at {coordinates:?}: {detail}")]
    InsertionFailed {
        /// Backend operation performing the insertion.
        operation: &'static str,
        /// Coordinates of the vertex passed to the insertion routine.
        coordinates: Vec<f64>,
        /// Underlying insertion diagnostic.
        detail: String,
    },

    /// A bistellar flip failed.
    #[error("{operation} failed on {target}: {detail}")]
    FlipFailed {
        /// Flip operation that failed.
        operation: &'static str,
        /// Human-readable target passed to the flip operation.
        target: String,
        /// Underlying flip diagnostic.
        detail: String,
    },

    /// A successful upstream flip returned data that violates this backend's contract.
    #[error(
        "{operation} returned unexpected output for {target}: expected {expected}, got {actual}"
    )]
    UnexpectedFlipOutput {
        /// Flip operation that returned malformed output.
        operation: &'static str,
        /// Human-readable target passed to the flip operation.
        target: String,
        /// Contract expected by the backend wrapper.
        expected: &'static str,
        /// Output shape or detail observed from the upstream result.
        actual: String,
    },
}

impl<VertexData: DataType, CellData: DataType, const D: usize>
    DelaunayBackend<VertexData, CellData, D>
{
    /// Builds a backend vertex from a coordinate slice and optional payload.
    fn build_vertex(
        coords: &[f64],
        data: Option<VertexData>,
        operation: &'static str,
    ) -> Result<RawVertex<VertexData, D>, DelaunayError> {
        let coords: [f64; D] =
            coords
                .try_into()
                .map_err(|_| DelaunayError::CoordinateDimensionMismatch {
                    actual: coords.len(),
                    expected: D,
                })?;
        let mut builder = VertexBuilder::<f64, VertexData, D>::default().point(Point::new(coords));
        if let Some(data) = data {
            builder = builder.data(data);
        }
        builder
            .build()
            .map_err(|err| DelaunayError::VertexBuildFailed {
                operation,
                detail: err.to_string(),
            })
    }

    /// Finds the 2D facet handle corresponding to a live interior edge.
    fn interior_facet_for_edge(&self, edge: EdgeKey) -> Option<FacetHandle> {
        self.interior_facets_by_edge.get(&edge).copied()
    }

    /// Builds the 2D interior edge-facet lookup from current cell adjacency.
    fn build_interior_facets_by_edge(
        dt: &RawTriangulation<VertexData, CellData, D>,
    ) -> HashMap<EdgeKey, FacetHandle> {
        let mut facets_by_edge = HashMap::new();
        if D != 2 {
            return facets_by_edge;
        }
        for (cell_key, cell) in dt.cells() {
            let vertices = cell.vertices();
            let Some(neighbors) = cell.neighbors() else {
                continue;
            };

            for (facet_index, neighbor) in neighbors.iter().enumerate() {
                if neighbor.is_none() {
                    continue;
                }

                let mut facet_vertices = vertices
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, &key)| (idx != facet_index).then_some(key));
                let Some(v0) = facet_vertices.next() else {
                    continue;
                };
                let Some(v1) = facet_vertices.next() else {
                    continue;
                };
                if facet_vertices.next().is_none() {
                    let Ok(facet_index) = u8::try_from(facet_index) else {
                        continue;
                    };
                    facets_by_edge
                        .entry(EdgeKey::new(v0, v1))
                        .or_insert_with(|| FacetHandle::new(cell_key, facet_index));
                }
            }
        }

        facets_by_edge
    }

    /// Refreshes cached edge adjacency after a topology mutation succeeds.
    fn rebuild_interior_facet_index(&mut self) {
        self.interior_facets_by_edge = Self::build_interior_facets_by_edge(&self.dt);
    }

    /// Returns whether the keyed edge is present in the triangulation.
    fn edge_exists(&self, edge: EdgeKey) -> bool {
        let v0 = edge.v0();
        let v1 = edge.v1();
        self.dt.tds().contains_vertex_key(v0)
            && self.dt.tds().contains_vertex_key(v1)
            && self
                .dt
                .incident_edges(v0)
                .any(|candidate| candidate == edge)
    }

    /// Create a new Delaunay backend from an existing Delaunay triangulation
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::backends::delaunay::DelaunayBackend;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    /// use causal_triangulations::geometry::traits::TriangulationQuery;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let backend = DelaunayBackend::<u32, i32, 2>::from_triangulation(dt);
    /// assert_eq!(backend.vertex_count(), 3);
    /// ```
    #[must_use]
    pub fn from_triangulation(
        dt: DelaunayTriangulation<AdaptiveKernel<f64>, VertexData, CellData, D>,
    ) -> Self {
        let interior_facets_by_edge = Self::build_interior_facets_by_edge(&dt);
        Self {
            dt,
            interior_facets_by_edge,
        }
    }

    /// Access the underlying Delaunay triangulation (read-only)
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// assert_eq!(backend.triangulation().number_of_vertices(), 3);
    /// ```
    #[must_use]
    pub const fn triangulation(
        &self,
    ) -> &DelaunayTriangulation<AdaptiveKernel<f64>, VertexData, CellData, D> {
        &self.dt
    }

    /// Check if the triangulation is valid and satisfies the Delaunay property.
    ///
    /// Uses the upstream cumulative validation (`DelaunayTriangulation::validate`) which
    /// checks neighbor pointer consistency, Euler characteristic, coherent orientation
    /// (Levels 1–3) and the Delaunay in-sphere property (Level 4).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// assert!(backend.is_delaunay());
    /// ```
    #[must_use]
    pub fn is_delaunay(&self) -> bool {
        self.dt.validate().is_ok()
    }

    /// Returns the high-level topology kind (`Euclidean`, `Toroidal`, etc.) of the
    /// underlying triangulation.
    ///
    /// This exposes the [`GlobalTopology`]
    /// metadata attached by [`DelaunayTriangulationBuilder`](delaunay::triangulation::builder::DelaunayTriangulationBuilder) at construction time.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let _kind = backend.topology_kind();
    /// ```
    #[must_use]
    pub const fn topology_kind(&self) -> TopologyKind {
        self.dt.topology_kind()
    }

    /// Returns the toroidal fundamental domain for periodic triangulations.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let tri = CdtTriangulation::from_toroidal_cdt(3, 3).unwrap();
    /// assert_eq!(tri.geometry().periodic_domain(), Some([1.0, 1.0]));
    /// ```
    #[must_use]
    pub const fn periodic_domain(&self) -> Option<[f64; D]> {
        match self.dt.global_topology() {
            GlobalTopology::Toroidal { domain, .. } => Some(domain),
            GlobalTopology::Euclidean | GlobalTopology::Spherical | GlobalTopology::Hyperbolic => {
                None
            }
        }
    }

    /// Returns the vertex payload for `key`, if present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let (key, _) = backend.triangulation().vertices().next().unwrap();
    /// assert!(backend.vertex_data_by_key(key).is_some());
    /// ```
    #[must_use]
    pub fn vertex_data_by_key(&self, key: VertexKey) -> Option<VertexData> {
        self.dt.tds().get_vertex_by_key(key)?.data().copied()
    }

    /// Returns the cell payload for `key`, if present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let (key, _) = backend.triangulation().cells().next().unwrap();
    /// assert_eq!(backend.cell_data_by_key(key), None);
    /// ```
    #[must_use]
    pub fn cell_data_by_key(&self, key: CellKey) -> Option<CellData> {
        self.dt.tds().get_cell(key)?.data().copied()
    }

    /// Sets the optional payload for a vertex identified by `key`.
    ///
    /// Returns the previous payload for a valid key.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::InvalidVertex`] if `key` is not present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let mut backend = DelaunayBackend2D::from_triangulation(dt);
    /// let (key, _) = backend.triangulation().vertices().next().unwrap();
    /// let previous = backend.set_vertex_data_by_key(key, Some(3)).unwrap();
    /// assert!(previous.is_some());
    /// assert_eq!(backend.vertex_data_by_key(key), Some(3));
    /// ```
    pub fn set_vertex_data_by_key(
        &mut self,
        key: VertexKey,
        data: Option<VertexData>,
    ) -> Result<Option<VertexData>, DelaunayError> {
        self.dt
            .set_vertex_data(key, data)
            .ok_or(DelaunayError::InvalidVertex { key })
    }

    /// Sets the optional payload for a cell identified by `key`.
    ///
    /// Returns the previous payload for a valid key.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::InvalidFace`] if `key` is not present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::geometry::DelaunayBackend2D;
    /// use causal_triangulations::geometry::generators::build_delaunay2_with_data;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0_u32),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ]).unwrap();
    /// let mut backend = DelaunayBackend2D::from_triangulation(dt);
    /// let (key, _) = backend.triangulation().cells().next().unwrap();
    /// let previous = backend.set_cell_data_by_key(key, Some(1)).unwrap();
    /// assert_eq!(previous, None);
    /// assert_eq!(backend.cell_data_by_key(key), Some(1));
    /// ```
    pub fn set_cell_data_by_key(
        &mut self,
        key: CellKey,
        data: Option<CellData>,
    ) -> Result<Option<CellData>, DelaunayError> {
        self.dt
            .set_cell_data(key, data)
            .ok_or(DelaunayError::InvalidFace { key })
    }
}

impl<VertexData: DataType, CellData: DataType, const D: usize> GeometryBackend
    for DelaunayBackend<VertexData, CellData, D>
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

impl<VertexData: DataType, CellData: DataType, const D: usize> TriangulationQuery
    for DelaunayBackend<VertexData, CellData, D>
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
    ) -> Option<(Self::VertexHandle, Self::VertexHandle)> {
        let (v0, v1) = edge.key.endpoints();
        let tds = self.dt.tds();

        let contains_v0 = tds.contains_vertex_key(v0);
        let contains_v1 = tds.contains_vertex_key(v1);
        // Fast reject for invalid endpoint handles.
        if !(contains_v0 && contains_v1) {
            log::trace!(
                "edge_endpoints: missing endpoint(s) for edge {:?} (contains_v0={}, contains_v1={})",
                edge.key,
                contains_v0,
                contains_v1,
            );
            return None;
        }

        // Validate membership using local adjacency around v0.
        // This is O(deg(v0)) rather than scanning all edges.
        let edge_exists = self.dt.incident_edges(v0).any(|candidate| {
            let (c0, c1) = candidate.endpoints();
            (c0 == v0 && c1 == v1) || (c0 == v1 && c1 == v0)
        });

        if edge_exists {
            return Some((
                DelaunayVertexHandle { key: v0 },
                DelaunayVertexHandle { key: v1 },
            ));
        }

        log::trace!(
            "edge_endpoints: unable to resolve edge {:?} (contains_v0={}, contains_v1={}, edge_exists={}, V={}, E={}, F={})",
            edge.key,
            contains_v0,
            contains_v1,
            edge_exists,
            self.dt.number_of_vertices(),
            self.dt.as_triangulation().number_of_edges(),
            self.dt.number_of_cells(),
        );

        None
    }

    fn edge_adjacent_faces(
        &self,
        edge: &Self::EdgeHandle,
    ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error> {
        if !self.edge_exists(edge.key) {
            return Err(DelaunayError::InvalidEdge {
                v0: edge.key.v0(),
                v1: edge.key.v1(),
            });
        }

        let Some(facet) = self.interior_facet_for_edge(edge.key) else {
            return Ok(None);
        };
        let face_0 = facet.cell_key();
        let facet_index = <usize as From<_>>::from(facet.facet_index());
        let Some(cell_0) = self.dt.tds().get_cell(face_0) else {
            return Err(DelaunayError::InvalidFace { key: face_0 });
        };
        let vertices_0 = cell_0.vertices();
        if vertices_0.len() != 3 || facet_index >= vertices_0.len() {
            return Ok(None);
        }
        let Some(face_1) = cell_0
            .neighbors()
            .and_then(|neighbors| neighbors.get(facet_index).copied().flatten())
        else {
            return Ok(None);
        };

        let mut endpoints = vertices_0
            .iter()
            .enumerate()
            .filter_map(|(idx, &key)| (idx != facet_index).then_some(key));
        let Some(endpoint_0) = endpoints.next() else {
            return Ok(None);
        };
        let Some(endpoint_1) = endpoints.next() else {
            return Ok(None);
        };
        if endpoints.next().is_some() {
            return Ok(None);
        }

        let Some(vertices_1) = self.dt.cell_vertices(face_1) else {
            return Err(DelaunayError::InvalidFace { key: face_1 });
        };
        if vertices_1.len() != 3 {
            return Ok(None);
        }
        let Some(opposite_1) = vertices_1
            .iter()
            .copied()
            .find(|&key| key != endpoint_0 && key != endpoint_1)
        else {
            return Ok(None);
        };

        Ok(Some(EdgeAdjacentFaces::new(
            (
                DelaunayVertexHandle { key: endpoint_0 },
                DelaunayVertexHandle { key: endpoint_1 },
            ),
            (
                DelaunayFaceHandle { key: face_0 },
                DelaunayFaceHandle { key: face_1 },
            ),
            (
                DelaunayVertexHandle {
                    key: vertices_0[facet_index],
                },
                DelaunayVertexHandle { key: opposite_1 },
            ),
        )))
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

impl<VertexData: DataType, CellData: DataType, const D: usize> TriangulationMut
    for DelaunayBackend<VertexData, CellData, D>
{
    fn insert_vertex(
        &mut self,
        coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error> {
        let vertex = Self::build_vertex(coords, None, "insert_vertex")?;
        let key = self
            .dt
            .insert(vertex)
            .map_err(|err| DelaunayError::InsertionFailed {
                operation: "insert_vertex",
                coordinates: coords.to_vec(),
                detail: err.to_string(),
            })?;
        self.rebuild_interior_facet_index();
        Ok(DelaunayVertexHandle { key })
    }

    fn remove_vertex(
        &mut self,
        vertex: Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        if !self.dt.tds().contains_vertex_key(vertex.key) {
            return Err(DelaunayError::InvalidVertex { key: vertex.key });
        }

        let info = self
            .dt
            .flip_k1_remove(vertex.key)
            .map_err(|err| DelaunayError::FlipFailed {
                operation: "flip_k1_remove",
                target: format!("vertex {:?}", vertex.key),
                detail: err.to_string(),
            })?;
        self.rebuild_interior_facet_index();
        Ok(info
            .new_cells
            .iter()
            .copied()
            .map(|key| DelaunayFaceHandle { key })
            .collect())
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
        edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::EdgeHandle, Self::FaceHandle>, Self::Error> {
        let facet = if self.edge_exists(edge.key) {
            self.interior_facet_for_edge(edge.key).ok_or_else(|| {
                DelaunayError::NonFlippableEdge {
                    v0: edge.key.v0(),
                    v1: edge.key.v1(),
                    reason: "edge is not an interior 2D facet shared by two cells",
                }
            })?
        } else {
            return Err(DelaunayError::InvalidEdge {
                v0: edge.key.v0(),
                v1: edge.key.v1(),
            });
        };
        let info = self
            .dt
            .flip_k2(facet)
            .map_err(|err| DelaunayError::FlipFailed {
                operation: "flip_k2",
                target: format!(
                    "edge {:?} -- {:?} via facet {:?}",
                    edge.key.v0(),
                    edge.key.v1(),
                    facet
                ),
                detail: err.to_string(),
            })?;
        self.rebuild_interior_facet_index();
        let inserted: Vec<_> = info.inserted_face_vertices.iter().copied().collect();
        let [v0, v1] = inserted.as_slice() else {
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: "flip_k2",
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                expected: "exactly two inserted-face vertices for the replacement edge",
                actual: format!("{} inserted-face vertices", inserted.len()),
            });
        };
        let affected_faces = info
            .new_cells
            .iter()
            .copied()
            .map(|key| DelaunayFaceHandle { key })
            .collect();
        Ok(FlipResult::new(
            DelaunayEdgeHandle {
                key: EdgeKey::new(*v0, *v1),
            },
            affected_faces,
        ))
    }

    fn can_flip_edge(&self, edge: &Self::EdgeHandle) -> bool {
        self.interior_facet_for_edge(edge.key).is_some()
    }

    fn subdivide_face(
        &mut self,
        face: Self::FaceHandle,
        point: &[Self::Coordinate],
    ) -> Result<SubdivisionResult<Self::VertexHandle, Self::FaceHandle>, Self::Error> {
        if !self.dt.tds().contains_cell_key(face.key) {
            return Err(DelaunayError::InvalidFace { key: face.key });
        }

        let vertex = Self::build_vertex(point, None, "subdivide_face")?;
        let info =
            self.dt
                .flip_k1_insert(face.key, vertex)
                .map_err(|err| DelaunayError::FlipFailed {
                    operation: "flip_k1_insert",
                    target: format!("face {:?} at point {:?}", face.key, point),
                    detail: err.to_string(),
                })?;
        self.rebuild_interior_facet_index();
        let Some(&new_vertex) = info.inserted_face_vertices.first() else {
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: "flip_k1_insert",
                target: format!("face {:?} at point {:?}", face.key, point),
                expected: "at least one inserted-face vertex for the inserted point",
                actual: "no inserted-face vertices".to_string(),
            });
        };
        Ok(SubdivisionResult::new(
            DelaunayVertexHandle { key: new_vertex },
            info.new_cells
                .iter()
                .copied()
                .map(|key| DelaunayFaceHandle { key })
                .collect(),
            face,
        ))
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::geometry::generators::{
        build_delaunay2_from_cells, build_delaunay2_with_data, generate_delaunay2,
        random_delaunay2, seeded_delaunay2,
    };
    use crate::util::saturating_usize_to_i32;
    use slotmap::KeyData;

    use super::*;

    #[test]
    fn test_is_delaunay_various_sizes() {
        // is_delaunay() should pass for valid triangulations of all sizes
        for n in [3, 4, 10, 20] {
            let dt = random_delaunay2(n, (0.0, 10.0));
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
        let dt = random_delaunay2(5, (0.0, 10.0));
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
        let dt = random_delaunay2(3, (0.0, 10.0));
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
        let dt = random_delaunay2(5, (0.0, 10.0));
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
        let dt = random_delaunay2(4, (0.0, 10.0));
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
        let dt = random_delaunay2(5, (0.0, 10.0));
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
        let dt = random_delaunay2(3, (0.0, 10.0));
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
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        // Use a high-generation key that cannot exist in the triangulation's slotmap
        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.vertex_coordinates(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key),
            "Expected InvalidVertex with matching key, got: {err}"
        );
    }

    #[test]
    fn test_face_vertices() {
        let dt = random_delaunay2(3, (0.0, 10.0));
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
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = CellKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };
        let err = backend.face_vertices(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key),
            "Expected InvalidFace with matching key, got: {err}"
        );
    }

    #[test]
    fn test_edge_endpoints() {
        let dt = random_delaunay2(4, (0.0, 10.0));
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
    fn test_edge_endpoints_for_hand_built_triangle() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build hand-built triangle");
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertices: HashSet<_> = backend.vertices().collect();
        let edges: Vec<_> = backend.edges().collect();

        assert_eq!(edges.len(), 3, "Hand-built triangle should expose 3 edges");

        for edge in &edges {
            let (v0, v1) = backend
                .edge_endpoints(edge)
                .expect("Should retrieve endpoints for hand-built triangle edge");
            assert!(
                vertices.contains(&v0),
                "First endpoint should be a valid vertex in the hand-built triangle"
            );
            assert!(
                vertices.contains(&v1),
                "Second endpoint should be a valid vertex in the hand-built triangle"
            );
        }
    }

    #[test]
    fn test_edge_endpoints_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let k1 = VertexKey::from(KeyData::from_ffi(u64::MAX - 1));
        let k2 = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayEdgeHandle {
            key: EdgeKey::new(k1, k2),
        };
        assert!(
            backend.edge_endpoints(&invalid_handle).is_none(),
            "Invalid edge handle should return None"
        );
    }

    #[test]
    fn test_edge_endpoints_non_edge_with_existing_vertices_returns_none() {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], 0),
            ([2.0, 0.0], 0),
            ([2.2, 1.2], 0),
            ([1.0, 2.0], 0),
            ([-0.2, 1.0], 0),
        ])
        .expect("Should build deterministic 5-point triangulation");
        let backend = DelaunayBackend::from_triangulation(dt);

        let edge_pairs: HashSet<_> = backend
            .edges()
            .map(|edge| {
                let (a, b) = edge.key.endpoints();
                (a, b)
            })
            .collect();

        let vertex_keys: Vec<_> = backend.vertices().map(|vh| vh.key).collect();

        let mut non_edge_pair = None;
        'find_non_edge: for i in 0..vertex_keys.len() {
            for j in (i + 1)..vertex_keys.len() {
                let a = vertex_keys[i];
                let b = vertex_keys[j];
                if !edge_pairs.contains(&(a, b)) && !edge_pairs.contains(&(b, a)) {
                    non_edge_pair = Some((a, b));
                    break 'find_non_edge;
                }
            }
        }

        let (a, b) =
            non_edge_pair.expect("A planar 5-vertex triangulation must have a non-edge pair");
        let non_edge_handle = DelaunayEdgeHandle {
            key: EdgeKey::new(a, b),
        };

        assert!(
            backend.edge_endpoints(&non_edge_handle).is_none(),
            "Non-edge handle with existing vertices should return None"
        );
    }

    #[test]
    fn test_adjacent_faces() {
        let dt = random_delaunay2(4, (0.0, 10.0));
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
        let dt = random_delaunay2(4, (0.0, 10.0));
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
        let dt = random_delaunay2(5, (0.0, 10.0));
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
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = CellKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };
        let err = backend.face_neighbors(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key),
            "Expected InvalidFace with matching key, got: {err}"
        );
    }

    #[test]
    fn test_adjacent_faces_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.adjacent_faces(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key),
            "Expected InvalidVertex with matching key, got: {err}"
        );
    }

    #[test]
    fn test_incident_edges_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.incident_edges(&invalid_handle).unwrap_err();
        assert!(
            matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key),
            "Expected InvalidVertex with matching key, got: {err}"
        );
    }

    #[test]
    fn test_dimension() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);
        assert_eq!(backend.dimension(), 2, "DelaunayBackend2D should be 2D");
    }

    #[test]
    fn test_euler_characteristic() {
        // For a planar triangulation without boundary: V - E + F = 1
        let dt = seeded_delaunay2(6, (0.0, 10.0), 42);
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
        let dt = seeded_delaunay2(8, (0.0, 10.0), 42);
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
        let dt = seeded_delaunay2(6, (0.0, 10.0), 42);
        let backend = DelaunayBackend::from_triangulation(dt);

        let vertex_count = backend.vertex_count();
        let edge_count = backend.edge_count();
        let face_count = backend.face_count();

        // Verify Euler characteristic for planar graphs
        // For a triangulation without the outer infinite face: V - E + F = 1
        // For a triangulation with the outer infinite face: V - E + F = 2
        // Note: Random triangulations may occasionally have degeneracies that result in χ = 0
        let euler = saturating_usize_to_i32(vertex_count) - saturating_usize_to_i32(edge_count)
            + saturating_usize_to_i32(face_count);
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
        let dt = random_delaunay2(3, (0.0, 10.0));
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
        let dt = random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);

        assert_eq!(
            backend.topology_kind(),
            TopologyKind::Euclidean,
            "Default builder construction should produce Euclidean topology"
        );
    }

    #[test]
    fn test_is_valid_runs_structural_validation() {
        // is_valid() runs Levels 1–3 (structural/topological) via as_triangulation().validate();
        // is_delaunay() runs Levels 1–4 (including the Delaunay property).
        // For a well-formed Delaunay triangulation both should pass.
        let dt = seeded_delaunay2(8, (0.0, 10.0), 99);
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
    fn test_mutation_methods_use_delaunay_edit_api() {
        let dt = build_delaunay2_from_cells(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit square should build");
        let mut backend = DelaunayBackend::from_triangulation(dt);
        let original_vertex_count = backend.vertex_count();
        let original_face_count = backend.face_count();

        let edge = backend
            .edges()
            .find(|edge| backend.can_flip_edge(edge))
            .expect("square has an interior edge");
        let flip = backend.flip_edge(edge);
        assert!(flip.is_ok());
        assert_eq!(backend.vertex_count(), original_vertex_count);
        assert_eq!(backend.face_count(), original_face_count);
        assert!(backend.is_valid());

        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("labeled triangle should build");
        let mut backend = DelaunayBackend::from_triangulation(dt);
        let original_vertex_count = backend.vertex_count();
        let original_face_count = backend.face_count();
        let face = backend.faces().next().expect("valid face handle");
        let subdivide = backend
            .subdivide_face(face, &[0.5, 1.0 / 3.0])
            .expect("face subdivision should use k=1 flip");
        assert_eq!(backend.vertex_count(), original_vertex_count + 1);
        assert_eq!(backend.face_count(), original_face_count + 2);
        assert!(backend.is_valid());

        backend
            .remove_vertex(subdivide.new_vertex)
            .expect("degree-3 inserted vertex should be removable");
        assert_eq!(backend.vertex_count(), original_vertex_count);
        assert_eq!(backend.face_count(), original_face_count);
        assert!(backend.is_valid());

        let inserted = backend.insert_vertex(&[0.25, 0.75]);
        assert!(inserted.is_ok());
        assert_eq!(backend.vertex_count(), original_vertex_count + 1);
        assert!(backend.is_valid());

        let vertex = backend.vertices().next().expect("valid vertex handle");

        assert!(matches!(
            backend.move_vertex(vertex, &[1.0, 1.0]),
            Err(DelaunayError::NotImplemented {
                operation: "move_vertex",
            })
        ));
        assert!(matches!(
            backend.insert_vertex(&[0.0]),
            Err(DelaunayError::CoordinateDimensionMismatch {
                actual: 1,
                expected: 2,
            })
        ));

        let bogus_vertex = VertexKey::from(KeyData::from_ffi(u64::MAX));
        assert!(matches!(
            backend.remove_vertex(DelaunayVertexHandle { key: bogus_vertex }),
            Err(DelaunayError::InvalidVertex { key }) if key == bogus_vertex,
        ));

        let bogus_face = CellKey::from(KeyData::from_ffi(u64::MAX));
        assert!(matches!(
            backend.subdivide_face(DelaunayFaceHandle { key: bogus_face }, &[0.25, 0.25]),
            Err(DelaunayError::InvalidFace { key }) if key == bogus_face,
        ));

        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("labeled triangle should build");
        let mut boundary_backend = DelaunayBackend::from_triangulation(dt);
        let boundary_edge = boundary_backend
            .edges()
            .next()
            .expect("single triangle has boundary edges");
        assert!(matches!(
            boundary_backend.flip_edge(boundary_edge),
            Err(DelaunayError::NonFlippableEdge { reason, .. })
                if reason.contains("interior 2D facet"),
        ));

        let counts_before_noops = (
            backend.vertex_count(),
            backend.edge_count(),
            backend.face_count(),
        );
        backend.clear();
        backend.reserve_capacity(32, 64);
        assert_eq!(
            (
                backend.vertex_count(),
                backend.edge_count(),
                backend.face_count(),
            ),
            counts_before_noops
        );
    }

    #[test]
    fn test_interior_facet_cache_updates_after_edge_flip() {
        let dt = build_delaunay2_from_cells(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit square should build");
        let mut backend = DelaunayBackend::from_triangulation(dt);
        assert_eq!(backend.interior_facets_by_edge.len(), 1);

        let edge = backend
            .edges()
            .find(|edge| backend.can_flip_edge(edge))
            .expect("square has one interior edge");
        let old_edge = edge.key;
        let flip = backend.flip_edge(edge).expect("interior edge should flip");

        assert_eq!(backend.interior_facets_by_edge.len(), 1);
        assert!(!backend.interior_facets_by_edge.contains_key(&old_edge));
        assert!(
            backend
                .interior_facets_by_edge
                .contains_key(&flip.new_edge.key)
        );
    }

    #[test]
    fn test_builder_produces_correct_vertex_count() {
        // Verify the builder path in generate_delaunay2 preserves vertex count
        for n in [3, 5, 10, 20] {
            let dt = generate_delaunay2(n, (0.0, 10.0), Some(42)).expect("Builder should succeed");
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

        // Verify the backend implements Send + Sync for safe concurrent use
        let dt = random_delaunay2(5, (0.0, 10.0));
        let backend = DelaunayBackend::from_triangulation(dt);
        assert_send_sync(&backend);
    }
}
