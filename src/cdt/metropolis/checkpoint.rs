#![forbid(unsafe_code)]

//! Checkpoint and resume validation for CDT Metropolis sampling.

use super::helpers::{
    action_for, actions_match, expected_measurement_count, expected_measurement_step,
};
use super::runner::{MetropolisAlgorithm, MetropolisConfig};
use super::telemetry::{MonteCarloStep, ProposalStatistics};
use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveStatistics, MoveType};
use crate::cdt::results::{
    CdtScalarTraceRow, Measurement, SimulationHistory, SimulationResultsBackend,
    SimulationResultsParts, scalar_trace_no_proposal_count, validate_scalar_trace_row_slice,
    validate_scalar_trace_rows, validate_trajectory_observables,
};
use crate::cdt::triangulation::{CdtTriangulation2D, CdtTriangulationCheckpointWireV1};
use crate::errors::{
    CdtError, CdtResult, CheckpointMoveCounter, CheckpointOperation, CheckpointResumeFailure,
    ProposalTelemetryCounter,
};
use markov_chain_monte_carlo::ChainCheckpoint;
use rand::rngs::Xoshiro256PlusPlus;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError, ser::Error as SerError,
};
use serde_json::Value;
use std::num::NonZeroU32;
use std::time::Duration;

const CHECKPOINT_FORMAT_VERSION: u32 = 1;

pub(crate) struct CdtMcmcCheckpointParts {
    pub(crate) triangulation: CdtTriangulation2D,
    pub(crate) accepted: usize,
    pub(crate) rejected: usize,
    pub(crate) config: MetropolisConfig,
    pub(crate) action_config: ActionConfig,
    pub(crate) current_step: NonZeroU32,
    pub(crate) current_action: f64,
    pub(crate) move_stats: MoveStatistics,
    pub(crate) proposal_stats: ProposalStatistics,
    pub(crate) steps: Vec<MonteCarloStep>,
    pub(crate) measurements: Vec<Measurement>,
    pub(crate) scalar_trace_rows: Vec<CdtScalarTraceRow>,
    pub(crate) elapsed_time: Duration,
    pub(crate) acceptance_rng: Xoshiro256PlusPlus,
    pub(crate) ergodics: ErgodicsSystem,
}

/// Length and outcome evidence for an already fully validated checkpoint prefix.
#[derive(Clone, Copy)]
pub(crate) struct ValidatedCheckpointPrefix {
    steps: usize,
    measurements: usize,
    scalar_trace_rows: usize,
    accepted_steps: usize,
    scalar_accepted: u64,
    scalar_rejected_proposal: u64,
    scalar_no_proposal: u64,
}

/// Finite action value stored in resumable checkpoint state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct CheckpointAction(f64);

impl CheckpointAction {
    /// Parses a raw checkpoint action into a finite stored action value.
    const fn new(value: f64) -> CdtResult<Self> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(checkpoint_resume_failed(
                CheckpointResumeFailure::NonFiniteCheckpointAction { stored: value },
            ))
        }
    }

    /// Returns the finite action value.
    const fn get(self) -> f64 {
        self.0
    }
}

/// Resumable checkpoint for a CDT Metropolis-Hastings run.
///
/// The in-memory [`ChainCheckpoint`] stores the current triangulation and chain
/// counters for sampler interoperation. The persistent representation is owned
/// entirely by CDT and additionally records action/config metadata, accumulated
/// telemetry, measurements, scalar traces, elapsed time, both RNG streams, and
/// the durable portion of the ergodic move system.
///
/// Checkpoints represent resumable runs after at least one completed
/// Metropolis step. Their current step is therefore stored and exposed as a
/// [`NonZeroU32`]; initial step-0 samples are measurement telemetry, not a
/// checkpoint position.
/// Deserialized checkpoints also validate that their stored action is finite
/// and matches the action recomputed from the restored triangulation.
///
/// # Serialization compatibility
///
/// [`Self::to_json`] emits a tagged version 1 record whose geometry uses
/// CDT-owned array indices rather than Delaunay TDS internals and whose chain
/// fields do not embed the MCMC crate's checkpoint representation. Releases that
/// support version 1 preserve this wire contract across compatible CDT,
/// `delaunay`, and `markov-chain-monte-carlo` upgrades. A future incompatible
/// format will receive a new tag; [`Self::from_json`] rejects unknown tags with
/// [`CdtError::UnsupportedCheckpointVersion`] and reports both versions.
///
/// Unversioned legacy payloads, including checkpoints written through the former
/// Delaunay 0.7 representation, are intentionally unsupported and must be
/// regenerated. Use trace CSV and simulation-summary JSON for durable analysis
/// and interchange; use this checkpoint JSON only when exact stochastic
/// continuation is required.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
/// };
///
/// fn main() -> CdtResult<()> {
///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
///     let checkpoint = MetropolisAlgorithm::new(
///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
///         ActionConfig::default(),
///     )
///     .run_to_checkpoint(tri)?;
///
///     assert_eq!(checkpoint.current_step().get(), 1);
///     assert_eq!(checkpoint.measurements().len(), 2);
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct CdtMcmcCheckpoint {
    pub(crate) chain: ChainCheckpoint<CdtTriangulation2D>,
    pub(crate) config: MetropolisConfig,
    pub(crate) action_config: ActionConfig,
    pub(crate) current_step: NonZeroU32,
    current_action: CheckpointAction,
    pub(crate) move_stats: MoveStatistics,
    pub(crate) proposal_stats: ProposalStatistics,
    pub(crate) steps: Vec<MonteCarloStep>,
    pub(crate) measurements: Vec<Measurement>,
    pub(crate) scalar_trace_rows: Vec<CdtScalarTraceRow>,
    pub(crate) elapsed_time: Duration,
    pub(crate) acceptance_rng: Xoshiro256PlusPlus,
    pub(crate) ergodics: ErgodicsSystem,
}

/// Borrowed top-level record emitted for checkpoint format version 1.
#[derive(Serialize)]
struct CdtMcmcCheckpointWireV1Ref<'a> {
    format_version: u32,
    triangulation: CdtTriangulationCheckpointWireV1,
    accepted: u64,
    rejected: u64,
    config: &'a MetropolisConfig,
    action_config: &'a ActionConfig,
    current_step: u32,
    current_action: f64,
    move_stats: &'a MoveStatistics,
    proposal_stats: &'a ProposalStatistics,
    steps: &'a [MonteCarloStep],
    measurements: &'a [Measurement],
    scalar_trace_rows: &'a [CdtScalarTraceRow],
    elapsed_time: CheckpointDurationWireV1,
    acceptance_rng: CheckpointRngWireV1,
    ergodics: ErgodicsCheckpointWireV1Ref<'a>,
}

