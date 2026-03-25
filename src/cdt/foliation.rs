//! Foliation data structures for Causal Dynamical Triangulations.
//!
//! A **foliation** assigns each vertex to a discrete time slice, enabling
//! classification of edges as spacelike (within a slice) or timelike (between
//! adjacent slices). This is the core causal structure of CDT.

use delaunay::core::triangulation_data_structure::VertexKey;
use delaunay::prelude::collections::VertexSecondaryMap;

/// Classification of an edge in a foliated triangulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeType {
    /// Both endpoints share the same time slice.
    Spacelike,
    /// Endpoints are in adjacent time slices (|Δt| = 1).
    Timelike,
}

/// Per-vertex time labels and slice bookkeeping for a CDT triangulation.
///
/// Stores the foliation in the CDT layer (not the geometry backend) to
/// maintain the CDT ↔ geometry separation. Uses [`VertexSecondaryMap`] for
/// O(1) lookup that shares the slotmap key space with the Delaunay backend.
#[derive(Debug)]
pub struct Foliation {
    /// Map from vertex key to time slice index.
    time_labels: VertexSecondaryMap<u32>,
    /// Number of vertices per time slice (`slice_sizes[t]`).
    slice_sizes: Vec<usize>,
    /// Total number of time slices.
    num_slices: u32,
}

impl Foliation {
    /// Creates a new foliation from pre-computed time labels.
    ///
    /// Callers must ensure that every vertex in the triangulation has an entry
    /// in `time_labels` and that all label values are in `0..num_slices`.
    #[must_use]
    pub fn new(time_labels: VertexSecondaryMap<u32>, num_slices: u32) -> Self {
        let mut slice_sizes = vec![0usize; num_slices as usize];
        for (_, &t) in &time_labels {
            debug_assert!(
                (t as usize) < slice_sizes.len(),
                "time label {t} out of range for {num_slices} slices"
            );
            let idx = t as usize;
            if idx < slice_sizes.len() {
                slice_sizes[idx] += 1;
            }
        }
        Self {
            time_labels,
            slice_sizes,
            num_slices,
        }
    }

