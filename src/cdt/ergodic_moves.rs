#![forbid(unsafe_code)]

//! Ergodic moves for 2D Causal Dynamical Triangulations.
//!
//! This module implements the standard local moves used in 2D CDT:
//! - (2,2) moves: flip the shared edge between two triangles
//! - (1,3) moves: split a triangle by inserting a vertex
//! - (3,1) moves: collapse a degree-3 vertex back to one triangle
//! - edge flips: retained as an API-compatible alias for the 2D (2,2) move

use crate::cdt::proposal_policy::{CdtMoveFamilyDistribution, CdtProposalPolicyView};
use crate::cdt::triangulation::{CdtTriangulation2D, LocalMoveBaseline, LocalMoveDelta};
use crate::config::CdtTopology;
use crate::errors::{
    BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure,
    CheckpointResumeFailure,
};
use crate::geometry::backends::delaunay::{
    DelaunayEdgeHandle, DelaunayFaceHandle, DelaunayFaceStableId, DelaunayVertexHandle,
};
use crate::geometry::traits::{
    EdgeAdjacentFaces, TriangulationMut, TriangulationQuery, exactly_three,
};
use rand::{RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Deserializer, Serialize};
use std::array;
use std::collections::HashSet;
use std::fmt::Display;

/// Types of ergodic moves available in 2D CDT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MoveType {
    /// (2,2) move: Flip edge between two triangles
    Move22,
    /// (1,3) move: Add a vertex by subdividing local CDT volume
    Move13Add,
    /// (3,1) move: Remove a vertex by collapsing local CDT volume
    Move31Remove,
    /// Edge flip: API-compatible alias for the 2D (2,2) move
    EdgeFlip,
}

impl MoveType {
    /// Stable ordering of the reversible 1+1 CDT proposal families.
    ///
    /// Policy implementations should iterate this array instead of relying on
    /// enum discriminants or serialization details. The order is part of the
    /// public proposal-policy contract.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveType;
    ///
    /// assert_eq!(MoveType::REVERSIBLE_1P1.len(), 4);
    /// assert_eq!(MoveType::REVERSIBLE_1P1[0], MoveType::Move22);
    /// ```
    pub const REVERSIBLE_1P1: [Self; 4] = [
        Self::Move22,
        Self::Move13Add,
        Self::Move31Remove,
        Self::EdgeFlip,
    ];

    /// Returns the stable textual identifier for this move family.
    ///
    /// These identifiers are intended for policy tables, telemetry schemas,
    /// and other external mappings. They do not expose enum discriminants or
    /// checkpoint serialization.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveType;
    ///
    /// assert_eq!(MoveType::Move13Add.identifier(), "move-1-3-add");
    /// ```
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Move22 => "move-2-2",
            Self::Move13Add => "move-1-3-add",
            Self::Move31Remove => "move-3-1-remove",
            Self::EdgeFlip => "edge-flip",
        }
    }

    /// Returns the reversible family used to propose the inverse transition.
    ///
    /// The two flip names are self-inverse, while `(1,3)` and `(3,1)` are an
    /// inverse pair.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveType;
    ///
    /// assert_eq!(MoveType::Move13Add.reverse(), MoveType::Move31Remove);
    /// assert_eq!(MoveType::EdgeFlip.reverse(), MoveType::EdgeFlip);
    /// ```
    #[must_use]
    pub const fn reverse(self) -> Self {
        match self {
            Self::Move22 => Self::Move22,
            Self::Move13Add => Self::Move31Remove,
            Self::Move31Remove => Self::Move13Add,
            Self::EdgeFlip => Self::EdgeFlip,
        }
    }
}

/// Result of attempting an ergodic move.
#[derive(Debug, Clone, PartialEq)]
pub enum MoveResult {
    /// Move was successfully applied and validated as the next CDT state
    Success,
    /// Move was rejected due to causality constraints
    CausalityViolation,
    /// Move was rejected due to geometric constraints
    GeometricViolation,
    /// Move was rejected for other reasons
    Rejected(CdtError),
    /// Move mutated geometry but failed a required post-mutation invariant refresh.
    ///
    /// Hard failures are rolled back by public move attempts and are tracked
    /// separately from ordinary proposal rejections in [`MoveStatistics`].
    HardFailure(CdtError),
}

/// Statistics tracking for ergodic moves.
///
/// Attempts count every selected move proposal. Accepted counts include only
/// moves that committed and validated successfully. Hard-failure counts record
/// proposals that mutated the backend but failed post-mutation CDT invariants;
/// those failures remain in the attempt denominator but are not counted as
/// accepted moves. Individual counters and aggregate totals saturate at
/// `u64::MAX` so long-running or restored simulations cannot wrap telemetry.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MoveStatistics {
    /// Number of (2,2) moves attempted
    moves_22_attempted: u64,
    /// Number of (2,2) moves accepted
    moves_22_accepted: u64,
    /// Number of (2,2) moves that mutated state but failed post-mutation invariants.
    #[serde(default)]
    moves_22_hard_failed: u64,
    /// Number of (1,3) moves attempted
    moves_13_attempted: u64,
    /// Number of (1,3) moves accepted
    moves_13_accepted: u64,
    /// Number of (1,3) moves that mutated state but failed post-mutation invariants.
    #[serde(default)]
    moves_13_hard_failed: u64,
    /// Number of (3,1) moves attempted
    moves_31_attempted: u64,
    /// Number of (3,1) moves accepted
    moves_31_accepted: u64,
    /// Number of (3,1) moves that mutated state but failed post-mutation invariants.
    #[serde(default)]
    moves_31_hard_failed: u64,
    /// Number of edge flips attempted
    edge_flips_attempted: u64,
    /// Number of edge flips accepted
    edge_flips_accepted: u64,
    /// Number of edge flips that mutated state but failed post-mutation invariants.
    #[serde(default)]
    edge_flips_hard_failed: u64,
}

#[derive(Deserialize)]
struct MoveStatisticsWire {
    moves_22_attempted: u64,
    moves_22_accepted: u64,
    #[serde(default)]
    moves_22_hard_failed: u64,
    moves_13_attempted: u64,
    moves_13_accepted: u64,
    #[serde(default)]
    moves_13_hard_failed: u64,
    moves_31_attempted: u64,
    moves_31_accepted: u64,
    #[serde(default)]
    moves_31_hard_failed: u64,
    edge_flips_attempted: u64,
    edge_flips_accepted: u64,
    #[serde(default)]
    edge_flips_hard_failed: u64,
}

impl<'de> Deserialize<'de> for MoveStatistics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = MoveStatisticsWire::deserialize(deserializer)?;
        Self::from_wire(&wire).map_err(serde::de::Error::custom)
    }
}

impl MoveStatistics {
    /// Creates a new statistics tracker.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let stats = MoveStatistics::new();
    /// assert_eq!(stats.attempted(MoveType::Move22), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    #[expect(
        clippy::too_many_arguments,
        reason = "test and serde helpers need to preserve the flat move-statistics wire shape"
    )]
    pub(crate) const fn from_validated_parts(
        moves_22_attempted: u64,
        moves_22_accepted: u64,
        moves_22_hard_failed: u64,
        moves_13_attempted: u64,
        moves_13_accepted: u64,
        moves_13_hard_failed: u64,
        moves_31_attempted: u64,
        moves_31_accepted: u64,
        moves_31_hard_failed: u64,
        edge_flips_attempted: u64,
        edge_flips_accepted: u64,
        edge_flips_hard_failed: u64,
    ) -> Self {
        Self {
            moves_22_attempted,
            moves_22_accepted,
            moves_22_hard_failed,
            moves_13_attempted,
            moves_13_accepted,
            moves_13_hard_failed,
            moves_31_attempted,
            moves_31_accepted,
            moves_31_hard_failed,
            edge_flips_attempted,
            edge_flips_accepted,
            edge_flips_hard_failed,
        }
    }

    /// Rebuilds move statistics from the serialized wire shape while preserving counter invariants.
    ///
    /// Deserialization rejects impossible counters such as accepted moves plus
    /// hard failures exceeding attempts, so public accessors can treat stored
    /// move-family counters as coherent telemetry.
    fn from_wire(wire: &MoveStatisticsWire) -> CdtResult<Self> {
        validate_move_counter(
            MoveType::Move22,
            wire.moves_22_attempted,
            wire.moves_22_accepted,
            wire.moves_22_hard_failed,
        )?;
        validate_move_counter(
            MoveType::Move13Add,
            wire.moves_13_attempted,
            wire.moves_13_accepted,
            wire.moves_13_hard_failed,
        )?;
        validate_move_counter(
            MoveType::Move31Remove,
            wire.moves_31_attempted,
            wire.moves_31_accepted,
            wire.moves_31_hard_failed,
        )?;
        validate_move_counter(
            MoveType::EdgeFlip,
            wire.edge_flips_attempted,
            wire.edge_flips_accepted,
            wire.edge_flips_hard_failed,
        )?;
        Ok(Self {
            moves_22_attempted: wire.moves_22_attempted,
            moves_22_accepted: wire.moves_22_accepted,
            moves_22_hard_failed: wire.moves_22_hard_failed,
            moves_13_attempted: wire.moves_13_attempted,
            moves_13_accepted: wire.moves_13_accepted,
            moves_13_hard_failed: wire.moves_13_hard_failed,
            moves_31_attempted: wire.moves_31_attempted,
            moves_31_accepted: wire.moves_31_accepted,
            moves_31_hard_failed: wire.moves_31_hard_failed,
            edge_flips_attempted: wire.edge_flips_attempted,
            edge_flips_accepted: wire.edge_flips_accepted,
            edge_flips_hard_failed: wire.edge_flips_hard_failed,
        })
    }

    /// Records an attempted move, saturating the matching counter at `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// assert_eq!(stats.attempted(MoveType::Move22), 1);
    /// ```
    pub const fn record_attempt(&mut self, move_type: MoveType) {
        match move_type {
            MoveType::Move22 => {
                self.moves_22_attempted = self.moves_22_attempted.saturating_add(1);
            }
            MoveType::Move13Add => {
                self.moves_13_attempted = self.moves_13_attempted.saturating_add(1);
            }
            MoveType::Move31Remove => {
                self.moves_31_attempted = self.moves_31_attempted.saturating_add(1);
            }
            MoveType::EdgeFlip => {
                self.edge_flips_attempted = self.edge_flips_attempted.saturating_add(1);
            }
        }
    }

    /// Records a successful move that committed and validated.
    ///
    /// The matching accepted counter saturates at `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_success(MoveType::EdgeFlip);
    /// assert_eq!(stats.accepted(MoveType::EdgeFlip), 1);
    /// ```
    pub const fn record_success(&mut self, move_type: MoveType) {
        match move_type {
            MoveType::Move22 => {
                if self
                    .moves_22_accepted
                    .saturating_add(self.moves_22_hard_failed)
                    >= self.moves_22_attempted
                {
                    self.moves_22_attempted = self.moves_22_attempted.saturating_add(1);
                }
                self.moves_22_accepted = self.moves_22_accepted.saturating_add(1);
            }
            MoveType::Move13Add => {
                if self
                    .moves_13_accepted
                    .saturating_add(self.moves_13_hard_failed)
                    >= self.moves_13_attempted
                {
                    self.moves_13_attempted = self.moves_13_attempted.saturating_add(1);
                }
                self.moves_13_accepted = self.moves_13_accepted.saturating_add(1);
            }
            MoveType::Move31Remove => {
                if self
                    .moves_31_accepted
                    .saturating_add(self.moves_31_hard_failed)
                    >= self.moves_31_attempted
                {
                    self.moves_31_attempted = self.moves_31_attempted.saturating_add(1);
                }
                self.moves_31_accepted = self.moves_31_accepted.saturating_add(1);
            }
            MoveType::EdgeFlip => {
                if self
                    .edge_flips_accepted
                    .saturating_add(self.edge_flips_hard_failed)
                    >= self.edge_flips_attempted
                {
                    self.edge_flips_attempted = self.edge_flips_attempted.saturating_add(1);
                }
                self.edge_flips_accepted = self.edge_flips_accepted.saturating_add(1);
            }
        }
    }

    /// Records a move that mutated state but failed a post-mutation invariant.
    ///
    /// Hard failures are distinct from ordinary proposal rejections and are not
    /// counted as accepted moves. Call this after the corresponding
    /// [`Self::record_attempt`] so acceptance-rate denominators continue to
    /// reflect all selected proposals. The matching hard-failure counter
    /// saturates at `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move31Remove);
    /// stats.record_hard_failure(MoveType::Move31Remove);
    /// assert_eq!(stats.accepted(MoveType::Move31Remove), 0);
    /// assert_eq!(stats.hard_failed(MoveType::Move31Remove), 1);
    /// ```
    pub const fn record_hard_failure(&mut self, move_type: MoveType) {
        match move_type {
            MoveType::Move22 => {
                if self
                    .moves_22_accepted
                    .saturating_add(self.moves_22_hard_failed)
                    >= self.moves_22_attempted
                {
                    self.moves_22_attempted = self.moves_22_attempted.saturating_add(1);
                }
                self.moves_22_hard_failed = self.moves_22_hard_failed.saturating_add(1);
            }
            MoveType::Move13Add => {
                if self
                    .moves_13_accepted
                    .saturating_add(self.moves_13_hard_failed)
                    >= self.moves_13_attempted
                {
                    self.moves_13_attempted = self.moves_13_attempted.saturating_add(1);
                }
                self.moves_13_hard_failed = self.moves_13_hard_failed.saturating_add(1);
            }
            MoveType::Move31Remove => {
                if self
                    .moves_31_accepted
                    .saturating_add(self.moves_31_hard_failed)
                    >= self.moves_31_attempted
                {
                    self.moves_31_attempted = self.moves_31_attempted.saturating_add(1);
                }
                self.moves_31_hard_failed = self.moves_31_hard_failed.saturating_add(1);
            }
            MoveType::EdgeFlip => {
                if self
                    .edge_flips_accepted
                    .saturating_add(self.edge_flips_hard_failed)
                    >= self.edge_flips_attempted
                {
                    self.edge_flips_attempted = self.edge_flips_attempted.saturating_add(1);
                }
                self.edge_flips_hard_failed = self.edge_flips_hard_failed.saturating_add(1);
            }
        }
    }

    /// Calculates acceptance rate for a specific move type.
    ///
    /// The returned ratio is accepted attempts divided by all attempts for that
    /// move type. Ordinary rejections and hard failures both remain in the
    /// denominator; hard failures are additionally visible through
    /// [`Self::total_hard_failures`] and the per-move hard-failure fields.
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
    /// This is the total number of committed, validated moves divided by total
    /// attempts across all move types. Hard failures are not accepted moves, so
    /// they lower this rate and are separately reported by
    /// [`Self::total_hard_failures`].
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
        let total_attempted = self.total_attempted();
        let total_accepted = self.total_accepted();

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
    /// This includes proposals that were later accepted, rejected, or recorded
    /// as hard failures. The returned total saturates at `u64::MAX`.
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
            .saturating_add(self.moves_13_attempted)
            .saturating_add(self.moves_31_attempted)
            .saturating_add(self.edge_flips_attempted)
    }

    /// Returns the total number of accepted moves across all move types.
    ///
    /// This counts only moves that committed and validated successfully. It does
    /// not include ordinary rejections or hard failures. The returned total
    /// saturates at `u64::MAX`.
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
            .saturating_add(self.moves_13_accepted)
            .saturating_add(self.moves_31_accepted)
            .saturating_add(self.edge_flips_accepted)
    }

    /// Returns the total number of hard failures across all move types.
    ///
    /// A hard failure means a proposal mutated backend state before a required
    /// post-mutation CDT invariant check failed. Public move attempts roll back
    /// the triangulation before returning [`MoveResult::HardFailure`]. The
    /// returned total saturates at `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_hard_failure(MoveType::Move13Add);
    /// stats.record_hard_failure(MoveType::EdgeFlip);
    /// assert_eq!(stats.total_hard_failures(), 2);
    /// ```
    #[must_use]
    pub const fn total_hard_failures(&self) -> u64 {
        self.moves_22_hard_failed
            .saturating_add(self.moves_13_hard_failed)
            .saturating_add(self.moves_31_hard_failed)
            .saturating_add(self.edge_flips_hard_failed)
    }

    /// Returns the attempted counter for one move family.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// assert_eq!(stats.attempted(MoveType::Move22), 1);
    /// ```
    #[must_use]
    pub const fn attempted(&self, move_type: MoveType) -> u64 {
        match move_type {
            MoveType::Move22 => self.moves_22_attempted,
            MoveType::Move13Add => self.moves_13_attempted,
            MoveType::Move31Remove => self.moves_31_attempted,
            MoveType::EdgeFlip => self.edge_flips_attempted,
        }
    }

    /// Returns the accepted counter for one move family.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_success(MoveType::Move13Add);
    /// assert_eq!(stats.accepted(MoveType::Move13Add), 1);
    /// ```
    #[must_use]
    pub const fn accepted(&self, move_type: MoveType) -> u64 {
        match move_type {
            MoveType::Move22 => self.moves_22_accepted,
            MoveType::Move13Add => self.moves_13_accepted,
            MoveType::Move31Remove => self.moves_31_accepted,
            MoveType::EdgeFlip => self.edge_flips_accepted,
        }
    }

    /// Returns the hard-failure counter for one move family.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_hard_failure(MoveType::EdgeFlip);
    /// assert_eq!(stats.hard_failed(MoveType::EdgeFlip), 1);
    /// ```
    #[must_use]
    pub const fn hard_failed(&self, move_type: MoveType) -> u64 {
        match move_type {
            MoveType::Move22 => self.moves_22_hard_failed,
            MoveType::Move13Add => self.moves_13_hard_failed,
            MoveType::Move31Remove => self.moves_31_hard_failed,
            MoveType::EdgeFlip => self.edge_flips_hard_failed,
        }
    }
}

