#![forbid(unsafe_code)]

//! Simulation result containers and post-simulation summaries.
//!
//! This module owns measurement records, complete simulation outputs, and
//! convenience analysis methods that summarize recorded measurements or inspect
//! the final triangulation state.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{MoveStatistics, MoveType};
use crate::cdt::metropolis::{
    ChainId, MetropolisConfig, MonteCarloStep, MonteCarloStepOutcome, ProposalStatistics, Trace,
    TraceError, TraceRecord, TraceStepOutcome,
    checkpoint::{chain_counters, checkpoint_resume_failed},
    helpers::{actions_match, expected_measurement_count, expected_measurement_step},
};
use crate::cdt::observables::{estimate_hausdorff_dimension, estimate_spectral_dimension};
use crate::cdt::triangulation::CdtSimplexCounts;
use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
use crate::errors::{
    CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure, CheckpointMoveCounter,
    CheckpointResumeFailure, MeasurementCountField, OutputFormat, ProposalTelemetryCounter,
    ScalarTraceField,
};
use crate::geometry::traits::TriangulationQuery;
use crate::geometry::{CdtTriangulation2D, SpacetimeCoordinate};
use crate::util::usize_to_f64;
use serde::de::Error as DeError;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::to_writer_pretty;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::time::Duration;

/// Scalar measurement data collected during simulation.
///
/// Use [`Self::new`] and builder-style methods such as
/// [`Self::try_with_volume_profile`] rather than struct literals outside this
/// crate; additional measurement fields may be added over time. Simplex counts
/// are stored as [`NonZeroU32`] so serialized results cannot represent an empty
/// measured triangulation. Use [`NonZeroU32::get`] when raw `u32` counts are
/// needed for reporting or arithmetic.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Measurement {
    /// Monte Carlo step when measurement was taken.
    step: u32,
    /// Current action value.
    action: f64,
    /// Nonzero number of vertices.
    vertices: NonZeroU32,
    /// Nonzero number of edges.
    edges: NonZeroU32,
    /// Nonzero number of triangles.
    triangles: NonZeroU32,
    /// Per-slice triangle counts `N₂(t)` from
    /// [`CdtTriangulation::volume_profile`](crate::cdt::triangulation::CdtTriangulation::volume_profile).
    ///
    /// Entry `t` counts classifiable CDT triangles assigned to time slab `t`;
    /// the vector is empty when the measured triangulation has no current
    /// foliation.
    volume_profile: Vec<u32>,
}

/// Base observable columns emitted by [`SimulationResultsBackend::scalar_trace`].
const SCALAR_TRACE_BASE_OBSERVABLES: [&str; 13] = [
    "action",
    "vertices",
    "edges",
    "triangles",
    "move_family",
    "delta_action",
    "delta_action_present",
    "action_before",
    "action_after",
    "action_after_present",
    "seed_low_u32",
    "seed_high_u32",
    "seed_present",
];

/// Single-chain identifier used by CDT result exports.
const CDT_TRACE_CHAIN_ID: ChainId = ChainId::new(0);

/// Invariant-bearing CDT trace outcome stored in serialized result/checkpoint rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CdtScalarTraceOutcome {
    /// A concrete proposal was accepted and committed.
    Accepted,
    /// A concrete proposal was rejected by Metropolis-Hastings.
    RejectedProposal,
    /// No concrete proposal was available or a local proposal check rejected it.
    NoProposal,
}

impl CdtScalarTraceOutcome {
    /// Converts the CDT-owned outcome into the upstream trace outcome type.
    const fn into_trace_outcome(self) -> TraceStepOutcome {
        match self {
            Self::Accepted => TraceStepOutcome::accepted(),
            Self::RejectedProposal => TraceStepOutcome::rejected_proposal(),
            Self::NoProposal => TraceStepOutcome::no_proposal(),
        }
    }
}

/// A rectangular scalar trace row captured from the upstream planned-step outcome.
#[derive(Debug, Clone)]
pub(crate) struct CdtScalarTraceRow {
    step: NonZeroU32,
    payload: CdtScalarTracePayload,
    log_prob: f64,
    action: f64,
    vertices: NonZeroU32,
    edges: NonZeroU32,
    triangles: NonZeroU32,
    move_type: MoveType,
    action_before: f64,
    seed: Option<u64>,
    volume_profile: Vec<u32>,
}

/// Outcome-specific scalar trace action evidence.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CdtScalarTracePayload {
    /// Accepted proposals must carry both post-move action and action delta.
    Accepted {
        delta_action: f64,
        action_after: f64,
    },
    /// Rejected concrete proposals may carry a scored proposal delta.
    RejectedProposal { delta_action: Option<f64> },
    /// Local proposal absence carries no concrete action evidence.
    NoProposal,
}

impl CdtScalarTracePayload {
    const fn from_parts(
        step: NonZeroU32,
        outcome: CdtScalarTraceOutcome,
        delta_action: Option<f64>,
        action_after: Option<f64>,
    ) -> CdtResult<Self> {
        match (outcome, delta_action, action_after) {
            (CdtScalarTraceOutcome::Accepted, Some(delta_action), Some(action_after)) => {
                Ok(Self::Accepted {
                    delta_action,
                    action_after,
                })
            }
            (CdtScalarTraceOutcome::Accepted, None, _)
            | (CdtScalarTraceOutcome::NoProposal, Some(_), _) => Err(checkpoint_resume_failed(
                CheckpointResumeFailure::ScalarTraceDeltaActionMismatch { step: step.get() },
            )),
            (CdtScalarTraceOutcome::Accepted, Some(_), None)
            | (CdtScalarTraceOutcome::RejectedProposal, _, Some(_))
            | (CdtScalarTraceOutcome::NoProposal, None, Some(_)) => Err(checkpoint_resume_failed(
                CheckpointResumeFailure::ScalarTraceActionAfterMismatch { step: step.get() },
            )),
            (CdtScalarTraceOutcome::RejectedProposal, delta_action, None) => {
                Ok(Self::RejectedProposal { delta_action })
            }
            (CdtScalarTraceOutcome::NoProposal, None, None) => Ok(Self::NoProposal),
        }
    }

    const fn outcome(self) -> CdtScalarTraceOutcome {
        match self {
            Self::Accepted { .. } => CdtScalarTraceOutcome::Accepted,
            Self::RejectedProposal { .. } => CdtScalarTraceOutcome::RejectedProposal,
            Self::NoProposal => CdtScalarTraceOutcome::NoProposal,
        }
    }

    const fn delta_action(self) -> Option<f64> {
        match self {
            Self::Accepted { delta_action, .. } => Some(delta_action),
            Self::RejectedProposal { delta_action } => delta_action,
            Self::NoProposal => None,
        }
    }

    const fn action_after(self) -> Option<f64> {
        match self {
            Self::Accepted { action_after, .. } => Some(action_after),
            Self::RejectedProposal { .. } | Self::NoProposal => None,
        }
    }
}

/// Raw scalar trace shape used only while deserializing result and checkpoint payloads.
#[derive(Deserialize)]
struct CdtScalarTraceRowWire {
    step: u32,
    outcome: CdtScalarTraceOutcome,
    log_prob: f64,
    action: f64,
    vertices: u32,
    edges: u32,
    triangles: u32,
    move_type: MoveType,
    delta_action: Option<f64>,
    action_before: f64,
    action_after: Option<f64>,
    seed: Option<u64>,
    volume_profile: Vec<u32>,
}

impl TryFrom<CdtScalarTraceRowWire> for CdtScalarTraceRow {
    type Error = CdtError;

    fn try_from(wire: CdtScalarTraceRowWire) -> Result<Self, Self::Error> {
        let step = NonZeroU32::new(wire.step).ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::ScalarTraceStepZero {
                actual: wire.step,
            })
        })?;
        let vertices = nonzero_scalar_trace_count(MeasurementCountField::Vertices, wire.vertices)?;
        let edges = nonzero_scalar_trace_count(MeasurementCountField::Edges, wire.edges)?;
        let triangles =
            nonzero_scalar_trace_count(MeasurementCountField::Triangles, wire.triangles)?;
        let payload = CdtScalarTracePayload::from_parts(
            step,
            wire.outcome,
            wire.delta_action,
            wire.action_after,
        )?;
        validate_scalar_trace_volume_profile(step, triangles, &wire.volume_profile)?;
        let row = Self {
            step,
            payload,
            log_prob: wire.log_prob,
            action: wire.action,
            vertices,
            edges,
            triangles,
            move_type: wire.move_type,
            action_before: wire.action_before,
            seed: wire.seed,
            volume_profile: wire.volume_profile,
        };
        validate_scalar_trace_finite_fields(&row)?;
        Ok(row)
    }
}

impl<'de> Deserialize<'de> for CdtScalarTraceRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CdtScalarTraceRowWire::deserialize(deserializer)?
            .try_into()
            .map_err(DeError::custom)
    }
}

impl Serialize for CdtScalarTraceRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("CdtScalarTraceRow", 13)?;
        state.serialize_field("step", &self.step)?;
        state.serialize_field("outcome", &self.payload.outcome())?;
        state.serialize_field("log_prob", &self.log_prob)?;
        state.serialize_field("action", &self.action)?;
        state.serialize_field("vertices", &self.vertices)?;
        state.serialize_field("edges", &self.edges)?;
        state.serialize_field("triangles", &self.triangles)?;
        state.serialize_field("move_type", &self.move_type)?;
        state.serialize_field("delta_action", &self.payload.delta_action())?;
        state.serialize_field("action_before", &self.action_before)?;
        state.serialize_field("action_after", &self.payload.action_after())?;
        state.serialize_field("seed", &self.seed)?;
        state.serialize_field("volume_profile", &self.volume_profile)?;
        state.end()
    }
}

impl CdtScalarTraceRow {
    /// Captures one post-step CDT scalar trace row.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimplexCount`] if the triangulation reports zero
    /// vertices, edges, or triangles at the completed step,
    /// [`CdtError::MeasurementCountOverflow`] if any live count cannot fit the
    /// compact trace storage type, or [`CdtError::CheckpointResumeFailed`] if a
    /// scalar trace field is non-finite.
    #[expect(
        clippy::too_many_arguments,
        reason = "trace rows mirror the upstream step outcome plus CDT scalar observables"
    )]
    pub(crate) fn new(
        step: NonZeroU32,
        outcome: CdtScalarTraceOutcome,
        log_prob: f64,
        action: f64,
        triangulation: &CdtTriangulation2D,
        move_type: MoveType,
        delta_action: Option<f64>,
        action_before: f64,
        action_after: Option<f64>,
        seed: Option<u64>,
    ) -> CdtResult<Self> {
        let counts = triangulation.simplex_counts()?;
        let payload = CdtScalarTracePayload::from_parts(step, outcome, delta_action, action_after)?;
        let row = Self {
            step,
            payload,
            log_prob,
            action,
            vertices: nonzero_usize_measurement_count(
                MeasurementCountField::Vertices,
                counts.vertex_count(),
            )?,
            edges: nonzero_usize_measurement_count(
                MeasurementCountField::Edges,
                counts.edge_count(),
            )?,
            triangles: nonzero_usize_measurement_count(
                MeasurementCountField::Triangles,
                counts.triangle_count(),
            )?,
            move_type,
            action_before,
            seed,
            volume_profile: triangulation.volume_profile()?,
        };
        validate_scalar_trace_finite_fields(&row)?;
        Ok(row)
    }

    /// Converts the stored CDT outcome into the upstream trace outcome.
    const fn outcome(&self) -> TraceStepOutcome {
        self.payload.outcome().into_trace_outcome()
    }

    /// Returns observable values in scalar trace observable order.
    fn observable_values(&self, volume_profile_len: usize) -> Vec<f64> {
        let (seed_low, seed_high, seed_present) = seed_observables(self.seed);
        let delta_action = self.payload.delta_action();
        let action_after = self.payload.action_after();
        let mut values =
            Vec::with_capacity(SCALAR_TRACE_BASE_OBSERVABLES.len() + volume_profile_len);
        values.extend([
            self.action,
            f64::from(self.vertices.get()),
            f64::from(self.edges.get()),
            f64::from(self.triangles.get()),
            f64::from(move_type_code(self.move_type)),
            delta_action.unwrap_or(0.0),
            option_presence(delta_action),
            self.action_before,
            action_after.unwrap_or(0.0),
            option_presence(action_after),
            seed_low,
            seed_high,
            seed_present,
        ]);
        values.extend((0..volume_profile_len).map(|index| {
            self.volume_profile
                .get(index)
                .map_or(0.0, |&volume| f64::from(volume))
        }));
        values
    }
}

impl Measurement {
    /// Creates a measurement with an empty volume profile from validated counts.
    ///
    /// This constructor records scalar simulation counts. Attach per-slice
    /// volume data with [`Self::try_with_volume_profile`] when the measured
    /// triangulation has a foliation.
    ///
    /// Use [`Self::try_new`] when converting raw counts from serialized data,
    /// test fixtures, or other boundary inputs.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidMeasurementAction`] when `action` is not finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    /// use std::num::NonZeroU32;
    ///
    /// let unit_count = NonZeroU32::MIN;
    /// let measurement = Measurement::new(10, -12.5, unit_count, unit_count, unit_count)?;
    /// assert_eq!(measurement.step(), 10);
    /// assert_eq!(measurement.vertices().get(), 1);
    /// assert!(measurement.volume_profile().is_empty());
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    pub fn new(
        step: u32,
        action: f64,
        vertices: NonZeroU32,
        edges: NonZeroU32,
        triangles: NonZeroU32,
    ) -> CdtResult<Self> {
        validate_measurement_action(step, action)?;
        Ok(Self {
            step,
            action,
            vertices,
            edges,
            triangles,
            volume_profile: Vec::new(),
        })
    }

