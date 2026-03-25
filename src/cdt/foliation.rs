//! Foliation data structures for Causal Dynamical Triangulations.
//!
//! A **foliation** assigns each vertex to a discrete time slice, enabling
//! classification of edges as spacelike (within a slice) or timelike (between
//! adjacent slices). This is the core causal structure of CDT.
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
        // Edges spanning more than one time slice violate causality;
        // still classifiable but validation will catch this.
        Some(EdgeType::Timelike)
    }
}

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
    #[must_use]
    pub fn from_slice_sizes(slice_sizes: Vec<usize>, num_slices: u32) -> Self {
        debug_assert_eq!(
            slice_sizes.len(),
            num_slices as usize,
            "slice_sizes length {} != num_slices {}",
            slice_sizes.len(),
            num_slices
        );
        Self {
            slice_sizes,
            num_slices,
        }
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
        let fol = Foliation::from_slice_sizes(vec![0, 0, 0], 3);
        assert_eq!(fol.num_slices(), 3);
        assert_eq!(fol.labeled_vertex_count(), 0);
        assert_eq!(fol.slice_sizes(), &[0, 0, 0]);
    }

    #[test]
    fn test_foliation_populated() {
        let fol = Foliation::from_slice_sizes(vec![3, 3], 2);
        assert_eq!(fol.num_slices(), 2);
        assert_eq!(fol.labeled_vertex_count(), 6);
        assert_eq!(fol.slice_sizes()[0], 3);
        assert_eq!(fol.slice_sizes()[1], 3);
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
    fn test_classify_edge_acausal_returns_timelike() {
        // |Δt| > 1: returns Timelike (validation catches the violation)
        assert_eq!(
            classify_edge(Some(0), Some(5)),
            Some(EdgeType::Timelike),
            "Acausal edges (|Δt| > 1) should still return Timelike"
        );
    }
}
