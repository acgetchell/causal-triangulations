#![forbid(unsafe_code)]

//! Foliation data structures for Causal Dynamical Triangulations.
//!
//! A **foliation** assigns each vertex to a discrete time slice, enabling
//! classification of edges as spacelike (within a slice), timelike (between
//! adjacent slices), or acausal (spanning multiple slices).
//! This is the core causal structure of CDT.
//!
//! Time labels are stored directly as vertex data in the Delaunay triangulation
//! (`Vertex<f64, u32, 2>` — the `u32` is the time-slice index). This mirrors
//! CGAL's `vertex->info()` used in CDT-plusplus.  The `Foliation` struct
//! tracks only aggregate bookkeeping (per-slice counts and total slices).

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU32;
use std::{error::Error, fmt};

/// Classification of an edge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    /// Both endpoints share the same time slice.
    Spacelike,
    /// Endpoints are in adjacent time slices (|Δt| = 1).
    Timelike,
    /// Endpoints span more than one time slice (|Δt| > 1), violating causality.
    Acausal,
}

/// Classification of a triangle (2-simplex) in a foliated 1+1 CDT.
///
/// In a valid foliated triangulation every triangle spans exactly two
/// adjacent time slices.  The type is determined by how many vertices
/// sit on the lower vs. upper slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SimplexType {
    /// **(2,1)** — two vertices at time *t*, one at *t + 1*.
    /// The spacelike base is in the lower slice.
    Up,
    /// **(1,2)** — one vertex at time *t*, two at *t + 1*.
    /// The spacelike base is in the upper slice.
    Down,
}

impl SimplexType {
    /// Encode as the `i32` value stored in simplex data.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::SimplexType;
    ///
    /// assert_eq!(SimplexType::Up.to_i32(), 1);
    /// assert_eq!(SimplexType::Down.to_i32(), -1);
    /// ```
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::Up => 1,
            Self::Down => -1,
        }
    }

    /// Decode from the `i32` value stored in simplex data.
    ///
    /// Returns `None` for values that do not represent a valid simplex type.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::SimplexType;
    ///
    /// assert_eq!(SimplexType::from_i32(1), Some(SimplexType::Up));
    /// assert_eq!(SimplexType::from_i32(0), None);
    /// ```
    #[must_use]
    pub const fn from_i32(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::Up),
            -1 => Some(Self::Down),
            _ => None,
        }
    }
}

/// Classifies an edge given the time labels of its two endpoints.
///
/// Returns `None` if either label is `None` (unlabeled vertex).
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::triangulation::{EdgeType, classify_edge};
///
/// assert_eq!(classify_edge(Some(2), Some(2)), Some(EdgeType::Spacelike));
/// assert_eq!(classify_edge(Some(2), Some(3)), Some(EdgeType::Timelike));
/// assert_eq!(classify_edge(Some(2), None), None);
/// ```
#[must_use]
pub fn classify_edge(t0: Option<u32>, t1: Option<u32>) -> Option<EdgeType> {
    let t0 = t0?;
    let t1 = t1?;
    if t0 == t1 {
        Some(EdgeType::Spacelike)
    } else if t0.abs_diff(t1) == 1 {
        Some(EdgeType::Timelike)
    } else {
        Some(EdgeType::Acausal)
    }
}

/// Classifies a triangle given the time labels of its three vertices.
///
/// Returns `None` if any label is missing, if the triangle is degenerate
/// (all vertices at the same time), or if it spans more than one time slice.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::triangulation::{SimplexType, classify_simplex};
///
/// assert_eq!(classify_simplex(Some(0), Some(0), Some(1)), Some(SimplexType::Up));
/// assert_eq!(classify_simplex(Some(0), Some(1), Some(1)), Some(SimplexType::Down));
/// assert_eq!(classify_simplex(Some(0), Some(0), Some(0)), None);
/// ```
#[must_use]
pub fn classify_simplex(t0: Option<u32>, t1: Option<u32>, t2: Option<u32>) -> Option<SimplexType> {
    let t0 = t0?;
    let t1 = t1?;
    let t2 = t2?;

    let min_t = t0.min(t1).min(t2);
    let max_t = t0.max(t1).max(t2);

    // Must span exactly one time slice (adjacent slices)
    if max_t - min_t != 1 {
        return None;
    }

    let lower_count = [t0, t1, t2].iter().filter(|&&t| t == min_t).count();
    match lower_count {
        2 => Some(SimplexType::Up),   // (2,1): two at t, one at t+1
        1 => Some(SimplexType::Down), // (1,2): one at t, two at t+1
        _ => None,                    // unreachable for 3 vertices spanning 2 values
    }
}

