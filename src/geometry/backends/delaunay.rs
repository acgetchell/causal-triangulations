#![forbid(unsafe_code)]

//! Delaunay backend - wraps the delaunay crate.
//!
//! Together with `src/geometry/generators.rs`, this module is one of only
//! two places that directly import from the `delaunay` crate.  All modules
//! outside `src/geometry/` access Delaunay functionality through the trait
//! abstractions and handle types defined here
//! (see `docs/dev/rust.md § Geometry Backend Isolation`).
// cspell:ignore vkey

use crate::DelaunayValidationLevel;
use crate::geometry::traits::{
    EdgeAdjacentFaces, EdgeAdjacentFacesResult, FlipResult, GeometryBackend, SubdivisionResult,
    TriangulationMut, TriangulationQuery,
};
use delaunay::flips::BistellarFlips;
use delaunay::geometry::kernel::AdaptiveKernel;
use delaunay::prelude::DataType;
use delaunay::prelude::export::{MeshExport, MeshExportError};
use delaunay::tds::{EdgeKey, FacetHandle, SimplexKey, Tds, Vertex, VertexKey};
use delaunay::topology::traits::{GlobalTopology, TopologyKind, ToroidalConstructionMode};
use delaunay::{
    DelaunayCheckPolicy, DelaunayTriangulation, SimplexBarycenterError, TopologyGuarantee,
};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::num::NonZeroUsize;

type DelaunayKernel = AdaptiveKernel<f64>;
type RawTriangulation<VertexData, SimplexData, const D: usize> =
    DelaunayTriangulation<DelaunayKernel, VertexData, SimplexData, D>;
type RawVertex<VertexData, const D: usize> = Vertex<VertexData, D>;
/// Upstream stable mesh-interchange value used by CDT summary exports.
pub(crate) type DelaunayMeshExport<const D: usize> = MeshExport<D>;

/// Delaunay backend wrapping the delaunay crate's triangulation (f64 coordinates).
///
/// # Mutation support
///
/// The [`TriangulationMut`] methods (`insert_vertex`, `remove_vertex`, `flip_edge`, etc.)
/// are backed by the upstream Delaunay edit API where possible. `move_vertex()`, `clear()`,
/// and `reserve_capacity()` are not yet implemented and return
/// [`DelaunayError::NotImplemented`].
///
/// # Serialization
///
/// Serde checkpoints store the upstream triangulation data structure plus its
/// global topology and topology-guarantee metadata. Deserialization rebuilds
/// transient backend indexes, including the interior-facet lookup used for local
/// 2D edge queries. Vertex/simplex incidence is maintained by Delaunay and is not
/// duplicated in checkpoints or backend caches.
///
/// This representation is version-bound because it embeds Delaunay's internal
/// triangulation structure. Serialized backends and enclosing CDT checkpoints
/// are supported only when read by the same build that wrote them or by a release
/// that explicitly documents checkpoint compatibility. Toroidal topology
/// checkpoints must contain finite, strictly positive periods; invalid domains
/// are rejected during deserialization before a backend can observe them.
#[derive(Debug)]
pub struct DelaunayBackend<VertexData, SimplexData, const D: usize> {
    /// The underlying Delaunay triangulation from the delaunay crate
    dt: RawTriangulation<VertexData, SimplexData, D>,
    /// Interior 2D edge to one incident facet suitable for k=2 local queries.
    interior_facets_by_edge: HashMap<EdgeKey, FacetHandle>,
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> Clone
    for DelaunayBackend<VertexData, SimplexData, D>
{
    fn clone(&self) -> Self {
        Self {
            dt: self.dt.clone(),
            interior_facets_by_edge: self.interior_facets_by_edge.clone(),
        }
    }
}

#[derive(Serialize)]
struct SerializedDelaunayBackendRef<'a, VertexData, SimplexData, const D: usize> {
    tds: &'a RawTriangulation<VertexData, SimplexData, D>,
    global_topology: SerializableGlobalTopology,
    topology_guarantee: SerializableTopologyGuarantee,
    delaunay_check_policy: SerializableDelaunayCheckPolicy,
}

#[derive(Serialize, Deserialize)]
#[serde(bound(
    serialize = "Tds<VertexData, SimplexData, D>: Serialize",
    deserialize = "Tds<VertexData, SimplexData, D>: Deserialize<'de>"
))]
struct SerializedDelaunayBackend<VertexData, SimplexData, const D: usize> {
    tds: Tds<VertexData, SimplexData, D>,
    global_topology: SerializableGlobalTopology,
    topology_guarantee: SerializableTopologyGuarantee,
    #[serde(default)]
    delaunay_check_policy: SerializableDelaunayCheckPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum SerializableGlobalTopology {
    Euclidean,
    Toroidal {
        domain: Vec<f64>,
        mode: SerializableToroidalConstructionMode,
    },
    Spherical,
    Hyperbolic,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum SerializableToroidalConstructionMode {
    Canonicalized,
    PeriodicImagePoint,
    Explicit,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum SerializableTopologyGuarantee {
    Pseudomanifold,
    PLManifold,
    PLManifoldStrict,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
enum SerializableDelaunayCheckPolicy {
    #[default]
    EndOnly,
    EveryN(usize),
}

impl<const D: usize> From<GlobalTopology<D>> for SerializableGlobalTopology {
    fn from(topology: GlobalTopology<D>) -> Self {
        match topology {
            GlobalTopology::Euclidean => Self::Euclidean,
            GlobalTopology::Toroidal { domain, mode } => Self::Toroidal {
                domain: domain.periods().to_vec(),
                mode: mode.into(),
            },
            GlobalTopology::Spherical => Self::Spherical,
            GlobalTopology::Hyperbolic => Self::Hyperbolic,
        }
    }
}

impl SerializableGlobalTopology {
    fn into_global_topology<const D: usize, E: DeError>(self) -> Result<GlobalTopology<D>, E> {
        match self {
            Self::Euclidean => Ok(GlobalTopology::Euclidean),
            Self::Toroidal { domain, mode } => {
                let actual = domain.len();
                let domain: [f64; D] = domain.try_into().map_err(|_| {
                    E::custom(format!(
                        "toroidal domain length mismatch: got {actual}, expected {D}"
                    ))
                })?;
                for (index, period) in domain.iter().copied().enumerate() {
                    if !period.is_finite() || period <= 0.0 {
                        return Err(E::custom(format!(
                            "invalid toroidal period at index {index}: {period}"
                        )));
                    }
                }
                let mode = mode.into_toroidal_construction_mode()?;
                GlobalTopology::try_toroidal(domain, mode).map_err(E::custom)
            }
            Self::Spherical => Ok(GlobalTopology::Spherical),
            Self::Hyperbolic => Ok(GlobalTopology::Hyperbolic),
        }
    }
}

impl From<ToroidalConstructionMode> for SerializableToroidalConstructionMode {
    fn from(mode: ToroidalConstructionMode) -> Self {
        match mode {
            ToroidalConstructionMode::PeriodicImagePoint => Self::PeriodicImagePoint,
            ToroidalConstructionMode::Explicit => Self::Explicit,
        }
    }
}

impl SerializableToroidalConstructionMode {
    fn into_toroidal_construction_mode<E: DeError>(self) -> Result<ToroidalConstructionMode, E> {
        match self {
            Self::Canonicalized => Err(E::custom(
                "legacy toroidal construction mode `Canonicalized` is not supported because it is not semantically equivalent to `PeriodicImagePoint`",
            )),
            Self::PeriodicImagePoint => Ok(ToroidalConstructionMode::PeriodicImagePoint),
            Self::Explicit => Ok(ToroidalConstructionMode::Explicit),
        }
    }
}

impl From<TopologyGuarantee> for SerializableTopologyGuarantee {
    fn from(guarantee: TopologyGuarantee) -> Self {
        match guarantee {
            TopologyGuarantee::Pseudomanifold => Self::Pseudomanifold,
            TopologyGuarantee::PLManifold => Self::PLManifold,
            TopologyGuarantee::PLManifoldStrict => Self::PLManifoldStrict,
        }
    }
}

impl From<SerializableTopologyGuarantee> for TopologyGuarantee {
    fn from(guarantee: SerializableTopologyGuarantee) -> Self {
        match guarantee {
            SerializableTopologyGuarantee::Pseudomanifold => Self::Pseudomanifold,
            SerializableTopologyGuarantee::PLManifold => Self::PLManifold,
            SerializableTopologyGuarantee::PLManifoldStrict => Self::PLManifoldStrict,
        }
    }
}

impl From<DelaunayCheckPolicy> for SerializableDelaunayCheckPolicy {
    fn from(policy: DelaunayCheckPolicy) -> Self {
        match policy {
            DelaunayCheckPolicy::EndOnly => Self::EndOnly,
            DelaunayCheckPolicy::EveryN(n) => Self::EveryN(n.get()),
        }
    }
}

impl SerializableDelaunayCheckPolicy {
    fn into_delaunay_check_policy<E: DeError>(self) -> Result<DelaunayCheckPolicy, E> {
        match self {
            Self::EndOnly => Ok(DelaunayCheckPolicy::EndOnly),
            Self::EveryN(n) => NonZeroUsize::new(n).map_or_else(
                || Err(E::custom("delaunay check interval must be non-zero")),
                |interval| Ok(DelaunayCheckPolicy::EveryN(interval)),
            ),
        }
    }
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> Serialize
    for DelaunayBackend<VertexData, SimplexData, D>
where
    RawTriangulation<VertexData, SimplexData, D>: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SerializedDelaunayBackendRef {
            tds: &self.dt,
            global_topology: self.dt.global_topology().into(),
            topology_guarantee: self.dt.topology_guarantee().into(),
            delaunay_check_policy: self.dt.delaunay_check_policy().into(),
        }
        .serialize(serializer)
    }
}

impl<'de, VertexData: DataType, SimplexData: DataType, const D: usize> Deserialize<'de>
    for DelaunayBackend<VertexData, SimplexData, D>
where
    Tds<VertexData, SimplexData, D>: Deserialize<'de>,
{
    fn deserialize<DE>(deserializer: DE) -> Result<Self, DE::Error>
    where
        DE: Deserializer<'de>,
    {
        let serialized = SerializedDelaunayBackend::deserialize(deserializer)?;
        let topology_guarantee = serialized.topology_guarantee.into();
        let global_topology = serialized.global_topology.into_global_topology()?;
        let mut dt = DelaunayTriangulation::try_from_tds_with_topology_context(
            serialized.tds,
            AdaptiveKernel::new(),
            topology_guarantee,
            global_topology,
        )
        .map_err(DE::Error::custom)?;
        dt.set_delaunay_check_policy(
            serialized
                .delaunay_check_policy
                .into_delaunay_check_policy()?,
        );
        Self::from_triangulation(dt).map_err(DE::Error::custom)
    }
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
}

/// Opaque handle for edges in Delaunay backend
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayEdgeHandle {
    key: EdgeKey,
}

/// Opaque handle for faces in Delaunay backend
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayFaceHandle {
    key: SimplexKey,
}

impl DelaunayFaceHandle {
    /// Returns the underlying simplex key for payload lookups and updates by key.
    #[must_use]
    pub(crate) const fn simplex_key(&self) -> SimplexKey {
        self.key
    }
}

/// Backend operation category carried by [`DelaunayError`] variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DelaunayOperation {
    /// Insert a vertex into the triangulation.
    InsertVertex,
    /// Move a vertex to new coordinates.
    MoveVertex,
    /// Remove a vertex through the upstream vertex-removal API.
    RemoveVertex,
    /// Build a vertex for face subdivision.
    SubdivideFace,
    /// Remove a vertex through the upstream k=1 flip API.
    FlipK1Remove,
    /// Insert a vertex through the upstream k=1 flip API.
    FlipK1Insert,
    /// Flip an interior edge through the upstream k=2 flip API.
    FlipK2,
    /// Clear all backend geometry.
    Clear,
    /// Reserve backend storage capacity.
    ReserveCapacity,
}

impl fmt::Display for DelaunayOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsertVertex => formatter.write_str("insert_vertex"),
            Self::MoveVertex => formatter.write_str("move_vertex"),
            Self::RemoveVertex => formatter.write_str("remove_vertex"),
            Self::SubdivideFace => formatter.write_str("subdivide_face"),
            Self::FlipK1Remove => formatter.write_str("flip_k1_remove"),
            Self::FlipK1Insert => formatter.write_str("flip_k1_insert"),
            Self::FlipK2 => formatter.write_str("flip_k2"),
            Self::Clear => formatter.write_str("clear"),
            Self::ReserveCapacity => formatter.write_str("reserve_capacity"),
        }
    }
}

