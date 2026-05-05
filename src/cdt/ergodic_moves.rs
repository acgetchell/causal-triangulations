#![forbid(unsafe_code)]

//! Ergodic moves for 2D Causal Dynamical Triangulations.
//!
//! This module implements the standard local moves used in 2D CDT:
//! - (2,2) moves: flip the shared edge between two triangles
//! - (1,3) moves: split a triangle by inserting a vertex
//! - (3,1) moves: collapse a degree-3 vertex back to one triangle
//! - edge flips: retained as an API-compatible alias for the 2D (2,2) move

use crate::config::CdtTopology;
use crate::errors::CdtError;
use crate::geometry::CdtTriangulation2D;
use crate::geometry::backends::delaunay::{DelaunayFaceHandle, DelaunayVertexHandle};
use crate::geometry::traits::{EdgeAdjacentFaces, TriangulationMut, TriangulationQuery};
use num_traits::cast::NumCast;
use rand::{RngExt, SeedableRng, rngs::StdRng};

/// Types of ergodic moves available in 2D CDT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Default)]
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
            let accepted: f64 = NumCast::from(accepted).unwrap_or(0.0);
            let attempted: f64 = NumCast::from(attempted).unwrap_or(f64::INFINITY);
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
            let total_accepted: f64 = NumCast::from(total_accepted).unwrap_or(0.0);
            let total_attempted: f64 = NumCast::from(total_attempted).unwrap_or(f64::INFINITY);
            total_accepted / total_attempted
        }
    }
}

/// Ergodic move system for CDT triangulations.
pub struct ErgodicsSystem {
    /// Move statistics
    pub stats: MoveStatistics,
    /// Random number generator
    rng: StdRng,
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
            rng: StdRng::seed_from_u64(seed),
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
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
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
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled triangle");
    /// let mut system = ErgodicsSystem::new();
    /// let result = system.attempt_13_move(&mut triangulation);
    /// assert!(matches!(result, MoveResult::Success | MoveResult::GeometricViolation));
    /// assert_eq!(system.stats.moves_13_attempted, 1);
    /// ```
    pub fn attempt_13_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move13Add);

        let geometric_candidates: Vec<_> = triangulation
            .geometry()
            .faces()
            .filter(|face| centroid(triangulation, face).is_some())
            .collect();
        if geometric_candidates.is_empty() {
            return MoveResult::GeometricViolation;
        }

        let candidates: Vec<_> = geometric_candidates
            .into_iter()
            .filter(|face| insertion_label(triangulation, face).is_some())
            .collect();
        let Some(face) =
            pick(&mut self.rng, candidates.len()).map(|index| candidates[index].clone())
        else {
            return if triangulation.has_foliation() {
                MoveResult::CausalityViolation
            } else {
                MoveResult::GeometricViolation
            };
        };

        let Some(point) = centroid(triangulation, &face) else {
            return MoveResult::Rejected(CdtError::ValidationFailed {
                check: "ergodic Move13Add candidate geometry".to_string(),
                detail: format!(
                    "face {:?} could not be converted to a 2D centroid",
                    face.cell_key()
                ),
            });
        };
        let label = insertion_label(triangulation, &face);

        let subdivision_target = format!("face {:?}", face.cell_key());
        let subdivision = {
            let mut geometry = triangulation.geometry_mut();
            geometry.subdivide_face(face, &point)
        };

        let subdivision = match subdivision {
            Ok(subdivision) => subdivision,
            Err(err) => {
                return reject_backend("subdivide_face", subdivision_target, &err);
            }
        };

        if let Some(InsertionLabel::Label(label)) = label {
            let set_label = {
                let mut geometry = triangulation.geometry_mut();
                geometry.set_vertex_data_by_key(subdivision.new_vertex.vertex_key(), Some(label))
            };
            if let Err(err) = set_label {
                return MoveResult::HardFailure(CdtError::BackendMutationFailed {
                    operation: "set_vertex_data_by_key".to_string(),
                    target: format!("vertex {:?}", subdivision.new_vertex.vertex_key()),
                    detail: err.to_string(),
                });
            }
        }

        self.finish_mutated_move(triangulation, MoveType::Move13Add)
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
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
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

        let geometric_candidates: Vec<_> = triangulation
            .geometry()
            .vertices()
            .filter(|vertex| neighbors3(triangulation, vertex).is_some())
            .collect();
        if geometric_candidates.is_empty() {
            return MoveResult::GeometricViolation;
        }

        let candidates: Vec<_> = geometric_candidates
            .into_iter()
            .filter(|vertex| removal_is_causal(triangulation, vertex))
            .collect();
        let Some(vertex) =
            pick(&mut self.rng, candidates.len()).map(|index| candidates[index].clone())
        else {
            return if triangulation.has_foliation() {
                MoveResult::CausalityViolation
            } else {
                MoveResult::GeometricViolation
            };
        };

        let removal_target = format!("vertex {:?}", vertex.vertex_key());
        let removal = {
            let mut geometry = triangulation.geometry_mut();
            geometry.remove_vertex(vertex)
        };

        match removal {
            Ok(_) => self.finish_mutated_move(triangulation, MoveType::Move31Remove),
            Err(err) => reject_backend("remove_vertex", removal_target, &err),
        }
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
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
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
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
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
        self.stats.record_attempt(move_type);

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
        let flip_result = {
            let mut geometry = triangulation.geometry_mut();
            geometry.flip_edge(edge)
        };

        match flip_result {
            Ok(_) => self.finish_mutated_move(triangulation, move_type),
            Err(err) => reject_backend("flip_edge", flip_target, &err),
        }
    }

    /// Completes a move after the backend mutation has already succeeded.
    fn finish_mutated_move(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
    ) -> MoveResult {
        match triangulation.synchronize_foliation_from_live_labels() {
            Ok(()) => {
                self.stats.record_success(move_type);
                MoveResult::Success
            }
            Err(err) => MoveResult::HardFailure(err),
        }
    }
}

