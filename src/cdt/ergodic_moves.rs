#![forbid(unsafe_code)]

//! Ergodic moves for 2D Causal Dynamical Triangulations.
//!
//! This module implements the standard local moves used in 2D CDT:
//! - (2,2) moves: flip the shared edge between two triangles
//! - (1,3) moves: split a triangle by inserting a vertex
//! - (3,1) moves: collapse a degree-3 vertex back to one triangle
//! - edge flips: retained as an API-compatible alias for the 2D (2,2) move

use crate::config::CdtTopology;
use crate::errors::{BackendMutationOperation, CdtError};
use crate::geometry::CdtTriangulation2D;
use crate::geometry::backends::delaunay::{DelaunayFaceHandle, DelaunayVertexHandle};
use crate::geometry::traits::{EdgeAdjacentFaces, TriangulationQuery};
use rand::{RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Serialize};

/// Types of ergodic moves available in 2D CDT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MoveType {
    /// (2,2) move: Flip edge between two triangles
    Move22,
    /// (1,3) move: Add vertex by subdividing triangle
    Move13Add,
    /// (3,1) move: Remove vertex by merging triangles
    Move31Remove,
    /// Edge flip: API-compatible alias for the 2D (2,2) move
    EdgeFlip,
}

/// Result of attempting an ergodic move.
#[derive(Debug, Clone, PartialEq)]
pub enum MoveResult {
    /// Move was successfully applied
    Success,
    /// Move was rejected due to causality constraints
    CausalityViolation,
    /// Move was rejected due to geometric constraints
    GeometricViolation,
    /// Move was rejected for other reasons
    Rejected(CdtError),
    /// Move mutated geometry but failed a required post-mutation invariant refresh
    HardFailure(CdtError),
}

/// Statistics tracking for ergodic moves.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MoveStatistics {
    /// Number of (2,2) moves attempted
    pub moves_22_attempted: u64,
    /// Number of (2,2) moves accepted
    pub moves_22_accepted: u64,
    /// Number of (1,3) moves attempted
    pub moves_13_attempted: u64,
    /// Number of (1,3) moves accepted
    pub moves_13_accepted: u64,
    /// Number of (3,1) moves attempted
    pub moves_31_attempted: u64,
    /// Number of (3,1) moves accepted
    pub moves_31_accepted: u64,
    /// Number of edge flips attempted
    pub edge_flips_attempted: u64,
    /// Number of edge flips accepted
    pub edge_flips_accepted: u64,
}

impl MoveStatistics {
    /// Creates a new statistics tracker.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    ///
    /// let stats = MoveStatistics::new();
    /// assert_eq!(stats.moves_22_attempted, 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an attempted move.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// assert_eq!(stats.moves_22_attempted, 1);
    /// ```
    pub const fn record_attempt(&mut self, move_type: MoveType) {
        match move_type {
            MoveType::Move22 => self.moves_22_attempted += 1,
            MoveType::Move13Add => self.moves_13_attempted += 1,
            MoveType::Move31Remove => self.moves_31_attempted += 1,
            MoveType::EdgeFlip => self.edge_flips_attempted += 1,
        }
    }

    /// Records a successful move.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_success(MoveType::EdgeFlip);
    /// assert_eq!(stats.edge_flips_accepted, 1);
    /// ```
    pub const fn record_success(&mut self, move_type: MoveType) {
        match move_type {
            MoveType::Move22 => self.moves_22_accepted += 1,
            MoveType::Move13Add => self.moves_13_accepted += 1,
            MoveType::Move31Remove => self.moves_31_accepted += 1,
            MoveType::EdgeFlip => self.edge_flips_accepted += 1,
        }
    }

    /// Calculates acceptance rate for a specific move type.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// stats.record_success(MoveType::Move22);
    /// assert_relative_eq!(stats.acceptance_rate(MoveType::Move22), 1.0);
    /// ```
    #[must_use]
    pub fn acceptance_rate(&self, move_type: MoveType) -> f64 {
        let (attempted, accepted) = match move_type {
            MoveType::Move22 => (self.moves_22_attempted, self.moves_22_accepted),
            MoveType::Move13Add => (self.moves_13_attempted, self.moves_13_accepted),
            MoveType::Move31Remove => (self.moves_31_attempted, self.moves_31_accepted),
            MoveType::EdgeFlip => (self.edge_flips_attempted, self.edge_flips_accepted),
        };

        if attempted == 0 {
            0.0
        } else {
            let accepted = count_to_f64(accepted);
            let attempted = count_to_f64(attempted);
            accepted / attempted
        }
    }