/// Owned top-level record accepted for checkpoint format version 1.
#[derive(Deserialize)]
struct CdtMcmcCheckpointWireV1 {
    format_version: u32,
    triangulation: CdtTriangulationCheckpointWireV1,
    accepted: u64,
    rejected: u64,
    config: MetropolisConfig,
    action_config: ActionConfig,
    current_step: u32,
    current_action: f64,
    move_stats: MoveStatistics,
    #[serde(default)]
    proposal_stats: ProposalStatistics,
    steps: Vec<MonteCarloStep>,
    measurements: Vec<Measurement>,
    scalar_trace_rows: Vec<CdtScalarTraceRow>,
    elapsed_time: CheckpointDurationWireV1,
    acceptance_rng: CheckpointRngWireV1,
    ergodics: ErgodicsCheckpointWireV1,
}

/// Platform-neutral duration representation frozen into checkpoint format v1.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct CheckpointDurationWireV1 {
    secs: u64,
    nanos: u32,
}

/// Dependency-neutral Xoshiro state representation frozen into checkpoint format v1.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct CheckpointRngWireV1 {
    state: [u64; 4],
}

/// Borrowed durable portion of the CDT proposal system.
#[derive(Serialize)]
struct ErgodicsCheckpointWireV1Ref<'a> {
    stats: &'a MoveStatistics,
    rng: CheckpointRngWireV1,
}

/// Owned durable portion of the CDT proposal system.
#[derive(Deserialize)]
struct ErgodicsCheckpointWireV1 {
    stats: MoveStatistics,
    rng: CheckpointRngWireV1,
}

/// Current rand-serde shape used only as an adapter to the stable v1 record.
#[derive(Serialize, Deserialize)]
struct CurrentXoshiroSerde {
    s: [u64; 4],
}

impl Serialize for CdtMcmcCheckpoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.wire_v1()
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CdtMcmcCheckpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CdtMcmcCheckpointWireV1::deserialize(deserializer)?;
        Self::from_wire_v1(wire).map_err(DeError::custom)
    }
}

impl CdtMcmcCheckpoint {
    /// Current CDT-owned checkpoint wire-format version.
    pub const FORMAT_VERSION: u32 = CHECKPOINT_FORMAT_VERSION;

    /// Serializes this checkpoint as a versioned CDT-owned JSON document.
    ///
    /// The resulting v1 document contains no `delaunay` TDS snapshot or
    /// `markov-chain-monte-carlo` checkpoint object. Its geometry relations use
    /// array indices, and both RNG streams use explicit four-word state records.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointSerializationFailed`] if live geometry or
    /// RNG state cannot be projected into the v1 representation, or if JSON
    /// encoding fails.
    pub fn to_json(&self) -> CdtResult<String> {
        serde_json::to_string(&self.wire_v1()?).map_err(|error| {
            checkpoint_serialization_failed(CheckpointOperation::Serialize, error.to_string())
        })
    }

    /// Loads and fully validates a versioned CDT-owned JSON checkpoint.
    ///
    /// Version 1 reconstructs checked Level 1–4 geometry, then validates CDT
    /// topology, foliation, causality, chain accounting, telemetry, measurements,
    /// traces, action state, elapsed time, and both RNG streams before returning.
    /// Unversioned legacy checkpoint JSON is intentionally rejected; this release
    /// provides no migration reader for the former dependency-shaped payload.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::UnsupportedCheckpointVersion`] when the envelope names
    /// a version newer or older than [`Self::FORMAT_VERSION`]. Malformed JSON and
    /// missing or invalid version tags return
    /// [`CdtError::CheckpointSerializationFailed`]. Restored-state invariant
    /// failures retain their specific [`CdtError`] variants.
    pub fn from_json(json: &str) -> CdtResult<Self> {
        let value: Value = serde_json::from_str(json).map_err(|error| {
            checkpoint_serialization_failed(CheckpointOperation::Deserialize, error.to_string())
        })?;
        let encountered = checkpoint_format_version(&value)?;
        if encountered != u64::from(Self::FORMAT_VERSION) {
            return Err(CdtError::UnsupportedCheckpointVersion {
                encountered,
                supported: Self::FORMAT_VERSION,
            });
        }
        let wire: CdtMcmcCheckpointWireV1 = serde_json::from_value(value).map_err(|error| {
            checkpoint_serialization_failed(CheckpointOperation::Deserialize, error.to_string())
        })?;
        Self::from_wire_v1(wire)
    }

    /// Projects this checkpoint into the exact version 1 persistent record.
    fn wire_v1(&self) -> CdtResult<CdtMcmcCheckpointWireV1Ref<'_>> {
        let accepted = u64::try_from(self.chain.accepted()).map_err(|_| {
            checkpoint_serialization_failed(
                CheckpointOperation::Serialize,
                "accepted chain counter cannot be represented as u64".to_string(),
            )
        })?;
        let rejected = u64::try_from(self.chain.rejected()).map_err(|_| {
            checkpoint_serialization_failed(
                CheckpointOperation::Serialize,
                "rejected chain counter cannot be represented as u64".to_string(),
            )
        })?;

