#![forbid(unsafe_code)]

//! Metropolis-Hastings algorithm for Causal Dynamical Triangulations.
//!
//! This module implements the Monte Carlo sampling algorithm used to sample
//! triangulation configurations according to the CDT path integral measure.
//!
//! The simulation selects an explicit local proposal site, applies it once on a
//! cloned proposed state, and only swaps that state into the live chain after
//! Metropolis-Hastings acceptance. Ordinary site failures are self-loop
//! proposal outcomes tracked in [`crate::cdt::metropolis::ProposalStatistics`].

use super::adapter::{
    CdtProposal, CdtProposalError, CdtProposalInfo, CdtTarget, restore_checkpoint_state,
};
use super::checkpoint::{
    CdtMcmcCheckpoint, CdtMcmcCheckpointParts, chain_counters, checkpoint_resume_failed,
    validate_checkpoint_counters, validate_resume_compatible,
};
use super::helpers::{
    action_for, actions_match, measurement_for, measurement_is_due, validate_metropolis_schedule,
};
use super::telemetry::{MonteCarloStep, MonteCarloStepOutcome, ProposalStatistics};
use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveStatistics, MoveType};
use crate::cdt::results::{
    CdtScalarTraceOutcome, CdtScalarTraceRow, Measurement, SimulationResultsBackend,
};
use crate::cdt::triangulation::SimulationEvent;
use crate::errors::{
    CdtError, CdtResult, CheckpointResumeFailure, ConfigurationSetting,
    MetropolisMoveApplicationFailure,
};
use crate::geometry::CdtTriangulation2D;
use markov_chain_monte_carlo::{
    Chain, ChainCheckpoint, DelayedStep, DelayedStepError, Sampler, StepOutcome,
};
use rand::{SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

/// Validated configuration for the Metropolis-Hastings algorithm.
///
/// Temperature and schedule invariants are checked before storage. The sampler
/// can therefore compute beta values, measurement cadence, and continuation step
/// counts without revalidating raw fields at every use.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetropolisConfig {
    /// Temperature parameter (1/β)
    temperature: f64,
    /// Nonzero number of Monte Carlo steps to perform
    steps: NonZeroU32,
    /// Number of thermalization steps before measurements
    thermalization_steps: u32,
    /// Nonzero frequency of measurements (take measurement every N steps)
    measurement_frequency: NonZeroU32,
    /// Optional RNG seed for reproducible simulations (default: None = random)
    seed: Option<u64>,
}

#[derive(Deserialize)]
struct MetropolisConfigWire {
    temperature: f64,
    steps: u32,
    thermalization_steps: u32,
    measurement_frequency: u32,
    seed: Option<u64>,
}

impl<'de> Deserialize<'de> for MetropolisConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MetropolisConfigWire::deserialize(deserializer)?;
        Self::new_with_seed(
            wire.temperature,
            wire.steps,
            wire.thermalization_steps,
            wire.measurement_frequency,
            wire.seed,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl Default for MetropolisConfig {
    /// Default Metropolis configuration for 2D CDT.
    fn default() -> Self {
        Self::from_validated_parts(
            1.0,
            default_step_count(),
            100,
            default_measurement_frequency(),
            None,
        )
    }
}

impl MetropolisConfig {
    /// Creates a new validated Metropolis configuration.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if `temperature` is
    /// not finite and positive, if `steps` or `measurement_frequency` is zero, if
    /// thermalization exceeds the step count, or if the schedule cannot produce a
    /// post-thermalization measurement.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 500, 50, 5)?;
    /// assert_eq!(config.steps().get(), 500);
    /// assert!(config.seed().is_none());
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn new(
        temperature: f64,
        steps: u32,
        thermalization_steps: u32,
        measurement_frequency: u32,
    ) -> CdtResult<Self> {
        Self::new_with_seed(
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
            None,
        )
    }

    /// Creates a new validated Metropolis configuration with an explicit RNG seed.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] for the same invalid
    /// temperature or schedule values as [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new_with_seed(1.0, 100, 10, 5, Some(42))?;
    /// assert_eq!(config.seed(), Some(42));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn new_with_seed(
        temperature: f64,
        steps: u32,
        thermalization_steps: u32,
        measurement_frequency: u32,
        seed: Option<u64>,
    ) -> CdtResult<Self> {
        validate_metropolis_schedule(
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
        )?;
        let Some(steps) = NonZeroU32::new(steps) else {
            return Err(CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::Steps,
                provided_value: steps.to_string(),
                expected: "≥ 1".to_string(),
            });
        };
        let Some(measurement_frequency) = NonZeroU32::new(measurement_frequency) else {
            return Err(CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::MeasurementFrequency,
                provided_value: measurement_frequency.to_string(),
                expected: "≥ 1".to_string(),
            });
        };
        Ok(Self::from_validated_parts(
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
            seed,
        ))
    }

    /// Builds a Metropolis configuration after raw schedule validation.
    ///
    /// This helper is used when a higher-level validated config already proved
    /// the temperature and schedule invariants, allowing conversion without a
    /// second fallible branch.
    pub(crate) const fn from_validated_parts(
        temperature: f64,
        steps: NonZeroU32,
        thermalization_steps: u32,
        measurement_frequency: NonZeroU32,
        seed: Option<u64>,
    ) -> Self {
        Self {
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
            seed,
        }
    }

    /// Sets the RNG seed for reproducible simulations.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(1.0, 100, 10, 5)?.with_seed(42);
    /// assert_eq!(config.seed(), Some(42));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Returns the temperature parameter.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5)?;
    /// assert_relative_eq!(config.temperature(), 2.0);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn temperature(&self) -> f64 {
        self.temperature
    }

    /// Returns the nonzero configured number of Monte Carlo steps.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5)?;
    /// assert_eq!(config.steps().get(), 100);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn steps(&self) -> NonZeroU32 {
        self.steps
    }

    /// Returns the number of thermalization steps.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5)?;
    /// assert_eq!(config.thermalization_steps(), 10);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn thermalization_steps(&self) -> u32 {
        self.thermalization_steps
    }

    /// Returns the nonzero measurement cadence.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5)?;
    /// assert_eq!(config.measurement_frequency().get(), 5);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn measurement_frequency(&self) -> NonZeroU32 {
        self.measurement_frequency
    }

    /// Returns the optional reproducibility seed.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5)?.with_seed(13);
    /// assert_eq!(config.seed(), Some(13));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn seed(&self) -> Option<u64> {
        self.seed
    }

    /// Returns the inverse temperature (β = 1/T).
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5)?;
    /// assert_relative_eq!(config.beta(), 0.5);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub fn beta(&self) -> f64 {
        1.0 / self.temperature
    }

    /// Confirms that stored simulation-specific configuration values are valid.
    ///
    /// This method is kept for code that wants a common validation hook across
    /// configuration-like types. Because [`MetropolisConfig`] validates before
    /// storage, it is an infallible debug assertion of the stored invariant.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(1.0, 100, 10, 5)?;
    /// config.validate();
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn validate(&self) {
        debug_assert!(self.temperature.is_finite() && self.temperature > 0.0);
        debug_assert!(self.thermalization_steps <= self.steps.get());
    }
}

/// Returns the built-in nonzero Metropolis step count.
const fn default_step_count() -> NonZeroU32 {
    match NonZeroU32::new(1000) {
        Some(steps) => steps,
        None => NonZeroU32::MIN,
    }
}

/// Returns the built-in nonzero measurement cadence.
const fn default_measurement_frequency() -> NonZeroU32 {
    match NonZeroU32::new(10) {
        Some(measurement_frequency) => measurement_frequency,
        None => NonZeroU32::MIN,
    }
}

struct MetropolisRunState {
    triangulation: CdtTriangulation2D,
    current_step: u32,
    current_action: f64,
    trace_seed: Option<u64>,
    acceptance_rng: Xoshiro256PlusPlus,
    ergodics: ErgodicsSystem,
    move_stats: MoveStatistics,
    proposal_stats: ProposalStatistics,
    steps: Vec<MonteCarloStep>,
    measurements: Vec<Measurement>,
    scalar_trace_rows: Vec<CdtScalarTraceRow>,
    elapsed_time: Duration,
}

/// Metropolis-Hastings algorithm implementation for CDT.
///
/// Accepts or rejects proposed CDT move types before applying them.
pub struct MetropolisAlgorithm {
    /// Algorithm configuration
    config: MetropolisConfig,
    /// Action calculation configuration
    action_config: ActionConfig,
}