    /// Calculates overall acceptance rate.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// stats.record_success(MoveType::Move22);
    /// assert_relative_eq!(stats.total_acceptance_rate(), 1.0);
    /// ```
    #[must_use]
    pub fn total_acceptance_rate(&self) -> f64 {
        let total_attempted = self.moves_22_attempted
            + self.moves_13_attempted
            + self.moves_31_attempted
            + self.edge_flips_attempted;
        let total_accepted = self.moves_22_accepted
            + self.moves_13_accepted
            + self.moves_31_accepted
            + self.edge_flips_accepted;

        if total_attempted == 0 {
            0.0
        } else {
            let total_accepted = count_to_f64(total_accepted);
            let total_attempted = count_to_f64(total_attempted);
            total_accepted / total_attempted
        }
    }

    /// Returns the total number of attempted moves across all move types.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// stats.record_attempt(MoveType::Move13Add);
    /// assert_eq!(stats.total_attempted(), 2);
    /// ```
    #[must_use]
    pub const fn total_attempted(&self) -> u64 {
        self.moves_22_attempted
            + self.moves_13_attempted
            + self.moves_31_attempted
            + self.edge_flips_attempted
    }

    /// Returns the total number of accepted moves across all move types.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_success(MoveType::Move22);
    /// stats.record_success(MoveType::EdgeFlip);
    /// assert_eq!(stats.total_accepted(), 2);
    /// ```
    #[must_use]
    pub const fn total_accepted(&self) -> u64 {
        self.moves_22_accepted
            + self.moves_13_accepted
            + self.moves_31_accepted
            + self.edge_flips_accepted
    }
}

/// Converts an accumulated move counter to a finite value for rate reporting.
#[expect(
    clippy::cast_precision_loss,
    reason = "move counters are converted only for aggregate acceptance-rate reporting"
)]
const fn count_to_f64(count: u64) -> f64 {
    count as f64
}

/// Ergodic move system for CDT triangulations.
#[derive(Clone, Serialize, Deserialize)]
pub struct ErgodicsSystem {
    /// Move statistics
    pub stats: MoveStatistics,
    /// Random number generator
    rng: Xoshiro256PlusPlus,
}

#[derive(Debug, Clone, Copy)]
enum InsertionLabel {
    Unfoliated,
    Label(u32),
}

