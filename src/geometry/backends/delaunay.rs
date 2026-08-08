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
use delaunay::prelude::collections::Uuid;
use delaunay::prelude::export::{MeshExport, MeshExportError};
use delaunay::prelude::{DataSerialize, DataType};
use delaunay::tds::{EdgeKey, FacetHandle, SimplexKey, Tds, Vertex, VertexKey};
use delaunay::topology::traits::{GlobalTopology, TopologyKind, ToroidalConstructionMode};
use delaunay::{
    DelaunayCheckPolicy, DelaunayTriangulation, SimplexBarycenterError, TopologyGuarantee,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::num::NonZeroUsize;
use std::ops::{Deref, DerefMut};

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
    /// Runtime identity used to reject handles from a different backend owner.
    owner_id: Uuid,
}

/// Transaction guard for one backend topology mutation.
///
/// The guard holds the only mutable backend borrow for the mutation window.
/// Standalone backend edits carry a snapshot that is restored unless
/// [`Self::commit`] succeeds; CDT edits may instead rely on an enclosing
/// caller-owned rollback or discard boundary.
struct DelaunayMutation<'a, VertexData: DataType, SimplexData: DataType, const D: usize> {
    backend: &'a mut DelaunayBackend<VertexData, SimplexData, D>,
    snapshot: Option<(
        RawTriangulation<VertexData, SimplexData, D>,
        HashMap<EdgeKey, FacetHandle>,
    )>,
}

impl<'a, VertexData: DataType, SimplexData: DataType, const D: usize>
    DelaunayMutation<'a, VertexData, SimplexData, D>
{
    /// Begins a mutation by capturing the canonical topology and derived index.
    fn new(backend: &'a mut DelaunayBackend<VertexData, SimplexData, D>) -> Self {
        let snapshot = Some((backend.dt.clone(), backend.interior_facets_by_edge.clone()));
        Self { backend, snapshot }
    }

    /// Begins a mutation inside a caller-owned rollback or discard boundary.
    ///
    /// The caller must restore the enclosing CDT snapshot or discard its
    /// speculative state whenever the operation or a later postcondition fails.
    const fn without_snapshot(
        backend: &'a mut DelaunayBackend<VertexData, SimplexData, D>,
    ) -> Self {
        Self {
            backend,
            snapshot: None,
        }
    }

    /// Publishes the mutated backend and discards rollback state.
    fn commit(mut self) {
        self.snapshot = None;
    }
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> Deref
    for DelaunayMutation<'_, VertexData, SimplexData, D>
{
    type Target = DelaunayBackend<VertexData, SimplexData, D>;

    fn deref(&self) -> &Self::Target {
        self.backend
    }
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> DerefMut
    for DelaunayMutation<'_, VertexData, SimplexData, D>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.backend
    }
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize> Drop
    for DelaunayMutation<'_, VertexData, SimplexData, D>
{
    fn drop(&mut self) {
        if let Some((dt, facets)) = self.snapshot.take() {
            self.backend.dt = dt;
            self.backend.interior_facets_by_edge = facets;
        }
    }
}

impl<VertexData: Clone, SimplexData: Clone, const D: usize> Clone
    for DelaunayBackend<VertexData, SimplexData, D>
{
    fn clone(&self) -> Self {
        Self {
            dt: self.dt.clone(),
            interior_facets_by_edge: self.interior_facets_by_edge.clone(),
            owner_id: Uuid::new_v4(),
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

impl<VertexData: DataSerialize, SimplexData: DataSerialize, const D: usize> Serialize
    for DelaunayBackend<VertexData, SimplexData, D>
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

/// Opaque runtime handle for a vertex in one Delaunay backend generation.
///
/// Equality and hashing include the backend owner, topology generation, and
/// local key, so handles are suitable for temporary [`HashMap`] and
/// [`HashSet`](std::collections::HashSet) indexes. Handles are deliberately not
/// serializable or durable identifiers; every clone and deserialization creates
/// a new owner.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayVertexHandle {
    key: VertexKey,
    owner_id: Uuid,
    generation: u64,
}

impl DelaunayVertexHandle {
    /// Returns the underlying slotmap key for use in secondary maps.
    #[must_use]
    pub(crate) const fn vertex_key(&self) -> VertexKey {
        self.key
    }
}

/// Opaque runtime handle for an edge in one Delaunay backend generation.
///
/// Equality and hashing include owner and generation provenance. The handle is
/// intended for transient maps and sets, not persistence across mutation,
/// cloning, or serialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayEdgeHandle {
    key: EdgeKey,
    owner_id: Uuid,
    generation: u64,
}

/// Opaque runtime handle for a face in one Delaunay backend generation.
///
/// Equality and hashing include owner and generation provenance. The handle is
/// intended for transient maps and sets, not persistence across mutation,
/// cloning, or serialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelaunayFaceHandle {
    key: SimplexKey,
    owner_id: Uuid,
    generation: u64,
}

/// Faces created by an internal inverse k=1 vertex removal.
#[derive(Debug, Clone)]
pub(crate) struct DelaunayRemovalResult {
    pub(crate) new_faces: Vec<DelaunayFaceHandle>,
}

/// Stable vertex identity used only to remap a proposal into a cloned owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DelaunayVertexStableId(Uuid);

/// Stable edge identity used only to remap a proposal into a cloned owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DelaunayEdgeStableId {
    v0: Uuid,
    v1: Uuid,
}

/// Stable face identity used only to remap a proposal into a cloned owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DelaunayFaceStableId(Uuid);

/// Kind of detached handle whose provenance failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DelaunayHandleKind {
    /// Vertex handle.
    Vertex,
    /// Edge handle.
    Edge,
    /// Face handle.
    Face,
}

impl fmt::Display for DelaunayHandleKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vertex => formatter.write_str("vertex"),
            Self::Edge => formatter.write_str("edge"),
            Self::Face => formatter.write_str("face"),
        }
    }
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

/// Structured contract failure returned by a successful upstream flip.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DelaunayFlipOutputFailure {
    /// The replacement edge reported by a k=2 flip was not live afterward.
    ReplacementEdgeUnavailable {
        /// Upstream lookup diagnostic.
        detail: String,
    },
    /// The flip reported the wrong number of vertices opposite its inserted faces.
    InsertedVertexCountMismatch {
        /// Number of inserted-face vertices required by the wrapper contract.
        expected: usize,
        /// Number of inserted-face vertices returned by the upstream flip.
        actual: usize,
        /// First unexpected vertex handle when the upstream result was oversized.
        first_unexpected: Option<String>,
    },
}