impl MetropolisAlgorithm {
    /// Creates a new Metropolis algorithm instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// let config = MetropolisConfig::new(1.0, 10, 2, 1)?;
    /// let _algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn new(config: MetropolisConfig, action_config: ActionConfig) -> Self {
        Self {
            config,
            action_config,
        }
    }

    pub(crate) const fn config(&self) -> &MetropolisConfig {
        &self.config
    }

    pub(crate) const fn action_config(&self) -> &ActionConfig {
        &self.action_config
    }

    /// Run the Monte Carlo simulation.
    ///
    /// Each step runs through the upstream planned-proposal sampler. CDT plans
    /// a concrete local move on a cloned state, the sampler applies the
    /// Metropolis-Hastings proposal-ratio correction and accept/reject draw, and
    /// CDT records domain telemetry after the sampler reports the outcome.
    /// Ordinary no-site and recoverable local-site rejections are recorded as
    /// self-loop proposals.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
    /// configuration is invalid, [`CdtError::InvalidConfiguration`] if the
    /// action configuration is invalid,
    /// [`CdtError::MetropolisMoveApplicationFailed`] if an accepted move causes
    /// a hard backend mutation failure,
    /// [`CdtError::PlannedProposalTelemetryMissing`] if the upstream sampler
    /// omits CDT step metadata or accepted-step action evidence,
    /// [`CdtError::InvalidSimplexCount`] if a live triangulation has zero
    /// vertices, edges, or triangles,
    /// [`CdtError::MeasurementCountOverflow`] if a measurement count cannot fit
    /// compact telemetry storage, [`CdtError::InvalidMeasurementAction`] if a
    /// measurement action is non-finite, or a validation error for unrecoverable
    /// triangulation failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53)?;
    ///     let config = MetropolisConfig::new(1.0, 2, 1, 1)?.with_seed(7);
    ///     let results = MetropolisAlgorithm::new(config, ActionConfig::default()).run(tri)?;
    ///     assert_eq!(results.steps().len(), 2);
    ///     Ok(())
    /// }
    /// ```
    pub fn run(&self, triangulation: CdtTriangulation2D) -> CdtResult<SimulationResultsBackend> {
        Ok(self.run_to_checkpoint(triangulation)?.into_results())
    }

    /// Run the simulation and return both the final results and checkpoint.
    ///
    /// The returned checkpoint can be resumed in memory with
    /// [`Self::resume_from_checkpoint`] when callers want cumulative results,
    /// or [`Self::resume_to_checkpoint`] when chunked drivers need another
    /// resumable checkpoint. If callers serialize it, successful deserialization
    /// also depends on the embedded triangulation passing the backend's checked
    /// reconstruction.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
    /// configuration is invalid, [`CdtError::InvalidConfiguration`] if the
    /// action configuration is invalid,
    /// [`CdtError::MetropolisMoveApplicationFailed`] if an accepted move causes
    /// a hard backend mutation failure,
    /// [`CdtError::PlannedProposalTelemetryMissing`] if the upstream sampler
    /// omits CDT step metadata or accepted-step action evidence,
    /// [`CdtError::InvalidSimplexCount`] if a live triangulation has zero
    /// vertices, edges, or triangles,
    /// [`CdtError::MeasurementCountOverflow`] if a measurement count cannot fit
    /// compact telemetry storage, [`CdtError::InvalidMeasurementAction`] if a
    /// measurement action is non-finite, or a validation error for unrecoverable
    /// triangulation failures.
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
    ///     let algorithm = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(13),
    ///         ActionConfig::default(),
    ///     );
    ///     let (results, checkpoint) = algorithm.run_with_checkpoint(tri)?;
    ///
    ///     assert_eq!(results.steps().len(), checkpoint.steps().len());
    ///     assert_eq!(checkpoint.current_step().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    pub fn run_with_checkpoint(
        &self,
        triangulation: CdtTriangulation2D,
    ) -> CdtResult<(SimulationResultsBackend, CdtMcmcCheckpoint)> {
        let checkpoint = self.run_to_checkpoint(triangulation)?;
        let results = checkpoint.clone().into_results();
        Ok((results, checkpoint))
    }

    /// Run the simulation and return a resumable checkpoint.
    ///
    /// The checkpoint embeds the current triangulation in the MCMC crate's
    /// [`ChainCheckpoint`] and stores CDT-specific
    /// proposal state, telemetry, and RNG streams beside it.
    ///
    /// Direct in-memory resume through [`Self::resume_from_checkpoint`] or
    /// [`Self::resume_to_checkpoint`] does not reserialize the triangulation.
    /// Serialized restore uses checked backend reconstruction, so snapshots
    /// whose evolved geometry is no longer Delaunay-valid may fail to
    /// deserialize even though the in-memory checkpoint can still be resumed.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
    /// configuration is invalid, [`CdtError::InvalidConfiguration`] if the
    /// action configuration is invalid,
    /// [`CdtError::MetropolisMoveApplicationFailed`] if an accepted move causes
    /// a hard backend mutation failure,
    /// [`CdtError::PlannedProposalTelemetryMissing`] if the upstream sampler
    /// omits CDT step metadata or accepted-step action evidence,
    /// [`CdtError::InvalidSimplexCount`] if a live triangulation has zero
    /// vertices, edges, or triangles,
    /// [`CdtError::MeasurementCountOverflow`] if a measurement count cannot fit
    /// compact telemetry storage, [`CdtError::InvalidMeasurementAction`] if a
    /// measurement action is non-finite, or a validation error for unrecoverable
    /// triangulation failures.
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
    ///         MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(13),
    ///         ActionConfig::default(),
    ///     )
    ///     .run_to_checkpoint(tri)?;
    ///
    ///     assert_eq!(checkpoint.current_step().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    pub fn run_to_checkpoint(
        &self,
        triangulation: CdtTriangulation2D,
    ) -> CdtResult<CdtMcmcCheckpoint> {
        self.config.validate();
        self.action_config.validate();

        let mut state = self.initial_state(triangulation)?;
        self.run_steps(&mut state, self.config.steps)?;
        state.into_checkpoint(self.config.clone(), self.action_config.clone())
    }

    /// Continue a checkpoint for this algorithm's configured step count.
    ///
    /// `self.config.steps` is interpreted as an additional number of steps. The
    /// returned [`SimulationResultsBackend`] is cumulative: it includes the
    /// checkpointed prefix and the resumed suffix.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the resumed
    /// Metropolis configuration is invalid, [`CdtError::InvalidConfiguration`]
    /// if the action configuration is invalid, or
    /// [`CdtError::CheckpointResumeFailed`] if the checkpoint is incompatible
    /// with this algorithm or internally inconsistent. Returns
    /// [`CdtError::MetropolisMoveApplicationFailed`],
    /// [`CdtError::PlannedProposalTelemetryMissing`] if resumed sampling omits
    /// CDT step metadata or accepted-step action evidence,
    /// [`CdtError::InvalidSimplexCount`] if a resumed live triangulation has zero
    /// vertices, edges, or triangles,
    /// [`CdtError::MeasurementCountOverflow`] if a resumed measurement count
    /// cannot fit compact telemetry storage,
    /// [`CdtError::InvalidMeasurementAction`] if a resumed measurement action is
    /// non-finite, or validation errors for failures during resumed sampling.
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
    ///     let action = ActionConfig::default();
    ///     let prefix = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(13),
    ///         action.clone(),
    ///     );
    ///     let checkpoint = prefix.run_to_checkpoint(tri)?;
    ///
    ///     let resumed = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(999),
    ///         action,
    ///     )
    ///     .resume_from_checkpoint(checkpoint)?;
    ///
    ///     assert_eq!(resumed.steps().len(), 4);
    ///     assert_eq!(resumed.config().steps().get(), 4);
    ///     Ok(())
    /// }
    /// ```
    pub fn resume_from_checkpoint(
        &self,
        checkpoint: CdtMcmcCheckpoint,
    ) -> CdtResult<SimulationResultsBackend> {
        self.resume_to_checkpoint(checkpoint)
            .map(CdtMcmcCheckpoint::into_results)
    }

    /// Continue a checkpoint for this algorithm's configured step count.
    ///
    /// `self.config.steps` is interpreted as an additional number of steps, and
    /// the returned checkpoint remains resumable. This supports chunked drivers
    /// such as sweep-based debug runs that size each chunk from the current
    /// triangulation volume while preserving RNG streams, counters,
    /// measurements, and checkpoint-compatible continuation state.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the resumed
    /// Metropolis configuration is invalid, [`CdtError::InvalidConfiguration`]
    /// if the action configuration is invalid, or
    /// [`CdtError::CheckpointResumeFailed`] if the checkpoint is incompatible
    /// with this algorithm or internally inconsistent. Returns
    /// [`CdtError::MetropolisMoveApplicationFailed`],
    /// [`CdtError::PlannedProposalTelemetryMissing`] if resumed sampling omits
    /// CDT step metadata or accepted-step action evidence,
    /// [`CdtError::InvalidSimplexCount`] if a resumed live triangulation has zero
    /// vertices, edges, or triangles,
    /// [`CdtError::MeasurementCountOverflow`] if a resumed measurement count
    /// cannot fit compact telemetry storage,
    /// [`CdtError::InvalidMeasurementAction`] if a resumed measurement action is
    /// non-finite, or validation errors for failures during resumed sampling.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let action = ActionConfig::default();
    ///     let checkpoint = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(13),
    ///         action.clone(),
    ///     )
    ///     .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    ///
    ///     let checkpoint = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 3, 0, 1)?.with_seed(999),
    ///         action,
    ///     )
    ///     .resume_to_checkpoint(checkpoint)?;
    ///
    ///     assert_eq!(checkpoint.current_step().get(), 5);
    ///     assert_eq!(checkpoint.config().steps().get(), 5);
    ///     Ok(())
    /// }
    /// ```
    pub fn resume_to_checkpoint(
        &self,
        checkpoint: CdtMcmcCheckpoint,
    ) -> CdtResult<CdtMcmcCheckpoint> {
        self.config.validate();
        self.action_config.validate();
        validate_resume_compatible(self, &checkpoint)?;

        let mut result_config = checkpoint.config.clone();
        let steps = checkpoint
            .current_step
            .get()
            .checked_add(self.config.steps.get())
            .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::StepCountOverflow))?;
        result_config.steps = NonZeroU32::new(steps)
            .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::StepCountOverflow))?;

        let mut state = MetropolisRunState::from_checkpoint(checkpoint)?;
        self.run_steps(&mut state, self.config.steps)?;
        state.into_checkpoint(result_config, self.action_config.clone())
    }

    fn initial_state(
        &self,
        mut triangulation: CdtTriangulation2D,
    ) -> CdtResult<MetropolisRunState> {
        let current_action = action_for(&self.action_config, &triangulation);
        let mut measurements = Vec::new();
        if measurement_is_due(
            0,
            self.config.thermalization_steps(),
            self.config.measurement_frequency(),
        ) {
            measurements.push(measurement_for(0, current_action, &triangulation)?);
            triangulation.record_event(SimulationEvent::MeasurementTaken {
                step: 0,
                action: current_action,
            });
        }

        Ok(MetropolisRunState {
            triangulation,
            current_step: 0,
            current_action,
            trace_seed: self.config.seed,
            acceptance_rng: simulation_rng(self.config.seed),
            ergodics: self.config.seed.map_or_else(ErgodicsSystem::new, |seed| {
                ErgodicsSystem::with_seed(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
            }),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: Vec::new(),
            measurements,
            scalar_trace_rows: Vec::new(),
            elapsed_time: Duration::ZERO,
        })
    }

    /// Advances mutable run state through the planned-proposal sampler.
    ///
    /// This is the only step loop used by fresh runs and checkpoint
    /// continuation. It rebuilds the generic chain view from CDT counters,
    /// persists the proposal RNG stream after each chunk, and keeps CDT-owned
    /// telemetry synchronized with upstream planned-proposal outcomes.
    fn run_steps(
        &self,
        state: &mut MetropolisRunState,
        additional_steps: NonZeroU32,
    ) -> CdtResult<()> {
        let start = Instant::now();
        let target = CdtTarget::new(self.action_config.clone(), self.config.temperature())?;
        let (accepted, rejected) = chain_counters(&state.move_stats)?;
        let checkpoint = ChainCheckpoint::new(state.triangulation.clone(), accepted, rejected);
        let chain = Chain::from_checkpoint(checkpoint, &target)?;
        let mut proposal =
            CdtProposal::from_ergodics(self.action_config.clone(), state.ergodics.clone());
        let mut acceptance_rng = state.acceptance_rng.clone();

        {
            let mut sampler = Sampler::new(chain, &target, &mut proposal, &mut acceptance_rng)?;

            for _ in 0..additional_steps.get() {
                let step = state
                    .current_step
                    .checked_add(1)
                    .and_then(NonZeroU32::new)
                    .ok_or_else(|| {
                        checkpoint_resume_failed(CheckpointResumeFailure::StepCountOverflow)
                    })?;
                let planned_step = match sampler.step_delayed() {
                    Ok(planned_step) => planned_step,
                    Err(err) => {
                        let error = planned_step_error(step.get(), err);
                        state.triangulation = sampler.chain_ref().state().clone();
                        drop(sampler);
                        state.acceptance_rng = acceptance_rng;
                        state.ergodics = proposal.into_ergodics();
                        state.elapsed_time += start.elapsed();
                        return Err(error);
                    }
                };
                debug_assert_eq!(
                    sampler.proposal_ref().last_step_info(),
                    planned_step.info,
                    "CDT proposal telemetry cache should mirror the upstream planned-step info"
                );
                record_planned_step(
                    self,
                    state,
                    step,
                    &planned_step,
                    planned_step
                        .info
                        .ok_or_else(|| missing_planned_step_info(step.get()))?,
                    sampler.proposal_ref().last_proposal_stats(),
                    sampler.chain_ref().state(),
                )?;
                // The upstream chain owns the geometry used by later proposals,
                // while CDT owns simulation metadata/history. Keep them in sync
                // after annotating the CDT state so final handoff and future
                // accepted proposals cannot discard recorded events.
                sampler.replace_state(state.triangulation.clone())?;
                state.current_step = step.get();
            }
        }

        state.acceptance_rng = acceptance_rng;
        state.ergodics = proposal.into_ergodics();
        state.elapsed_time += start.elapsed();
        Ok(())
    }
}

impl MetropolisRunState {
    /// Restores the mutable simulation state from a validated checkpoint.
    ///
    /// The generic MCMC checkpoint rechecks target compatibility, then CDT
    /// recomputes the action so serialized telemetry cannot silently diverge
    /// from the invariant-checked triangulation payload.
    fn from_checkpoint(checkpoint: CdtMcmcCheckpoint) -> CdtResult<Self> {
        validate_checkpoint_counters(&checkpoint)?;
        let stored_action = checkpoint.current_action();
        let target = CdtTarget::new(
            checkpoint.action_config.clone(),
            checkpoint.config.temperature(),
        )?;
        let triangulation = restore_checkpoint_state(checkpoint.chain, &target)?;
        triangulation.validate_evolved_cdt()?;
        let actual_action = action_for(&checkpoint.action_config, &triangulation);
        if !actions_match(actual_action, stored_action) {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::ActionMismatch {
                    stored: stored_action,
                    recomputed: actual_action,
                },
            ));
        }

        Ok(Self {
            triangulation,
            current_step: checkpoint.current_step.get(),
            current_action: stored_action,
            trace_seed: checkpoint.config.seed(),
            acceptance_rng: checkpoint.acceptance_rng,
            ergodics: checkpoint.ergodics,
            move_stats: checkpoint.move_stats,
            proposal_stats: checkpoint.proposal_stats,
            steps: checkpoint.steps,
            measurements: checkpoint.measurements,
            scalar_trace_rows: checkpoint.scalar_trace_rows,
            elapsed_time: checkpoint.elapsed_time,
        })
    }

    /// Converts mutable run state into a resumable CDT/MCMC checkpoint.
    ///
    /// The conversion rebuilds the generic chain counters from CDT move
    /// statistics so serialized checkpoints keep both accounting systems in
    /// lockstep.
    fn into_checkpoint(
        self,
        config: MetropolisConfig,
        action_config: ActionConfig,
    ) -> CdtResult<CdtMcmcCheckpoint> {
        self.triangulation.validate_evolved_cdt()?;
        let (accepted, rejected) = chain_counters(&self.move_stats)?;
        let current_step = NonZeroU32::new(self.current_step).ok_or_else(|| {
            checkpoint_resume_failed(CheckpointResumeFailure::StepTelemetryLengthMismatch {
                actual: 0,
                expected: 1,
            })
        })?;
        CdtMcmcCheckpoint::from_parts(CdtMcmcCheckpointParts {
            triangulation: self.triangulation,
            accepted,
            rejected,
            config,
            action_config,
            current_step,
            current_action: self.current_action,
            move_stats: self.move_stats,
            proposal_stats: self.proposal_stats,
            steps: self.steps,
            measurements: self.measurements,
            scalar_trace_rows: self.scalar_trace_rows,
            elapsed_time: self.elapsed_time,
            acceptance_rng: self.acceptance_rng,
            ergodics: self.ergodics,
        })
    }
}