impl ErgodicsSystem {
    /// Creates a new ergodics system.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::ErgodicsSystem;
    ///
    /// let system = ErgodicsSystem::new();
    /// assert_eq!(system.stats.moves_22_attempted, 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: MoveStatistics::new(),
            rng: rand::make_rng(),
        }
    }

    /// Creates a new ergodics system with a deterministic random seed.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::ErgodicsSystem;
    ///
    /// let mut a = ErgodicsSystem::with_seed(7);
    /// let mut b = ErgodicsSystem::with_seed(7);
    /// assert_eq!(a.select_random_move(), b.select_random_move());
    /// ```
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self {
            stats: MoveStatistics::new(),
            rng: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }

    /// Selects a random move type.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    ///
    /// let mut system = ErgodicsSystem::new();
    /// let move_type = system.select_random_move();
    /// assert!(matches!(
    ///     move_type,
    ///     MoveType::Move22 | MoveType::Move13Add | MoveType::Move31Remove | MoveType::EdgeFlip
    /// ));
    /// ```
    #[must_use]
    pub fn select_random_move(&mut self) -> MoveType {
        match self.rng.random_range(0..4) {
            0 => MoveType::Move22,
            1 => MoveType::Move13Add,
            2 => MoveType::Move31Remove,
            _ => MoveType::EdgeFlip,
        }
    }

    /// Attempts a (2,2) move on the triangulation.
    ///
    /// A (2,2) move flips an interior edge shared by two triangles. In a
    /// foliated triangulation the replacement triangles must still each have
    /// exactly one spacelike edge and two timelike edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_from_cells(
    ///     &[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1), ([1.0, 1.0], 1)],
    ///     &[vec![0, 1, 2], vec![1, 3, 2]],
    /// )
    /// .expect("build square CDT");
    /// let backend = DelaunayBackend2D::from_triangulation(dt)
    ///     .expect("Delaunay input should validate");
    /// let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled square");
    /// let mut system = ErgodicsSystem::new();
    /// let result = system.attempt_22_move(&mut triangulation);
    /// assert!(matches!(
    ///     result,
    ///     MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    /// ));
    /// assert_eq!(system.stats.moves_22_attempted, 1);
    /// ```
    pub fn attempt_22_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move22);
        self.attempt_causal_edge_flip(triangulation, MoveType::Move22)
    }

    /// Attempts a (1,3) move on the triangulation.
    ///
    /// A (1,3) move inserts a vertex at the selected triangle centroid. For a
    /// foliated triangle, the inserted vertex receives the unique time label
    /// that keeps all three replacement triangles causal.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt)
    ///     .expect("Delaunay input should validate");
    /// let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled triangle");
    /// let mut system = ErgodicsSystem::new();
    /// let result = system.attempt_13_move(&mut triangulation);
    /// assert!(matches!(result, MoveResult::Success | MoveResult::GeometricViolation));
    /// assert_eq!(system.stats.moves_13_attempted, 1);
    /// ```
    pub fn attempt_13_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move13Add);
        self.attempt_13_move_mutating(triangulation)
    }

    /// Rollback boundary for an accepted (1,3) move application.
    ///
    /// Clones a snapshot, subdivides the selected face, applies the inserted
    /// vertex label, and finishes CDT bookkeeping. Once the snapshot exists,
    /// every early return must pass a `MoveResult` through `rollback_if_failed`
    /// or restore state itself.
    fn attempt_13_move_mutating(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        let mut geometric_candidate_seen = false;
        let mut causal_candidate_count = 0;
        let mut selected_candidate = None;

        for face in triangulation.geometry().faces() {
            let Some(point) = centroid(triangulation, &face) else {
                continue;
            };
            geometric_candidate_seen = true;

            let Some(label) = insertion_label(triangulation, &face) else {
                continue;
            };

            causal_candidate_count += 1;
            if self.rng.random_range(0..causal_candidate_count) == 0 {
                selected_candidate = Some((face, point, label));
            }
        }

        let Some((face, point, label)) = selected_candidate else {
            if !geometric_candidate_seen {
                return MoveResult::GeometricViolation;
            }
            return if triangulation.has_foliation() {
                MoveResult::CausalityViolation
            } else {
                MoveResult::GeometricViolation
            };
        };

        let subdivision_target = format!("face {:?}", face.cell_key());
        let snapshot = triangulation.clone();
        let subdivision = triangulation.subdivide_face(face, &point);

        let subdivision = match subdivision {
            Ok(subdivision) => subdivision,
            Err(err) => {
                let result = reject_backend(
                    BackendMutationOperation::SubdivideFace,
                    subdivision_target,
                    &err,
                );
                return rollback_if_failed(triangulation, snapshot, result);
            }
        };

        if let InsertionLabel::Label(label) = label {
            let set_label = triangulation.set_vertex_data(&subdivision.new_vertex, Some(label));
            if let Err(err) = set_label {
                let result = MoveResult::HardFailure(CdtError::BackendMutationFailed {
                    operation: BackendMutationOperation::SetVertexDataByKey,
                    target: format!("vertex {:?}", subdivision.new_vertex.vertex_key()),
                    detail: err.to_string(),
                });
                return rollback_if_failed(triangulation, snapshot, result);
            }
        }

        let result = self.finish_mutated_move(triangulation, MoveType::Move13Add);
        rollback_if_failed(triangulation, snapshot, result)
    }

    /// Attempts a (3,1) move on the triangulation.
    ///
    /// A (3,1) move removes a degree-3 vertex if its neighbouring vertices can
    /// form one causal replacement triangle and the removal does not empty a
    /// time slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt)
    ///     .expect("Delaunay input should validate");
    /// let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled triangle");
    /// let mut system = ErgodicsSystem::new();
    /// let _ = system.attempt_13_move(&mut triangulation);
    /// let result = system.attempt_31_move(&mut triangulation);
    /// assert!(matches!(
    ///     result,
    ///     MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    /// ));
    /// assert_eq!(system.stats.moves_31_attempted, 1);
    /// ```
    pub fn attempt_31_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move31Remove);
        self.attempt_31_move_mutating(triangulation)
    }

    /// Rollback boundary for an accepted (3,1) move application.
    ///
    /// Clones a snapshot, removes the selected vertex, and finishes CDT
    /// bookkeeping. Once the snapshot exists, every early return must pass a
    /// `MoveResult` through `rollback_if_failed` or restore state itself.
    fn attempt_31_move_mutating(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        let mut geometric_candidate_seen = false;
        let mut causal_candidate_count = 0;
        let mut selected_vertex = None;

        for vertex in triangulation.geometry().vertices() {
            let Some(neighbors) = neighbors3(triangulation, &vertex) else {
                continue;
            };
            geometric_candidate_seen = true;

            if !removal_candidate_is_causal(triangulation, &vertex, &neighbors) {
                continue;
            }

            causal_candidate_count += 1;
            if self.rng.random_range(0..causal_candidate_count) == 0 {
                selected_vertex = Some(vertex);
            }
        }

        let Some(vertex) = selected_vertex else {
            if !geometric_candidate_seen {
                return MoveResult::GeometricViolation;
            }
            return if triangulation.has_foliation() {
                MoveResult::CausalityViolation
            } else {
                MoveResult::GeometricViolation
            };
        };

        let removal_target = format!("vertex {:?}", vertex.vertex_key());
        let snapshot = triangulation.clone();
        let removal = triangulation.remove_vertex(vertex);

        let result = match removal {
            Ok(_) => self.finish_mutated_move(triangulation, MoveType::Move31Remove),
            Err(err) => {
                reject_backend(BackendMutationOperation::RemoveVertex, removal_target, &err)
            }
        };
        rollback_if_failed(triangulation, snapshot, result)
    }

    /// Attempts an edge flip move on the triangulation.
    ///
    /// In 2D this is the same bistellar k=2 operation as [`Self::attempt_22_move`].
    /// The separate method is retained for API compatibility and records
    /// `EdgeFlip` statistics.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_from_cells(
    ///     &[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1), ([1.0, 1.0], 1)],
    ///     &[vec![0, 1, 2], vec![1, 3, 2]],
    /// )
    /// .expect("build square CDT");
    /// let backend = DelaunayBackend2D::from_triangulation(dt)
    ///     .expect("Delaunay input should validate");
    /// let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled square");
    /// let mut system = ErgodicsSystem::new();
    /// let result = system.attempt_edge_flip(&mut triangulation);
    /// assert!(matches!(
    ///     result,
    ///     MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    /// ));
    /// assert_eq!(system.stats.edge_flips_attempted, 1);
    /// ```
    pub fn attempt_edge_flip(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::EdgeFlip);
        self.attempt_causal_edge_flip(triangulation, MoveType::EdgeFlip)
    }

    /// Attempts a random ergodic move on the triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt)
    ///     .expect("Delaunay input should validate");
    /// let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled triangle");
    /// let mut system = ErgodicsSystem::new();
    /// let result = system.attempt_random_move(&mut triangulation);
    /// assert!(matches!(
    ///     result,
    ///     MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    /// ));
    /// ```
    pub fn attempt_random_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        let move_type = self.select_random_move();
        match move_type {
            MoveType::Move22 => self.attempt_22_move(triangulation),
            MoveType::Move13Add => self.attempt_13_move(triangulation),
            MoveType::Move31Remove => self.attempt_31_move(triangulation),
            MoveType::EdgeFlip => self.attempt_edge_flip(triangulation),
        }
    }

    /// Applies the shared 2D k=2 edge-flip implementation for `Move22` and `EdgeFlip`.
    fn attempt_causal_edge_flip(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
    ) -> MoveResult {
        let mut geometric_candidate_seen = false;
        let mut causal_candidate_count = 0;
        let mut selected_edge = None;

        let geometry = triangulation.geometry();
        for edge in geometry.edges() {
            let Ok(Some(adjacent)) = geometry.edge_adjacent_faces(&edge) else {
                continue;
            };
            geometric_candidate_seen = true;
            if !flip_is_causal(triangulation, &adjacent) {
                continue;
            }

            causal_candidate_count += 1;
            if self.rng.random_range(0..causal_candidate_count) == 0 {
                selected_edge = Some(edge);
            }
        }

        let Some(edge) = selected_edge else {
            if geometric_candidate_seen && triangulation.has_foliation() {
                return MoveResult::CausalityViolation;
            }
            return MoveResult::GeometricViolation;
        };

        let flip_target = format!("{edge:?}");
        let snapshot = triangulation.clone();
        let flip_result = triangulation.flip_edge(edge);

        let result = match flip_result {
            Ok(_) => self.finish_mutated_move(triangulation, move_type),
            Err(err) => reject_backend(BackendMutationOperation::FlipEdge, flip_target, &err),
        };
        rollback_if_failed(triangulation, snapshot, result)
    }

    /// Completes a move after the backend mutation has already succeeded.
    fn finish_mutated_move(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
    ) -> MoveResult {
        if let Err(err) = triangulation.synchronize_foliation_from_live_labels() {
            return MoveResult::HardFailure(err);
        }

        self.stats.record_success(move_type);
        MoveResult::Success
    }
}

