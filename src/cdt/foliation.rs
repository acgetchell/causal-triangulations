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

/// Classification of an edge in a foliated triangulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeType {
    /// Both endpoints share the same time slice.
    Spacelike,
    /// Endpoints are in adjacent time slices (|Δt| = 1).
    Timelike,
    /// Endpoints span more than one time slice (|Δt| > 1), violating causality.
    Acausal,
}

/// Classification of a triangle (cell) in a foliated 1+1 CDT.
///
/// In a valid foliated triangulation every triangle spans exactly two
/// adjacent time slices.  The type is determined by how many vertices
/// sit on the lower vs. upper slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellType {
    /// **(2,1)** — two vertices at time *t*, one at *t + 1*.
    /// The spacelike base is in the lower slice.
    Up,
    /// **(1,2)** — one vertex at time *t*, two at *t + 1*.
    /// The spacelike base is in the upper slice.
    Down,
}

impl CellType {
    /// Encode as the `i32` value stored in cell data.
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::Up => 1,
            Self::Down => -1,
        }
    }

    /// Decode from the `i32` value stored in cell data.
    ///
    /// Returns `None` for values that do not represent a valid cell type.
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
#[must_use]
pub fn classify_cell(t0: Option<u32>, t1: Option<u32>, t2: Option<u32>) -> Option<CellType> {
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
        2 => Some(CellType::Up),   // (2,1): two at t, one at t+1
        1 => Some(CellType::Down), // (1,2): one at t, two at t+1
        _ => None,                 // unreachable for 3 vertices spanning 2 values
    }
}

/// Error type for foliation construction and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoliationError {
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
}

impl std::fmt::Display for FoliationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
            Self::EmptySlice { slice } => write!(f, "time slice {slice} is empty"),
            Self::SliceSizeSumMismatch { sum, labeled } => write!(
                f,
                "slice_sizes sum ({sum}) does not match labeled vertex count ({labeled})"
            ),
        }
    }
}

impl std::error::Error for FoliationError {}

/// Per-slice bookkeeping for a CDT triangulation.
///
/// Time labels are stored on vertices directly (as vertex data in the
/// Delaunay triangulation). This struct tracks only the per-slice vertex
/// counts and the total number of slices.
#[derive(Debug)]
pub struct Foliation {
    /// Number of vertices per time slice (`slice_sizes[t]`).
    slice_sizes: Vec<usize>,
    /// Total number of time slices.
    num_slices: u32,
}

impl Foliation {
    /// Creates a new foliation from pre-computed per-slice vertex counts.
    ///
    /// # Errors
    ///
    /// Returns error if `slice_sizes.len() != num_slices`.
    pub fn from_slice_sizes(
        slice_sizes: Vec<usize>,
        num_slices: u32,
    ) -> Result<Self, FoliationError> {
        if slice_sizes.len() != num_slices as usize {
            return Err(FoliationError::SliceSizeMismatch {
                slice_sizes_len: slice_sizes.len(),
                num_slices,
            });
        }
        Ok(Self {
            slice_sizes,
            num_slices,
        })
    }

    /// Returns the number of vertices in each time slice.
    #[must_use]
    pub fn slice_sizes(&self) -> &[usize] {
        &self.slice_sizes
    }

    /// Returns the total number of time slices.
    #[must_use]
    pub const fn num_slices(&self) -> u32 {
        self.num_slices
    }

    /// Returns the total number of labeled vertices (sum of all slice sizes).
    #[must_use]
    pub fn labeled_vertex_count(&self) -> usize {
        self.slice_sizes.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_type_equality() {
        assert_eq!(EdgeType::Spacelike, EdgeType::Spacelike);
        assert_eq!(EdgeType::Timelike, EdgeType::Timelike);
        assert_ne!(EdgeType::Spacelike, EdgeType::Timelike);
    }

    #[test]
    fn test_foliation_empty() {
        let fol = Foliation::from_slice_sizes(vec![0, 0, 0], 3).expect("valid foliation");
        assert_eq!(fol.num_slices(), 3);
        assert_eq!(fol.labeled_vertex_count(), 0);
        assert_eq!(fol.slice_sizes(), &[0, 0, 0]);
    }

    #[test]
    fn test_foliation_populated() {
        let fol = Foliation::from_slice_sizes(vec![3, 3], 2).expect("valid foliation");
        assert_eq!(fol.num_slices(), 2);
        assert_eq!(fol.labeled_vertex_count(), 6);
        assert_eq!(fol.slice_sizes()[0], 3);
        assert_eq!(fol.slice_sizes()[1], 3);
    }

    #[test]
    fn test_foliation_slice_size_mismatch() {
        let result = Foliation::from_slice_sizes(vec![3, 3], 3);
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
    // CellType tests
    // =========================================================================

    #[test]
    fn test_cell_type_encoding_roundtrip() {
        assert_eq!(
            CellType::from_i32(CellType::Up.to_i32()),
            Some(CellType::Up)
        );
        assert_eq!(
            CellType::from_i32(CellType::Down.to_i32()),
            Some(CellType::Down)
        );
    }

    #[test]
    fn test_cell_type_from_invalid_i32() {
        assert_eq!(CellType::from_i32(0), None);
        assert_eq!(CellType::from_i32(2), None);
        assert_eq!(CellType::from_i32(-2), None);
    }

    #[test]
    fn test_classify_cell_up() {
        // Two at t=0, one at t=1 → Up (2,1)
        assert_eq!(classify_cell(Some(0), Some(0), Some(1)), Some(CellType::Up));
        assert_eq!(classify_cell(Some(0), Some(1), Some(0)), Some(CellType::Up));
        assert_eq!(classify_cell(Some(1), Some(0), Some(0)), Some(CellType::Up));
    }

    #[test]
    fn test_classify_cell_down() {
        // One at t=0, two at t=1 → Down (1,2)
        assert_eq!(
            classify_cell(Some(1), Some(1), Some(0)),
            Some(CellType::Down)
        );
        assert_eq!(
            classify_cell(Some(1), Some(0), Some(1)),
            Some(CellType::Down)
        );
        assert_eq!(
            classify_cell(Some(0), Some(1), Some(1)),
            Some(CellType::Down)
        );
    }

    #[test]
    fn test_classify_cell_same_slice_returns_none() {
        // All vertices at same time → None (degenerate)
        assert_eq!(classify_cell(Some(2), Some(2), Some(2)), None);
    }

    #[test]
    fn test_classify_cell_spans_two_slices_returns_none() {
        // Spans more than one slice → None (acausal)
        assert_eq!(classify_cell(Some(0), Some(1), Some(2)), None);
    }

    #[test]
    fn test_classify_cell_unlabeled_returns_none() {
        assert_eq!(classify_cell(Some(0), Some(0), None), None);
        assert_eq!(classify_cell(None, Some(0), Some(1)), None);
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
}