/// Records one upstream planned-proposal result into CDT-specific telemetry.
///
/// Fresh and resumed simulations use the same upstream sampler path so
/// checkpoint continuation cannot drift from ordinary sampling behavior.
fn record_planned_step(
    algorithm: &MetropolisAlgorithm,
    state: &mut MetropolisRunState,
    step: NonZeroU32,
    planned_step: &DelayedStep<CdtProposalInfo>,
    info: CdtProposalInfo,
    proposal_stats: &ProposalStatistics,
    triangulation: &CdtTriangulation2D,
) -> CdtResult<()> {
    record_planned_step_parts(
        algorithm,
        state,
        step,
        PlannedStepRecord {
            outcome: planned_step.outcome,
            log_prob_after: planned_step.log_prob_after,
            info,
            proposal_stats,
            triangulation,
        },
    )
}

/// CDT-owned state needed to translate one upstream planned step.
///
/// Keeping these fields together lets production code and regression tests run
/// the same translation path while preserving the relationship between
/// reconstructed `action_after` values and public `delta_action` telemetry.
/// Accepted records must carry either CDT's `action_after` or the upstream
/// `log_prob_after` needed to reconstruct it.
#[derive(Clone, Copy)]
struct PlannedStepRecord<'a> {
    outcome: StepOutcome,
    log_prob_after: Option<f64>,
    info: CdtProposalInfo,
    proposal_stats: &'a ProposalStatistics,
    triangulation: &'a CdtTriangulation2D,
}

/// Converts upstream step outcomes into CDT scalar trace outcomes.
const fn scalar_trace_outcome_for_step(
    step: u32,
    outcome: StepOutcome,
) -> CdtResult<CdtScalarTraceOutcome> {
    match outcome {
        StepOutcome::Accepted => Ok(CdtScalarTraceOutcome::Accepted),
        StepOutcome::RejectedProposal => Ok(CdtScalarTraceOutcome::RejectedProposal),
        StepOutcome::NoProposal => Ok(CdtScalarTraceOutcome::NoProposal),
        outcome => Err(CdtError::UnexpectedPlannedStepOutcome { step, outcome }),
    }
}

/// Builds validated public step telemetry outcome from upstream planned-step data.
fn step_outcome_for_trace(
    config: &MetropolisConfig,
    step: NonZeroU32,
    trace_outcome: CdtScalarTraceOutcome,
    action_before: f64,
    log_prob_after: Option<f64>,
    info: &CdtProposalInfo,
) -> CdtResult<MonteCarloStepOutcome> {
    match trace_outcome {
        CdtScalarTraceOutcome::Accepted => {
            let applied_action = info
                .action_after
                .or_else(|| {
                    log_prob_after.map(|log_prob_after| -config.temperature() * log_prob_after)
                })
                .ok_or_else(|| missing_planned_step_info(step.get()))?;
            MonteCarloStepOutcome::accepted_transition(
                step,
                action_before,
                applied_action,
                applied_action - action_before,
            )
        }
        CdtScalarTraceOutcome::RejectedProposal => {
            MonteCarloStepOutcome::rejected_proposal(step, info.delta_action)
        }
        CdtScalarTraceOutcome::NoProposal => Ok(MonteCarloStepOutcome::NoProposal),
    }
}

/// Apply one planned-step record to CDT run state and public telemetry.
///
/// Accepted steps may reconstruct `action_after` from upstream log-probability
/// data. When that happens, the public [`MonteCarloStep`] receives a matching
/// `delta_action` derived from the same reconstructed action. Missing
/// accepted-step action evidence is rejected before any run state or telemetry
/// is mutated.
fn record_planned_step_parts(
    algorithm: &MetropolisAlgorithm,
    state: &mut MetropolisRunState,
    step: NonZeroU32,
    record: PlannedStepRecord<'_>,
) -> CdtResult<()> {
    let PlannedStepRecord {
        outcome,
        log_prob_after,
        info,
        proposal_stats,
        triangulation,
    } = record;
    let move_type = info.move_type;
    let action_before = state.current_action;
    let trace_outcome = scalar_trace_outcome_for_step(step.get(), outcome)?;
    let step_outcome = step_outcome_for_trace(
        &algorithm.config,
        step,
        trace_outcome,
        action_before,
        log_prob_after,
        &info,
    )?;
    let action_after = step_outcome.action_after();
    let delta_action = step_outcome.delta_action();

    let mut next_move_stats = state.move_stats.clone();
    next_move_stats.record_attempt(move_type);
    let mut accepted_candidate = action_after.map(|applied_action| {
        next_move_stats.record_success(move_type);
        AcceptedCandidate::new(
            step,
            move_type,
            action_before,
            applied_action,
            triangulation,
        )
    });
    if let Some(candidate) = &accepted_candidate {
        candidate.validate_if_due(next_move_stats.total_accepted())?;
    }

    let trace_action = accepted_candidate
        .as_ref()
        .map_or(state.current_action, AcceptedCandidate::action_after);
    let trace_triangulation = accepted_candidate
        .as_ref()
        .map_or(&state.triangulation, AcceptedCandidate::triangulation);
    let step_entry = MonteCarloStep::new(step, move_type, action_before, step_outcome)?;
    let scalar_trace_row = CdtScalarTraceRow::new(
        step,
        trace_outcome,
        -trace_action / algorithm.config.temperature(),
        trace_action,
        trace_triangulation,
        move_type,
        delta_action,
        action_before,
        action_after,
        state.trace_seed,
    )?;
    let measurement =
        staged_measurement_for_step(algorithm, step, trace_action, trace_triangulation)?;

    state.move_stats = next_move_stats;
    if let Some(candidate) = accepted_candidate.take() {
        let (applied_action, triangulation) = candidate.into_parts();
        state.current_action = applied_action;
        state.triangulation = triangulation;
    } else {
        state
            .triangulation
            .record_event(SimulationEvent::MoveAttempted {
                move_type,
                step: step.get().into(),
            });
    }

    state.proposal_stats.extend(proposal_stats);
    match trace_outcome {
        CdtScalarTraceOutcome::Accepted => state.proposal_stats.record_accepted_transition(),
        CdtScalarTraceOutcome::RejectedProposal => {
            state.proposal_stats.record_metropolis_rejection();
        }
        CdtScalarTraceOutcome::NoProposal => {}
    }

    state.steps.push(step_entry);
    state.scalar_trace_rows.push(scalar_trace_row);

    if let Some(measurement) = measurement {
        state.measurements.push(measurement);
        state
            .triangulation
            .record_event(SimulationEvent::MeasurementTaken {
                step: step.get().into(),
                action: state.current_action,
            });
    }

    Ok(())
}

/// Staged accepted proposal state that has its action and triangulation together.
struct AcceptedCandidate {
    action_after: f64,
    triangulation: CdtTriangulation2D,
}

impl AcceptedCandidate {
    /// Builds the accepted-state candidate before it is committed to live run state.
    fn new(
        step: NonZeroU32,
        move_type: MoveType,
        action_before: f64,
        action_after: f64,
        triangulation: &CdtTriangulation2D,
    ) -> Self {
        let mut triangulation = triangulation.clone();
        triangulation.record_event(SimulationEvent::MoveAttempted {
            move_type,
            step: step.get().into(),
        });
        triangulation.record_event(SimulationEvent::MoveAccepted {
            move_type,
            step: step.get().into(),
            action_change: action_after - action_before,
        });
        Self {
            action_after,
            triangulation,
        }
    }

    /// Returns the accepted action carried with the staged triangulation.
    const fn action_after(&self) -> f64 {
        self.action_after
    }

    /// Borrows the staged accepted triangulation.
    const fn triangulation(&self) -> &CdtTriangulation2D {
        &self.triangulation
    }

    /// Validates the staged triangulation at the configured backend cadence.
    fn validate_if_due(&self, accepted_moves: u64) -> CdtResult<()> {
        validate_evolved_cdt_candidate_if_due(&self.triangulation, accepted_moves)
    }

    /// Splits the validated accepted candidate for commitment to run state.
    fn into_parts(self) -> (f64, CdtTriangulation2D) {
        (self.action_after, self.triangulation)
    }
}

/// Builds a measurement for a planned step only when the configured cadence is due.
fn staged_measurement_for_step(
    algorithm: &MetropolisAlgorithm,
    step: NonZeroU32,
    action: f64,
    triangulation: &CdtTriangulation2D,
) -> CdtResult<Option<Measurement>> {
    if measurement_is_due(
        step.get(),
        algorithm.config.thermalization_steps(),
        algorithm.config.measurement_frequency(),
    ) {
        measurement_for(step.get(), action, triangulation).map(Some)
    } else {
        Ok(None)
    }
}

/// Maps upstream planned-proposal failures into CDT runner errors.
///
/// Proposal-stage errors preserve move-family context through
/// [`CdtError::MetropolisMoveApplicationFailed`]. Future upstream variants use
/// [`CdtError::PlannedProposalStepFailed`] so they remain distinct from CDT's own
/// missing-telemetry and unsupported-outcome invariants.
fn planned_step_error(step: u32, error: DelayedStepError<CdtProposalError>) -> CdtError {
    match error {
        DelayedStepError::Mcmc(err) => CdtError::Mcmc(err),
        DelayedStepError::Plan(err)
        | DelayedStepError::ProposedLogProb(err)
        | DelayedStepError::LogQRatio(err)
        | DelayedStepError::Commit(err) => proposal_step_error(step, err),
        unexpected => CdtError::PlannedProposalStepFailed {
            step,
            detail: unexpected.to_string(),
        },
    }
}

/// Converts a CDT proposal error into the public accepted-move failure shape.
///
/// This keeps planned-proposal sampler errors compatible with the historical CDT error
/// contract for hard move-application failures.
fn proposal_step_error(step: u32, error: CdtProposalError) -> CdtError {
    match error {
        CdtProposalError::ApplicationFailed {
            move_type,
            attempt,
            source,
        } => accepted_move_error(step, move_type, attempt, source),
    }
}

/// Builds the explicit error used when upstream planned-step telemetry is absent.
const fn missing_planned_step_info(step: u32) -> CdtError {
    CdtError::PlannedProposalTelemetryMissing { step }
}

/// Runs the expensive full evolved-state validation only when the backend policy is due.
#[cfg(test)]
fn validate_evolved_cdt_if_due(state: &MetropolisRunState) -> CdtResult<()> {
    validate_evolved_cdt_candidate_if_due(&state.triangulation, state.move_stats.total_accepted())
}

/// Runs expensive evolved-state validation for a staged accepted triangulation.
fn validate_evolved_cdt_candidate_if_due(
    triangulation: &CdtTriangulation2D,
    accepted_moves: u64,
) -> CdtResult<()> {
    if triangulation
        .geometry()
        .should_check_delaunay_after(accepted_moves)
    {
        triangulation.validate_evolved_cdt()?;
    }
    Ok(())
}

/// Builds the RNG used only for Metropolis acceptance draws.
///
/// This keeps acceptance randomness separate from move-site selection, so seeded
/// simulations are reproducible while unseeded simulations still draw fresh entropy.
fn simulation_rng(seed: Option<u64>) -> Xoshiro256PlusPlus {
    seed.map_or_else(rand::make_rng, Xoshiro256PlusPlus::seed_from_u64)
}

/// Builds the simulation-level error for an accepted move that could not be applied.
///
/// The move kernels keep causal, geometric, and backend failures orthogonal; this
/// wrapper adds the Metropolis step, move type, and retry context callers need to
/// debug a failed accepted application.
fn accepted_move_error(
    step: u32,
    move_type: MoveType,
    attempts: usize,
    source: CdtError,
) -> CdtError {
    CdtError::MetropolisMoveApplicationFailed {
        step,
        move_type,
        attempts,
        source: MetropolisMoveApplicationFailure::from(source),
    }
}

#[cfg(test)]
mod tests {
    use super::super::adapter::{
        CdtProposal, CdtProposalError, CdtProposalPlan, concrete_log_q_ratio, propose_concrete_plan,
    };
    use super::super::helpers::{SimplexCounts, proposed_delta_action, simplex_counts};
    use super::super::telemetry::CdtProposalSiteRejection;
    use super::*;
    use crate::cdt::action::CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT;
    use crate::cdt::ergodic_moves::proposal_site_count;
    use crate::cdt::foliation::FoliationError;
    use crate::cdt::triangulation::CdtTriangulation;
    use crate::errors::{BackendMutationOperation, CheckpointMoveCounter, ConfigurationSetting};
    use crate::geometry::traits::TriangulationQuery;
    use approx::assert_relative_eq;
    use markov_chain_monte_carlo::{
        Chain, DelayedProposal, DelayedStepError, DiscreteProposalRatio, McmcError, Target,
    };
    use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
    use serde_json::{from_str, to_string, to_value};
    use std::assert_matches;
    use std::error::Error;
    use std::num::{NonZeroU32, NonZeroUsize};