impl Default for ErgodicsSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Restores the triangulation when a public move attempt does not complete successfully.
fn rollback_if_failed(
    triangulation: &mut CdtTriangulation2D,
    snapshot: CdtTriangulation2D,
    result: MoveResult,
) -> MoveResult {
    if matches!(result, MoveResult::Success) {
        return result;
    }

    *triangulation = snapshot;
    result
}

/// Converts an unexpected backend edit error into the move-level rejection shape.
///
/// Candidate selection should screen out ordinary geometric and causal
/// rejections before mutation. Reaching this helper means the backend refused
/// a selected site or returned an operation-specific error that should remain
/// visible to callers.
fn reject_backend(
    operation: BackendMutationOperation,
    target: impl Into<String>,
    err: &impl ToString,
) -> MoveResult {
    MoveResult::Rejected(CdtError::BackendMutationFailed {
        operation,
        target: target.into(),
        detail: err.to_string(),
    })
}

/// Computes topology-aware time distance between two slice labels.
///
/// Toroidal triangulations wrap around the time circle, so labels `0` and
/// `T-1` are one step apart; open-boundary triangulations use raw absolute
/// difference.
fn time_dist(triangulation: &CdtTriangulation2D, t0: u32, t1: u32) -> u32 {
    let raw = t0.abs_diff(t1);
    if matches!(triangulation.metadata().topology, CdtTopology::Toroidal) {
        let total = triangulation.time_slices();
        if total > 0 && t0 < total && t1 < total {
            return raw.min(total - raw);
        }
    }
    raw
}