        Ok(CdtMcmcCheckpointWireV1Ref {
            format_version: Self::FORMAT_VERSION,
            triangulation: self.triangulation().checkpoint_wire_v1()?,
            accepted,
            rejected,
            config: &self.config,
            action_config: &self.action_config,
            current_step: self.current_step.get(),
            current_action: self.current_action.get(),
            move_stats: &self.move_stats,
            proposal_stats: &self.proposal_stats,
            steps: &self.steps,
            measurements: &self.measurements,
            scalar_trace_rows: &self.scalar_trace_rows,
            elapsed_time: CheckpointDurationWireV1 {
                secs: self.elapsed_time.as_secs(),
                nanos: self.elapsed_time.subsec_nanos(),
            },
            acceptance_rng: checkpoint_rng_wire(&self.acceptance_rng)?,
            ergodics: ErgodicsCheckpointWireV1Ref {
                stats: self.ergodics.stats(),
                rng: checkpoint_rng_wire(self.ergodics.checkpoint_rng())?,
            },
        })
    }

    /// Hydrates and validates the exact version 1 persistent record.
    fn from_wire_v1(wire: CdtMcmcCheckpointWireV1) -> CdtResult<Self> {
        if wire.format_version != Self::FORMAT_VERSION {
            return Err(CdtError::UnsupportedCheckpointVersion {
                encountered: u64::from(wire.format_version),
                supported: Self::FORMAT_VERSION,
            });
        }
        let accepted = usize::try_from(wire.accepted).map_err(|_| {
            checkpoint_resume_failed(CheckpointResumeFailure::CounterConversionOverflow {
                counter: CheckpointMoveCounter::Accepted,
            })
        })?;
        let rejected = usize::try_from(wire.rejected).map_err(|_| {
            checkpoint_resume_failed(CheckpointResumeFailure::CounterConversionOverflow {
                counter: CheckpointMoveCounter::Rejected,
            })
        })?;
        let current_step = checkpoint_current_step(wire.current_step)?;
        let current_action = CheckpointAction::new(wire.current_action)?;
        let elapsed_time = checkpoint_duration(wire.elapsed_time)?;
        let acceptance_rng = checkpoint_rng(wire.acceptance_rng)?;
        let ergodics = ErgodicsSystem::from_checkpoint_parts(
            wire.ergodics.stats,
            checkpoint_rng(wire.ergodics.rng)?,
        );
        let checkpoint = Self {
            chain: ChainCheckpoint::new(
                CdtTriangulation2D::from_checkpoint_wire_v1(wire.triangulation)?,
                accepted,
                rejected,
            ),
            config: wire.config,
            action_config: wire.action_config,
            current_step,
            current_action,
            move_stats: wire.move_stats,
            proposal_stats: wire.proposal_stats,
            steps: wire.steps,
            measurements: wire.measurements,
            scalar_trace_rows: wire.scalar_trace_rows,
            elapsed_time,
            acceptance_rng,
            ergodics,
        };
        validate_checkpoint_counters(&checkpoint)?;
        Ok(checkpoint)
    }

    pub(crate) fn from_parts(parts: CdtMcmcCheckpointParts) -> CdtResult<Self> {
        let checkpoint = Self::assemble(parts)?;
        validate_checkpoint_counters(&checkpoint)?;
        Ok(checkpoint)
    }

    /// Constructs a checkpoint by validating only telemetry appended to a trusted prefix.
    pub(crate) fn from_parts_after_validated_prefix(
        parts: CdtMcmcCheckpointParts,
        prefix: ValidatedCheckpointPrefix,
    ) -> CdtResult<Self> {
        let checkpoint = Self::assemble(parts)?;
        validate_checkpoint_after_prefix(&checkpoint, prefix)?;
        Ok(checkpoint)
    }

    /// Assembles typed checkpoint storage before cross-stream validation.
    fn assemble(parts: CdtMcmcCheckpointParts) -> CdtResult<Self> {
        let current_action = CheckpointAction::new(parts.current_action)?;
        Ok(Self {
            chain: ChainCheckpoint::new(parts.triangulation, parts.accepted, parts.rejected),
            config: parts.config,
            action_config: parts.action_config,
            current_step: parts.current_step,
            current_action,
            move_stats: parts.move_stats,
            proposal_stats: parts.proposal_stats,
            steps: parts.steps,
            measurements: parts.measurements,
            scalar_trace_rows: parts.scalar_trace_rows,
            elapsed_time: parts.elapsed_time,
            acceptance_rng: parts.acceptance_rng,
            ergodics: parts.ergodics,
        })
    }

    /// Captures the boundary already proven by a full checkpoint validation.
    pub(crate) fn validated_prefix(&self) -> CdtResult<ValidatedCheckpointPrefix> {
        Ok(ValidatedCheckpointPrefix {
            steps: self.steps.len(),
            measurements: self.measurements.len(),
            scalar_trace_rows: self.scalar_trace_rows.len(),
            accepted_steps: self.chain.accepted(),
            scalar_accepted: self.proposal_stats.accepted_transitions(),
            scalar_rejected_proposal: self.proposal_stats.metropolis_rejections(),
            scalar_no_proposal: scalar_trace_no_proposal_count(&self.proposal_stats)?,
        })
    }

    /// Returns the generic MCMC chain checkpoint for upstream interop.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.chain().total_steps(), 1);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub const fn chain(&self) -> &ChainCheckpoint<CdtTriangulation2D> {
        &self.chain
    }

    /// Returns the checkpointed triangulation state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.triangulation().time_slices().get(), 3);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn triangulation(&self) -> &CdtTriangulation2D {
        self.chain.state()
    }

    /// Returns the Metropolis configuration used when the checkpoint was made.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.config().steps().get(), 1);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn config(&self) -> &MetropolisConfig {
        &self.config
    }

    /// Returns the action configuration used when the checkpoint was made.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.action_config(), &ActionConfig::default());
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn action_config(&self) -> &ActionConfig {
        &self.action_config
    }

    /// Returns the nonzero last completed Monte Carlo step.
    ///
    /// The value is a [`NonZeroU32`] because checkpoints are produced only
    /// after at least one Metropolis step has completed. Use
    /// [`NonZeroU32::get`] when interoperating with raw serialized step
    /// counters or measurement rows.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.current_step().get(), 1);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn current_step(&self) -> NonZeroU32 {
        self.current_step
    }

    /// Returns the action of the checkpointed triangulation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert!(checkpoint.current_action().is_finite());
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn current_action(&self) -> f64 {
        self.current_action.get()
    }

    /// Returns accumulated move statistics through the checkpoint step.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.move_stats().total_attempted(), 1);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn move_stats(&self) -> &MoveStatistics {
        &self.move_stats
    }

    /// Returns accumulated proposal-kernel telemetry through the checkpoint step.
    ///
    /// Checkpoint validation requires these counters to account for exactly the
    /// same move-family proposals and accepted/rejected transitions as the
    /// stored chain and step telemetry.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.proposal_stats().move_family_proposals(), 1);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn proposal_stats(&self) -> &ProposalStatistics {
        &self.proposal_stats
    }

    /// Returns accumulated step telemetry through the checkpoint step.
    ///
    /// Step telemetry starts at step 1 and uses
    /// [`MonteCarloStep::step`](super::MonteCarloStep::step) to preserve that
    /// nonzero invariant. Initial step-0 samples, when present, are returned by
    /// [`Self::measurements`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.steps().len(), checkpoint.current_step().get() as usize);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub fn steps(&self) -> &[MonteCarloStep] {
        &self.steps
    }

    /// Returns accumulated measurements through the checkpoint step.
    ///
    /// Measurements follow the configured post-thermalization cadence. The
    /// initial step `0` appears only when the checkpoint schedule has zero
    /// thermalization.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMcmcCheckpoint, CdtResult, CdtTriangulation,
    ///     MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// # fn checkpoint() -> CdtResult<CdtMcmcCheckpoint> {
    /// MetropolisAlgorithm::new(
    ///     MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///     ActionConfig::default(),
    /// )
    /// .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)
    /// # }
    /// # let checkpoint = checkpoint()?;
    /// assert_eq!(checkpoint.measurements().first().map(|m| m.step()), Some(0));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Reconstructs the chronological simulation history without allocating.
    ///
    /// The returned iterator borrows the checkpoint's canonical step,
    /// measurement, and triangulation metadata instead of reading a duplicate
    /// serialized event log.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    ///     SimulationEvent,
    /// };
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let checkpoint = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///         ActionConfig::default(),
    ///     )
    ///     .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///
    ///     assert_matches!(
    ///         checkpoint.simulation_history().next(),
    ///         Some(SimulationEvent::Created { .. })
    ///     );
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn simulation_history(&self) -> SimulationHistory<'_> {
        SimulationHistory::new(self.triangulation(), &self.steps, &self.measurements)
    }

    /// Converts the checkpoint into a complete simulation result snapshot.
    ///
    /// This consumes the checkpoint and keeps all accumulated steps,
    /// measurements, move statistics, proposal statistics, elapsed time, and the
    /// checkpointed triangulation in the returned [`SimulationResultsBackend`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError`] if this checkpoint's components no longer satisfy
    /// the result-snapshot invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let checkpoint = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
    ///         ActionConfig::default(),
    ///     )
    ///     .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///
    ///     let results = checkpoint.into_results()?;
    ///     assert_eq!(results.steps().len(), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn into_results(self) -> CdtResult<SimulationResultsBackend> {
        let (triangulation, _, _) = self.chain.into_parts();
        let parts = SimulationResultsParts::new(
            self.config,
            self.action_config,
            self.move_stats,
            self.proposal_stats,
            self.steps,
            self.measurements,
            self.scalar_trace_rows,
            self.elapsed_time,
            triangulation,
        )?;
        Ok(SimulationResultsBackend::from_parts(parts))
    }
}