    /// Creates a measurement from raw scalar counts.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidMeasurementCount`] when any count is zero, or
    /// [`CdtError::InvalidMeasurementAction`] when `action` is not finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::try_new(10, -12.5, 64, 180, 117)?;
    /// assert_eq!(measurement.vertices().get(), 64);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    pub fn try_new(
        step: u32,
        action: f64,
        vertices: u32,
        edges: u32,
        triangles: u32,
    ) -> CdtResult<Self> {
        Self::new(
            step,
            action,
            nonzero_measurement_count(MeasurementCountField::Vertices, vertices)?,
            nonzero_measurement_count(MeasurementCountField::Edges, edges)?,
            nonzero_measurement_count(MeasurementCountField::Triangles, triangles)?,
        )
    }

    /// Creates a measurement from a validated CDT simplex-count snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::MeasurementCountOverflow`] when any live count exceeds
    /// `u32::MAX`, or [`CdtError::InvalidMeasurementAction`] when `action` is not
    /// finite.
    pub(crate) fn try_from_simplex_counts(
        step: u32,
        action: f64,
        counts: CdtSimplexCounts,
    ) -> CdtResult<Self> {
        Self::new(
            step,
            action,
            nonzero_usize_measurement_count(
                MeasurementCountField::Vertices,
                counts.vertex_count(),
            )?,
            nonzero_usize_measurement_count(MeasurementCountField::Edges, counts.edge_count())?,
            nonzero_usize_measurement_count(
                MeasurementCountField::Triangles,
                counts.triangle_count(),
            )?,
        )
    }

    /// Returns the Monte Carlo step when this measurement was taken.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::try_new(10, -12.5, 64, 180, 117)?;
    /// assert_eq!(measurement.step(), 10);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn step(&self) -> u32 {
        self.step
    }

    /// Returns the finite action value recorded at this measurement.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::try_new(10, -12.5, 64, 180, 117)?;
    /// assert_relative_eq!(measurement.action(), -12.5);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn action(&self) -> f64 {
        self.action
    }

    /// Returns the nonzero vertex count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::try_new(10, -12.5, 64, 180, 117)?;
    /// assert_eq!(measurement.vertices().get(), 64);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn vertices(&self) -> NonZeroU32 {
        self.vertices
    }

    /// Returns the nonzero edge count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::try_new(10, -12.5, 64, 180, 117)?;
    /// assert_eq!(measurement.edges().get(), 180);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn edges(&self) -> NonZeroU32 {
        self.edges
    }

    /// Returns the nonzero triangle count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::try_new(10, -12.5, 64, 180, 117)?;
    /// assert_eq!(measurement.triangles().get(), 117);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn triangles(&self) -> NonZeroU32 {
        self.triangles
    }

    /// Returns per-slice triangle counts `N₂(t)` by time slab.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement =
    ///     Measurement::try_new(10, -12.5, 64, 180, 117)?.try_with_volume_profile(vec![39, 39, 39])?;
    /// assert_eq!(measurement.volume_profile(), &[39, 39, 39]);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub fn volume_profile(&self) -> &[u32] {
        &self.volume_profile
    }

    /// Returns this measurement with a validated per-slice volume profile attached.
    ///
    /// The profile entries are triangle counts `N₂(t)` by time slab, matching
    /// [`CdtTriangulation::volume_profile`](crate::cdt::triangulation::CdtTriangulation::volume_profile).
    /// An empty profile represents a measurement whose triangulation has no
    /// current foliation. Non-empty profiles may include zero-count slices, but
    /// their total must not exceed [`Self::triangles`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidMeasurementVolumeProfile`] when the profile
    /// total exceeds this measurement's stored triangle count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement =
    ///     Measurement::try_new(20, -10.0, 12, 26, 12)?.try_with_volume_profile(vec![6, 6, 0])?;
    /// assert_eq!(measurement.volume_profile(), &[6, 6, 0]);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    pub fn try_with_volume_profile(mut self, volume_profile: Vec<u32>) -> CdtResult<Self> {
        validate_measurement_volume_profile(self.step, self.triangles, &volume_profile)?;
        self.volume_profile = volume_profile;
        Ok(self)
    }
}

/// Raw measurement shape used only while deserializing result and checkpoint payloads.
///
/// Serialized measurements stay JSON-numeric for compatibility with analysis
/// tools, while conversion through [`Measurement::try_new`] and
/// [`Measurement::try_with_volume_profile`] rejects zero counts, non-finite
/// actions, and impossible volume-profile totals before the public
/// [`Measurement`] stores them.
#[derive(Deserialize)]
struct MeasurementWire {
    step: u32,
    action: f64,
    vertices: u32,
    edges: u32,
    triangles: u32,
    volume_profile: Vec<u32>,
}

impl TryFrom<MeasurementWire> for Measurement {
    type Error = CdtError;

    fn try_from(wire: MeasurementWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.step,
            wire.action,
            wire.vertices,
            wire.edges,
            wire.triangles,
        )?
        .try_with_volume_profile(wire.volume_profile)
    }
}

impl<'de> Deserialize<'de> for Measurement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        MeasurementWire::deserialize(deserializer)?
            .try_into()
            .map_err(DeError::custom)
    }
}

/// Parses a raw measurement count into its nonzero domain type.
///
fn nonzero_measurement_count(field: MeasurementCountField, value: u32) -> CdtResult<NonZeroU32> {
    NonZeroU32::new(value).ok_or(CdtError::InvalidMeasurementCount {
        field,
        provided_value: value,
    })
}

/// Parses a raw scalar trace count into its nonzero domain type.
fn nonzero_scalar_trace_count(field: MeasurementCountField, value: u32) -> CdtResult<NonZeroU32> {
    NonZeroU32::new(value).ok_or(CdtError::InvalidScalarTraceCount {
        field,
        provided_value: value,
    })
}

/// Rejects scalar trace profiles that cannot fit the stored triangle count.
fn validate_scalar_trace_volume_profile(
    step: NonZeroU32,
    triangles: NonZeroU32,
    volume_profile: &[u32],
) -> CdtResult<()> {
    let Some(profile_total) = volume_profile_total(volume_profile) else {
        return Ok(());
    };
    if profile_total > u64::from(triangles.get()) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceVolumeProfileExceedsTriangles {
                step: step.get(),
                profile_total,
                triangles: triangles.get(),
            },
        ));
    }
    Ok(())
}

/// Rejects measurement profiles that cannot fit the stored triangle count.
fn validate_measurement_volume_profile(
    step: u32,
    triangles: NonZeroU32,
    volume_profile: &[u32],
) -> CdtResult<()> {
    let Some(profile_total) = volume_profile_total(volume_profile) else {
        return Ok(());
    };
    if profile_total > u64::from(triangles.get()) {
        return Err(CdtError::InvalidMeasurementVolumeProfile {
            step,
            profile_total,
            triangles: triangles.get(),
        });
    }
    Ok(())
}

/// Sums non-empty volume profiles in a wider type.
fn volume_profile_total(volume_profile: &[u32]) -> Option<u64> {
    if volume_profile.is_empty() {
        return None;
    }
    Some(volume_profile.iter().map(|&volume| u64::from(volume)).sum())
}

fn nonzero_usize_measurement_count(
    field: MeasurementCountField,
    value: usize,
) -> CdtResult<NonZeroU32> {
    let value = u32::try_from(value).map_err(|_| CdtError::MeasurementCountOverflow {
        field,
        provided_value: value,
        max: u32::MAX,
    })?;
    nonzero_measurement_count(field, value)
}

/// Checks that a measurement action is finite before storage.
const fn validate_measurement_action(step: u32, action: f64) -> CdtResult<()> {
    if action.is_finite() {
        Ok(())
    } else {
        Err(CdtError::InvalidMeasurementAction {
            step,
            provided_value: action,
        })
    }
}

/// Complete output from a Metropolis-Hastings CDT simulation.
///
/// Values are produced by [`MetropolisAlgorithm::run`](crate::cdt::metropolis::MetropolisAlgorithm::run)
/// and include raw Monte Carlo steps, recorded measurements, final geometry,
/// and convenience methods for common post-simulation summaries.
///
/// Serde serialization preserves the complete result object, including the
/// final triangulation checkpoint. Use [`Self::write_summary_json`] when you
/// want an analysis-friendly JSON report with configuration, aggregate
/// statistics, step telemetry, and measurements. Deserialization validates the
/// stored configuration, action parameters, and final evolved-CDT triangulation
/// invariants, mapping any validation failure into the serde deserializer error.
#[derive(Debug, Serialize)]
pub struct SimulationResultsBackend {
    /// Configuration used for the simulation
    config: MetropolisConfig,
    /// Action configuration used
    action_config: ActionConfig,
    /// Metropolis-level ergodic move statistics
    move_stats: MoveStatistics,
    /// Concrete proposal-kernel telemetry
    #[serde(default)]
    proposal_stats: ProposalStatistics,
    /// All Monte Carlo steps performed
    steps: Vec<MonteCarloStep>,
    /// Measurements taken during simulation
    measurements: Vec<Measurement>,
    /// Upstream-compatible scalar trace rows for completed Metropolis steps
    scalar_trace_rows: Vec<CdtScalarTraceRow>,
    /// Total simulation time
    elapsed_time: Duration,
    /// Final triangulation state
    triangulation: CdtTriangulation2D,
}

#[derive(Deserialize)]
struct SimulationResultsBackendWire {
    config: MetropolisConfig,
    action_config: ActionConfig,
    move_stats: MoveStatistics,
    #[serde(default)]
    proposal_stats: ProposalStatistics,
    steps: Vec<MonteCarloStep>,
    measurements: Vec<Measurement>,
    scalar_trace_rows: Vec<CdtScalarTraceRow>,
    elapsed_time: Duration,
    triangulation: CdtTriangulation2D,
}

/// Validated components for constructing a simulation result snapshot.
pub(crate) struct SimulationResultsParts {
    config: MetropolisConfig,
    action_config: ActionConfig,
    move_stats: MoveStatistics,
    proposal_stats: ProposalStatistics,
    steps: Vec<MonteCarloStep>,
    measurements: Vec<Measurement>,
    scalar_trace_rows: Vec<CdtScalarTraceRow>,
    elapsed_time: Duration,
    triangulation: CdtTriangulation2D,
}

impl SimulationResultsParts {
    /// Parses raw result components into a validated result-parts proof.
    #[expect(
        clippy::too_many_arguments,
        reason = "result snapshots are assembled from independent telemetry streams"
    )]
    pub(crate) fn new(
        config: MetropolisConfig,
        action_config: ActionConfig,
        move_stats: MoveStatistics,
        proposal_stats: ProposalStatistics,
        steps: Vec<MonteCarloStep>,
        measurements: Vec<Measurement>,
        scalar_trace_rows: Vec<CdtScalarTraceRow>,
        elapsed_time: Duration,
        triangulation: CdtTriangulation2D,
    ) -> CdtResult<Self> {
        config.validate();
        action_config.validate();
        triangulation.validate_evolved_cdt()?;
        validate_result_telemetry(
            &config,
            &move_stats,
            &proposal_stats,
            &steps,
            &measurements,
            &scalar_trace_rows,
        )?;
        Ok(Self {
            config,
            action_config,
            move_stats,
            proposal_stats,
            steps,
            measurements,
            scalar_trace_rows,
            elapsed_time,
            triangulation,
        })
    }
}

impl TryFrom<SimulationResultsBackendWire> for SimulationResultsBackend {
    type Error = CdtError;

    fn try_from(wire: SimulationResultsBackendWire) -> Result<Self, Self::Error> {
        wire.config.validate();
        wire.action_config.validate();
        wire.triangulation.validate_evolved_cdt()?;
        Self::new(
            wire.config,
            wire.action_config,
            wire.move_stats,
            wire.proposal_stats,
            wire.steps,
            wire.measurements,
            wire.scalar_trace_rows,
            wire.elapsed_time,
            wire.triangulation,
        )
    }
}

impl<'de> Deserialize<'de> for SimulationResultsBackend {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SimulationResultsBackendWire::deserialize(deserializer)?
            .try_into()
            .map_err(DeError::custom)
    }
}

/// Validates that public result snapshots contain coherent chain telemetry.
///
/// This protects the [`SimulationResultsBackend`] contract when data crosses a
/// serialization boundary or is assembled by crate-internal tests: step records,
/// measurements, move counters, and proposal counters must describe the same
/// completed chain, or the initial construction snapshot produced when callers
/// build a triangulation without running Metropolis sampling.
fn validate_result_telemetry(
    config: &MetropolisConfig,
    move_stats: &MoveStatistics,
    proposal_stats: &ProposalStatistics,
    steps: &[MonteCarloStep],
    measurements: &[Measurement],
    scalar_trace_rows: &[CdtScalarTraceRow],
) -> CdtResult<()> {
    if is_initial_construction_snapshot(
        move_stats,
        proposal_stats,
        steps,
        measurements,
        scalar_trace_rows,
    ) {
        return Ok(());
    }

    let expected_steps = usize::try_from(config.steps().get()).unwrap_or(usize::MAX);
    if steps.len() != expected_steps {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::StepTelemetryLengthMismatch {
                actual: steps.len(),
                expected: expected_steps,
            },
        ));
    }

    let (accepted, rejected) = chain_counters(move_stats)?;
    let total_moves = accepted.checked_add(rejected).ok_or_else(|| {
        checkpoint_resume_failed(CheckpointResumeFailure::CounterConversionOverflow {
            counter: CheckpointMoveCounter::Attempted,
        })
    })?;
    if total_moves != steps.len() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::StepTelemetryLengthMismatch {
                actual: steps.len(),
                expected: total_moves,
            },
        ));
    }

    validate_result_steps(steps, accepted)?;
    validate_result_measurements(config, steps, measurements)?;
    validate_result_proposal_stats(proposal_stats, steps.len(), accepted, rejected)?;
    validate_scalar_trace_rows(config, proposal_stats, steps, scalar_trace_rows)
}

