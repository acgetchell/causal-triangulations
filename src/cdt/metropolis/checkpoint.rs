#![forbid(unsafe_code)]

//! Checkpoint and resume validation for CDT Metropolis sampling.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveStatistics, MoveType};
use crate::cdt::results::{Measurement, SimulationResultsBackend, SimulationResultsParts};
use crate::errors::{CdtError, CdtResult, CheckpointMoveCounter, CheckpointResumeFailure};
use crate::geometry::CdtTriangulation2D;
use markov_chain_monte_carlo::ChainCheckpoint;
use rand::rngs::Xoshiro256PlusPlus;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;

use super::helpers::actions_match;
use super::runner::{MetropolisAlgorithm, MetropolisConfig};
use super::telemetry::{MonteCarloStep, ProposalStatistics};

pub(crate) struct CdtMcmcCheckpointParts {
    pub(crate) triangulation: CdtTriangulation2D,
    pub(crate) accepted: usize,
    pub(crate) rejected: usize,
    pub(crate) config: MetropolisConfig,
    pub(crate) action_config: ActionConfig,
    pub(crate) current_step: u32,
    pub(crate) current_action: f64,
    pub(crate) move_stats: MoveStatistics,
    pub(crate) proposal_stats: ProposalStatistics,
    pub(crate) steps: Vec<MonteCarloStep>,
    pub(crate) measurements: Vec<Measurement>,
    pub(crate) elapsed_time: Duration,
    pub(crate) acceptance_rng: Xoshiro256PlusPlus,
    pub(crate) ergodics: ErgodicsSystem,
}

/// Resumable checkpoint for a CDT Metropolis-Hastings run.
///
/// The embedded [`ChainCheckpoint`] stores the current triangulation and
/// accepted/rejected chain counters using the shared MCMC crate's portable
/// checkpoint type. CDT adds the domain-specific runtime state needed for
/// scientific continuation: action/config metadata, accumulated telemetry,
/// both RNG streams, and the ergodic move system.
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
///     assert_eq!(checkpoint.current_step(), 1);
///     assert_eq!(checkpoint.measurements().len(), 2);
///     Ok(())
/// }
/// ```
#[derive(Clone, Serialize)]
pub struct CdtMcmcCheckpoint {
    pub(crate) chain: ChainCheckpoint<CdtTriangulation2D>,
    pub(crate) config: MetropolisConfig,
    pub(crate) action_config: ActionConfig,
    pub(crate) current_step: u32,
    pub(crate) current_action: f64,
    pub(crate) move_stats: MoveStatistics,
    #[serde(default)]
    pub(crate) proposal_stats: ProposalStatistics,
    pub(crate) steps: Vec<MonteCarloStep>,
    pub(crate) measurements: Vec<Measurement>,
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
        let checkpoint = Self {
            chain: wire.chain,
            config: wire.config,
            action_config: wire.action_config,
            current_step: wire.current_step,
            current_action: wire.current_action,
            move_stats: wire.move_stats,
            proposal_stats: wire.proposal_stats,
            steps: wire.steps,
            measurements: wire.measurements,
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
        let checkpoint = Self {
            chain: ChainCheckpoint::new(parts.triangulation, parts.accepted, parts.rejected),
            config: parts.config,
            action_config: parts.action_config,
            current_step: parts.current_step,
            current_action: parts.current_action,
            move_stats: parts.move_stats,
            proposal_stats: parts.proposal_stats,
            steps: parts.steps,
            measurements: parts.measurements,
            elapsed_time: parts.elapsed_time,
            acceptance_rng: parts.acceptance_rng,
            ergodics: parts.ergodics,
        };
        validate_checkpoint_counters(&checkpoint)?;
        Ok(checkpoint)
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
    /// assert_eq!(checkpoint.triangulation().time_slices(), 3);
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
    /// assert_eq!(checkpoint.config().steps(), 1);
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

    /// Returns the last completed Monte Carlo step.
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
    /// assert_eq!(checkpoint.current_step(), 1);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn current_step(&self) -> u32 {
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
        self.current_action
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
    /// assert_eq!(checkpoint.steps().len(), checkpoint.current_step() as usize);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub fn steps(&self) -> &[MonteCarloStep] {
        &self.steps
    }

    /// Returns accumulated measurements through the checkpoint step.
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
    /// assert_eq!(checkpoint.measurements().first().map(|m| m.step), Some(0));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Converts the checkpoint into a complete simulation result snapshot.
    ///
    /// This consumes the checkpoint and keeps all accumulated steps,
    /// measurements, move statistics, proposal statistics, elapsed time, and the
    /// checkpointed triangulation in the returned [`SimulationResultsBackend`].
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
    ///     let results = checkpoint.into_results();
    ///     assert_eq!(results.steps().len(), 1);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn into_results(self) -> SimulationResultsBackend {
        let (triangulation, _, _) = self.chain.into_parts();
        SimulationResultsBackend::from_parts(SimulationResultsParts {
            config: self.config,
            action_config: self.action_config,
            move_stats: self.move_stats,
            proposal_stats: self.proposal_stats,
            steps: self.steps,
            measurements: self.measurements,
            elapsed_time: self.elapsed_time,
            triangulation,
        })
    }
}