/// Reads the numeric version tag before attempting to decode a version payload.
fn checkpoint_format_version(value: &Value) -> CdtResult<u64> {
    value
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            checkpoint_serialization_failed(
                CheckpointOperation::Deserialize,
                "missing or non-integer `format_version` tag; unversioned checkpoints are not supported"
                    .to_string(),
            )
        })
}

/// Wraps an encoding diagnostic in the public checkpoint I/O error contract.
fn checkpoint_serialization_failed(operation: CheckpointOperation, detail: String) -> CdtError {
    CdtError::CheckpointSerializationFailed {
        operation,
        target: "MCMC".to_string(),
        detail,
    }
}

/// Projects the current rand implementation's private serde shape into v1 state words.
fn checkpoint_rng_wire(rng: &Xoshiro256PlusPlus) -> CdtResult<CheckpointRngWireV1> {
    let value = serde_json::to_value(rng).map_err(|error| {
        checkpoint_serialization_failed(CheckpointOperation::Serialize, error.to_string())
    })?;
    let current: CurrentXoshiroSerde = serde_json::from_value(value).map_err(|error| {
        checkpoint_serialization_failed(CheckpointOperation::Serialize, error.to_string())
    })?;
    if current.s == [0; 4] {
        return Err(checkpoint_serialization_failed(
            CheckpointOperation::Serialize,
            "Xoshiro RNG has an invalid all-zero state".to_string(),
        ));
    }
    Ok(CheckpointRngWireV1 { state: current.s })
}

/// Hydrates v1 state words through the current rand implementation's serde adapter.
fn checkpoint_rng(wire: CheckpointRngWireV1) -> CdtResult<Xoshiro256PlusPlus> {
    if wire.state == [0; 4] {
        return Err(checkpoint_serialization_failed(
            CheckpointOperation::Deserialize,
            "Xoshiro RNG has an invalid all-zero state".to_string(),
        ));
    }
    let value = serde_json::to_value(CurrentXoshiroSerde { s: wire.state }).map_err(|error| {
        checkpoint_serialization_failed(CheckpointOperation::Deserialize, error.to_string())
    })?;
    serde_json::from_value(value).map_err(|error| {
        checkpoint_serialization_failed(CheckpointOperation::Deserialize, error.to_string())
    })
}

/// Parses the normalized seconds/nanoseconds duration representation.
fn checkpoint_duration(wire: CheckpointDurationWireV1) -> CdtResult<Duration> {
    if wire.nanos >= 1_000_000_000 {
        return Err(checkpoint_serialization_failed(
            CheckpointOperation::Deserialize,
            format!(
                "elapsed-time nanoseconds {} must be less than 1000000000",
                wire.nanos
            ),
        ));
    }
    Ok(Duration::new(wire.secs, wire.nanos))
}

/// Builds the public checkpoint-resume error wrapper for CDT-owned resume invariants.
pub(crate) const fn checkpoint_resume_failed(failure: CheckpointResumeFailure) -> CdtError {
    CdtError::CheckpointResumeFailed { failure }
}

/// Parses a raw checkpoint step into the nonzero resumable-step invariant.
const fn checkpoint_current_step(step: u32) -> CdtResult<NonZeroU32> {
    match NonZeroU32::new(step) {
        Some(step) => Ok(step),
        None => Err(checkpoint_resume_failed(
            CheckpointResumeFailure::CheckpointCurrentStepZero { actual: step },
        )),
    }
}

/// Verifies that a checkpoint can be resumed by the requested algorithm.
///
/// Resume accepts a different fresh seed because serialized checkpoints carry
/// their own RNG streams, but rejects physics and sampling schedule changes
/// that would make the cumulative chain scientifically ambiguous.
///
/// # Errors
///
/// Returns [`CdtError::CheckpointResumeFailed`] when physics settings or
/// sampling schedule settings differ from the checkpoint, or when the
/// checkpoint counters and telemetry fail validation.
pub(crate) fn validate_resume_compatible<P>(
    algorithm: &MetropolisAlgorithm<P>,
    checkpoint: &CdtMcmcCheckpoint,
) -> CdtResult<()> {
    ResumeCompatibleActionConfig::new(algorithm.action_config(), &checkpoint.action_config)?;
    if algorithm.config().temperature().to_bits() != checkpoint.config.temperature().to_bits() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::IncompatibleTemperature,
        ));
    }
    if algorithm.config().thermalization_steps() != checkpoint.config.thermalization_steps() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::IncompatibleThermalizationSchedule,
        ));
    }
    if algorithm.config().measurement_frequency() != checkpoint.config.measurement_frequency() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::IncompatibleMeasurementFrequency,
        ));
    }
    validate_checkpoint_counters(checkpoint)
}

/// Proof that requested and checkpointed action couplings describe the same resume target.
struct ResumeCompatibleActionConfig;

impl ResumeCompatibleActionConfig {
    /// Parses two action configurations into resume-compatible physics evidence.
    const fn new(requested: &ActionConfig, checkpointed: &ActionConfig) -> CdtResult<Self> {
        if action_configs_match(requested, checkpointed) {
            Ok(Self)
        } else {
            Err(checkpoint_resume_failed(
                CheckpointResumeFailure::IncompatibleActionConfiguration,
            ))
        }
    }
}

/// Compares action couplings by exact value or one-ULP serde JSON round-trip drift.
const fn action_configs_match(left: &ActionConfig, right: &ActionConfig) -> bool {
    resume_couplings_match(left.coupling_0(), right.coupling_0())
        && resume_couplings_match(left.coupling_2(), right.coupling_2())
        && resume_couplings_match(left.cosmological_constant(), right.cosmological_constant())
}

/// Accepts exact couplings plus adjacent finite values introduced by JSON round trips.
const fn resume_couplings_match(left: f64, right: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && ordered_f64_bits(left).abs_diff(ordered_f64_bits(right)) <= 1
}

/// Maps finite `f64` values into a monotonic bit ordering for ULP distance checks.
const fn ordered_f64_bits(value: f64) -> u64 {
    const SIGN_MASK: u64 = 1 << 63;
    let bits = value.to_bits();
    if bits & SIGN_MASK == 0 {
        bits | SIGN_MASK
    } else {
        !bits
    }
}