impl fmt::Display for DelaunayFlipOutputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReplacementEdgeUnavailable { detail } => {
                write!(
                    formatter,
                    "replacement edge is unavailable after the flip: {detail}"
                )
            }
            Self::InsertedVertexCountMismatch {
                expected,
                actual,
                first_unexpected,
            } => {
                write!(
                    formatter,
                    "reported {actual} inserted-face vertices, expected {expected}"
                )?;
                if let Some(vertex) = first_unexpected {
                    write!(formatter, "; first unexpected vertex {vertex}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for DelaunayFlipOutputFailure {}

/// Delaunay backend errors preserving typed mutation and validation context.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DelaunayError {
    /// A detached handle belongs to a different backend instance.
    #[error("foreign {kind} handle: owner {handle_owner} does not match backend {backend_owner}")]
    ForeignHandle {
        /// Kind of handle being resolved.
        kind: DelaunayHandleKind,
        /// Owner recorded by the handle.
        handle_owner: Uuid,
        /// Current backend owner.
        backend_owner: Uuid,
    },

    /// A detached handle predates the current topology generation.
    #[error(
        "stale {kind} handle: generation {handle_generation} does not match current generation {current_generation}"
    )]
    StaleHandle {
        /// Kind of handle being resolved.
        kind: DelaunayHandleKind,
        /// Generation recorded by the handle.
        handle_generation: u64,
        /// Current backend topology generation.
        current_generation: u64,
    },

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
    #[error("{operation} returned unexpected output for {target}: {failure}")]
    UnexpectedFlipOutput {
        /// Flip operation that returned malformed output.
        operation: DelaunayOperation,
        /// Human-readable target passed to the flip operation.
        target: String,
        /// Structured contract failure in the upstream result.
        #[source]
        failure: DelaunayFlipOutputFailure,
    },

    /// A local topology edit could not update the derived interior-facet index.
    #[error("failed to update the interior-facet index after {operation}: {detail}")]
    InteriorFacetIndexUpdateFailed {
        /// Mutation whose local index update failed.
        operation: DelaunayOperation,
        /// Underlying topology-query diagnostic.
        detail: String,
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
        Self::populate_interior_facets_by_edge(dt, &mut facets_by_edge);
        facets_by_edge
    }

    /// Populates a caller-owned interior-facet map without replacing its allocation.
    fn populate_interior_facets_by_edge(
        dt: &RawTriangulation<VertexData, SimplexData, D>,
        facets_by_edge: &mut HashMap<EdgeKey, FacetHandle>,
    ) {
        if D != 2 {
            return;
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
    }

    /// Refreshes cached edge adjacency after a topology mutation succeeds.
    fn rebuild_interior_facet_index(&mut self) {
        self.interior_facets_by_edge.clear();
        Self::populate_interior_facets_by_edge(&self.dt, &mut self.interior_facets_by_edge);
    }

    /// Collects every edge belonging to the simplices an edit will replace.
    fn local_edges_for_simplices(
        &self,
        simplices: &[SimplexKey],
        operation: DelaunayOperation,
    ) -> Result<Vec<EdgeKey>, DelaunayError> {
        if D != 2 {
            return Ok(Vec::new());
        }

        let mut edges = Vec::with_capacity(simplices.len().saturating_mul(3));
        for &simplex_key in simplices {
            let vertices = self.dt.simplex_vertices(simplex_key).map_err(|err| {
                DelaunayError::InteriorFacetIndexUpdateFailed {
                    operation,
                    detail: err.to_string(),
                }
            })?;
            for first in 0..vertices.len() {
                for second in (first + 1)..vertices.len() {
                    let edge = self
                        .dt
                        .edge_key(vertices[first], vertices[second])
                        .map_err(|err| DelaunayError::InteriorFacetIndexUpdateFailed {
                            operation,
                            detail: err.to_string(),
                        })?;
                    if !edges.contains(&edge) {
                        edges.push(edge);
                    }
                }
            }
        }
        Ok(edges)
    }

    /// Resolves the two live simplices sharing an indexed interior facet.
    fn simplices_adjacent_to_facet(
        &self,
        facet: FacetHandle,
        operation: DelaunayOperation,
    ) -> Result<[SimplexKey; 2], DelaunayError> {
        let simplex_key = facet.simplex_key();
        let simplex = self.dt.simplex(simplex_key).ok_or_else(|| {
            DelaunayError::InteriorFacetIndexUpdateFailed {
                operation,
                detail: format!("facet source simplex {simplex_key:?} is not live"),
            }
        })?;
        let neighbor = simplex
            .neighbors()
            .and_then(|mut neighbors| neighbors.nth(usize::from(facet.facet_index())))
            .flatten()
            .ok_or_else(|| DelaunayError::InteriorFacetIndexUpdateFailed {
                operation,
                detail: format!("facet {facet:?} has no adjacent simplex"),
            })?;
        Ok([simplex_key, neighbor])
    }

    /// Applies the facet-index delta described by one realized local edit.
    fn update_interior_facet_index(
        &mut self,
        removed_edges: &[EdgeKey],
        new_simplices: &[SimplexKey],
        operation: DelaunayOperation,
    ) -> Result<(), DelaunayError> {
        if D != 2 {
            return Ok(());
        }
        for edge in removed_edges {
            self.interior_facets_by_edge.remove(edge);
        }
        for &simplex_key in new_simplices {
            let simplex = self.dt.simplex(simplex_key).ok_or_else(|| {
                DelaunayError::InteriorFacetIndexUpdateFailed {
                    operation,
                    detail: format!("new simplex {simplex_key:?} is not live"),
                }
            })?;
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
                    .filter_map(|(index, &vertex)| (index != facet_index).then_some(vertex));
                let (Some(first), Some(second), None) = (
                    facet_vertices.next(),
                    facet_vertices.next(),
                    facet_vertices.next(),
                ) else {
                    continue;
                };
                let edge = self.dt.edge_key(first, second).map_err(|err| {
                    DelaunayError::InteriorFacetIndexUpdateFailed {
                        operation,
                        detail: err.to_string(),
                    }
                })?;
                let facet_index = u8::try_from(facet_index).map_err(|err| {
                    DelaunayError::InteriorFacetIndexUpdateFailed {
                        operation,
                        detail: err.to_string(),
                    }
                })?;
                let facet = self
                    .dt
                    .facet_handle(simplex_key, facet_index)
                    .map_err(|err| DelaunayError::InteriorFacetIndexUpdateFailed {
                        operation,
                        detail: err.to_string(),
                    })?;
                self.interior_facets_by_edge.entry(edge).or_insert(facet);
            }
        }
        Ok(())
    }

    /// Resolves the replacement edge produced by a flip inside a rollback guard.
    ///
    /// The upstream flip mutates the triangulation before returning its result metadata. If
    /// that metadata does not identify a live replacement edge, the whole backend mutation
    /// must be rolled back rather than publishing the changed triangulation with an error.
    fn replacement_edge_key(&self, v0: VertexKey, v1: VertexKey) -> Result<EdgeKey, DelaunayError> {
        self.dt
            .edge_key(v0, v1)
            .map_err(|err| DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("replacement edge {v0:?} -- {v1:?}"),
                failure: DelaunayFlipOutputFailure::ReplacementEdgeUnavailable {
                    detail: err.to_string(),
                },
            })
    }

    /// Validates embedding after a mutation without an upstream realization postcondition.
    ///
    /// High-level upstream bistellar flips already run cumulative Level 1-4
    /// realization validation transactionally and therefore do not call this
    /// helper. Other mutation paths are checked here so every successful backend
    /// edit has the same postcondition without duplicating whole-mesh scans.
    fn validate_embedding_after_mutation(
        &self,
        operation: DelaunayOperation,
        target: impl Display,
    ) -> Result<(), DelaunayError> {
        Self::map_embedding_validation_error(self.validate_embedding(), operation, target)
    }

    /// Adds mutation context to an embedding validation failure.
    fn map_embedding_validation_error(
        validation: Result<(), DelaunayError>,
        operation: DelaunayOperation,
        target: impl Display,
    ) -> Result<(), DelaunayError> {
        let Err(error) = validation else {
            return Ok(());
        };
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
            owner_id: Uuid::new_v4(),
        };
        backend.validate_delaunay()?;
        Ok(backend)
    }
}

impl<VertexData, SimplexData, const D: usize> DelaunayBackend<VertexData, SimplexData, D> {
    /// Returns the topology generation attached to newly issued handles.
    fn handle_generation(&self) -> u64 {
        self.dt.topology_generation()
    }

    /// Creates a vertex handle scoped to this owner and generation.
    fn vertex_handle(&self, key: VertexKey) -> DelaunayVertexHandle {
        DelaunayVertexHandle {
            key,
            owner_id: self.owner_id,
            generation: self.handle_generation(),
        }
    }