/// Checks one move family's serialized counters before they become invariant-bearing telemetry.
const fn validate_move_counter(
    move_type: MoveType,
    attempted: u64,
    accepted: u64,
    hard_failed: u64,
) -> CdtResult<()> {
    let terminal = accepted.saturating_add(hard_failed);
    if terminal > attempted {
        Err(CdtError::CheckpointResumeFailed {
            failure: CheckpointResumeFailure::MoveTerminalOutcomesExceedAttempted {
                move_type,
                terminal,
                attempted,
            },
        })
    } else {
        Ok(())
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
    stats: MoveStatistics,
    /// Random number generator
    rng: Xoshiro256PlusPlus,
    /// Authoritative cached local-site universes for proposal sampling.
    #[serde(skip)]
    site_cache: MoveSiteCache,
}

#[derive(Clone, Default)]
struct MoveSiteCache {
    move_22: MoveFamilySites,
    move_13_add: MoveFamilySites,
    move_31_remove: MoveFamilySites,
    edge_flip: MoveFamilySites,
}

/// Cached sampleable sites for one move family on one triangulation instance version.
///
/// The instance identity prevents reusing sites across distinct triangulation
/// values whose modification counters happen to match. The modification count
/// keeps ordinary self-loop proposal outcomes cheap while accepted mutations
/// force the next sample to rebuild stale local handles.
#[derive(Clone, Default)]
struct MoveFamilySites {
    instance_id: Option<u64>,
    modification_count: Option<u64>,
    sites: Vec<ProposalSite>,
    geometric_candidate_seen: bool,
}

impl MoveSiteCache {
    /// Rebuilds the selected move-family cache for a new triangulation version.
    fn ensure_current(&mut self, triangulation: &CdtTriangulation2D, move_type: MoveType) {
        let instance_id = triangulation.instance_id();
        let modification_count = triangulation.metadata().modification_count();
        let family = self.family(move_type);
        if family.instance_id == Some(instance_id)
            && family.modification_count == Some(modification_count)
        {
            return;
        }

        *self.family_mut(move_type) =
            Self::collect_family(triangulation, move_type, instance_id, modification_count);
    }

    /// Returns cached sites for one move family.
    const fn family(&self, move_type: MoveType) -> &MoveFamilySites {
        match move_type {
            MoveType::Move22 => &self.move_22,
            MoveType::Move13Add => &self.move_13_add,
            MoveType::Move31Remove => &self.move_31_remove,
            MoveType::EdgeFlip => &self.edge_flip,
        }
    }

    /// Returns mutable cached sites for one move family.
    const fn family_mut(&mut self, move_type: MoveType) -> &mut MoveFamilySites {
        match move_type {
            MoveType::Move22 => &mut self.move_22,
            MoveType::Move13Add => &mut self.move_13_add,
            MoveType::Move31Remove => &mut self.move_31_remove,
            MoveType::EdgeFlip => &mut self.edge_flip,
        }
    }

    /// Materializes every sampleable site for one move family.
    fn collect_family(
        triangulation: &CdtTriangulation2D,
        move_type: MoveType,
        instance_id: u64,
        modification_count: u64,
    ) -> MoveFamilySites {
        let mut sites = Vec::new();
        let geometric_candidate_seen =
            visit_proposal_sites(triangulation, move_type, |site| sites.push(site));
        MoveFamilySites {
            instance_id: Some(instance_id),
            modification_count: Some(modification_count),
            sites,
            geometric_candidate_seen,
        }
    }
}

/// Labeling instruction for a vertex inserted by a `(1,3)` proposal.
///
/// Unfoliated triangulations do not carry vertex time labels, while foliated
/// proposals must write the selected causal slice label before validation.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InsertionLabel {
    Unfoliated,
    Label(u32),
}

/// Concrete foliated `(1,3)` proposal realized as a spacelike-link split.
///
/// The candidate keeps both backend operations together: subdivide the chosen
/// face at `point`, label the new vertex, then flip the original spacelike edge.
#[derive(Clone, Debug)]
pub(crate) struct FoliatedInsertionCandidate {
    edge: DelaunayEdgeHandle,
    face: DelaunayFaceHandle,
    point: [f64; 2],
    label: u32,
}

/// Concrete foliated `(3,1)` proposal realized as flip-then-collapse.
///
/// The vertex cannot be collapsed directly in foliated CDT; the stored edge is
/// flipped first to expose the inverse local volume move.
#[derive(Clone, Debug)]
pub(crate) struct FoliatedRemovalCandidate {
    vertex: DelaunayVertexHandle,
    flip_edge: DelaunayEdgeHandle,
}

/// Concrete local site selected from a move-family proposal.
///
/// Metropolis planning samples one of these after counting the same site
/// universe used in the Hastings correction, so selection and denominator
/// semantics remain aligned.
#[derive(Clone, Debug)]
pub(crate) enum ProposalSite {
    EdgeFlip(DelaunayEdgeHandle),
    FaceSubdivision {
        face: DelaunayFaceHandle,
        point: [f64; 2],
        label: InsertionLabel,
    },
    FoliatedInsertion(FoliatedInsertionCandidate),
    VertexRemoval(DelaunayVertexHandle),
    FoliatedRemoval(FoliatedRemovalCandidate),
}

impl ProposalSite {
    /// Rebinds a sampled site to a cloned triangulation through stable entity IDs.
    ///
    /// Raw backend keys are owner-local. This explicit conversion is the only
    /// path by which a site sampled from the live chain may enter a speculative
    /// proposal owner.
    pub(crate) fn remap_for_clone(
        &self,
        source: &CdtTriangulation2D,
        target: &CdtTriangulation2D,
    ) -> Result<Self, crate::geometry::backends::delaunay::DelaunayError> {
        let source_geometry = source.geometry();
        let target_geometry = target.geometry();
        match self {
            Self::EdgeFlip(edge) => source_geometry
                .stable_edge_id(edge)
                .and_then(|id| target_geometry.resolve_edge_id(id))
                .map(Self::EdgeFlip),
            Self::FaceSubdivision { face, point, label } => source_geometry
                .stable_face_id(face)
                .and_then(|id| target_geometry.resolve_face_id(id))
                .map(|face| Self::FaceSubdivision {
                    face,
                    point: *point,
                    label: *label,
                }),
            Self::FoliatedInsertion(candidate) => {
                let edge_id = source_geometry.stable_edge_id(&candidate.edge)?;
                let face_id = source_geometry.stable_face_id(&candidate.face)?;
                Ok(Self::FoliatedInsertion(FoliatedInsertionCandidate {
                    edge: target_geometry.resolve_edge_id(edge_id)?,
                    face: target_geometry.resolve_face_id(face_id)?,
                    point: candidate.point,
                    label: candidate.label,
                }))
            }
            Self::VertexRemoval(vertex) => source_geometry
                .stable_vertex_id(vertex)
                .and_then(|id| target_geometry.resolve_vertex_id(id))
                .map(Self::VertexRemoval),
            Self::FoliatedRemoval(candidate) => {
                let vertex_id = source_geometry.stable_vertex_id(&candidate.vertex)?;
                let edge_id = source_geometry.stable_edge_id(&candidate.flip_edge)?;
                Ok(Self::FoliatedRemoval(FoliatedRemovalCandidate {
                    vertex: target_geometry.resolve_vertex_id(vertex_id)?,
                    flip_edge: target_geometry.resolve_edge_id(edge_id)?,
                }))
            }
        }
    }
}

/// Result of sampling a move family over its concrete local site universe.
///
/// `site_count` is the forward proposal denominator, `site` is the sampled site
/// when one exists, and `geometric_candidate_seen` distinguishes empty geometry
/// from causally rejected geometry for public move-result reporting.
pub(crate) struct ProposalSiteSelection {
    pub(crate) site_count: usize,
    pub(crate) site: Option<ProposalSite>,
    geometric_candidate_seen: bool,
}