/// Recognizes a construction-only result before Metropolis steps have run.
fn is_initial_construction_snapshot(
    move_stats: &MoveStatistics,
    proposal_stats: &ProposalStatistics,
    steps: &[MonteCarloStep],
    measurements: &[Measurement],
    scalar_trace_rows: &[CdtScalarTraceRow],
) -> bool {
    steps.is_empty()
        && scalar_trace_rows.is_empty()
        && move_stats.total_attempted() == 0
        && move_stats.total_accepted() == 0
        && move_stats.total_hard_failures() == 0
        && proposal_stats.move_family_proposals() == 0
        && proposal_stats.observed_forward_sites() == 0
        && proposal_stats.rejected_transitions() == 0
        && proposal_stats.accepted_transitions() == 0
        && proposal_stats.hard_failures() == 0
        && matches!(measurements, [measurement] if measurement.step() == 0)
}

/// Checks per-step records against accepted-count and action-delta invariants.
fn validate_result_steps(steps: &[MonteCarloStep], accepted: usize) -> CdtResult<()> {
    let accepted_steps = steps.iter().filter(|step| step.accepted()).count();
    if accepted_steps != accepted {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::StepTelemetryAcceptedCountMismatch {
                actual: accepted_steps,
                expected: accepted,
            },
        ));
    }

    for (index, step) in steps.iter().enumerate() {
        let expected_step = u32::try_from(index + 1).map_err(|_| {
            checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryIndexOverflow)
        })?;
        let step_number = step.step().get();
        if step_number != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::StepTelemetrySequenceMismatch {
                    actual: step_number,
                    expected: expected_step,
                },
            ));
        }
    }
    Ok(())
}

/// Checks post-thermalization measurement cadence and finite measured actions.
///
/// This mirrors the runner's measurement schedule so result deserialization
/// rejects samples from the thermalization window instead of treating them as
/// equilibrium data.
fn validate_result_measurements(
    config: &MetropolisConfig,
    steps: &[MonteCarloStep],
    measurements: &[Measurement],
) -> CdtResult<()> {
    let current_step = u32::try_from(steps.len()).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryIndexOverflow)
    })?;
    let expected_measurements = expected_measurement_count(
        current_step,
        config.thermalization_steps(),
        config.measurement_frequency(),
    )
    .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::MeasurementCountOverflow))?;
    if measurements.len() != expected_measurements {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::MeasurementCountMismatch {
                actual: measurements.len(),
                expected: expected_measurements,
            },
        ));
    }

    for (index, measurement) in measurements.iter().enumerate() {
        let expected_step = expected_measurement_step(
            index,
            config.thermalization_steps(),
            config.measurement_frequency(),
        )
        .ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::MeasurementStepOverflow)
        })?;
        if measurement.step() != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::MeasurementStepMismatch {
                    actual: measurement.step(),
                    expected: expected_step,
                },
            ));
        }
    }
    Ok(())
}

/// Checks proposal counters against step and move-counter outcomes.
///
/// Completed result snapshots must have one terminal proposal outcome per
/// recorded step and no hard failures. This keeps public
/// [`Self::proposal_stats`](SimulationResultsBackend::proposal_stats) accessors
/// from exposing counters that cannot describe the recorded chain.
fn validate_result_proposal_stats(
    proposal_stats: &ProposalStatistics,
    steps: usize,
    accepted: usize,
    rejected: usize,
) -> CdtResult<()> {
    if proposal_stats.hard_failures() != 0 {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalHardFailures {
                actual: proposal_stats.hard_failures(),
            },
        ));
    }
    let steps = u64::try_from(steps).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
            counter: ProposalTelemetryCounter::MoveFamilyProposals,
        })
    })?;
    if proposal_stats.move_family_proposals() != steps {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalMoveFamilyCountMismatch {
                actual: proposal_stats.move_family_proposals(),
                expected: steps,
            },
        ));
    }

    let accepted = u64::try_from(accepted).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
            counter: ProposalTelemetryCounter::AcceptedTransitions,
        })
    })?;
    if proposal_stats.accepted_transitions() != accepted {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalAcceptedCountMismatch {
                actual: proposal_stats.accepted_transitions(),
                expected: accepted,
            },
        ));
    }

    let rejected = u64::try_from(rejected).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
            counter: ProposalTelemetryCounter::RejectedTransitions,
        })
    })?;
    let actual_rejected = proposal_stats.rejected_transitions();
    if actual_rejected != rejected {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalRejectedCountMismatch {
                actual: actual_rejected,
                expected: rejected,
            },
        ));
    }
    Ok(())
}

/// Checks scalar trace rows against step and proposal telemetry.
///
/// Trace rows are serialized result/checkpoint state, so they are validated at
/// the same boundary as steps, measurements, and proposal counters before later
/// code can trust them for CSV export.
pub(crate) fn validate_scalar_trace_rows(
    config: &MetropolisConfig,
    proposal_stats: &ProposalStatistics,
    steps: &[MonteCarloStep],
    scalar_trace_rows: &[CdtScalarTraceRow],
) -> CdtResult<()> {
    if scalar_trace_rows.len() != steps.len() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceLengthMismatch {
                actual: scalar_trace_rows.len(),
                expected: steps.len(),
            },
        ));
    }

    let mut accepted = 0_u64;
    let mut rejected_proposal = 0_u64;
    let mut no_proposal = 0_u64;
    for (step, row) in steps.iter().zip(scalar_trace_rows) {
        validate_scalar_trace_row(config, step, row)?;
        match row.payload.outcome() {
            CdtScalarTraceOutcome::Accepted => accepted = accepted.saturating_add(1),
            CdtScalarTraceOutcome::RejectedProposal => {
                rejected_proposal = rejected_proposal.saturating_add(1);
            }
            CdtScalarTraceOutcome::NoProposal => no_proposal = no_proposal.saturating_add(1),
        }
    }

    if accepted != proposal_stats.accepted_transitions() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceAcceptedCountMismatch {
                actual: accepted,
                expected: proposal_stats.accepted_transitions(),
            },
        ));
    }
    if rejected_proposal != proposal_stats.metropolis_rejections() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceRejectedProposalCountMismatch {
                actual: rejected_proposal,
                expected: proposal_stats.metropolis_rejections(),
            },
        ));
    }
    let expected_no_proposal = scalar_trace_no_proposal_count(proposal_stats)?;
    if no_proposal != expected_no_proposal {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceNoProposalCountMismatch {
                actual: no_proposal,
                expected: expected_no_proposal,
            },
        ));
    }

    Ok(())
}

/// Checks one scalar trace row against the corresponding step telemetry.
fn validate_scalar_trace_row(
    config: &MetropolisConfig,
    step: &MonteCarloStep,
    row: &CdtScalarTraceRow,
) -> CdtResult<()> {
    let step_number = step.step().get();
    let row_step = row.step.get();
    if row.step != step.step() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceStepMismatch {
                actual: row_step,
                expected: step_number,
            },
        ));
    }
    if row.move_type != step.move_type() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceMoveTypeMismatch {
                step: step_number,
                actual: row.move_type,
                expected: step.move_type(),
            },
        ));
    }
    let expected_outcome = scalar_trace_outcome_from_step(step);
    if row.payload.outcome() != expected_outcome {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceAcceptedMismatch {
                step: step_number,
                actual: row.payload.outcome(),
                expected: expected_outcome,
            },
        ));
    }
    if row.seed != config.seed() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceSeedMismatch {
                step: step_number,
                actual: row.seed,
                expected: config.seed(),
            },
        ));
    }
    if !actions_match(row.action_before, step.action_before()) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceActionBeforeMismatch { step: step_number },
        ));
    }
    if !optional_actions_match(row.payload.delta_action(), step.delta_action()) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceDeltaActionMismatch { step: step_number },
        ));
    }
    if !optional_actions_match(row.payload.action_after(), step.action_after()) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceActionAfterMismatch { step: step_number },
        ));
    }
    let expected_action = step.action_after().unwrap_or_else(|| step.action_before());
    if !actions_match(row.action, expected_action) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceActionMismatch { step: step_number },
        ));
    }
    let expected_log_prob = -row.action / config.temperature();
    if !actions_match(row.log_prob, expected_log_prob) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceLogProbMismatch { step: step_number },
        ));
    }
    Ok(())
}

const fn scalar_trace_outcome_from_step(step: &MonteCarloStep) -> CdtScalarTraceOutcome {
    match step.outcome() {
        MonteCarloStepOutcome::Accepted(_) => CdtScalarTraceOutcome::Accepted,
        MonteCarloStepOutcome::RejectedProposal(_) => CdtScalarTraceOutcome::RejectedProposal,
        MonteCarloStepOutcome::NoProposal => CdtScalarTraceOutcome::NoProposal,
    }
}

/// Checks finite scalar trace numeric fields.
fn validate_scalar_trace_finite_fields(row: &CdtScalarTraceRow) -> CdtResult<()> {
    validate_scalar_trace_finite(row.step, ScalarTraceField::LogProb, row.log_prob)?;
    validate_scalar_trace_finite(row.step, ScalarTraceField::Action, row.action)?;
    validate_scalar_trace_finite(row.step, ScalarTraceField::ActionBefore, row.action_before)?;
    if let Some(delta_action) = row.payload.delta_action() {
        validate_scalar_trace_finite(row.step, ScalarTraceField::DeltaAction, delta_action)?;
    }
    if let Some(action_after) = row.payload.action_after() {
        validate_scalar_trace_finite(row.step, ScalarTraceField::ActionAfter, action_after)?;
    }
    Ok(())
}

/// Checks one finite scalar trace value.
const fn validate_scalar_trace_finite(
    step: NonZeroU32,
    field: ScalarTraceField,
    value: f64,
) -> CdtResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(checkpoint_resume_failed(
            CheckpointResumeFailure::NonFiniteScalarTraceValue {
                step: step.get(),
                field,
            },
        ))
    }
}

/// Compares optional action-like telemetry with the configured float tolerance.
fn optional_actions_match(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => actions_match(left, right),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

/// Computes aggregate no-proposal trace outcomes from proposal telemetry.
fn scalar_trace_no_proposal_count(proposal_stats: &ProposalStatistics) -> CdtResult<u64> {
    [
        proposal_stats.no_site_proposals(),
        proposal_stats.site_causality_rejections(),
        proposal_stats.site_geometric_rejections(),
        proposal_stats.site_backend_rejections(),
    ]
    .into_iter()
    .try_fold(0_u64, |total, count| {
        total.checked_add(count).ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
                counter: ProposalTelemetryCounter::RejectedTransitions,
            })
        })
    })
}

/// Converts a known CDT step number into an upstream trace row index.
fn step_to_trace_index(step: NonZeroU32) -> usize {
    usize::try_from(step.get()).unwrap_or(usize::MAX)
}

/// Stable numeric move-family codes used in scalar trace exports.
const fn move_type_code(move_type: MoveType) -> u32 {
    match move_type {
        MoveType::Move22 => 22,
        MoveType::Move13Add => 13,
        MoveType::Move31Remove => 31,
        MoveType::EdgeFlip => 1,
    }
}

/// Encodes optional numeric fields without introducing blank cells into trace CSV.
const fn option_presence(value: Option<f64>) -> f64 {
    if value.is_some() { 1.0 } else { 0.0 }
}

/// Splits an optional seed into exactly representable numeric columns.
fn seed_observables(seed: Option<u64>) -> (f64, f64, f64) {
    let Some(seed) = seed else {
        return (0.0, 0.0, 0.0);
    };
    let low = seed & u64::from(u32::MAX);
    let high = seed >> 32;
    let low = u32::try_from(low).unwrap_or(u32::MAX);
    let high = u32::try_from(high).unwrap_or(u32::MAX);
    (f64::from(low), f64::from(high), 1.0)
}

#[derive(Serialize)]
struct SimulationSummary<'a> {
    config: &'a CdtConfig,
    metropolis_config: &'a MetropolisConfig,
    action_config: &'a ActionConfig,
    move_stats: &'a MoveStatistics,
    proposal_stats: &'a ProposalStatistics,
    aggregate: AggregateSummary,
    final_triangulation: TriangulationSummary,
    steps: &'a [MonteCarloStep],
    measurements: &'a [Measurement],
}

#[derive(Serialize)]
struct AggregateSummary {
    acceptance_rate: f64,
    average_action: f64,
    elapsed_time_ms: u128,
    measurement_count: usize,
    step_count: usize,
    average_volume_profile: Vec<f64>,
    volume_fluctuations: Vec<f64>,
}

/// Serializable final-triangulation section for summary JSON output.
///
/// The scalar counts keep the existing summary shape, while `mesh` carries the
/// coordinate and connectivity payload added for notebook visualization and
/// downstream analysis consumers.
#[derive(Serialize)]
struct TriangulationSummary {
    vertices: usize,
    edges: usize,
    triangles: usize,
    time_slices: u32,
    topology: CdtTopology,
    mesh: TriangulationMeshSummary,
}