/// Error type for foliation construction and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FoliationError {
    /// No time slices were supplied for foliation bookkeeping.
    EmptyFoliation,
    /// `slice_sizes` length does not match `num_slices`.
    SliceSizeMismatch {
        /// Actual length of the `slice_sizes` vector.
        slice_sizes_len: usize,
        /// Expected number of slices.
        num_slices: u32,
    },
    /// The number of labeled vertices does not match the triangulation vertex count.
    LabelCountMismatch {
        /// Number of vertices with time labels.
        labeled: usize,
        /// Expected vertex count from the triangulation.
        expected: usize,
    },
    /// A specific vertex is missing a live time label.
    MissingVertexLabel {
        /// Zero-based index of the vertex in backend iteration order.
        vertex: usize,
    },
    /// A specific vertex has a time label outside the allowed slice range.
    OutOfRangeVertexLabel {
        /// Zero-based index of the vertex in backend iteration order.
        vertex: usize,
        /// Observed label value on that vertex.
        label: u32,
        /// Exclusive upper bound for valid labels (`0..expected_range_end`).
        expected_range_end: usize,
    },
    /// Live per-slice labeling does not match stored foliation bookkeeping.
    LabelMismatch {
        /// Slice index where mismatch was detected.
        slice: usize,
        /// Stored count for this slice.
        expected: usize,
        /// Live count observed from backend vertex labels.
        actual: usize,
    },
    /// Stored foliation bookkeeping no longer matches the current triangulation revision.
    StaleBookkeeping {
        /// Modification count when foliation bookkeeping was last synchronized.
        synced_at_modification: Option<u64>,
        /// Current triangulation modification count.
        current_modification_count: u64,
    },
    /// A time slice contains no vertices.
    EmptySlice {
        /// The index of the empty slice.
        slice: usize,
    },
    /// The sum of per-slice sizes does not match the labeled vertex count.
    SliceSizeSumMismatch {
        /// Sum of `slice_sizes`.
        sum: usize,
        /// Total labeled vertex count.
        labeled: usize,
    },
    /// The spacelike subgraph of a spatial slice does not contain every
    /// labeled vertex of that slice.
    ///
    /// On a toroidal CDT every vertex of slice `t` must participate in at
    /// least one spacelike edge so the spacelike subgraph forms a closed S¹.
    /// This variant fires when the number of distinct vertices observed in
    /// the spacelike edges of slice `t` differs from `slice_sizes[t]`.
    SpacelikeSubgraphSizeMismatch {
        /// Slice index whose spacelike subgraph has the wrong vertex count.
        slice: usize,
        /// Number of distinct vertices observed via spacelike edges.
        observed: usize,
        /// Expected vertex count from `slice_sizes[slice]`.
        expected: usize,
    },
    /// A vertex on a toroidal spatial slice has the wrong number of
    /// spacelike neighbours (it must have exactly 2 to form a closed S¹).
    SpacelikeDegreeViolation {
        /// Slice index where the violation was detected.
        slice: usize,
        /// Human-readable identifier for the offending vertex (e.g.
        /// `"VertexKey(123v1)"`).  Pre-formatted so this enum stays
        /// backend-agnostic.
        vertex: String,
        /// Observed number of spacelike neighbours (expected 2).
        observed_degree: usize,
    },
    /// The spacelike subgraph of a toroidal spatial slice is not a single
    /// closed S¹ cycle (e.g. multiple disjoint cycles, a non-simple cycle,
    /// or a cycle whose length differs from `slice_sizes[t]`).
    SpacelikeNonClosedRing {
        /// Slice index where the violation was detected.
        slice: usize,
        /// Number of vertices reached when walking from an arbitrary start.
        walked: usize,
        /// Expected number of vertices in the closed ring.
        expected: usize,
    },
    /// An open-boundary spatial slice has the wrong number of interval
    /// endpoints.
    SpacelikeOpenSliceEndpointCount {
        /// Slice index where the violation was detected.
        slice: usize,
        /// Number of degree-1 endpoints observed in the spacelike subgraph.
        observed: usize,
        /// Expected number of endpoints for this open interval.
        expected: usize,
    },
    /// A vertex on an open-boundary spatial slice has the wrong number of
    /// spacelike neighbours.
    SpacelikeOpenSliceDegreeViolation {
        /// Slice index where the violation was detected.
        slice: usize,
        /// Human-readable identifier for the offending vertex (e.g.
        /// `"VertexKey(123v1)"`). Pre-formatted so this enum stays
        /// backend-agnostic.
        vertex: String,
        /// Observed number of spacelike neighbours.
        observed_degree: usize,
    },
    /// The spacelike subgraph of an open-boundary spatial slice is not a
    /// single interval.
    SpacelikeNonOpenInterval {
        /// Slice index where the violation was detected.
        slice: usize,
        /// Number of vertices reached when walking from one endpoint.
        walked: usize,
        /// Expected number of vertices in the open interval.
        expected: usize,
    },
    /// Timelike edges in an open-boundary slab cross when the adjacent
    /// spatial slices are viewed as ordered intervals.
    OpenBoundarySlabEdgeCrossing {
        /// Lower time slice of the slab where the crossing was detected.
        lower_slice: usize,
        /// Human-readable identifier for the first crossing edge.
        first_edge: String,
        /// Human-readable identifier for the second crossing edge.
        second_edge: String,
    },
    /// The spacelike path order of an open-boundary slice disagrees with the
    /// backend spatial coordinate order.
    OpenBoundarySpatialOrderMismatch {
        /// Slice index where the mismatch was detected.
        slice: usize,
        /// Number of vertices in the combinatorial slice path.
        path_vertices: usize,
        /// Number of vertices in the coordinate-sorted slice path.
        coordinate_vertices: usize,
    },
    /// An open-boundary vertex's backend `y` coordinate disagrees with its
    /// integer foliation time label.
    OpenBoundaryTimeCoordinateMismatch {
        /// Slice label stored on the vertex.
        label: u32,
        /// Human-readable identifier for the offending vertex.
        vertex: String,
        /// Backend `y` coordinate.
        y: String,
    },
    /// A toroidal spatial slice is missing the timelike adjacency required
    /// for temporal wrap-around — every slice must have at least one
    /// timelike edge to both its `(t-1) mod T` and `(t+1) mod T` neighbours.
    ///
    /// This catches "toroidal-tagged but actually a cylinder" misuse where
    /// the underlying mesh has open time boundaries.  χ = 0 alone does not
    /// distinguish a torus from a cylinder, so we additionally verify the
    /// temporal wrap.
    MissingTemporalWrapAround {
        /// Slice index that is missing a timelike neighbour.
        slice: usize,
        /// The expected neighbour slice (either `(slice-1) mod T` or
        /// `(slice+1) mod T`) that has no incident timelike edge.
        missing_neighbor: usize,
    },
}