    /// Returns the time slice label for a vertex, or `None` if unlabeled.
    #[must_use]
    pub fn time_label(&self, key: VertexKey) -> Option<u32> {
        self.time_labels.get(key).copied()
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

    /// Returns the total number of labeled vertices.
    #[must_use]
    pub fn labeled_vertex_count(&self) -> usize {
        self.time_labels.len()
    }

    /// Classifies an edge given the vertex keys of its two endpoints.
    ///
    /// Returns `None` if either endpoint lacks a time label.
    #[must_use]
    pub fn classify_edge(&self, v0: VertexKey, v1: VertexKey) -> Option<EdgeType> {
        let t0 = self.time_labels.get(v0).copied()?;
        let t1 = self.time_labels.get(v1).copied()?;
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

    /// Inserts or updates a time label for a vertex.
    pub fn set_time_label(&mut self, key: VertexKey, time: u32) {
        // Decrement old slice count if previously labeled
        if let Some(&old_t) = self.time_labels.get(key) {
            let idx = old_t as usize;
            if idx < self.slice_sizes.len() {
                self.slice_sizes[idx] = self.slice_sizes[idx].saturating_sub(1);
            }
        }
        self.time_labels.insert(key, time);
        let idx = time as usize;
        if idx < self.slice_sizes.len() {
            self.slice_sizes[idx] += 1;
        }
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
        let labels = VertexSecondaryMap::new();
        let fol = Foliation::new(labels, 3);
        assert_eq!(fol.num_slices(), 3);
        assert_eq!(fol.labeled_vertex_count(), 0);
        assert_eq!(fol.slice_sizes(), &[0, 0, 0]);
    }

    /// Build a small triangulation and test Foliation methods directly.
    #[test]
    fn test_foliation_populated() {
        let dt = crate::util::generate_seeded_delaunay2(6, (0.0, 10.0), 42);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();
        assert!(vkeys.len() >= 3, "Need at least 3 vertices");

        let mut labels = VertexSecondaryMap::new();
        // Assign first half to slice 0, rest to slice 1
        let mid = vkeys.len() / 2;
        for &k in &vkeys[..mid] {
            labels.insert(k, 0);
        }
        for &k in &vkeys[mid..] {
            labels.insert(k, 1);
        }

        let fol = Foliation::new(labels, 2);
        assert_eq!(fol.num_slices(), 2);
        assert_eq!(fol.labeled_vertex_count(), vkeys.len());
        assert_eq!(fol.slice_sizes()[0], mid);
        assert_eq!(fol.slice_sizes()[1], vkeys.len() - mid);

        // time_label lookups
        assert_eq!(fol.time_label(vkeys[0]), Some(0));
        assert_eq!(fol.time_label(vkeys[mid]), Some(1));
    }

    #[test]
    fn test_classify_edge_spacelike() {
        let dt = crate::util::generate_seeded_delaunay2(4, (0.0, 10.0), 1);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();

        let mut labels = VertexSecondaryMap::new();
        for &k in &vkeys {
            labels.insert(k, 0); // all same slice
        }
        let fol = Foliation::new(labels, 1);

        assert_eq!(
            fol.classify_edge(vkeys[0], vkeys[1]),
            Some(EdgeType::Spacelike)
        );
    }

    #[test]
    fn test_classify_edge_timelike() {
        let dt = crate::util::generate_seeded_delaunay2(4, (0.0, 10.0), 1);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();

        let mut labels = VertexSecondaryMap::new();
        labels.insert(vkeys[0], 0);
        labels.insert(vkeys[1], 1);
        let fol = Foliation::new(labels, 2);

        assert_eq!(
            fol.classify_edge(vkeys[0], vkeys[1]),
            Some(EdgeType::Timelike)
        );
    }

    #[test]
    fn test_classify_edge_unlabeled_returns_none() {
        let dt = crate::util::generate_seeded_delaunay2(4, (0.0, 10.0), 1);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();

        let mut labels = VertexSecondaryMap::new();
        labels.insert(vkeys[0], 0);
        // vkeys[1] intentionally unlabeled
        let fol = Foliation::new(labels, 1);

        assert_eq!(fol.classify_edge(vkeys[0], vkeys[1]), None);
    }

    #[test]
    fn test_classify_edge_acausal_returns_timelike() {
        // |Δt| > 1: current behavior returns Timelike (validation catches the violation)
        let dt = crate::util::generate_seeded_delaunay2(4, (0.0, 10.0), 1);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();

        let mut labels = VertexSecondaryMap::new();
        labels.insert(vkeys[0], 0);
        labels.insert(vkeys[1], 5); // |Δt| = 5
        let fol = Foliation::new(labels, 6);

        assert_eq!(
            fol.classify_edge(vkeys[0], vkeys[1]),
            Some(EdgeType::Timelike),
            "Acausal edges (|Δt| > 1) should still return Timelike"
        );
    }

    #[test]
    fn test_set_time_label_new_vertex() {
        let dt = crate::util::generate_seeded_delaunay2(4, (0.0, 10.0), 1);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();

        let labels = VertexSecondaryMap::new();
        let mut fol = Foliation::new(labels, 2);

        assert_eq!(fol.labeled_vertex_count(), 0);
        assert_eq!(fol.slice_sizes(), &[0, 0]);

        // First-time labeling of a vertex
        fol.set_time_label(vkeys[0], 1);
        assert_eq!(fol.labeled_vertex_count(), 1);
        assert_eq!(fol.slice_sizes(), &[0, 1]);
        assert_eq!(fol.time_label(vkeys[0]), Some(1));
    }

    #[test]
    fn test_set_time_label_updates_slice_sizes() {
        let dt = crate::util::generate_seeded_delaunay2(4, (0.0, 10.0), 1);
        let vkeys: Vec<_> = dt.vertices().map(|(k, _)| k).collect();

        let mut labels = VertexSecondaryMap::new();
        labels.insert(vkeys[0], 0);
        labels.insert(vkeys[1], 0);
        let mut fol = Foliation::new(labels, 2);

        assert_eq!(fol.slice_sizes(), &[2, 0]);

        // Move vkeys[1] from slice 0 → slice 1
        fol.set_time_label(vkeys[1], 1);
        assert_eq!(fol.slice_sizes(), &[1, 1]);
        assert_eq!(fol.time_label(vkeys[1]), Some(1));
    }
}