/// Serializable mesh payload embedded in summary JSON output.
///
/// The payload is intentionally backend-handle-free: vertices are assigned
/// stable zero-based indices, and triangle records reference those indices so
/// notebook and visualization consumers can reconstruct the final triangulation
/// without depending on backend handle formatting or allocation order.
#[derive(Serialize)]
struct TriangulationMeshSummary {
    /// Coordinate column names matching each vertex coordinate vector.
    coordinate_columns: [&'static str; 2],
    /// Stable vertex table used by triangle index triples.
    vertices: Vec<TriangulationVertexSummary>,
    /// Triangle vertex indices into [`Self::vertices`].
    triangles: Vec<[usize; 3]>,
}

/// Serializable vertex row used by [`TriangulationMeshSummary`].
///
/// Each row preserves the validated spacetime coordinate and optional CDT time
/// label for one final-state vertex. The separate `index` field is the stable
/// value referenced by triangle triples in summary JSON.
#[derive(Serialize)]
struct TriangulationVertexSummary {
    /// Stable zero-based index used by triangle references.
    index: usize,
    /// Backend spacetime coordinates for external visualization and mesh consumers.
    coordinates: SpacetimeCoordinate,
    /// Optional CDT time label stored on the vertex.
    time: Option<u32>,
}

/// Builds a stable final-triangulation mesh payload for summary JSON output.
///
/// The vertex table is sorted by CDT time label and backend coordinates so
/// downstream notebooks can consume the summary without depending on backend
/// handle allocation order.
fn triangulation_mesh_summary(
    triangulation: &CdtTriangulation2D,
) -> CdtResult<TriangulationMeshSummary> {
    let geometry = triangulation.geometry();
    let mut vertices = Vec::with_capacity(geometry.vertex_count());
    for handle in geometry.vertices() {
        let coordinates =
            geometry
                .vertex_coordinates(&handle)
                .map_err(|err| CdtError::ValidationFailed {
                    check: CdtValidationCheck::Geometry,
                    failure: CdtValidationFailure::VertexCoordinateReadFailed {
                        vertex: format!("{handle:?}"),
                        detail: err.to_string(),
                    },
                })?;
        let coordinate =
            SpacetimeCoordinate::try_from_space_time_slice(&coordinates).map_err(|err| {
                CdtError::ValidationFailed {
                    check: CdtValidationCheck::Geometry,
                    failure: CdtValidationFailure::from_spacetime_coordinate_error(
                        format!("{handle:?}"),
                        err,
                    ),
                }
            })?;
        let time = triangulation.time_label(&handle);
        vertices.push((handle, coordinate, time));
    }

    vertices.sort_by(|left, right| {
        left.2
            .cmp(&right.2)
            .then_with(|| left.1.time().total_cmp(&right.1.time()))
            .then_with(|| left.1.space().total_cmp(&right.1.space()))
    });

    let mut vertex_indices = HashMap::with_capacity(vertices.len());
    let mut vertex_summaries = Vec::with_capacity(vertices.len());
    for (index, (handle, coordinates, time)) in vertices.into_iter().enumerate() {
        vertex_indices.insert(handle, index);
        vertex_summaries.push(TriangulationVertexSummary {
            index,
            coordinates,
            time,
        });
    }

    let mut triangles = Vec::with_capacity(geometry.face_count());
    for face in geometry.faces() {
        let face_vertices =
            geometry
                .face_vertices(&face)
                .map_err(|err| CdtError::ValidationFailed {
                    check: CdtValidationCheck::Geometry,
                    failure: CdtValidationFailure::FaceVerticesUnavailable {
                        face: format!("{face:?}"),
                        detail: err.to_string(),
                    },
                })?;
        if face_vertices.len() != 3 {
            return Err(CdtError::ValidationFailed {
                check: CdtValidationCheck::Geometry,
                failure: CdtValidationFailure::FaceVertexCount {
                    face: format!("{face:?}"),
                    actual: face_vertices.len(),
                    expected: 3,
                },
            });
        }

        let mut triangle = [0_usize; 3];
        for (position, vertex) in face_vertices.iter().enumerate() {
            let Some(index) = vertex_indices.get(vertex) else {
                return Err(CdtError::ValidationFailed {
                    check: CdtValidationCheck::Geometry,
                    failure: CdtValidationFailure::FaceVerticesUnavailable {
                        face: format!("{face:?}"),
                        detail: format!(
                            "face references vertex {vertex:?}, but it is absent from the vertex table"
                        ),
                    },
                });
            };
            triangle[position] = *index;
        }
        triangles.push(triangle);
    }
    triangles.sort_unstable();

    Ok(TriangulationMeshSummary {
        coordinate_columns: SpacetimeCoordinate::coordinate_columns(),
        vertices: vertex_summaries,
        triangles,
    })
}

impl SimulationResultsBackend {
    /// Creates a validated simulation result snapshot from explicit components.
    ///
    /// This constructor is crate-internal because the component set is
    /// invariant-heavy: move counters, proposal counters, step telemetry,
    /// measurement cadence, and final triangulation state must all describe the
    /// same completed chain. Public callers normally obtain results from
    /// [`MetropolisAlgorithm`](crate::cdt::metropolis::MetropolisAlgorithm) or
    /// from deserializing a previously emitted result snapshot.
    ///
    /// The supplied [`MoveStatistics`] and [`ProposalStatistics`] are preserved
    /// verbatim after validation so serialized telemetry keeps the same wire
    /// shape as results produced by the Metropolis runner.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if `config` is not a
    /// runnable Metropolis configuration, or [`CdtError::InvalidConfiguration`] if
    /// `action_config` contains non-finite couplings.
    ///
    /// The final triangulation is checked with the crate's evolved-CDT validation
    /// path, so this can also return backend validation, topology, foliation,
    /// causality, or simplex-classification errors, including
    /// [`CdtError::DelaunayValidationFailed`], [`CdtError::TopologyMismatch`],
    /// [`CdtError::Foliation`], [`CdtError::CausalityViolation`], and
    /// [`CdtError::ValidationFailed`].
    ///
    #[expect(
        clippy::too_many_arguments,
        reason = "constructor mirrors the serialized simulation result components"
    )]
    pub(crate) fn new(
        config: MetropolisConfig,
        action_config: ActionConfig,
        move_stats: MoveStatistics,
        proposal_stats: ProposalStatistics,
        steps: Vec<MonteCarloStep>,
        measurements: Vec<Measurement>,
        scalar_trace_rows: Vec<CdtScalarTraceRow>,
        elapsed_time: Duration,
        triangulation: CdtTriangulation2D,
    ) -> CdtResult<Self> {
        Ok(Self::from_parts(SimulationResultsParts::new(
            config,
            action_config,
            move_stats,
            proposal_stats,
            steps,
            measurements,
            scalar_trace_rows,
            elapsed_time,
            triangulation,
        )?))
    }

    /// Creates a result snapshot from components that were already validated by this crate.
    pub(crate) fn from_parts(parts: SimulationResultsParts) -> Self {
        Self {
            config: parts.config,
            action_config: parts.action_config,
            move_stats: parts.move_stats,
            proposal_stats: parts.proposal_stats,
            steps: parts.steps,
            measurements: parts.measurements,
            scalar_trace_rows: parts.scalar_trace_rows,
            elapsed_time: parts.elapsed_time,
            triangulation: parts.triangulation,
        }
    }

    /// Returns the Metropolis configuration used for this result.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7);
    ///     let results = MetropolisAlgorithm::new(
    ///         config.clone(),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.config(), &config);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn config(&self) -> &MetropolisConfig {
        &self.config
    }

    /// Returns the action configuration used for this result.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let action_config = ActionConfig::new(1.0, 0.0, 0.1)?;
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         action_config.clone(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.action_config(), &action_config);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn action_config(&self) -> &ActionConfig {
        &self.action_config
    }

    /// Returns Metropolis-level ergodic move statistics.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.move_stats().total_attempted(), 1);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn move_stats(&self) -> &MoveStatistics {
        &self.move_stats
    }

    /// Returns concrete proposal-kernel telemetry.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///
    ///     assert_eq!(results.proposal_stats().move_family_proposals(), 1);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn proposal_stats(&self) -> &ProposalStatistics {
        &self.proposal_stats
    }

    /// Returns recorded Monte Carlo step telemetry.
    ///
    /// These entries describe completed Metropolis transitions and therefore
    /// start at step 1. Step-0 construction or initial-state snapshots are
    /// represented by [`Measurement`] values returned from [`Self::measurements`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.steps().len(), 1);
    ///     assert_eq!(results.steps()[0].step().get(), 1);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn steps(&self) -> &[MonteCarloStep] {
        &self.steps
    }

    /// Returns recorded measurements.
    ///
    /// For Metropolis runs, measurements are recorded on the configured cadence
    /// at or after [`MetropolisConfig::thermalization_steps`]. Construction-only
    /// results produced by [`run_simulation`](crate::run_simulation) with
    /// simulation disabled contain a single step-0 construction snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.measurements().len(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Builds an upstream scalar trace for every completed Metropolis step.
    ///
    /// The trace uses chain id `0` and stores upstream accept/proposal metadata in
    /// the fixed trace columns. CDT observables are numeric and rectangular:
    /// current action, vertex/edge/triangle counts, stable move-family code
    /// (`22`, `13`, `31`, or `1` for edge flip), action-delta fields, the RNG
    /// seed split into exactly representable `u32` halves when available, and
    /// zero-filled `volume_profile_*` columns for per-slice triangle counts.
    ///
    /// # Errors
    ///
    /// Returns [`TraceError`] if the upstream trace header or row widths are
    /// rejected.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtError, CdtResult, CdtTriangulation, MetropolisAlgorithm,
    ///     MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///
    ///     let trace = results.scalar_trace().map_err(|err| CdtError::OutputWriteFailed {
    ///         path: "in-memory scalar trace".to_string(),
    ///         format: causal_triangulations::prelude::errors::OutputFormat::Csv,
    ///         detail: err.to_string(),
    ///     })?;
    ///     assert_eq!(trace.len(), results.steps().len());
    ///     Ok(())
    /// }
    /// ```
    pub fn scalar_trace(&self) -> Result<Trace, TraceError> {
        let mut observable_names = SCALAR_TRACE_BASE_OBSERVABLES
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let volume_profile_len = self
            .scalar_trace_rows
            .iter()
            .map(|row| row.volume_profile.len())
            .max()
            .unwrap_or(0);
        observable_names
            .extend((0..volume_profile_len).map(|index| format!("volume_profile_{index}")));

        let mut trace = Trace::new(observable_names)?;
        for row in &self.scalar_trace_rows {
            trace.push(TraceRecord::new(
                CDT_TRACE_CHAIN_ID,
                step_to_trace_index(row.step),
                row.outcome(),
                row.log_prob,
                row.observable_values(volume_profile_len),
            ))?;
        }
        Ok(trace)
    }

    /// Returns total wall-clock time recorded for the run.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     let _elapsed = results.elapsed_time();
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn elapsed_time(&self) -> Duration {
        self.elapsed_time
    }

    /// Returns the final triangulation state.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.triangulation().slice_sizes(), &[4, 4, 4]);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn triangulation(&self) -> &CdtTriangulation2D {
        &self.triangulation
    }

    /// Calculates the acceptance rate for the simulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert!((0.0..=1.0).contains(&results.acceptance_rate()));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn acceptance_rate(&self) -> f64 {
        if self.steps.is_empty() {
            return 0.0;
        }

        let accepted_count = self.steps.iter().filter(|step| step.accepted()).count();
        let total_count = self.steps.len();

        let Some(accepted_f64) = usize_to_f64(accepted_count) else {
            return 0.0;
        };
        let Some(total_f64) = usize_to_f64(total_count) else {
            return 0.0;
        };

        accepted_f64 / total_f64
    }

    /// Calculates the average action over all measurements.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert!(results.average_action().is_finite());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn average_action(&self) -> f64 {
        if self.measurements.is_empty() {
            return 0.0;
        }

        let sum: f64 = self.measurements.iter().map(Measurement::action).sum();
        let count = self.measurements.len();

        let Some(count_f64) = usize_to_f64(count) else {
            return 0.0;
        };

        sum / count_f64
    }

    /// Averages [`Measurement::volume_profile`] values after thermalization.
    ///
    /// The result has one entry per measured time slice.  Missing entries in a
    /// measurement are treated as zero, which keeps unfoliated simulations
    /// represented by an empty profile rather than a partially inferred one.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = MetropolisConfig::new(1.0, 20, 10, 5)?.with_seed(7);
    ///     let results = MetropolisAlgorithm::new(
    ///         config,
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     let profile = results.average_volume_profile();
    ///     assert_eq!(profile.len(), results.triangulation().slice_sizes().len());
    ///     assert!(profile.iter().all(|volume| volume.is_finite()));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn average_volume_profile(&self) -> Vec<f64> {
        let mut measurement_count = 0_usize;
        let mut profile_len = 0_usize;
        for measurement in self.equilibrium_measurements_iter() {
            measurement_count += 1;
            profile_len = profile_len.max(measurement.volume_profile().len());
        }
        if measurement_count == 0 || profile_len == 0 {
            return Vec::new();
        }

        let mut sums = vec![0.0; profile_len];
        for measurement in self.equilibrium_measurements_iter() {
            for (index, &volume) in measurement.volume_profile().iter().enumerate() {
                sums[index] += <f64 as From<u32>>::from(volume);
            }
        }

        let Some(count) = usize_to_f64(measurement_count) else {
            return Vec::new();
        };
        sums.into_iter().map(|sum| sum / count).collect()
    }

    /// Computes per-slice standard deviations of [`Measurement::volume_profile`].
    ///
    /// The sample standard deviation is evaluated over equilibrium
    /// measurements, using the same post-thermalization selection as
    /// [`Self::average_volume_profile`].  Returns an empty vector when fewer
    /// than two equilibrium measurements are available.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = MetropolisConfig::new(1.0, 20, 10, 5)?.with_seed(7);
    ///     let results = MetropolisAlgorithm::new(
    ///         config,
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     let fluctuations = results.volume_fluctuations();
    ///     assert_eq!(fluctuations.len(), results.triangulation().slice_sizes().len());
    ///     assert!(fluctuations.iter().all(|volume| volume.is_finite()));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn volume_fluctuations(&self) -> Vec<f64> {
        let means = self.average_volume_profile();
        let n = self.equilibrium_measurements_iter().count();
        if n < 2 || means.is_empty() {
            return Vec::new();
        }

        let mut variances = vec![0.0; means.len()];
        for measurement in self.equilibrium_measurements_iter() {
            for (index, mean) in means.iter().enumerate() {
                let volume = measurement
                    .volume_profile()
                    .get(index)
                    .map_or(0.0, |&volume| <f64 as From<u32>>::from(volume));
                let delta = volume - mean;
                variances[index] += delta * delta;
            }
        }

        let Some(denominator) = usize_to_f64(n - 1) else {
            return Vec::new();
        };
        variances
            .into_iter()
            .map(|variance| (variance / denominator).sqrt())
            .collect()
    }

    /// Estimates the Hausdorff dimension of the final triangulation.
    ///
    /// This is a single-state post-simulation observable computed from
    /// `self.triangulation`, the final triangulation, using dual-graph geodesic
    /// ball growth through [`estimate_hausdorff_dimension`]. It does not
    /// average over equilibrium measurements; doing so would require storing
    /// triangulation snapshots in [`Measurement`] or rerunning the chain.
    /// For ensemble-style recorded data, see [`Self::average_volume_profile`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] if the final triangulation's
    /// backend face adjacency cannot be resolved or if observable counters cannot
    /// be represented as finite fitting inputs.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert!(results
    ///         .hausdorff_dimension_estimate()?
    ///         .is_some_and(f64::is_finite));
    ///     Ok(())
    /// }
    /// ```
    pub fn hausdorff_dimension_estimate(&self) -> CdtResult<Option<f64>> {
        estimate_hausdorff_dimension(&self.triangulation)
    }

    /// Estimates the spectral dimension of the final triangulation.
    ///
    /// This is a single-state post-simulation observable computed from
    /// `self.triangulation`, the final triangulation, using dual-graph
    /// diffusion return probability through [`estimate_spectral_dimension`].
    /// It does not average over equilibrium measurements; doing so would
    /// require storing triangulation snapshots in [`Measurement`] or rerunning
    /// the chain. For ensemble-style recorded data, see
    /// [`Self::average_volume_profile`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] if the final triangulation's
    /// backend face adjacency cannot be resolved or if observable counters cannot
    /// be represented as finite fitting inputs.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_toroidal_cdt(6, 6)?)?;
    ///     assert!(results
    ///         .spectral_dimension_estimate()?
    ///         .is_some_and(f64::is_finite));
    ///     Ok(())
    /// }
    /// ```
    pub fn spectral_dimension_estimate(&self) -> CdtResult<Option<f64>> {
        estimate_spectral_dimension(&self.triangulation)
    }

    /// Returns measurements after thermalization.
    ///
    /// Metropolis runs record measurements only after the configured
    /// thermalization boundary, at completed-move counts divisible by
    /// [`MetropolisConfig::measurement_frequency`]. This accessor defines
    /// equilibrium as `measurement.step() >= thermalization_steps`, so a
    /// measurement taken exactly on the thermalization boundary is included.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = MetropolisConfig::new(1.0, 2, 1, 1)?.with_seed(7);
    ///     let results = MetropolisAlgorithm::new(
    ///         config,
    ///         ActionConfig::default(),
    ///     )
    ///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///     assert_eq!(results.equilibrium_measurements().len(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn equilibrium_measurements(&self) -> Vec<&Measurement> {
        self.equilibrium_measurements_iter().collect()
    }

    /// Iterates over measurements after thermalization without allocating.
    fn equilibrium_measurements_iter(&self) -> impl Iterator<Item = &Measurement> {
        self.measurements
            .iter()
            .filter(|measurement| measurement.step() >= self.config.thermalization_steps())
    }

    /// Writes the upstream scalar trace CSV for every completed Metropolis step.
    ///
    /// This uses [`Self::scalar_trace`] and therefore delegates the table shape
    /// and CSV escaping rules to `markov-chain-monte-carlo`.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::OutputWriteFailed`] if the file or a parent directory
    /// cannot be created, if trace construction fails, or if writing the CSV
    /// fails.
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 1, 1)?,
    ///         ActionConfig::default(),
    ///     )
    ///     .run(tri)?;
    ///     results.write_trace_csv("trace.csv")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn write_trace_csv(&self, path: impl AsRef<Path>) -> CdtResult<()> {
        let path = path.as_ref();
        ensure_parent_directory(path, OutputFormat::Csv)?;
        let trace = self
            .scalar_trace()
            .map_err(|err| output_error(path, OutputFormat::Csv, err))?;
        let file = File::create(path).map_err(|err| output_error(path, OutputFormat::Csv, err))?;
        let mut writer = BufWriter::new(file);
        trace
            .write_csv(&mut writer)
            .map_err(|err| output_error(path, OutputFormat::Csv, err))?;
        writer
            .flush()
            .map_err(|err| output_error(path, OutputFormat::Csv, err))
    }

    /// Writes a JSON summary for external analysis and run bookkeeping.
    ///
    /// The summary stores the validated top-level CLI/configuration parameters,
    /// action and Metropolis configuration, aggregate statistics, final
    /// triangulation counts and mesh indices, Monte Carlo step telemetry, and
    /// all measurements. The aggregate `average_action` is computed from
    /// [`Self::equilibrium_measurements`] so it excludes the initial snapshot and
    /// thermalization window.
    ///
    /// Pass the same [`ValidatedCdtConfig`] used to create the run so the
    /// serialized summary cannot pair accepted telemetry with an invalid or
    /// unrelated raw configuration.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::OutputWriteFailed`] if the file or a parent directory
    /// cannot be created, if the final-triangulation mesh cannot be exported, or
    /// if JSON serialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let validated = CdtConfig {
    ///         simulate: true,
    ///         steps: 2,
    ///         thermalization_steps: 1,
    ///         measurement_frequency: 1,
    ///         ..CdtConfig::new(12, 3)
    ///     }
    ///     .into_validated()?;
    ///     let results = causal_triangulations::run_simulation(&validated)?;
    ///     results.write_summary_json(&validated, "summary.json")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn write_summary_json(
        &self,
        config: &ValidatedCdtConfig,
        path: impl AsRef<Path>,
    ) -> CdtResult<()> {
        let path = path.as_ref();
        let summary = SimulationSummary {
            config: config.config(),
            metropolis_config: &self.config,
            action_config: &self.action_config,
            move_stats: &self.move_stats,
            proposal_stats: &self.proposal_stats,
            aggregate: AggregateSummary {
                acceptance_rate: self.acceptance_rate(),
                average_action: mean_measurement_action(self.equilibrium_measurements_iter()),
                elapsed_time_ms: self.elapsed_time.as_millis(),
                measurement_count: self.measurements.len(),
                step_count: self.steps.len(),
                average_volume_profile: self.average_volume_profile(),
                volume_fluctuations: self.volume_fluctuations(),
            },
            final_triangulation: TriangulationSummary {
                vertices: self.triangulation.vertex_count(),
                edges: self.triangulation.edge_count(),
                triangles: self.triangulation.face_count(),
                time_slices: self.triangulation.time_slices().get(),
                topology: self.triangulation.metadata().topology(),
                mesh: triangulation_mesh_summary(&self.triangulation)?,
            },
            steps: &self.steps,
            measurements: &self.measurements,
        };

        ensure_parent_directory(path, OutputFormat::Json)?;
        let file = File::create(path).map_err(|err| output_error(path, OutputFormat::Json, err))?;
        let mut writer = BufWriter::new(file);
        to_writer_pretty(&mut writer, &summary)
            .map_err(|err| output_error(path, OutputFormat::Json, err))?;
        writeln!(writer).map_err(|err| output_error(path, OutputFormat::Json, err))?;
        writer
            .flush()
            .map_err(|err| output_error(path, OutputFormat::Json, err))
    }
}