impl FoliationError {
    /// Returns whether this validation error describes a shape-invalid local
    /// proposal that should be treated as an ordinary geometric rejection after
    /// a successful backend mutation.
    ///
    /// Move kernels use this classification after a backend edit has already
    /// produced a structurally valid mesh, but evolved-CDT validation rejects
    /// the candidate because it no longer has the requested spatial topology or
    /// foliation shape. Stale bookkeeping, missing labels, and other unexpected
    /// validation failures remain hard errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::errors::FoliationError;
    ///
    /// let shape_error = FoliationError::SpacelikeOpenSliceEndpointCount {
    ///     slice: 2,
    ///     observed: 3,
    ///     expected: 2,
    /// };
    /// assert!(shape_error.is_post_mutation_candidate_rejection());
    ///
    /// let stale = FoliationError::StaleBookkeeping {
    ///     synced_at_modification: Some(4),
    ///     current_modification_count: 5,
    /// };
    /// assert!(!stale.is_post_mutation_candidate_rejection());
    /// ```
    #[must_use]
    pub const fn is_post_mutation_candidate_rejection(&self) -> bool {
        matches!(
            self,
            Self::SpacelikeSubgraphSizeMismatch { .. }
                | Self::SpacelikeDegreeViolation { .. }
                | Self::SpacelikeNonClosedRing { .. }
                | Self::SpacelikeOpenSliceEndpointCount { .. }
                | Self::SpacelikeOpenSliceDegreeViolation { .. }
                | Self::SpacelikeNonOpenInterval { .. }
                | Self::OpenBoundarySlabEdgeCrossing { .. }
                | Self::OpenBoundarySpatialOrderMismatch { .. }
                | Self::OpenBoundaryTimeCoordinateMismatch { .. }
                | Self::MissingTemporalWrapAround { .. }
        )
    }
}