/// Builds the public checkpoint-resume error wrapper for CDT-owned resume invariants.
pub(crate) const fn checkpoint_resume_failed(failure: CheckpointResumeFailure) -> CdtError {
    CdtError::CheckpointResumeFailed { failure }
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
pub(crate) fn validate_resume_compatible(
    algorithm: &MetropolisAlgorithm,
    checkpoint: &CdtMcmcCheckpoint,
) -> CdtResult<()> {
    if !action_configs_match(algorithm.action_config(), &checkpoint.action_config) {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::IncompatibleActionConfiguration,
        ));
    }
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

/// Compares action couplings with the same tolerance used for persisted action values.
fn action_configs_match(left: &ActionConfig, right: &ActionConfig) -> bool {
    actions_match(left.coupling_0(), right.coupling_0())
        && actions_match(left.coupling_2(), right.coupling_2())
        && actions_match(left.cosmological_constant(), right.cosmological_constant())
}

/// Checks that serialized chain counters and CDT telemetry agree.
///
/// This protects the public resume contract by rejecting checkpoints whose
/// generic MCMC counters, CDT move counters, step telemetry, or measurement
/// schedule cannot all describe the same chain prefix.
///
/// # Errors
///
/// Returns [`CdtError::InvalidSimulationConfiguration`] for invalid
/// Metropolis settings, [`CdtError::InvalidConfiguration`] for invalid action
/// couplings, or [`CdtError::CheckpointResumeFailed`] when serialized chain
/// counters, step telemetry, or measurements do not match the configured
/// sampling schedule.
pub(crate) fn validate_checkpoint_counters(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    checkpoint.config.validate();
    checkpoint.action_config.validate();

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
    let checkpoint_step = usize::try_from(checkpoint.current_step).unwrap_or(usize::MAX);
    if checkpoint.chain.total_steps() != checkpoint_step {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::ChainStepMismatch {
                chain_steps: checkpoint.chain.total_steps(),
                checkpoint_step: checkpoint.current_step,
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
    validate_checkpoint_steps(checkpoint)?;
    validate_checkpoint_measurements(checkpoint)?;
    Ok(())
}

/// Checks that serialized per-step telemetry forms the exact prefix being resumed.
fn validate_checkpoint_steps(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    let accepted_steps = checkpoint.steps.iter().filter(|step| step.accepted).count();
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
        if step.step != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::StepTelemetrySequenceMismatch {
                    actual: step.step,
                    expected: expected_step,
                },
            ));
        }
        if !step.action_before.is_finite() {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::NonFiniteStepActionBefore { step: step.step },
            ));
        }
        if let Some(delta_action) = step.delta_action
            && !delta_action.is_finite()
        {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::NonFiniteStepDeltaAction { step: step.step },
            ));
        }
        if step.accepted && step.delta_action.is_none() {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::AcceptedStepMissingDeltaAction { step: step.step },
            ));
        }
        match (step.accepted, step.action_after) {
            (true, Some(action_after)) if action_after.is_finite() => {
                if let Some(delta_action) = step.delta_action
                    && !actions_match(action_after, step.action_before + delta_action)
                {
                    return Err(checkpoint_resume_failed(
                        CheckpointResumeFailure::StepActionAfterDeltaMismatch { step: step.step },
                    ));
                }
            }
            (true, Some(_)) => {
                return Err(checkpoint_resume_failed(
                    CheckpointResumeFailure::NonFiniteStepActionAfter { step: step.step },
                ));
            }
            (true, None) => {
                return Err(checkpoint_resume_failed(
                    CheckpointResumeFailure::AcceptedStepMissingActionAfter { step: step.step },
                ));
            }
            (false, Some(_)) => {
                return Err(checkpoint_resume_failed(
                    CheckpointResumeFailure::RejectedStepHasActionAfter { step: step.step },
                ));
            }
            (false, None) => {}
        }
    }
    Ok(())
}

/// Checks that serialized measurements match the configured sampling schedule.
fn validate_checkpoint_measurements(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    let expected_measurements = usize::try_from(
        u64::from(checkpoint.current_step) / u64::from(checkpoint.config.measurement_frequency())
            + 1,
    )
    .map_err(|_| checkpoint_resume_failed(CheckpointResumeFailure::MeasurementCountOverflow))?;
    if checkpoint.measurements.len() != expected_measurements {
        return Err(checkpoint_resume_failed(
            CheckpointResumeFailure::MeasurementCountMismatch {
                actual: checkpoint.measurements.len(),
                expected: expected_measurements,
            },
        ));
    }

    for (index, measurement) in checkpoint.measurements.iter().enumerate() {
        let expected_step = u64::try_from(index)
            .ok()
            .and_then(|index| {
                index.checked_mul(u64::from(checkpoint.config.measurement_frequency()))
            })
            .and_then(|step| u32::try_from(step).ok())
            .ok_or_else(|| {
                checkpoint_resume_failed(CheckpointResumeFailure::MeasurementStepOverflow)
            })?;
        if measurement.step != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::MeasurementStepMismatch {
                    actual: measurement.step,
                    expected: expected_step,
                },
            ));
        }
        if !measurement.action.is_finite() {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::NonFiniteMeasurementAction {
                    step: measurement.step,
                },
            ));
        }
    }
    Ok(())
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