    /// Creates an edge handle scoped to this owner and generation.
    fn edge_handle(&self, key: EdgeKey) -> DelaunayEdgeHandle {
        DelaunayEdgeHandle {
            key,
            owner_id: self.owner_id,
            generation: self.handle_generation(),
        }
    }

    /// Creates a face handle scoped to this owner and generation.
    fn face_handle(&self, key: SimplexKey) -> DelaunayFaceHandle {
        DelaunayFaceHandle {
            key,
            owner_id: self.owner_id,
            generation: self.handle_generation(),
        }
    }

    fn validate_handle_provenance(
        &self,
        kind: DelaunayHandleKind,
        owner_id: Uuid,
        generation: u64,
    ) -> Result<(), DelaunayError> {
        if owner_id != self.owner_id {
            return Err(DelaunayError::ForeignHandle {
                kind,
                handle_owner: owner_id,
                backend_owner: self.owner_id,
            });
        }
        let current_generation = self.handle_generation();
        if generation != current_generation {
            return Err(DelaunayError::StaleHandle {
                kind,
                handle_generation: generation,
                current_generation,
            });
        }
        Ok(())
    }

    fn validate_vertex_handle(
        &self,
        vertex: &DelaunayVertexHandle,
    ) -> Result<VertexKey, DelaunayError> {
        self.validate_handle_provenance(
            DelaunayHandleKind::Vertex,
            vertex.owner_id,
            vertex.generation,
        )?;
        self.dt
            .contains_vertex_key(vertex.key)
            .then_some(vertex.key)
            .ok_or(DelaunayError::InvalidVertex { key: vertex.key })
    }

    fn validate_edge_handle(&self, edge: &DelaunayEdgeHandle) -> Result<EdgeKey, DelaunayError> {
        self.validate_handle_provenance(DelaunayHandleKind::Edge, edge.owner_id, edge.generation)?;
        let (v0, v1) = edge.key.endpoints();
        let edge_exists = self.dt.contains_vertex_key(v0)
            && self.dt.contains_vertex_key(v1)
            && self
                .dt
                .incident_edges(v0)
                .any(|candidate| candidate == edge.key);
        edge_exists
            .then_some(edge.key)
            .ok_or_else(|| DelaunayError::InvalidEdge {
                v0: edge.key.v0(),
                v1: edge.key.v1(),
            })
    }

    fn validate_face_handle(&self, face: &DelaunayFaceHandle) -> Result<SimplexKey, DelaunayError> {
        self.validate_handle_provenance(DelaunayHandleKind::Face, face.owner_id, face.generation)?;
        self.dt
            .contains_simplex(face.key)
            .then_some(face.key)
            .ok_or(DelaunayError::InvalidFace { key: face.key })
    }

    /// Converts an owner-bound vertex handle into stable clone-remapping identity.
    pub(crate) fn stable_vertex_id(
        &self,
        vertex: &DelaunayVertexHandle,
    ) -> Result<DelaunayVertexStableId, DelaunayError> {
        let key = self.validate_vertex_handle(vertex)?;
        self.dt
            .vertex_uuid_from_key(key)
            .map(DelaunayVertexStableId)
            .ok_or(DelaunayError::InvalidVertex { key })
    }