impl fmt::Display for FoliationError {
    #[expect(
        clippy::too_many_lines,
        reason = "the public foliation error enum has many diagnostic variants, and keeping their Display text together makes coverage auditable"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFoliation => write!(f, "foliation must contain at least one time slice"),
            Self::SliceSizeMismatch {
                slice_sizes_len,
                num_slices,
            } => write!(
                f,
                "slice_sizes length ({slice_sizes_len}) != num_slices ({num_slices})"
            ),
            Self::LabelCountMismatch { labeled, expected } => write!(
                f,
                "labeled vertex count ({labeled}) does not match triangulation vertex count ({expected})"
            ),
            Self::MissingVertexLabel { vertex } => {
                write!(f, "vertex index {vertex} is missing a time label")
            }
            Self::OutOfRangeVertexLabel {
                vertex,
                label,
                expected_range_end,
            } => write!(
                f,
                "vertex index {vertex} has out-of-range time label {label}; expected 0..{expected_range_end}"
            ),
            Self::LabelMismatch {
                slice,
                expected,
                actual,
            } => write!(
                f,
                "time slice {slice} has stored count {expected}, but live labels report {actual}"
            ),
            Self::StaleBookkeeping {
                synced_at_modification,
                current_modification_count,
            } => write!(
                f,
                "stored foliation bookkeeping is stale: synchronized at modification \
                 {synced_at_modification:?}, current modification {current_modification_count}"
            ),
            Self::EmptySlice { slice } => write!(f, "time slice {slice} is empty"),
            Self::SliceSizeSumMismatch { sum, labeled } => write!(
                f,
                "slice_sizes sum ({sum}) does not match labeled vertex count ({labeled})"
            ),
            Self::SpacelikeSubgraphSizeMismatch {
                slice,
                observed,
                expected,
            } => write!(
                f,
                "spatial slice {slice} spacelike subgraph contains {observed} \
                 vertices but slice_sizes reports {expected}"
            ),
            Self::SpacelikeDegreeViolation {
                slice,
                vertex,
                observed_degree,
            } => write!(
                f,
                "toroidal spatial slice {slice} vertex {vertex} has {observed_degree} \
                 spacelike neighbours, expected exactly 2 for a closed S¹"
            ),
            Self::SpacelikeNonClosedRing {
                slice,
                walked,
                expected,
            } => write!(
                f,
                "toroidal spatial slice {slice} is not a single closed S¹: walked a \
                 cycle of length {walked} but slice has {expected} vertices"
            ),
            Self::SpacelikeOpenSliceEndpointCount {
                slice,
                observed,
                expected,
            } => write!(
                f,
                "open-boundary spatial slice {slice} has {observed} interval endpoints, \
                 expected {expected}"
            ),
            Self::SpacelikeOpenSliceDegreeViolation {
                slice,
                vertex,
                observed_degree,
            } => write!(
                f,
                "open-boundary spatial slice {slice} vertex {vertex} has {observed_degree} \
                 spacelike neighbours, expected 1 at interval endpoints and 2 in the interior"
            ),
            Self::SpacelikeNonOpenInterval {
                slice,
                walked,
                expected,
            } => write!(
                f,
                "open-boundary spatial slice {slice} is not a single interval: walked \
                 {walked} vertices but slice has {expected} vertices"
            ),
            Self::OpenBoundarySlabEdgeCrossing {
                lower_slice,
                first_edge,
                second_edge,
            } => write!(
                f,
                "open-boundary slab {lower_slice}->{upper_slice} has crossing timelike \
                 edges {first_edge} and {second_edge}",
                upper_slice = lower_slice.saturating_add(1)
            ),
            Self::OpenBoundarySpatialOrderMismatch {
                slice,
                path_vertices,
                coordinate_vertices,
            } => write!(
                f,
                "open-boundary spatial slice {slice} path order does not match backend \
                 x-coordinate order ({path_vertices} path vertices, {coordinate_vertices} \
                 coordinate-sorted vertices)"
            ),
            Self::OpenBoundaryTimeCoordinateMismatch { label, vertex, y } => write!(
                f,
                "open-boundary vertex {vertex} has time label {label} but backend y-coordinate {y}"
            ),
            Self::MissingTemporalWrapAround {
                slice,
                missing_neighbor,
            } => write!(
                f,
                "toroidal foliation has no timelike edge from slice {slice} to slice \
                 {missing_neighbor}; toroidal topology requires temporal wrap-around"
            ),
        }
    }
}

impl Error for FoliationError {}

/// Per-slice bookkeeping for a CDT triangulation.
///
/// Time labels are stored on vertices directly (as vertex data in the
/// Delaunay triangulation). This struct tracks only the per-slice vertex
/// counts and the total number of slices. Deserialization validates the same
/// invariants as [`Self::from_slice_sizes`], so checkpoint data cannot create
/// empty or mismatched slice bookkeeping.
#[derive(Debug, Clone, Serialize)]
pub struct Foliation {
    /// Number of vertices per time slice (`slice_sizes[t]`).
    slice_sizes: Vec<usize>,
    /// Nonzero total number of time slices.
    num_slices: NonZeroU32,
}