/// Checks that serialized chain counters and CDT telemetry agree.
///
/// This protects the public resume contract by rejecting checkpoints whose
/// generic MCMC counters, CDT move counters, step telemetry, or measurement
/// schedule cannot all describe the same chain prefix.
///
/// # Errors
///
/// Returns [`CdtError::CheckpointResumeFailed`] when serialized chain counters,
/// step telemetry, measurements, scalar trace rows, or stored action do not
/// match the configured sampling schedule and restored triangulation state.
/// [`MetropolisConfig`] and [`ActionConfig`] validate before storage, so this
/// helper only audits relationships among their checkpointed data.
pub(crate) fn validate_checkpoint_counters(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    validate_checkpoint_summary(checkpoint)?;
    validate_checkpoint_steps(checkpoint)?;
    validate_checkpoint_measurements(checkpoint)?;
    validate_scalar_trace_rows(
        &checkpoint.config,
        &checkpoint.proposal_stats,
        &checkpoint.steps,
        &checkpoint.scalar_trace_rows,
    )?;
    validate_trajectory_observables(
        &checkpoint.action_config,
        &checkpoint.steps,
        &checkpoint.measurements,
        &checkpoint.scalar_trace_rows,
        checkpoint.triangulation(),
    )
}

/// Checks aggregate checkpoint state without replaying its telemetry streams.
fn validate_checkpoint_summary(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    checkpoint.config.validate();
    checkpoint.action_config.validate();
    validate_checkpoint_current_action(checkpoint)?;

    let (accepted, rejected) = chain_counters(&checkpoint.move_stats)?;
    if checkpoint.chain.accepted() != accepted || checkpoint.chain.rejected() != rejected {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ChainCounterMismatch {
                chain_accepted: checkpoint.chain.accepted(),
                chain_rejected: checkpoint.chain.rejected(),
                move_accepted: accepted,
                move_rejected: rejected,
            },
        ));
    }
    let current_step = checkpoint.current_step.get();
    let checkpoint_step = usize::try_from(current_step).unwrap_or(usize::MAX);
    if checkpoint.chain.total_steps() != checkpoint_step {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ChainStepMismatch {
                chain_steps: checkpoint.chain.total_steps(),
                checkpoint_step: current_step,
            },
        ));
    }
    if checkpoint.steps.len() != checkpoint.chain.total_steps() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::StepTelemetryLengthMismatch {
                actual: checkpoint.steps.len(),
                expected: checkpoint.chain.total_steps(),
            },
        ));
    }
    validate_checkpoint_proposal_stats(checkpoint, accepted, rejected)?;
    Ok(())
}

/// Validates telemetry generated after one trusted in-process checkpoint boundary.
fn validate_checkpoint_after_prefix(
    checkpoint: &CdtMcmcCheckpoint,
    prefix: ValidatedCheckpointPrefix,
) -> CdtResult<()> {
    validate_checkpoint_summary(checkpoint)?;
    validate_checkpoint_step_suffix(checkpoint, prefix)?;
    validate_checkpoint_measurement_suffix(checkpoint, prefix)?;
    validate_checkpoint_scalar_trace_suffix(checkpoint, prefix)?;

    let steps = checkpoint.steps.get(prefix.steps..).ok_or_else(|| {
        checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryLengthMismatch {
            actual: checkpoint.steps.len(),
            expected: prefix.steps,
        })
    })?;
    let measurements = checkpoint
        .measurements
        .get(prefix.measurements..)
        .ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::MeasurementCountMismatch {
                actual: checkpoint.measurements.len(),
                expected: prefix.measurements,
            })
        })?;
    let scalar_trace_rows = checkpoint
        .scalar_trace_rows
        .get(prefix.scalar_trace_rows..)
        .ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::ScalarTraceLengthMismatch {
                actual: checkpoint.scalar_trace_rows.len(),
                expected: prefix.scalar_trace_rows,
            })
        })?;
    // These three streams contain only entries appended after the trusted
    // prefix. Trajectory validation rewinds the suffix steps from the final
    // triangulation to reconstruct and validate the prefix-boundary state.
    validate_trajectory_observables(
        &checkpoint.action_config,
        steps,
        measurements,
        scalar_trace_rows,
        checkpoint.triangulation(),
    )
}

/// Verifies that the stored checkpoint action is finite and matches the restored state.
fn validate_checkpoint_current_action(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    let recomputed = action_for(&checkpoint.action_config, checkpoint.triangulation());
    let stored = checkpoint.current_action.get();
    if !actions_match(stored, recomputed) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ActionMismatch { stored, recomputed },
        ));
    }
    Ok(())
}

/// Checks proposal telemetry against checkpoint step and chain counters.
///
/// This preserves the public resume contract: a deserialized checkpoint must not
/// silently default, drop, or double-count proposal outcomes relative to the
/// MCMC chain prefix it claims to resume.
fn validate_checkpoint_proposal_stats(
    checkpoint: &CdtMcmcCheckpoint,
    accepted: usize,
    rejected: usize,
) -> CdtResult<()> {
    if checkpoint.proposal_stats.hard_failures() != 0 {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalHardFailures {
                actual: checkpoint.proposal_stats.hard_failures(),
            },
        ));
    }

    let steps = u64::try_from(checkpoint.steps.len()).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
            counter: ProposalTelemetryCounter::MoveFamilyProposals,
        })
    })?;
    if checkpoint.proposal_stats.move_family_proposals() != steps {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalMoveFamilyCountMismatch {
                actual: checkpoint.proposal_stats.move_family_proposals(),
                expected: steps,
            },
        ));
    }

    let accepted = u64::try_from(accepted).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
            counter: ProposalTelemetryCounter::AcceptedTransitions,
        })
    })?;
    if checkpoint.proposal_stats.accepted_transitions() != accepted {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ProposalAcceptedCountMismatch {
                actual: checkpoint.proposal_stats.accepted_transitions(),
                expected: accepted,
            },
        ));
    }

    let rejected = u64::try_from(rejected).map_err(|_| {
        checkpoint_resume_failed(CheckpointResumeFailure::ProposalCounterOverflow {
            counter: ProposalTelemetryCounter::RejectedTransitions,
        })
    })?;
    let actual_rejected = checkpoint.proposal_stats.rejected_transitions();
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

/// Checks that serialized per-step telemetry forms the exact prefix being resumed.
fn validate_checkpoint_steps(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    let accepted_steps = checkpoint
        .steps
        .iter()
        .filter(|step| step.accepted())
        .count();
    if accepted_steps != checkpoint.chain.accepted() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::StepTelemetryAcceptedCountMismatch {
                actual: accepted_steps,
                expected: checkpoint.chain.accepted(),
            },
        ));
    }

    for (index, step) in checkpoint.steps.iter().enumerate() {
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

/// Checks only steps appended after an already validated checkpoint prefix.
fn validate_checkpoint_step_suffix(
    checkpoint: &CdtMcmcCheckpoint,
    prefix: ValidatedCheckpointPrefix,
) -> CdtResult<()> {
    let suffix = checkpoint.steps.get(prefix.steps..).ok_or_else(|| {
        checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryLengthMismatch {
            actual: checkpoint.steps.len(),
            expected: prefix.steps,
        })
    })?;
    let suffix_accepted = suffix.iter().filter(|step| step.accepted()).count();
    let actual_accepted = prefix.accepted_steps.saturating_add(suffix_accepted);
    if actual_accepted != checkpoint.chain.accepted() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::StepTelemetryAcceptedCountMismatch {
                actual: actual_accepted,
                expected: checkpoint.chain.accepted(),
            },
        ));
    }

    for (offset, step) in suffix.iter().enumerate() {
        let expected_step = prefix
            .steps
            .checked_add(offset)
            .and_then(|index| index.checked_add(1))
            .and_then(|index| u32::try_from(index).ok())
            .ok_or_else(|| {
                checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryIndexOverflow)
            })?;
        if step.step().get() != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::StepTelemetrySequenceMismatch {
                    actual: step.step().get(),
                    expected: expected_step,
                },
            ));
        }
    }
    Ok(())
}