    /// Resolves stable vertex identity into a current owner-bound handle.
    pub(crate) fn resolve_vertex_id(
        &self,
        stable_id: DelaunayVertexStableId,
    ) -> Result<DelaunayVertexHandle, DelaunayError> {
        self.dt
            .vertex_key_from_uuid(&stable_id.0)
            .map(|key| self.vertex_handle(key))
            .ok_or_else(|| DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Two,
                detail: format!(
                    "stable vertex {} is not present in the target owner",
                    stable_id.0
                ),
            })
    }

    /// Converts an owner-bound face handle into stable clone-remapping identity.
    pub(crate) fn stable_face_id(
        &self,
        face: &DelaunayFaceHandle,
    ) -> Result<DelaunayFaceStableId, DelaunayError> {
        let key = self.validate_face_handle(face)?;
        self.dt
            .simplex_uuid_from_key(key)
            .map(DelaunayFaceStableId)
            .ok_or(DelaunayError::InvalidFace { key })
    }

    /// Resolves stable face identity into a current owner-bound handle.
    pub(crate) fn resolve_face_id(
        &self,
        stable_id: DelaunayFaceStableId,
    ) -> Result<DelaunayFaceHandle, DelaunayError> {
        self.dt
            .simplex_key_from_uuid(&stable_id.0)
            .map(|key| self.face_handle(key))
            .ok_or_else(|| DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Two,
                detail: format!(
                    "stable face {} is not present in the target owner",
                    stable_id.0
                ),
            })
    }

    /// Resolves a stable face only when it survived a later local primitive.
    pub(crate) fn resolve_live_face_id(
        &self,
        stable_id: DelaunayFaceStableId,
    ) -> Option<DelaunayFaceHandle> {
        self.dt
            .simplex_key_from_uuid(&stable_id.0)
            .map(|key| self.face_handle(key))
    }

    /// Converts an owner-bound edge handle into stable endpoint identity.
    pub(crate) fn stable_edge_id(
        &self,
        edge: &DelaunayEdgeHandle,
    ) -> Result<DelaunayEdgeStableId, DelaunayError> {
        let key = self.validate_edge_handle(edge)?;
        let (v0, v1) = key.endpoints();
        let v0 = self
            .dt
            .vertex_uuid_from_key(v0)
            .ok_or(DelaunayError::InvalidVertex { key: v0 })?;
        let v1 = self
            .dt
            .vertex_uuid_from_key(v1)
            .ok_or(DelaunayError::InvalidVertex { key: v1 })?;
        Ok(DelaunayEdgeStableId { v0, v1 })
    }

    /// Resolves stable edge identity into a current owner-bound handle.
    pub(crate) fn resolve_edge_id(
        &self,
        stable_id: DelaunayEdgeStableId,
    ) -> Result<DelaunayEdgeHandle, DelaunayError> {
        let v0 = self.dt.vertex_key_from_uuid(&stable_id.v0).ok_or_else(|| {
            DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Two,
                detail: format!(
                    "stable edge endpoint {} is not present in the target owner",
                    stable_id.v0
                ),
            }
        })?;
        let v1 = self.dt.vertex_key_from_uuid(&stable_id.v1).ok_or_else(|| {
            DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Two,
                detail: format!(
                    "stable edge endpoint {} is not present in the target owner",
                    stable_id.v1
                ),
            }
        })?;
        self.dt
            .edge_key(v0, v1)
            .map(|key| self.edge_handle(key))
            .map_err(|_| DelaunayError::InvalidEdge { v0, v1 })
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
    /// Uses the upstream cumulative
    /// [`DelaunayTriangulation::validate`] operation, which checks structural
    /// and topological validity (Levels 1–3), straight-line
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
    pub fn is_delaunay(&self) -> bool
    where
        VertexData: DataType,
        SimplexData: DataType,
    {
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
    pub fn validate_delaunay(&self) -> Result<(), DelaunayError>
    where
        VertexData: DataType,
        SimplexData: DataType,
    {
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
    pub fn validate_embedding(&self) -> Result<(), DelaunayError>
    where
        VertexData: DataType,
        SimplexData: DataType,
    {
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
    pub fn validate_structural(&self) -> Result<(), DelaunayError>
    where
        VertexData: DataType,
        SimplexData: DataType,
    {
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
    /// [`DelaunayCheckPolicy::EveryN`] cadence semantics as the upstream
    /// Delaunay crate.
    #[must_use]
    pub(crate) fn should_check_delaunay_after(&self, completed_mutations: u64) -> bool {
        usize::try_from(completed_mutations)
            .is_ok_and(|count| self.dt.delaunay_check_policy().should_check(count))
    }

    /// Returns the high-level topology kind, such as
    /// [`TopologyKind::Euclidean`] or [`TopologyKind::Toroidal`], of the
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
        let key = self.validate_face_handle(face)?;
        let point = self
            .dt
            .simplex_barycenter(key)
            .map_err(|error| match error {
                SimplexBarycenterError::MissingSimplex { .. } => DelaunayError::InvalidFace { key },
                error => DelaunayError::FaceBarycenterFailed {
                    key,
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
        let key = self.validate_vertex_handle(vertex)?;
        self.dt
            .vertex(key)
            .map(|vertex| vertex.uuid().to_string())
            .ok_or(DelaunayError::InvalidVertex { key })
    }

    /// Returns the copied payload for a current owner-bound vertex handle.
    ///
    /// Returns `Ok(None)` when the live vertex has no attached payload.
    ///
    /// # Errors
    ///
    /// Returns a typed provenance or invalid-key error when `vertex` is not a
    /// live handle issued by this backend generation.
    pub fn vertex_data(
        &self,
        vertex: &DelaunayVertexHandle,
    ) -> Result<Option<VertexData>, DelaunayError>
    where
        VertexData: Copy,
    {
        let key = self.validate_vertex_handle(vertex)?;
        Ok(self
            .dt
            .vertex(key)
            .and_then(|vertex| vertex.data().copied()))
    }

    /// Returns the copied payload for a current owner-bound face handle.
    ///
    /// Returns `Ok(None)` when the live face has no attached payload.
    ///
    /// # Errors
    ///
    /// Returns a typed provenance or invalid-key error when `face` is not a
    /// live handle issued by this backend generation.
    pub fn simplex_data(
        &self,
        face: &DelaunayFaceHandle,
    ) -> Result<Option<SimplexData>, DelaunayError>
    where
        SimplexData: Copy,
    {
        let key = self.validate_face_handle(face)?;
        Ok(self
            .dt
            .simplex(key)
            .and_then(|simplex| simplex.data().copied()))
    }

    /// Replaces the payload for a current owner-bound vertex handle.
    ///
    /// Returns the previous payload, including `None` when the vertex had no
    /// attached payload.
    ///
    /// # Errors
    ///
    /// Returns a typed provenance or invalid-key error when `vertex` is not a
    /// live handle issued by this backend generation.
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
    ///     let mut backend = DelaunayBackend2D::from_triangulation(dt)?;
    ///     let vertex = backend.vertices().next().ok_or_else(|| {
    ///         DelaunayError::ValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: "validated triangle has no vertex".to_string(),
    ///         }
    ///     })?;
    ///     let previous = backend.vertex_data(&vertex)?;
    ///
    ///     assert_eq!(backend.set_vertex_data(&vertex, Some(7))?, previous);
    ///     assert_eq!(backend.vertex_data(&vertex)?, Some(7));
    ///     Ok(())
    /// }
    /// ```
    pub fn set_vertex_data(
        &mut self,
        vertex: &DelaunayVertexHandle,
        data: Option<VertexData>,
    ) -> Result<Option<VertexData>, DelaunayError> {
        let key = self.validate_vertex_handle(vertex)?;
        self.set_vertex_data_by_key(key, data)
    }

    /// Replaces the payload for a current owner-bound face handle.
    ///
    /// Returns the previous payload, including `None` when the face had no
    /// attached payload.
    ///
    /// # Errors
    ///
    /// Returns a typed provenance or invalid-key error when `face` is not a
    /// live handle issued by this backend generation.
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
    ///     let mut backend = DelaunayBackend2D::from_triangulation(dt)?;
    ///     let face = backend.faces().next().ok_or_else(|| {
    ///         DelaunayError::ValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: "validated triangle has no face".to_string(),
    ///         }
    ///     })?;
    ///     let previous = backend.simplex_data(&face)?;
    ///
    ///     assert_eq!(backend.set_simplex_data(&face, Some(-3))?, previous);
    ///     assert_eq!(backend.simplex_data(&face)?, Some(-3));
    ///     Ok(())
    /// }
    /// ```
    pub fn set_simplex_data(
        &mut self,
        face: &DelaunayFaceHandle,
        data: Option<SimplexData>,
    ) -> Result<Option<SimplexData>, DelaunayError> {
        let key = self.validate_face_handle(face)?;
        self.set_simplex_data_by_key(key, data)
    }

    /// Returns the vertex payload for `key`, if present.
    #[must_use]
    pub(crate) fn vertex_data_by_key(&self, key: VertexKey) -> Option<VertexData>
    where
        VertexData: Copy,
    {
        self.dt.vertex(key)?.data().copied()
    }

    /// Returns the simplex payload for `key`, if present.
    #[must_use]
    pub(crate) fn simplex_data_by_key(&self, key: SimplexKey) -> Option<SimplexData>
    where
        SimplexData: Copy,
    {
        self.dt.simplex(key)?.data().copied()
    }

    /// Sets the optional payload for a vertex identified by `key`.
    ///
    /// Returns the previous payload for a valid key.
    ///
    /// # Errors
    ///
    /// Returns [`DelaunayError::InvalidVertex`] if `key` is not present.
    pub(crate) fn set_vertex_data_by_key(
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
    pub(crate) fn set_simplex_data_by_key(
        &mut self,
        key: SimplexKey,
        data: Option<SimplexData>,
    ) -> Result<Option<SimplexData>, DelaunayError> {
        self.dt
            .set_simplex_data(key, data)
            .map_err(|_| DelaunayError::InvalidFace { key })
    }
}

impl<VertexData, SimplexData, const D: usize> GeometryBackend
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
        self.dt.vertices().map(|(key, _)| self.vertex_handle(key))
    }

    fn edges(&self) -> impl Iterator<Item = Self::EdgeHandle> + '_ {
        self.dt.edges().map(|key| self.edge_handle(key))
    }

    fn faces(&self) -> impl Iterator<Item = Self::FaceHandle> + '_ {
        self.dt.simplices().map(|(key, _)| self.face_handle(key))
    }

    fn vertex_coordinates<'a>(
        &'a self,
        vertex: &Self::VertexHandle,
    ) -> Result<&'a [Self::Coordinate], Self::Error> {
        let key = self.validate_vertex_handle(vertex)?;
        let coords = self
            .dt
            .vertex_coords(key)
            .ok_or(DelaunayError::InvalidVertex { key })?;
        Ok(coords)
    }

    fn face_vertices<'a>(
        &'a self,
        face: &Self::FaceHandle,
    ) -> Result<impl ExactSizeIterator<Item = Self::VertexHandle> + 'a, Self::Error> {
        let key = self.validate_face_handle(face)?;
        let vkeys = self
            .dt
            .simplex_vertices(key)
            .map_err(|_| DelaunayError::InvalidFace { key })?;
        Ok(vkeys.iter().copied().map(|key| self.vertex_handle(key)))
    }

    fn edge_endpoints(
        &self,
        edge: &Self::EdgeHandle,
    ) -> Result<(Self::VertexHandle, Self::VertexHandle), Self::Error> {
        let key = self.validate_edge_handle(edge)?;
        let (v0, v1) = key.endpoints();
        Ok((self.vertex_handle(v0), self.vertex_handle(v1)))
    }

    fn edge_adjacent_faces(
        &self,
        edge: &Self::EdgeHandle,
    ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error> {
        let edge_key = self.validate_edge_handle(edge)?;

        let Some(facet) = self.interior_facet_for_edge(edge_key) else {
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
                self.vertex_handle(endpoint_0),
                self.vertex_handle(endpoint_1),
            ),
            (self.face_handle(face_0), self.face_handle(face_1)),
            (
                self.vertex_handle(vertices_0[facet_index]),
                self.vertex_handle(opposite_1),
            ),
        )))
    }

    fn adjacent_faces<'a>(
        &'a self,
        vertex: &Self::VertexHandle,
    ) -> Result<impl Iterator<Item = Self::FaceHandle> + 'a, Self::Error> {
        let key = self.validate_vertex_handle(vertex)?;
        Ok(self
            .dt
            .as_triangulation()
            .adjacent_simplices(key)
            .map(|key| self.face_handle(key)))
    }

    fn incident_edges<'a>(
        &'a self,
        vertex: &Self::VertexHandle,
    ) -> Result<impl Iterator<Item = Self::EdgeHandle> + 'a, Self::Error> {
        let key = self.validate_vertex_handle(vertex)?;
        Ok(self.dt.incident_edges(key).map(|key| self.edge_handle(key)))
    }

    fn face_neighbors<'a>(
        &'a self,
        face: &Self::FaceHandle,
    ) -> Result<impl Iterator<Item = Self::FaceHandle> + 'a, Self::Error> {
        let key = self.validate_face_handle(face)?;
        Ok(self
            .dt
            .simplex_neighbors(key)
            .map(|key| self.face_handle(key)))
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