/// Returns the mean action across a measurement stream.
fn mean_measurement_action<'a>(measurements: impl IntoIterator<Item = &'a Measurement>) -> f64 {
    let mut sum = 0.0;
    let mut count = 0_usize;
    for measurement in measurements {
        sum += measurement.action();
        count += 1;
    }

    if count == 0 {
        return 0.0;
    }

    let Some(count) = usize_to_f64(count) else {
        return 0.0;
    };
    sum / count
}

/// Creates a parent directory for configured output paths when needed.
fn ensure_parent_directory(path: &Path, format: OutputFormat) -> CdtResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent).map_err(|err| output_error(path, format, err))?;
    }
    Ok(())
}

/// Builds a typed output error without exposing I/O dependencies in public APIs.
fn output_error(path: &Path, format: OutputFormat, err: impl Display) -> CdtError {
    CdtError::OutputWriteFailed {
        path: path.display().to_string(),
        format,
        detail: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::ergodic_moves::MoveType;
    use crate::cdt::foliation::FoliationError;
    use crate::cdt::metropolis::MetropolisAlgorithm;
    use crate::cdt::triangulation::CdtTriangulation;
    use crate::errors::{ConfigurationSetting, MeasurementCountField};
    use crate::geometry::traits::TriangulationQuery;
    use approx::assert_relative_eq;
    use serde_json::{Value, from_str, to_value};
    use std::assert_matches;
    use std::env;
    use std::fs;
    use std::num::NonZeroU32;
    use std::path::PathBuf;
    use std::process;
    use std::thread;

    fn metropolis_config(
        temperature: f64,
        steps: u32,
        thermalization_steps: u32,
        measurement_frequency: u32,
    ) -> MetropolisConfig {
        MetropolisConfig::new(
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
        )
        .expect("test Metropolis config should be valid")
    }

    fn seeded_metropolis_config(
        temperature: f64,
        steps: u32,
        thermalization_steps: u32,
        measurement_frequency: u32,
        seed: u64,
    ) -> MetropolisConfig {
        MetropolisConfig::new_with_seed(
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
            Some(seed),
        )
        .expect("test Metropolis config should be valid")
    }

    fn action_config(coupling_0: f64, coupling_2: f64, cosmological_constant: f64) -> ActionConfig {
        ActionConfig::new(coupling_0, coupling_2, cosmological_constant)
            .expect("test action config should be valid")
    }

    fn step_number(step: u32) -> NonZeroU32 {
        NonZeroU32::new(step).expect("test step number should be nonzero")
    }

    fn accepted_step(
        step: u32,
        move_type: MoveType,
        action_before: f64,
        action_after: f64,
    ) -> MonteCarloStep {
        MonteCarloStep::accepted_step(
            step_number(step),
            move_type,
            action_before,
            action_after,
            action_after - action_before,
        )
        .expect("test accepted step should satisfy action invariants")
    }

    fn rejected_proposal_step(
        step: u32,
        move_type: MoveType,
        action_before: f64,
        delta_action: Option<f64>,
    ) -> MonteCarloStep {
        MonteCarloStep::rejected_proposal(step_number(step), move_type, action_before, delta_action)
            .expect("test rejected step should satisfy action invariants")
    }

    fn no_proposal_step(step: u32, move_type: MoveType, action_before: f64) -> MonteCarloStep {
        MonteCarloStep::no_proposal(step_number(step), move_type, action_before)
            .expect("test no-proposal step should satisfy action invariants")
    }

    fn measurement(
        step: u32,
        action: f64,
        vertices: u32,
        edges: u32,
        triangles: u32,
    ) -> Measurement {
        Measurement::try_new(step, action, vertices, edges, triangles)
            .expect("test measurement should satisfy action and count invariants")
    }

    /// Builds a result container around deterministic geometry for summary-method tests.
    fn results_with(
        config: MetropolisConfig,
        steps: Vec<MonteCarloStep>,
        measurements: Vec<Measurement>,
        triangulation: CdtTriangulation2D,
    ) -> SimulationResultsBackend {
        SimulationResultsBackend {
            config,
            action_config: ActionConfig::default(),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps,
            measurements,
            scalar_trace_rows: Vec::new(),
            elapsed_time: Duration::from_millis(100),
            triangulation,
        }
    }

    fn valid_rejected_result(triangulation: CdtTriangulation2D) -> SimulationResultsBackend {
        let config = metropolis_config(1.0, 1, 0, 1);
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        let scalar_trace_rows = vec![
            CdtScalarTraceRow::new(
                step_number(1),
                CdtScalarTraceOutcome::NoProposal,
                -0.0 / config.temperature(),
                0.0,
                &triangulation,
                MoveType::Move22,
                None,
                0.0,
                None,
                config.seed(),
            )
            .expect("trace row should build"),
        ];
        SimulationResultsBackend::new(
            config,
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 0, 1, 0, 0, 0, 0, 0, 0),
            vec![no_proposal_step(1, MoveType::Move22, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            scalar_trace_rows,
            Duration::ZERO,
            triangulation,
        )
        .expect("valid rejected result should construct")
    }

    fn assert_checkpoint_resume_failure(error: &CdtError, expected: &CheckpointResumeFailure) {
        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure
            } if failure == expected
        );
    }

    /// Asserts two equal-length floating-point slices using relative tolerance.
    fn assert_slice_relative_eq(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (&actual, &expected) in actual.iter().zip(expected) {
            assert_relative_eq!(actual, expected, epsilon = 1e-12);
        }
    }

    /// Asserts two optional estimates match without exact floating-point comparison.
    fn assert_optional_relative_eq(actual: Option<f64>, expected: Option<f64>) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => {
                assert_relative_eq!(actual, expected, epsilon = 1e-12);
            }
            (None, None) => {}
            other => panic!("expected matching optional estimates, got {other:?}"),
        }
    }

    /// Returns a unique temporary path for output-writer tests.
    fn temp_output_path(name: &str) -> PathBuf {
        let thread_name = safe_thread_name();
        env::temp_dir().join(format!(
            "causal-triangulations-{name}-{}-{}",
            process::id(),
            thread_name
        ))
    }

    /// Returns the current test thread name with path separators and
    /// reserved characters removed.
    fn safe_thread_name() -> String {
        thread::current()
            .name()
            .unwrap_or("test")
            .chars()
            .map(|ch| match ch {
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
                ch if ch.is_control() => '_',
                ch => ch,
            })
            .collect()
    }

    #[test]
    fn public_constructor_accepts_valid_components_and_preserves_accessors() {
        let config = seeded_metropolis_config(1.5, 1, 0, 1, 23);
        let action_config = action_config(1.0, 0.5, 0.25);
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        move_stats.record_success(MoveType::Move22);
        let proposal_stats = ProposalStatistics::from_validated_parts(1, 7, 0, 0, 0, 0, 0, 1, 0);
        let step = accepted_step(1, MoveType::Move22, 4.0, 3.5);
        let measurements = vec![
            measurement(0, 4.0, 12, 26, 12)
                .try_with_volume_profile(vec![4, 4, 4])
                .expect("volume profile should fit triangle count"),
            measurement(1, 3.5, 12, 26, 12)
                .try_with_volume_profile(vec![4, 4, 4])
                .expect("volume profile should fit triangle count"),
        ];
        let elapsed = Duration::from_millis(42);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let scalar_trace_rows = vec![
            CdtScalarTraceRow::new(
                step.step(),
                CdtScalarTraceOutcome::Accepted,
                -3.5 / config.temperature(),
                3.5,
                &triangulation,
                step.move_type(),
                step.delta_action(),
                step.action_before(),
                step.action_after(),
                config.seed(),
            )
            .expect("trace row should build"),
        ];

        let results = SimulationResultsBackend::new(
            config.clone(),
            action_config.clone(),
            move_stats,
            proposal_stats.clone(),
            vec![step],
            measurements,
            scalar_trace_rows,
            elapsed,
            triangulation,
        )
        .expect("valid result components should construct");

        assert_eq!(results.config(), &config);
        assert_eq!(results.action_config(), &action_config);
        assert_eq!(results.move_stats().total_attempted(), 1);
        assert_eq!(results.move_stats().total_accepted(), 1);
        assert_eq!(results.proposal_stats(), &proposal_stats);
        assert_eq!(results.steps()[0].step().get(), 1);
        assert_eq!(results.measurements()[0].volume_profile(), &[4, 4, 4]);
        assert_eq!(results.elapsed_time(), elapsed);
        assert_eq!(results.triangulation().slice_sizes(), &[4, 4, 4]);
    }

    #[test]
    fn public_constructor_rejects_pre_thermalization_measurement_stream() {
        let config = metropolis_config(1.0, 4, 2, 2);
        let mut move_stats = MoveStatistics::new();
        for _ in 0..4 {
            move_stats.record_attempt(MoveType::Move22);
        }
        let steps = (1..=4)
            .map(|step| no_proposal_step(step, MoveType::Move22, f64::from(step)))
            .collect::<Vec<_>>();
        let measurements = vec![
            measurement(0, 0.0, 12, 26, 12),
            measurement(4, 4.0, 12, 26, 12),
        ];
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            config,
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(4, 0, 4, 0, 0, 0, 0, 0, 0),
            steps,
            measurements,
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("pre-thermalization measurements should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::MeasurementStepMismatch {
                    actual: 0,
                    expected: 2
                }
            }
        );
    }

    #[test]
    fn public_constructor_rejects_missing_scheduled_measurement() {
        let config = metropolis_config(1.0, 4, 2, 2);
        let mut move_stats = MoveStatistics::new();
        for _ in 0..4 {
            move_stats.record_attempt(MoveType::Move22);
        }
        let steps = (1..=4)
            .map(|step| no_proposal_step(step, MoveType::Move22, f64::from(step)))
            .collect::<Vec<_>>();
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            config,
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(4, 0, 4, 0, 0, 0, 0, 0, 0),
            steps,
            vec![measurement(2, 2.0, 12, 26, 12)],
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("missing scheduled measurement should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::MeasurementCountMismatch {
                    actual: 1,
                    expected: 2
                }
            }
        );
    }

    #[test]
    fn public_constructor_rejects_hard_proposal_failures_with_count() {
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            metropolis_config(1.0, 1, 0, 1),
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 1),
            vec![no_proposal_step(1, MoveType::Move22, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("hard proposal failures cannot appear in completed results");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalHardFailures { actual: 1 }
            }
        );
    }

    #[test]
    fn public_constructor_rejects_proposal_accepted_count_mismatch() {
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        move_stats.record_success(MoveType::Move22);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            metropolis_config(1.0, 1, 0, 1),
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 1, 1, 0, 0, 0, 0, 0, 0),
            vec![accepted_step(1, MoveType::Move22, 0.0, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("accepted proposal totals must match accepted steps");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalAcceptedCountMismatch {
                    actual: 0,
                    expected: 1
                }
            }
        );
    }

    #[test]
    fn public_constructor_rejects_proposal_rejected_count_mismatch() {
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            metropolis_config(1.0, 1, 0, 1),
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0),
            vec![no_proposal_step(1, MoveType::Move22, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("rejected proposal totals must match rejected steps");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalRejectedCountMismatch {
                    actual: 0,
                    expected: 1
                }
            }
        );
    }

    #[test]
    fn public_constructor_rejects_scalar_trace_length_mismatch() {
        let config = metropolis_config(1.0, 1, 0, 1);
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            config,
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 0, 1, 0, 0, 0, 0, 0, 0),
            vec![no_proposal_step(1, MoveType::Move22, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("scalar trace rows must align with step telemetry");

        assert_checkpoint_resume_failure(
            &error,
            &CheckpointResumeFailure::ScalarTraceLengthMismatch {
                actual: 0,
                expected: 1,
            },
        );
    }

    #[test]
    fn public_constructor_rejects_scalar_trace_acceptance_mismatch() {
        let config = metropolis_config(1.0, 1, 0, 1);
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let scalar_trace_rows = vec![
            CdtScalarTraceRow::new(
                step_number(1),
                CdtScalarTraceOutcome::Accepted,
                -0.0 / config.temperature(),
                0.0,
                &triangulation,
                MoveType::Move22,
                Some(0.0),
                0.0,
                Some(0.0),
                config.seed(),
            )
            .expect("trace row should build"),
        ];

        let error = SimulationResultsBackend::new(
            config,
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 0, 1, 0, 0, 0, 0, 0, 0),
            vec![no_proposal_step(1, MoveType::Move22, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            scalar_trace_rows,
            Duration::ZERO,
            triangulation,
        )
        .expect_err("scalar trace outcomes must match step acceptance");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ScalarTraceAcceptedMismatch {
                    step: 1,
                    actual: CdtScalarTraceOutcome::Accepted,
                    expected: CdtScalarTraceOutcome::NoProposal
                }
            }
        );
    }

    #[test]
    fn public_constructor_rejects_scalar_trace_action_mismatches() {
        let config = seeded_metropolis_config(1.5, 1, 0, 1, 23);
        let step = accepted_step(1, MoveType::Move22, 4.0, 3.5);
        let cases = [
            (
                -3.5 / config.temperature(),
                3.5,
                Some(-0.5),
                4.25,
                Some(3.5),
                CheckpointResumeFailure::ScalarTraceActionBeforeMismatch { step: 1 },
            ),
            (
                -3.5 / config.temperature(),
                3.5,
                Some(-0.25),
                4.0,
                Some(3.5),
                CheckpointResumeFailure::ScalarTraceDeltaActionMismatch { step: 1 },
            ),
            (
                -3.5 / config.temperature(),
                3.5,
                Some(-0.5),
                4.0,
                Some(3.25),
                CheckpointResumeFailure::ScalarTraceActionAfterMismatch { step: 1 },
            ),
            (
                -3.25 / config.temperature(),
                3.25,
                Some(-0.5),
                4.0,
                Some(3.5),
                CheckpointResumeFailure::ScalarTraceActionMismatch { step: 1 },
            ),
            (
                -4.0 / config.temperature(),
                3.5,
                Some(-0.5),
                4.0,
                Some(3.5),
                CheckpointResumeFailure::ScalarTraceLogProbMismatch { step: 1 },
            ),
        ];

        for (log_prob, action, delta_action, action_before, action_after, expected) in cases {
            let mut move_stats = MoveStatistics::new();
            move_stats.record_attempt(MoveType::Move22);
            move_stats.record_success(MoveType::Move22);
            let triangulation =
                CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
            let scalar_trace_rows = vec![
                CdtScalarTraceRow::new(
                    step.step(),
                    CdtScalarTraceOutcome::Accepted,
                    log_prob,
                    action,
                    &triangulation,
                    step.move_type(),
                    delta_action,
                    action_before,
                    action_after,
                    config.seed(),
                )
                .expect("finite scalar trace row should build before cross-telemetry validation"),
            ];

            let error = SimulationResultsBackend::new(
                config.clone(),
                ActionConfig::default(),
                move_stats,
                ProposalStatistics::from_validated_parts(1, 7, 0, 0, 0, 0, 0, 1, 0),
                vec![step.clone()],
                vec![
                    measurement(0, 4.0, 12, 26, 12),
                    measurement(1, 3.5, 12, 26, 12),
                ],
                scalar_trace_rows,
                Duration::ZERO,
                triangulation,
            )
            .expect_err("scalar trace action telemetry must match step telemetry");

            assert_checkpoint_resume_failure(&error, &expected);
        }
    }

    #[test]
    fn public_constructor_rejects_scalar_trace_seed_mismatch() {
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 23);
        let step = no_proposal_step(1, MoveType::Move22, 0.0);
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(MoveType::Move22);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let scalar_trace_rows = vec![
            CdtScalarTraceRow::new(
                step.step(),
                CdtScalarTraceOutcome::NoProposal,
                -0.0 / config.temperature(),
                0.0,
                &triangulation,
                step.move_type(),
                None,
                0.0,
                None,
                Some(99),
            )
            .expect("row should build before cross-telemetry validation"),
        ];

        let error = SimulationResultsBackend::new(
            config,
            ActionConfig::default(),
            move_stats,
            ProposalStatistics::from_validated_parts(1, 0, 1, 0, 0, 0, 0, 0, 0),
            vec![step],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            scalar_trace_rows,
            Duration::ZERO,
            triangulation,
        )
        .expect_err("scalar trace seed must match simulation config");

        assert_checkpoint_resume_failure(
            &error,
            &CheckpointResumeFailure::ScalarTraceSeedMismatch {
                step: 1,
                actual: Some(99),
                expected: Some(23),
            },
        );
    }

    #[test]
    fn scalar_trace_validation_rejects_step_and_move_mismatches() {
        let config = metropolis_config(1.0, 1, 0, 1);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let step = no_proposal_step(1, MoveType::Move22, 0.0);
        let cases = [
            (
                CdtScalarTraceRow::new(
                    step_number(2),
                    CdtScalarTraceOutcome::NoProposal,
                    -0.0 / config.temperature(),
                    0.0,
                    &triangulation,
                    MoveType::Move22,
                    None,
                    0.0,
                    None,
                    config.seed(),
                )
                .expect("trace row should build"),
                CheckpointResumeFailure::ScalarTraceStepMismatch {
                    actual: 2,
                    expected: 1,
                },
            ),
            (
                CdtScalarTraceRow::new(
                    step.step(),
                    CdtScalarTraceOutcome::NoProposal,
                    -0.0 / config.temperature(),
                    0.0,
                    &triangulation,
                    MoveType::EdgeFlip,
                    None,
                    0.0,
                    None,
                    config.seed(),
                )
                .expect("trace row should build"),
                CheckpointResumeFailure::ScalarTraceMoveTypeMismatch {
                    step: 1,
                    actual: MoveType::EdgeFlip,
                    expected: MoveType::Move22,
                },
            ),
        ];

        for (row, expected) in cases {
            let error = validate_scalar_trace_rows(
                &config,
                &ProposalStatistics::from_validated_parts(1, 0, 1, 0, 0, 0, 0, 0, 0),
                std::slice::from_ref(&step),
                &[row],
            )
            .expect_err("scalar trace row identity must match step telemetry");

            assert_checkpoint_resume_failure(&error, &expected);
        }
    }

    #[test]
    fn scalar_trace_validation_rejects_optional_delta_action_presence_mismatch() {
        let config = metropolis_config(1.0, 1, 0, 1);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let step = rejected_proposal_step(1, MoveType::Move22, 4.0, Some(0.5));
        let row = CdtScalarTraceRow::new(
            step.step(),
            CdtScalarTraceOutcome::RejectedProposal,
            -4.0 / config.temperature(),
            4.0,
            &triangulation,
            step.move_type(),
            None,
            step.action_before(),
            step.action_after(),
            config.seed(),
        )
        .expect("row should build before cross-telemetry validation");

        let error = validate_scalar_trace_rows(
            &config,
            &ProposalStatistics::from_validated_parts(1, 7, 0, 0, 0, 0, 0, 1, 0),
            &[step],
            &[row],
        )
        .expect_err("optional delta_action presence must match step telemetry");

        assert_checkpoint_resume_failure(
            &error,
            &CheckpointResumeFailure::ScalarTraceDeltaActionMismatch { step: 1 },
        );
    }

    #[test]
    fn scalar_trace_validation_rejects_aggregate_counter_mismatches() {
        let config = metropolis_config(1.0, 1, 0, 1);
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let accepted_step = accepted_step(1, MoveType::Move22, 4.0, 3.5);
        let rejected_step = rejected_proposal_step(1, MoveType::Move22, 4.0, Some(0.5));
        let no_proposal_step = no_proposal_step(1, MoveType::Move22, 4.0);
        let accepted_row = CdtScalarTraceRow::new(
            accepted_step.step(),
            CdtScalarTraceOutcome::Accepted,
            -3.5 / config.temperature(),
            3.5,
            &triangulation,
            accepted_step.move_type(),
            accepted_step.delta_action(),
            accepted_step.action_before(),
            accepted_step.action_after(),
            config.seed(),
        )
        .expect("accepted scalar trace row should build");
        let rejected_row = CdtScalarTraceRow::new(
            rejected_step.step(),
            CdtScalarTraceOutcome::RejectedProposal,
            -4.0 / config.temperature(),
            4.0,
            &triangulation,
            rejected_step.move_type(),
            rejected_step.delta_action(),
            rejected_step.action_before(),
            rejected_step.action_after(),
            config.seed(),
        )
        .expect("rejected scalar trace row should build");
        let no_proposal_row = CdtScalarTraceRow::new(
            no_proposal_step.step(),
            CdtScalarTraceOutcome::NoProposal,
            -4.0 / config.temperature(),
            4.0,
            &triangulation,
            no_proposal_step.move_type(),
            no_proposal_step.delta_action(),
            no_proposal_step.action_before(),
            no_proposal_step.action_after(),
            config.seed(),
        )
        .expect("no-proposal scalar trace row should build");
        let cases = [
            (
                accepted_step,
                accepted_row,
                ProposalStatistics::from_validated_parts(1, 7, 0, 0, 0, 0, 0, 0, 0),
                CheckpointResumeFailure::ScalarTraceAcceptedCountMismatch {
                    actual: 1,
                    expected: 0,
                },
            ),
            (
                rejected_step,
                rejected_row,
                ProposalStatistics::from_validated_parts(1, 7, 0, 0, 0, 0, 0, 0, 0),
                CheckpointResumeFailure::ScalarTraceRejectedProposalCountMismatch {
                    actual: 1,
                    expected: 0,
                },
            ),
            (
                no_proposal_step,
                no_proposal_row,
                ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0),
                CheckpointResumeFailure::ScalarTraceNoProposalCountMismatch {
                    actual: 1,
                    expected: 0,
                },
            ),
        ];

        for (step, row, proposal_stats, expected) in cases {
            let error = validate_scalar_trace_rows(&config, &proposal_stats, &[step], &[row])
                .expect_err("scalar trace aggregate counters must match proposal telemetry");

            assert_checkpoint_resume_failure(&error, &expected);
        }
    }

    #[test]
    fn scalar_trace_export_encodes_rejected_rows_and_absent_seed_observables() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let mut row = CdtScalarTraceRow::new(
            step_number(1),
            CdtScalarTraceOutcome::RejectedProposal,
            -4.0,
            4.0,
            &triangulation,
            MoveType::Move22,
            Some(0.5),
            4.0,
            None,
            None,
        )
        .expect("rejected trace row should build");
        row.volume_profile = vec![2, 4];
        let mut results = results_with(
            metropolis_config(1.0, 1, 0, 1),
            Vec::new(),
            Vec::new(),
            triangulation,
        );
        results.scalar_trace_rows = vec![row];

        let trace = results.scalar_trace().expect("scalar trace should export");
        assert_eq!(trace.len(), 1);
        assert_eq!(
            trace.observable_names(),
            &[
                "action",
                "vertices",
                "edges",
                "triangles",
                "move_family",
                "delta_action",
                "delta_action_present",
                "action_before",
                "action_after",
                "action_after_present",
                "seed_low_u32",
                "seed_high_u32",
                "seed_present",
                "volume_profile_0",
                "volume_profile_1",
            ]
        );
        let record = &trace.records()[0];
        assert_eq!(record.outcome(), TraceStepOutcome::rejected_proposal());
        assert_relative_eq!(record.log_prob(), -4.0);
        assert_eq!(
            record.observable_values(),
            &[
                4.0, 12.0, 23.0, 12.0, 22.0, 0.5, 1.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0,
            ]
        );
    }

    #[test]
    fn scalar_trace_row_rejects_outcome_payload_mismatch_before_storage() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let cases = [
            (
                CdtScalarTraceOutcome::Accepted,
                None,
                Some(4.0),
                CheckpointResumeFailure::ScalarTraceDeltaActionMismatch { step: 1 },
            ),
            (
                CdtScalarTraceOutcome::Accepted,
                Some(0.0),
                None,
                CheckpointResumeFailure::ScalarTraceActionAfterMismatch { step: 1 },
            ),
            (
                CdtScalarTraceOutcome::RejectedProposal,
                Some(0.0),
                Some(4.0),
                CheckpointResumeFailure::ScalarTraceActionAfterMismatch { step: 1 },
            ),
            (
                CdtScalarTraceOutcome::NoProposal,
                Some(0.0),
                None,
                CheckpointResumeFailure::ScalarTraceDeltaActionMismatch { step: 1 },
            ),
            (
                CdtScalarTraceOutcome::NoProposal,
                None,
                Some(4.0),
                CheckpointResumeFailure::ScalarTraceActionAfterMismatch { step: 1 },
            ),
        ];

        for (outcome, delta_action, action_after, expected) in cases {
            let error = CdtScalarTraceRow::new(
                step_number(1),
                outcome,
                -4.0,
                4.0,
                &triangulation,
                MoveType::Move22,
                delta_action,
                4.0,
                action_after,
                None,
            )
            .expect_err("scalar trace rows should reject mismatched outcome payloads");

            assert_checkpoint_resume_failure(&error, &expected);
        }
    }

    #[test]
    fn scalar_trace_row_rejects_nonfinite_numeric_fields_before_storage() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let cases = [
            (
                ScalarTraceField::LogProb,
                CdtScalarTraceOutcome::RejectedProposal,
                f64::NAN,
                0.0,
                None,
                0.0,
                None,
            ),
            (
                ScalarTraceField::Action,
                CdtScalarTraceOutcome::RejectedProposal,
                -0.0,
                f64::INFINITY,
                None,
                0.0,
                None,
            ),
            (
                ScalarTraceField::ActionBefore,
                CdtScalarTraceOutcome::RejectedProposal,
                -0.0,
                0.0,
                None,
                f64::NEG_INFINITY,
                None,
            ),
            (
                ScalarTraceField::DeltaAction,
                CdtScalarTraceOutcome::RejectedProposal,
                -0.0,
                0.0,
                Some(f64::NAN),
                0.0,
                None,
            ),
            (
                ScalarTraceField::ActionAfter,
                CdtScalarTraceOutcome::Accepted,
                -0.0,
                0.0,
                Some(0.0),
                0.0,
                Some(f64::INFINITY),
            ),
        ];

        for (
            expected_field,
            outcome,
            log_prob,
            action,
            delta_action,
            action_before,
            action_after,
        ) in cases
        {
            let error = CdtScalarTraceRow::new(
                step_number(1),
                outcome,
                log_prob,
                action,
                &triangulation,
                MoveType::Move22,
                delta_action,
                action_before,
                action_after,
                None,
            )
            .expect_err("non-finite scalar trace fields should be rejected");

            assert_matches!(
                error,
                CdtError::CheckpointResumeFailed {
                    failure: CheckpointResumeFailure::NonFiniteScalarTraceValue {
                        step: 1,
                        field,
                    }
                } if field == expected_field
            );
        }
    }

    #[test]
    fn public_constructor_rejects_invalid_metropolis_config() {
        let error =
            MetropolisConfig::new(0.0, 1, 0, 1).expect_err("zero temperature should be rejected");

        assert_matches!(
            error,
            CdtError::InvalidSimulationConfiguration {
                ref setting,
                ref provided_value,
                ref expected,
            } if *setting == ConfigurationSetting::Temperature
                && provided_value == "0"
                && expected == "finite and positive"
        );
    }

    #[test]
    fn public_constructor_rejects_invalid_action_config() {
        let error = ActionConfig::new(f64::NAN, 0.0, 0.0)
            .expect_err("non-finite action coupling should be rejected");

        assert_matches!(
            error,
            CdtError::InvalidConfiguration {
                ref setting,
                ref provided_value,
                ref expected,
            } if *setting == ConfigurationSetting::Coupling0 && provided_value == "NaN" && expected == "finite"
        );
    }

    #[test]
    fn public_constructor_rejects_stale_final_foliation() {
        let mut triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let vertex = triangulation
            .geometry()
            .vertices()
            .next()
            .expect("strip should contain vertices");
        let label = triangulation
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
            .expect("strip vertices should be labeled");
        triangulation
            .set_vertex_data(&vertex, Some(label))
            .expect("rewriting an existing label should mark foliation stale");

        let error = SimulationResultsBackend::new(
            metropolis_config(1.0, 1, 0, 1),
            ActionConfig::default(),
            MoveStatistics::new(),
            ProposalStatistics::new(),
            vec![no_proposal_step(1, MoveType::Move22, 0.0)],
            vec![
                measurement(0, 0.0, 12, 26, 12),
                measurement(1, 0.0, 12, 26, 12),
            ],
            Vec::new(),
            Duration::ZERO,
            triangulation,
        )
        .expect_err("stale foliation should be rejected");

        assert_matches!(
            error,
            CdtError::Foliation(FoliationError::StaleBookkeeping { .. })
        );
    }

    #[test]
    fn deserialization_rejects_invalid_metropolis_config() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let json = serde_json::to_string(&results).expect("results should serialize");
        let roundtrip: SimulationResultsBackend =
            serde_json::from_str(&json).expect("valid serialized results should load");
        assert_relative_eq!(roundtrip.config.temperature(), 1.0);

        let invalid_json = json.replacen("\"temperature\":1.0", "\"temperature\":0.0", 1);
        assert_ne!(invalid_json, json);

        let error = serde_json::from_str::<SimulationResultsBackend>(&invalid_json)
            .expect_err("validated deserialization should reject zero temperature");
        let message = error.to_string();
        assert!(
            message.contains("temperature") && message.contains("finite and positive"),
            "serde error should preserve validation context, got {message}"
        );
    }

    #[test]
    fn deserialization_rejects_zero_step_telemetry() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let mut payload = to_value(&results).expect("results should serialize");
        payload["steps"][0]["step"] = to_value(0_u32).expect("step should serialize");

        let error = from_str::<SimulationResultsBackend>(&payload.to_string())
            .expect_err("zero Monte Carlo step telemetry should fail while parsing");
        assert!(
            error.to_string().contains("nonzero") || error.to_string().contains("invalid value"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn deserialization_rejects_zero_scalar_trace_step() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let mut payload = to_value(&results).expect("results should serialize");
        payload["scalar_trace_rows"][0]["step"] = to_value(0_u32).expect("step should serialize");

        let error = from_str::<SimulationResultsBackend>(&payload.to_string())
            .expect_err("zero scalar trace step should fail while parsing");
        assert!(
            error
                .to_string()
                .contains("scalar trace step must be nonzero"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn deserialization_rejects_zero_measurement_counts_by_field() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);

        for field in ["vertices", "edges", "triangles"] {
            let mut payload = to_value(&results).expect("results should serialize");
            payload["measurements"][0][field] = to_value(0_u32).expect("count should serialize");

            let error = from_str::<SimulationResultsBackend>(&payload.to_string())
                .expect_err("zero measurement count should fail while parsing");
            let message = error.to_string();
            assert!(
                message.contains("Invalid measurement count") && message.contains(field),
                "unexpected serde error for {field}: {message}"
            );
        }
    }

    #[test]
    fn deserialization_rejects_zero_scalar_trace_counts_by_field() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);

        for field in ["vertices", "edges", "triangles"] {
            let mut payload = to_value(&results).expect("results should serialize");
            payload["scalar_trace_rows"][0][field] =
                to_value(0_u32).expect("count should serialize");

            let error = from_str::<SimulationResultsBackend>(&payload.to_string())
                .expect_err("zero scalar trace count should fail while parsing");
            let message = error.to_string();
            assert!(
                message.contains("Invalid scalar trace count") && message.contains(field),
                "unexpected serde error for {field}: {message}"
            );
        }
    }

    #[test]
    fn deserialization_rejects_scalar_trace_volume_profile_above_triangle_count() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let mut payload = to_value(&results).expect("results should serialize");
        payload["scalar_trace_rows"][0]["volume_profile"] =
            to_value([13_u32]).expect("volume profile should serialize");

        let error = from_str::<SimulationResultsBackend>(&payload.to_string())
            .expect_err("scalar trace volume profile cannot exceed stored triangle count");

        assert!(
            error
                .to_string()
                .contains("scalar trace volume profile total 13")
                && error.to_string().contains("triangle count 12"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn deserialization_rejects_measurement_volume_profile_above_triangle_count() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let mut payload = to_value(&results).expect("results should serialize");
        payload["measurements"][0]["volume_profile"] =
            to_value([13_u32]).expect("volume profile should serialize");

        let error = from_str::<SimulationResultsBackend>(&payload.to_string())
            .expect_err("measurement volume profile cannot exceed stored triangle count");

        assert!(
            error
                .to_string()
                .contains("Invalid measurement volume profile at step 0: total 13")
                && error.to_string().contains("triangle count 12"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn deserialization_defaults_missing_proposal_stats() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let mut payload = to_value(&results).expect("results should serialize");
        payload
            .as_object_mut()
            .expect("results payload should be an object")
            .remove("proposal_stats");

        let error = from_str::<SimulationResultsBackend>(&payload.to_string())
            .expect_err("missing proposal stats should fail result telemetry validation");
        assert!(
            error.to_string().contains("proposal")
                || error.to_string().contains("move-family count mismatch"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn deserialization_rejects_missing_scalar_trace_rows() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = valid_rejected_result(triangulation);
        let mut payload = to_value(&results).expect("results should serialize");
        payload
            .as_object_mut()
            .expect("results payload should be an object")
            .remove("scalar_trace_rows");

        let error = from_str::<SimulationResultsBackend>(&payload.to_string())
            .expect_err("missing scalar trace rows should fail result deserialization");
        assert!(
            error
                .to_string()
                .contains("missing field `scalar_trace_rows`"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn result_wire_validation_rejects_invalid_metropolis_config() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let error = SimulationResultsBackend::try_from(SimulationResultsBackendWire {
            config: metropolis_config(1.0, 1, 0, 1),
            action_config: ActionConfig::default(),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: vec![],
            measurements: vec![],
            scalar_trace_rows: vec![],
            elapsed_time: Duration::ZERO,
            triangulation,
        })
        .expect_err("invalid result telemetry should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::StepTelemetryLengthMismatch { .. }
            }
        );
    }

    #[test]
    fn result_wire_validation_rejects_invalid_action_config() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let error = SimulationResultsBackend::try_from(SimulationResultsBackendWire {
            config: metropolis_config(1.0, 1, 0, 1),
            action_config: ActionConfig::default(),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: vec![],
            measurements: vec![],
            scalar_trace_rows: vec![],
            elapsed_time: Duration::ZERO,
            triangulation,
        })
        .expect_err("invalid result telemetry should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::StepTelemetryLengthMismatch { .. }
            }
        );
    }

    #[test]
    fn measurement_builders_preserve_scalar_counts_and_profile() {
        let measurement = measurement(7, -3.5, 12, 26, 12)
            .try_with_volume_profile(vec![6, 6, 0])
            .expect("volume profile should fit triangle count");

        assert_eq!(measurement.step(), 7);
        assert_relative_eq!(measurement.action(), -3.5);
        assert_eq!(measurement.vertices().get(), 12);
        assert_eq!(measurement.edges().get(), 26);
        assert_eq!(measurement.triangles().get(), 12);
        assert_eq!(measurement.volume_profile(), &[6, 6, 0]);
    }

    #[test]
    fn measurement_builder_rejects_volume_profile_above_triangle_count() {
        let error = measurement(7, -3.5, 12, 26, 12)
            .try_with_volume_profile(vec![13])
            .expect_err("volume profile cannot exceed stored triangle count");

        assert_matches!(
            error,
            CdtError::InvalidMeasurementVolumeProfile {
                step: 7,
                profile_total: 13,
                triangles: 12,
            }
        );
    }

    #[test]
    fn measurement_try_new_rejects_each_zero_count_with_field_context() {
        let cases = [
            (0, 26, 12, MeasurementCountField::Vertices),
            (12, 0, 12, MeasurementCountField::Edges),
            (12, 26, 0, MeasurementCountField::Triangles),
        ];

        for (vertices, edges, triangles, expected_field) in cases {
            let error = Measurement::try_new(7, -3.5, vertices, edges, triangles)
                .expect_err("zero measurement counts should be rejected");

            assert_matches!(
                error,
                CdtError::InvalidMeasurementCount {
                    field,
                    provided_value: 0,
                } if field == expected_field
            );
        }
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn measurement_from_simplex_counts_rejects_count_overflow() {
        let oversized = usize::try_from(u32::MAX).expect("u32::MAX should fit usize") + 1;
        let counts = CdtSimplexCounts::try_new(oversized, 26, 12)
            .expect("oversized live count should still be a positive CDT count");

        let error = Measurement::try_from_simplex_counts(7, -3.5, counts)
            .expect_err("measurement counts above u32::MAX should be rejected");

        assert_matches!(
            error,
            CdtError::MeasurementCountOverflow {
                field: MeasurementCountField::Vertices,
                provided_value,
                max: u32::MAX,
            } if provided_value == oversized
        );
    }

    #[test]
    fn measurement_try_new_rejects_nonfinite_action() {
        for action in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let error = Measurement::try_new(7, action, 12, 26, 12)
                .expect_err("non-finite measurement action should be rejected");

            assert_matches!(
                error,
                CdtError::InvalidMeasurementAction {
                    step: 7,
                    provided_value,
                } if provided_value.to_bits() == action.to_bits()
            );
        }
    }

    #[test]
    fn writes_trace_csv_with_upstream_step_metadata() {
        let results = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 7),
            ActionConfig::default(),
        )
        .run(CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build"))
        .expect("short Metropolis run should complete");
        let trace = results
            .scalar_trace()
            .expect("CDT scalar trace columns should be valid");
        let path = temp_output_path("trace.csv");

        results
            .write_trace_csv(&path)
            .expect("trace CSV output should write");
        let csv = fs::read_to_string(&path).expect("CSV output should be readable");
        fs::remove_file(&path).expect("temporary CSV should be removable");

        assert_eq!(trace.len(), results.steps().len());
        assert_eq!(trace.records()[0].chain_id(), ChainId::new(0));
        assert!(trace.records()[0].outcome().had_proposal());
        assert!(
            trace
                .observable_names()
                .iter()
                .any(|name| name == "volume_profile_0")
        );
        assert!(csv.starts_with(
            "chain_id,step,accepted,proposed,log_prob,action,vertices,edges,triangles,move_family"
        ));
        assert_eq!(csv.lines().count().saturating_sub(1), results.steps().len());
    }

    #[test]
    fn writes_summary_json_with_config_and_aggregates() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 1, 0, 1),
            vec![accepted_step(1, MoveType::Move22, 3.0, 2.5)],
            vec![measurement(1, 2.5, 12, 26, 12)],
            triangulation,
        );
        let config = CdtConfig {
            steps: 1,
            thermalization_steps: 0,
            measurement_frequency: 1,
            simulate: true,
            ..CdtConfig::new(12, 3)
        }
        .into_validated()
        .expect("summary config should validate");
        let path = temp_output_path("summary.json");

        results
            .write_summary_json(&config, &path)
            .expect("JSON output should write");
        let json = fs::read_to_string(&path).expect("JSON output should be readable");
        fs::remove_file(&path).expect("temporary JSON should be removable");
        let parsed: Value = from_str(&json).expect("summary should be valid JSON");

        assert_eq!(parsed["config"]["vertices"], 12);
        assert_eq!(parsed["aggregate"]["measurement_count"], 1);
        assert_eq!(parsed["aggregate"]["step_count"], 1);
        assert_eq!(parsed["final_triangulation"]["time_slices"], 3);
        let mesh = &parsed["final_triangulation"]["mesh"];
        let coordinate_columns = mesh["coordinate_columns"]
            .as_array()
            .expect("mesh coordinate columns should serialize as an array");
        assert_eq!(coordinate_columns.len(), 2);
        assert_eq!(coordinate_columns[0].as_str(), Some("x"));
        assert_eq!(coordinate_columns[1].as_str(), Some("y"));
        let vertices = mesh["vertices"]
            .as_array()
            .expect("mesh vertices should serialize as an array");
        assert_eq!(vertices.len(), 12);
        for (expected_index, vertex) in vertices.iter().enumerate() {
            assert_eq!(
                vertex["index"], expected_index,
                "mesh vertices should use stable zero-based indices"
            );
            assert!(
                vertex["time"].as_u64().is_some_and(|time| time < 3),
                "mesh vertex should include a valid foliation time label: {vertex}"
            );
            let coordinates = vertex["coordinates"]
                .as_array()
                .expect("mesh vertex coordinates should serialize as an array");
            assert_eq!(coordinates.len(), 2);
            assert!(
                coordinates
                    .iter()
                    .all(|coordinate| coordinate.as_f64().is_some_and(f64::is_finite)),
                "mesh vertex coordinates should be finite x/y values: {vertex}"
            );
        }
        let triangles = mesh["triangles"]
            .as_array()
            .expect("mesh triangles should serialize as an array");
        assert_eq!(triangles.len(), 12);
        for triangle in triangles {
            let indices = triangle
                .as_array()
                .expect("mesh triangles should serialize as index arrays");
            assert_eq!(indices.len(), 3);
            for index in indices {
                assert!(
                    index
                        .as_u64()
                        .is_some_and(|index| index < vertices.len() as u64),
                    "triangle index should refer to a serialized vertex: {triangle}"
                );
            }
        }
        assert_eq!(parsed["measurements"][0]["step"], 1);
    }

    #[test]
    fn output_writers_reject_file_parent() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );
        let parent_file = temp_output_path("not-a-directory");
        fs::write(&parent_file, b"not a directory").expect("parent fixture file should write");

        let csv_path = parent_file.join("trace.csv");
        let csv_error = results
            .write_trace_csv(&csv_path)
            .expect_err("CSV writer should reject a parent path that is a file");
        let CdtError::OutputWriteFailed {
            path,
            format,
            detail,
        } = csv_error
        else {
            panic!("expected CSV output write failure, got {csv_error:?}");
        };
        assert_eq!(format, OutputFormat::Csv);
        assert_eq!(path, csv_path.display().to_string());
        assert!(!detail.is_empty());

        let json_path = parent_file.join("summary.json");
        let config = CdtConfig::new(12, 3)
            .into_validated()
            .expect("summary config should validate");
        let json_error = results
            .write_summary_json(&config, &json_path)
            .expect_err("JSON writer should reject a parent path that is a file");
        let CdtError::OutputWriteFailed {
            path,
            format,
            detail,
        } = json_error
        else {
            panic!("expected JSON output write failure, got {json_error:?}");
        };
        assert_eq!(format, OutputFormat::Json);
        assert_eq!(path, json_path.display().to_string());
        assert!(!detail.is_empty());

        fs::remove_file(&parent_file).expect("parent fixture file should be removable");
    }

    #[test]
    fn summary_json_average_action_uses_equilibrium_measurements() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 2, 1, 1),
            vec![],
            vec![
                measurement(0, 100.0, 12, 26, 12),
                measurement(1, 4.0, 12, 26, 12),
                measurement(2, 6.0, 12, 26, 12),
            ],
            triangulation,
        );
        let config = CdtConfig {
            steps: 2,
            thermalization_steps: 1,
            measurement_frequency: 1,
            simulate: true,
            ..CdtConfig::new(12, 3)
        }
        .into_validated()
        .expect("summary config should validate");
        let path = temp_output_path("equilibrium-summary.json");

        results
            .write_summary_json(&config, &path)
            .expect("JSON output should write");
        let json = fs::read_to_string(&path).expect("JSON output should be readable");
        fs::remove_file(&path).expect("temporary JSON should be removable");
        let parsed: Value = from_str(&json).expect("summary should be valid JSON");

        assert_relative_eq!(
            parsed["aggregate"]["average_action"]
                .as_f64()
                .expect("average action should be numeric"),
            5.0
        );
    }

    #[test]
    fn summaries_use_post_thermalization_measurements() {
        let config = metropolis_config(1.0, 20, 10, 5);
        let steps = vec![
            accepted_step(1, MoveType::Move22, 3.0, 2.5),
            rejected_proposal_step(2, MoveType::Move13Add, 2.5, Some(0.8)),
            accepted_step(3, MoveType::Move31Remove, 2.5, 2.0),
        ];
        let measurements = vec![
            measurement(0, 1.0, 3, 3, 1)
                .try_with_volume_profile(vec![1, 0, 0])
                .expect("volume profile should fit triangle count"),
            measurement(10, 2.0, 4, 5, 2)
                .try_with_volume_profile(vec![1, 1, 0])
                .expect("volume profile should fit triangle count"),
            measurement(15, 3.0, 5, 7, 3)
                .try_with_volume_profile(vec![1, 2, 0])
                .expect("volume profile should fit triangle count"),
        ];
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(config, steps, measurements, triangulation);

        assert_relative_eq!(results.acceptance_rate(), 2.0 / 3.0);
        assert_relative_eq!(results.average_action(), 2.0);
        assert_slice_relative_eq(&results.average_volume_profile(), &[1.0, 1.5, 0.0]);
        assert_slice_relative_eq(&results.volume_fluctuations(), &[0.0, 0.5_f64.sqrt(), 0.0]);

        let equilibrium = results.equilibrium_measurements();
        assert_eq!(equilibrium.len(), 2);
        assert_eq!(equilibrium[0].step(), 10);
        assert_eq!(equilibrium[1].step(), 15);
    }

    #[test]
    fn volume_observables_treat_missing_profile_entries_as_zero() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 20, 10, 5),
            vec![],
            vec![
                measurement(10, 2.0, 4, 5, 13)
                    .try_with_volume_profile(vec![4, 8, 1])
                    .expect("volume profile should fit triangle count"),
                measurement(15, 3.0, 5, 7, 6)
                    .try_with_volume_profile(vec![6])
                    .expect("volume profile should fit triangle count"),
            ],
            triangulation,
        );

        assert_slice_relative_eq(&results.average_volume_profile(), &[5.0, 4.0, 0.5]);
        assert_slice_relative_eq(
            &results.volume_fluctuations(),
            &[2.0_f64.sqrt(), 32.0_f64.sqrt(), 0.5_f64.sqrt()],
        );
    }

    #[test]
    fn volume_observables_are_empty_when_profiles_are_empty() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 20, 10, 5),
            vec![],
            vec![measurement(10, 2.0, 4, 5, 2), measurement(15, 3.0, 5, 7, 3)],
            triangulation,
        );

        assert!(results.average_volume_profile().is_empty());
        assert!(results.volume_fluctuations().is_empty());
    }

    #[test]
    fn volume_fluctuations_are_empty_for_single_equilibrium_measurement() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 20, 10, 5),
            vec![],
            vec![
                measurement(0, 1.0, 3, 3, 1)
                    .try_with_volume_profile(vec![1])
                    .expect("volume profile should fit triangle count"),
                measurement(10, 2.0, 4, 5, 2)
                    .try_with_volume_profile(vec![2])
                    .expect("volume profile should fit triangle count"),
            ],
            triangulation,
        );

        assert_slice_relative_eq(&results.average_volume_profile(), &[2.0]);
        assert!(results.volume_fluctuations().is_empty());
    }

    #[test]
    fn summaries_are_empty_for_no_steps_or_measurements() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            metropolis_config(1.0, 20, 10, 5),
            vec![],
            vec![],
            triangulation,
        );

        assert_relative_eq!(results.acceptance_rate(), 0.0);
        assert_relative_eq!(results.average_action(), 0.0);
        assert!(results.equilibrium_measurements().is_empty());
        assert!(results.average_volume_profile().is_empty());
        assert!(results.volume_fluctuations().is_empty());
    }

    #[test]
    fn dimension_estimates_delegate_to_final_triangulation() {
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(6, 6).expect("periodic torus should build");
        let results = results_with(
            metropolis_config(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );

        assert_optional_relative_eq(
            results
                .hausdorff_dimension_estimate()
                .expect("result triangulation adjacency should be readable"),
            estimate_hausdorff_dimension(results.triangulation())
                .expect("result triangulation adjacency should be readable"),
        );
        assert_optional_relative_eq(
            results
                .spectral_dimension_estimate()
                .expect("result triangulation adjacency should be readable"),
            estimate_spectral_dimension(results.triangulation())
                .expect("result triangulation adjacency should be readable"),
        );
    }

    #[test]
    fn dimension_estimates_return_none_for_tiny_final_triangulation() {
        let triangulation = CdtTriangulation::from_seeded_points(3, 1, 2, 53)
            .expect("seeded triangle should build");
        let results = results_with(
            metropolis_config(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );

        assert!(
            results
                .hausdorff_dimension_estimate()
                .expect("tiny triangulation adjacency should be readable")
                .is_none()
        );
        assert!(
            results
                .spectral_dimension_estimate()
                .expect("tiny triangulation adjacency should be readable")
                .is_none()
        );
    }
}
