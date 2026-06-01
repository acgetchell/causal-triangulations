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

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveStatistics, MoveType};
use crate::cdt::results::{Measurement, SimulationResultsBackend};
use crate::cdt::triangulation::SimulationEvent;
use crate::errors::{
    CdtError, CdtResult, CheckpointResumeFailure, ConfigurationSetting,
    MetropolisMoveApplicationFailure,
};
use crate::geometry::CdtTriangulation2D;
use rand::{Rng, RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use super::adapter::{
    CdtTarget, concrete_log_q_ratio, propose_concrete_plan, restore_checkpoint_state,
};
use super::checkpoint::{
    CdtMcmcCheckpoint, CdtMcmcCheckpointParts, chain_counters, checkpoint_resume_failed,
    validate_checkpoint_counters, validate_resume_compatible,
};
use super::helpers::{
    action_for, actions_match, measurement_for, measurement_is_due, validate_metropolis_schedule,
};
use super::telemetry::{MonteCarloStep, ProposalStatistics};
use std::num::NonZeroU32;

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
    acceptance_rng: Xoshiro256PlusPlus,
    ergodics: ErgodicsSystem,
    move_stats: MoveStatistics,
    proposal_stats: ProposalStatistics,
    steps: Vec<MonteCarloStep>,
    measurements: Vec<Measurement>,
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
    /// Each step proposes a move type, computes the Metropolis-Hastings
    /// acceptance probability from the move's simplex-count delta, and only
    /// mutates the triangulation when that proposal is accepted. Accepted moves
    /// that fail during backend application are rolled back and retried at
    /// another randomly selected local site.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
    /// configuration is invalid, [`CdtError::InvalidConfiguration`] if the
    /// action configuration is invalid,
    /// [`CdtError::MetropolisMoveApplicationFailed`] if an accepted move causes
    /// a hard backend mutation failure, or a validation error for
    /// unrecoverable triangulation failures. Accepted move types that cannot
    /// find a realizable local site after bounded retries are recorded as
    /// rejected proposals.
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
    /// a hard backend mutation failure, or a validation error for
    /// unrecoverable triangulation failures.
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
    ///     assert_eq!(checkpoint.current_step(), 2);
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
    /// [`ChainCheckpoint`](markov_chain_monte_carlo::ChainCheckpoint) and stores CDT-specific
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
    /// a hard backend mutation failure, or a validation error for
    /// unrecoverable triangulation failures.
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
    ///     assert_eq!(checkpoint.current_step(), 2);
    ///     Ok(())
    /// }
    /// ```
    pub fn run_to_checkpoint(
        &self,
        triangulation: CdtTriangulation2D,
    ) -> CdtResult<CdtMcmcCheckpoint> {
        self.config.validate();
        self.action_config.validate();

        let mut state = self.initial_state(triangulation);
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
    /// [`CdtError::MetropolisMoveApplicationFailed`] or validation errors for
    /// failures during resumed sampling.
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
    /// [`CdtError::MetropolisMoveApplicationFailed`] or validation errors for
    /// failures during resumed sampling.
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
    ///     assert_eq!(checkpoint.current_step(), 5);
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
            .checked_add(self.config.steps.get())
            .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::StepCountOverflow))?;
        result_config.steps = NonZeroU32::new(steps)
            .ok_or_else(|| checkpoint_resume_failed(CheckpointResumeFailure::StepCountOverflow))?;

        let mut state = MetropolisRunState::from_checkpoint(checkpoint)?;
        self.run_steps(&mut state, self.config.steps)?;
        state.into_checkpoint(result_config, self.action_config.clone())
    }

    fn initial_state(&self, mut triangulation: CdtTriangulation2D) -> MetropolisRunState {
        let current_action = action_for(&self.action_config, &triangulation);
        let mut measurements = Vec::new();
        if measurement_is_due(
            0,
            self.config.thermalization_steps(),
            self.config.measurement_frequency(),
        ) {
            measurements.push(measurement_for(0, current_action, &triangulation));
            triangulation.record_event(SimulationEvent::MeasurementTaken {
                step: 0,
                action: current_action,
            });
        }

        MetropolisRunState {
            triangulation,
            current_step: 0,
            current_action,
            acceptance_rng: simulation_rng(self.config.seed),
            ergodics: self.config.seed.map_or_else(ErgodicsSystem::new, |seed| {
                ErgodicsSystem::with_seed(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
            }),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: Vec::new(),
            measurements,
            elapsed_time: Duration::ZERO,
        }
    }

    fn run_steps(
        &self,
        state: &mut MetropolisRunState,
        additional_steps: NonZeroU32,
    ) -> CdtResult<()> {
        let start = Instant::now();
        for _ in 0..additional_steps.get() {
            let step = state.current_step.checked_add(1).ok_or_else(|| {
                checkpoint_resume_failed(CheckpointResumeFailure::StepCountOverflow)
            })?;
            run_one_step(self, state, step)?;
            state.current_step = step;
        }
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
        let target = CdtTarget::new(
            checkpoint.action_config.clone(),
            checkpoint.config.temperature(),
        )?;
        let triangulation = restore_checkpoint_state(checkpoint.chain, &target)?;
        triangulation.validate_evolved_cdt()?;
        let actual_action = action_for(&checkpoint.action_config, &triangulation);
        if !actions_match(actual_action, checkpoint.current_action) {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::ActionMismatch {
                    stored: checkpoint.current_action,
                    recomputed: actual_action,
                },
            ));
        }

        Ok(Self {
            triangulation,
            current_step: checkpoint.current_step,
            current_action: checkpoint.current_action,
            acceptance_rng: checkpoint.acceptance_rng,
            ergodics: checkpoint.ergodics,
            move_stats: checkpoint.move_stats,
            proposal_stats: checkpoint.proposal_stats,
            steps: checkpoint.steps,
            measurements: checkpoint.measurements,
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
        CdtMcmcCheckpoint::from_parts(CdtMcmcCheckpointParts {
            triangulation: self.triangulation,
            accepted,
            rejected,
            config,
            action_config,
            current_step: self.current_step,
            current_action: self.current_action,
            move_stats: self.move_stats,
            proposal_stats: self.proposal_stats,
            steps: self.steps,
            measurements: self.measurements,
            elapsed_time: self.elapsed_time,
            acceptance_rng: self.acceptance_rng,
            ergodics: self.ergodics,
        })
    }
}