    /// Applies the Metropolis acceptance rule to a proposed action change.
    ///
    /// Factoring this out keeps the probability rule isolated from move selection
    /// and makes deterministic unit tests possible with a seeded RNG.
    fn metropolis_accept<R: Rng + ?Sized>(
        delta_action: f64,
        temperature: f64,
        rng: &mut R,
    ) -> bool {
        metropolis_accept_log_alpha(-delta_action / temperature, rng)
    }

    fn metropolis_accept_log_alpha<R: Rng + ?Sized>(log_alpha: f64, rng: &mut R) -> bool {
        log_alpha >= 0.0 || rng.random::<f64>() < log_alpha.exp()
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

    fn assert_optional_relative_eq(left: Option<f64>, right: Option<f64>) {
        match (left, right) {
            (Some(left), Some(right)) => assert_relative_eq!(left, right, epsilon = 1e-12),
            (None, None) => {}
            other => panic!("expected matching optional floats, got {other:?}"),
        }
    }

    type CanonicalVertexSignature = (Option<u32>, Vec<u64>);
    type CanonicalFaceSignature = (Option<i32>, Vec<CanonicalVertexSignature>);

    fn canonical_vertex_signatures(
        triangulation: &CdtTriangulation2D,
    ) -> Vec<CanonicalVertexSignature> {
        let geometry = triangulation.geometry();
        let mut vertices = geometry
            .vertices()
            .map(|vertex| {
                let coordinates = geometry
                    .vertex_coordinates(&vertex)
                    .expect("test vertex coordinates should resolve")
                    .into_iter()
                    .map(f64::to_bits)
                    .collect();
                (
                    geometry.vertex_data_by_key(vertex.vertex_key()),
                    coordinates,
                )
            })
            .collect::<Vec<_>>();
        vertices.sort();
        vertices
    }

    fn canonical_face_signatures(
        triangulation: &CdtTriangulation2D,
    ) -> Vec<CanonicalFaceSignature> {
        let geometry = triangulation.geometry();
        let mut faces = geometry
            .faces()
            .map(|face| {
                let mut vertices = geometry
                    .face_vertices(&face)
                    .expect("test face vertices should resolve")
                    .into_iter()
                    .map(|vertex| {
                        let coordinates = geometry
                            .vertex_coordinates(&vertex)
                            .expect("test face vertex coordinates should resolve")
                            .into_iter()
                            .map(f64::to_bits)
                            .collect();
                        (
                            geometry.vertex_data_by_key(vertex.vertex_key()),
                            coordinates,
                        )
                    })
                    .collect::<Vec<_>>();
                vertices.sort();
                (geometry.simplex_data_by_key(face.simplex_key()), vertices)
            })
            .collect::<Vec<_>>();
        faces.sort();
        faces
    }

    fn assert_canonical_triangulations_match(
        left: &CdtTriangulation2D,
        right: &CdtTriangulation2D,
    ) {
        assert_eq!(left.vertex_count(), right.vertex_count());
        assert_eq!(left.edge_count(), right.edge_count());
        assert_eq!(left.face_count(), right.face_count());
        assert_eq!(left.slice_sizes(), right.slice_sizes());
        assert_eq!(
            left.volume_profile()
                .expect("left canonical profile should be valid"),
            right
                .volume_profile()
                .expect("right canonical profile should be valid")
        );
        assert_eq!(
            left.metadata().time_slices(),
            right.metadata().time_slices()
        );
        assert_eq!(left.metadata().dimension(), right.metadata().dimension());
        assert_eq!(left.metadata().topology(), right.metadata().topology());
        assert_eq!(
            left.metadata().modification_count(),
            right.metadata().modification_count()
        );
        assert_eq!(
            to_value(left.metadata().simulation_history())
                .expect("left simulation history should serialize"),
            to_value(right.metadata().simulation_history())
                .expect("right simulation history should serialize")
        );
        assert_eq!(
            canonical_vertex_signatures(left),
            canonical_vertex_signatures(right)
        );
        assert_eq!(
            canonical_face_signatures(left),
            canonical_face_signatures(right)
        );
    }

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

    fn short_checkpoint() -> CdtMcmcCheckpoint {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 13),
            ActionConfig::default(),
        )
        .run_to_checkpoint(triangulation)
        .expect("short prefix run should checkpoint")
    }

    fn serializable_rejected_checkpoint(action_config: ActionConfig) -> CdtMcmcCheckpoint {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 13);
        let algorithm = MetropolisAlgorithm::new(config.clone(), action_config.clone());
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        let move_type = state.ergodics.select_random_move();
        let _acceptance_rng_marker: f64 = state.acceptance_rng.random();

        state.move_stats.record_attempt(move_type);
        state
            .triangulation
            .record_event(SimulationEvent::MoveAttempted { move_type, step: 1 });
        state.steps.push(rejected_proposal_step(
            1,
            move_type,
            state.current_action,
            proposed_delta_action(
                &action_config,
                simplex_counts(&state.triangulation),
                move_type,
            ),
        ));
        state.current_step = 1;
        state.proposal_stats.record_move_family(1);
        state.proposal_stats.record_metropolis_rejection();
        state.scalar_trace_rows.push(
            CdtScalarTraceRow::new(
                step_number(1),
                CdtScalarTraceOutcome::RejectedProposal,
                -state.current_action / config.temperature(),
                state.current_action,
                &state.triangulation,
                move_type,
                state.steps[0].delta_action(),
                state.current_action,
                None,
                config.seed(),
            )
            .expect("trace row should build"),
        );
        state.measurements.push(
            measurement_for(1, state.current_action, &state.triangulation)
                .expect("measurement should build"),
        );
        state
            .triangulation
            .record_event(SimulationEvent::MeasurementTaken {
                step: 1,
                action: state.current_action,
            });

        state
            .into_checkpoint(config, action_config)
            .expect("synthetic rejected checkpoint should validate")
    }

    fn synthetic_one_step_checkpoint_parts(accepted: bool) -> CdtMcmcCheckpointParts {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 13);
        let action_config = ActionConfig::default();
        let current_action = action_for(&action_config, &triangulation);
        let temperature = config.temperature();
        let seed = config.seed();
        let move_type = MoveType::Move22;
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(move_type);
        if accepted {
            move_stats.record_success(move_type);
        }
        let outcome = if accepted {
            CdtScalarTraceOutcome::Accepted
        } else {
            CdtScalarTraceOutcome::RejectedProposal
        };

        CdtMcmcCheckpointParts {
            triangulation: triangulation.clone(),
            accepted: usize::from(accepted),
            rejected: usize::from(!accepted),
            config,
            action_config,
            current_step: step_number(1),
            current_action,
            move_stats,
            proposal_stats: if accepted {
                ProposalStatistics::from_validated_parts(1, 1, 0, 0, 0, 0, 0, 1, 0)
            } else {
                ProposalStatistics::from_validated_parts(1, 1, 0, 0, 0, 0, 1, 0, 0)
            },
            steps: vec![if accepted {
                accepted_step(1, move_type, current_action, current_action)
            } else {
                rejected_proposal_step(1, move_type, current_action, None)
            }],
            measurements: vec![
                measurement_for(0, current_action, &triangulation)
                    .expect("initial measurement should build"),
                measurement_for(1, current_action, &triangulation)
                    .expect("step measurement should build"),
            ],
            scalar_trace_rows: vec![
                CdtScalarTraceRow::new(
                    step_number(1),
                    outcome,
                    -current_action / temperature,
                    current_action,
                    &triangulation,
                    move_type,
                    accepted.then_some(0.0),
                    current_action,
                    accepted.then_some(current_action),
                    seed,
                )
                .expect("trace row should build"),
            ],
            elapsed_time: Duration::ZERO,
            acceptance_rng: simulation_rng(Some(1)),
            ergodics: ErgodicsSystem::with_seed(2),
        }
    }

    fn synthetic_one_step_checkpoint(accepted: bool) -> CdtMcmcCheckpoint {
        CdtMcmcCheckpoint::from_parts(synthetic_one_step_checkpoint_parts(accepted))
            .expect("synthetic checkpoint should validate")
    }

    fn empty_run_state(triangulation: CdtTriangulation2D) -> MetropolisRunState {
        MetropolisRunState {
            triangulation,
            current_step: 0,
            current_action: 0.0,
            trace_seed: Some(1),
            acceptance_rng: simulation_rng(Some(1)),
            ergodics: ErgodicsSystem::with_seed(2),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: Vec::new(),
            measurements: Vec::new(),
            scalar_trace_rows: Vec::new(),
            elapsed_time: Duration::ZERO,
        }
    }

    fn assert_checkpoint_resume_failed<T>(
        result: CdtResult<T>,
        matches_failure: impl FnOnce(&CheckpointResumeFailure) -> bool,
        expected_detail: &str,
    ) {
        let Err(CdtError::CheckpointResumeFailed { failure }) = result else {
            panic!("expected checkpoint resume failure");
        };
        assert!(
            matches_failure(&failure),
            "unexpected checkpoint resume failure: {failure:?}"
        );
        let detail = failure.to_string();
        assert!(
            detail.contains(expected_detail),
            "expected detail to contain {expected_detail:?}, got {detail:?}"
        );
    }

    #[test]
    fn full_validation_cadence_uses_delaunay_check_policy() {
        let mut triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        triangulation.set_delaunay_check_interval(NonZeroUsize::new(1));
        let vertex = triangulation
            .geometry()
            .vertices()
            .find(|vertex| triangulation.time_label(vertex) == Some(1))
            .expect("fixture has a slice-1 vertex");
        triangulation
            .set_vertex_data(&vertex, Some(0))
            .expect("fixture vertex label can be edited");

        let mut state = empty_run_state(triangulation);
        state.move_stats.record_success(MoveType::Move22);

        assert!(
            validate_evolved_cdt_if_due(&state).is_err(),
            "EveryN(1) should run full validation after the first accepted move"
        );
    }

    #[test]
    fn end_only_validation_policy_defers_until_checkpoint() {
        let mut triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        triangulation.set_delaunay_check_interval(None);
        let vertex = triangulation
            .geometry()
            .vertices()
            .find(|vertex| triangulation.time_label(vertex) == Some(1))
            .expect("fixture has a slice-1 vertex");
        triangulation
            .set_vertex_data(&vertex, Some(0))
            .expect("fixture vertex label can be edited");

        let mut state = empty_run_state(triangulation);
        state.move_stats.record_success(MoveType::Move22);

        validate_evolved_cdt_if_due(&state)
            .expect("EndOnly should skip cadence validation on accepted moves");
        assert!(
            state
                .into_checkpoint(MetropolisConfig::default(), ActionConfig::default())
                .is_err(),
            "mandatory checkpoint validation should still catch the invalid final state"
        );
    }

    #[test]
    fn test_metropolis_config() {
        let config = metropolis_config(2.0, 500, 50, 5);
        assert_relative_eq!(config.temperature(), 2.0);
        assert_relative_eq!(config.beta(), 0.5);
        assert_eq!(config.steps().get(), 500);
        assert!(config.seed().is_none());

        let seeded = config.with_seed(123);
        assert_eq!(seeded.seed(), Some(123));
    }

    #[test]
    fn backend_counts_vertices_edges() {
        // Use fixed seed
        const TRIANGULATION_SEED: u64 = 53;

        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, TRIANGULATION_SEED)
            .expect("Failed to create triangulation with fixed seed");
        let geometry = triangulation.geometry();

        // We intentionally do NOT rely on the upstream deep validation here, since it can be flaky
        // for some generated point sets. Backend-level validity means the triangulation is
        // structurally usable by this crate (counts and iterators behave as expected).
        assert!(
            geometry.is_valid(),
            "Triangulation should be structurally valid for backend queries"
        );

        // Ensure the backend exposes the expected simplex counts.
        assert_eq!(
            geometry.vertex_count(),
            5,
            "Vertex count should match requested seeded generation"
        );
        assert!(geometry.edge_count() > 0, "Should have edges");
        assert!(geometry.face_count() > 0, "Should have faces");
    }

    #[test]
    fn test_action_calculation() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 1, 2).expect("Failed to create triangulation");

        let config = MetropolisConfig::default();
        let action_config = ActionConfig::default();
        let _algorithm = MetropolisAlgorithm::new(config, action_config.clone());

        let geometry = triangulation.geometry();
        let action = action_config.calculate_action(
            geometry.vertex_count(),
            geometry.edge_count(),
            geometry.face_count(),
        );

        // Since we're using a random triangulation, just verify it returns a finite value
        assert!(action.is_finite());
    }

    #[test]
    fn test_cdt_target_log_prob() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53)
            .expect("Failed to create triangulation");

        let target =
            CdtTarget::new(ActionConfig::default(), 1.0).expect("valid target configuration");

        let log_prob = Target::log_prob(&target, &triangulation);
        assert!(log_prob.is_finite(), "log_prob should be finite");

        // log_prob = -action/T, so with T=1 it should be the negative of the action
        let g = triangulation.geometry();
        let action = ActionConfig::default().calculate_action(
            g.vertex_count(),
            g.edge_count(),
            g.face_count(),
        );
        assert_relative_eq!(log_prob, -action);
    }

    #[test]
    fn seeded_simulation_runs_moves() {
        let config = seeded_metropolis_config(1.0, 10, 2, 2, 42);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        let triangulation =
            CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed to create");
        let results = algorithm
            .run(triangulation)
            .expect("simulation should run with real move loop");

        assert_eq!(results.steps().len(), 10);
        assert_relative_eq!(
            results.move_stats().total_acceptance_rate(),
            results.acceptance_rate()
        );
        assert!(results.measurements().iter().all(|measurement| {
            measurement.action().is_finite()
                && measurement.vertices().get() > 0
                && measurement.edges().get() > 0
                && measurement.triangles().get() > 0
        }));
    }

    #[test]
    fn run_skips_pre_thermalization_measurements() {
        let config = seeded_metropolis_config(1.0, 4, 2, 2, 42);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let results = algorithm
            .run(triangulation)
            .expect("simulation should run with post-thermalization measurements");
        let measurement_steps = results
            .measurements()
            .iter()
            .map(Measurement::step)
            .collect::<Vec<_>>();

        assert_eq!(measurement_steps, vec![2, 4]);
        assert!(
            results
                .measurements()
                .iter()
                .all(|measurement| measurement.step() >= results.config().thermalization_steps())
        );
    }

    #[test]
    fn run_with_checkpoint_returns_matching_results_and_checkpoint() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 3, 0, 1, 13),
            ActionConfig::default(),
        );

        let (results, checkpoint) = algorithm
            .run_with_checkpoint(triangulation)
            .expect("checkpointed run should complete");

        assert_eq!(checkpoint.current_step().get(), 3);
        assert_eq!(results.steps().len(), checkpoint.steps().len());
        assert_eq!(results.config(), checkpoint.config());
        let checkpoint_results = checkpoint.into_results();
        assert_eq!(
            results.triangulation().vertex_count(),
            checkpoint_results.triangulation().vertex_count()
        );
        checkpoint_results
            .triangulation()
            .validate()
            .expect("checkpoint triangulation should satisfy evolved invariants");
    }

    #[test]
    fn checkpoint_accessors_report_consistent_snapshot() {
        let checkpoint = short_checkpoint();
        let current_step = usize::try_from(checkpoint.current_step().get())
            .expect("u32 step count should fit usize");
        let accepted_moves = usize::try_from(checkpoint.move_stats().total_accepted())
            .expect("test accepted move count should fit usize");
        let last_step = checkpoint
            .steps()
            .last()
            .expect("checkpoint should contain step telemetry");
        let last_measurement = checkpoint
            .measurements()
            .last()
            .expect("checkpoint should contain measurements");

        assert_eq!(
            checkpoint.chain().state().vertex_count(),
            checkpoint.triangulation().vertex_count()
        );
        assert_eq!(checkpoint.chain().total_steps(), current_step);
        assert_eq!(checkpoint.chain().accepted(), accepted_moves);
        assert_eq!(checkpoint.config().steps(), checkpoint.current_step());
        assert_eq!(checkpoint.action_config(), &ActionConfig::default());
        assert!(checkpoint.current_action().is_finite());
        assert_eq!(
            checkpoint.move_stats().total_attempted(),
            u64::from(checkpoint.current_step().get())
        );
        assert_eq!(
            checkpoint.proposal_stats().move_family_proposals(),
            u64::from(checkpoint.current_step().get())
        );
        assert_eq!(last_step.step(), checkpoint.current_step());
        assert_eq!(last_measurement.step(), checkpoint.current_step().get());
        assert_relative_eq!(
            last_measurement.action(),
            checkpoint.current_action(),
            epsilon = 1e-12
        );
    }

    #[test]
    fn serialized_checkpoint_resumes_from_stored_rng_state() {
        let action_config = ActionConfig::default();
        let checkpoint = serializable_rejected_checkpoint(action_config.clone());
        let checkpoint_json = to_string(&checkpoint).expect("checkpoint should serialize");
        let checkpoint: CdtMcmcCheckpoint =
            from_str(&checkpoint_json).expect("checkpoint should deserialize");
        let alternate_checkpoint: CdtMcmcCheckpoint =
            from_str(&checkpoint_json).expect("checkpoint should deserialize again");
        let first_resume_algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 6, 0, 1, 999),
            action_config.clone(),
        );
        let first_resumed = first_resume_algorithm
            .resume_from_checkpoint(checkpoint)
            .expect("resume should complete");
        let second_resume_algorithm =
            MetropolisAlgorithm::new(seeded_metropolis_config(1.0, 6, 0, 1, 123), action_config);
        let second_resumed = second_resume_algorithm
            .resume_from_checkpoint(alternate_checkpoint)
            .expect("resume should ignore fresh seed and use checkpoint RNG state");

        assert_eq!(first_resumed.config().steps().get(), 7);
        assert_eq!(first_resumed.steps().len(), 7);
        assert_eq!(first_resumed.steps()[1].step().get(), 2);
        first_resumed
            .triangulation()
            .validate_topology()
            .expect("resumed triangulation should preserve topology");
        first_resumed
            .triangulation()
            .validate_foliation()
            .expect("resumed triangulation should preserve foliation");
        first_resumed
            .triangulation()
            .validate_causality()
            .expect("resumed triangulation should preserve causality");
        first_resumed
            .triangulation()
            .validate_simplex_classification()
            .expect("resumed triangulation should preserve simplex classification");
        assert_eq!(
            to_value(first_resumed.steps()).expect("steps should serialize"),
            to_value(second_resumed.steps()).expect("steps should serialize")
        );
        assert_eq!(
            to_value(first_resumed.measurements()).expect("measurements should serialize"),
            to_value(second_resumed.measurements()).expect("measurements should serialize")
        );
        assert_eq!(
            to_value(first_resumed.move_stats()).expect("stats should serialize"),
            to_value(second_resumed.move_stats()).expect("stats should serialize")
        );
        assert_eq!(
            first_resumed.triangulation().vertex_count(),
            second_resumed.triangulation().vertex_count()
        );
        assert_eq!(
            first_resumed.triangulation().edge_count(),
            second_resumed.triangulation().edge_count()
        );
        assert_eq!(
            first_resumed.triangulation().face_count(),
            second_resumed.triangulation().face_count()
        );
        assert_eq!(
            first_resumed.triangulation().slice_sizes(),
            second_resumed.triangulation().slice_sizes()
        );
    }

    #[test]
    fn chunked_checkpoint_resume_matches_one_shot_seeded_run() {
        let action_config = ActionConfig::default();
        let one_shot = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 10, 0, 1, 19),
            action_config.clone(),
        )
        .run(CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build"))
        .expect("one-shot run should complete");

        let prefix = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 4, 0, 1, 19),
            action_config.clone(),
        )
        .run_to_checkpoint(
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build"),
        )
        .expect("prefix run should checkpoint");

        let chunked_checkpoint =
            MetropolisAlgorithm::new(seeded_metropolis_config(1.0, 6, 0, 1, 999), action_config)
                .resume_to_checkpoint(prefix)
                .expect("chunked checkpoint resume should complete");
        let chunked = chunked_checkpoint.into_results();

        assert_eq!(chunked.config().steps().get(), 10);
        assert_eq!(
            to_value(one_shot.steps()).expect("steps should serialize"),
            to_value(chunked.steps()).expect("steps should serialize")
        );
        assert_eq!(
            to_value(one_shot.measurements()).expect("measurements should serialize"),
            to_value(chunked.measurements()).expect("measurements should serialize")
        );
        assert_eq!(
            to_value(one_shot.move_stats()).expect("move stats should serialize"),
            to_value(chunked.move_stats()).expect("move stats should serialize")
        );
        assert_eq!(
            to_value(one_shot.proposal_stats()).expect("proposal stats should serialize"),
            to_value(chunked.proposal_stats()).expect("proposal stats should serialize")
        );
        assert_canonical_triangulations_match(one_shot.triangulation(), chunked.triangulation());
        assert_eq!(
            one_shot
                .triangulation()
                .volume_profile()
                .expect("one-shot profile should be valid"),
            chunked
                .triangulation()
                .volume_profile()
                .expect("chunked profile should be valid")
        );
    }

    #[test]
    fn serialized_checkpoint_missing_proposal_stats_rejects_nonempty_checkpoint() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload
            .as_object_mut()
            .expect("checkpoint payload should be an object")
            .remove("proposal_stats");

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("nonempty checkpoint missing proposal stats should be rejected");
        };

        assert!(
            error
                .to_string()
                .contains("proposal move-family count mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn serialized_checkpoint_rejects_zero_current_step() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload
            .as_object_mut()
            .expect("checkpoint payload should be an object")
            .insert(
                "current_step".to_string(),
                to_value(0_u32).expect("step should serialize"),
            );

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("checkpoint step zero should fail while parsing");
        };

        assert!(
            error.to_string().contains("current_step must be nonzero"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn serialized_checkpoint_rejects_mismatched_current_action() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload["current_action"] =
            to_value(checkpoint.current_action() + 1.0).expect("action should serialize");

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("checkpoint action mismatch should fail while parsing");
        };

        assert!(
            error.to_string().contains("checkpoint action mismatch"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn checkpoint_validation_rejects_nonfinite_current_action() {
        for current_action in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut parts = synthetic_one_step_checkpoint_parts(false);
            parts.current_action = current_action;

            assert_checkpoint_resume_failed(
                CdtMcmcCheckpoint::from_parts(parts),
                |failure| match failure {
                    CheckpointResumeFailure::NonFiniteCheckpointAction { stored } => {
                        stored.to_bits() == current_action.to_bits()
                    }
                    _ => false,
                },
                "checkpoint action is non-finite",
            );
        }
    }

    #[test]
    fn serialized_checkpoint_rejects_zero_step_telemetry() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload["steps"][0]["step"] = to_value(0_u32).expect("step should serialize");

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("checkpoint telemetry step zero should fail while parsing");
        };

        assert!(
            error.to_string().contains("nonzero") || error.to_string().contains("invalid value"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn serialized_checkpoint_rejects_zero_scalar_trace_step() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload["scalar_trace_rows"][0]["step"] = to_value(0_u32).expect("step should serialize");

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("zero scalar trace step should fail while parsing checkpoint");
        };

        assert!(
            error
                .to_string()
                .contains("scalar trace step must be nonzero"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn serialized_checkpoint_rejects_zero_measurement_counts_by_field() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());

        for field in ["vertices", "edges", "triangles"] {
            let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
            payload["measurements"][0][field] = to_value(0_u32).expect("count should serialize");

            let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
                panic!("zero measurement count should fail while parsing checkpoint");
            };
            let message = error.to_string();
            assert!(
                message.contains("Invalid measurement count") && message.contains(field),
                "unexpected serde error for {field}: {message}"
            );
        }
    }

    #[test]
    fn serialized_checkpoint_rejects_zero_scalar_trace_counts_by_field() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());

        for field in ["vertices", "edges", "triangles"] {
            let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
            payload["scalar_trace_rows"][0][field] =
                to_value(0_u32).expect("count should serialize");

            let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
                panic!("zero scalar trace count should fail while parsing checkpoint");
            };
            let message = error.to_string();
            assert!(
                message.contains("Invalid scalar trace count") && message.contains(field),
                "unexpected serde error for {field}: {message}"
            );
        }
    }

    #[test]
    fn serialized_checkpoint_rejects_missing_scalar_trace_rows() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload
            .as_object_mut()
            .expect("checkpoint payload should be an object")
            .remove("scalar_trace_rows");

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("missing scalar trace rows should fail while parsing checkpoint");
        };

        assert!(
            error
                .to_string()
                .contains("missing field `scalar_trace_rows`"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn serialized_checkpoint_rejects_counter_mismatch_on_restore() {
        let checkpoint = serializable_rejected_checkpoint(ActionConfig::default());
        let mut payload = to_value(&checkpoint).expect("checkpoint should serialize");
        payload
            .as_object_mut()
            .expect("checkpoint payload should be an object")
            .insert(
                "current_step".to_string(),
                to_value(2_u32).expect("step should serialize"),
            );

        let Err(error) = from_str::<CdtMcmcCheckpoint>(&payload.to_string()) else {
            panic!("checkpoint counter mismatch should be rejected during deserialization");
        };
        assert!(
            error
                .to_string()
                .contains("chain step count does not match checkpoint step"),
            "unexpected serde error: {error}"
        );
    }

    #[test]
    fn resume_rejects_incompatible_action_config() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            action_config(2.0, 1.0, 0.1),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::IncompatibleActionConfiguration
                )
            },
            "action configuration",
        );
    }

    #[test]
    fn resume_to_checkpoint_rejects_incompatible_action_config() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            action_config(2.0, 1.0, 0.1),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_to_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::IncompatibleActionConfiguration
                )
            },
            "action configuration",
        );
    }

    #[test]
    fn resume_rejects_action_config_within_action_value_tolerance() {
        let checkpoint = short_checkpoint();
        let checkpoint_action = checkpoint.action_config();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            action_config(
                checkpoint_action.coupling_0() + f64::EPSILON,
                checkpoint_action.coupling_2(),
                checkpoint_action.cosmological_constant(),
            ),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::IncompatibleActionConfiguration
                )
            },
            "action configuration",
        );
    }

    #[test]
    fn resume_rejects_incompatible_sampling_schedule() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 2, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::IncompatibleMeasurementFrequency
                )
            },
            "measurement frequency",
        );
    }

    #[test]
    fn resume_rejects_incompatible_temperature() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(2.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| matches!(failure, CheckpointResumeFailure::IncompatibleTemperature),
            "temperature differs",
        );
    }

    #[test]
    fn resume_rejects_incompatible_thermalization_schedule() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 1, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::IncompatibleThermalizationSchedule
                )
            },
            "thermalization schedule",
        );
    }

    #[test]
    fn resume_rejects_chain_step_mismatch() {
        let mut checkpoint = short_checkpoint();
        checkpoint.current_step = step_number(checkpoint.current_step.get() + 1);
        let chain_steps = checkpoint.chain.total_steps();
        let checkpoint_step = checkpoint.current_step.get();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::ChainStepMismatch {
                        chain_steps: actual_chain_steps,
                        checkpoint_step: actual_checkpoint_step,
                    } if *actual_chain_steps == chain_steps
                        && *actual_checkpoint_step == checkpoint_step
                )
            },
            "chain step count",
        );
    }

    #[test]
    fn resume_rejects_inconsistent_checkpoint_counters() {
        let mut checkpoint = short_checkpoint();
        checkpoint.move_stats.record_attempt(MoveType::Move22);
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::ChainCounterMismatch { .. }
                )
            },
            "chain counters",
        );
    }

    #[test]
    fn resume_rejects_inconsistent_step_telemetry() {
        let mut checkpoint = short_checkpoint();
        checkpoint.steps.pop();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::StepTelemetryLengthMismatch { .. }
                )
            },
            "step telemetry length",
        );
    }

    #[test]
    fn resume_rejects_nonsequential_step_telemetry() {
        let mut checkpoint = short_checkpoint();
        let move_type = checkpoint.steps[0].move_type();
        let action_before = checkpoint.steps[0].action_before();
        checkpoint.steps[0] = no_proposal_step(2, move_type, action_before);
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::StepTelemetrySequenceMismatch { .. }
                )
            },
            "step telemetry must be sequential",
        );
    }

    #[test]
    fn resume_rejects_step_acceptance_counter_mismatch() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0] = rejected_proposal_step(
            1,
            checkpoint.steps[0].move_type(),
            checkpoint.current_action(),
            None,
        );
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::StepTelemetryAcceptedCountMismatch { .. }
                )
            },
            "accepted step count mismatch",
        );
    }

    #[test]
    fn resume_rejects_checkpoint_proposal_hard_failures() {
        let mut checkpoint = synthetic_one_step_checkpoint(false);
        checkpoint.proposal_stats =
            ProposalStatistics::from_validated_parts(1, 1, 0, 0, 0, 0, 0, 0, 1);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::ProposalHardFailures { actual: 1 }
                )
            },
            "hard failures",
        );
    }

    #[test]
    fn resume_rejects_checkpoint_proposal_accepted_count_mismatch() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.proposal_stats =
            ProposalStatistics::from_validated_parts(1, 1, 1, 0, 0, 0, 0, 0, 0);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::ProposalAcceptedCountMismatch {
                        actual: 0,
                        expected: 1
                    }
                )
            },
            "accepted-transition count mismatch",
        );
    }

    #[test]
    fn resume_rejects_checkpoint_proposal_rejected_count_mismatch() {
        let mut checkpoint = synthetic_one_step_checkpoint(false);
        checkpoint.proposal_stats =
            ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::ProposalRejectedCountMismatch {
                        actual: 0,
                        expected: 1
                    }
                )
            },
            "rejected-transition count mismatch",
        );
    }

    #[test]
    fn step_constructor_rejects_nonfinite_action_before() {
        let error = MonteCarloStep::no_proposal(step_number(1), MoveType::Move22, f64::NAN)
            .expect_err("non-finite action_before should be rejected before storage");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::NonFiniteStepActionBefore { step: 1 }
            }
        );
    }

    #[test]
    fn step_constructor_rejects_nonfinite_delta_action() {
        let error = MonteCarloStep::rejected_proposal(
            step_number(1),
            MoveType::Move22,
            0.0,
            Some(f64::NAN),
        )
        .expect_err("non-finite delta_action should be rejected before storage");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::NonFiniteStepDeltaAction { step: 1 }
            }
        );
    }

    #[test]
    fn step_constructor_rejects_action_after_delta_mismatch() {
        let error = MonteCarloStep::accepted_step(step_number(1), MoveType::Move22, 0.0, 1.0, 0.0)
            .expect_err("action_after/delta mismatch should be rejected before storage");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::StepActionAfterDeltaMismatch { step: 1 }
            }
        );
    }

    #[test]
    fn step_constructor_rejects_nonfinite_step_action_after() {
        let error =
            MonteCarloStep::accepted_step(step_number(1), MoveType::Move22, 0.0, f64::NAN, 0.0)
                .expect_err("non-finite action_after should be rejected before storage");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::NonFiniteStepActionAfter { step: 1 }
            }
        );
    }

    #[test]
    fn resume_rejects_missing_scheduled_measurement() {
        let mut checkpoint = short_checkpoint();
        checkpoint.measurements.pop();
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::MeasurementCountMismatch { .. }
                )
            },
            "scheduled measurement count mismatch",
        );
    }

    #[test]
    fn resume_rejects_measurement_step_mismatch() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.measurements[1] =
            Measurement::try_new(2, checkpoint.measurements[1].action(), 12, 26, 12)
                .expect("replacement measurement should satisfy invariants");

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::MeasurementStepMismatch {
                        actual: 2,
                        expected: 1
                    }
                )
            },
            "measurement telemetry step mismatch",
        );
    }

    #[test]
    fn checkpoint_from_parts_rejects_action_mismatch() {
        let mut parts = synthetic_one_step_checkpoint_parts(false);
        parts.current_action += 1.0;

        assert_checkpoint_resume_failed(
            CdtMcmcCheckpoint::from_parts(parts),
            |failure| matches!(failure, CheckpointResumeFailure::ActionMismatch { .. }),
            "checkpoint action mismatch",
        );
    }

    #[test]
    fn checkpoint_config_deserialization_rejects_invalid_metropolis_config() {
        let payload = r#"{"temperature":0.0,"steps":2,"thermalization_steps":0,"measurement_frequency":1,"seed":13}"#;
        let Err(error) = from_str::<MetropolisConfig>(payload) else {
            panic!("expected Metropolis configuration failure");
        };
        let message = error.to_string();
        assert!(
            message.contains("temperature") && message.contains("finite and positive"),
            "unexpected serde error: {message}"
        );
    }

    #[test]
    fn chain_counters_rejects_accepted_above_attempted() {
        let stats = MoveStatistics::from_validated_parts(0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let Err(CdtError::CheckpointResumeFailed { failure }) = chain_counters(&stats) else {
            panic!("expected impossible move statistics to fail");
        };

        assert_matches!(
            failure,
            CheckpointResumeFailure::MoveAcceptedExceedsAttempted {
                move_type: MoveType::Move22
            }
        );
        let detail = failure.to_string();
        assert!(detail.contains("accepted move count exceeds attempted move count"));
    }

    #[test]
    fn chain_counters_rejects_counter_sum_overflow() {
        let stats = MoveStatistics::from_validated_parts(u64::MAX, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0);
        let Err(CdtError::CheckpointResumeFailed { failure }) = chain_counters(&stats) else {
            panic!("expected overflowing move statistics to fail");
        };

        assert_matches!(
            failure,
            CheckpointResumeFailure::MoveCounterOverflow {
                counter: CheckpointMoveCounter::Attempted
            }
        );
        let detail = failure.to_string();
        assert!(detail.contains("attempted move count exceeds u64::MAX"));
    }

    #[test]
    fn chain_counters_rejects_nonzero_hard_failures() {
        let stats = MoveStatistics::from_validated_parts(3, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        let Err(CdtError::CheckpointResumeFailed { failure }) = chain_counters(&stats) else {
            panic!("expected impossible hard-failure statistics to fail");
        };

        assert_matches!(
            failure,
            CheckpointResumeFailure::MoveHardFailures {
                move_type: MoveType::Move22
            }
        );
        let detail = failure.to_string();
        assert!(detail.contains("Move22"));
        assert!(detail.contains("hard-failure move count must be zero"));
    }

    #[test]
    fn explicit_cdt_volume_profiles_count_time_slabs() {
        let strip = CdtTriangulation::from_cdt_strip(4, 3).expect("create Delaunay strip");
        assert_eq!(
            strip
                .volume_profile()
                .expect("strip profile should be valid"),
            vec![6, 6, 0]
        );

        let torus = CdtTriangulation::from_toroidal_cdt(3, 3).expect("create periodic torus");
        assert_eq!(
            torus
                .volume_profile()
                .expect("torus profile should be valid"),
            vec![6, 6, 6]
        );
    }

    #[test]
    fn measurement_records_volume_profile_for_foliated_triangulation() {
        let triangulation = CdtTriangulation::from_cdt_strip(4, 3).expect("create Delaunay strip");
        let measurement =
            measurement_for(0, 1.0, &triangulation).expect("measurement should build");

        assert_eq!(measurement.volume_profile(), &[6, 6, 0]);
        assert_eq!(
            measurement.volume_profile().iter().sum::<u32>(),
            measurement.triangles().get()
        );
    }

    #[test]
    fn volume_profile_is_empty_without_current_foliation() {
        let triangulation =
            CdtTriangulation::from_seeded_points(5, 2, 2, 53).expect("create seeded triangulation");
        let measurement =
            measurement_for(0, 1.0, &triangulation).expect("measurement should build");

        assert!(!triangulation.has_foliation());
        assert!(
            triangulation
                .volume_profile()
                .expect("unfoliated triangulation should report an empty profile")
                .is_empty()
        );
        assert!(measurement.volume_profile().is_empty());
    }

    #[test]
    fn seeded_simulation_deterministic() {
        let run = |seed: u64| {
            let config = seeded_metropolis_config(1.0, 20, 5, 5, seed);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
            algorithm.run(tri).expect("seeded simulation should run")
        };

        let first = run(123);
        let second = run(123);

        assert_eq!(first.steps().len(), second.steps().len());
        for (first, second) in first.steps().iter().zip(second.steps().iter()) {
            assert_eq!(first.move_type(), second.move_type());
            assert_eq!(first.accepted(), second.accepted());
            assert_relative_eq!(first.action_before(), second.action_before());
            assert_optional_relative_eq(first.delta_action(), second.delta_action());
        }
    }

    #[test]
    fn delta_action_uses_count_deltas() {
        let action_config = ActionConfig::default();
        let before = SimplexCounts {
            vertices: 5,
            edges: 8,
            triangles: 4,
        };

        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move22)
                .expect("2,2 delta should be finite"),
            0.0
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::EdgeFlip)
                .expect("edge flip delta should be finite"),
            0.0
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move13Add)
                .expect("1,3 delta should be finite"),
            2.0 * CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT,
            epsilon = 1e-12
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move31Remove)
                .expect("3,1 delta should be finite"),
            -2.0 * CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT,
            epsilon = 1e-12
        );
    }

    #[test]
    fn delta_action_rejects_bad_counts() {
        let action_config = ActionConfig::default();

        assert_eq!(
            proposed_delta_action(
                &action_config,
                SimplexCounts {
                    vertices: 0,
                    edges: 2,
                    triangles: 1,
                },
                MoveType::Move31Remove,
            ),
            None
        );
        assert_eq!(
            proposed_delta_action(
                &action_config,
                SimplexCounts {
                    vertices: usize::MAX,
                    edges: 8,
                    triangles: 4,
                },
                MoveType::Move13Add,
            ),
            None
        );
    }

    #[test]
    fn accepts_non_positive_delta() {
        let mut rng = StdRng::seed_from_u64(7);
        assert!(metropolis_accept(0.0, 1.0, &mut rng));
        assert!(metropolis_accept(-1.0, 1.0, &mut rng));
    }

    #[test]
    fn concrete_plan_site_rejection_is_self_loop_telemetry() {
        let triangulation =
            CdtTriangulation::from_seeded_points(3, 1, 2, 53).expect("Failed to create");
        let action_config = ActionConfig::default();
        let counts_before = simplex_counts(&triangulation);
        let action_before = action_for(&action_config, &triangulation);
        let mut moves = ErgodicsSystem::with_seed(7);
        let mut proposal_stats = ProposalStatistics::new();

        let result = propose_concrete_plan(
            &triangulation,
            &mut moves,
            &mut proposal_stats,
            &action_config,
            MoveType::Move31Remove,
            action_before,
        )
        .expect("site rejection is an ordinary proposal outcome");

        assert!(result.is_none());
        assert_eq!(proposal_stats.move_family_proposals(), 1);
        assert_eq!(proposal_stats.no_site_proposals(), 1);
        assert_eq!(simplex_counts(&triangulation), counts_before);
        assert_relative_eq!(action_for(&action_config, &triangulation), action_before);
    }

    #[test]
    fn proposal_statistics_saturate_extreme_counters() {
        let mut stats = ProposalStatistics::from_validated_parts(
            u64::MAX,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
        );

        stats.record_move_family(2);
        stats.record_no_site();
        stats.record_site_rejection(&CdtProposalSiteRejection::CausalityViolation);
        stats.record_site_rejection(&CdtProposalSiteRejection::GeometricViolation);
        stats.record_site_rejection(&CdtProposalSiteRejection::Kernel(
            CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::Steps,
                provided_value: "0".to_string(),
                expected: "≥ 1".to_string(),
            },
        ));
        stats.record_metropolis_rejection();
        stats.record_accepted_transition();
        stats.record_hard_failure();

        assert_eq!(stats.move_family_proposals(), u64::MAX);
        assert_eq!(stats.observed_forward_sites(), u64::MAX);
        assert_eq!(stats.no_site_proposals(), u64::MAX);
        assert_eq!(stats.site_causality_rejections(), u64::MAX);
        assert_eq!(stats.site_geometric_rejections(), u64::MAX);
        assert_eq!(stats.site_backend_rejections(), u64::MAX);
        assert_eq!(stats.metropolis_rejections(), u64::MAX);
        assert_eq!(stats.accepted_transitions(), u64::MAX);
        assert_eq!(stats.hard_failures(), u64::MAX);
    }

    #[test]
    fn run_records_proposal_statistics_for_each_selected_move_family() {
        let config = seeded_metropolis_config(1.0, 12, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config.clone(), ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_seeded_points(3, 1, 2, 53).expect("Failed to create");

        let results = algorithm
            .run(triangulation)
            .expect("short run should finish");
        let proposal_stats = results.proposal_stats();
        let classified_proposals = proposal_stats.no_site_proposals()
            + proposal_stats.site_causality_rejections()
            + proposal_stats.site_geometric_rejections()
            + proposal_stats.site_backend_rejections()
            + proposal_stats.metropolis_rejections()
            + proposal_stats.accepted_transitions()
            + proposal_stats.hard_failures();

        assert_eq!(
            proposal_stats.move_family_proposals(),
            u64::from(config.steps().get())
        );
        assert_eq!(
            proposal_stats.move_family_proposals(),
            results.move_stats().total_attempted()
        );
        assert_eq!(
            proposal_stats.accepted_transitions(),
            results.move_stats().total_accepted()
        );
        assert_eq!(classified_proposals, proposal_stats.move_family_proposals());
    }

    #[test]
    fn accepted_planned_step_records_attempt_on_committed_state() {
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let committed = triangulation.clone();
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        let action = state.current_action;
        let move_type = MoveType::Move22;
        let info = CdtProposalInfo {
            move_type,
            action_before: action,
            action_after: Some(action),
            delta_action: Some(0.0),
        };
        let proposal_stats = ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0);

        record_planned_step_parts(
            &algorithm,
            &mut state,
            step_number(1),
            PlannedStepRecord {
                outcome: StepOutcome::Accepted,
                log_prob_after: Some(-action),
                info,
                proposal_stats: &proposal_stats,
                triangulation: &committed,
            },
        )
        .expect("accepted planned step should record telemetry");

        let history = state.triangulation.metadata().simulation_history();
        assert!(
            history.iter().any(|event| matches!(
                event,
                SimulationEvent::MoveAttempted {
                    move_type: recorded_move,
                    step
                } if *recorded_move == move_type && *step == 1
            )),
            "attempt event should remain on the committed triangulation"
        );
        assert!(
            history.iter().any(|event| matches!(
                event,
                SimulationEvent::MoveAccepted {
                    move_type: recorded_move,
                    step,
                    ..
                } if *recorded_move == move_type && *step == 1
            )),
            "accepted move event should remain on the committed triangulation"
        );
    }

    #[test]
    fn accepted_planned_step_reconstructs_missing_action_after_from_log_prob() {
        let temperature = 2.0;
        let config = seeded_metropolis_config(temperature, 1, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let committed = triangulation.clone();
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        let action_before = state.current_action;
        let reconstructed_action = action_before + 0.25;
        let move_type = MoveType::Move22;
        let proposal_stats = ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0);

        record_planned_step_parts(
            &algorithm,
            &mut state,
            step_number(1),
            PlannedStepRecord {
                outcome: StepOutcome::Accepted,
                log_prob_after: Some(-reconstructed_action / temperature),
                info: CdtProposalInfo {
                    move_type,
                    action_before,
                    action_after: None,
                    delta_action: None,
                },
                proposal_stats: &proposal_stats,
                triangulation: &committed,
            },
        )
        .expect("accepted planned step should record fallback action");

        assert_relative_eq!(state.current_action, reconstructed_action, epsilon = 1e-12);
        assert_relative_eq!(
            state.steps[0]
                .action_after()
                .expect("accepted step should record reconstructed action"),
            reconstructed_action,
            epsilon = 1e-12
        );
        assert_relative_eq!(
            state.steps[0]
                .delta_action()
                .expect("accepted step should record reconstructed action change"),
            reconstructed_action - action_before,
            epsilon = 1e-12
        );
    }

    #[test]
    fn accepted_planned_step_derives_delta_action_from_recorded_action_after() {
        let temperature = 2.0;
        let config = seeded_metropolis_config(temperature, 1, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let committed = triangulation.clone();
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        let action_before = state.current_action;
        let action_after = action_before + 0.5;
        let proposal_stats = ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0);

        record_planned_step_parts(
            &algorithm,
            &mut state,
            step_number(1),
            PlannedStepRecord {
                outcome: StepOutcome::Accepted,
                log_prob_after: None,
                info: CdtProposalInfo {
                    move_type: MoveType::Move22,
                    action_before,
                    action_after: Some(action_after),
                    delta_action: Some(999.0),
                },
                proposal_stats: &proposal_stats,
                triangulation: &committed,
            },
        )
        .expect("accepted planned step should record explicit action_after");

        assert_relative_eq!(
            state.steps[0]
                .delta_action()
                .expect("accepted step should record action change"),
            action_after - action_before,
            epsilon = 1e-12
        );
    }

    #[test]
    fn accepted_planned_step_rejects_missing_action_evidence_without_mutating_state() {
        let temperature = 2.0;
        let config = seeded_metropolis_config(temperature, 1, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let committed = triangulation.clone();
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        let action_before = state.current_action;
        let steps_before = state.steps.len();
        let measurements_before = state.measurements.len();
        let attempted_before = state.move_stats.total_attempted();
        let accepted_before = state.move_stats.total_accepted();
        let proposal_stats_before = state.proposal_stats.clone();
        let proposal_stats = ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0);

        let err = record_planned_step_parts(
            &algorithm,
            &mut state,
            step_number(23),
            PlannedStepRecord {
                outcome: StepOutcome::Accepted,
                log_prob_after: None,
                info: CdtProposalInfo {
                    move_type: MoveType::Move22,
                    action_before,
                    action_after: None,
                    delta_action: None,
                },
                proposal_stats: &proposal_stats,
                triangulation: &committed,
            },
        )
        .expect_err("accepted step without action evidence should be rejected");

        assert_matches!(err, CdtError::PlannedProposalTelemetryMissing { step: 23 });
        assert_relative_eq!(state.current_action, action_before, epsilon = 1e-12);
        assert_eq!(state.steps.len(), steps_before);
        assert_eq!(state.measurements.len(), measurements_before);
        assert_eq!(state.move_stats.total_attempted(), attempted_before);
        assert_eq!(state.move_stats.total_accepted(), accepted_before);
        assert_eq!(state.proposal_stats, proposal_stats_before);
    }

    #[test]
    fn accepted_planned_step_validation_failure_does_not_mutate_state() {
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("Delaunay torus should build");
        let mut invalid_candidate = triangulation.clone();
        invalid_candidate.set_delaunay_check_interval(NonZeroUsize::new(1));
        let vertex = invalid_candidate
            .geometry()
            .vertices()
            .find(|vertex| invalid_candidate.time_label(vertex) == Some(1))
            .expect("fixture has a slice-1 vertex");
        invalid_candidate
            .set_vertex_data(&vertex, Some(0))
            .expect("fixture vertex label can be edited");
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        let triangulation_before = state.triangulation.clone();
        let action_before = state.current_action;
        let steps_before = state.steps.len();
        let measurements_before = state.measurements.len();
        let attempted_before = state.move_stats.total_attempted();
        let accepted_before = state.move_stats.total_accepted();
        let proposal_stats_before = state.proposal_stats.clone();
        let proposal_stats = ProposalStatistics::from_validated_parts(1, 0, 0, 0, 0, 0, 0, 0, 0);

        let error = record_planned_step_parts(
            &algorithm,
            &mut state,
            step_number(1),
            PlannedStepRecord {
                outcome: StepOutcome::Accepted,
                log_prob_after: Some(-action_before),
                info: CdtProposalInfo {
                    move_type: MoveType::Move22,
                    action_before,
                    action_after: Some(action_before),
                    delta_action: Some(0.0),
                },
                proposal_stats: &proposal_stats,
                triangulation: &invalid_candidate,
            },
        )
        .expect_err("accepted invalid triangulation should fail staged validation");

        assert_matches!(
            error,
            CdtError::Foliation(FoliationError::StaleBookkeeping { .. })
        );
        assert_canonical_triangulations_match(&state.triangulation, &triangulation_before);
        assert_relative_eq!(state.current_action, action_before, epsilon = 1e-12);
        assert_eq!(state.steps.len(), steps_before);
        assert_eq!(state.measurements.len(), measurements_before);
        assert_eq!(state.move_stats.total_attempted(), attempted_before);
        assert_eq!(state.move_stats.total_accepted(), accepted_before);
        assert_eq!(state.proposal_stats, proposal_stats_before);
    }

    #[test]
    fn run_steps_rejects_step_count_overflow_before_planned_sampler_step() {
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 2);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let mut state = algorithm
            .initial_state(triangulation)
            .expect("initial state should build");
        state.current_step = u32::MAX;
        let steps_before = state.steps.len();
        let measurements_before = state.measurements.len();
        let attempted_before = state.move_stats.total_attempted();
        let accepted_before = state.move_stats.total_accepted();
        let proposal_stats_before = state.proposal_stats.clone();

        let err = algorithm
            .run_steps(
                &mut state,
                NonZeroU32::new(1).expect("one additional step is nonzero"),
            )
            .expect_err("overflowing step counter should reject continuation");

        assert_matches!(
            err,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::StepCountOverflow
            }
        );
        assert_eq!(state.current_step, u32::MAX);
        assert_eq!(state.steps.len(), steps_before);
        assert_eq!(state.measurements.len(), measurements_before);
        assert_eq!(state.move_stats.total_attempted(), attempted_before);
        assert_eq!(state.move_stats.total_accepted(), accepted_before);
        assert_eq!(state.proposal_stats, proposal_stats_before);
    }

    #[test]
    fn planned_step_error_preserves_upstream_mcmc_error() {
        assert_matches!(
            planned_step_error(23, DelayedStepError::Mcmc(McmcError::NanLogQRatio)),
            CdtError::Mcmc(McmcError::NanLogQRatio)
        );
    }

    #[test]
    fn planned_step_error_maps_proposal_failures_to_accepted_move_error() {
        fn proposal_error() -> CdtProposalError {
            CdtProposalError::ApplicationFailed {
                move_type: MoveType::Move31Remove,
                attempt: 3,
                source: CdtError::BackendMutationFailed {
                    operation: BackendMutationOperation::RemoveVertex,
                    target: "vertex VertexKey(7v1)".to_string(),
                    detail: "backend rejected removal".to_string(),
                },
            }
        }

        let cases = [
            DelayedStepError::Plan(proposal_error()),
            DelayedStepError::ProposedLogProb(proposal_error()),
            DelayedStepError::LogQRatio(proposal_error()),
            DelayedStepError::Commit(proposal_error()),
        ];

        for error in cases {
            assert_matches!(
                planned_step_error(23, error),
                CdtError::MetropolisMoveApplicationFailed {
                    step: 23,
                    move_type: MoveType::Move31Remove,
                    attempts: 3,
                    source: MetropolisMoveApplicationFailure::BackendMutation {
                        operation: BackendMutationOperation::RemoveVertex,
                        ..
                    }
                }
            );
        }
    }

    #[test]
    fn missing_planned_step_info_reports_step_context() {
        assert_matches!(
            missing_planned_step_info(23),
            CdtError::PlannedProposalTelemetryMissing { step: 23 }
        );
    }

    #[test]
    fn run_rejects_zero_frequency() {
        let err = MetropolisConfig::new(1.0, 10, 2, 0).expect_err("zero cadence is invalid");
        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, ConfigurationSetting::MeasurementFrequency);
                assert_eq!(provided_value, "0");
                assert_eq!(expected, "≥ 1");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_bad_temperature() {
        for bad_temp in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let err =
                MetropolisConfig::new(bad_temp, 10, 2, 2).expect_err("bad temperature is invalid");
            match err {
                CdtError::InvalidSimulationConfiguration {
                    setting, expected, ..
                } => {
                    assert_eq!(setting, ConfigurationSetting::Temperature, "T={bad_temp}");
                    assert_eq!(expected, "finite and positive", "T={bad_temp}");
                }
                other => panic!(
                    "Expected InvalidSimulationConfiguration for T={bad_temp}, got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn validate_requires_measurement() {
        let err = MetropolisConfig::new(1.0, 19, 15, 10).expect_err(
            "Configuration should require at least one post-thermalization measurement",
        );

        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, ConfigurationSetting::MeasurementSchedule);
                assert!(
                    provided_value.contains("steps=19")
                        && provided_value.contains("thermalization_steps=15")
                        && provided_value.contains("measurement_frequency=10"),
                    "Unexpected provided value: {provided_value}"
                );
                assert_eq!(expected, "at least one post-thermalization measurement");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_overflow() {
        let err = MetropolisConfig::new(1.0, u32::MAX, u32::MAX, 2)
            .expect_err("unreachable post-thermalization measurement should be rejected");

        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, ConfigurationSetting::MeasurementSchedule);
                assert!(
                    provided_value.contains("steps=4294967295")
                        && provided_value.contains("thermalization_steps=4294967295")
                        && provided_value.contains("measurement_frequency=2"),
                    "Unexpected provided value: {provided_value}"
                );
                assert_eq!(expected, "at least one post-thermalization measurement");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn run_accepts_boundary_schedule() {
        let config = seeded_metropolis_config(1.0, 20, 15, 10, 42);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let results = algorithm.run(tri).expect("valid schedule should run");
        assert_eq!(results.steps().len(), 20);
        assert!(
            results
                .measurements()
                .iter()
                .any(|measurement| measurement.step() >= 15)
        );
    }

    #[test]
    fn run_validates_action_config() {
        let err = ActionConfig::new(f64::INFINITY, 1.0, 0.1)
            .expect_err("invalid action config should be rejected before simulation");
        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, ConfigurationSetting::Coupling0);
                assert_eq!(provided_value, "inf");
                assert_eq!(expected, "finite");
            }
            other => panic!("Expected InvalidConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn cdt_target_rejects_invalid_temperature() {
        for temperature in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let Err(err) = CdtTarget::new(ActionConfig::default(), temperature) else {
                panic!("temperature {temperature:?} should be rejected");
            };

            match err {
                CdtError::InvalidSimulationConfiguration {
                    setting,
                    provided_value: _,
                    expected,
                } => {
                    assert_eq!(setting, ConfigurationSetting::Temperature);
                    assert_eq!(expected, "finite and positive");
                }
                other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
            }
        }
    }

    #[test]
    fn cdt_target_rejects_invalid_action_config() {
        let err = ActionConfig::new(f64::NAN, 1.0, 0.0)
            .expect_err("invalid action config should be rejected");

        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value: _,
                expected,
            } => {
                assert_eq!(setting, ConfigurationSetting::Coupling0);
                assert_eq!(expected, "finite");
            }
            other => panic!("Expected InvalidConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn cdt_proposal_rejects_invalid_action_config() {
        let err = ActionConfig::new(1.0, f64::NEG_INFINITY, 0.0)
            .expect_err("invalid action config should be rejected");

        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value: _,
                expected,
            } => {
                assert_eq!(setting, ConfigurationSetting::Coupling2);
                assert_eq!(expected, "finite");
            }
            other => panic!("Expected InvalidConfiguration, got {other:?}"),
        }
        assert!(ActionConfig::new(1.0, f64::NEG_INFINITY, 0.0).is_err());
    }

    #[test]
    fn unseeded_config_uses_random_rng() {
        let config = metropolis_config(1.0, 5, 1, 1); // no seed
        assert!(config.seed().is_none());

        let mut rng = simulation_rng(config.seed());
        let draw = rng.random::<f64>();
        assert!((0.0..1.0).contains(&draw));
    }

    #[test]
    fn cdt_proposal_scores_planned_proposal() {
        let action_config = ActionConfig::default();
        let target =
            CdtTarget::new(action_config.clone(), 1.0).expect("valid target configuration");
        let mut proposal = CdtProposal::with_seed(action_config, 7);
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
        let mut rng = StdRng::seed_from_u64(7);

        let plan = proposal
            .propose_plan(&triangulation, &mut rng)
            .expect("planning should not fail")
            .expect("CDT proposals always select a move type");
        let info = proposal.info(&plan);
        let proposed_log_prob = proposal
            .proposed_log_prob(&triangulation, &plan, &target)
            .expect("scoring should not fail");

        assert_eq!(info.move_type, plan.move_type());
        assert_relative_eq!(
            info.delta_action
                .expect("concrete plan info should include action delta"),
            plan.delta_action(),
            epsilon = 1e-12
        );
        assert_relative_eq!(proposed_log_prob, -plan.action_after(), epsilon = 1e-12);
    }

    #[test]
    fn cdt_proposal_log_q_ratio_uses_forward_and_reverse_site_counts() {
        let action_config = ActionConfig::default();
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("toroidal CDT should build");
        let mut moves = ErgodicsSystem::with_seed(19);
        let mut proposal_stats = ProposalStatistics::new();
        let action_before = action_for(&action_config, &triangulation);
        let plan = propose_concrete_plan(
            &triangulation,
            &mut moves,
            &mut proposal_stats,
            &action_config,
            MoveType::Move13Add,
            action_before,
        )
        .expect("planning should not hard-fail")
        .expect("toroidal triangulation should have a volume-add proposal");

        let forward_sites = proposal_site_count(&triangulation, MoveType::Move13Add);
        let reverse_sites = proposal_site_count(&plan.proposed_state, MoveType::Move31Remove);
        assert!(forward_sites > 0);
        assert!(reverse_sites > 0);

        let expected = DiscreteProposalRatio::from_counts(forward_sites, reverse_sites)
            .expect("positive forward proposal sites should build a ratio")
            .log_q_ratio();
        assert_relative_eq!(
            concrete_log_q_ratio(&triangulation, &plan),
            expected,
            epsilon = 1e-12
        );

        let proposal = CdtProposal::new(action_config);
        assert_relative_eq!(
            proposal
                .log_q_ratio(&triangulation, &plan)
                .expect("proposal-ratio scoring should not fail"),
            expected,
            epsilon = 1e-12
        );
    }

    #[test]
    fn concrete_plan_does_not_mutate_ergodics_move_stats() {
        let action_config = ActionConfig::default();
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("toroidal CDT should build");
        let mut moves = ErgodicsSystem::with_seed(19);
        let mut proposal_stats = ProposalStatistics::new();
        let action_before = action_for(&action_config, &triangulation);

        let _plan = propose_concrete_plan(
            &triangulation,
            &mut moves,
            &mut proposal_stats,
            &action_config,
            MoveType::Move13Add,
            action_before,
        )
        .expect("planning should not hard-fail")
        .expect("toroidal triangulation should have a volume-add proposal");

        assert_eq!(moves.stats().total_attempted(), 0);
        assert_eq!(moves.stats().total_accepted(), 0);
        assert_eq!(moves.stats().total_hard_failures(), 0);
    }

    #[test]
    fn cdt_proposal_scores_concrete_plan_from_proposed_state() {
        let action_config = ActionConfig::default();
        let target =
            CdtTarget::new(action_config.clone(), 1.0).expect("valid target configuration");
        let proposal = CdtProposal::with_seed(action_config, 7);
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
        let action_before = action_for(&ActionConfig::default(), &triangulation);
        let plan = CdtProposalPlan {
            move_type: MoveType::Move31Remove,
            action_before,
            action_after: action_before,
            delta_action: 0.0,
            forward_site_count: 0,
            reverse_site_count: 0,
            proposed_state: triangulation.clone(),
        };

        let proposed_log_prob = proposal
            .proposed_log_prob(&triangulation, &plan, &target)
            .expect("scoring a concrete plan should not fail");

        assert_relative_eq!(proposed_log_prob, -action_before, epsilon = 1e-12);
    }

    #[test]
    fn cdt_proposal_uses_planned_sampler_path() {
        let action_config = ActionConfig::default();
        let target =
            CdtTarget::new(action_config.clone(), 1.0).expect("valid target configuration");
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
        let mut chain = Chain::new(triangulation, &target)
            .expect("initial state should have finite log probability");
        let mut proposal = CdtProposal::with_seed(action_config, 7);
        let mut rng = StdRng::seed_from_u64(11);

        let step = chain
            .step_delayed(&target, &mut proposal, &mut rng)
            .expect("ordinary no-site outcomes must be planned-step rejections, not errors");

        let info = step
            .info
            .expect("planned CDT steps should report proposal info");
        assert_eq!(proposal.last_step_info(), Some(info));
        assert_eq!(proposal.last_proposal_stats().move_family_proposals(), 1);
        assert_eq!(proposal.last_proposal_stats().accepted_transitions(), 0);
        assert_eq!(proposal.last_proposal_stats().metropolis_rejections(), 0);
        assert!(!step.outcome.is_accepted() || step.log_prob_after.is_some());
    }

    #[test]
    fn cdt_proposal_commit_applies_concrete_planned_state() {
        let action_config = ActionConfig::default();
        let mut proposal = CdtProposal::with_seed(action_config.clone(), 11);
        let mut triangulation =
            CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed to create");
        let proposed_state =
            CdtTriangulation::from_seeded_points(6, 1, 2, 59).expect("Failed to create proposal");
        let proposed_counts = simplex_counts(&proposed_state);
        let action_before = action_for(&action_config, &triangulation);
        let action_after = action_for(&action_config, &proposed_state);
        let plan = CdtProposalPlan {
            move_type: MoveType::Move13Add,
            action_before,
            action_after,
            delta_action: action_after - action_before,
            forward_site_count: proposal_site_count(&triangulation, MoveType::Move13Add),
            reverse_site_count: proposal_site_count(&proposed_state, MoveType::Move31Remove),
            proposed_state,
        };
        let mut rng = StdRng::seed_from_u64(11);

        proposal
            .commit(&mut triangulation, plan, &mut rng)
            .expect("committing a concrete plan should swap in the planned state");

        assert_eq!(simplex_counts(&triangulation), proposed_counts);
        assert_relative_eq!(action_for(&action_config, &triangulation), action_after);
    }

    #[test]
    fn cdt_proposal_error_preserves_typed_sources() {
        let source = CdtError::BackendMutationFailed {
            operation: BackendMutationOperation::SetVertexDataByKey,
            target: "vertex VertexKey(7)".to_string(),
            detail: "missing vertex".to_string(),
        };
        let err = CdtProposalError::ApplicationFailed {
            move_type: MoveType::Move13Add,
            attempt: 2,
            source: source.clone(),
        };

        assert_eq!(
            Error::source(&err).map(ToString::to_string),
            Some(source.to_string())
        );
        assert!(err.to_string().contains("Move13Add"));
        assert!(err.to_string().contains("attempt 2"));

        let cdt_error = CdtError::from(err);
        assert_matches!(
            cdt_error,
            CdtError::ProposalApplicationFailed {
                move_type: MoveType::Move13Add,
                attempt: 2,
                source: MetropolisMoveApplicationFailure::BackendMutation {
                    operation: BackendMutationOperation::SetVertexDataByKey,
                    ..
                },
            }
        );

        let site_rejection = CdtProposalSiteRejection::Kernel(source.clone());
        assert_eq!(
            Error::source(&site_rejection).map(ToString::to_string),
            Some(source.to_string())
        );
    }
}