/// Checks that serialized measurements match the configured post-thermalization schedule.
fn validate_checkpoint_measurements(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    let expected_measurements = expected_measurement_count(
        checkpoint.current_step.get(),
        checkpoint.config.thermalization_steps(),
        checkpoint.config.measurement_frequency(),
    )
    .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::MeasurementCountOverflow))?;
    if checkpoint.measurements.len() != expected_measurements {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::MeasurementCountMismatch {
                actual: checkpoint.measurements.len(),
                expected: expected_measurements,
            },
        ));
    }

    for (index, measurement) in checkpoint.measurements.iter().enumerate() {
        let expected_step = expected_measurement_step(
            index,
            checkpoint.config.thermalization_steps(),
            checkpoint.config.measurement_frequency(),
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

/// Checks only measurements appended after an already validated checkpoint prefix.
fn validate_checkpoint_measurement_suffix(
    checkpoint: &CdtMcmcCheckpoint,
    prefix: ValidatedCheckpointPrefix,
) -> CdtResult<()> {
    let expected_measurements = expected_measurement_count(
        checkpoint.current_step.get(),
        checkpoint.config.thermalization_steps(),
        checkpoint.config.measurement_frequency(),
    )
    .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::MeasurementCountOverflow))?;
    if checkpoint.measurements.len() != expected_measurements {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::MeasurementCountMismatch {
                actual: checkpoint.measurements.len(),
                expected: expected_measurements,
            },
        ));
    }

    let suffix = checkpoint
        .measurements
        .get(prefix.measurements..)
        .ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::MeasurementCountMismatch {
                actual: checkpoint.measurements.len(),
                expected: prefix.measurements,
            })
        })?;
    for (offset, measurement) in suffix.iter().enumerate() {
        let index = prefix.measurements.checked_add(offset).ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::MeasurementStepOverflow)
        })?;
        let expected_step = expected_measurement_step(
            index,
            checkpoint.config.thermalization_steps(),
            checkpoint.config.measurement_frequency(),
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

/// Checks only scalar rows appended after an already validated checkpoint prefix.
fn validate_checkpoint_scalar_trace_suffix(
    checkpoint: &CdtMcmcCheckpoint,
    prefix: ValidatedCheckpointPrefix,
) -> CdtResult<()> {
    if checkpoint.scalar_trace_rows.len() != checkpoint.steps.len() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ScalarTraceLengthMismatch {
                actual: checkpoint.scalar_trace_rows.len(),
                expected: checkpoint.steps.len(),
            },
        ));
    }
    let steps = checkpoint.steps.get(prefix.steps..).ok_or_else(|| {
        checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryLengthMismatch {
            actual: checkpoint.steps.len(),
            expected: prefix.steps,
        })
    })?;
    let rows = checkpoint
        .scalar_trace_rows
        .get(prefix.scalar_trace_rows..)
        .ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::ScalarTraceLengthMismatch {
                actual: checkpoint.scalar_trace_rows.len(),
                expected: prefix.scalar_trace_rows,
            })
        })?;
    let accepted = checkpoint
        .proposal_stats
        .accepted_transitions()
        .checked_sub(prefix.scalar_accepted)
        .ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::ScalarTraceAcceptedCountMismatch {
                actual: checkpoint.proposal_stats.accepted_transitions(),
                expected: prefix.scalar_accepted,
            })
        })?;
    let rejected_proposal = checkpoint
        .proposal_stats
        .metropolis_rejections()
        .checked_sub(prefix.scalar_rejected_proposal)
        .ok_or_else(|| {
            checkpoint_resume_failed(
                CheckpointResumeFailure::ScalarTraceRejectedProposalCountMismatch {
                    actual: checkpoint.proposal_stats.metropolis_rejections(),
                    expected: prefix.scalar_rejected_proposal,
                },
            )
        })?;
    let total_no_proposal = scalar_trace_no_proposal_count(&checkpoint.proposal_stats)?;
    let no_proposal = total_no_proposal
        .checked_sub(prefix.scalar_no_proposal)
        .ok_or_else(|| {
            checkpoint_resume_failed(
                CheckpointResumeFailure::ScalarTraceNoProposalCountMismatch {
                    actual: total_no_proposal,
                    expected: prefix.scalar_no_proposal,
                },
            )
        })?;
    validate_scalar_trace_row_slice(
        &checkpoint.config,
        steps,
        rows,
        accepted,
        rejected_proposal,
        no_proposal,
    )
}

/// Sums move counters without allowing invalid serialized telemetry to wrap.
fn checked_move_counter_sum(counter: CheckpointMoveCounter, counters: [u64; 4]) -> CdtResult<u64> {
    counters.into_iter().try_fold(0_u64, |total, count| {
        total.checked_add(count).ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::MoveCounterOverflow { counter })
        })
    })
}

/// Rejects per-move counter states that cannot be produced by the sampler.
fn validate_move_counter_bounds(move_stats: &MoveStatistics) -> CdtResult<()> {
    let counters = [
        (
            MoveType::Move22,
            move_stats.attempted(MoveType::Move22),
            move_stats.accepted(MoveType::Move22),
            move_stats.hard_failed(MoveType::Move22),
        ),
        (
            MoveType::Move13Add,
            move_stats.attempted(MoveType::Move13Add),
            move_stats.accepted(MoveType::Move13Add),
            move_stats.hard_failed(MoveType::Move13Add),
        ),
        (
            MoveType::Move31Remove,
            move_stats.attempted(MoveType::Move31Remove),
            move_stats.accepted(MoveType::Move31Remove),
            move_stats.hard_failed(MoveType::Move31Remove),
        ),
        (
            MoveType::EdgeFlip,
            move_stats.attempted(MoveType::EdgeFlip),
            move_stats.accepted(MoveType::EdgeFlip),
            move_stats.hard_failed(MoveType::EdgeFlip),
        ),
    ];

    for (move_type, attempted, accepted, hard_failed) in counters {
        if hard_failed != 0 {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::MoveHardFailures { move_type },
            ));
        }

        if accepted > attempted {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::MoveAcceptedExceedsAttempted { move_type },
            ));
        }
    }

    Ok(())
}