/// Executes one additional Metropolis step against an initialized run state.
///
/// Fresh and resumed simulations use this shared path so checkpoint
/// continuation cannot drift from ordinary sampling behavior.
fn run_one_step(
    algorithm: &MetropolisAlgorithm,
    state: &mut MetropolisRunState,
    step: u32,
) -> CdtResult<()> {
    let move_type = state.ergodics.select_random_move();
    state.move_stats.record_attempt(move_type);
    state
        .triangulation
        .record_event(SimulationEvent::MoveAttempted {
            move_type,
            step: step.into(),
        });

    let action_before = state.current_action;
    // A selected move family is not yet a concrete proposal; self-loop outcomes
    // such as no usable site or a rejected sampled site must not report a ΔS.
    let mut delta_action = None;

    let mut accepted = false;
    let mut action_after = None;

    let plan = match propose_concrete_plan(
        &state.triangulation,
        &mut state.ergodics,
        &mut state.proposal_stats,
        &algorithm.action_config,
        move_type,
        action_before,
    ) {
        Ok(plan) => plan,
        Err(err) => {
            state.move_stats.record_hard_failure(move_type);
            state.proposal_stats.record_hard_failure();
            return Err(accepted_move_error(
                step,
                move_type,
                err.attempt,
                err.source,
            ));
        }
    };

    if let Some(plan) = plan {
        delta_action = plan.delta_action;
        let log_alpha = -(plan.action_after.expect("planned moves have actions") - action_before)
            / algorithm.config.temperature()
            + concrete_log_q_ratio(&state.triangulation, &plan);

        if metropolis_accept_log_alpha(log_alpha, &mut state.acceptance_rng) {
            let applied_action = plan.action_after.expect("planned moves have actions");
            state.triangulation = plan.proposed_state;
            accepted = true;
            action_after = Some(applied_action);
            state.current_action = applied_action;
            state.move_stats.record_success(move_type);
            state.proposal_stats.record_accepted_transition();
            state
                .triangulation
                .record_event(SimulationEvent::MoveAccepted {
                    move_type,
                    step: step.into(),
                    action_change: applied_action - action_before,
                });
            validate_evolved_cdt_if_due(state)?;
        } else {
            state.proposal_stats.record_metropolis_rejection();
        }
    }

    state.steps.push(MonteCarloStep {
        step,
        move_type,
        accepted,
        action_before,
        action_after,
        delta_action,
    });

    if measurement_is_due(
        step,
        algorithm.config.thermalization_steps(),
        algorithm.config.measurement_frequency(),
    ) {
        state.measurements.push(measurement_for(
            step,
            state.current_action,
            &state.triangulation,
        ));
        state
            .triangulation
            .record_event(SimulationEvent::MeasurementTaken {
                step: step.into(),
                action: state.current_action,
            });
    }

    Ok(())
}