impl Default for ErgodicsSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Selects a random candidate index without borrowing the candidate list.
fn pick(rng: &mut StdRng, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(rng.random_range(0..len))
}

/// Converts an unexpected backend edit error into the move-level rejection shape.
///
/// Candidate selection should screen out ordinary geometric and causal
/// rejections before mutation. Reaching this helper means the backend refused
/// a selected site or returned an operation-specific error that should remain
/// visible to callers.
fn reject_backend(
    operation: impl Into<String>,
    target: impl Into<String>,
    err: &impl ToString,
) -> MoveResult {
    MoveResult::Rejected(CdtError::BackendMutationFailed {
        operation: operation.into(),
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

/// Reads time labels for a vertex tuple when the triangulation is foliated.
///
/// Missing labels make a move site invalid because subsequent foliation
/// resynchronization would fail after mutation.
fn labels(
    triangulation: &CdtTriangulation2D,
    vertices: &[DelaunayVertexHandle],
) -> Option<Vec<u32>> {
    if !triangulation.has_foliation() {
        return None;
    }
    vertices
        .iter()
        .map(|vertex| {
            triangulation
                .geometry()
                .vertex_data_by_key(vertex.vertex_key())
        })
        .collect()
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
    let Some(labels) = labels(triangulation, vertices) else {
        return false;
    };
    let [t0, t1, t2] = labels.as_slice() else {
        return false;
    };
    cdt_labels(triangulation, [*t0, *t1, *t2])
}

/// Checks the CDT triangle rule for three live backend vertices without allocation.
fn cdt_vertex_triple(
    triangulation: &CdtTriangulation2D,
    vertices: [&DelaunayVertexHandle; 3],
) -> bool {
    if !triangulation.has_foliation() {
        return true;
    }

    let labels = vertices.map(|vertex| {
        triangulation
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
    });
    let [Some(t0), Some(t1), Some(t2)] = labels else {
        return false;
    };
    cdt_labels(triangulation, [t0, t1, t2])
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
    let labels = labels(triangulation, &vertices)?;
    let [t0, t1, t2] = labels.as_slice() else {
        return None;
    };
    let mut candidates = vec![*t0, *t1, *t2];
    candidates.sort_unstable();
    candidates.dedup();

    candidates.into_iter().find_map(|candidate| {
        let valid = cdt_labels(triangulation, [candidate, *t0, *t1])
            && cdt_labels(triangulation, [candidate, *t1, *t2])
            && cdt_labels(triangulation, [candidate, *t2, *t0]);
        valid.then_some(InsertionLabel::Label(candidate))
    })
}

/// Computes a 2D face centroid for the `(1,3)` insertion point.
///
/// Returning `None` keeps malformed or non-triangular faces out of the mutation
/// path instead of relying on the backend to reject them later.
fn centroid(triangulation: &CdtTriangulation2D, face: &DelaunayFaceHandle) -> Option<[f64; 2]> {
    let vertices = triangulation.geometry().face_vertices(face).ok()?;
    if vertices.len() != 3 {
        return None;
    }

    let mut coords = Vec::with_capacity(3);
    for vertex in vertices {
        let vertex_coords = triangulation.geometry().vertex_coordinates(&vertex).ok()?;
        let [x, y] = vertex_coords.as_slice() else {
            return None;
        };
        coords.push([*x, *y]);
    }

    if matches!(triangulation.metadata().topology, CdtTopology::Toroidal) {
        return toroidal_centroid(&coords, triangulation.geometry().periodic_domain()?);
    }

    let mut centroid = [0.0, 0.0];
    for [x, y] in coords {
        centroid[0] += x;
        centroid[1] += y;
    }
    centroid[0] /= 3.0;
    centroid[1] /= 3.0;
    Some(centroid)
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

/// Counts live vertices carrying a given time label.
///
/// `(3,1)` removal uses this to avoid emptying a time slice after deleting a
/// labeled vertex.
fn label_count(triangulation: &CdtTriangulation2D, label: u32) -> usize {
    triangulation
        .geometry()
        .vertices()
        .filter(|vertex| {
            triangulation
                .geometry()
                .vertex_data_by_key(vertex.vertex_key())
                == Some(label)
        })
        .count()
}

/// Collects the three distinct neighboring vertices around a removable vertex.
///
/// A `(3,1)` move is geometrically available only at a degree-3 vertex whose
/// adjacent faces collapse back to one replacement triangle.
fn neighbors3(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
) -> Option<Vec<DelaunayVertexHandle>> {
    let adjacent_faces = triangulation.geometry().adjacent_faces(vertex).ok()?;
    if adjacent_faces.len() != 3 {
        return None;
    }

    let mut neighbors = Vec::with_capacity(3);
    for face in adjacent_faces {
        for candidate in triangulation.geometry().face_vertices(&face).ok()? {
            if &candidate != vertex && !neighbors.iter().any(|seen| seen == &candidate) {
                neighbors.push(candidate);
            }
        }
    }

    (neighbors.len() == 3).then_some(neighbors)
}

/// Checks CDT-specific preconditions for a `(3,1)` removal.
///
/// The replacement triangle must be causal, and in foliated triangulations the
/// removed vertex must not be the last live vertex in its time slice.
fn removal_is_causal(triangulation: &CdtTriangulation2D, vertex: &DelaunayVertexHandle) -> bool {
    let Some(neighbors) = neighbors3(triangulation, vertex) else {
        return false;
    };
    if !cdt_vertices(triangulation, &neighbors) {
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
    label_count(triangulation, label) > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::DelaunayBackend2D;
    use crate::geometry::generators::{build_delaunay2_from_cells, build_delaunay2_with_data};
    use approx::{abs_diff_eq, assert_relative_eq};
    use std::collections::HashSet;

    /// Builds the minimal foliated triangle fixture used by `(1,3)` tests.
    fn single_triangle() -> CdtTriangulation2D {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
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
        let backend = DelaunayBackend2D::from_triangulation(dt);
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
    fn unwraps_toroidal_centroid() {
        let triangulation =
            CdtTriangulation2D::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        let face = triangulation
            .geometry()
            .faces()
            .find(|face| {
                let vertices = triangulation
                    .geometry()
                    .face_vertices(face)
                    .expect("face vertices");
                let mut zero_x = 0;
                let mut boundary_x = 0;
                let mut zero_y = 0;
                let mut next_y = 0;
                for vertex in vertices {
                    let coords = triangulation
                        .geometry()
                        .vertex_coordinates(&vertex)
                        .expect("vertex coordinates");
                    if abs_diff_eq!(coords[0], 0.0, epsilon = 1e-12) {
                        zero_x += 1;
                    }
                    if abs_diff_eq!(coords[0], 0.75, epsilon = 1e-12) {
                        boundary_x += 1;
                    }
                    if abs_diff_eq!(coords[1], 0.0, epsilon = 1e-12) {
                        zero_y += 1;
                    }
                    if abs_diff_eq!(coords[1], 1.0 / 3.0, epsilon = 1e-12) {
                        next_y += 1;
                    }
                }
                zero_x == 1 && boundary_x == 2 && zero_y == 2 && next_y == 1
            })
            .expect("wrap-around face");

        let point = centroid(&triangulation, &face).expect("toroidal centroid");

        assert_relative_eq!(point[0], 5.0 / 6.0, epsilon = 1e-12);
        assert_relative_eq!(point[1], 1.0 / 9.0, epsilon = 1e-12);
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