/// Converts CDT move statistics into generic MCMC chain counters.
///
/// Accepted and rejected counts are derived from proposal accounting, with
/// overflow and impossible accepted-above-attempted states reported as
/// checkpoint resume errors instead of panicking.
///
/// # Errors
///
/// Returns [`CdtError::CheckpointResumeFailed`] when serialized move counters
/// contain hard failures, accepted counts above attempted counts, arithmetic
/// overflow, or values that cannot fit into the upstream checkpoint counter
/// type.
pub(crate) fn chain_counters(move_stats: &MoveStatistics) -> CdtResult<(usize, usize)> {
    validate_move_counter_bounds(move_stats)?;
    let attempted = checked_move_counter_sum(
        CheckpointMoveCounter::Attempted,
        [
            move_stats.attempted(MoveType::Move22),
            move_stats.attempted(MoveType::Move13Add),
            move_stats.attempted(MoveType::Move31Remove),
            move_stats.attempted(MoveType::EdgeFlip),
        ],
    )?;
    let accepted = checked_move_counter_sum(
        CheckpointMoveCounter::Accepted,
        [
            move_stats.accepted(MoveType::Move22),
            move_stats.accepted(MoveType::Move13Add),
            move_stats.accepted(MoveType::Move31Remove),
            move_stats.accepted(MoveType::EdgeFlip),
        ],
    )?;
    let rejected = attempted.checked_sub(accepted).ok_or_else(|| {
        checkpoint_resume_failed(CheckpointResumeFailure::TotalAcceptedExceedsAttempted)
    })?;
    Ok((
        usize::try_from(accepted).map_err(|_| {
            checkpoint_resume_failed(CheckpointResumeFailure::CounterConversionOverflow {
                counter: CheckpointMoveCounter::Accepted,
            })
        })?,
        usize::try_from(rejected).map_err(|_| {
            checkpoint_resume_failed(CheckpointResumeFailure::CounterConversionOverflow {
                counter: CheckpointMoveCounter::Rejected,
            })
        })?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::triangulation::CdtTriangulation;
    use serde_json::{json, to_value};
    use std::assert_matches;
    use std::num::NonZeroUsize;

    fn one_step_checkpoint() -> CdtMcmcCheckpoint {
        MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 1, 0, 1)
                .expect("test configuration should validate")
                .with_seed(13),
            ActionConfig::default(),
        )
        .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3).expect("CDT strip should build"))
        .expect("one-step run should checkpoint")
    }

    fn one_step_checkpoint_payload() -> Value {
        to_value(
            one_step_checkpoint()
                .wire_v1()
                .expect("wire projection should work"),
        )
        .expect("wire should serialize")
    }

    fn assert_checkpoint_deserialization_detail(error: &CdtError, expected: &str) {
        assert_matches!(
            error,
            CdtError::CheckpointSerializationFailed {
                operation: CheckpointOperation::Deserialize,
                detail,
                ..
            } if detail.contains(expected)
        );
    }

    fn checkpoint_json_error(json: &str, expectation: &str) -> CdtError {
        let Err(error) = CdtMcmcCheckpoint::from_json(json) else {
            panic!("{expectation}");
        };
        error
    }

    #[test]
    fn checkpoint_current_step_rejects_zero_with_typed_failure() {
        let error = checkpoint_current_step(0)
            .expect_err("zero checkpoint step should not become a resumable position");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::CheckpointCurrentStepZero { actual: 0 }
            }
        );
    }

    #[test]
    fn checkpoint_json_uses_cdt_owned_v1_shape() {
        let checkpoint = one_step_checkpoint();
        let json = checkpoint.to_json().expect("checkpoint should serialize");
        let payload: Value = serde_json::from_str(&json).expect("checkpoint JSON should parse");

        assert_eq!(payload["format_version"], json!(1));
        assert!(payload.get("chain").is_none());
        assert!(payload["triangulation"]["geometry"]["vertices"].is_array());
        assert!(payload["triangulation"]["geometry"]["simplices"].is_array());
        assert!(payload["acceptance_rng"]["state"].is_array());
        assert!(payload["ergodics"]["rng"]["state"].is_array());
        assert!(!json.contains("\"tds\""));
        assert!(!json.contains("simplex_vertices"));
        assert!(!json.contains("simplex_neighbors"));

        let restored =
            CdtMcmcCheckpoint::from_json(&json).expect("version 1 checkpoint should deserialize");
        let restored_payload: Value = serde_json::from_str(
            &restored
                .to_json()
                .expect("restored checkpoint should serialize"),
        )
        .expect("restored checkpoint JSON should parse");
        assert_eq!(payload, restored_payload);
    }

    #[test]
    fn checkpoint_json_rejects_unknown_version_with_typed_context() {
        let checkpoint = one_step_checkpoint();
        let mut payload = to_value(checkpoint.wire_v1().expect("wire projection should work"))
            .expect("wire should serialize");
        payload["format_version"] = json!(2);

        let Err(error) = CdtMcmcCheckpoint::from_json(&payload.to_string()) else {
            panic!("unsupported checkpoint version should be rejected");
        };
        assert_matches!(
            error,
            CdtError::UnsupportedCheckpointVersion {
                encountered: 2,
                supported: CdtMcmcCheckpoint::FORMAT_VERSION,
            }
        );
    }

    #[test]
    fn checkpoint_json_wraps_malformed_document() {
        let error = checkpoint_json_error("{", "malformed checkpoint JSON should be rejected");

        assert_checkpoint_deserialization_detail(&error, "EOF while parsing");
    }

    #[test]
    fn checkpoint_json_wraps_malformed_v1_body() {
        let mut payload = one_step_checkpoint_payload();
        payload
            .as_object_mut()
            .expect("wire should be a JSON object")
            .remove("accepted");

        let error = checkpoint_json_error(
            &payload.to_string(),
            "incomplete version 1 body should be rejected",
        );

        assert_checkpoint_deserialization_detail(&error, "missing field `accepted`");
    }

    #[test]
    fn checkpoint_serde_rejects_unknown_version() {
        let mut payload = one_step_checkpoint_payload();
        payload["format_version"] = json!(2);

        let Err(error) = serde_json::from_value::<CdtMcmcCheckpoint>(payload) else {
            panic!("Serde entry point should enforce the wire version");
        };

        assert!(
            error
                .to_string()
                .contains("Unsupported MCMC checkpoint format version 2")
        );
    }

    #[test]
    fn checkpoint_json_rejects_all_zero_rng_states() {
        let mut acceptance_payload = one_step_checkpoint_payload();
        acceptance_payload["acceptance_rng"]["state"] = json!([0, 0, 0, 0]);
        let acceptance_error = checkpoint_json_error(
            &acceptance_payload.to_string(),
            "all-zero acceptance RNG should be rejected",
        );
        assert_checkpoint_deserialization_detail(&acceptance_error, "invalid all-zero state");

        let mut proposal_payload = one_step_checkpoint_payload();
        proposal_payload["ergodics"]["rng"]["state"] = json!([0, 0, 0, 0]);
        let proposal_error = checkpoint_json_error(
            &proposal_payload.to_string(),
            "all-zero proposal RNG should be rejected",
        );
        assert_checkpoint_deserialization_detail(&proposal_error, "invalid all-zero state");
    }

    #[test]
    fn checkpoint_json_rejects_non_normalized_duration() {
        let mut payload = one_step_checkpoint_payload();
        payload["elapsed_time"]["nanos"] = json!(1_000_000_000_u64);

        let error = checkpoint_json_error(
            &payload.to_string(),
            "non-normalized duration should be rejected",
        );

        assert_checkpoint_deserialization_detail(&error, "must be less than 1000000000");
    }

    #[test]
    fn checkpoint_json_rejects_wrong_geometry_dimension() {
        let mut payload = one_step_checkpoint_payload();
        payload["triangulation"]["geometry"]["vertices"][0]["coordinates"] = json!([0.0]);

        let error = checkpoint_json_error(
            &payload.to_string(),
            "wrong-dimensional geometry should be rejected",
        );

        assert_checkpoint_deserialization_detail(&error, "coordinate dimension 1; expected 2");
    }

    #[test]
    fn checkpoint_json_rejects_zero_foliation_slice_count() {
        let mut payload = one_step_checkpoint_payload();
        payload["triangulation"]["foliation"]["num_slices"] = json!(0);

        let error = checkpoint_json_error(
            &payload.to_string(),
            "zero foliation slice count should be rejected",
        );

        assert_checkpoint_deserialization_detail(&error, "`num_slices` must be nonzero");
    }

    #[test]
    fn checkpoint_json_rejects_foliation_metadata_slice_count_mismatch() {
        let mut payload = one_step_checkpoint_payload();
        payload["triangulation"]["foliation"]["num_slices"] = json!(2);

        let error = checkpoint_json_error(
            &payload.to_string(),
            "foliation and metadata slice counts should agree",
        );

        assert_checkpoint_deserialization_detail(
            &error,
            "foliation `num_slices` 2 does not match metadata `time_slices` 3",
        );
    }

    #[test]
    fn checkpoint_json_rejects_zero_delaunay_check_interval() {
        let mut payload = one_step_checkpoint_payload();
        payload["triangulation"]["geometry"]["delaunay_check_policy"] = json!({ "EveryN": 0 });

        let error = checkpoint_json_error(
            &payload.to_string(),
            "zero Delaunay validation cadence should be rejected",
        );

        assert_checkpoint_deserialization_detail(
            &error,
            "delaunay check interval must be non-zero",
        );
    }

    #[test]
    fn checkpoint_v1_preserves_delaunay_check_interval() {
        let mut triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("CDT strip should build");
        triangulation.set_delaunay_check_interval(NonZeroUsize::new(8));
        let checkpoint = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 1, 0, 1)
                .expect("test configuration should validate")
                .with_seed(17),
            ActionConfig::default(),
        )
        .run_to_checkpoint(triangulation)
        .expect("one-step run should checkpoint");

        let json = checkpoint.to_json().expect("checkpoint should serialize");
        let restored = CdtMcmcCheckpoint::from_json(&json)
            .expect("checkpoint with validation cadence should restore");

        assert!(
            !restored
                .triangulation()
                .geometry()
                .should_check_delaunay_after(7)
        );
        assert!(
            restored
                .triangulation()
                .geometry()
                .should_check_delaunay_after(8)
        );
    }

    #[test]
    fn checkpoint_json_rejects_unversioned_legacy_payload() {
        let checkpoint = one_step_checkpoint();
        let mut payload = to_value(checkpoint.wire_v1().expect("wire projection should work"))
            .expect("wire should serialize");
        payload
            .as_object_mut()
            .expect("wire should be a JSON object")
            .remove("format_version");

        let Err(error) = CdtMcmcCheckpoint::from_json(&payload.to_string()) else {
            panic!("unversioned checkpoint should be rejected");
        };
        assert_matches!(
            error,
            CdtError::CheckpointSerializationFailed {
                operation: CheckpointOperation::Deserialize,
                ref detail,
                ..
            } if detail.contains("unversioned checkpoints are not supported")
        );
    }

    #[test]
    fn checkpoint_json_rejects_out_of_bounds_geometry_relation() {
        let checkpoint = one_step_checkpoint();
        let mut payload = to_value(checkpoint.wire_v1().expect("wire projection should work"))
            .expect("wire should serialize");
        payload["triangulation"]["geometry"]["simplices"][0]["vertex_indices"][0] = json!(u64::MAX);

        let Err(error) = CdtMcmcCheckpoint::from_json(&payload.to_string()) else {
            panic!("invalid relation should fail checked geometry hydration");
        };
        assert_matches!(
            error,
            CdtError::CheckpointSerializationFailed {
                operation: CheckpointOperation::Deserialize,
                ref detail,
                ..
            } if detail.contains("references vertex index")
        );
    }

    #[test]
    fn committed_v1_fixture_loads_and_resumes() {
        let checkpoint = CdtMcmcCheckpoint::from_json(include_str!(
            "../../../tests/fixtures/checkpoint_v1.json"
        ))
        .expect("committed v1 checkpoint fixture should remain readable");

        assert_eq!(checkpoint.current_step().get(), 1);
        assert_eq!(checkpoint.chain().accepted(), 0);
        assert_eq!(checkpoint.chain().rejected(), 1);
        assert_eq!(checkpoint.triangulation().slice_sizes(), &[4, 4, 4]);

        let resumed = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 1, 0, 1)
                .expect("resume configuration should validate")
                .with_seed(999),
            ActionConfig::default(),
        )
        .resume_from_checkpoint(checkpoint)
        .expect("committed v1 fixture should resume");
        assert_eq!(resumed.steps().len(), 2);
    }

    #[test]
    fn checkpoint_v1_round_trips_exact_toroidal_realization() {
        let checkpoint = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 1, 0, 1)
                .expect("test configuration should validate")
                .with_seed(23),
            ActionConfig::default(),
        )
        .run_to_checkpoint(
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("toroidal CDT should build"),
        )
        .expect("toroidal run should checkpoint");
        let original_json = checkpoint.to_json().expect("checkpoint should serialize");
        let original: Value =
            serde_json::from_str(&original_json).expect("checkpoint JSON should parse");
        assert!(
            original["triangulation"]["geometry"]["simplices"]
                .as_array()
                .expect("simplices should be an array")
                .iter()
                .any(|simplex| simplex["periodic_vertex_offsets"].is_array()),
            "toroidal checkpoint should preserve periodic lift offsets"
        );

        let restored = CdtMcmcCheckpoint::from_json(&original_json)
            .expect("toroidal checkpoint should restore");
        restored
            .triangulation()
            .validate_topology()
            .expect("restored topology should validate");
        restored
            .triangulation()
            .validate_foliation()
            .expect("restored foliation should validate");
        restored
            .triangulation()
            .validate_causality()
            .expect("restored causality should validate");
        let restored: Value = serde_json::from_str(
            &restored
                .to_json()
                .expect("restored checkpoint should serialize"),
        )
        .expect("restored checkpoint JSON should parse");
        assert_eq!(original["triangulation"], restored["triangulation"]);
    }
}