/// Runs the expensive full evolved-state validation only when the backend policy is due.
fn validate_evolved_cdt_if_due(state: &MetropolisRunState) -> CdtResult<()> {
    if state
        .triangulation
        .geometry()
        .should_check_delaunay_after(state.move_stats.total_accepted())
    {
        state.triangulation.validate_evolved_cdt()?;
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

/// Applies the Metropolis acceptance rule to a proposed action change.
///
/// Factoring this out keeps the probability rule isolated from move selection
/// and makes deterministic unit tests possible with a seeded RNG.
#[cfg(test)]
fn metropolis_accept<R: Rng + ?Sized>(delta_action: f64, temperature: f64, rng: &mut R) -> bool {
    metropolis_accept_log_alpha(-delta_action / temperature, rng)
}

fn metropolis_accept_log_alpha<R: Rng + ?Sized>(log_alpha: f64, rng: &mut R) -> bool {
    log_alpha >= 0.0 || rng.random::<f64>() < log_alpha.exp()
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
        CdtProposal, CdtProposalError, CdtProposalPlan, site_count_to_f64,
    };
    use super::super::helpers::{SimplexCounts, proposed_delta_action, simplex_counts};
    use super::super::telemetry::CdtProposalSiteRejection;
    use super::*;
    use crate::cdt::action::CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT;
    use crate::cdt::ergodic_moves::proposal_site_count;
    use crate::cdt::triangulation::CdtTriangulation;
    use crate::errors::{BackendMutationOperation, CheckpointMoveCounter, ConfigurationSetting};
    use crate::geometry::traits::TriangulationQuery;
    use approx::assert_relative_eq;
    use markov_chain_monte_carlo::{Chain, DelayedProposal, Target};
    use rand::rngs::StdRng;
    use serde_json::{from_str, to_string, to_value};
    use std::assert_matches;
    use std::error::Error;
    use std::num::NonZeroUsize;

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
        assert_eq!(left.volume_profile(), right.volume_profile());
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
        let mut state = algorithm.initial_state(triangulation);
        let move_type = state.ergodics.select_random_move();
        let _acceptance_rng_marker: f64 = state.acceptance_rng.random();

        state.move_stats.record_attempt(move_type);
        state
            .triangulation
            .record_event(SimulationEvent::MoveAttempted { move_type, step: 1 });
        state.steps.push(MonteCarloStep {
            step: 1,
            move_type,
            accepted: false,
            action_before: state.current_action,
            action_after: None,
            delta_action: proposed_delta_action(
                &action_config,
                simplex_counts(&state.triangulation),
                move_type,
            ),
        });
        state.current_step = 1;
        state.proposal_stats.record_move_family(1);
        state.proposal_stats.record_metropolis_rejection();
        state.measurements.push(measurement_for(
            1,
            state.current_action,
            &state.triangulation,
        ));
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

    fn synthetic_one_step_checkpoint(accepted: bool) -> CdtMcmcCheckpoint {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let config = seeded_metropolis_config(1.0, 1, 0, 1, 13);
        let action_config = ActionConfig::default();
        let current_action = action_for(&action_config, &triangulation);
        let move_type = MoveType::Move22;
        let mut move_stats = MoveStatistics::new();
        move_stats.record_attempt(move_type);
        if accepted {
            move_stats.record_success(move_type);
        }

        CdtMcmcCheckpoint::from_parts(CdtMcmcCheckpointParts {
            triangulation: triangulation.clone(),
            accepted: usize::from(accepted),
            rejected: usize::from(!accepted),
            config,
            action_config,
            current_step: 1,
            current_action,
            move_stats,
            proposal_stats: if accepted {
                ProposalStatistics::from_validated_parts(1, 1, 0, 0, 0, 0, 0, 1, 0)
            } else {
                ProposalStatistics::from_validated_parts(1, 1, 0, 0, 0, 0, 1, 0, 0)
            },
            steps: vec![MonteCarloStep {
                step: 1,
                move_type,
                accepted,
                action_before: current_action,
                action_after: accepted.then_some(current_action),
                delta_action: accepted.then_some(0.0),
            }],
            measurements: vec![
                measurement_for(0, current_action, &triangulation),
                measurement_for(1, current_action, &triangulation),
            ],
            elapsed_time: Duration::ZERO,
            acceptance_rng: simulation_rng(Some(1)),
            ergodics: ErgodicsSystem::with_seed(2),
        })
        .expect("synthetic checkpoint should validate")
    }

    fn empty_run_state(triangulation: CdtTriangulation2D) -> MetropolisRunState {
        MetropolisRunState {
            triangulation,
            current_step: 0,
            current_action: 0.0,
            acceptance_rng: simulation_rng(Some(1)),
            ergodics: ErgodicsSystem::with_seed(2),
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: Vec::new(),
            measurements: Vec::new(),
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
            u32::try_from(geometry.vertex_count()).unwrap_or_default(),
            u32::try_from(geometry.edge_count()).unwrap_or_default(),
            u32::try_from(geometry.face_count()).unwrap_or_default(),
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
            u32::try_from(g.vertex_count()).unwrap_or_default(),
            u32::try_from(g.edge_count()).unwrap_or_default(),
            u32::try_from(g.face_count()).unwrap_or_default(),
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
            measurement.action.is_finite()
                && measurement.vertices > 0
                && measurement.edges > 0
                && measurement.triangles > 0
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
            .map(|measurement| measurement.step)
            .collect::<Vec<_>>();

        assert_eq!(measurement_steps, vec![2, 4]);
        assert!(
            results
                .measurements()
                .iter()
                .all(|measurement| measurement.step >= results.config().thermalization_steps())
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

        assert_eq!(checkpoint.current_step(), 3);
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
        let current_step =
            usize::try_from(checkpoint.current_step()).expect("u32 step count should fit usize");
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
        assert_eq!(checkpoint.config().steps().get(), checkpoint.current_step());
        assert_eq!(checkpoint.action_config(), &ActionConfig::default());
        assert!(checkpoint.current_action().is_finite());
        assert_eq!(
            checkpoint.move_stats().total_attempted(),
            u64::from(checkpoint.current_step())
        );
        assert_eq!(
            checkpoint.proposal_stats().move_family_proposals(),
            u64::from(checkpoint.current_step())
        );
        assert_eq!(last_step.step, checkpoint.current_step());
        assert_eq!(last_measurement.step, checkpoint.current_step());
        assert_relative_eq!(
            last_measurement.action,
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
        assert_eq!(first_resumed.steps()[1].step, 2);
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
            one_shot.triangulation().volume_profile(),
            chunked.triangulation().volume_profile()
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
        checkpoint.current_step += 1;
        let chain_steps = checkpoint.chain.total_steps();
        let checkpoint_step = checkpoint.current_step;
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
        checkpoint.steps[0].step = 2;
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
        let mut checkpoint = short_checkpoint();
        if let Some(step) = checkpoint.steps.iter_mut().find(|step| step.accepted) {
            step.accepted = false;
            step.action_after = None;
        } else {
            let step = &mut checkpoint.steps[0];
            step.accepted = true;
            step.delta_action = Some(0.0);
            step.action_after = Some(step.action_before);
        }
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
    fn resume_rejects_nonfinite_step_action_before() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0].action_before = f64::NAN;

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::NonFiniteStepActionBefore { step: 1 }
                )
            },
            "non-finite action_before",
        );
    }

    #[test]
    fn resume_rejects_nonfinite_step_delta_action() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0].delta_action = Some(f64::NAN);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::NonFiniteStepDeltaAction { step: 1 }
                )
            },
            "non-finite delta_action",
        );
    }

    #[test]
    fn resume_rejects_accepted_step_missing_delta_action() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0].delta_action = None;

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::AcceptedStepMissingDeltaAction { step: 1 }
                )
            },
            "missing delta_action",
        );
    }

    #[test]
    fn resume_rejects_action_after_delta_mismatch() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0].action_after = Some(checkpoint.steps[0].action_before + 1.0);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::StepActionAfterDeltaMismatch { step: 1 }
                )
            },
            "action_after does not match",
        );
    }

    #[test]
    fn resume_rejects_nonfinite_step_action_after() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0].action_after = Some(f64::NAN);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::NonFiniteStepActionAfter { step: 1 }
                )
            },
            "non-finite action_after",
        );
    }

    #[test]
    fn resume_rejects_accepted_step_missing_action_after() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.steps[0].action_after = None;

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::AcceptedStepMissingActionAfter { step: 1 }
                )
            },
            "missing action_after",
        );
    }

    #[test]
    fn resume_rejects_rejected_step_with_action_after() {
        let mut checkpoint = synthetic_one_step_checkpoint(false);
        checkpoint.steps[0].action_after = Some(checkpoint.steps[0].action_before);

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::RejectedStepHasActionAfter { step: 1 }
                )
            },
            "unexpectedly has action_after",
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
        checkpoint.measurements[1].step = 2;

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
    fn resume_rejects_nonfinite_measurement_action() {
        let mut checkpoint = synthetic_one_step_checkpoint(true);
        checkpoint.measurements[1].action = f64::NAN;

        assert_checkpoint_resume_failed(
            validate_checkpoint_counters(&checkpoint),
            |failure| {
                matches!(
                    failure,
                    CheckpointResumeFailure::NonFiniteMeasurementAction { step: 1 }
                )
            },
            "non-finite action",
        );
    }

    #[test]
    fn resume_rejects_checkpoint_action_mismatch() {
        let mut checkpoint = short_checkpoint();
        checkpoint.current_action += 1.0;
        let algorithm = MetropolisAlgorithm::new(
            seeded_metropolis_config(1.0, 2, 0, 1, 999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
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
        assert_eq!(strip.volume_profile(), vec![6, 6, 0]);

        let torus = CdtTriangulation::from_toroidal_cdt(3, 3).expect("create periodic torus");
        assert_eq!(torus.volume_profile(), vec![6, 6, 6]);
    }

    #[test]
    fn measurement_records_volume_profile_for_foliated_triangulation() {
        let triangulation = CdtTriangulation::from_cdt_strip(4, 3).expect("create Delaunay strip");
        let measurement = measurement_for(0, 1.0, &triangulation);

        assert_eq!(measurement.volume_profile, vec![6, 6, 0]);
        assert_eq!(
            measurement.volume_profile.iter().sum::<u32>(),
            measurement.triangles
        );
    }

    #[test]
    fn volume_profile_is_empty_without_current_foliation() {
        let triangulation =
            CdtTriangulation::from_seeded_points(5, 2, 2, 53).expect("create seeded triangulation");
        let measurement = measurement_for(0, 1.0, &triangulation);

        assert!(!triangulation.has_foliation());
        assert!(triangulation.volume_profile().is_empty());
        assert!(measurement.volume_profile.is_empty());
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
            assert_eq!(first.move_type, second.move_type);
            assert_eq!(first.accepted, second.accepted);
            assert_relative_eq!(first.action_before, second.action_before);
            assert_optional_relative_eq(first.delta_action, second.delta_action);
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
                    vertices: u32::MAX,
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
                .any(|measurement| measurement.step >= 15)
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
    fn cdt_proposal_scores_delayed_plan() {
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
        assert_optional_relative_eq(info.delta_action, plan.delta_action());
        if let Some(action_after) = plan.action_after() {
            assert_relative_eq!(proposed_log_prob, -action_after, epsilon = 1e-12);
        } else {
            assert!(proposed_log_prob.is_infinite() && proposed_log_prob.is_sign_negative());
        }
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

        let expected =
            site_count_to_f64(forward_sites).ln() - site_count_to_f64(reverse_sites).ln();
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
    fn cdt_proposal_scores_impossible_plan_as_negative_infinity() {
        let action_config = ActionConfig::default();
        let target =
            CdtTarget::new(action_config.clone(), 1.0).expect("valid target configuration");
        let proposal = CdtProposal::with_seed(action_config, 7);
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
        let plan = CdtProposalPlan {
            move_type: MoveType::Move31Remove,
            action_before: 1.0,
            action_after: None,
            delta_action: None,
            forward_site_count: 0,
            reverse_site_count: 0,
            proposed_state: triangulation.clone(),
        };

        let proposed_log_prob = proposal
            .proposed_log_prob(&triangulation, &plan, &target)
            .expect("scoring an impossible count delta should not fail");

        assert!(proposed_log_prob.is_infinite() && proposed_log_prob.is_sign_negative());
    }

    #[test]
    fn cdt_proposal_uses_delayed_chain() {
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
            .expect("ordinary no-site outcomes must be delayed-step rejections, not errors");

        assert_eq!(step.outcome.has_proposal(), step.info.is_some());
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
            action_after: Some(action_after),
            delta_action: Some(action_after - action_before),
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