/// Identifies which layer owns rollback for a topology mutation.
#[derive(Clone, Copy)]
enum MutationRollback {
    /// The backend must restore itself before returning an error.
    Backend,
    /// An enclosing CDT transaction restores or discards the whole state.
    Caller,
}

impl<VertexData: DataType, SimplexData: DataType, const D: usize>
    DelaunayBackend<VertexData, SimplexData, D>
{
    /// Opens a topology mutation with the requested rollback ownership.
    fn mutation(
        &mut self,
        rollback: MutationRollback,
    ) -> DelaunayMutation<'_, VertexData, SimplexData, D> {
        match rollback {
            MutationRollback::Backend => DelaunayMutation::new(self),
            MutationRollback::Caller => DelaunayMutation::without_snapshot(self),
        }
    }

    /// Removes one vertex with either backend- or caller-owned rollback.
    fn remove_vertex_with_rollback(
        &mut self,
        vertex: &DelaunayVertexHandle,
        rollback: MutationRollback,
    ) -> Result<DelaunayRemovalResult, DelaunayError> {
        let vertex_key = self.validate_vertex_handle(vertex)?;

        let inverse_k1 = self.dt.can_flip_k1_remove(vertex_key).ok();
        let removed_edges = inverse_k1
            .as_ref()
            .map(|feasibility| {
                self.local_edges_for_simplices(
                    &feasibility.removed_simplices,
                    DelaunayOperation::FlipK1Remove,
                )
            })
            .transpose()?;
        let mut mutation = self.mutation(rollback);
        let removal = if inverse_k1.is_some() {
            mutation
                .dt
                .flip_k1_remove(vertex_key)
                .map(Some)
                .map_err(|err| err.to_string())
        } else {
            mutation
                .dt
                .delete_vertex(vertex_key)
                .map(|_| None)
                .map_err(|err| err.to_string())
        };
        let info = removal.map_err(|err| DelaunayError::RemovalFailed {
            operation: if inverse_k1.is_some() {
                DelaunayOperation::FlipK1Remove
            } else {
                DelaunayOperation::RemoveVertex
            },
            target: format!("vertex {:?}", vertex.key),
            detail: err,
        })?;
        let new_faces = if let (Some(removed_edges), Some(info)) = (removed_edges, info) {
            mutation.update_interior_facet_index(
                &removed_edges,
                &info.new_simplices,
                DelaunayOperation::FlipK1Remove,
            )?;
            info.new_simplices
                .iter()
                .copied()
                .map(|key| mutation.face_handle(key))
                .collect()
        } else {
            mutation.rebuild_interior_facet_index();
            Vec::new()
        };
        if inverse_k1.is_none() {
            mutation.validate_embedding_after_mutation(
                DelaunayOperation::RemoveVertex,
                format!("vertex {:?}", vertex.key),
            )?;
        }
        mutation.commit();
        Ok(DelaunayRemovalResult { new_faces })
    }

    /// Flips one edge with either backend- or caller-owned rollback.
    fn flip_edge_with_rollback(
        &mut self,
        edge: &DelaunayEdgeHandle,
        rollback: MutationRollback,
    ) -> Result<FlipResult<DelaunayEdgeHandle, DelaunayFaceHandle>, DelaunayError> {
        let edge_key = self.validate_edge_handle(edge)?;
        let facet = if self.edge_exists(edge_key) {
            self.interior_facet_for_edge(edge_key).ok_or_else(|| {
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
        let removed_simplices =
            self.simplices_adjacent_to_facet(facet, DelaunayOperation::FlipK2)?;
        let removed_edges =
            self.local_edges_for_simplices(&removed_simplices, DelaunayOperation::FlipK2)?;
        let mut mutation = self.mutation(rollback);
        let info = mutation
            .dt
            .flip_k2(facet)
            .map_err(|err| DelaunayError::FlipFailed {
                operation: DelaunayOperation::FlipK2,
                target: format!(
                    "edge {:?} -- {:?} via facet {:?}",
                    edge.key.v0(),
                    edge.key.v1(),
                    facet
                ),
                detail: err.to_string(),
            })?;
        let mut inserted = info.inserted_face_vertices.iter().copied();
        let Some(v0) = inserted.next() else {
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                    expected: 2,
                    actual: 0,
                    first_unexpected: None,
                },
            });
        };
        let Some(v1) = inserted.next() else {
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                    expected: 2,
                    actual: 1,
                    first_unexpected: None,
                },
            });
        };
        if let Some(extra) = inserted.next() {
            let actual = 3 + inserted.count();
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                target: format!("edge {:?} -- {:?}", edge.key.v0(), edge.key.v1()),
                failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                    expected: 2,
                    actual,
                    first_unexpected: Some(format!("{extra:?}")),
                },
            });
        }
        let replacement_edge = mutation.replacement_edge_key(v0, v1)?;
        mutation.update_interior_facet_index(
            &removed_edges,
            &info.new_simplices,
            DelaunayOperation::FlipK2,
        )?;
        let affected_faces = info
            .new_simplices
            .iter()
            .copied()
            .map(|key| mutation.face_handle(key))
            .collect();
        let result = FlipResult::new(mutation.edge_handle(replacement_edge), affected_faces);
        mutation.commit();
        Ok(result)
    }

    /// Subdivides one face with either backend- or caller-owned rollback.
    fn subdivide_face_with_rollback(
        &mut self,
        face: DelaunayFaceHandle,
        point: &[f64],
        rollback: MutationRollback,
    ) -> Result<SubdivisionResult<DelaunayVertexHandle, DelaunayFaceHandle>, DelaunayError> {
        let face_key = self.validate_face_handle(&face)?;

        let vertex = Self::build_vertex(point, None, DelaunayOperation::SubdivideFace)?;
        let mut mutation = self.mutation(rollback);
        let removed_edges =
            mutation.local_edges_for_simplices(&[face_key], DelaunayOperation::FlipK1Insert)?;
        let info = mutation
            .dt
            .flip_k1_insert(face_key, vertex)
            .map_err(|err| DelaunayError::FlipFailed {
                operation: DelaunayOperation::FlipK1Insert,
                target: format!("face {:?} at point {:?}", face.key, point),
                detail: err.to_string(),
            })?;
        let mut inserted = info.inserted_face_vertices.iter().copied();
        let Some(new_vertex) = inserted.next() else {
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK1Insert,
                target: format!("face {:?} at point {:?}", face.key, point),
                failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                    expected: 1,
                    actual: 0,
                    first_unexpected: None,
                },
            });
        };
        if let Some(extra) = inserted.next() {
            let actual = 2 + inserted.count();
            return Err(DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK1Insert,
                target: format!("face {:?} at point {:?}", face.key, point),
                failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                    expected: 1,
                    actual,
                    first_unexpected: Some(format!("{extra:?}")),
                },
            });
        }
        mutation.update_interior_facet_index(
            &removed_edges,
            &info.new_simplices,
            DelaunayOperation::FlipK1Insert,
        )?;
        let result = SubdivisionResult::new(
            mutation.vertex_handle(new_vertex),
            info.new_simplices
                .iter()
                .copied()
                .map(|key| mutation.face_handle(key))
                .collect(),
            face,
        );
        mutation.commit();
        Ok(result)
    }

    /// Removes a vertex inside an enclosing rollback or discard boundary.
    pub(crate) fn remove_vertex_in_caller_transaction(
        &mut self,
        vertex: &DelaunayVertexHandle,
    ) -> Result<DelaunayRemovalResult, DelaunayError> {
        self.remove_vertex_with_rollback(vertex, MutationRollback::Caller)
    }

    /// Returns the faces an inverse k=1 vertex removal would replace.
    pub(crate) fn removal_affected_faces(
        &self,
        vertex: &DelaunayVertexHandle,
    ) -> Result<Vec<DelaunayFaceHandle>, DelaunayError> {
        let vertex_key = self.validate_vertex_handle(vertex)?;
        self.dt
            .can_flip_k1_remove(vertex_key)
            .map(|feasibility| {
                feasibility
                    .removed_simplices
                    .iter()
                    .copied()
                    .map(|key| self.face_handle(key))
                    .collect()
            })
            .map_err(|err| DelaunayError::RemovalFailed {
                operation: DelaunayOperation::FlipK1Remove,
                target: format!("vertex {:?}", vertex.key),
                detail: err.to_string(),
            })
    }

    /// Flips an edge inside an enclosing rollback or discard boundary.
    pub(crate) fn flip_edge_in_caller_transaction(
        &mut self,
        edge: &DelaunayEdgeHandle,
    ) -> Result<FlipResult<DelaunayEdgeHandle, DelaunayFaceHandle>, DelaunayError> {
        self.flip_edge_with_rollback(edge, MutationRollback::Caller)
    }

    /// Subdivides a face inside an enclosing rollback or discard boundary.
    pub(crate) fn subdivide_face_in_caller_transaction(
        &mut self,
        face: DelaunayFaceHandle,
        point: &[f64],
    ) -> Result<SubdivisionResult<DelaunayVertexHandle, DelaunayFaceHandle>, DelaunayError> {
        self.subdivide_face_with_rollback(face, point, MutationRollback::Caller)
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
        let mut mutation = DelaunayMutation::new(self);
        let key =
            mutation
                .dt
                .insert_vertex(vertex)
                .map_err(|err| DelaunayError::InsertionFailed {
                    operation: DelaunayOperation::InsertVertex,
                    coordinates: coords.to_vec(),
                    detail: err.to_string(),
                })?;
        mutation.rebuild_interior_facet_index();
        mutation.validate_embedding_after_mutation(
            DelaunayOperation::InsertVertex,
            format!("{coords:?}"),
        )?;
        let handle = mutation.vertex_handle(key);
        mutation.commit();
        Ok(handle)
    }

    fn remove_vertex(&mut self, vertex: Self::VertexHandle) -> Result<(), Self::Error> {
        self.remove_vertex_with_rollback(&vertex, MutationRollback::Backend)
            .map(|_| ())
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
        self.flip_edge_with_rollback(&edge, MutationRollback::Backend)
    }

    fn can_flip_edge(&self, edge: &Self::EdgeHandle) -> bool {
        self.validate_edge_handle(edge)
            .ok()
            .and_then(|key| self.interior_facet_for_edge(key))
            .is_some_and(|facet| self.dt.can_flip_k2(facet).is_ok())
    }

    fn can_subdivide_face(&self, face: &Self::FaceHandle, point: &[Self::Coordinate]) -> bool {
        let Ok(vertex) = Self::build_vertex(point, None, DelaunayOperation::SubdivideFace) else {
            return false;
        };
        self.validate_face_handle(face)
            .is_ok_and(|key| self.dt.can_flip_k1_insert(key, &vertex).is_ok())
    }

    fn can_collapse_vertex(&self, vertex: &Self::VertexHandle) -> bool {
        self.validate_vertex_handle(vertex)
            .is_ok_and(|key| self.dt.can_flip_k1_remove(key).is_ok())
    }

    fn subdivide_face(
        &mut self,
        face: Self::FaceHandle,
        point: &[Self::Coordinate],
    ) -> Result<SubdivisionResult<Self::VertexHandle, Self::FaceHandle>, Self::Error> {
        self.subdivide_face_with_rollback(face, point, MutationRollback::Backend)
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
    use crate::CdtError;
    use crate::geometry::DelaunayBackend2D;
    use crate::geometry::generators::{
        DelaunayTriangulation2D, build_delaunay2_from_simplices, build_delaunay2_with_data,
        generate_delaunay2, random_delaunay2, seeded_delaunay2,
    };
    use crate::{CdtTriangulation, CdtValidationProfile};
    use approx::assert_relative_eq;
    use delaunay::DelaunayRepairPolicy;
    use delaunay::prelude::construction::{ConstructionOptions, DelaunayTriangulationBuilder};
    use serde_json::{Error as JsonError, Value, error::Category, from_str, to_string, to_value};
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
            owner_id: Uuid::new_v4(),
        }
    }

    /// `serde_json` wraps custom visitor failures as data errors; assert that
    /// structured category first, then keep detail matching in one place.
    fn assert_json_data_error(error: &JsonError, expected_details: &[&str]) {
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
        let json = to_string(&backend).expect("backend should serialize");
        let invalid_json = json.replace(
            r#""delaunay_check_policy":"EndOnly""#,
            r#""delaunay_check_policy":{"EveryN":0}"#,
        );

        let error = from_str::<DelaunayBackend2D>(&invalid_json)
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
            failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                expected: 2,
                actual: 1,
                first_unexpected: None,
            },
        };
        assert_eq!(
            malformed.to_string(),
            "flip_k2 returned unexpected output for edge VertexKey(1v1) -- VertexKey(2v1): reported 1 inserted-face vertices, expected 2"
        );

        let malformed_insert = DelaunayError::UnexpectedFlipOutput {
            operation: DelaunayOperation::FlipK1Insert,
            target: "face SimplexKey(3v1) at point [0.5, 0.5]".to_string(),
            failure: DelaunayFlipOutputFailure::InsertedVertexCountMismatch {
                expected: 1,
                actual: 2,
                first_unexpected: Some("VertexKey(4v1)".to_string()),
            },
        };
        assert_eq!(
            malformed_insert.to_string(),
            "flip_k1_insert returned unexpected output for face SimplexKey(3v1) at point [0.5, 0.5]: reported 2 inserted-face vertices, expected 1; first unexpected vertex VertexKey(4v1)"
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
            Err(CdtError::DelaunayValidationFailed {
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
            Err(CdtError::DelaunayValidationFailed {
                level: DelaunayValidationLevel::Five,
                ..
            })
        );
    }

    #[test]
    fn embedding_validation_errors_trigger_transaction_rollback() {
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
            let serialized_before = to_value(&backend).expect("backend should serialize");
            let expected_facets = backend.interior_facets_by_edge.clone();
            let error = {
                let vertex = DelaunayBackend::<u32, i32, 2>::build_vertex(
                    &[0.25, 0.25],
                    None,
                    DelaunayOperation::InsertVertex,
                )
                .expect("test vertex should build");
                let mut mutation = DelaunayMutation::new(&mut backend);
                mutation
                    .dt
                    .insert_vertex(vertex)
                    .expect("inside-point insertion should mutate the guarded backend");
                mutation.rebuild_interior_facet_index();
                DelaunayBackend::<u32, i32, 2>::map_embedding_validation_error(
                    Err(validation_error),
                    DelaunayOperation::InsertVertex,
                    "[0.25, 0.25]".to_string(),
                )
                .expect_err("failed embedding validation should reject the mutation")
            };

            assert_matches!(
                error,
                DelaunayError::ValidationFailed {
                    level: DelaunayValidationLevel::Four,
                    detail,
                } if detail == expected_detail
            );
            assert_eq!(
                to_value(&backend).expect("restored backend should serialize"),
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
        let invalid_handle = backend.vertex_handle(bogus_key);
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
            let vertices: Vec<_> = backend
                .face_vertices(face)
                .expect("Should retrieve vertices for valid face")
                .collect();
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
        let invalid_handle = backend.face_handle(bogus_key);
        let Err(err) = backend.face_vertices(&invalid_handle) else {
            panic!("invalid face handle should fail");
        };
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
        let invalid_handle = backend.face_handle(bogus_key);

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
                        let [x, y] = coordinates else {
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
            .next()
            .expect("inserted vertex should have at least one incident edge");
        backend
            .remove_vertex(subdivision.new_vertex)
            .expect("inserted vertex should be removable");
        assert!(
            matches!(
                backend.edge_endpoints(&invalid_handle),
                Err(DelaunayError::StaleHandle {
                    kind: DelaunayHandleKind::Edge,
                    ..
                })
            ),
            "stale edge handle should return a provenance error"
        );
    }

    #[test]
    fn detached_handles_enforce_owner_and_topology_generation() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1)])
            .expect("triangle fixture should build");
        let mut backend = validated_backend(dt);
        let foreign_owner = backend.clone();
        let vertex = backend.vertices().next().expect("fixture has a vertex");
        let edge = backend.edges().next().expect("fixture has an edge");
        let face = backend.faces().next().expect("fixture has a face");

        assert!(HashSet::from([vertex.clone()]).contains(&vertex));
        assert_matches!(
            foreign_owner.vertex_coordinates(&vertex),
            Err(DelaunayError::ForeignHandle {
                kind: DelaunayHandleKind::Vertex,
                ..
            })
        );
        assert_matches!(
            foreign_owner.edge_endpoints(&edge),
            Err(DelaunayError::ForeignHandle {
                kind: DelaunayHandleKind::Edge,
                ..
            })
        );
        let Err(error) = foreign_owner.face_vertices(&face) else {
            panic!("a cloned backend must reject the source owner's face handle");
        };
        assert_matches!(
            error,
            DelaunayError::ForeignHandle {
                kind: DelaunayHandleKind::Face,
                ..
            }
        );

        backend
            .subdivide_face(face.clone(), &[0.25, 0.25])
            .expect("subdivision should advance the topology generation");
        assert_matches!(
            backend.vertex_coordinates(&vertex),
            Err(DelaunayError::StaleHandle {
                kind: DelaunayHandleKind::Vertex,
                ..
            })
        );
        assert_matches!(
            backend.edge_endpoints(&edge),
            Err(DelaunayError::StaleHandle {
                kind: DelaunayHandleKind::Edge,
                ..
            })
        );
        let Err(error) = backend.face_vertices(&face) else {
            panic!("a topology mutation must stale previously issued face handles");
        };
        assert_matches!(
            error,
            DelaunayError::StaleHandle {
                kind: DelaunayHandleKind::Face,
                ..
            }
        );
    }

    #[test]
    fn checked_payload_accessors_update_live_handles_and_reject_invalid_provenance() {
        let dt =
            build_delaunay2_with_data(&[([0.0, 0.0], 0_u32), ([1.0, 0.0], 0), ([0.0, 1.0], 1)])
                .expect("triangle fixture should build");
        let mut backend = validated_backend(dt);
        let mut foreign_owner = backend.clone();
        let vertex = backend.vertices().next().expect("fixture has a vertex");
        let face = backend.faces().next().expect("fixture has a face");
        let original_vertex_data = backend
            .vertex_data(&vertex)
            .expect("live vertex payload should be readable");
        let original_simplex_data = backend
            .simplex_data(&face)
            .expect("live simplex payload should be readable");

        assert_eq!(
            backend
                .set_vertex_data(&vertex, Some(7))
                .expect("live vertex payload should be writable"),
            original_vertex_data
        );
        assert_eq!(
            backend
                .vertex_data(&vertex)
                .expect("updated vertex payload should be readable"),
            Some(7)
        );
        assert_eq!(
            backend
                .set_simplex_data(&face, Some(-3))
                .expect("live simplex payload should be writable"),
            original_simplex_data
        );
        assert_eq!(
            backend
                .simplex_data(&face)
                .expect("updated simplex payload should be readable"),
            Some(-3)
        );

        assert_matches!(
            foreign_owner.set_vertex_data(&vertex, Some(9)),
            Err(DelaunayError::ForeignHandle {
                kind: DelaunayHandleKind::Vertex,
                ..
            })
        );
        assert_matches!(
            foreign_owner.set_simplex_data(&face, Some(9)),
            Err(DelaunayError::ForeignHandle {
                kind: DelaunayHandleKind::Face,
                ..
            })
        );

        backend
            .subdivide_face(face.clone(), &[0.25, 0.25])
            .expect("subdivision should advance the topology generation");
        assert_matches!(
            backend.set_vertex_data(&vertex, Some(9)),
            Err(DelaunayError::StaleHandle {
                kind: DelaunayHandleKind::Vertex,
                ..
            })
        );
        assert_matches!(
            backend.set_simplex_data(&face, Some(9)),
            Err(DelaunayError::StaleHandle {
                kind: DelaunayHandleKind::Face,
                ..
            })
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
            let adjacent: Vec<_> = backend
                .adjacent_faces(vertex)
                .expect("Should retrieve adjacent faces for valid vertex")
                .collect();
            assert!(
                !adjacent.is_empty(),
                "Each vertex should have at least one adjacent face"
            );

            // Verify each adjacent face contains this vertex
            for face_handle in &adjacent {
                let mut face_vertices = backend
                    .face_vertices(face_handle)
                    .expect("Should retrieve face vertices");
                assert!(
                    face_vertices.any(|candidate| candidate == *vertex),
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
        let vertex_id = backend
            .stable_vertex_id(&vertex)
            .expect("fixture vertex should have stable identity");

        let first: Vec<_> = backend
            .adjacent_faces(&vertex)
            .expect("first adjacency query should read maintained incidence")
            .collect();
        let second: Vec<_> = backend
            .adjacent_faces(&vertex)
            .expect("second adjacency query should read the same incidence")
            .collect();
        assert_eq!(first, second);

        let face = backend
            .faces()
            .next()
            .expect("triangle fixture should contain a face");
        backend
            .subdivide_face(face, &[0.25, 0.25])
            .expect("subdivision should update maintained incidence");
        let vertex = backend
            .resolve_vertex_id(vertex_id)
            .expect("stable fixture vertex should survive subdivision");
        assert!(
            backend
                .adjacent_faces(&vertex)
                .expect("adjacency query after mutation should read updated incidence")
                .count()
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
            let incident: Vec<_> = backend
                .incident_edges(vertex)
                .expect("Should retrieve incident edges for valid vertex")
                .collect();
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
            let neighbors: Vec<_> = backend
                .face_neighbors(face)
                .expect("Should retrieve neighbors for valid face")
                .collect();

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
        let invalid_handle = backend.face_handle(bogus_key);
        let Err(err) = backend.face_neighbors(&invalid_handle) else {
            panic!("invalid face handle should fail");
        };
        assert_matches!(err, DelaunayError::InvalidFace { key } if key == bogus_key);
    }

    #[test]
    fn test_adjacent_faces_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);

        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = backend.vertex_handle(bogus_key);
        let Err(err) = backend.adjacent_faces(&invalid_handle) else {
            panic!("invalid vertex handle should fail");
        };
        assert_matches!(err, DelaunayError::InvalidVertex { key } if key == bogus_key);
    }

    #[test]
    fn test_incident_edges_invalid_handle() {
        let dt = random_delaunay2(3, (0.0, 10.0));
        let backend = validated_backend(dt);

        let bogus_key = VertexKey::from(KeyData::from_ffi(u64::MAX));
        let invalid_handle = backend.vertex_handle(bogus_key);
        let Err(err) = backend.incident_edges(&invalid_handle) else {
            panic!("invalid vertex handle should fail");
        };
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
            let neighbors: Vec<_> = backend
                .face_neighbors(&face)
                .expect("Should retrieve neighbors")
                .collect();
            for neighbor in &neighbors {
                let mut reverse = backend
                    .face_neighbors(neighbor)
                    .expect("Neighbor should have neighbors");
                assert!(
                    reverse.any(|candidate| candidate == face),
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
        assert!(backend.can_flip_edge(&edge));
        backend
            .flip_edge(edge)
            .expect("interior square edge should flip");
        assert_eq!(backend.vertex_count(), original_vertex_count);
        assert_eq!(backend.face_count(), original_face_count);
        assert!(backend.is_valid());

        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("labeled triangle should build");
        let mut backend = validated_backend(dt);
        let original_vertex_count = backend.vertex_count();
        let original_face_count = backend.face_count();
        let face = backend.faces().next().expect("valid face handle");
        assert!(!backend.can_subdivide_face(&face, &[0.5, 0.0]));
        assert_matches!(
            backend.subdivide_face(face.clone(), &[0.5, 0.0]),
            Err(DelaunayError::FlipFailed {
                operation: DelaunayOperation::FlipK1Insert,
                ..
            })
        );
        assert_eq!(backend.vertex_count(), original_vertex_count);
        assert_eq!(backend.face_count(), original_face_count);
        assert!(backend.can_subdivide_face(&face, &[0.5, 1.0 / 3.0]));
        let subdivide = backend
            .subdivide_face(face, &[0.5, 1.0 / 3.0])
            .expect("face subdivision should use k=1 flip");
        assert_eq!(backend.vertex_count(), original_vertex_count + 1);
        assert_eq!(backend.face_count(), original_face_count + 2);
        assert!(backend.is_valid());

        assert!(backend.can_collapse_vertex(&subdivide.new_vertex));
        let (): () = backend
            .remove_vertex(subdivide.new_vertex)
            .expect("degree-3 inserted vertex should be removable");
        assert_eq!(backend.vertex_count(), original_vertex_count);
        assert_eq!(backend.face_count(), original_face_count);
        assert!(backend.is_valid());

        backend
            .insert_vertex(&[0.25, 0.75])
            .expect("valid interior point should insert");
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
        let bogus_vertex_handle = backend.vertex_handle(bogus_vertex);
        assert!(!backend.can_collapse_vertex(&bogus_vertex_handle));
        assert_matches!(
            backend.remove_vertex(bogus_vertex_handle),
            Err(DelaunayError::InvalidVertex { key }) if key == bogus_vertex,
        );

        let bogus_face = SimplexKey::from(KeyData::from_ffi(u64::MAX));
        let bogus_face_handle = backend.face_handle(bogus_face);
        assert!(!backend.can_subdivide_face(&bogus_face_handle, &[0.25, 0.25]));
        assert_matches!(
            backend.subdivide_face(bogus_face_handle, &[0.25, 0.25]),
            Err(DelaunayError::InvalidFace { key }) if key == bogus_face,
        );
    }

    #[test]
    fn boundary_edits_fail_exact_preflights() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("labeled triangle should build");
        let mut backend = validated_backend(dt);
        let vertex = backend
            .vertices()
            .next()
            .expect("single triangle has boundary vertices");
        assert!(!backend.can_collapse_vertex(&vertex));
        let edge = backend
            .edges()
            .next()
            .expect("single triangle has boundary edges");
        assert_matches!(
            backend.flip_edge(edge),
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
        let serialized_before = to_value(&backend).expect("backend should serialize");
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
            to_value(&backend).expect("restored backend should serialize"),
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
        let mut value = to_value(&backend).expect("backend should serialize");
        set_non_delaunay_quad_diagonal(&mut value);
        let invalid_json = to_string(&value).expect("corrupt backend should serialize");

        let error = from_str::<DelaunayBackend2D>(&invalid_json)
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
        assert_interior_facet_index_matches_rebuild(&backend);
    }

    #[test]
    fn local_facet_index_updates_match_full_rebuild_after_inverse_volume_pair() {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], 0),
            ([1.0, 0.0], 0),
            ([0.0, 1.0], 1),
            ([1.0, 1.0], 1),
        ])
        .expect("subdivision fixture should build");
        let mut backend = validated_backend(dt);
        let face = backend.faces().next().expect("fixture has a face");
        let point = backend
            .face_barycenter(&face)
            .expect("face barycenter should resolve");
        let subdivision = backend
            .subdivide_face(face, &point)
            .expect("face subdivision should succeed");
        assert_interior_facet_index_matches_rebuild(&backend);

        backend
            .remove_vertex(subdivision.new_vertex)
            .expect("inserted degree-3 vertex should collapse");
        assert_interior_facet_index_matches_rebuild(&backend);
    }

    fn assert_interior_facet_index_matches_rebuild(backend: &DelaunayBackend2D) {
        let rebuilt = DelaunayBackend2D::build_interior_facets_by_edge(&backend.dt);
        let actual_edges: std::collections::HashSet<_> =
            backend.interior_facets_by_edge.keys().copied().collect();
        let rebuilt_edges: std::collections::HashSet<_> = rebuilt.keys().copied().collect();
        assert_eq!(actual_edges, rebuilt_edges);
    }

    #[test]
    fn replacement_edge_lookup_failure_triggers_transaction_rollback() {
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
        let serialized_before = to_value(&backend).expect("backend should serialize");
        let expected_facets = backend.interior_facets_by_edge.clone();
        let error = {
            let facet = *backend
                .interior_facets_by_edge
                .values()
                .next()
                .expect("square should have one interior facet");
            let mut mutation = DelaunayMutation::new(&mut backend);
            let info = mutation
                .dt
                .flip_k2(facet)
                .expect("interior edge should flip");
            mutation.rebuild_interior_facet_index();
            let live_vertex = info.inserted_face_vertices[0];
            let missing_vertex = VertexKey::from(KeyData::from_ffi(u64::MAX));
            mutation
                .replacement_edge_key(live_vertex, missing_vertex)
                .expect_err("missing replacement vertex should fail edge reconstruction")
        };

        assert_matches!(
            error,
            DelaunayError::UnexpectedFlipOutput {
                operation: DelaunayOperation::FlipK2,
                ..
            }
        );
        assert_eq!(
            to_value(&backend).expect("restored backend should serialize"),
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