impl ErgodicsSystem {
    /// Creates a new ergodics system.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    ///
    /// let system = ErgodicsSystem::new();
    /// assert_eq!(system.stats().attempted(MoveType::Move22), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: MoveStatistics::new(),
            rng: rand::make_rng(),
            site_cache: MoveSiteCache::default(),
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
            site_cache: MoveSiteCache::default(),
        }
    }

    /// Returns accumulated move statistics.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    ///
    /// let system = ErgodicsSystem::new();
    /// assert_eq!(system.stats().attempted(MoveType::Move22), 0);
    /// ```
    #[must_use]
    pub const fn stats(&self) -> &MoveStatistics {
        &self.stats
    }

    /// Returns the proposal RNG for CDT-owned checkpoint projection.
    pub(crate) const fn checkpoint_rng(&self) -> &Xoshiro256PlusPlus {
        &self.rng
    }

    /// Rebuilds the durable parts of an ergodic system after checkpoint hydration.
    pub(crate) fn from_checkpoint_parts(stats: MoveStatistics, rng: Xoshiro256PlusPlus) -> Self {
        Self {
            stats,
            rng,
            site_cache: MoveSiteCache::default(),
        }
    }

    /// Replaces move statistics after an internal speculative operation.
    ///
    /// This keeps public callers from mutating sampler accounting independently
    /// of actual move execution while allowing proposal planning to roll back
    /// temporary counters after applying a candidate to a cloned state.
    pub(crate) const fn replace_stats(&mut self, stats: MoveStatistics) -> MoveStatistics {
        std::mem::replace(&mut self.stats, stats)
    }

    /// Selects a random move type.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    /// use std::assert_matches;
    ///
    /// let mut system = ErgodicsSystem::new();
    /// let move_type = system.select_random_move();
    /// assert_matches!(
    ///     move_type,
    ///     MoveType::Move22 | MoveType::Move13Add | MoveType::Move31Remove | MoveType::EdgeFlip
    /// );
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

    /// Samples one family from a checked normalized policy distribution.
    ///
    /// The proposal RNG remains owned by the serialized ergodic-move system,
    /// keeping family and site selection reproducible across CDT checkpoints.
    pub(crate) fn select_move_family(
        &mut self,
        distribution: &CdtMoveFamilyDistribution,
    ) -> MoveType {
        let draw = self.rng.random::<u64>() >> (u64::BITS - 53);
        select_move_family_at(distribution, draw)
    }

    /// Returns an immutable proposal-policy view for one move family.
    ///
    /// The view borrows `triangulation` and this system's canonical versioned
    /// site cache. Cache synchronization may allocate when this family has not
    /// yet been inspected for the current state, but the returned view counts
    /// and iterates sites without further allocation or triangulation clones.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// # fn main() -> CdtResult<()> {
    /// let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut moves = ErgodicsSystem::new();
    /// let view = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
    /// assert_eq!(view.family(), MoveType::Move13Add);
    /// assert_eq!(view.reverse_family(), MoveType::Move31Remove);
    /// assert_eq!(view.offered_sites().len(), view.offered_site_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn proposal_policy_view<'a>(
        &'a mut self,
        triangulation: &'a CdtTriangulation2D,
        move_type: MoveType,
    ) -> CdtProposalPolicyView<'a> {
        self.site_cache.ensure_current(triangulation, move_type);
        CdtProposalPolicyView::new(
            triangulation,
            move_type,
            &self.site_cache.family(move_type).sites,
        )
    }

    /// Samples one concrete proposal site from the same cached universe used for proposal counts.
    ///
    /// The cache materializes the deterministic visitor output once per
    /// triangulation version, so counting and uniform sampling cannot drift
    /// apart for the Metropolis-Hastings proposal ratio.
    pub(crate) fn select_proposal_site(
        &mut self,
        triangulation: &CdtTriangulation2D,
        move_type: MoveType,
    ) -> ProposalSiteSelection {
        let site_count = self
            .proposal_policy_view(triangulation, move_type)
            .offered_site_count();
        let selected_site = if site_count == 0 {
            None
        } else {
            let index = self.rng.random_range(0..site_count);
            let view = self.proposal_policy_view(triangulation, move_type);
            view.site_at(index)
        };
        let geometric_candidate_seen = self.site_cache.family(move_type).geometric_candidate_seen;

        ProposalSiteSelection {
            site_count,
            site: selected_site,
            geometric_candidate_seen,
        }
    }

    /// Applies one previously selected proposal site.
    ///
    /// Matching the site variant against the move family keeps accidental
    /// cross-family application as a normal geometric rejection without touching
    /// backend state.
    pub(crate) fn apply_proposal_site(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
        site: ProposalSite,
    ) -> MoveResult {
        let snapshot = triangulation.clone();
        let result = self.apply_proposal_site_to_draft(triangulation, move_type, site);
        rollback_if_failed(triangulation, snapshot, result)
    }

    /// Applies one proposal inside a caller-owned transaction.
    ///
    /// The Metropolis adapter already applies proposals to a speculative clone,
    /// so it uses this path to avoid cloning that draft again. Direct move APIs
    /// use [`Self::apply_proposal_site`], which owns the rollback snapshot.
    pub(crate) fn apply_proposal_site_to_draft(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
        site: ProposalSite,
    ) -> MoveResult {
        match (move_type, site) {
            (MoveType::Move22 | MoveType::EdgeFlip, ProposalSite::EdgeFlip(edge)) => {
                self.apply_edge_flip_site(triangulation, move_type, &edge)
            }
            (MoveType::Move13Add, ProposalSite::FaceSubdivision { face, point, label }) => {
                self.apply_face_subdivision_site(triangulation, face, point, label)
            }
            (MoveType::Move13Add, ProposalSite::FoliatedInsertion(candidate)) => {
                self.apply_foliated_insertion_site(triangulation, candidate)
            }
            (MoveType::Move31Remove, ProposalSite::VertexRemoval(vertex)) => {
                self.apply_vertex_removal_site(triangulation, &vertex)
            }
            (MoveType::Move31Remove, ProposalSite::FoliatedRemoval(candidate)) => {
                self.apply_foliated_removal_site(triangulation, &candidate)
            }
            (
                MoveType::Move22 | MoveType::EdgeFlip,
                ProposalSite::FaceSubdivision { .. }
                | ProposalSite::FoliatedInsertion(_)
                | ProposalSite::VertexRemoval(_)
                | ProposalSite::FoliatedRemoval(_),
            )
            | (
                MoveType::Move13Add,
                ProposalSite::EdgeFlip(_)
                | ProposalSite::VertexRemoval(_)
                | ProposalSite::FoliatedRemoval(_),
            )
            | (
                MoveType::Move31Remove,
                ProposalSite::EdgeFlip(_)
                | ProposalSite::FaceSubdivision { .. }
                | ProposalSite::FoliatedInsertion(_),
            ) => MoveResult::GeometricViolation,
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
    /// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_from_simplices(
    ///         &[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1), ([1.0, 1.0], 1)],
    ///         &[vec![0, 1, 2], vec![1, 3, 2]],
    ///     )?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     let mut system = ErgodicsSystem::new();
    ///     let result = system.attempt_22_move(&mut triangulation);
    ///     assert_matches!(
    ///         result,
    ///         MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    ///     );
    ///     assert_eq!(system.stats().attempted(MoveType::Move22), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn attempt_22_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move22);
        let result = self.attempt_causal_edge_flip(triangulation, MoveType::Move22);
        self.record_hard_failure_if_needed(MoveType::Move22, result)
    }

    /// Attempts a (1,3) move on the triangulation.
    ///
    /// On foliated triangulations, the move is realized as a spacelike-link
    /// split: the kernel subdivides one adjacent face, labels the inserted
    /// vertex on the split link's time slice, then flips the original spacelike
    /// link away. Open-boundary kernels use interior slices; toroidal kernels
    /// wrap time. Unfoliated geometry experiments retain the bare triangle
    /// subdivision path.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     let mut system = ErgodicsSystem::new();
    ///     let result = system.attempt_13_move(&mut triangulation);
    ///     assert_matches!(result, MoveResult::Success | MoveResult::GeometricViolation);
    ///     assert_eq!(system.stats().attempted(MoveType::Move13Add), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn attempt_13_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move13Add);
        let result = self.attempt_13_move_mutating(triangulation);
        self.record_hard_failure_if_needed(MoveType::Move13Add, result)
    }

    /// Rollback boundary for an accepted (1,3) move application.
    ///
    /// Clones a snapshot, subdivides the selected face, applies the inserted
    /// vertex label, and finishes CDT bookkeeping. Once the snapshot exists,
    /// every early return must pass a [`MoveResult`] through
    /// [`rollback_if_failed`] or restore state itself.
    fn attempt_13_move_mutating(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        let selection = self.select_proposal_site(triangulation, MoveType::Move13Add);
        let Some(site) = selection.site else {
            return rejected_empty_selection(triangulation, &selection);
        };

        self.apply_proposal_site(triangulation, MoveType::Move13Add, site)
    }

    /// Attempts a (3,1) move on the triangulation.
    ///
    /// On foliated triangulations, this inverse volume move targets a degree-4
    /// local configuration produced by a spacelike-link split. The kernel flips
    /// a timelike support edge so the removable vertex becomes degree 3, then
    /// collapses it while preserving the topology-specific minimum slice size.
    /// Unfoliated geometry experiments retain the direct degree-3 collapse path.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     let mut system = ErgodicsSystem::new();
    ///     let _ = system.attempt_13_move(&mut triangulation);
    ///     let result = system.attempt_31_move(&mut triangulation);
    ///     assert_matches!(
    ///         result,
    ///         MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    ///     );
    ///     assert_eq!(system.stats().attempted(MoveType::Move31Remove), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn attempt_31_move(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::Move31Remove);
        let result = self.attempt_31_move_mutating(triangulation);
        self.record_hard_failure_if_needed(MoveType::Move31Remove, result)
    }

    /// Rollback boundary for an accepted (3,1) move application.
    ///
    /// Clones a snapshot, removes the selected vertex, and finishes CDT
    /// bookkeeping. Once the snapshot exists, every early return must pass a
    /// [`MoveResult`] through [`rollback_if_failed`] or restore state itself.
    fn attempt_31_move_mutating(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        let selection = self.select_proposal_site(triangulation, MoveType::Move31Remove);
        let Some(site) = selection.site else {
            return rejected_empty_selection(triangulation, &selection);
        };

        self.apply_proposal_site(triangulation, MoveType::Move31Remove, site)
    }

    /// Attempts an edge flip move on the triangulation.
    ///
    /// In 2D this is the same bistellar k=2 operation as [`Self::attempt_22_move`].
    /// The separate method is retained for API compatibility and records
    /// [`MoveType::EdgeFlip`] statistics.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_from_simplices(
    ///         &[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.0, 1.0], 1), ([1.0, 1.0], 1)],
    ///         &[vec![0, 1, 2], vec![1, 3, 2]],
    ///     )?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     let mut system = ErgodicsSystem::new();
    ///     let result = system.attempt_edge_flip(&mut triangulation);
    ///     assert_matches!(
    ///         result,
    ///         MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    ///     );
    ///     assert_eq!(system.stats().attempted(MoveType::EdgeFlip), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn attempt_edge_flip(&mut self, triangulation: &mut CdtTriangulation2D) -> MoveResult {
        self.stats.record_attempt(MoveType::EdgeFlip);
        let result = self.attempt_causal_edge_flip(triangulation, MoveType::EdgeFlip);
        self.record_hard_failure_if_needed(MoveType::EdgeFlip, result)
    }

    /// Attempts a random ergodic move on the triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::moves::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let mut triangulation = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     let mut system = ErgodicsSystem::new();
    ///     let result = system.attempt_random_move(&mut triangulation);
    ///     assert_matches!(
    ///         result,
    ///         MoveResult::Success | MoveResult::CausalityViolation | MoveResult::GeometricViolation
    ///     );
    ///     Ok(())
    /// }
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

    /// Applies the shared 2D k=2 edge-flip implementation for
    /// [`MoveType::Move22`] and [`MoveType::EdgeFlip`].
    fn attempt_causal_edge_flip(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
    ) -> MoveResult {
        let selection = self.select_proposal_site(triangulation, move_type);
        let Some(site) = selection.site else {
            return rejected_empty_selection(triangulation, &selection);
        };

        self.apply_proposal_site(triangulation, move_type, site)
    }

    /// Applies a selected edge-flip site inside the active transaction.
    ///
    /// This is shared by [`MoveType::Move22`] and [`MoveType::EdgeFlip`], which
    /// are public names for the same 2D k=2 bistellar flip kernel.
    fn apply_edge_flip_site(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
        edge: &DelaunayEdgeHandle,
    ) -> MoveResult {
        let adjacent = match triangulation.geometry().edge_adjacent_faces(edge) {
            Ok(Some(adjacent)) => adjacent,
            Ok(None) => return MoveResult::GeometricViolation,
            Err(err) => {
                return MoveResult::HardFailure(local_geometry_error("edge adjacency", &err));
            }
        };
        let mut baseline = match triangulation.begin_local_move() {
            Ok(baseline) => baseline,
            Err(err) => return MoveResult::HardFailure(err),
        };
        let removed_faces = [adjacent.faces.0, adjacent.faces.1];
        if let Err(err) = triangulation.record_locally_removed_faces(&mut baseline, &removed_faces)
        {
            return MoveResult::HardFailure(err);
        }
        let flip_result = triangulation.flip_edge_in_caller_transaction(edge);

        match flip_result {
            Ok(result) => self.finish_mutated_move(
                triangulation,
                move_type,
                baseline,
                &result.affected_faces,
                LocalMoveDelta::Flip,
            ),
            Err(err) => reject_backend(
                BackendMutationOperation::FlipEdge,
                format!("{edge:?}"),
                &err,
            ),
        }
    }

    /// Applies a selected unfoliated face subdivision site.
    ///
    /// The mutation is finalized only after optional vertex labeling,
    /// foliation synchronization, and evolved-CDT validation all succeed.
    fn apply_face_subdivision_site(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        face: DelaunayFaceHandle,
        point: [f64; 2],
        label: InsertionLabel,
    ) -> MoveResult {
        let face_key = face.simplex_key();
        let label_value = match label {
            InsertionLabel::Unfoliated => None,
            InsertionLabel::Label(label) => Some(label),
        };
        let mut baseline = match triangulation.begin_local_move() {
            Ok(baseline) => baseline,
            Err(err) => return MoveResult::HardFailure(err),
        };
        if let Err(err) =
            triangulation.record_locally_removed_faces(&mut baseline, std::slice::from_ref(&face))
        {
            return MoveResult::HardFailure(err);
        }
        let subdivision = triangulation.subdivide_face_in_caller_transaction(face, &point);

        let subdivision = match subdivision {
            Ok(subdivision) => subdivision,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::SubdivideFace,
                    format!("face {face_key:?}"),
                    &err,
                );
            }
        };

        if let InsertionLabel::Label(label) = label {
            let set_label = triangulation.set_vertex_data(&subdivision.new_vertex, Some(label));
            if let Err(err) = set_label {
                return reject_backend(
                    BackendMutationOperation::SetVertexData,
                    format!("vertex {:?}", subdivision.new_vertex.vertex_key()),
                    &err,
                );
            }
        }

        self.finish_mutated_move(
            triangulation,
            MoveType::Move13Add,
            baseline,
            &subdivision.new_faces,
            LocalMoveDelta::Insert { label: label_value },
        )
    }

    /// Applies a foliated volume-add move as a spacelike-link split.
    ///
    /// The backend only exposes primitive bistellar edits, so this composes a
    /// face subdivision with an immediate flip of the original spacelike link.
    /// The intermediate same-slice triangle is never finalized as CDT state.
    fn apply_foliated_insertion_site(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        candidate: FoliatedInsertionCandidate,
    ) -> MoveResult {
        let face_key = candidate.face.simplex_key();
        let mut baseline = match triangulation.begin_local_move() {
            Ok(baseline) => baseline,
            Err(err) => return MoveResult::HardFailure(err),
        };
        if let Err(err) = triangulation
            .record_locally_removed_faces(&mut baseline, std::slice::from_ref(&candidate.face))
        {
            return MoveResult::HardFailure(err);
        }
        let stable_edge = match triangulation.geometry().stable_edge_id(&candidate.edge) {
            Ok(stable_edge) => stable_edge,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::FlipEdge,
                    format!("{:?}", candidate.edge),
                    err,
                );
            }
        };
        let subdivision =
            triangulation.subdivide_face_in_caller_transaction(candidate.face, &candidate.point);
        let subdivision = match subdivision {
            Ok(subdivision) => subdivision,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::SubdivideFace,
                    format!("face {face_key:?}"),
                    &err,
                );
            }
        };

        if let Err(err) =
            triangulation.set_vertex_data(&subdivision.new_vertex, Some(candidate.label))
        {
            return reject_backend(
                BackendMutationOperation::SetVertexData,
                format!("vertex {:?}", subdivision.new_vertex.vertex_key()),
                &err,
            );
        }

        let subdivision_faces = match stable_face_ids(triangulation, &subdivision.new_faces) {
            Ok(faces) => faces,
            Err(err) => return MoveResult::HardFailure(err),
        };

        let edge = match triangulation.geometry().resolve_edge_id(stable_edge) {
            Ok(edge) => edge,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::FlipEdge,
                    format!("{:?}", candidate.edge),
                    err,
                );
            }
        };
        let adjacent = match triangulation.geometry().edge_adjacent_faces(&edge) {
            Ok(Some(adjacent)) => adjacent,
            Ok(None) => return MoveResult::GeometricViolation,
            Err(err) => {
                return MoveResult::HardFailure(local_geometry_error("edge adjacency", &err));
            }
        };
        let removed_faces = [adjacent.faces.0, adjacent.faces.1];
        if let Err(err) =
            triangulation.record_intermediate_removed_faces(&mut baseline, &removed_faces)
        {
            return MoveResult::HardFailure(err);
        }
        let flip_result = triangulation.flip_edge_in_caller_transaction(&edge);
        match flip_result {
            Ok(result) => {
                let mut affected_faces = surviving_faces(triangulation, &subdivision_faces);
                affected_faces.extend(result.affected_faces);
                self.finish_mutated_move(
                    triangulation,
                    MoveType::Move13Add,
                    baseline,
                    &affected_faces,
                    LocalMoveDelta::Insert {
                        label: Some(candidate.label),
                    },
                )
            }
            Err(err) => reject_backend(
                BackendMutationOperation::FlipEdge,
                format!("{:?}", candidate.edge),
                &err,
            ),
        }
    }

    /// Applies a selected unfoliated vertex-removal site.
    ///
    /// The candidate was already screened for causal and replacement-triangle
    /// preconditions; this function owns the backend edit, validation, and
    /// rollback boundary.
    fn apply_vertex_removal_site(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        vertex: &DelaunayVertexHandle,
    ) -> MoveResult {
        let label = triangulation
            .geometry()
            .vertex_data_by_key(vertex.vertex_key());
        let removed_faces = match triangulation.geometry().removal_affected_faces(vertex) {
            Ok(faces) => faces,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::RemoveVertex,
                    format!("vertex {:?}", vertex.vertex_key()),
                    &err,
                );
            }
        };
        let mut baseline = match triangulation.begin_local_move() {
            Ok(baseline) => baseline,
            Err(err) => return MoveResult::HardFailure(err),
        };
        if let Err(err) = triangulation.record_locally_removed_faces(&mut baseline, &removed_faces)
        {
            return MoveResult::HardFailure(err);
        }
        let removal = triangulation.remove_vertex_in_caller_transaction(vertex);

        match removal {
            Ok(result) => self.finish_mutated_move(
                triangulation,
                MoveType::Move31Remove,
                baseline,
                &result.new_faces,
                LocalMoveDelta::Remove { label },
            ),
            Err(err) => reject_backend(
                BackendMutationOperation::RemoveVertex,
                format!("vertex {:?}", vertex.vertex_key()),
                &err,
            ),
        }
    }

    /// Applies a foliated inverse volume move as flip-then-collapse.
    fn apply_foliated_removal_site(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        candidate: &FoliatedRemovalCandidate,
    ) -> MoveResult {
        let label = triangulation
            .geometry()
            .vertex_data_by_key(candidate.vertex.vertex_key());
        let adjacent = match triangulation
            .geometry()
            .edge_adjacent_faces(&candidate.flip_edge)
        {
            Ok(Some(adjacent)) => adjacent,
            Ok(None) => return MoveResult::GeometricViolation,
            Err(err) => {
                return MoveResult::HardFailure(local_geometry_error("edge adjacency", &err));
            }
        };
        let mut baseline = match triangulation.begin_local_move() {
            Ok(baseline) => baseline,
            Err(err) => return MoveResult::HardFailure(err),
        };
        let removed_faces = [adjacent.faces.0, adjacent.faces.1];
        if let Err(err) = triangulation.record_locally_removed_faces(&mut baseline, &removed_faces)
        {
            return MoveResult::HardFailure(err);
        }
        let stable_vertex = match triangulation.geometry().stable_vertex_id(&candidate.vertex) {
            Ok(stable_vertex) => stable_vertex,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::RemoveVertex,
                    format!("vertex {:?}", candidate.vertex.vertex_key()),
                    err,
                );
            }
        };
        let flip_result = triangulation.flip_edge_in_caller_transaction(&candidate.flip_edge);
        let flip_result = match flip_result {
            Ok(result) => result,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::FlipEdge,
                    format!("{:?}", candidate.flip_edge),
                    &err,
                );
            }
        };
        let flip_faces = match stable_face_ids(triangulation, &flip_result.affected_faces) {
            Ok(faces) => faces,
            Err(err) => return MoveResult::HardFailure(err),
        };

        let vertex = match triangulation.geometry().resolve_vertex_id(stable_vertex) {
            Ok(vertex) => vertex,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::RemoveVertex,
                    format!("vertex {:?}", candidate.vertex.vertex_key()),
                    err,
                );
            }
        };
        let removal_faces = match triangulation.geometry().removal_affected_faces(&vertex) {
            Ok(faces) => faces,
            Err(err) => {
                return reject_backend(
                    BackendMutationOperation::RemoveVertex,
                    format!("vertex {:?}", candidate.vertex.vertex_key()),
                    &err,
                );
            }
        };
        if let Err(err) =
            triangulation.record_intermediate_removed_faces(&mut baseline, &removal_faces)
        {
            return MoveResult::HardFailure(err);
        }
        let removal = triangulation.remove_vertex_in_caller_transaction(&vertex);
        match removal {
            Ok(result) => {
                let mut affected_faces = surviving_faces(triangulation, &flip_faces);
                affected_faces.extend(result.new_faces);
                self.finish_mutated_move(
                    triangulation,
                    MoveType::Move31Remove,
                    baseline,
                    &affected_faces,
                    LocalMoveDelta::Remove { label },
                )
            }
            Err(err) => reject_backend(
                BackendMutationOperation::RemoveVertex,
                format!("vertex {:?}", candidate.vertex.vertex_key()),
                &err,
            ),
        }
    }

    /// Completes a move after the backend mutation has already succeeded.
    fn finish_mutated_move(
        &mut self,
        triangulation: &mut CdtTriangulation2D,
        move_type: MoveType,
        baseline: LocalMoveBaseline,
        affected_faces: &[DelaunayFaceHandle],
        delta: LocalMoveDelta,
    ) -> MoveResult {
        if let Err(err) = triangulation.finish_local_move(baseline, affected_faces, delta) {
            if err.is_post_mutation_candidate_rejection() {
                return MoveResult::GeometricViolation;
            }
            return MoveResult::HardFailure(err);
        }

        self.stats.record_success(move_type);
        MoveResult::Success
    }

    /// Records hard-failure telemetry without treating it as acceptance.
    const fn record_hard_failure_if_needed(
        &mut self,
        move_type: MoveType,
        result: MoveResult,
    ) -> MoveResult {
        if matches!(result, MoveResult::HardFailure(_)) {
            self.stats.record_hard_failure(move_type);
        }
        result
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

/// Converts an empty sampled site set into the public move-result category.
///
/// Seeing geometric candidates but no sampleable foliated sites means causality
/// filtered every candidate; no geometric candidates means the move is
/// unavailable for topological/backend shape reasons.
fn rejected_empty_selection(
    triangulation: &CdtTriangulation2D,
    selection: &ProposalSiteSelection,
) -> MoveResult {
    if selection.geometric_candidate_seen && triangulation.has_foliation() {
        MoveResult::CausalityViolation
    } else {
        MoveResult::GeometricViolation
    }
}

/// Captures stable identity for faces that may survive a later primitive edit.
fn stable_face_ids(
    triangulation: &CdtTriangulation2D,
    faces: &[DelaunayFaceHandle],
) -> CdtResult<Vec<DelaunayFaceStableId>> {
    faces
        .iter()
        .map(|face| {
            triangulation
                .geometry()
                .stable_face_id(face)
                .map_err(|err| local_geometry_error("stable face identity", &err))
        })
        .collect()
}

/// Resolves only intermediate faces that remain live after a composite move.
fn surviving_faces(
    triangulation: &CdtTriangulation2D,
    stable_faces: &[DelaunayFaceStableId],
) -> Vec<DelaunayFaceHandle> {
    stable_faces
        .iter()
        .filter_map(|&face| triangulation.geometry().resolve_live_face_id(face))
        .collect()
}

/// Converts an unexpected local geometry query failure into typed CDT validation context.
fn local_geometry_error(context: &str, err: impl Display) -> CdtError {
    CdtError::ValidationFailed {
        check: CdtValidationCheck::Geometry,
        failure: CdtValidationFailure::BackendGeometry {
            detail: format!("{context} query failed: {err}"),
        },
    }
}

/// Converts an unexpected backend edit error into the move-level rejection shape.
///
/// Candidate selection should screen out ordinary geometric and causal
/// rejections before mutation. Reaching this helper means the backend refused
/// a selected site or returned an operation-specific error that should remain
/// visible to callers.
fn reject_backend(
    operation: BackendMutationOperation,
    target: String,
    err: impl Display,
) -> MoveResult {
    MoveResult::Rejected(CdtError::BackendMutationFailed {
        operation,
        target,
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
    if matches!(triangulation.metadata().topology(), CdtTopology::Toroidal) {
        let total = triangulation.time_slices().get();
        if t0 < total && t1 < total {
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

/// Checks whether a k=2 edge flip passes the cheap deterministic edit guards.
fn edge_flip_candidate_is_sampleable(
    triangulation: &CdtTriangulation2D,
    edge: &DelaunayEdgeHandle,
    adjacent: &EdgeAdjacentFaces<DelaunayVertexHandle, DelaunayFaceHandle>,
) -> bool {
    let (opposite_0, opposite_1) = &adjacent.opposite_vertices;
    triangulation.geometry().can_flip_edge(edge)
        && flip_is_causal(triangulation, adjacent)
        && !edge_exists_between(triangulation, opposite_0, opposite_1)
}

/// Finds the inserted vertex label that makes a `(1,3)` subdivision causal.
///
/// The candidate label must keep all three replacement triangles valid CDT
/// triangles; unfoliated triangulations return a marker that skips labeling.
/// Toroidal triangulations use a separate spacelike-link split path, because a
/// bare face subdivision is not compatible with the closed spatial slice
/// invariant.
fn insertion_label(
    triangulation: &CdtTriangulation2D,
    face: &DelaunayFaceHandle,
) -> Option<InsertionLabel> {
    if !triangulation.has_foliation() {
        return Some(InsertionLabel::Unfoliated);
    }

    causal_insertion_label(triangulation, face)
}

/// Checks exact backend preconditions for face subdivision.
fn insertion_candidate_is_sampleable(
    triangulation: &CdtTriangulation2D,
    face: &DelaunayFaceHandle,
    point: &[f64; 2],
) -> bool {
    triangulation.geometry().can_subdivide_face(face, point)
}

/// Returns the two time slices adjacent to an interior spatial slice.
///
/// Toroidal time wraps at both ends. Open time admits volume-changing link
/// splits only on interior slices because the move has the standard
/// `(+1, +3, +2)` simplex-count delta only when two adjacent triangles exist.
const fn neighboring_time_labels(
    triangulation: &CdtTriangulation2D,
    label: u32,
) -> Option<(u32, u32)> {
    let total = triangulation.time_slices().get();
    if total < 3 || label >= total {
        return None;
    }
    match triangulation.metadata().topology() {
        CdtTopology::Toroidal => {
            let previous = if label == 0 { total - 1 } else { label - 1 };
            Some((previous, (label + 1) % total))
        }
        CdtTopology::OpenBoundary => {
            if label > 0 && label + 1 < total {
                Some((label - 1, label + 1))
            } else {
                None
            }
        }
    }
}

/// Checks whether two labels bracket one spatial slice in configured time.
const fn labels_bracket_slice(
    triangulation: &CdtTriangulation2D,
    base: u32,
    first: u32,
    second: u32,
) -> bool {
    let Some((previous, next)) = neighboring_time_labels(triangulation, base) else {
        return false;
    };
    (first == previous && second == next) || (first == next && second == previous)
}

/// Selects a valid foliated `(1,3)` candidate around one spacelike link.
fn foliated_insertion_candidate(
    triangulation: &CdtTriangulation2D,
    edge: DelaunayEdgeHandle,
    adjacent: &EdgeAdjacentFaces<DelaunayVertexHandle, DelaunayFaceHandle>,
) -> Option<FoliatedInsertionCandidate> {
    let (endpoint_0, endpoint_1) = &adjacent.endpoints;
    let endpoint_0_label = triangulation
        .geometry()
        .vertex_data_by_key(endpoint_0.vertex_key())?;
    let endpoint_1_label = triangulation
        .geometry()
        .vertex_data_by_key(endpoint_1.vertex_key())?;
    if endpoint_0_label != endpoint_1_label {
        return None;
    }

    let (opposite_0, opposite_1) = &adjacent.opposite_vertices;
    let opposite_0_label = triangulation
        .geometry()
        .vertex_data_by_key(opposite_0.vertex_key())?;
    let opposite_1_label = triangulation
        .geometry()
        .vertex_data_by_key(opposite_1.vertex_key())?;
    if !labels_bracket_slice(
        triangulation,
        endpoint_0_label,
        opposite_0_label,
        opposite_1_label,
    ) {
        return None;
    }
    if !cdt_vertex_triple(triangulation, [endpoint_0, endpoint_1, opposite_0])
        || !cdt_vertex_triple(triangulation, [endpoint_0, endpoint_1, opposite_1])
    {
        return None;
    }

    let face = adjacent.faces.0.clone();
    let point = triangulation.geometry().face_barycenter(&face).ok()?;
    Some(FoliatedInsertionCandidate {
        edge,
        face,
        point,
        label: endpoint_0_label,
    })
}

/// Checks exact backend preconditions for the first foliated insertion edit.
fn foliated_insertion_candidate_is_sampleable(
    triangulation: &CdtTriangulation2D,
    candidate: &FoliatedInsertionCandidate,
) -> bool {
    triangulation
        .geometry()
        .can_subdivide_face(&candidate.face, &candidate.point)
}

/// Finds a CDT-valid inserted vertex label without topology-specific guards.
///
/// This keeps the raw causality check reusable for tests that intentionally
/// build a local subdivision fixture before exercising the inverse `(3,1)`
/// move, while production `(1,3)` proposals apply topology guards first.
fn causal_insertion_label(
    triangulation: &CdtTriangulation2D,
    face: &DelaunayFaceHandle,
) -> Option<InsertionLabel> {
    let vertices = triangulation.geometry().face_vertices(face).ok()?;
    let [v0, v1, v2] = exactly_three(vertices)?;
    let [t0, t1, t2] = vertex_labels3(triangulation, [&v0, &v1, &v2])?;

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

/// Returns the other endpoint of an edge if `vertex` is incident to it.
fn other_endpoint(
    triangulation: &CdtTriangulation2D,
    edge: &DelaunayEdgeHandle,
    vertex: &DelaunayVertexHandle,
) -> Option<DelaunayVertexHandle> {
    let (first, second) = triangulation.geometry().edge_endpoints(edge).ok()?;
    if &first == vertex {
        Some(second)
    } else if &second == vertex {
        Some(first)
    } else {
        None
    }
}

/// Returns true when a live edge connects the two vertices.
fn edge_exists_between(
    triangulation: &CdtTriangulation2D,
    first: &DelaunayVertexHandle,
    second: &DelaunayVertexHandle,
) -> bool {
    triangulation
        .geometry()
        .incident_edges(first)
        .is_ok_and(|edges| {
            edges.into_iter().any(|edge| {
                triangulation
                    .geometry()
                    .edge_endpoints(&edge)
                    .is_ok_and(|(left, right)| &left == second || &right == second)
            })
        })
}

/// Order-independent key for one triangular face's vertices.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FaceVertexSet([DelaunayVertexHandle; 3]);

impl FaceVertexSet {
    /// Canonicalizes one triangular face by ascending backend vertex key.
    fn new(mut vertices: [DelaunayVertexHandle; 3]) -> Self {
        vertices.sort_unstable_by_key(DelaunayVertexHandle::vertex_key);
        Self(vertices)
    }
}

/// Builds one canonical face-membership index for a removal-family rebuild.
fn face_vertex_index(triangulation: &CdtTriangulation2D) -> HashSet<FaceVertexSet> {
    triangulation
        .geometry()
        .faces()
        .filter_map(|face| {
            triangulation
                .geometry()
                .face_vertices(&face)
                .ok()
                .and_then(exactly_three)
                .map(FaceVertexSet::new)
        })
        .collect()
}

/// Returns true when the indexed live complex contains the three vertices.
fn face_exists_with_vertices_in(
    faces: &HashSet<FaceVertexSet>,
    vertices: &[DelaunayVertexHandle; 3],
) -> bool {
    faces.contains(&FaceVertexSet::new(vertices.clone()))
}

/// Returns true when a live face already spans exactly the three vertices.
#[cfg(test)]
fn face_exists_with_vertices(
    triangulation: &CdtTriangulation2D,
    vertices: &[DelaunayVertexHandle; 3],
) -> bool {
    face_exists_with_vertices_in(&face_vertex_index(triangulation), vertices)
}

/// Checks whether two opposite vertices match an unordered pair.
fn opposites_match_pair(
    adjacent: &EdgeAdjacentFaces<DelaunayVertexHandle, DelaunayFaceHandle>,
    first: &DelaunayVertexHandle,
    second: &DelaunayVertexHandle,
) -> bool {
    let (opposite_0, opposite_1) = &adjacent.opposite_vertices;
    (opposite_0 == first && opposite_1 == second) || (opposite_0 == second && opposite_1 == first)
}

/// Selects the at most two foliated inverse `(3,1)` candidates at one vertex.
fn foliated_removal_candidates(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
) -> Option<[Option<FoliatedRemovalCandidate>; 2]> {
    let label = triangulation
        .geometry()
        .vertex_data_by_key(vertex.vertex_key())?;
    let slice = usize::try_from(label).ok()?;
    let minimum_slice_size = match triangulation.metadata().topology() {
        CdtTopology::Toroidal => 3,
        CdtTopology::OpenBoundary => 2,
    };
    if triangulation
        .slice_sizes()
        .get(slice)
        .is_none_or(|&count| count <= minimum_slice_size)
    {
        return None;
    }

    let mut incident_edges = triangulation.geometry().incident_edges(vertex).ok()?;
    let incident_edge_array = [
        incident_edges.next()?,
        incident_edges.next()?,
        incident_edges.next()?,
        incident_edges.next()?,
    ];
    if incident_edges.next().is_some() {
        return None;
    }
    let incident_edges = incident_edge_array;

    let mut spacelike_neighbors: [Option<DelaunayVertexHandle>; 2] = array::from_fn(|_| None);
    let mut timelike_neighbors: [Option<(DelaunayVertexHandle, DelaunayEdgeHandle, u32)>; 2] =
        array::from_fn(|_| None);
    let mut spacelike_count = 0;
    let mut timelike_count = 0;
    for edge in incident_edges {
        let neighbor = other_endpoint(triangulation, &edge, vertex)?;
        let neighbor_label = triangulation
            .geometry()
            .vertex_data_by_key(neighbor.vertex_key())?;
        match time_dist(triangulation, label, neighbor_label) {
            0 => {
                let slot = spacelike_neighbors.get_mut(spacelike_count)?;
                *slot = Some(neighbor);
                spacelike_count += 1;
            }
            1 => {
                let slot = timelike_neighbors.get_mut(timelike_count)?;
                *slot = Some((neighbor, edge, neighbor_label));
                timelike_count += 1;
            }
            _ => return None,
        }
    }

    let [Some(space_0), Some(space_1)] = spacelike_neighbors else {
        return None;
    };
    let [
        Some((_, time_edge_0, time_label_0)),
        Some((_, time_edge_1, time_label_1)),
    ] = timelike_neighbors
    else {
        return None;
    };
    if !labels_bracket_slice(triangulation, label, time_label_0, time_label_1) {
        return None;
    }
    if edge_exists_between(triangulation, &space_0, &space_1) {
        return None;
    }

    Some([time_edge_0, time_edge_1].map(|edge| {
        let Ok(Some(adjacent)) = triangulation.geometry().edge_adjacent_faces(&edge) else {
            return None;
        };
        opposites_match_pair(&adjacent, &space_0, &space_1).then(|| FoliatedRemovalCandidate {
            vertex: vertex.clone(),
            flip_edge: edge,
        })
    }))
}

/// Checks whether collapsing a degree-3 vertex would duplicate an existing face.
#[cfg(test)]
fn removal_candidate_is_sampleable(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
    neighbors: &[DelaunayVertexHandle; 3],
) -> bool {
    !face_exists_with_vertices(triangulation, neighbors)
        && triangulation.geometry().can_collapse_vertex(vertex)
}

/// Indexed variant used while rebuilding the complete removal-family site set.
fn removal_candidate_is_sampleable_in(
    triangulation: &CdtTriangulation2D,
    faces: &HashSet<FaceVertexSet>,
    vertex: &DelaunayVertexHandle,
    neighbors: &[DelaunayVertexHandle; 3],
) -> bool {
    !face_exists_with_vertices_in(faces, neighbors)
        && triangulation.geometry().can_collapse_vertex(vertex)
}

/// Checks backend-local preconditions for the foliated flip-then-collapse move.
fn foliated_removal_candidate_is_sampleable(
    triangulation: &CdtTriangulation2D,
    candidate: &FoliatedRemovalCandidate,
) -> bool {
    triangulation.geometry().can_flip_edge(&candidate.flip_edge)
}

/// Collects the three distinct neighboring vertices around a removable vertex.
///
/// A `(3,1)` move is geometrically available only at a degree-3 vertex whose
/// adjacent faces collapse back to one replacement triangle.
fn neighbors3(
    triangulation: &CdtTriangulation2D,
    vertex: &DelaunayVertexHandle,
) -> Option<[DelaunayVertexHandle; 3]> {
    let mut adjacent_faces = triangulation.geometry().adjacent_faces(vertex).ok()?;
    // `adjacent_faces` must return exactly three faces, and `face_vertices`
    // should contribute one distinct non-self neighbor from each face. The
    // slots, count, self-skip, and dedup checks enforce that degree-3 contract.
    let adjacent_face_array = [
        adjacent_faces.next()?,
        adjacent_faces.next()?,
        adjacent_faces.next()?,
    ];
    if adjacent_faces.next().is_some() {
        return None;
    }
    let adjacent_faces = adjacent_face_array;

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

/// Counts concrete local sites that can realize `move_type` from `triangulation`.
///
/// The Metropolis-Hastings proposal ratio uses this to account for asymmetric
/// forward and reverse site multiplicities for volume-changing CDT moves.
/// Counts include only sites that pass the same deterministic pre-mutation
/// guards as the mutating executor.
#[cfg(test)]
pub(crate) fn proposal_site_count(
    triangulation: &CdtTriangulation2D,
    move_type: MoveType,
) -> usize {
    let mut cache = MoveSiteCache::default();
    cache.ensure_current(triangulation, move_type);
    CdtProposalPolicyView::new(triangulation, move_type, &cache.family(move_type).sites)
        .offered_site_count()
}

/// Visits every sampleable proposal site for one move family.
///
/// The return value records whether any coarse geometric candidate existed,
/// even if no candidate survived causal or backend-local screening.
fn visit_proposal_sites(
    triangulation: &CdtTriangulation2D,
    move_type: MoveType,
    mut visit: impl FnMut(ProposalSite),
) -> bool {
    match move_type {
        MoveType::Move22 | MoveType::EdgeFlip => edge_flip_sites(triangulation, &mut visit),
        MoveType::Move13Add => insertion_sites(triangulation, &mut visit),
        MoveType::Move31Remove => removal_sites(triangulation, &mut visit),
    }
}

/// Visits sampleable k=2 edge-flip sites.
///
/// Each visited edge has two adjacent faces, passes the causal flip guard, and
/// avoids creating a duplicate edge between opposite vertices.
fn edge_flip_sites(
    triangulation: &CdtTriangulation2D,
    visit: &mut impl FnMut(ProposalSite),
) -> bool {
    let geometry = triangulation.geometry();
    let mut geometric_candidate_seen = false;
    for edge in geometry.edges() {
        let Ok(Some(adjacent)) = geometry.edge_adjacent_faces(&edge) else {
            continue;
        };
        geometric_candidate_seen = true;
        if edge_flip_candidate_is_sampleable(triangulation, &edge, &adjacent) {
            visit(ProposalSite::EdgeFlip(edge));
        }
    }
    geometric_candidate_seen
}

/// Visits sampleable `(1,3)` insertion sites for the triangulation topology.
///
/// Foliated triangulations use the spacelike-link split visitor; ordinary face
/// subdivision is reserved for unfoliated states.
fn insertion_sites(
    triangulation: &CdtTriangulation2D,
    visit: &mut impl FnMut(ProposalSite),
) -> bool {
    if triangulation.has_foliation() {
        return foliated_insertion_sites(triangulation, visit);
    }

    let mut geometric_candidate_seen = false;
    for face in triangulation.geometry().faces() {
        let Ok(point) = triangulation.geometry().face_barycenter(&face) else {
            continue;
        };
        geometric_candidate_seen = true;
        let Some(label) = insertion_label(triangulation, &face) else {
            continue;
        };

        if insertion_candidate_is_sampleable(triangulation, &face, &point) {
            visit(ProposalSite::FaceSubdivision { face, point, label });
        }
    }
    geometric_candidate_seen
}

/// Visits sampleable foliated `(1,3)` spacelike-link split sites.
///
/// The visitor only receives candidates whose adjacent faces identify a
/// same-slice edge with neighboring time labels and a finite insertion point.
fn foliated_insertion_sites(
    triangulation: &CdtTriangulation2D,
    visit: &mut impl FnMut(ProposalSite),
) -> bool {
    let geometry = triangulation.geometry();
    let mut geometric_candidate_seen = false;
    for edge in geometry.edges() {
        let Ok(Some(adjacent)) = geometry.edge_adjacent_faces(&edge) else {
            continue;
        };
        geometric_candidate_seen = true;
        let Some(insert) = foliated_insertion_candidate(triangulation, edge, &adjacent) else {
            continue;
        };
        if foliated_insertion_candidate_is_sampleable(triangulation, &insert) {
            visit(ProposalSite::FoliatedInsertion(insert));
        }
    }
    geometric_candidate_seen
}

/// Visits sampleable `(3,1)` removal sites for the triangulation topology.
///
/// Foliated states use flip-then-collapse candidates; unfoliated states use
/// direct degree-3 vertex collapses.
fn removal_sites(triangulation: &CdtTriangulation2D, visit: &mut impl FnMut(ProposalSite)) -> bool {
    if triangulation.has_foliation() {
        let mut geometric_candidate_seen = false;
        for vertex in triangulation.geometry().vertices() {
            let Some(removals) = foliated_removal_candidates(triangulation, &vertex) else {
                continue;
            };
            geometric_candidate_seen = true;
            for remove in removals.into_iter().flatten() {
                if foliated_removal_candidate_is_sampleable(triangulation, &remove) {
                    visit(ProposalSite::FoliatedRemoval(remove));
                }
            }
        }
        return geometric_candidate_seen;
    }

    let mut faces = None;
    let mut geometric_candidate_seen = false;
    for vertex in triangulation.geometry().vertices() {
        let Some(neighbors) = neighbors3(triangulation, &vertex) else {
            continue;
        };
        geometric_candidate_seen = true;
        if removal_candidate_is_causal(triangulation, &vertex, &neighbors) {
            let faces = faces.get_or_insert_with(|| face_vertex_index(triangulation));
            if removal_candidate_is_sampleable_in(triangulation, faces, &vertex, &neighbors) {
                visit(ProposalSite::VertexRemoval(vertex));
            }
        }
    }
    geometric_candidate_seen
}

/// Maps one 53-bit categorical draw to the distribution's exact integer masses.
///
/// [`CdtMoveFamilyDistribution`] guarantees that the four masses partition the
/// complete draw range, so the final branch is an internal invariant fallback
/// rather than unreported probability assigned to one family.
fn select_move_family_at(distribution: &CdtMoveFamilyDistribution, draw: u64) -> MoveType {
    debug_assert!(draw < (1_u64 << 53));
    let mut cumulative = 0_u64;
    for family in MoveType::REVERSIBLE_1P1 {
        cumulative += distribution.sample_mass(family);
        if draw < cumulative {
            return family;
        }
    }

    debug_assert_eq!(cumulative, 1_u64 << 53);
    MoveType::EdgeFlip
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::action::ActionConfig;
    use crate::cdt::foliation::FoliationError;
    use crate::errors::{CdtValidationCheck, CdtValidationFailure, DelaunayValidationLevel};
    use crate::geometry::DelaunayBackend2D;
    use crate::geometry::backends::delaunay::DelaunayError;
    use crate::geometry::generators::{build_delaunay2_from_simplices, build_delaunay2_with_data};
    use approx::assert_relative_eq;
    use std::assert_matches;
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
        let dt = build_delaunay2_from_simplices(
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

    type CanonicalVertexSignature = (Option<u32>, Vec<u64>);

    #[derive(Debug, PartialEq, Eq)]
    struct CanonicalTriangulationSignature {
        vertices: Vec<CanonicalVertexSignature>,
        faces: Vec<Vec<CanonicalVertexSignature>>,
        slice_sizes: Vec<usize>,
        slab_triangle_profile: Vec<u32>,
    }

    /// Captures a handle-independent exact signature of one test triangulation.
    fn canonical_signature(triangulation: &CdtTriangulation2D) -> CanonicalTriangulationSignature {
        let geometry = triangulation.geometry();
        let vertex_signature = |vertex: &DelaunayVertexHandle| {
            let coordinates = geometry
                .vertex_coordinates(vertex)
                .expect("test vertex coordinates should resolve")
                .iter()
                .copied()
                .map(f64::to_bits)
                .collect();
            (
                geometry.vertex_data_by_key(vertex.vertex_key()),
                coordinates,
            )
        };

        let mut vertices = geometry
            .vertices()
            .map(|vertex| vertex_signature(&vertex))
            .collect::<Vec<_>>();
        vertices.sort();

        let mut faces = geometry
            .faces()
            .map(|face| {
                let mut vertices = geometry
                    .face_vertices(&face)
                    .expect("test face vertices should resolve")
                    .map(|vertex| vertex_signature(&vertex))
                    .collect::<Vec<_>>();
                vertices.sort();
                vertices
            })
            .collect::<Vec<_>>();
        faces.sort();

        CanonicalTriangulationSignature {
            vertices,
            faces,
            slice_sizes: triangulation.slice_sizes().to_vec(),
            slab_triangle_profile: triangulation
                .slab_triangle_profile()
                .expect("test slab profile should be valid"),
        }
    }

    /// Counts spacelike-link insertion sites without using the production visitor or cache.
    fn independently_count_foliated_insertions(triangulation: &CdtTriangulation2D) -> usize {
        let total_slices = triangulation.time_slices().get();
        triangulation
            .geometry()
            .edges()
            .filter(|edge| {
                let Ok(Some(adjacent)) = triangulation.geometry().edge_adjacent_faces(edge) else {
                    return false;
                };
                let (endpoint_0, endpoint_1) = &adjacent.endpoints;
                let Some(endpoint_0_label) = triangulation
                    .geometry()
                    .vertex_data_by_key(endpoint_0.vertex_key())
                else {
                    return false;
                };
                let Some(endpoint_1_label) = triangulation
                    .geometry()
                    .vertex_data_by_key(endpoint_1.vertex_key())
                else {
                    return false;
                };
                if endpoint_0_label != endpoint_1_label {
                    return false;
                }

                let (opposite_0, opposite_1) = &adjacent.opposite_vertices;
                let Some(opposite_0_label) = triangulation
                    .geometry()
                    .vertex_data_by_key(opposite_0.vertex_key())
                else {
                    return false;
                };
                let Some(opposite_1_label) = triangulation
                    .geometry()
                    .vertex_data_by_key(opposite_1.vertex_key())
                else {
                    return false;
                };
                let bracketed = match triangulation.metadata().topology() {
                    CdtTopology::Toroidal if total_slices >= 3 => {
                        let previous = if endpoint_0_label == 0 {
                            total_slices - 1
                        } else {
                            endpoint_0_label - 1
                        };
                        let next = (endpoint_0_label + 1) % total_slices;
                        (opposite_0_label == previous && opposite_1_label == next)
                            || (opposite_0_label == next && opposite_1_label == previous)
                    }
                    CdtTopology::OpenBoundary
                        if endpoint_0_label > 0 && endpoint_0_label + 1 < total_slices =>
                    {
                        let previous = endpoint_0_label - 1;
                        let next = endpoint_0_label + 1;
                        (opposite_0_label == previous && opposite_1_label == next)
                            || (opposite_0_label == next && opposite_1_label == previous)
                    }
                    CdtTopology::Toroidal | CdtTopology::OpenBoundary => false,
                };
                if !bracketed {
                    return false;
                }

                let face = &adjacent.faces.0;
                triangulation
                    .geometry()
                    .face_barycenter(face)
                    .is_ok_and(|point| triangulation.geometry().can_subdivide_face(face, &point))
            })
            .count()
    }

    /// Counts inverse link-split sites without using the production visitor or cache.
    #[expect(
        clippy::too_many_lines,
        reason = "the independent test oracle spells out every structural and backend precondition without sharing production helpers"
    )]
    fn independently_count_foliated_removals(triangulation: &CdtTriangulation2D) -> usize {
        let total_slices = triangulation.time_slices().get();
        let time_distance = |first: u32, second: u32| {
            let raw = first.abs_diff(second);
            match triangulation.metadata().topology() {
                CdtTopology::Toroidal => raw.min(total_slices - raw),
                CdtTopology::OpenBoundary => raw,
            }
        };

        triangulation
            .geometry()
            .vertices()
            .map(|vertex| {
                let Some(label) = triangulation
                    .geometry()
                    .vertex_data_by_key(vertex.vertex_key())
                else {
                    return 0;
                };
                if label >= total_slices {
                    return 0;
                }
                let Ok(slice) = usize::try_from(label) else {
                    return 0;
                };
                let minimum_slice_size = match triangulation.metadata().topology() {
                    CdtTopology::Toroidal => 3,
                    CdtTopology::OpenBoundary => 2,
                };
                if triangulation
                    .slice_sizes()
                    .get(slice)
                    .is_none_or(|&size| size <= minimum_slice_size)
                {
                    return 0;
                }

                let Ok(incident_edges) = triangulation.geometry().incident_edges(&vertex) else {
                    return 0;
                };
                let incident_edges = incident_edges.collect::<Vec<_>>();
                if incident_edges.len() != 4 {
                    return 0;
                }

                let mut spacelike = Vec::new();
                let mut timelike = Vec::new();
                for edge in incident_edges {
                    let Ok((first, second)) = triangulation.geometry().edge_endpoints(&edge) else {
                        return 0;
                    };
                    let neighbor = if first == vertex {
                        second
                    } else if second == vertex {
                        first
                    } else {
                        return 0;
                    };
                    let Some(neighbor_label) = triangulation
                        .geometry()
                        .vertex_data_by_key(neighbor.vertex_key())
                    else {
                        return 0;
                    };
                    if neighbor_label >= total_slices {
                        return 0;
                    }
                    match time_distance(label, neighbor_label) {
                        0 => spacelike.push(neighbor),
                        1 => timelike.push((edge, neighbor_label)),
                        _ => return 0,
                    }
                }
                let [space_0, space_1] = spacelike.as_slice() else {
                    return 0;
                };
                let [(time_edge_0, time_label_0), (time_edge_1, time_label_1)] =
                    timelike.as_slice()
                else {
                    return 0;
                };

                let bracketed = match triangulation.metadata().topology() {
                    CdtTopology::Toroidal => {
                        let previous = if label == 0 {
                            total_slices - 1
                        } else {
                            label - 1
                        };
                        let next = (label + 1) % total_slices;
                        (*time_label_0 == previous && *time_label_1 == next)
                            || (*time_label_0 == next && *time_label_1 == previous)
                    }
                    CdtTopology::OpenBoundary => {
                        label > 0
                            && label + 1 < total_slices
                            && ((*time_label_0 == label - 1 && *time_label_1 == label + 1)
                                || (*time_label_0 == label + 1 && *time_label_1 == label - 1))
                    }
                };
                if !bracketed {
                    return 0;
                }

                let space_neighbors_already_connected = triangulation
                    .geometry()
                    .incident_edges(space_0)
                    .is_ok_and(|edges| {
                        edges.into_iter().any(|edge| {
                            triangulation.geometry().edge_endpoints(&edge).is_ok_and(
                                |(first, second)| &first == space_1 || &second == space_1,
                            )
                        })
                    });
                if space_neighbors_already_connected {
                    return 0;
                }

                [time_edge_0, time_edge_1]
                    .into_iter()
                    .filter(|edge| {
                        let Ok(Some(adjacent)) = triangulation.geometry().edge_adjacent_faces(edge)
                        else {
                            return false;
                        };
                        let (opposite_0, opposite_1) = &adjacent.opposite_vertices;
                        let opposites_are_space_neighbors = (opposite_0 == space_0
                            && opposite_1 == space_1)
                            || (opposite_0 == space_1 && opposite_1 == space_0);
                        opposites_are_space_neighbors
                            && triangulation.geometry().can_flip_edge(edge)
                    })
                    .count()
            })
            .sum()
    }

    /// Checks the exact immutable backend preflight used by one proposal site.
    fn counted_site_is_backend_feasible(
        triangulation: &CdtTriangulation2D,
        site: &ProposalSite,
    ) -> bool {
        match site {
            ProposalSite::EdgeFlip(edge) => triangulation.geometry().can_flip_edge(edge),
            ProposalSite::FaceSubdivision { face, point, .. } => {
                triangulation.geometry().can_subdivide_face(face, point)
            }
            ProposalSite::FoliatedInsertion(insert) => triangulation
                .geometry()
                .can_subdivide_face(&insert.face, &insert.point),
            ProposalSite::VertexRemoval(vertex) => {
                triangulation.geometry().can_collapse_vertex(vertex)
            }
            ProposalSite::FoliatedRemoval(remove) => {
                triangulation.geometry().can_flip_edge(&remove.flip_edge)
            }
        }
    }

    /// Checks count/sampling identity and immutable backend feasibility on one fixture.
    fn assert_counted_sites_match_backend_feasibility(triangulation: &CdtTriangulation2D) {
        let mut system = ErgodicsSystem::with_seed(0x146);
        let mut total_sites = 0;
        for move_type in [
            MoveType::Move22,
            MoveType::Move13Add,
            MoveType::Move31Remove,
            MoveType::EdgeFlip,
        ] {
            let family = MoveSiteCache::collect_family(
                triangulation,
                move_type,
                triangulation.instance_id(),
                triangulation.metadata().modification_count(),
            );
            let counted = proposal_site_count(triangulation, move_type);
            let view = system.proposal_policy_view(triangulation, move_type);
            let viewed = view.offered_site_count();
            let viewed_ids = view.offered_sites().collect::<Vec<_>>();
            for (ordinal, site) in viewed_ids.iter().enumerate() {
                assert_eq!(site.family(), move_type);
                assert_eq!(site.ordinal(), ordinal);
                view.validate_site(*site)
                    .expect("fresh policy-view site should validate");
            }
            let sampled = system.select_proposal_site(triangulation, move_type);

            assert_eq!(counted, family.sites.len(), "count drift for {move_type:?}");
            assert_eq!(viewed, counted, "policy-view drift for {move_type:?}");
            assert_eq!(
                viewed_ids.len(),
                counted,
                "policy-view enumeration drift for {move_type:?}"
            );
            assert_eq!(
                sampled.site_count, counted,
                "sampling drift for {move_type:?}"
            );
            assert_eq!(
                sampled.site.is_some(),
                counted > 0,
                "sample presence drift for {move_type:?}"
            );
            for site in &family.sites {
                assert!(
                    counted_site_is_backend_feasible(triangulation, site),
                    "counted {move_type:?} site {site:?} failed its immutable backend preflight"
                );
            }
            total_sites += counted;
        }
        assert!(
            total_sites > 0,
            "representative fixture exposed no proposal sites"
        );
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
            stats.record_hard_failure(move_type);
            assert_relative_eq!(stats.acceptance_rate(move_type), 0.0);
            stats.record_attempt(move_type);
            stats.record_success(move_type);
            assert_relative_eq!(stats.acceptance_rate(move_type), 0.5);
        }

        assert_eq!(stats.moves_22_attempted, 2);
        assert_eq!(stats.moves_13_attempted, 2);
        assert_eq!(stats.moves_31_attempted, 2);
        assert_eq!(stats.edge_flips_attempted, 2);
        assert_eq!(stats.moves_22_accepted, 1);
        assert_eq!(stats.moves_13_accepted, 1);
        assert_eq!(stats.moves_31_accepted, 1);
        assert_eq!(stats.edge_flips_accepted, 1);
        assert_eq!(stats.moves_22_hard_failed, 1);
        assert_eq!(stats.moves_13_hard_failed, 1);
        assert_eq!(stats.moves_31_hard_failed, 1);
        assert_eq!(stats.edge_flips_hard_failed, 1);
        assert_eq!(stats.total_hard_failures(), 4);
        assert_relative_eq!(stats.total_acceptance_rate(), 0.5);
    }

    #[test]
    fn move_statistics_saturate_extreme_counters() {
        let mut stats = MoveStatistics {
            moves_22_attempted: u64::MAX,
            moves_13_attempted: 1,
            moves_31_attempted: 1,
            edge_flips_attempted: 1,
            moves_22_accepted: u64::MAX,
            moves_13_accepted: 1,
            moves_31_accepted: 1,
            edge_flips_accepted: 1,
            moves_22_hard_failed: u64::MAX,
            moves_13_hard_failed: 1,
            moves_31_hard_failed: 1,
            edge_flips_hard_failed: 1,
        };

        stats.record_attempt(MoveType::Move22);
        stats.record_success(MoveType::Move22);
        stats.record_hard_failure(MoveType::Move22);

        assert_eq!(stats.moves_22_attempted, u64::MAX);
        assert_eq!(stats.moves_22_accepted, u64::MAX);
        assert_eq!(stats.moves_22_hard_failed, u64::MAX);
        assert_eq!(stats.total_attempted(), u64::MAX);
        assert_eq!(stats.total_accepted(), u64::MAX);
        assert_eq!(stats.total_hard_failures(), u64::MAX);
    }

    #[test]
    fn move_statistics_saturate_each_move_type_counter() {
        for move_type in [
            MoveType::Move22,
            MoveType::Move13Add,
            MoveType::Move31Remove,
            MoveType::EdgeFlip,
        ] {
            let mut stats = MoveStatistics::new();
            match move_type {
                MoveType::Move22 => {
                    stats.moves_22_attempted = u64::MAX;
                    stats.moves_22_accepted = u64::MAX;
                    stats.moves_22_hard_failed = u64::MAX;
                }
                MoveType::Move13Add => {
                    stats.moves_13_attempted = u64::MAX;
                    stats.moves_13_accepted = u64::MAX;
                    stats.moves_13_hard_failed = u64::MAX;
                }
                MoveType::Move31Remove => {
                    stats.moves_31_attempted = u64::MAX;
                    stats.moves_31_accepted = u64::MAX;
                    stats.moves_31_hard_failed = u64::MAX;
                }
                MoveType::EdgeFlip => {
                    stats.edge_flips_attempted = u64::MAX;
                    stats.edge_flips_accepted = u64::MAX;
                    stats.edge_flips_hard_failed = u64::MAX;
                }
            }

            stats.record_attempt(move_type);
            stats.record_success(move_type);
            stats.record_hard_failure(move_type);

            let (attempted, accepted, hard_failed) = match move_type {
                MoveType::Move22 => (
                    stats.moves_22_attempted,
                    stats.moves_22_accepted,
                    stats.moves_22_hard_failed,
                ),
                MoveType::Move13Add => (
                    stats.moves_13_attempted,
                    stats.moves_13_accepted,
                    stats.moves_13_hard_failed,
                ),
                MoveType::Move31Remove => (
                    stats.moves_31_attempted,
                    stats.moves_31_accepted,
                    stats.moves_31_hard_failed,
                ),
                MoveType::EdgeFlip => (
                    stats.edge_flips_attempted,
                    stats.edge_flips_accepted,
                    stats.edge_flips_hard_failed,
                ),
            };
            assert_eq!(attempted, u64::MAX);
            assert_eq!(accepted, u64::MAX);
            assert_eq!(hard_failed, u64::MAX);
            assert_relative_eq!(stats.acceptance_rate(move_type), 1.0);
        }
    }

    #[test]
    fn hard_failure_result_updates_stats_without_acceptance() {
        let mut system = ErgodicsSystem::new();
        system.stats.record_attempt(MoveType::Move13Add);

        let result = system.record_hard_failure_if_needed(
            MoveType::Move13Add,
            MoveResult::HardFailure(CdtError::ValidationFailed {
                check: CdtValidationCheck::ErgodicMoveCandidateGeometry,
                failure: CdtValidationFailure::ErgodicMoveCandidateGeometry {
                    detail: "simulated hard failure".to_string(),
                },
            }),
        );

        assert_matches!(result, MoveResult::HardFailure(_));
        assert_eq!(system.stats.moves_13_attempted, 1);
        assert_eq!(system.stats.moves_13_accepted, 0);
        assert_eq!(system.stats.moves_13_hard_failed, 1);
        assert_relative_eq!(system.stats.acceptance_rate(MoveType::Move13Add), 0.0);
    }

    #[test]
    fn backend_embedding_failure_remains_a_recoverable_rejection() {
        let result = reject_backend(
            BackendMutationOperation::FlipEdge,
            "candidate edge".to_string(),
            DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Four,
                detail: "simplices intersect outside their shared face".to_string(),
            },
        );

        assert_matches!(
            result,
            MoveResult::Rejected(CdtError::BackendMutationFailed {
                operation: BackendMutationOperation::FlipEdge,
                target,
                detail,
            }) if target == "candidate edge"
                && detail.contains("simplices intersect outside their shared face")
        );
    }

    #[test]
    fn move_statistics_defaults_hard_failures_for_legacy_payloads() {
        let stats: MoveStatistics = serde_json::from_str(
            r#"{
                "moves_22_attempted": 1,
                "moves_22_accepted": 1,
                "moves_13_attempted": 2,
                "moves_13_accepted": 0,
                "moves_31_attempted": 3,
                "moves_31_accepted": 0,
                "edge_flips_attempted": 4,
                "edge_flips_accepted": 1
            }"#,
        )
        .expect("legacy move statistics should deserialize");

        assert_eq!(stats.total_attempted(), 10);
        assert_eq!(stats.total_accepted(), 2);
        assert_eq!(stats.total_hard_failures(), 0);
    }

    #[test]
    fn move_statistics_from_wire_rejects_terminal_outcomes_above_attempts() {
        let wire = MoveStatisticsWire {
            moves_22_attempted: 1,
            moves_22_accepted: 1,
            moves_22_hard_failed: 1,
            moves_13_attempted: 0,
            moves_13_accepted: 0,
            moves_13_hard_failed: 0,
            moves_31_attempted: 0,
            moves_31_accepted: 0,
            moves_31_hard_failed: 0,
            edge_flips_attempted: 0,
            edge_flips_accepted: 0,
            edge_flips_hard_failed: 0,
        };

        let error = MoveStatistics::from_wire(&wire)
            .expect_err("accepted plus hard failures above attempts should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::MoveTerminalOutcomesExceedAttempted {
                    move_type: MoveType::Move22,
                    terminal: 2,
                    attempted: 1,
                }
            }
        );
    }

    #[test]
    fn move_statistics_from_wire_accepts_saturated_terminal_counters() {
        let wire = MoveStatisticsWire {
            moves_22_attempted: u64::MAX,
            moves_22_accepted: u64::MAX,
            moves_22_hard_failed: 1,
            moves_13_attempted: 0,
            moves_13_accepted: 0,
            moves_13_hard_failed: 0,
            moves_31_attempted: 0,
            moves_31_accepted: 0,
            moves_31_hard_failed: 0,
            edge_flips_attempted: 0,
            edge_flips_accepted: 0,
            edge_flips_hard_failed: 0,
        };

        let stats = MoveStatistics::from_wire(&wire)
            .expect("saturated terminal counters should deserialize");

        assert_eq!(stats.attempted(MoveType::Move22), u64::MAX);
        assert_eq!(stats.accepted(MoveType::Move22), u64::MAX);
        assert_eq!(stats.hard_failed(MoveType::Move22), 1);
    }

    #[test]
    fn move_22_uses_real_tri() {
        let mut system = ErgodicsSystem::with_seed(0);
        let mut triangulation = square_two_triangles();

        let result = system.attempt_22_move(&mut triangulation);

        assert_eq!(system.stats.moves_22_attempted, 1);
        assert_eq!(result, MoveResult::Success);
        assert_eq!(system.stats.moves_22_accepted, 1);
        assert!(
            triangulation
                .geometry()
                .triangulation()
                .is_valid_structure()
                .is_ok()
        );
        assert!(triangulation.validate_causality().is_ok());
        triangulation
            .validate()
            .expect("accepted k=2 move should preserve full evolved CDT invariants");
    }

    #[test]
    fn move_22_rejects_boundary_edge() {
        let mut system = ErgodicsSystem::with_seed(0);
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
    fn minimal_open_boundary_state_rejects_nonreversible_volume_insertion() {
        let mut system = ErgodicsSystem::with_seed(0);
        let mut triangulation = single_triangle();
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count(),
        );

        let result = system.attempt_13_move(&mut triangulation);

        assert_eq!(system.stats.moves_13_attempted, 1);
        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );
        assert!(
            triangulation
                .geometry()
                .triangulation()
                .is_valid_structure()
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
            triangulation.metadata().modification_count(),
        );

        let face = triangulation
            .geometry()
            .faces()
            .next()
            .expect("triangle face");
        let point = triangulation
            .geometry()
            .face_barycenter(&face)
            .expect("triangle barycenter");
        triangulation
            .subdivide_face_in_caller_transaction(face, &point)
            .expect("subdivide fixture face");
        assert_ne!(triangulation.vertex_count(), counts_before.0);

        let result = rollback_if_failed(
            &mut triangulation,
            snapshot,
            MoveResult::HardFailure(CdtError::ValidationFailed {
                check: CdtValidationCheck::ErgodicMoveCandidateGeometry,
                failure: CdtValidationFailure::ErgodicMoveCandidateGeometry {
                    detail: "simulated post-mutation failure".to_string(),
                },
            }),
        );

        assert_matches!(result, MoveResult::HardFailure(_));
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );
        assert!(triangulation.validate().is_ok());
    }

    #[test]
    fn checked_toroidal_wrapper_rejects_topology_mismatch() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");
        let result = CdtTriangulation2D::with_topology(backend, 3, 2, CdtTopology::Toroidal);

        assert_matches!(
            result,
            Err(CdtError::TopologyMismatch {
                topology,
                euler_characteristic: 1,
                expected_euler_characteristics,
                ..
            }) if topology == CdtTopology::Toroidal && expected_euler_characteristics == [0]
        );
    }

    #[test]
    fn is_post_mutation_candidate_rejection_classifies_topological_shape_errors() {
        let shape_errors = [
            CdtError::TopologyMismatch {
                topology: CdtTopology::Toroidal,
                euler_characteristic: 1,
                expected_euler_characteristics: vec![0],
                vertices: 4,
                edges: 5,
                faces: 2,
            },
            CdtError::Foliation(FoliationError::SpacelikeNonClosedRing {
                slice: 0,
                walked: 3,
                expected: 4,
            }),
            CdtError::Foliation(FoliationError::SpacelikeOpenSliceEndpointCount {
                slice: 1,
                observed: 3,
                expected: 2,
            }),
            CdtError::Foliation(FoliationError::OpenBoundarySlabEdgeCrossing {
                lower_slice: 0,
                first_edge: "a->d".to_string(),
                second_edge: "b->c".to_string(),
            }),
            CdtError::Foliation(FoliationError::OpenBoundarySpatialOrderMismatch {
                slice: 2,
                path_vertices: 4,
                coordinate_vertices: 4,
            }),
            CdtError::Foliation(FoliationError::OpenBoundaryTimeCoordinateMismatch {
                label: 2,
                vertex: "VertexKey(4v1)".to_string(),
                y: "2.1".to_string(),
            }),
        ];

        for err in shape_errors {
            assert!(
                err.is_post_mutation_candidate_rejection(),
                "shape-invalid post-mutation candidate should be an ordinary rejection: {err}"
            );
        }
    }

    #[test]
    fn is_post_mutation_candidate_rejection_keeps_unexpected_failures_hard() {
        let hard_errors = [
            CdtError::Foliation(FoliationError::MissingVertexLabel { vertex: 0 }),
            CdtError::Foliation(FoliationError::StaleBookkeeping {
                synced_at_modification: Some(1),
                current_modification_count: 2,
            }),
            CdtError::DelaunayValidationFailed {
                level: DelaunayValidationLevel::Four,
                detail: "level-four fixture failure".to_string(),
            },
            CdtError::ValidationFailed {
                check: CdtValidationCheck::Causality,
                failure: CdtValidationFailure::InvalidCdtTriangle {
                    face: "FaceKey(1f1)".to_string(),
                    spacelike_edges: 0,
                    timelike_edges: 3,
                },
            },
        ];

        for err in hard_errors {
            assert!(
                !err.is_post_mutation_candidate_rejection(),
                "unexpected post-mutation validation failure should remain hard: {err}"
            );
        }
    }

    #[test]
    fn proposal_site_count_matches_open_boundary_move_availability() {
        let mut system = ErgodicsSystem::with_seed(0);
        let mut triangulation = single_triangle();

        assert_eq!(proposal_site_count(&triangulation, MoveType::Move22), 0);
        assert_eq!(proposal_site_count(&triangulation, MoveType::EdgeFlip), 0);
        assert_eq!(proposal_site_count(&triangulation, MoveType::Move13Add), 0);
        assert_eq!(
            proposal_site_count(&triangulation, MoveType::Move31Remove),
            0
        );

        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count(),
        );
        let insert = system.attempt_13_move(&mut triangulation);
        assert_eq!(insert, MoveResult::GeometricViolation);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );
        assert_eq!(proposal_site_count(&triangulation, MoveType::Move13Add), 0);
        assert_eq!(
            proposal_site_count(&triangulation, MoveType::Move31Remove),
            0
        );

        let removal = system.attempt_31_move(&mut triangulation);
        assert_eq!(removal, MoveResult::GeometricViolation);
        assert_eq!(proposal_site_count(&triangulation, MoveType::Move13Add), 0);
        assert_eq!(
            proposal_site_count(&triangulation, MoveType::Move31Remove),
            0
        );
    }

    #[test]
    fn selected_proposal_site_reports_count_and_empty_state() {
        let mut system = ErgodicsSystem::with_seed(11);
        let empty_triangulation = single_triangle();

        let empty_selection =
            system.select_proposal_site(&empty_triangulation, MoveType::Move31Remove);
        assert_eq!(empty_selection.site_count, 0);
        assert!(empty_selection.site.is_none());

        let mut triangulation =
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build");
        let insertion_selection = system.select_proposal_site(&triangulation, MoveType::Move13Add);
        assert_eq!(
            insertion_selection.site_count,
            proposal_site_count(&triangulation, MoveType::Move13Add)
        );
        let Some(insertion_site) = insertion_selection.site else {
            panic!("nonempty insertion selection should include a concrete site");
        };

        let result =
            system.apply_proposal_site(&mut triangulation, MoveType::Move13Add, insertion_site);
        assert_eq!(result, MoveResult::Success);

        let removal_selection = system.select_proposal_site(&triangulation, MoveType::Move31Remove);
        assert_eq!(
            removal_selection.site_count,
            proposal_site_count(&triangulation, MoveType::Move31Remove)
        );
        assert!(removal_selection.site.is_some());
    }

    #[test]
    fn exact_proposal_counts_match_backend_feasibility_on_open_and_toroidal_fixtures() {
        let open = CdtTriangulation2D::from_cdt_strip(4, 3)
            .expect("representative open-boundary CDT should build");
        assert_counted_sites_match_backend_feasibility(&open);

        let toroidal = CdtTriangulation2D::from_toroidal_cdt(4, 4)
            .expect("representative toroidal CDT should build");
        assert_counted_sites_match_backend_feasibility(&toroidal);
    }

    #[test]
    fn independently_enumerated_volume_sites_match_proposal_counts() {
        for mut triangulation in [
            CdtTriangulation2D::from_cdt_strip(4, 3)
                .expect("representative open-boundary CDT should build"),
            CdtTriangulation2D::from_toroidal_cdt(4, 4)
                .expect("representative toroidal CDT should build"),
        ] {
            assert_eq!(
                proposal_site_count(&triangulation, MoveType::Move13Add),
                independently_count_foliated_insertions(&triangulation)
            );
            assert_eq!(
                proposal_site_count(&triangulation, MoveType::Move31Remove),
                independently_count_foliated_removals(&triangulation)
            );

            let mut moves = ErgodicsSystem::with_seed(0x1331);
            let inserted = (0..64).any(|_| {
                matches!(
                    moves.attempt_13_move(&mut triangulation),
                    MoveResult::Success
                )
            });
            assert!(
                inserted,
                "fixture should expose at least one successful insertion"
            );
            assert_eq!(
                proposal_site_count(&triangulation, MoveType::Move31Remove),
                independently_count_foliated_removals(&triangulation)
            );
        }
    }

    #[test]
    fn exact_proposal_preflights_reject_coarse_but_infeasible_sites() {
        let insertion = single_triangle();
        let face = insertion
            .geometry()
            .faces()
            .next()
            .expect("triangle fixture should have one face");
        let degenerate_point = [0.5_f64, 0.0];
        assert!(
            degenerate_point
                .iter()
                .all(|coordinate| coordinate.is_finite())
        );
        assert!(!insertion_candidate_is_sampleable(
            &insertion,
            &face,
            &degenerate_point
        ));

        let removal = square_two_triangles();
        let vertices: Vec<_> = removal.geometry().vertices().collect();
        let vertex = vertices
            .iter()
            .find(|candidate| {
                removal
                    .geometry()
                    .incident_edges(candidate)
                    .is_ok_and(|edges| edges.count() == 3)
            })
            .expect("square fixture should have a diagonal endpoint");
        let neighbors: Vec<_> = vertices
            .iter()
            .filter(|candidate| *candidate != vertex)
            .cloned()
            .collect();
        let [neighbor_0, neighbor_1, neighbor_2] = neighbors.as_slice() else {
            panic!("square fixture should have three alternate vertices");
        };
        let neighbors = [neighbor_0.clone(), neighbor_1.clone(), neighbor_2.clone()];
        assert!(!face_exists_with_vertices(&removal, &neighbors));
        assert!(!removal_candidate_is_sampleable(
            &removal, vertex, &neighbors
        ));
    }

    #[test]
    fn proposal_site_cache_records_no_site_self_loops_without_mutating() {
        let mut system = ErgodicsSystem::with_seed(11);
        let triangulation = single_triangle();
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count(),
        );

        let empty_selection = system.select_proposal_site(&triangulation, MoveType::Move31Remove);
        assert_eq!(empty_selection.site_count, 0);
        assert!(empty_selection.site.is_none());
        let cached_family = system.site_cache.family(MoveType::Move31Remove);
        assert_eq!(cached_family.instance_id, Some(triangulation.instance_id()));
        assert_eq!(
            cached_family.modification_count,
            Some(triangulation.metadata().modification_count())
        );

        let retry_selection = system.select_proposal_site(&triangulation, MoveType::Move31Remove);
        assert_eq!(retry_selection.site_count, 0);
        assert!(retry_selection.site.is_none());
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move31Remove)
                .modification_count,
            Some(counts_before.3)
        );
    }

    #[test]
    fn proposal_site_cache_remains_current_after_rolled_back_mutation() {
        let mut system = ErgodicsSystem::with_seed(11);
        let mut triangulation =
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build");
        let initial_modification_count = triangulation.metadata().modification_count();

        let insertion_selection = system.select_proposal_site(&triangulation, MoveType::Move13Add);
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move13Add)
                .modification_count,
            Some(initial_modification_count)
        );
        assert!(insertion_selection.site_count > 0);
        assert!(insertion_selection.site.is_some());
        let face = triangulation
            .geometry()
            .faces()
            .next()
            .expect("strip fixture should contain a face");
        let point = triangulation
            .geometry()
            .face_barycenter(&face)
            .expect("triangle barycenter should resolve");
        let label = causal_insertion_label(&triangulation, &face)
            .expect("labeled face should admit a deliberately incomplete link split");
        let insertion_site = ProposalSite::FaceSubdivision { face, point, label };

        let result =
            system.apply_proposal_site(&mut triangulation, MoveType::Move13Add, insertion_site);
        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(
            triangulation.metadata().modification_count(),
            initial_modification_count
        );
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move13Add)
                .modification_count,
            Some(initial_modification_count),
            "rolled-back mutations leave the old cache current"
        );

        let removal_selection = system.select_proposal_site(&triangulation, MoveType::Move31Remove);
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move31Remove)
                .modification_count,
            Some(initial_modification_count)
        );
        assert_eq!(
            removal_selection.site_count,
            proposal_site_count(&triangulation, MoveType::Move31Remove)
        );
        assert!(removal_selection.site.is_none());
    }

    #[test]
    fn proposal_site_cache_remains_current_after_ordinary_rejection() {
        let mut system = ErgodicsSystem::with_seed(11);
        let mut triangulation =
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build");
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count(),
        );

        let insertion_selection = system.select_proposal_site(&triangulation, MoveType::Move13Add);
        let Some(insertion_site) = insertion_selection.site else {
            panic!("open-boundary strip should expose an insertion site");
        };
        let cached_modification_count = system
            .site_cache
            .family(MoveType::Move13Add)
            .modification_count;

        let result =
            system.apply_proposal_site(&mut triangulation, MoveType::Move22, insertion_site);
        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );

        let retry_selection = system.select_proposal_site(&triangulation, MoveType::Move13Add);
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move13Add)
                .modification_count,
            cached_modification_count
        );
        assert_eq!(retry_selection.site_count, insertion_selection.site_count);
        assert!(retry_selection.site.is_some());
    }

    #[test]
    fn proposal_site_cache_tracks_cloned_proposed_states_by_modification_count() {
        let mut system = ErgodicsSystem::with_seed(11);
        let original =
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build");

        let insertion_selection = system.select_proposal_site(&original, MoveType::Move13Add);
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move13Add)
                .modification_count,
            Some(original.metadata().modification_count())
        );
        let Some(insertion_site) = insertion_selection.site else {
            panic!("open-boundary strip should expose one insertion site");
        };

        let mut proposed_state = original.clone();
        let insertion_site = insertion_site
            .remap_for_clone(&original, &proposed_state)
            .expect("proposal site should remap into the cloned owner");
        let result =
            system.apply_proposal_site(&mut proposed_state, MoveType::Move13Add, insertion_site);
        assert_eq!(result, MoveResult::Success);
        assert!(
            proposed_state.metadata().modification_count()
                > original.metadata().modification_count()
        );

        let proposed_selection =
            system.select_proposal_site(&proposed_state, MoveType::Move31Remove);
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move31Remove)
                .modification_count,
            Some(proposed_state.metadata().modification_count())
        );
        assert!(proposed_selection.site.is_some());

        let original_selection = system.select_proposal_site(&original, MoveType::Move13Add);
        assert_eq!(
            system
                .site_cache
                .family(MoveType::Move13Add)
                .modification_count,
            Some(original.metadata().modification_count())
        );
        assert_eq!(
            original_selection.site_count,
            insertion_selection.site_count
        );
    }

    #[test]
    fn proposal_site_cache_refreshes_for_distinct_triangulation_sources() {
        let mut system = ErgodicsSystem::with_seed(11);
        let first = CdtTriangulation2D::from_cdt_strip(4, 3)
            .expect("first open-boundary strip should build");
        let second = CdtTriangulation2D::from_cdt_strip(4, 3)
            .expect("second open-boundary strip should build");
        assert_eq!(
            first.metadata().modification_count(),
            second.metadata().modification_count()
        );

        let first_selection = system.select_proposal_site(&first, MoveType::Move13Add);
        let first_instance_id = system
            .site_cache
            .family(MoveType::Move13Add)
            .instance_id
            .expect("selection should populate cache instance identity");
        assert!(first_selection.site_count > 0);

        let second_selection = system.select_proposal_site(&second, MoveType::Move13Add);
        assert_eq!(
            system.site_cache.family(MoveType::Move13Add).instance_id,
            Some(second.instance_id())
        );
        assert_ne!(
            first_instance_id,
            system
                .site_cache
                .family(MoveType::Move13Add)
                .instance_id
                .expect("second selection should populate cache instance identity")
        );
        assert_eq!(second_selection.site_count, first_selection.site_count);
    }

    #[test]
    fn proposal_site_cache_refreshes_across_topology_replacement() {
        let mut system = ErgodicsSystem::with_seed(11);
        let toroidal =
            CdtTriangulation2D::from_toroidal_cdt(4, 3).expect("toroidal fixture should build");
        let strip = CdtTriangulation2D::from_cdt_strip(4, 3).expect("strip fixture should build");
        assert_eq!(
            toroidal.metadata().modification_count(),
            strip.metadata().modification_count()
        );
        assert_ne!(toroidal.instance_id(), strip.instance_id());

        let toroidal_selection = system.select_proposal_site(&toroidal, MoveType::Move13Add);
        assert_eq!(
            system.site_cache.family(MoveType::Move13Add).instance_id,
            Some(toroidal.instance_id())
        );
        let Some(toroidal_site) = toroidal_selection.site else {
            panic!("toroidal fixture should expose insertion sites");
        };
        assert_matches!(toroidal_site, ProposalSite::FoliatedInsertion(_));

        let strip_selection = system.select_proposal_site(&strip, MoveType::Move13Add);
        assert_eq!(
            system.site_cache.family(MoveType::Move13Add).instance_id,
            Some(strip.instance_id())
        );
        assert_eq!(
            strip_selection.site_count,
            proposal_site_count(&strip, MoveType::Move13Add)
        );
        let Some(strip_site) = strip_selection.site else {
            panic!("open-boundary strip fixture should expose insertion sites");
        };
        assert_matches!(strip_site, ProposalSite::FoliatedInsertion(_));
    }

    #[test]
    fn proposal_site_cache_refreshes_for_cloned_triangulation_instances() {
        let mut system = ErgodicsSystem::with_seed(11);
        let original =
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build");
        let cloned = original.clone();
        assert_eq!(
            original.metadata().modification_count(),
            cloned.metadata().modification_count()
        );
        assert_ne!(original.instance_id(), cloned.instance_id());

        let original_selection = system.select_proposal_site(&original, MoveType::Move13Add);
        assert_eq!(
            system.site_cache.family(MoveType::Move13Add).instance_id,
            Some(original.instance_id())
        );
        assert!(original_selection.site_count > 0);

        let cloned_selection = system.select_proposal_site(&cloned, MoveType::Move13Add);
        assert_eq!(
            system.site_cache.family(MoveType::Move13Add).instance_id,
            Some(cloned.instance_id())
        );
        assert_eq!(cloned_selection.site_count, original_selection.site_count);
    }

    #[test]
    fn mismatched_proposal_site_rejects_without_mutating() {
        let mut system = ErgodicsSystem::with_seed(11);
        let mut triangulation =
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build");
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count(),
        );

        let insertion_selection = system.select_proposal_site(&triangulation, MoveType::Move13Add);
        let Some(insertion_site) = insertion_selection.site else {
            panic!("open-boundary strip should expose one insertion site");
        };

        let result =
            system.apply_proposal_site(&mut triangulation, MoveType::Move22, insertion_site);

        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );
        triangulation
            .validate()
            .expect("rejecting a mismatched proposal site should leave CDT invariants intact");
    }

    #[test]
    fn proposal_site_count_handles_nonuniform_toroidal_profiles() {
        let mut system = ErgodicsSystem::with_seed(7);
        let mut triangulation =
            CdtTriangulation2D::from_toroidal_cdt_spatial_vertex_profile(&[3, 4, 5, 4])
                .expect("nonuniform toroidal CDT should build");

        let initial_insertions = proposal_site_count(&triangulation, MoveType::Move13Add);
        let initial_removals = proposal_site_count(&triangulation, MoveType::Move31Remove);
        assert!(
            initial_insertions > 0,
            "nonuniform toroidal profiles should expose counted insertion sites"
        );
        assert!(
            initial_removals > 0,
            "nonuniform toroidal profiles should expose counted inverse-volume sites"
        );

        let result = system.attempt_13_move(&mut triangulation);
        assert_eq!(result, MoveResult::Success);
        triangulation
            .validate()
            .expect("counted nonuniform-profile insertion should preserve CDT invariants");
        assert!(proposal_site_count(&triangulation, MoveType::Move13Add) > 0);
    }

    #[test]
    fn removal_sampleable_guard_rejects_existing_replacement_face() {
        let triangulation = single_triangle();
        let face = triangulation
            .geometry()
            .faces()
            .next()
            .expect("triangle fixture should have one face");
        let vertices = triangulation
            .geometry()
            .face_vertices(&face)
            .expect("triangle face should resolve vertices")
            .collect::<Vec<_>>();
        let [v0, v1, v2] = vertices.as_slice() else {
            panic!("triangle fixture face should have exactly three vertices");
        };
        let replacement_vertices = [v0.clone(), v1.clone(), v2.clone()];

        assert!(face_exists_with_vertices(
            &triangulation,
            &replacement_vertices
        ));
        assert!(!removal_candidate_is_sampleable(
            &triangulation,
            v0,
            &replacement_vertices
        ));
    }

    #[test]
    fn open_boundary_move_31_reports_unavailable_without_inverse_site() {
        let mut system = ErgodicsSystem::with_seed(0);
        let mut triangulation = single_triangle();
        let result = system.attempt_13_move(&mut triangulation);
        assert_matches!(result, MoveResult::GeometricViolation);
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.metadata().modification_count(),
        );

        let result = system.attempt_31_move(&mut triangulation);

        assert_eq!(system.stats.moves_31_attempted, 1);
        assert_eq!(result, MoveResult::GeometricViolation);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
                triangulation.metadata().modification_count(),
            ),
            counts_before
        );
        assert!(
            triangulation
                .geometry()
                .triangulation()
                .is_valid_structure()
                .is_ok()
        );
        assert!(triangulation.validate_causality().is_ok());
    }

    #[test]
    fn move_31_requires_degree_three() {
        let mut system = ErgodicsSystem::with_seed(0);
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
    fn periodic_toroidal_move_13_splits_spacelike_link() {
        let mut system = ErgodicsSystem::with_seed(7);
        let mut triangulation =
            CdtTriangulation2D::from_toroidal_cdt(8, 8).expect("build toroidal CDT");
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        );

        let result = system.attempt_13_move(&mut triangulation);

        assert_eq!(
            result,
            MoveResult::Success,
            "periodic toroidal Move13Add should split a spacelike link, got {result:?}"
        );
        assert_eq!(system.stats.moves_13_accepted, 1);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
            ),
            (
                counts_before.0 + 1,
                counts_before.1 + 3,
                counts_before.2 + 2
            ),
            "accepted periodic toroidal insertion should apply the CDT volume-move count delta"
        );
        triangulation
            .validate()
            .expect("accepted periodic toroidal Move13Add should preserve evolved CDT invariants");
    }

    #[test]
    fn periodic_toroidal_move_31_reverses_link_split() {
        let mut system = ErgodicsSystem::with_seed(0);
        let mut triangulation =
            CdtTriangulation2D::from_toroidal_cdt(8, 8).expect("build toroidal CDT");
        let insert = system.attempt_13_move(&mut triangulation);
        assert_eq!(insert, MoveResult::Success);
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        );

        let mut removed = false;
        for _ in 0..64 {
            match system.attempt_31_move(&mut triangulation) {
                MoveResult::Success => {
                    removed = true;
                    break;
                }
                MoveResult::Rejected(_) | MoveResult::GeometricViolation => {}
                other => panic!("unexpected toroidal removal result: {other:?}"),
            }
        }

        assert!(
            removed,
            "sampleable periodic toroidal Move31Remove sites should include upstream offset-aware successes"
        );
        assert_eq!(system.stats.moves_31_accepted, 1);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
            ),
            (
                counts_before.0 - 1,
                counts_before.1 - 3,
                counts_before.2 - 2
            ),
            "accepted periodic toroidal removal should reverse a local 1,3 subdivision"
        );
        assert!(
            triangulation
                .slab_triangle_profile()
                .expect("accepted toroidal removal profile should be valid")
                .iter()
                .all(|&count| count >= 3),
            "accepted periodic toroidal removal must preserve nonempty closed spatial slices"
        );
        triangulation.validate().expect(
            "accepted periodic toroidal Move31Remove should preserve evolved CDT invariants",
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the exhaustive round-trip test keeps proposal application, rejection atomicity, stable-identity lookup, and canonical-state evidence together"
    )]
    fn every_small_fixture_insertion_has_an_exact_canonical_inverse() {
        for original in [
            CdtTriangulation2D::from_cdt_strip(4, 3).expect("open-boundary strip should build"),
            CdtTriangulation2D::from_toroidal_cdt(3, 3).expect("toroidal CDT should build"),
        ] {
            let original_signature = canonical_signature(&original);
            let action_config = ActionConfig::default();
            let original_action = action_config.calculate_action(
                original.vertex_count(),
                original.edge_count(),
                original.face_count(),
            );
            let insertion_family = MoveSiteCache::collect_family(
                &original,
                MoveType::Move13Add,
                original.instance_id(),
                original.metadata().modification_count(),
            );
            assert!(!insertion_family.sites.is_empty());
            let mut successful_round_trips = 0;

            for insertion_site in insertion_family.sites {
                assert_matches!(insertion_site, ProposalSite::FoliatedInsertion(_));
                let mut triangulation = original.clone();
                let original_vertices = triangulation
                    .geometry()
                    .vertices()
                    .map(|vertex| {
                        triangulation
                            .geometry()
                            .stable_vertex_id(&vertex)
                            .expect("initial vertex identity should resolve")
                    })
                    .collect::<HashSet<_>>();
                let insertion_site = insertion_site
                    .remap_for_clone(&original, &triangulation)
                    .expect("insertion site should remap to its test clone");
                let mut system = ErgodicsSystem::with_seed(0x13_31);
                let insertion_result = system.apply_proposal_site(
                    &mut triangulation,
                    MoveType::Move13Add,
                    insertion_site,
                );
                match insertion_result {
                    MoveResult::Success => {}
                    MoveResult::Rejected(_)
                    | MoveResult::GeometricViolation
                    | MoveResult::CausalityViolation => {
                        assert_eq!(canonical_signature(&triangulation), original_signature);
                        continue;
                    }
                    MoveResult::HardFailure(error) => {
                        panic!("offered insertion produced a hard failure: {error}")
                    }
                }

                let added_vertices = triangulation
                    .geometry()
                    .vertices()
                    .filter_map(|vertex| {
                        let stable_id = triangulation
                            .geometry()
                            .stable_vertex_id(&vertex)
                            .expect("evolved vertex identity should resolve");
                        (!original_vertices.contains(&stable_id)).then_some(stable_id)
                    })
                    .collect::<Vec<_>>();
                let [added_vertex] = added_vertices.as_slice() else {
                    panic!("one insertion should create exactly one stable vertex identity");
                };

                let removal_family = MoveSiteCache::collect_family(
                    &triangulation,
                    MoveType::Move31Remove,
                    triangulation.instance_id(),
                    triangulation.metadata().modification_count(),
                );
                let inverse_sites = removal_family
                    .sites
                    .into_iter()
                    .filter(|site| {
                        let ProposalSite::FoliatedRemoval(candidate) = site else {
                            return false;
                        };
                        triangulation
                            .geometry()
                            .stable_vertex_id(&candidate.vertex)
                            .is_ok_and(|stable_id| stable_id == *added_vertex)
                    })
                    .collect::<Vec<_>>();
                assert!(
                    !inverse_sites.is_empty(),
                    "the inserted vertex should expose at least one inverse site"
                );

                let proposed_signature = canonical_signature(&triangulation);
                let mut exact_inverse = None;
                for inverse_site in inverse_sites {
                    let mut trial = triangulation.clone();
                    let inverse_site = inverse_site
                        .remap_for_clone(&triangulation, &trial)
                        .expect("inverse site should remap to its test clone");
                    let mut inverse_system = ErgodicsSystem::with_seed(0x31_13);
                    match inverse_system.apply_proposal_site(
                        &mut trial,
                        MoveType::Move31Remove,
                        inverse_site,
                    ) {
                        MoveResult::Success => {
                            trial
                                .validate()
                                .expect("round-tripped CDT should remain valid");
                            assert_eq!(canonical_signature(&trial), original_signature);
                            exact_inverse = Some(trial);
                            break;
                        }
                        MoveResult::Rejected(_)
                        | MoveResult::GeometricViolation
                        | MoveResult::CausalityViolation => {
                            assert_eq!(canonical_signature(&trial), proposed_signature);
                        }
                        MoveResult::HardFailure(error) => {
                            panic!("offered inverse produced a hard failure: {error}")
                        }
                    }
                }
                let triangulation = exact_inverse
                    .expect("one concrete inverse support edge should restore the original CDT");
                successful_round_trips += 1;
                triangulation
                    .validate()
                    .expect("round-tripped CDT should remain valid");
                assert_eq!(canonical_signature(&triangulation), original_signature);
                assert_relative_eq!(
                    action_config.calculate_action(
                        triangulation.vertex_count(),
                        triangulation.edge_count(),
                        triangulation.face_count(),
                    ),
                    original_action,
                    epsilon = f64::EPSILON
                );
            }
            assert!(
                successful_round_trips > 0,
                "representative fixture should contain a round-trippable insertion"
            );
        }
    }

    #[test]
    fn proposal_site_count_tracks_toroidal_inverse_sites() {
        let mut system = ErgodicsSystem::with_seed(0);
        let mut triangulation =
            CdtTriangulation2D::from_toroidal_cdt(8, 8).expect("build toroidal CDT");

        assert!(
            proposal_site_count(&triangulation, MoveType::Move31Remove) > 0,
            "initial toroidal CDT should expose sampleable inverse-volume sites"
        );
        assert!(proposal_site_count(&triangulation, MoveType::Move13Add) > 0);

        let mut inserted = false;
        for _ in 0..64 {
            match system.attempt_13_move(&mut triangulation) {
                MoveResult::Success => {
                    inserted = true;
                    break;
                }
                MoveResult::Rejected(_) | MoveResult::CausalityViolation => {}
                other => panic!("unexpected toroidal insertion result: {other:?}"),
            }
        }
        assert!(
            inserted,
            "sampleable toroidal insertion sites should include successes"
        );

        assert!(
            proposal_site_count(&triangulation, MoveType::Move31Remove) > 0,
            "a toroidal spacelike-link split should expose at least one sampleable inverse site"
        );

        let mut removed = false;
        for _ in 0..64 {
            match system.attempt_31_move(&mut triangulation) {
                MoveResult::Success => {
                    removed = true;
                    break;
                }
                MoveResult::Rejected(_) | MoveResult::GeometricViolation => {}
                other => panic!("unexpected toroidal removal result: {other:?}"),
            }
        }
        assert!(
            removed,
            "sampleable toroidal inverse sites should include successes"
        );
        triangulation
            .validate()
            .expect("proposal-counted toroidal inverse move should preserve CDT invariants");
    }

    #[test]
    fn periodic_toroidal_move_31_rejects_minimal_slice_without_inverse_site() {
        let mut system = ErgodicsSystem::with_seed(7);
        let mut triangulation =
            CdtTriangulation2D::from_toroidal_cdt(3, 3).expect("build minimal toroidal CDT");
        let counts_before = (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        );
        let profile_before = triangulation
            .slab_triangle_profile()
            .expect("initial minimal toroidal profile should be valid");

        let result = system.attempt_31_move(&mut triangulation);

        assert_matches!(
            result,
            MoveResult::GeometricViolation,
            "minimal periodic toroidal slices should reject volume removal without an inverse site"
        );
        assert_eq!(system.stats.moves_31_attempted, 1);
        assert_eq!(system.stats.moves_31_accepted, 0);
        assert_eq!(
            (
                triangulation.vertex_count(),
                triangulation.edge_count(),
                triangulation.face_count(),
            ),
            counts_before,
            "rejected minimal toroidal removal must preserve simplex counts"
        );
        assert_eq!(
            triangulation
                .slab_triangle_profile()
                .expect("rejected minimal toroidal profile should be valid"),
            profile_before,
            "rejected minimal toroidal removal must preserve closed spatial slices"
        );
        triangulation
            .validate()
            .expect("rejected minimal toroidal removal should preserve CDT invariants");
    }

    #[test]
    fn periodic_toroidal_k2_move_attempts_preserve_invariants() {
        type AttemptK2Move = fn(&mut ErgodicsSystem, &mut CdtTriangulation2D) -> MoveResult;
        type K2MoveCase = (MoveType, u64, AttemptK2Move);

        let cases: [K2MoveCase; 2] = [
            (MoveType::Move22, 11, ErgodicsSystem::attempt_22_move),
            (MoveType::EdgeFlip, 13, ErgodicsSystem::attempt_edge_flip),
        ];

        for (move_type, seed, attempt_move) in cases {
            let mut system = ErgodicsSystem::with_seed(seed);
            let mut triangulation =
                CdtTriangulation2D::from_toroidal_cdt(8, 8).expect("build toroidal CDT");

            let result = attempt_move(&mut system, &mut triangulation);

            assert_matches!(
                result,
                MoveResult::Success
                    | MoveResult::CausalityViolation
                    | MoveResult::GeometricViolation,
                "periodic toroidal {move_type:?} should not fail through backend offset handling"
            );
            triangulation.validate().expect(
                "periodic toroidal k=2 move attempt should preserve evolved CDT invariants",
            );
        }
    }

    #[test]
    fn edge_flip_uses_own_stats() {
        let mut system = ErgodicsSystem::with_seed(0);
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
        let mut system = ErgodicsSystem::with_seed(0);

        let mut move_types = HashSet::new();
        for _ in 0..100 {
            move_types.insert(system.select_random_move());
        }

        assert!(move_types.len() > 1);
    }

    #[test]
    fn quantized_family_selector_matches_every_reported_support_interval() {
        let distribution = CdtMoveFamilyDistribution::from_weights([
            f64::MIN_POSITIVE,
            1.0,
            f64::MIN_POSITIVE,
            f64::MIN_POSITIVE,
        ])
        .expect("extreme finite family weights should remain supported");
        let mut interval_start = 0_u64;

        for family in MoveType::REVERSIBLE_1P1 {
            let mass = distribution.sample_mass(family);
            assert!(mass > 0, "positive raw weight lost support for {family:?}");
            assert_eq!(select_move_family_at(&distribution, interval_start), family);
            assert_eq!(
                select_move_family_at(&distribution, interval_start + mass - 1),
                family
            );
            interval_start += mass;
        }

        assert_eq!(interval_start, 1_u64 << 53);
        assert_eq!(
            select_move_family_at(&distribution, (1_u64 << 53) - 1),
            MoveType::EdgeFlip
        );
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