/// Checks whether three time labels form one valid 2D CDT triangle.
///
/// A valid foliated CDT triangle has exactly one spacelike edge and two
/// timelike edges, using toroidal time distance where appropriate.
fn cdt_labels(triangulation: &CdtTriangulation2D, labels: [u32; 3]) -> bool {
    let mut spacelike = 0;
    let mut timelike = 0;

    for (a, b) in [
        (labels[0], labels[1]),
        (labels[1], labels[2]),
        (labels[2], labels[0]),
    ] {
        match time_dist(triangulation, a, b) {
            0 => spacelike += 1,
            1 => timelike += 1,
            _ => return false,
        }
    }

    spacelike == 1 && timelike == 2
}

/// Checks the CDT triangle rule for live backend vertices.
///
/// Unfoliated triangulations bypass the CDT time-label constraint because
/// there is no causal labeling to preserve.
fn cdt_vertices(triangulation: &CdtTriangulation2D, vertices: &[DelaunayVertexHandle]) -> bool {
    if !triangulation.has_foliation() {
        return true;
    }
    let [v0, v1, v2] = vertices else {
        return false;
    };
    cdt_vertex_triple(triangulation, [v0, v1, v2])
}

/// Reads three live backend vertex labels without allocating.
fn vertex_labels3(
    triangulation: &CdtTriangulation2D,
    vertices: [&DelaunayVertexHandle; 3],
) -> Option<[u32; 3]> {
    let labels = vertices.map(|vertex| {
        triangulation
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
    });
    let [Some(t0), Some(t1), Some(t2)] = labels else {
        return None;
    };
    Some([t0, t1, t2])
}

/// Checks the CDT triangle rule for three live backend vertices without allocation.
fn cdt_vertex_triple(
    triangulation: &CdtTriangulation2D,
    vertices: [&DelaunayVertexHandle; 3],
) -> bool {
    if !triangulation.has_foliation() {
        return true;
    }

    vertex_labels3(triangulation, vertices).is_some_and(|labels| cdt_labels(triangulation, labels))
}

/// Checks whether flipping an edge would preserve CDT triangle causality.
///
/// This is a pre-mutation guard for `(2,2)` and `EdgeFlip`, both of which share
/// the same underlying Delaunay k=2 flip.
fn flip_is_causal(
    triangulation: &CdtTriangulation2D,
    adjacent: &EdgeAdjacentFaces<DelaunayVertexHandle, DelaunayFaceHandle>,
) -> bool {
    let (endpoint_0, endpoint_1) = &adjacent.endpoints;
    let (opposite_0, opposite_1) = &adjacent.opposite_vertices;

    cdt_vertex_triple(triangulation, [endpoint_0, opposite_0, opposite_1])
        && cdt_vertex_triple(triangulation, [endpoint_1, opposite_0, opposite_1])
}

/// Finds the inserted vertex label that makes a `(1,3)` subdivision causal.
///
/// The candidate label must keep all three replacement triangles valid CDT
/// triangles; unfoliated triangulations return a marker that skips labeling.
fn insertion_label(
    triangulation: &CdtTriangulation2D,
    face: &DelaunayFaceHandle,
) -> Option<InsertionLabel> {
    if !triangulation.has_foliation() {
        return Some(InsertionLabel::Unfoliated);
    }

    let vertices = triangulation.geometry().face_vertices(face).ok()?;
    let [v0, v1, v2] = vertices.as_slice() else {
        return None;
    };
    let [t0, t1, t2] = vertex_labels3(triangulation, [v0, v1, v2])?;

    let candidates = [t0, t1, t2];
    for (index, candidate) in candidates.into_iter().enumerate() {
        if candidates[..index].contains(&candidate) {
            continue;
        }
        let valid = cdt_labels(triangulation, [candidate, t0, t1])
            && cdt_labels(triangulation, [candidate, t1, t2])
            && cdt_labels(triangulation, [candidate, t2, t0]);
        if valid {
            return Some(InsertionLabel::Label(candidate));
        }
    }
    None
}