/// Reason a live edge cannot be used as a k=2 flip target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NonFlippableEdgeReason {
    /// The edge is not represented by an interior facet shared by two simplices.
    NotInteriorFacet,
}

impl fmt::Display for NonFlippableEdgeReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInteriorFacet => {
                formatter.write_str("edge is not an interior 2D facet shared by two simplices")
            }
        }
    }
}

/// Delaunay backend errors preserving typed mutation and validation context.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DelaunayError {
    /// Operation is not yet implemented
    #[error("not implemented: {operation}")]
    NotImplemented {
        /// Name of the unimplemented operation
        operation: DelaunayOperation,
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
        reason: NonFlippableEdgeReason,
    },

    /// Invalid face/simplex handle (key not found in triangulation)
    #[error("invalid face handle: key {key:?} not found in triangulation")]
    InvalidFace {
        /// The simplex key that was looked up.
        key: SimplexKey,
    },

    /// Upstream topology-aware barycenter computation failed for a live face.
    #[error("failed to compute barycenter for face {key:?}: {detail}")]
    FaceBarycenterFailed {
        /// The simplex key whose barycenter was requested.
        key: SimplexKey,
        /// Underlying barycenter diagnostic.
        detail: String,
    },

    /// Coordinate slice length does not match the backend dimension.
    #[error("coordinate dimension mismatch: got {actual}, expected {expected}")]
    CoordinateDimensionMismatch {
        /// Actual number of coordinates supplied.
        actual: usize,
        /// Expected coordinate count.
        expected: usize,
    },

    /// Coordinate value cannot be used by geometric predicates.
    #[error("non-finite coordinate for {operation}: axis {axis} = {value}")]
    NonFiniteCoordinate {
        /// Backend operation that received the coordinate.
        operation: DelaunayOperation,
        /// Coordinate axis.
        axis: usize,
        /// Supplied non-finite value.
        value: f64,
    },

    /// Vertex construction failed before a backend mutation could be attempted.
    #[error("failed to build vertex for {operation}: {detail}")]
    VertexBuildFailed {
        /// Backend operation that needed a new vertex.
        operation: DelaunayOperation,
        /// Underlying builder diagnostic.
        detail: String,
    },

    /// A Delaunay insertion failed after the vertex was built.
    #[error("{operation} insertion failed at {coordinates:?}: {detail}")]
    InsertionFailed {
        /// Backend operation performing the insertion.
        operation: DelaunayOperation,
        /// Coordinates of the vertex passed to the insertion routine.
        coordinates: Vec<f64>,
        /// Underlying insertion diagnostic.
        detail: String,
    },

    /// A vertex-removal mutation failed after the vertex handle was accepted.
    #[error("{operation} failed on {target}: {detail}")]
    RemovalFailed {
        /// Vertex-removal operation that failed.
        operation: DelaunayOperation,
        /// Human-readable target passed to the removal operation.
        target: String,
        /// Underlying removal diagnostic.
        detail: String,
    },

    /// A bistellar flip failed.
    #[error("{operation} failed on {target}: {detail}")]
    FlipFailed {
        /// Flip operation that failed.
        operation: DelaunayOperation,
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
        operation: DelaunayOperation,
        /// Human-readable target passed to the flip operation.
        target: String,
        /// Contract expected by the backend wrapper.
        expected: &'static str,
        /// Output shape or detail observed from the upstream result.
        actual: String,
    },

    /// Upstream Delaunay backend validation failed.
    #[error("Delaunay backend validation failed [{level}]: {detail}")]
    ValidationFailed {
        /// Cumulative upstream validation level being enforced.
        level: DelaunayValidationLevel,
        /// Underlying validation diagnostic.
        detail: String,
    },
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize>
    DelaunayBackend<VertexData, SimplexData, D>
{
    /// Builds a backend vertex from a coordinate slice and optional payload.
    fn build_vertex(
        coords: &[f64],
        data: Option<VertexData>,
        operation: DelaunayOperation,
    ) -> Result<RawVertex<VertexData, D>, DelaunayError> {
        let coords: [f64; D] =
            coords
                .try_into()
                .map_err(|_| DelaunayError::CoordinateDimensionMismatch {
                    actual: coords.len(),
                    expected: D,
                })?;
        for (axis, value) in coords.iter().copied().enumerate() {
            if !value.is_finite() {
                return Err(DelaunayError::NonFiniteCoordinate {
                    operation,
                    axis,
                    value,
                });
            }
        }
        data.map_or_else(
            || Vertex::try_new(coords),
            |data| Vertex::try_new_with_data(coords, data),
        )
        .map_err(|err| DelaunayError::VertexBuildFailed {
            operation,
            detail: err.to_string(),
        })
    }

    /// Finds the 2D facet handle corresponding to a live interior edge.
    fn interior_facet_for_edge(&self, edge: EdgeKey) -> Option<FacetHandle> {
        self.interior_facets_by_edge.get(&edge).copied()
    }

    /// Builds the 2D interior edge-facet lookup from current simplex adjacency.
    fn build_interior_facets_by_edge(
        dt: &RawTriangulation<VertexData, SimplexData, D>,
    ) -> HashMap<EdgeKey, FacetHandle> {
        let mut facets_by_edge = HashMap::new();
        if D != 2 {
            return facets_by_edge;
        }
        for (simplex_key, simplex) in dt.simplices() {
            let vertices = simplex.vertices();
            let Some(neighbors) = simplex.neighbors() else {
                continue;
            };

            for (facet_index, neighbor) in neighbors.enumerate() {
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
                if facet_vertices.next().is_none()
                    && let Ok(facet_index) = u8::try_from(facet_index)
                    && let Ok(edge) = dt.edge_key(v0, v1)
                    && let Ok(facet) = dt.facet_handle(simplex_key, facet_index)
                {
                    facets_by_edge.entry(edge).or_insert(facet);
                }
            }
        }

        facets_by_edge
    }

    /// Refreshes cached edge adjacency after a topology mutation succeeds.
    fn rebuild_interior_facet_index(&mut self) {
        self.interior_facets_by_edge = Self::build_interior_facets_by_edge(&self.dt);
    }

    /// Restores the raw triangulation and all caches to a saved snapshot.
    fn restore_mutation_snapshot(
        &mut self,
        dt_before: RawTriangulation<VertexData, SimplexData, D>,
        facets_before: HashMap<EdgeKey, FacetHandle>,
    ) {
        self.dt = dt_before;
        self.interior_facets_by_edge = facets_before;
    }

    /// Resolves the replacement edge produced by a flip without releasing rollback state.
    ///
    /// The upstream flip mutates the triangulation before returning its result metadata. If
    /// that metadata does not identify a live replacement edge, the whole backend mutation
    /// must be rolled back rather than publishing the changed triangulation with an error.
    fn replacement_edge_key_or_restore(
        &mut self,
        v0: VertexKey,
        v1: VertexKey,
        dt_before: RawTriangulation<VertexData, SimplexData, D>,
        facets_before: HashMap<EdgeKey, FacetHandle>,
    ) -> Result<EdgeKey, DelaunayError> {
        match self.dt.edge_key(v0, v1) {
            Ok(key) => Ok(key),
            Err(err) => {
                self.restore_mutation_snapshot(dt_before, facets_before);
                Err(DelaunayError::UnexpectedFlipOutput {
                    operation: DelaunayOperation::FlipK2,
                    target: format!("replacement edge {v0:?} -- {v1:?}"),
                    expected: "a live replacement edge after the k=2 flip",
                    actual: err.to_string(),
                })
            }
        }
    }

    /// Returns simplex keys adjacent to `vertex` from the maintained incidence relation.
    fn adjacent_simplex_keys(&self, vertex: VertexKey) -> Result<Vec<SimplexKey>, DelaunayError> {
        self.dt
            .incidence()
            .map(|incidence| incidence.adjacent_simplices(vertex).collect())
            .map_err(|err| DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Three,
                detail: err.to_string(),
            })
    }

    /// Validates embedding after a mutation without an upstream realization postcondition.
    ///
    /// High-level upstream bistellar flips already run cumulative Level 1-4
    /// realization validation transactionally and therefore do not call this
    /// helper. Other mutation paths are checked here so every successful backend
    /// edit has the same postcondition without duplicating whole-mesh scans.
    fn validate_embedding_or_restore(
        &mut self,
        dt_before: RawTriangulation<VertexData, SimplexData, D>,
        facets_before: HashMap<EdgeKey, FacetHandle>,
        operation: DelaunayOperation,
        target: impl Display,
    ) -> Result<(), DelaunayError> {
        let validation = self.validate_embedding();
        self.restore_if_embedding_invalid(validation, dt_before, facets_before, operation, target)
    }

    /// Completes embedding validation, restoring a rejected mutation on failure.
    fn restore_if_embedding_invalid(
        &mut self,
        validation: Result<(), DelaunayError>,
        dt_before: RawTriangulation<VertexData, SimplexData, D>,
        facets_before: HashMap<EdgeKey, FacetHandle>,
        operation: DelaunayOperation,
        target: impl Display,
    ) -> Result<(), DelaunayError> {
        let Err(error) = validation else {
            return Ok(());
        };
        self.restore_mutation_snapshot(dt_before, facets_before);
        Err(match error {
            DelaunayError::ValidationFailed { level, detail } => DelaunayError::ValidationFailed {
                level,
                detail: format!("{operation} produced invalid geometry for {target}: {detail}"),
            },
            other => DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Four,
                detail: format!("{operation} produced invalid geometry for {target}: {other}"),
            },
        })
    }

    /// Returns whether the keyed edge is present in the triangulation.
    fn edge_exists(&self, edge: EdgeKey) -> bool {
        let v0 = edge.v0();
        let v1 = edge.v1();
        self.dt.contains_vertex_key(v0)
            && self.dt.contains_vertex_key(v1)
            && self
                .dt
                .incident_edges(v0)
                .any(|candidate| candidate == edge)
    }

    /// Creates a Delaunay backend from an existing validated Delaunay triangulation.
    ///
    /// The input is validated with the upstream Level 1-5 Delaunay validator before
    /// the backend is returned, so public callers cannot wrap malformed or
    /// non-Delaunay connectivity.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::ValidationFailed`] if the upstream triangulation
    /// fails structural, topological, orientation, or Delaunay predicate checks.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// fn main() -> Result<(), DelaunayError> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])
    ///     .map_err(|err| DelaunayError::ValidationFailed {
    ///         level: DelaunayValidationLevel::Five,
    ///         detail: err.to_string(),
    ///     })?;
    ///
    ///     let backend = DelaunayBackend::<u32, i32, 2>::from_triangulation(dt)?;
    ///     assert_eq!(backend.vertex_count(), 3);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_triangulation(
        dt: DelaunayTriangulation<AdaptiveKernel<f64>, VertexData, SimplexData, D>,
    ) -> Result<Self, DelaunayError> {
        let interior_facets_by_edge = Self::build_interior_facets_by_edge(&dt);
        let backend = Self {
            dt,
            interior_facets_by_edge,
        };
        backend.validate_delaunay()?;
        Ok(backend)
    }

    /// Access the underlying Delaunay triangulation (read-only)
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// fn main() -> Result<(), DelaunayError> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])
    ///     .map_err(|err| DelaunayError::ValidationFailed {
    ///         level: DelaunayValidationLevel::Five,
    ///         detail: err.to_string(),
    ///     })?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt)?;
    ///     assert_eq!(backend.triangulation().number_of_vertices(), 3);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn triangulation(
        &self,
    ) -> &DelaunayTriangulation<AdaptiveKernel<f64>, VertexData, SimplexData, D> {
        &self.dt
    }

    /// Check if the triangulation is valid and satisfies the Delaunay property.
    ///
    /// Uses the upstream cumulative validation (`DelaunayTriangulation::validate`) which
    /// checks structural and topological validity (Levels 1–3), straight-line
    /// embedding validity (Level 4), and the Delaunay empty-circumsphere property
    /// (Level 5).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// fn main() -> Result<(), DelaunayError> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])
    ///     .map_err(|err| DelaunayError::ValidationFailed {
    ///         level: DelaunayValidationLevel::Five,
    ///         detail: err.to_string(),
    ///     })?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt)?;
    ///     assert!(backend.is_delaunay());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn is_delaunay(&self) -> bool {
        self.validate_delaunay().is_ok()
    }

    /// Validates the triangulation with the upstream full Delaunay validator.
    ///
    /// This delegates to [`DelaunayTriangulation::validate`], which performs
    /// the cumulative Level 1-5 checks: structural and topological validity,
    /// straight-line embedding validity, and the Delaunay empty-circumsphere
    /// predicate.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::ValidationFailed`] with the upstream diagnostic
    /// when any Level 1-5 validation check fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// fn main() -> Result<(), DelaunayError> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])
    ///     .map_err(|err| DelaunayError::ValidationFailed {
    ///         level: DelaunayValidationLevel::Five,
    ///         detail: err.to_string(),
    ///     })?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt)?;
    ///     backend.validate_delaunay()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_delaunay(&self) -> Result<(), DelaunayError> {
        self.dt
            .validate()
            .map_err(|err| DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Five,
                detail: err.to_string(),
            })
    }

    /// Validates the straight-line embedding without requiring Delaunay-ness.
    ///
    /// This delegates to the upstream cumulative Level 1-4 realization validator.
    /// It checks structural and topological validity, rejects degenerate maximal
    /// simplices, and rejects intersections outside shared faces. It deliberately
    /// omits the Level 5 empty-circumsphere predicate so evolved CDT states can be
    /// geometrically safe without remaining Delaunay.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::ValidationFailed`] with the upstream diagnostic
    /// when any Level 1-4 embedding validation check fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// fn main() -> Result<(), DelaunayError> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])
    ///     .map_err(|err| DelaunayError::ValidationFailed {
    ///         level: DelaunayValidationLevel::Five,
    ///         detail: err.to_string(),
    ///     })?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt)?;
    ///     backend.validate_embedding()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_embedding(&self) -> Result<(), DelaunayError> {
        self.dt
            .as_triangulation()
            .validate_realization()
            .map_err(|err| DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Four,
                detail: err.to_string(),
            })
    }

    /// Validates structural geometry invariants used by evolved CDT states.
    ///
    /// This delegates to the upstream triangulation validator, which performs
    /// cumulative Level 1-3 TDS, topology, and manifold checks without
    /// requiring Level 4 embedding validity or the Level 5 empty-circumsphere
    /// predicate. Use [`Self::validate_embedding`] for evolved-state geometric
    /// safety and [`Self::validate_delaunay`] for initialization-grade Level 1-5
    /// validation.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::ValidationFailed`] with the upstream diagnostic
    /// when any structural validation check fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///
    ///     assert!(backend.validate_structural().is_ok());
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_structural(&self) -> Result<(), DelaunayError> {
        self.dt
            .as_triangulation()
            .validate()
            .map_err(|err| DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Three,
                detail: err.to_string(),
            })
    }

    /// Configures global validation cadence using Delaunay's check policy.
    ///
    /// The policy is stored with checkpoints and is consumed by CDT sampling as a
    /// cadence over accepted local mutations. `None` restores the upstream
    /// end-only policy; `Some(n)` runs a full CDT evolved-state validation when
    /// the accepted-move count is a multiple of `n`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    /// use std::num::NonZeroUsize;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let mut backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///
    ///     backend.set_delaunay_check_interval(NonZeroUsize::new(16));
    ///     backend.set_delaunay_check_interval(None);
    ///     Ok(())
    /// }
    /// ```
    pub fn set_delaunay_check_interval(&mut self, interval: Option<NonZeroUsize>) {
        let policy = interval.map_or(DelaunayCheckPolicy::EndOnly, DelaunayCheckPolicy::EveryN);
        self.dt.set_delaunay_check_policy(policy);
    }

    /// Returns `true` when the current Delaunay check policy is due.
    ///
    /// CDT passes the accepted local-mutation count here to reuse the same
    /// `EveryN` cadence semantics as the upstream Delaunay crate.
    #[must_use]
    pub(crate) fn should_check_delaunay_after(&self, completed_mutations: u64) -> bool {
        usize::try_from(completed_mutations)
            .is_ok_and(|count| self.dt.delaunay_check_policy().should_check(count))
    }

    /// Returns the high-level topology kind (`Euclidean`, `Toroidal`, etc.) of the
    /// underlying triangulation.
    ///
    /// This exposes the [`GlobalTopology`]
    /// metadata attached by [`DelaunayTriangulationBuilder`](delaunay::DelaunayTriangulationBuilder)
    /// at construction time.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let _kind = backend.topology_kind();
    ///     Ok(())
    /// }
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
    /// use causal_triangulations::CdtResult;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_toroidal_cdt(3, 3)?;
    ///     assert_eq!(tri.geometry().periodic_domain(), Some([3.0, 3.0]));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn periodic_domain(&self) -> Option<[f64; D]> {
        match self.dt.global_topology() {
            GlobalTopology::Toroidal { domain, .. } => Some(*domain.periods()),
            GlobalTopology::Euclidean | GlobalTopology::Spherical | GlobalTopology::Hyperbolic => {
                None
            }
        }
    }

    /// Computes the upstream topology-aware barycenter of a live face.
    ///
    /// Periodic triangulations use the simplex's stored lift offsets before
    /// canonicalizing the result back into the fundamental domain.
    pub(crate) fn face_barycenter(
        &self,
        face: &DelaunayFaceHandle,
    ) -> Result<[f64; D], DelaunayError> {
        let point = self
            .dt
            .simplex_barycenter(face.key)
            .map_err(|error| match error {
                SimplexBarycenterError::MissingSimplex { .. } => {
                    DelaunayError::InvalidFace { key: face.key }
                }
                error => DelaunayError::FaceBarycenterFailed {
                    key: face.key,
                    detail: error.to_string(),
                },
            })?;
        Ok(*point.coords())
    }

    /// Returns Delaunay's stable, detached mesh-interchange export.
    pub(crate) fn mesh_export(&self) -> Result<DelaunayMeshExport<D>, MeshExportError> {
        self.dt.to_mesh_export()
    }

    /// Returns the stable Delaunay UUID used for a vertex in mesh exports.
    pub(crate) fn vertex_export_id(
        &self,
        vertex: &DelaunayVertexHandle,
    ) -> Result<String, DelaunayError> {
        self.dt
            .vertex(vertex.key)
            .map(|vertex| vertex.uuid().to_string())
            .ok_or(DelaunayError::InvalidVertex { key: vertex.key })
    }

    /// Returns the vertex payload for `key`, if present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let key = backend
    ///         .triangulation()
    ///         .vertices()
    ///         .next()
    ///         .map(|(key, _)| key)
    ///         .ok_or_else(|| CdtError::ValidationFailed {
    ///             check: CdtValidationCheck::Geometry,
    ///             failure: CdtValidationFailure::BackendGeometry {
    ///                 detail: "validated triangle should contain a vertex".to_string(),
    ///             },
    ///         })?;
    ///     assert!(backend.vertex_data_by_key(key).is_some());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn vertex_data_by_key(&self, key: VertexKey) -> Option<VertexData> {
        self.dt.vertex(key)?.data().copied()
    }

    /// Returns the simplex payload for `key`, if present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let key = backend
    ///         .triangulation()
    ///         .simplices()
    ///         .next()
    ///         .map(|(key, _)| key)
    ///         .ok_or_else(|| CdtError::ValidationFailed {
    ///             check: CdtValidationCheck::Geometry,
    ///             failure: CdtValidationFailure::BackendGeometry {
    ///                 detail: "validated triangle should contain a simplex".to_string(),
    ///             },
    ///         })?;
    ///     assert_eq!(backend.simplex_data_by_key(key), None);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn simplex_data_by_key(&self, key: SimplexKey) -> Option<SimplexData> {
        self.dt.simplex(key)?.data().copied()
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
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{
    ///     BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let mut backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let key = backend
    ///         .triangulation()
    ///         .vertices()
    ///         .next()
    ///         .map(|(key, _)| key)
    ///         .ok_or_else(|| CdtError::ValidationFailed {
    ///             check: CdtValidationCheck::Geometry,
    ///             failure: CdtValidationFailure::BackendGeometry {
    ///                 detail: "validated triangle should contain a vertex".to_string(),
    ///             },
    ///         })?;
    ///     let previous = backend.set_vertex_data_by_key(key, Some(3)).map_err(|err| {
    ///         CdtError::BackendMutationFailed {
    ///             operation: BackendMutationOperation::SetVertexDataByKey,
    ///             target: format!("vertex {key:?}"),
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     assert!(previous.is_some());
    ///     assert_eq!(backend.vertex_data_by_key(key), Some(3));
    ///     Ok(())
    /// }
    /// ```
    pub fn set_vertex_data_by_key(
        &mut self,
        key: VertexKey,
        data: Option<VertexData>,
    ) -> Result<Option<VertexData>, DelaunayError> {
        self.dt
            .set_vertex_data(key, data)
            .map_err(|_| DelaunayError::InvalidVertex { key })
    }

    /// Sets the optional payload for a simplex identified by `key`.
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
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{
    ///     BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0_u32),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let mut backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let key = backend
    ///         .triangulation()
    ///         .simplices()
    ///         .next()
    ///         .map(|(key, _)| key)
    ///         .ok_or_else(|| CdtError::ValidationFailed {
    ///             check: CdtValidationCheck::Geometry,
    ///             failure: CdtValidationFailure::BackendGeometry {
    ///                 detail: "validated triangle should contain a simplex".to_string(),
    ///             },
    ///         })?;
    ///     let previous = backend.set_simplex_data_by_key(key, Some(1)).map_err(|err| {
    ///         CdtError::BackendMutationFailed {
    ///             operation: BackendMutationOperation::SetSimplexDataByKey,
    ///             target: format!("simplex {key:?}"),
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     assert_eq!(previous, None);
    ///     assert_eq!(backend.simplex_data_by_key(key), Some(1));
    ///     Ok(())
    /// }
    /// ```
    pub fn set_simplex_data_by_key(
        &mut self,
        key: SimplexKey,
        data: Option<SimplexData>,
    ) -> Result<Option<SimplexData>, DelaunayError> {
        self.dt
            .set_simplex_data(key, data)
            .map_err(|_| DelaunayError::InvalidFace { key })
    }
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> GeometryBackend
    for DelaunayBackend<VertexData, SimplexData, D>
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

impl<VertexData: DataType, SimplexData: DataType, const D: usize> TriangulationQuery
    for DelaunayBackend<VertexData, SimplexData, D>
{
    fn vertex_count(&self) -> usize {
        self.dt.number_of_vertices()
    }

    fn edge_count(&self) -> usize {
        self.dt.as_triangulation().number_of_edges()
    }

    fn face_count(&self) -> usize {
        self.dt.number_of_simplices()
    }

    fn dimension(&self) -> usize {
        D
    }

    fn vertices(&self) -> impl Iterator<Item = Self::VertexHandle> + '_ {
        self.dt
            .vertices()
            .map(|(key, _)| DelaunayVertexHandle { key })
    }

    fn edges(&self) -> impl Iterator<Item = Self::EdgeHandle> + '_ {
        self.dt.edges().map(|key| DelaunayEdgeHandle { key })
    }

    fn faces(&self) -> impl Iterator<Item = Self::FaceHandle> + '_ {
        self.dt
            .simplices()
            .map(|(key, _)| DelaunayFaceHandle { key })
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
            .simplex_vertices(face.key)
            .map_err(|_| DelaunayError::InvalidFace { key: face.key })?;
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
        let contains_v0 = self.dt.contains_vertex_key(v0);
        let contains_v1 = self.dt.contains_vertex_key(v1);
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
            self.dt.number_of_simplices(),
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
        let face_0 = facet.simplex_key();
        let facet_index = usize::from(facet.facet_index());
        let Some(simplex_0) = self.dt.simplex(face_0) else {
            return Err(DelaunayError::InvalidFace { key: face_0 });
        };
        let vertices_0 = simplex_0.vertices();
        if vertices_0.len() != 3 || facet_index >= vertices_0.len() {
            return Ok(None);
        }
        let Some(face_1) = simplex_0
            .neighbors()
            .and_then(|mut neighbors| neighbors.nth(facet_index).flatten())
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

        let vertices_1 = self
            .dt
            .simplex_vertices(face_1)
            .map_err(|_| DelaunayError::InvalidFace { key: face_1 })?;
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
        if !self.dt.contains_vertex_key(vertex.key) {
            return Err(DelaunayError::InvalidVertex { key: vertex.key });
        }
        Ok(self
            .adjacent_simplex_keys(vertex.key)?
            .into_iter()
            .map(|key| DelaunayFaceHandle { key })
            .collect())
    }

    fn incident_edges(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::EdgeHandle>, Self::Error> {
        if !self.dt.contains_vertex_key(vertex.key) {
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
        if !self.dt.contains_simplex(face.key) {
            return Err(DelaunayError::InvalidFace { key: face.key });
        }
        Ok(self
            .dt
            .simplex_neighbors(face.key)
            .map(|key| DelaunayFaceHandle { key })
            .collect())
    }

    fn is_valid(&self) -> bool {
        // Structural minimum: must have enough vertices and at least one simplex.
        if self.dt.number_of_vertices() <= D || self.dt.number_of_simplices() == 0 {
            return false;
        }

        // Use structural/topological validation via the
        // Triangulation layer (neighbor pointers, Euler characteristic, coherent
        // orientation) without Level 4 embedding or Level 5 Delaunay checks.
        // Use validate_embedding() for Levels 1–4 and is_delaunay() for Levels 1–5.
        self.dt.as_triangulation().validate().is_ok()
    }
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> TriangulationMut
    for DelaunayBackend<VertexData, SimplexData, D>
{
    fn insert_vertex(
        &mut self,
        coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error> {
        let vertex = Self::build_vertex(coords, None, DelaunayOperation::InsertVertex)?;
        let dt_before = self.dt.clone();
        let facets_before = self.interior_facets_by_edge.clone();
        let key = match self.dt.insert_vertex(vertex) {
            Ok(key) => key,
            Err(err) => {
                self.restore_mutation_snapshot(dt_before, facets_before);
                return Err(DelaunayError::InsertionFailed {
                    operation: DelaunayOperation::InsertVertex,
                    coordinates: coords.to_vec(),
                    detail: err.to_string(),
                });
            }
        };
        self.rebuild_interior_facet_index();
        self.validate_embedding_or_restore(
            dt_before,
            facets_before,
            DelaunayOperation::InsertVertex,
            format!("{coords:?}"),
        )?;
        Ok(DelaunayVertexHandle { key })
    }

    fn remove_vertex(&mut self, vertex: Self::VertexHandle) -> Result<(), Self::Error> {
        if !self.dt.contains_vertex_key(vertex.key) {
            return Err(DelaunayError::InvalidVertex { key: vertex.key });
        }

        let dt_before = self.dt.clone();
        let facets_before = self.interior_facets_by_edge.clone();
        let inverse_k1 = self.dt.can_flip_k1_remove(vertex.key).is_ok();
        let removal = if inverse_k1 {
            self.dt
                .flip_k1_remove(vertex.key)
                .map(|_| ())
                .map_err(|err| err.to_string())
        } else {
            self.dt
                .delete_vertex(vertex.key)
                .map(|_| ())
                .map_err(|err| err.to_string())
        };
        match removal {
            Ok(()) => {}
            Err(err) => {
                self.restore_mutation_snapshot(dt_before, facets_before);
                return Err(DelaunayError::RemovalFailed {
                    operation: if inverse_k1 {
                        DelaunayOperation::FlipK1Remove
                    } else {
                        DelaunayOperation::RemoveVertex
                    },
                    target: format!("vertex {:?}", vertex.key),
                    detail: err,
                });
            }
        }
        self.rebuild_interior_facet_index();
        if !inverse_k1 {
            self.validate_embedding_or_restore(
                dt_before,
                facets_before,
                DelaunayOperation::RemoveVertex,
                format!("vertex {:?}", vertex.key),
            )?;
        }
        Ok(())
    }

    fn move_vertex(
        &mut self,
        _vertex: Self::VertexHandle,
        _new_coords: &[Self::Coordinate],
    ) -> Result<(), Self::Error> {
        // TODO: Implement vertex movement.
        Err(DelaunayError::NotImplemented {
            operation: DelaunayOperation::MoveVertex,
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
                    reason: NonFlippableEdgeReason::NotInteriorFacet,
                }
            })?
        } else {
            return Err(DelaunayError::InvalidEdge {
                v0: edge.key.v0(),
                v1: edge.key.v1(),
            });
        };
        let dt_before = self.dt.clone();
        let facets_before = self.interior_facets_by_edge.clone();
        let info = match self.dt.flip_k2(facet) {
            Ok(info) => info,
            Err(err) => {
                self.restore_mutation_snapshot(dt_before, facets_before);
                return Err(DelaunayError::FlipFailed {
                    operation: DelaunayOperation::FlipK2,
                    target: format!(
                        "edge {:?} -- {:?} via facet {:?}",
                        edge.key.v0(),
                        edge.key.v1(),
                        facet
                    ),
                    detail: err.to_string(),
                });
            }
        };
        let mut inserted = info.inserted_face_vertices.iter().copied();
        let Some(v0) = inserted.next() else {
            self.restore_mutation_snapshot(dt_before, facets_before);
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                expected: "exactly two inserted-face vertices for the replacement edge",
                actual: "0 inserted-face vertices".to_string(),
            });
        };
        let Some(v1) = inserted.next() else {
            self.restore_mutation_snapshot(dt_before, facets_before);
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                expected: "exactly two inserted-face vertices for the replacement edge",
                actual: "1 inserted-face vertices".to_string(),
            });
        };
        if let Some(extra) = inserted.next() {
            self.restore_mutation_snapshot(dt_before, facets_before);
            let actual = 3 + inserted.count();
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                expected: "exactly two inserted-face vertices for the replacement edge",
                actual: format!("{actual} inserted-face vertices including unexpected {extra:?}"),
            });
        }
        let replacement_edge =
            self.replacement_edge_key_or_restore(v0, v1, dt_before, facets_before)?;
        self.rebuild_interior_facet_index();
        // `flip_k2` commits only after upstream cumulative realization validation.
        let affected_faces = info
            .new_simplices
            .iter()
            .copied()
            .map(|key| DelaunayFaceHandle { key })
            .collect();
        Ok(FlipResult::new(
            DelaunayEdgeHandle {
                key: replacement_edge,
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
        if !self.dt.contains_simplex(face.key) {
            return Err(DelaunayError::InvalidFace { key: face.key });
        }

        let vertex = Self::build_vertex(point, None, DelaunayOperation::SubdivideFace)?;
        let dt_before = self.dt.clone();
        let facets_before = self.interior_facets_by_edge.clone();
        let info = match self.dt.flip_k1_insert(face.key, vertex) {
            Ok(info) => info,
            Err(err) => {
                self.restore_mutation_snapshot(dt_before, facets_before);
                return Err(DelaunayError::FlipFailed {
                    operation: DelaunayOperation::FlipK1Insert,
                    target: format!("face {:?} at point {:?}", face.key, point),
                    detail: err.to_string(),
                });
            }
        };
        let mut inserted = info.inserted_face_vertices.iter().copied();
        let Some(new_vertex) = inserted.next() else {
            self.restore_mutation_snapshot(dt_before, facets_before);
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK1Insert,
                target: format!("face {:?} at point {:?}", face.key, point),
                expected: "exactly one inserted-face vertex for the inserted point",
                actual: "0 inserted-face vertices".to_string(),
            });
        };
        if let Some(extra) = inserted.next() {
            self.restore_mutation_snapshot(dt_before, facets_before);
            let actual = 2 + inserted.count();
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK1Insert,
                target: format!("face {:?} at point {:?}", face.key, point),
                expected: "exactly one inserted-face vertex for the inserted point",
                actual: format!("{actual} inserted-face vertices including unexpected {extra:?}"),
            });
        }
        self.rebuild_interior_facet_index();
        // `flip_k1_insert` commits only after upstream cumulative realization validation.
        Ok(SubdivisionResult::new(
            DelaunayVertexHandle { key: new_vertex },
            info.new_simplices
                .iter()
                .copied()
                .map(|key| DelaunayFaceHandle { key })
                .collect(),
            face,
        ))
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        Err(DelaunayError::NotImplemented {
            operation: DelaunayOperation::Clear,
        })
    }

    fn reserve_capacity(&mut self, _vertices: usize, _faces: usize) -> Result<(), Self::Error> {
        Err(DelaunayError::NotImplemented {
            operation: DelaunayOperation::ReserveCapacity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::DelaunayBackend2D;
    use crate::geometry::generators::{
        DelaunayTriangulation2D, build_delaunay2_from_simplices, build_delaunay2_with_data,
        generate_delaunay2, random_delaunay2, seeded_delaunay2,
    };
    use crate::{CdtTriangulation, CdtValidationProfile};
    use approx::assert_relative_eq;
    use delaunay::DelaunayRepairPolicy;
    use delaunay::prelude::construction::{ConstructionOptions, DelaunayTriangulationBuilder};
    use serde_json::{Value, error::Category};
    use slotmap::KeyData;
    use std::assert_matches;
    use std::collections::HashSet;
    use std::num::NonZeroU32;

    /// Wraps generated test fixtures through the public checked constructor.
    fn validated_backend(dt: DelaunayTriangulation2D) -> DelaunayBackend2D {
        DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangulation should validate")
    }

    /// Builds an embedding-valid explicit quad whose chosen diagonal is not Delaunay.
    fn embedded_non_delaunay_backend() -> DelaunayBackend2D {
        let vertices = [
            Vertex::try_new_with_data([0.0, 0.0], 0_u32).expect("valid vertex"),
            Vertex::try_new_with_data([4.0, 0.0], 0).expect("valid vertex"),
            Vertex::try_new_with_data([4.0, 1.0], 1).expect("valid vertex"),
            Vertex::try_new_with_data([1.0, 1.0], 1).expect("valid vertex"),
        ];
        let simplices = vec![vec![0, 1, 2], vec![0, 2, 3]];
        let dt =
            DelaunayTriangulationBuilder::try_from_vertices_and_simplices(&vertices, &simplices)
                .expect("valid explicit simplex indices")
                .simplex_data_type::<i32>()
                .construction_options(
                    ConstructionOptions::default().without_final_delaunay_enforcement(),
                )
                .build()
                .expect("non-Delaunay quad should pass Levels 1-4 embedding validation");
        let interior_facets_by_edge = DelaunayBackend2D::build_interior_facets_by_edge(&dt);
        DelaunayBackend {
            dt,
            interior_facets_by_edge,
        }
    }

    /// `serde_json` wraps custom visitor failures as data errors; assert that
    /// structured category first, then keep detail matching in one place.
    fn assert_json_data_error(error: &serde_json::Error, expected_details: &[&str]) {
        assert_eq!(error.classify(), Category::Data);
        let message = error.to_string();
        for expected_detail in expected_details {
            assert!(
                message.contains(expected_detail),
                "deserialization error {message:?} did not contain {expected_detail:?}"
            );
        }
    }

    /// `serde::de::value::Error` does not expose categories, so centralize the
    /// remaining custom-message assertions for direct conversion tests.
    fn assert_value_deserialization_error(
        error: &serde::de::value::Error,
        expected_details: &[&str],
    ) {
        let message = error.to_string();
        for expected_detail in expected_details {
            assert!(
                message.contains(expected_detail),
                "value deserialization error {message:?} did not contain {expected_detail:?}"
            );
        }
    }

    /// Rewrites a serialized convex-quad TDS to use the non-Delaunay diagonal so
    /// backend deserialization must fail during checked reconstruction.
    fn set_non_delaunay_quad_diagonal(value: &mut Value) {
        let tds = value
            .get("tds")
            .expect("serialized backend should contain a TDS");
        let vertices = tds
            .get("vertices")
            .and_then(Value::as_array)
            .expect("serialized TDS should contain vertices");
        let find_vertex_uuid = |target: [f64; 2]| {
            vertices
                .iter()
                .find_map(|vertex| {
                    let point = vertex.get("point")?.as_array()?;
                    let coords = [point.first()?.as_f64()?, point.get(1)?.as_f64()?];
                    if coords.map(f64::to_bits) == target.map(f64::to_bits) {
                        vertex.get("uuid")?.as_str().map(str::to_string)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("serialized quad should contain vertex {target:?}"))
        };
        let v0 = find_vertex_uuid([0.0, 0.0]);
        let v1 = find_vertex_uuid([4.0, 0.0]);
        let v2 = find_vertex_uuid([4.0, 2.0]);
        let v3 = find_vertex_uuid([1.0, 2.0]);

        let simplex_uuids: Vec<_> = tds
            .get("simplices")
            .and_then(Value::as_array)
            .expect("serialized TDS should contain simplices")
            .iter()
            .filter_map(|simplex| simplex.get("uuid")?.as_str())
            .map(str::to_string)
            .collect();
        assert_eq!(
            simplex_uuids.len(),
            2,
            "convex quad fixture should serialize exactly two simplices"
        );

        let simplex_vertices = value
            .get_mut("tds")
            .and_then(|tds| tds.get_mut("simplex_vertices"))
            .and_then(Value::as_object_mut)
            .expect("serialized TDS should contain simplex_vertices");
        simplex_vertices.insert(
            simplex_uuids[0].clone(),
            Value::Array(vec![
                Value::String(v0.clone()),
                Value::String(v1),
                Value::String(v2.clone()),
            ]),
        );
        simplex_vertices.insert(
            simplex_uuids[1].clone(),
            Value::Array(vec![
                Value::String(v0),
                Value::String(v2),
                Value::String(v3),
            ]),
        );

        let simplex_neighbors = value
            .get_mut("tds")
            .and_then(|tds| tds.get_mut("simplex_neighbors"))
            .and_then(Value::as_object_mut)
            .expect("serialized TDS should contain simplex_neighbors");
        simplex_neighbors.insert(
            simplex_uuids[0].clone(),
            Value::Array(vec![
                Value::Null,
                Value::String(simplex_uuids[1].clone()),
                Value::Null,
            ]),
        );
        simplex_neighbors.insert(
            simplex_uuids[1].clone(),
            Value::Array(vec![
                Value::Null,
                Value::Null,
                Value::String(simplex_uuids[0].clone()),
            ]),
        );
    }

    #[test]
    fn toroidal_topology_deserialization_rejects_invalid_periods() {
        for period in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let topology = SerializableGlobalTopology::Toroidal {
                domain: vec![period, 1.0],
                mode: SerializableToroidalConstructionMode::Explicit,
            };

            let error = topology
                .into_global_topology::<2, serde::de::value::Error>()
                .expect_err("invalid toroidal period should fail deserialization");

            assert_value_deserialization_error(&error, &["invalid toroidal period"]);
        }
    }

    #[test]
    fn toroidal_topology_deserialization_rejects_domain_length_mismatch() {
        let topology = SerializableGlobalTopology::Toroidal {
            domain: vec![1.0],
            mode: SerializableToroidalConstructionMode::Explicit,
        };

        let error = topology
            .into_global_topology::<2, serde::de::value::Error>()
            .expect_err("wrong-dimensional toroidal domain should fail deserialization");

        assert_value_deserialization_error(
            &error,
            &["toroidal domain length mismatch", "got 1", "expected 2"],
        );
    }

    #[test]
    fn toroidal_topology_deserialization_rejects_legacy_canonicalized_mode() {
        let topology = SerializableGlobalTopology::Toroidal {
            domain: vec![1.0, 1.0],
            mode: SerializableToroidalConstructionMode::Canonicalized,
        };

        let error = topology
            .into_global_topology::<2, serde::de::value::Error>()
            .expect_err("legacy canonicalized topology must fail deserialization");

        assert_value_deserialization_error(
            &error,
            &[
                "Canonicalized",
                "not semantically equivalent",
                "PeriodicImagePoint",
            ],
        );
    }

    #[test]
    fn backend_deserialization_rejects_zero_delaunay_check_interval() {
        let dt =
            build_delaunay2_with_data(&[([0.0, 0.0], 0_u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
                .expect("labeled triangle should build");
        let backend = validated_backend(dt);
        let json = serde_json::to_string(&backend).expect("backend should serialize");
        let invalid_json = json.replace(
            r#""delaunay_check_policy":"EndOnly""#,
            r#""delaunay_check_policy":{"EveryN":0}"#,
        );

        let error = serde_json::from_str::<DelaunayBackend2D>(&invalid_json)
            .expect_err("zero validation cadence must be rejected during deserialization");

        assert_json_data_error(&error, &["delaunay check interval must be non-zero"]);
    }

    #[test]
    fn delaunay_operation_display_covers_all_operations() {
        let cases = [
            (DelaunayOperation::InsertVertex, "insert_vertex"),
            (DelaunayOperation::MoveVertex, "move_vertex"),
            (DelaunayOperation::RemoveVertex, "remove_vertex"),
            (DelaunayOperation::SubdivideFace, "subdivide_face"),
            (DelaunayOperation::FlipK1Remove, "flip_k1_remove"),
            (DelaunayOperation::FlipK1Insert, "flip_k1_insert"),
            (DelaunayOperation::FlipK2, "flip_k2"),
            (DelaunayOperation::Clear, "clear"),
            (DelaunayOperation::ReserveCapacity, "reserve_capacity"),
        ];

        for (operation, expected) in cases {
            assert_eq!(operation.to_string(), expected);
        }
    }

    #[test]
    fn non_flippable_edge_reason_display_covers_all_reasons() {
        assert_eq!(
            NonFlippableEdgeReason::NotInteriorFacet.to_string(),
            "edge is not an interior 2D facet shared by two simplices"
        );
    }

    #[test]
    fn test_delaunay_mutation_error_messages_preserve_context() {
        let insertion = DelaunayError::InsertionFailed {
            operation: DelaunayOperation::InsertVertex,
            coordinates: vec![0.25, 0.75],
            detail: "duplicate point".to_string(),
        };
        assert_eq!(
            insertion.to_string(),
            "insert_vertex insertion failed at [0.25, 0.75]: duplicate point"
        );

        let flip = DelaunayError::FlipFailed {
            operation: DelaunayOperation::FlipK2,
            target: "edge VertexKey(1v1) -- VertexKey(2v1)".to_string(),
            detail: "non-convex cavity".to_string(),
        };
        assert_eq!(
            flip.to_string(),
            "flip_k2 failed on edge VertexKey(1v1) -- VertexKey(2v1): non-convex cavity"
        );

        let removal = DelaunayError::RemovalFailed {
            operation: DelaunayOperation::RemoveVertex,
            target: "vertex VertexKey(1v1)".to_string(),
            detail: "cavity is not retriangulable".to_string(),
        };
        assert_eq!(
            removal.to_string(),
            "remove_vertex failed on vertex VertexKey(1v1): cavity is not retriangulable"
        );

        let malformed = DelaunayError::UnexpectedFlipOutput {
            operation: DelaunayOperation::FlipK2,
            target: "edge VertexKey(1v1) -- VertexKey(2v1)".to_string(),
            expected: "exactly two inserted-face vertices for the replacement edge",
            actual: "1 inserted-face vertices".to_string(),
        };
        assert_eq!(
            malformed.to_string(),
            "flip_k2 returned unexpected output for edge VertexKey(1v1) -- VertexKey(2v1): expected exactly two inserted-face vertices for the replacement edge, got 1 inserted-face vertices"
        );

        let malformed_insert = DelaunayError::UnexpectedFlipOutput {
            operation: DelaunayOperation::FlipK1Insert,
            target: "face SimplexKey(3v1) at point [0.5, 0.5]".to_string(),
            expected: "exactly one inserted-face vertex for the inserted point",
            actual: "2 inserted-face vertices including unexpected VertexKey(4v1)".to_string(),
        };
        assert_eq!(
            malformed_insert.to_string(),
            "flip_k1_insert returned unexpected output for face SimplexKey(3v1) at point [0.5, 0.5]: expected exactly one inserted-face vertex for the inserted point, got 2 inserted-face vertices including unexpected VertexKey(4v1)"
        );

        let validation = DelaunayError::ValidationFailed {
            level: DelaunayValidationLevel::Three,
            detail: "orientation check failed".to_string(),
        };
        assert_eq!(
            validation.to_string(),
            "Delaunay backend validation failed [Level 1-3]: orientation check failed"
        );
    }

    #[test]
    fn test_is_delaunay_various_sizes() {
        // is_delaunay() should pass for valid triangulations of all sizes
        for n in [3, 4, 10, 20] {
            let dt = random_delaunay2(n, (0.0, 10.0));
            let backend = validated_backend(dt);
            assert!(
                backend.is_delaunay(),
                "Triangulation with {n} vertices should satisfy Delaunay property"
            );
        }
    }

    #[test]
    fn test_is_valid_and_is_delaunay_consistency() {
        // is_delaunay (Levels 1–5) implies embedding validity (Levels 1–4)
        // and structural validity (Levels 1–3).
        let dt = random_delaunay2(5, (0.0, 10.0));
        let backend = validated_backend(dt);

        assert!(backend.is_valid(), "Triangulation should be valid");
        backend
            .validate_delaunay()
            .expect("full upstream Level 1-5 validation should pass");
        assert!(
            backend.is_delaunay(),
            "Valid Delaunay triangulation should pass is_delaunay"
        );
    }

    #[test]
    fn embedding_validation_accepts_non_delaunay_straight_line_realization() {
        let backend = embedded_non_delaunay_backend();

        backend
            .validate_structural()
            .expect("explicit quad should pass Levels 1-3 structural validation");
        backend
            .validate_embedding()
            .expect("explicit quad should pass Level 4 embedding validation");
        assert_matches!(
            backend.validate_delaunay(),
            Err(DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Five,
                ..
            })
        );

        let mut initial_triangulation = CdtTriangulation::try_new(backend.clone(), 2, 2)
            .expect("embedding-valid quad should enter unfoliated CDT state");
        initial_triangulation
            .assign_foliation_by_y(NonZeroU32::new(2).expect("fixture has two slices"))
            .expect("quad labels should form a strict causal foliation");
        assert_matches!(
            initial_triangulation.validate_initial_delaunay_cdt(),
            Err(crate::CdtError::DelaunayValidationFailed {
                level: DelaunayValidationLevel::Five,
                ..
            })
        );

        let mut triangulation = CdtTriangulation::try_new(backend, 2, 2)
            .expect("embedding-valid quad should enter unfoliated CDT state");
        triangulation
            .assign_foliation_by_y(NonZeroU32::new(2).expect("fixture has two slices"))
            .expect("quad labels should form a strict causal foliation");
        triangulation
            .validate()
            .expect("default validation should use the evolved profile");
        triangulation
            .validate_with_profile(CdtValidationProfile::Evolved)
            .expect("evolved CDT validation should not require Level 5 Delaunay-ness");
        assert_matches!(
            triangulation.validate_with_profile(CdtValidationProfile::StrictDelaunay),
            Err(crate::CdtError::DelaunayValidationFailed {
                level: DelaunayValidationLevel::Five,
                ..
            })
        );
    }

    #[test]
    fn embedding_validation_errors_restore_mutation_snapshot() {
        let validation_errors = [
            (
                DelaunayError::ValidationFailed {
                    level: DelaunayValidationLevel::Four,
                    detail: "non-adjacent simplices intersect".to_string(),
                },
                "insert_vertex produced invalid geometry for [0.25, 0.25]: non-adjacent simplices intersect",
            ),
            (
                DelaunayError::NotImplemented {
                    operation: DelaunayOperation::ReserveCapacity,
                },
                "insert_vertex produced invalid geometry for [0.25, 0.25]: not implemented: reserve_capacity",
            ),
        ];

        for (validation_error, expected_detail) in validation_errors {
            let dt =
                build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 0)])
                    .expect("triangle should build");
            let mut backend = validated_backend(dt);
            let serialized_before =
                serde_json::to_value(&backend).expect("backend should serialize");
            let dt_before = backend.dt.clone();
            let facets_before = backend.interior_facets_by_edge.clone();
            let expected_facets = facets_before.clone();
            backend
                .insert_vertex(&[0.25, 0.25])
                .expect("inside-point insertion should commit");

            backend
                .validate_structural()
                .expect("inside-point insertion should preserve structural validity");
            backend
                .validate_embedding()
                .expect("inside-point insertion should preserve the embedding");
            assert_ne!(
                serde_json::to_value(&backend).expect("mutated backend should serialize"),
                serialized_before,
                "test setup should mutate the triangulation before injecting failure"
            );

            let error = backend
                .restore_if_embedding_invalid(
                    Err(validation_error),
                    dt_before,
                    facets_before,
                    DelaunayOperation::InsertVertex,
                    "[0.25, 0.25]".to_string(),
                )
                .expect_err("failed embedding validation should reject the mutation");

            assert_matches!(
                error,
                DelaunayError::ValidationFailed {
                    level: DelaunayValidationLevel::Four,
                    detail,
                } if detail == expected_detail
            );
            assert_eq!(
                serde_json::to_value(&backend).expect("restored backend should serialize"),
                serialized_before
            );
            assert_eq!(backend.interior_facets_by_edge, expected_facets);
        }
    }

    #[test]
    fn test_is_delaunay_minimal_triangulation() {
        // Test with minimal triangulation (3 vertices)
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

        // Use a high-generation key that cannot exist in the triangulation's slotmap
        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.vertex_coordinates(&invalid_handle).unwrap_err();
        assert_matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key);
    }

    #[test]
    fn test_face_vertices() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

        let bogus_key = SimplexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };
        let err = backend.face_vertices(&invalid_handle).unwrap_err();
        assert_matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key);
    }

    #[test]
    fn face_barycenter_matches_euclidean_triangle_centroid() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1)])
            .expect("triangle fixture should build");
        let backend = validated_backend(dt);
        let face = backend
            .faces()
            .next()
            .expect("triangle fixture should contain a face");

        let point = backend
            .face_barycenter(&face)
            .expect("Euclidean face barycenter should resolve");

        assert_relative_eq!(point[0], 1.0 / 3.0, epsilon = 1e-15);
        assert_relative_eq!(point[1], 1.0 / 3.0, epsilon = 1e-15);
    }

    #[test]
    fn face_barycenter_rejects_invalid_handle() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1)])
            .expect("triangle fixture should build");
        let backend = validated_backend(dt);
        let bogus_key = SimplexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };

        let error = backend.face_barycenter(&invalid_handle).unwrap_err();

        assert_matches!(error, DelaunayError::InvalidFace { key } if key == bogus_key);
    }

    #[test]
    fn face_barycenter_lifts_periodic_simplex_before_averaging() {
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("toroidal fixture should build");
        let backend = triangulation.geometry();
        let domain = backend
            .periodic_domain()
            .expect("toroidal fixture should expose its periodic domain");

        let (face, coordinates) = backend
            .faces()
            .find_map(|face| {
                let vertices = backend.face_vertices(&face).ok()?;
                let coordinates: Vec<[f64; 2]> = vertices
                    .into_iter()
                    .map(|vertex| {
                        let coordinates = backend.vertex_coordinates(&vertex).ok()?;
                        let [x, y] = coordinates.as_slice() else {
                            return None;
                        };
                        Some([*x, *y])
                    })
                    .collect::<Option<_>>()?;
                let coordinates: [[f64; 2]; 3] = coordinates.try_into().ok()?;
                let crosses_seam = (0..2).any(|axis| {
                    let minimum = coordinates
                        .iter()
                        .map(|point| point[axis])
                        .fold(f64::INFINITY, f64::min);
                    let maximum = coordinates
                        .iter()
                        .map(|point| point[axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    maximum - minimum > domain[axis] / 2.0
                });
                crosses_seam.then_some((face, coordinates))
            })
            .expect("periodic fixture should contain a simplex crossing a domain seam");

        let reference = coordinates[0];
        let mut expected = [0.0; 2];
        for axis in 0..2 {
            let period = domain[axis];
            expected[axis] = coordinates
                .iter()
                .map(|point| {
                    let delta = point[axis] - reference[axis];
                    if delta > period / 2.0 {
                        point[axis] - period
                    } else if delta < -period / 2.0 {
                        point[axis] + period
                    } else {
                        point[axis]
                    }
                })
                .sum::<f64>()
                / 3.0;
            expected[axis] = expected[axis].rem_euclid(period);
        }

        let point = backend
            .face_barycenter(&face)
            .expect("periodic face barycenter should resolve");

        assert_relative_eq!(point[0], expected[0], epsilon = 1e-12);
        assert_relative_eq!(point[1], expected[1], epsilon = 1e-12);
    }

    #[test]
    fn test_edge_endpoints() {
        let dt = random_delaunay2(4, (0.0, 10.0));
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

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
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1)])
            .expect("triangle fixture should build");
        let mut backend = validated_backend(dt);
        let face = backend
            .faces()
            .next()
            .expect("triangle fixture should contain a face");
        let subdivision = backend
            .subdivide_face(face, &[0.25, 0.25])
            .expect("triangle subdivision should succeed");
        let invalid_handle = backend
            .incident_edges(&subdivision.new_vertex)
            .expect("inserted vertex should have incident edges")
            .into_iter()
            .next()
            .expect("inserted vertex should have at least one incident edge");
        backend
            .remove_vertex(subdivision.new_vertex)
            .expect("inserted vertex should be removable");
        assert!(
            backend.edge_endpoints(&invalid_handle).is_none(),
            "Stale edge handle should return None"
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
        let backend = validated_backend(dt);

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
        assert!(
            backend.dt.edge_key(a, b).is_err(),
            "Delaunay 0.8 should reject non-edges before constructing a handle"
        );
    }

    #[test]
    fn test_adjacent_faces() {
        let dt = random_delaunay2(4, (0.0, 10.0));
        let backend = validated_backend(dt);

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
    fn adjacent_faces_maintained_incidence_reflects_mutation() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1)])
            .expect("triangle fixture should build");
        let mut backend = validated_backend(dt);
        let vertex = backend
            .vertices()
            .next()
            .expect("triangle fixture should contain a vertex");

        let first = backend
            .adjacent_faces(&vertex)
            .expect("first adjacency query should read maintained incidence");
        let second = backend
            .adjacent_faces(&vertex)
            .expect("second adjacency query should read the same incidence");
        assert_eq!(first, second);

        let face = backend
            .faces()
            .next()
            .expect("triangle fixture should contain a face");
        backend
            .subdivide_face(face, &[0.25, 0.25])
            .expect("subdivision should update maintained incidence");
        assert!(
            backend
                .adjacent_faces(&vertex)
                .expect("adjacency query after mutation should read updated incidence")
                .len()
                >= first.len()
        );
    }

    #[test]
    fn test_incident_edges() {
        let dt = random_delaunay2(4, (0.0, 10.0));
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

        let bogus_key = SimplexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayFaceHandle { key: bogus_key };
        let err = backend.face_neighbors(&invalid_handle).unwrap_err();
        assert_matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key);
    }

    #[test]
    fn test_adjacent_faces_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);

        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.adjacent_faces(&invalid_handle).unwrap_err();
        assert_matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key);
    }

    #[test]
    fn test_incident_edges_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);

        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = DelaunayVertexHandle { key: bogus_key };
        let err = backend.incident_edges(&invalid_handle).unwrap_err();
        assert_matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key);
    }

    #[test]
    fn test_dimension() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);
        assert_eq!(backend.dimension(), 2, "DelaunayBackend2D should be 2D");
    }

    #[test]
    fn test_euler_characteristic() {
        // For a planar triangulation without boundary: V - E + F = 1
        let dt = seeded_delaunay2(6, (0.0, 10.0), 42);
        let backend = validated_backend(dt);
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
        let backend = validated_backend(dt);

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
        let backend = validated_backend(dt);

        let vertex_count = backend.vertex_count();
        let edge_count = backend.edge_count();
        let face_count = backend.face_count();

        // Verify Euler characteristic for planar graphs
        // For a triangulation without the outer infinite face: V - E + F = 1
        // For a triangulation with the outer infinite face: V - E + F = 2
        // Note: Random triangulations may occasionally have degeneracies that result in χ = 0
        let euler = vertex_count as i128 - edge_count as i128 + face_count as i128;
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
        let backend = validated_backend(dt);

        // Test all vertices are accessible
        assert_eq!(
            backend.vertices().count(),
            3,
            "Should have exactly 3 vertices"
        );

        // Test all edges are accessible
        assert_eq!(backend.edges().count(), 3, "Should have exactly 3 edges");

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
        let backend = validated_backend(dt);

        assert_eq!(
            backend.topology_kind(),
            TopologyKind::Euclidean,
            "Default builder construction should produce Euclidean topology"
        );
    }

    #[test]
    fn test_is_valid_runs_structural_validation() {
        // is_valid() runs Levels 1–3, validate_embedding() runs Levels 1–4,
        // and is_delaunay() runs Levels 1–5.
        // For a well-formed Delaunay triangulation both should pass.
        let dt = seeded_delaunay2(8, (0.0, 10.0), 99);
        let backend = validated_backend(dt);

        let valid = backend.is_valid();
        let delaunay = backend.is_delaunay();

        assert!(valid, "Seeded triangulation should be structurally valid");
        assert!(
            delaunay,
            "Seeded triangulation should satisfy Delaunay property"
        );
        // is_delaunay() (Levels 1–5) implies is_valid() (Levels 1–3)
        assert!(delaunay && valid, "is_delaunay() should imply is_valid()");
    }

    #[test]
    fn test_mutation_methods_use_delaunay_edit_api() {
        let dt = build_delaunay2_from_simplices(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit square should build");
        let mut backend = validated_backend(dt);
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
        let mut backend = validated_backend(dt);
        let original_vertex_count = backend.vertex_count();
        let original_face_count = backend.face_count();
        let face = backend.faces().next().expect("valid face handle");
        let subdivide = backend
            .subdivide_face(face, &[0.5, 1.0 / 3.0])
            .expect("face subdivision should use k=1 flip");
        assert_eq!(backend.vertex_count(), original_vertex_count + 1);
        assert_eq!(backend.face_count(), original_face_count + 2);
        assert!(backend.is_valid());

        let (): () = backend
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

        assert_matches!(
            backend.move_vertex(vertex, &[1.0, 1.0]),
            Err(DelaunayError::NotImplemented {
                operation: DelaunayOperation::MoveVertex,
            })
        );
        assert_matches!(
            backend.insert_vertex(&[0.0]),
            Err(DelaunayError::CoordinateDimensionMismatch {
                actual: 1,
                expected: 2,
            })
        );
        assert_matches!(
            backend.insert_vertex(&[f64::NAN, 0.0]),
            Err(DelaunayError::NonFiniteCoordinate {
                operation: DelaunayOperation::InsertVertex,
                axis: 0,
                value,
            }) if value.is_nan()
        );

        let bogus_vertex = VertexKey::from(KeyData::from_ffi(u64::MAX));
        assert_matches!(
            backend.remove_vertex(DelaunayVertexHandle { key: bogus_vertex }),
            Err(DelaunayError::InvalidVertex { key }) if key == bogus_vertex,
        );

        let bogus_face = SimplexKey::from(KeyData::from_ffi(u64::MAX));
        assert_matches!(
            backend.subdivide_face(DelaunayFaceHandle { key: bogus_face }, &[0.25, 0.25]),
            Err(DelaunayError::InvalidFace { key }) if key == bogus_face,
        );

        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("labeled triangle should build");
        let mut boundary_backend = validated_backend(dt);
        let boundary_edge = boundary_backend
            .edges()
            .next()
            .expect("single triangle has boundary edges");
        assert_matches!(
            boundary_backend.flip_edge(boundary_edge),
            Err(DelaunayError::NonFlippableEdge { reason, .. })
                if reason == NonFlippableEdgeReason::NotInteriorFacet,
        );
    }

    #[test]
    fn failed_vertex_removal_restores_backend_snapshot() {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], 0),
            ([1.0, 0.0], 0),
            ([0.0, 1.0], 0),
            ([1.0, 1.0], 0),
            ([0.18, 0.42], 1),
            ([0.52, 0.18], 1),
            ([0.64, 0.72], 1),
        ])
        .expect("deletion rollback fixture should build");
        let mut backend = validated_backend(dt);
        backend
            .dt
            .set_delaunay_repair_policy(DelaunayRepairPolicy::Never);

        let vertex = backend
            .vertices()
            .find(|vertex| {
                backend
                    .vertex_coordinates(vertex)
                    .is_ok_and(|coordinates| coordinates == [0.18, 0.42])
            })
            .expect("rollback fixture vertex should be present");
        let serialized_before = serde_json::to_value(&backend).expect("backend should serialize");
        let facets_before = backend.interior_facets_by_edge.clone();

        let error = backend
            .remove_vertex(vertex)
            .expect_err("disabled repair should reject this deletion");

        assert_matches!(
            error,
            DelaunayError::RemovalFailed {
                operation: DelaunayOperation::RemoveVertex,
                ..
            }
        );
        assert_eq!(
            serde_json::to_value(&backend).expect("restored backend should serialize"),
            serialized_before
        );
        assert_eq!(backend.interior_facets_by_edge, facets_before);
    }

    #[test]
    fn generic_vertex_removal_retriangulates_cavity() {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], 0),
            ([1.0, 0.0], 0),
            ([0.0, 1.0], 0),
            ([1.0, 1.0], 0),
            ([0.18, 0.42], 1),
            ([0.52, 0.18], 1),
            ([0.64, 0.72], 1),
        ])
        .expect("generic deletion fixture should build");
        let mut backend = validated_backend(dt);
        let vertex = backend
            .vertices()
            .find(|vertex| {
                backend
                    .vertex_coordinates(vertex)
                    .is_ok_and(|coordinates| coordinates == [0.18, 0.42])
            })
            .expect("generic deletion fixture vertex should be present");
        assert!(
            backend.dt.can_flip_k1_remove(vertex.key).is_err(),
            "fixture must exercise generic cavity deletion"
        );
        let original_vertex_count = backend.vertex_count();
        let original_face_count = backend.face_count();

        assert_matches!(backend.remove_vertex(vertex), Ok(()));

        assert_eq!(backend.vertex_count(), original_vertex_count - 1);
        assert_eq!(backend.face_count(), original_face_count - 2);
        assert!(backend.is_valid());
    }

    #[test]
    fn backend_rejects_non_delaunay_connectivity() {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], 0),
            ([4.0, 0.0], 0),
            ([4.0, 2.0], 1),
            ([1.0, 2.0], 1),
        ])
        .expect("convex quad should build");
        let backend = validated_backend(dt);
        let mut value = serde_json::to_value(&backend).expect("backend should serialize");
        set_non_delaunay_quad_diagonal(&mut value);
        let invalid_json = serde_json::to_string(&value).expect("corrupt backend should serialize");

        let error = serde_json::from_str::<DelaunayBackend2D>(&invalid_json)
            .expect_err("non-Delaunay connectivity must be rejected");

        assert!(
            error.to_string().contains("Delaunay verification failed"),
            "unexpected deserialization error: {error}"
        );
    }

    #[test]
    fn clear_and_reserve_report_unsupported_without_mutating() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("labeled triangle should build");
        let mut backend = validated_backend(dt);
        let counts_before = (
            backend.vertex_count(),
            backend.edge_count(),
            backend.face_count(),
        );

        assert_matches!(
            backend.clear(),
            Err(DelaunayError::NotImplemented {
                operation: DelaunayOperation::Clear,
            })
        );
        assert_matches!(
            backend.reserve_capacity(32, 64),
            Err(DelaunayError::NotImplemented {
                operation: DelaunayOperation::ReserveCapacity,
            })
        );
        assert_eq!(
            (
                backend.vertex_count(),
                backend.edge_count(),
                backend.face_count(),
            ),
            counts_before
        );
    }

    #[test]
    fn test_interior_facet_cache_updates_after_edge_flip() {
        let dt = build_delaunay2_from_simplices(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit square should build");
        let mut backend = validated_backend(dt);
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
    fn replacement_edge_lookup_failure_restores_flip_snapshot() {
        let dt = build_delaunay2_from_simplices(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit square should build");
        let mut backend = validated_backend(dt);
        let serialized_before = serde_json::to_value(&backend).expect("backend should serialize");
        let dt_before = backend.dt.clone();
        let facets_before = backend.interior_facets_by_edge.clone();
        let expected_facets = facets_before.clone();
        let facet = *backend
            .interior_facets_by_edge
            .values()
            .next()
            .expect("square should have one interior facet");
        let info = backend
            .dt
            .flip_k2(facet)
            .expect("interior edge should flip");
        backend.rebuild_interior_facet_index();
        assert_ne!(
            serde_json::to_value(&backend).expect("mutated backend should serialize"),
            serialized_before,
            "test setup should mutate the triangulation before injecting failure"
        );

        let live_vertex = info.inserted_face_vertices[0];
        let missing_vertex = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let error = backend
            .replacement_edge_key_or_restore(live_vertex, missing_vertex, dt_before, facets_before)
            .expect_err("missing replacement vertex should fail edge reconstruction");

        assert_matches!(
            error,
            DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                ..
            }
        );
        assert_eq!(
            serde_json::to_value(&backend).expect("restored backend should serialize"),
            serialized_before
        );
        assert_eq!(backend.interior_facets_by_edge, expected_facets);
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
        let backend = validated_backend(dt);
        assert_send_sync(&backend);
    }
}
