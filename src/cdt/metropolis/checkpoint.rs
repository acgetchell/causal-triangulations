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
use crate::cdt::triangulation::CdtTriangulation2D;
use crate::errors::{
    CdtError, CdtResult, CheckpointMoveCounter, CheckpointResumeFailure, ProposalTelemetryCounter,
};
use markov_chain_monte_carlo::ChainCheckpoint;
use rand::rngs::Xoshiro256PlusPlus;
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};
use std::num::NonZeroU32;
use std::time::Duration;

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
/// The embedded [`ChainCheckpoint`] stores the current triangulation and
/// accepted/rejected chain counters using the shared MCMC crate's
/// checkpoint type. CDT adds the domain-specific runtime state needed for
/// scientific continuation: action/config metadata, accumulated telemetry,
/// both RNG streams, and the ergodic move system.
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
/// The Serde representation is version-bound and includes internal state from
/// this crate, `delaunay`, and `markov-chain-monte-carlo`. Serialized checkpoint
/// files are supported only when read by the same build that wrote them or by a
/// release that explicitly documents checkpoint compatibility. In-memory
/// continuation and same-build serialization round trips remain supported.
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
#[derive(Clone, Serialize)]
pub struct CdtMcmcCheckpoint {
    pub(crate) chain: ChainCheckpoint<CdtTriangulation2D>,
    pub(crate) config: MetropolisConfig,
    pub(crate) action_config: ActionConfig,
    pub(crate) current_step: NonZeroU32,
    current_action: CheckpointAction,
    pub(crate) move_stats: MoveStatistics,
    #[serde(default)]
    pub(crate) proposal_stats: ProposalStatistics,
    pub(crate) steps: Vec<MonteCarloStep>,
    pub(crate) measurements: Vec<Measurement>,
    pub(crate) scalar_trace_rows: Vec<CdtScalarTraceRow>,
    pub(crate) elapsed_time: Duration,
    pub(crate) acceptance_rng: Xoshiro256PlusPlus,
    pub(crate) ergodics: ErgodicsSystem,
}

#[derive(Deserialize)]
struct CdtMcmcCheckpointWire {
    chain: ChainCheckpoint<CdtTriangulation2D>,
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
    elapsed_time: Duration,
    acceptance_rng: Xoshiro256PlusPlus,
    ergodics: ErgodicsSystem,
}

impl<'de> Deserialize<'de> for CdtMcmcCheckpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CdtMcmcCheckpointWire::deserialize(deserializer)?;
        let current_step = checkpoint_current_step(wire.current_step).map_err(DeError::custom)?;
        let current_action = CheckpointAction::new(wire.current_action).map_err(DeError::custom)?;
        let checkpoint = Self {
            chain: wire.chain,
            config: wire.config,
            action_config: wire.action_config,
            current_step,
            current_action,
            move_stats: wire.move_stats,
            proposal_stats: wire.proposal_stats,
            steps: wire.steps,
            measurements: wire.measurements,
            scalar_trace_rows: wire.scalar_trace_rows,
            elapsed_time: wire.elapsed_time,
            acceptance_rng: wire.acceptance_rng,
            ergodics: wire.ergodics,
        };
        validate_checkpoint_counters(&checkpoint).map_err(DeError::custom)?;
        Ok(checkpoint)
    }
}

impl CdtMcmcCheckpoint {
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
    use std::assert_matches;

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
}