/// Reads a 2D vertex coordinate from the backend.
fn vertex_point_2d(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
) -> Option<[f64; 2]> {
    let coords = triangulation.geometry().vertex_coordinates(vertex).ok()?;
    let [x, y] = coords.as_slice() else {
        return None;
    };
    Some([*x, *y])
}

/// Computes a 2D face centroid for the `(1,3)` insertion point.
///
/// Returning `None` keeps malformed or non-triangular faces out of the mutation
/// path instead of relying on the backend to reject them later.
fn centroid(triangulation: &CdtTriangulation2D, face: &DelaunayFaceHandle) -> Option<[f64; 2]> {
    let vertices = triangulation.geometry().face_vertices(face).ok()?;
    let [v0, v1, v2] = vertices.as_slice() else {
        return None;
    };
    let coords = [
        vertex_point_2d(triangulation, v0)?,
        vertex_point_2d(triangulation, v1)?,
        vertex_point_2d(triangulation, v2)?,
    ];

    if matches!(triangulation.metadata().topology, CdtTopology::Toroidal) {
        return toroidal_centroid(&coords, triangulation.geometry().periodic_domain()?);
    }

    Some([
        (coords[0][0] + coords[1][0] + coords[2][0]) / 3.0,
        (coords[0][1] + coords[1][1] + coords[2][1]) / 3.0,
    ])
}

/// Computes a centroid in one periodic image, then wraps it back into the domain.
fn toroidal_centroid(coords: &[[f64; 2]], domain: [f64; 2]) -> Option<[f64; 2]> {
    let [reference, rest @ ..] = coords else {
        return None;
    };
    if rest.len() != 2
        || domain
            .iter()
            .any(|period| !period.is_finite() || *period <= 0.0)
    {
        return None;
    }

    let mut centroid = *reference;
    for coord in rest {
        for axis in 0..2 {
            let period = domain[axis];
            let mut unwrapped = coord[axis];
            let delta = unwrapped - reference[axis];
            if delta > period / 2.0 {
                unwrapped -= period;
            } else if delta < -period / 2.0 {
                unwrapped += period;
            }
            centroid[axis] += unwrapped;
        }
    }

    for axis in 0..2 {
        centroid[axis] = (centroid[axis] / 3.0).rem_euclid(domain[axis]);
    }
    Some(centroid)
}

/// Collects the three distinct neighboring vertices around a removable vertex.
///
/// A `(3,1)` move is geometrically available only at a degree-3 vertex whose
/// adjacent faces collapse back to one replacement triangle.
fn neighbors3(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
) -> Option<[DelaunayVertexHandle; 3]> {
    let adjacent_faces = triangulation.geometry().adjacent_faces(vertex).ok()?;
    if adjacent_faces.len() != 3 {
        return None;
    }

    let mut neighbors = [None, None, None];
    let mut neighbor_count = 0;
    for face in adjacent_faces {
        for candidate in triangulation.geometry().face_vertices(&face).ok()? {
            if &candidate == vertex
                || neighbors[..neighbor_count]
                    .iter()
                    .flatten()
                    .any(|seen| seen == &candidate)
            {
                continue;
            }
            let slot = neighbors.get_mut(neighbor_count)?;
            if slot.is_some() {
                return None;
            }
            *slot = Some(candidate);
            neighbor_count += 1;
        }
    }

    let [Some(v0), Some(v1), Some(v2)] = neighbors else {
        return None;
    };
    Some([v0, v1, v2])
}