#[derive(Deserialize)]
struct SerializedFoliation {
    slice_sizes: Vec<usize>,
    num_slices: u32,
}

impl<'de> Deserialize<'de> for Foliation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = SerializedFoliation::deserialize(deserializer)?;
        Self::from_raw_slice_sizes(serialized.slice_sizes, serialized.num_slices)
            .map_err(D::Error::custom)
    }
}

impl Foliation {
    /// Creates a new foliation from pre-computed per-slice vertex counts.
    ///
    /// # Errors
    ///
    /// Returns error if there are no slices, if `slice_sizes.len() != num_slices`,
    /// or if any slice is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtResult, Foliation};
    /// use std::num::NonZeroU32;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let Some(num_slices) = NonZeroU32::new(2) else {
    ///         return Ok(());
    ///     };
    ///     let foliation = Foliation::from_slice_sizes(vec![3, 4], num_slices)?;
    ///     assert_eq!(foliation.num_slices().get(), 2);
    ///     assert_eq!(foliation.labeled_vertex_count(), 7);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_slice_sizes(
        slice_sizes: Vec<usize>,
        num_slices: NonZeroU32,
    ) -> Result<Self, FoliationError> {
        Self::from_nonzero_slice_sizes(slice_sizes, num_slices)
    }

    /// Parses raw serialized slice counts before rebuilding validated foliation state.
    ///
    /// Public constructors require [`NonZeroU32`], but checkpoint payloads still
    /// carry raw integers. This helper keeps serde restore on the same
    /// validation path as normal construction without exposing a raw-count API.
    fn from_raw_slice_sizes(
        slice_sizes: Vec<usize>,
        num_slices: u32,
    ) -> Result<Self, FoliationError> {
        let Some(num_slices) = NonZeroU32::new(num_slices) else {
            return Err(FoliationError::EmptyFoliation);
        };
        Self::from_nonzero_slice_sizes(slice_sizes, num_slices)
    }

    /// Builds foliation bookkeeping after the slice-count nonzero proof exists.
    ///
    /// The remaining checks protect the public [`Self::from_slice_sizes`]
    /// contract: the number of per-slice counts must match `num_slices`, and no
    /// slice may be empty.
    fn from_nonzero_slice_sizes(
        slice_sizes: Vec<usize>,
        num_slices: NonZeroU32,
    ) -> Result<Self, FoliationError> {
        if slice_sizes.is_empty() {
            return Err(FoliationError::EmptyFoliation);
        }
        if slice_sizes.len() != num_slices.get() as usize {
            return Err(FoliationError::SliceSizeMismatch {
                slice_sizes_len: slice_sizes.len(),
                num_slices: num_slices.get(),
            });
        }
        if let Some(slice) = slice_sizes.iter().position(|&slice_size| slice_size == 0) {
            return Err(FoliationError::EmptySlice { slice });
        }
        Ok(Self {
            slice_sizes,
            num_slices,
        })
    }

    /// Returns the number of vertices in each time slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtResult, Foliation};
    /// use std::num::NonZeroU32;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let Some(num_slices) = NonZeroU32::new(2) else {
    ///         return Ok(());
    ///     };
    ///     let foliation = Foliation::from_slice_sizes(
    ///         vec![3, 4],
    ///         num_slices,
    ///     )?;
    ///     assert_eq!(foliation.slice_sizes(), &[3, 4]);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn slice_sizes(&self) -> &[usize] {
        &self.slice_sizes
    }

    /// Returns the total number of time slices.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtResult, Foliation};
    /// use std::num::NonZeroU32;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let Some(num_slices) = NonZeroU32::new(2) else {
    ///         return Ok(());
    ///     };
    ///     let foliation = Foliation::from_slice_sizes(
    ///         vec![3, 4],
    ///         num_slices,
    ///     )?;
    ///     assert_eq!(foliation.num_slices().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn num_slices(&self) -> NonZeroU32 {
        self.num_slices
    }

    /// Returns the total number of labeled vertices (sum of all slice sizes).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtResult, Foliation};
    /// use std::num::NonZeroU32;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let Some(num_slices) = NonZeroU32::new(2) else {
    ///         return Ok(());
    ///     };
    ///     let foliation = Foliation::from_slice_sizes(
    ///         vec![3, 4],
    ///         num_slices,
    ///     )?;
    ///     assert_eq!(foliation.labeled_vertex_count(), 7);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn labeled_vertex_count(&self) -> usize {
        self.slice_sizes.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::from_str;

    fn slice_count(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test slice count should be nonzero")
    }

    #[test]
    fn test_edge_type_equality() {
        assert_eq!(EdgeType::Spacelike, EdgeType::Spacelike);
        assert_eq!(EdgeType::Timelike, EdgeType::Timelike);
        assert_ne!(EdgeType::Spacelike, EdgeType::Timelike);
    }

    #[test]
    fn test_foliation_empty_slice_rejected() {
        let result = Foliation::from_slice_sizes(vec![0, 0, 0], slice_count(3));
        let err = result.expect_err("empty slices should be rejected");
        assert_eq!(err, FoliationError::EmptySlice { slice: 0 });
    }

    #[test]
    fn test_foliation_zero_slices_rejected() {
        let result = Foliation::from_raw_slice_sizes(vec![], 0);
        let err = result.expect_err("zero-slice foliations should be rejected");
        assert_eq!(err, FoliationError::EmptyFoliation);
    }

    #[test]
    fn test_foliation_populated() {
        let fol = Foliation::from_slice_sizes(vec![3, 3], slice_count(2)).expect("valid foliation");
        assert_eq!(fol.num_slices().get(), 2);
        assert_eq!(fol.labeled_vertex_count(), 6);
        assert_eq!(fol.slice_sizes()[0], 3);
        assert_eq!(fol.slice_sizes()[1], 3);
    }

    #[test]
    fn test_foliation_slice_size_mismatch() {
        let result = Foliation::from_slice_sizes(vec![3, 3], slice_count(3));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err,
            FoliationError::SliceSizeMismatch {
                slice_sizes_len: 2,
                num_slices: 3,
            }
        );
    }

    #[test]
    fn foliation_deserialization_rejects_invalid_bookkeeping() {
        let zero_slice_err = from_str::<Foliation>(r#"{"slice_sizes":[],"num_slices":0}"#)
            .expect_err("zero slices should fail deserialization");
        assert!(
            zero_slice_err
                .to_string()
                .contains("at least one time slice")
        );

        let empty_err = from_str::<Foliation>(r#"{"slice_sizes":[0],"num_slices":1}"#)
            .expect_err("empty slices should fail deserialization");
        assert!(empty_err.to_string().contains("empty"));

        let mismatch_err = from_str::<Foliation>(r#"{"slice_sizes":[3,3],"num_slices":3}"#)
            .expect_err("mismatched slice counts should fail deserialization");
        assert!(mismatch_err.to_string().contains("slice_sizes length"));
    }

    #[test]
    fn test_classify_edge_spacelike() {
        assert_eq!(classify_edge(Some(0), Some(0)), Some(EdgeType::Spacelike));
    }

    #[test]
    fn test_classify_edge_timelike() {
        assert_eq!(classify_edge(Some(0), Some(1)), Some(EdgeType::Timelike));
    }

    #[test]
    fn test_classify_edge_unlabeled_returns_none() {
        assert_eq!(classify_edge(Some(0), None), None);
        assert_eq!(classify_edge(None, Some(1)), None);
        assert_eq!(classify_edge(None, None), None);
    }

    #[test]
    fn test_classify_edge_acausal() {
        // |Δt| > 1: returns Acausal
        assert_eq!(
            classify_edge(Some(0), Some(5)),
            Some(EdgeType::Acausal),
            "Edges with |Δt| > 1 should be Acausal"
        );
        assert_eq!(
            classify_edge(Some(0), Some(2)),
            Some(EdgeType::Acausal),
            "Edges with |Δt| = 2 should be Acausal"
        );
    }

    // =========================================================================
    // SimplexType tests
    // =========================================================================

    #[test]
    fn test_simplex_type_encoding_roundtrip() {
        assert_eq!(
            SimplexType::from_i32(SimplexType::Up.to_i32()),
            Some(SimplexType::Up)
        );
        assert_eq!(
            SimplexType::from_i32(SimplexType::Down.to_i32()),
            Some(SimplexType::Down)
        );
    }

    #[test]
    fn test_simplex_type_from_invalid_i32() {
        assert_eq!(SimplexType::from_i32(0), None);
        assert_eq!(SimplexType::from_i32(2), None);
        assert_eq!(SimplexType::from_i32(-2), None);
    }

    #[test]
    fn test_classify_simplex_up() {
        // Two at t=0, one at t=1 → Up (2,1)
        assert_eq!(
            classify_simplex(Some(0), Some(0), Some(1)),
            Some(SimplexType::Up)
        );
        assert_eq!(
            classify_simplex(Some(0), Some(1), Some(0)),
            Some(SimplexType::Up)
        );
        assert_eq!(
            classify_simplex(Some(1), Some(0), Some(0)),
            Some(SimplexType::Up)
        );
    }

    #[test]
    fn test_classify_simplex_down() {
        // One at t=0, two at t=1 → Down (1,2)
        assert_eq!(
            classify_simplex(Some(1), Some(1), Some(0)),
            Some(SimplexType::Down)
        );
        assert_eq!(
            classify_simplex(Some(1), Some(0), Some(1)),
            Some(SimplexType::Down)
        );
        assert_eq!(
            classify_simplex(Some(0), Some(1), Some(1)),
            Some(SimplexType::Down)
        );
    }

    #[test]
    fn test_classify_simplex_same_slice_returns_none() {
        // All vertices at same time → None (degenerate)
        assert_eq!(classify_simplex(Some(2), Some(2), Some(2)), None);
    }

    #[test]
    fn test_classify_simplex_spans_two_slices_returns_none() {
        // Spans more than one slice → None (acausal)
        assert_eq!(classify_simplex(Some(0), Some(1), Some(2)), None);
    }

    #[test]
    fn test_classify_simplex_unlabeled_returns_none() {
        assert_eq!(classify_simplex(Some(0), Some(0), None), None);
        assert_eq!(classify_simplex(None, Some(0), Some(1)), None);
    }

    // =========================================================================
    // FoliationError variant tests
    // =========================================================================

    #[test]
    fn test_foliation_error_label_count_mismatch_display() {
        let err = FoliationError::LabelCountMismatch {
            labeled: 5,
            expected: 10,
        };
        let msg = err.to_string();
        assert!(
            msg.contains('5') && msg.contains("10"),
            "Display should include both counts: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_empty_slice_display() {
        let err = FoliationError::EmptySlice { slice: 2 };
        let msg = err.to_string();
        assert!(
            msg.contains('2') && msg.contains("empty"),
            "Display should mention slice index: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_empty_foliation_display() {
        let err = FoliationError::EmptyFoliation;
        assert_eq!(
            err.to_string(),
            "foliation must contain at least one time slice"
        );
    }

    #[test]
    fn test_foliation_error_missing_vertex_label_display() {
        let err = FoliationError::MissingVertexLabel { vertex: 4 };
        let msg = err.to_string();
        assert!(
            msg.contains('4') && msg.contains("missing"),
            "Display should include vertex index and missing-label context: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_label_mismatch_display() {
        let err = FoliationError::LabelMismatch {
            slice: 1,
            expected: 3,
            actual: 2,
        };
        let msg = err.to_string();
        assert!(
            msg.contains('1') && msg.contains('3') && msg.contains('2'),
            "Display should include slice and both counts: {msg}"
        );
    }

    #[test]
    fn test_out_of_range_label_display() {
        let err = FoliationError::OutOfRangeVertexLabel {
            vertex: 2,
            label: 9,
            expected_range_end: 3,
        };
        let msg = err.to_string();
        assert!(
            msg.contains('2')
                && msg.contains('9')
                && msg.contains("out-of-range")
                && msg.contains("0..3"),
            "Display should include vertex index, label, and expected range: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_slice_size_sum_mismatch_display() {
        let err = FoliationError::SliceSizeSumMismatch {
            sum: 7,
            labeled: 10,
        };
        let msg = err.to_string();
        assert!(
            msg.contains('7') && msg.contains("10"),
            "Display should include both values: {msg}"
        );
    }

    #[test]
    fn stale_bookkeeping_display() {
        let err = FoliationError::StaleBookkeeping {
            synced_at_modification: Some(123),
            current_modification_count: 456,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("stale")
                && msg.contains("synchronized at modification")
                && msg.contains("Some(123)")
                && msg.contains("current modification 456"),
            "Display should describe stale bookkeeping and both modification counts: {msg}"
        );
    }

    #[test]
    fn stale_bookkeeping_equality() {
        let err = FoliationError::StaleBookkeeping {
            synced_at_modification: Some(123),
            current_modification_count: 456,
        };
        assert_eq!(
            err,
            FoliationError::StaleBookkeeping {
                synced_at_modification: Some(123),
                current_modification_count: 456,
            }
        );
        assert_ne!(
            err,
            FoliationError::StaleBookkeeping {
                synced_at_modification: None,
                current_modification_count: 456,
            }
        );
        assert_ne!(
            err,
            FoliationError::StaleBookkeeping {
                synced_at_modification: Some(123),
                current_modification_count: 789,
            }
        );
    }

    #[test]
    fn test_foliation_error_equality() {
        assert_eq!(
            FoliationError::EmptySlice { slice: 0 },
            FoliationError::EmptySlice { slice: 0 },
        );
        assert_ne!(
            FoliationError::EmptySlice { slice: 0 },
            FoliationError::EmptySlice { slice: 1 },
        );
    }

    #[test]
    fn test_foliation_error_spacelike_subgraph_size_mismatch_display() {
        let err = FoliationError::SpacelikeSubgraphSizeMismatch {
            slice: 2,
            observed: 4,
            expected: 5,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("slice 2") && msg.contains("contains 4") && msg.contains("reports 5"),
            "Display should describe the spatial slice and both vertex counts: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_spacelike_degree_violation_display() {
        let err = FoliationError::SpacelikeDegreeViolation {
            slice: 1,
            vertex: "VertexKey(7v3)".to_string(),
            observed_degree: 3,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("slice 1")
                && msg.contains("VertexKey(7v3)")
                && msg.contains("3 spacelike neighbours")
                && msg.contains("closed S¹"),
            "Display should identify the slice, vertex, observed degree, and S¹ expectation: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_spacelike_non_closed_ring_display() {
        let err = FoliationError::SpacelikeNonClosedRing {
            slice: 0,
            walked: 3,
            expected: 6,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("slice 0")
                && msg.contains("length 3")
                && msg.contains("6 vertices")
                && msg.contains("closed S¹"),
            "Display should describe slice index, walked length, expected ring length, and S¹ expectation: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_missing_temporal_wraparound_display() {
        let err = FoliationError::MissingTemporalWrapAround {
            slice: 0,
            missing_neighbor: 2,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("slice 0")
                && msg.contains("slice 2")
                && msg.contains("temporal wrap-around"),
            "Display should describe missing wrap-around between specific slices: {msg}"
        );
    }

    #[test]
    fn test_foliation_error_open_boundary_variants_display() {
        let cases = [
            (
                FoliationError::SpacelikeOpenSliceEndpointCount {
                    slice: 2,
                    observed: 3,
                    expected: 2,
                },
                ["open-boundary", "slice 2", "3 interval endpoints"],
            ),
            (
                FoliationError::SpacelikeOpenSliceDegreeViolation {
                    slice: 1,
                    vertex: "VertexKey(7v3)".to_string(),
                    observed_degree: 0,
                },
                ["open-boundary", "VertexKey(7v3)", "0 spacelike neighbours"],
            ),
            (
                FoliationError::SpacelikeNonOpenInterval {
                    slice: 4,
                    walked: 3,
                    expected: 5,
                },
                ["open-boundary", "slice 4", "single interval"],
            ),
            (
                FoliationError::OpenBoundarySlabEdgeCrossing {
                    lower_slice: 0,
                    first_edge: "a->d".to_string(),
                    second_edge: "b->c".to_string(),
                },
                ["open-boundary slab 0->1", "a->d", "b->c"],
            ),
            (
                FoliationError::OpenBoundarySpatialOrderMismatch {
                    slice: 3,
                    path_vertices: 4,
                    coordinate_vertices: 5,
                },
                ["open-boundary", "slice 3", "x-coordinate order"],
            ),
            (
                FoliationError::OpenBoundaryTimeCoordinateMismatch {
                    label: 6,
                    vertex: "VertexKey(9v1)".to_string(),
                    y: "6.25".to_string(),
                },
                ["open-boundary", "VertexKey(9v1)", "y-coordinate 6.25"],
            ),
        ];

        for (err, expected_fragments) in cases {
            let msg = err.to_string();
            for fragment in expected_fragments {
                assert!(
                    msg.contains(fragment),
                    "Display should contain {fragment:?}: {msg}"
                );
            }
        }
    }

    #[test]
    fn test_foliation_error_spacelike_variant_equality() {
        let a = FoliationError::SpacelikeSubgraphSizeMismatch {
            slice: 2,
            observed: 4,
            expected: 5,
        };
        let b = a.clone();
        let c = FoliationError::SpacelikeSubgraphSizeMismatch {
            slice: 2,
            observed: 5,
            expected: 5,
        };
        assert_eq!(a, b, "identical Spacelike* variants should compare equal");
        assert_ne!(
            a, c,
            "Spacelike* variants differing in any field should compare unequal"
        );
    }
}