/// Checks CDT-specific preconditions for a geometric `(3,1)` removal candidate.
fn removal_candidate_is_causal(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
    neighbors: &[DelaunayVertexHandle; 3],
) -> bool {
    if !cdt_vertices(triangulation, neighbors) {
        return false;
    }
    if !triangulation.has_foliation() {
        return true;
    }

    let Some(label) = triangulation
        .geometry()
        .vertex_data_by_key(vertex.vertex_key())
    else {
        return false;
    };
    let Ok(slice) = usize::try_from(label) else {
        return false;
    };
    triangulation
        .slice_sizes()
        .get(slice)
        .is_some_and(|&count| count > 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::CdtValidationCheck;
    use crate::geometry::DelaunayBackend2D;
    use crate::geometry::generators::{build_delaunay2_from_cells, build_delaunay2_with_data};
    use approx::assert_relative_eq;
    use std::collections::HashSet;

    /// Builds the minimal foliated triangle fixture used by `(1,3)` tests.
    fn single_triangle() -> CdtTriangulation2D {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");
        CdtTriangulation2D::from_labeled_delaunay(backend, 2, 2).expect("wrap labeled triangle")
    }

    /// Builds two foliated triangles sharing one interior edge for k=2 flips.
    fn square_two_triangles() -> CdtTriangulation2D {
        let dt = build_delaunay2_from_cells(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("build square CDT");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay square should validate");
        CdtTriangulation2D::from_labeled_delaunay(backend, 2, 2).expect("wrap square CDT")
    }

    #[test]
    fn test_move_statistics() {
        let mut stats = MoveStatistics::new();

        stats.record_attempt(MoveType::Move22);
        stats.record_attempt(MoveType::Move22);
        stats.record_success(MoveType::Move22);

        assert_eq!(stats.moves_22_attempted, 2);
        assert_eq!(stats.moves_22_accepted, 1);
        assert_relative_eq!(stats.acceptance_rate(MoveType::Move22), 0.5);
    }

    #[test]
    fn move_stats_variants() {
        let mut stats = MoveStatistics::new();

        for move_type in [
            MoveType::Move22,
            MoveType::Move13Add,
            MoveType::Move31Remove,
            MoveType::EdgeFlip,
        ] {
            assert_relative_eq!(stats.acceptance_rate(move_type), 0.0);
            stats.record_attempt(move_type);
            stats.record_success(move_type);
            assert_relative_eq!(stats.acceptance_rate(move_type), 1.0);
        }

        assert_eq!(stats.moves_22_attempted, 1);
        assert_eq!(stats.moves_13_attempted, 1);
        assert_eq!(stats.moves_31_attempted, 1);
        assert_eq!(stats.edge_flips_attempted, 1);
        assert_eq!(stats.moves_22_accepted, 1);
        assert_eq!(stats.moves_13_accepted, 1);
        assert_eq!(stats.moves_31_accepted, 1);
        assert_eq!(stats.edge_flips_accepted, 1);
        assert_relative_eq!(stats.total_acceptance_rate(), 1.0);
    }

    #[test]
    fn move_22_uses_real_tri() {
        let mut system = ErgodicsSystem::new();
        let mut triangulation = square_two_triangles();

        let result = system.attempt_22_move(&mut triangulation);

        assert_eq!(system.stats.moves_22_attempted, 1);
        assert_eq!(result, MoveResult::Success);
        assert_eq!(system.stats.moves_22_accepted, 1);
        assert!(
            triangulation
                .geometry()
                .triangulation()
                .tds()
                .is_valid()
                .is_ok()
        );
        assert!(triangulation.validate_causality().is_ok());
    }

    #[test]
    fn move_22_rejects_boundary_edge() {
        let mut system = ErgodicsSystem::new();
        let mut triangulation = single_triangle();
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        );

        let result = system.attempt_22_move(&mut triangulation);

        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(system.stats.moves_22_attempted, 1);
        assert_eq!(system.stats.moves_22_accepted, 0);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
            ),
            counts_before
        );
    }

    #[test]
    fn move_13_inserts_labeled_vertex() {
        let mut system = ErgodicsSystem::new();
        let mut triangulation = single_triangle();
        let before_vertices = triangulation.vertex_count();

        let result = system.attempt_13_move(&mut triangulation);

        assert_eq!(system.stats.moves_13_attempted, 1);
        assert_eq!(result, MoveResult::Success);
        assert_eq!(triangulation.vertex_count(), before_vertices + 1);
        assert!(
            triangulation
                .geometry()
                .triangulation()
                .tds()
                .is_valid()
                .is_ok()
        );
        assert!(triangulation.validate_causality().is_ok());
        assert!(triangulation.has_foliation());
    }

    #[test]
    fn rollback_restores_snapshot_on_hard_failure() {
        let mut triangulation = single_triangle();
        let snapshot = triangulation.clone();
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count,
        );

        let face = triangulation
            .geometry()
            .faces()
            .next()
            .expect("triangle face");
        let point = centroid(&triangulation, &face).expect("triangle centroid");
        triangulation
            .subdivide_face(face, &point)
            .expect("subdivide fixture face");
        assert_ne!(triangulation.vertex_count(), counts_before.0);

        let result = rollback_if_failed(
            &mut triangulation,
            snapshot,
            MoveResult::HardFailure(CdtError::ValidationFailed {
                check: CdtValidationCheck::ErgodicMoveCandidateGeometry,
                detail: "simulated post-mutation failure".to_string(),
            }),
        );

        assert!(matches!(result, MoveResult::HardFailure(_)));
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count,
            ),
            counts_before
        );
        assert!(triangulation.validate().is_ok());
    }

    #[test]
    fn unwraps_toroidal_centroid() {
        let point = toroidal_centroid(&[[0.0, 0.0], [3.0, 0.0], [3.0, 1.0]], [4.0, 3.0])
            .expect("toroidal centroid");

        assert_relative_eq!(point[0], 10.0 / 3.0, epsilon = 1e-12);
        assert_relative_eq!(point[1], 1.0 / 3.0, epsilon = 1e-12);
    }

    #[test]
    fn toroidal_centroid_wraps_across_both_periodic_seams() {
        let point = toroidal_centroid(&[[3.9, 2.9], [0.1, 2.8], [0.2, 0.1]], [4.0, 3.0])
            .expect("toroidal centroid across both seams");

        assert_relative_eq!(point[0], 0.066_666_666_666_666_43, epsilon = 1e-12);
        assert_relative_eq!(point[1], 2.933_333_333_333_333, epsilon = 1e-12);
    }

    #[test]
    fn toroidal_centroid_handles_half_period_ties_deterministically() {
        let point = toroidal_centroid(&[[0.0, 0.0], [2.0, 0.0], [2.0, 1.5]], [4.0, 3.0])
            .expect("half-period centroid should remain defined");

        assert_relative_eq!(point[0], 4.0 / 3.0, epsilon = 1e-12);
        assert_relative_eq!(point[1], 0.5, epsilon = 1e-12);
    }

    #[test]
    fn checked_toroidal_wrapper_rejects_topology_mismatch() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");
        let result = CdtTriangulation2D::with_topology(backend, 3, 2, CdtTopology::Toroidal);

        assert!(matches!(
            result,
            Err(CdtError::TopologyMismatch {
                topology,
                euler_characteristic: 1,
                expected_euler_characteristics,
                ..
            }) if topology == CdtTopology::Toroidal && expected_euler_characteristics == [0]
        ));
    }

    #[test]
    fn move_31_removes_degree_three() {
        let mut system = ErgodicsSystem::new();
        let mut triangulation = single_triangle();
        let result = system.attempt_13_move(&mut triangulation);
        assert!(matches!(result, MoveResult::Success));
        let before_vertices = triangulation.vertex_count();

        let result = system.attempt_31_move(&mut triangulation);

        assert_eq!(system.stats.moves_31_attempted, 1);
        assert_eq!(result, MoveResult::Success);
        assert_eq!(triangulation.vertex_count(), before_vertices - 1);
        assert!(
            triangulation
                .geometry()
                .triangulation()
                .tds()
                .is_valid()
                .is_ok()
        );
        assert!(triangulation.validate_causality().is_ok());
    }

    #[test]
    fn move_31_requires_degree_three() {
        let mut system = ErgodicsSystem::new();
        let mut triangulation = single_triangle();
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        );

        let result = system.attempt_31_move(&mut triangulation);

        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(system.stats.moves_31_attempted, 1);
        assert_eq!(system.stats.moves_31_accepted, 0);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
            ),
            counts_before
        );
    }

    #[test]
    fn periodic_toroidal_move_13_reports_backend_offset_limitation() {
        let mut system = ErgodicsSystem::with_seed(7);
        let mut triangulation =
            CdtTriangulation2D::from_toroidal_cdt(8, 8).expect("build toroidal CDT");
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        );

        let result = system.attempt_13_move(&mut triangulation);

        assert!(
            matches!(
                result,
                MoveResult::Rejected(CdtError::BackendMutationFailed {
                    ref operation,
                    ref detail,
                    ..
                }) if *operation == BackendMutationOperation::SubdivideFace
                    && detail.contains("periodic external cell")
                    && detail.contains("aligned periodic offsets")
            ),
            "periodic toroidal Move13Add should expose the upstream periodic-offset flip limitation, got {result:?}"
        );
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
            ),
            counts_before,
            "rejected periodic toroidal mutation should roll back geometry"
        );
        triangulation
            .validate()
            .expect("rejected periodic toroidal move should preserve evolved CDT invariants");
    }

    #[test]
    fn edge_flip_uses_own_stats() {
        let mut system = ErgodicsSystem::new();
        let mut triangulation = square_two_triangles();

        let result = system.attempt_edge_flip(&mut triangulation);

        assert_eq!(system.stats.edge_flips_attempted, 1);
        assert_eq!(system.stats.moves_22_attempted, 0);
        assert_eq!(result, MoveResult::Success);
        assert_eq!(system.stats.edge_flips_accepted, 1);
        assert!(triangulation.validate_causality().is_ok());
    }

    #[test]
    fn test_random_move_selection() {
        let mut system = ErgodicsSystem::new();

        let mut move_types = HashSet::new();
        for _ in 0..100 {
            move_types.insert(system.select_random_move());
        }

        assert!(move_types.len() > 1);
    }

    #[test]
    fn test_total_acceptance_rate() {
        let mut stats = MoveStatistics::new();

        stats.record_attempt(MoveType::Move22);
        stats.record_success(MoveType::Move22);
        stats.record_attempt(MoveType::Move13Add);

        assert_relative_eq!(stats.total_acceptance_rate(), 0.5);
    }

    #[test]
    fn total_acceptance_no_attempts() {
        let stats = MoveStatistics::new();

        assert_relative_eq!(stats.total_acceptance_rate(), 0.0);
    }
}
