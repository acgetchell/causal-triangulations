#![forbid(unsafe_code)]

//! Metropolis-Hastings algorithm for Causal Dynamical Triangulations.
//!
//! This module implements the Monte Carlo sampling algorithm used to sample
//! triangulation configurations according to the CDT path integral measure.
//!
//! The simulation accepts or rejects a proposed move type before mutating the
//! triangulation. Accepted moves are then applied through the CDT ergodic move
//! kernels; failed accepted applications are rolled back and retried at another
//! randomly selected local site.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
use crate::cdt::results::{Measurement, SimulationResultsBackend};
use crate::cdt::triangulation::SimulationEvent;
use crate::config::validate_schedule;
use crate::errors::{CdtError, CdtResult, CheckpointResumeReason};
use crate::geometry::CdtTriangulation2D;
use crate::util::saturating_usize_to_u32;
use markov_chain_monte_carlo::{Chain, ChainCheckpoint, DelayedProposal, Target};
use rand::{Rng, RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::hint::cold_path;
use std::time::{Duration, Instant};

const ACCEPTED_MOVE_RETRIES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SimplexCounts {
    vertices: u32,
    edges: u32,
    triangles: u32,
}

/// Configuration for the Metropolis-Hastings algorithm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetropolisConfig {
    /// Temperature parameter (1/β)
    pub temperature: f64,
    /// Number of Monte Carlo steps to perform
    pub steps: u32,
    /// Number of thermalization steps before measurements
    pub thermalization_steps: u32,
    /// Frequency of measurements (take measurement every N steps)
    pub measurement_frequency: u32,
    /// Optional RNG seed for reproducible simulations (default: None = random)
    pub seed: Option<u64>,
}

impl Default for MetropolisConfig {
    /// Default Metropolis configuration for 2D CDT.
    fn default() -> Self {
        Self {
            temperature: 1.0,
            steps: 1000,
            thermalization_steps: 100,
            measurement_frequency: 10,
            seed: None,
        }
    }
}

impl MetropolisConfig {
    /// Creates a new Metropolis configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 500, 50, 5);
    /// assert_eq!(config.steps, 500);
    /// assert!(config.seed.is_none());
    /// ```
    #[must_use]
    pub const fn new(
        temperature: f64,
        steps: u32,
        thermalization_steps: u32,
        measurement_frequency: u32,
    ) -> Self {
        Self {
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
            seed: None,
        }
    }

    /// Sets the RNG seed for reproducible simulations.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(1.0, 100, 10, 5).with_seed(42);
    /// assert_eq!(config.seed, Some(42));
    /// ```
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Returns the inverse temperature (β = 1/T).
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5);
    /// assert_relative_eq!(config.beta(), 0.5);
    /// ```
    #[must_use]
    pub fn beta(&self) -> f64 {
        1.0 / self.temperature
    }

    /// Validates simulation-specific configuration values.
    ///
    /// # Errors
    ///
    /// Returns a structured error describing the invalid simulation setting.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(1.0, 100, 10, 5);
    /// assert!(config.validate().is_ok());
    /// ```
    pub fn validate(&self) -> CdtResult<()> {
        validate_schedule(
            self.temperature,
            self.steps,
            self.thermalization_steps,
            self.measurement_frequency,
            |setting, provided_value, expected| {
                invalid_sim_config(setting, provided_value, expected)
            },
        )
    }
}

/// Adapts shared schedule validation errors to the Metropolis-specific error variant.
fn invalid_sim_config(setting: &str, provided_value: String, expected: String) -> CdtError {
    CdtError::InvalidSimulationConfiguration {
        setting: setting.to_string(),
        provided_value,
        expected,
    }
}

/// Rejects temperatures that would make target log probabilities non-finite.
fn validate_temperature(temperature: f64) -> CdtResult<()> {
    if temperature.is_finite() && temperature > 0.0 {
        Ok(())
    } else {
        Err(invalid_sim_config(
            "temperature",
            temperature.to_string(),
            "finite and positive".to_string(),
        ))
    }
}

/// Result of a Monte Carlo step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloStep {
    /// Step number
    pub step: u32,
    /// Move type attempted
    pub move_type: MoveType,
    /// Whether the move was accepted
    pub accepted: bool,
    /// Action before the move
    pub action_before: f64,
    /// Action after the move (if accepted)
    pub action_after: Option<f64>,
    /// Change in action (ΔS)
    pub delta_action: Option<f64>,
}

// ---------------------------------------------------------------------------
// MCMC trait implementations for CDT
// ---------------------------------------------------------------------------

/// Target distribution for CDT: log-probability from the Regge action.
///
/// Computes `log_prob = -S / T` where `S` is the discrete Regge action
/// and `T` is the temperature.
pub struct CdtTarget {
    action_config: ActionConfig,
    temperature: f64,
}

impl CdtTarget {
    /// Creates a new CDT target distribution.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidConfiguration`] if the action couplings are
    /// non-finite, or [`CdtError::InvalidSimulationConfiguration`] if
    /// `temperature` is not finite and positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::action::ActionConfig;
    /// use causal_triangulations::prelude::simulation::CdtTarget;
    ///
    /// let _target = CdtTarget::new(ActionConfig::default(), 1.0)?;
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn new(action_config: ActionConfig, temperature: f64) -> CdtResult<Self> {
        action_config.validate()?;
        validate_temperature(temperature)?;
        Ok(Self {
            action_config,
            temperature,
        })
    }
}

impl Target<CdtTriangulation2D> for CdtTarget {
    fn log_prob(&self, state: &CdtTriangulation2D) -> f64 {
        let counts = simplex_counts(state);
        let action =
            self.action_config
                .calculate_action(counts.vertices, counts.edges, counts.triangles);
        -action / self.temperature
    }
}

/// Delayed CDT move proposal selected before mutating a triangulation.
///
/// A plan records the selected [`MoveType`] and its count-level action delta
/// before any triangulation mutation occurs. The delayed proposal API scores
/// this plan first, then applies it only if the Metropolis step accepts it.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::action::ActionConfig;
/// use causal_triangulations::prelude::moves::MoveType;
/// use causal_triangulations::prelude::simulation::{
///     CdtProposal, CdtProposalError, CdtTriangulation,
/// };
/// use markov_chain_monte_carlo::DelayedProposal;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), CdtProposalError> {
/// let tri = CdtTriangulation::from_cdt_strip(4, 3).expect("valid CDT strip");
/// let mut proposal =
///     CdtProposal::with_seed(ActionConfig::default(), 7).expect("valid action config");
/// let mut rng = StdRng::seed_from_u64(11);
///
/// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
///     return Ok(());
/// };
/// assert!(matches!(
///     plan.move_type(),
///     MoveType::Move22 | MoveType::Move13Add | MoveType::Move31Remove | MoveType::EdgeFlip
/// ));
/// assert!(plan.action_before().is_finite());
/// if let (Some(delta), Some(action_after)) = (plan.delta_action(), plan.action_after()) {
///     approx::assert_relative_eq!(
///         action_after,
///         plan.action_before() + delta,
///         epsilon = 1e-12
///     );
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CdtProposalPlan {
    move_type: MoveType,
    action_before: f64,
    action_after: Option<f64>,
    delta_action: Option<f64>,
    proposed_state: CdtTriangulation2D,
}

impl CdtProposalPlan {
    /// Returns the proposed move type.
    #[must_use]
    pub const fn move_type(&self) -> MoveType {
        self.move_type
    }

    /// Returns the current action used to score this proposal.
    #[must_use]
    pub const fn action_before(&self) -> f64 {
        self.action_before
    }

    /// Returns the proposed action if the move's simplex-count delta is valid.
    ///
    /// A value of `None` means the selected move cannot be scored from the
    /// current simplex counts, so [`DelayedProposal::proposed_log_prob`] treats
    /// the plan as impossible.
    #[must_use]
    pub const fn action_after(&self) -> Option<f64> {
        self.action_after
    }

    /// Returns the proposal action change, if it can be evaluated.
    #[must_use]
    pub const fn delta_action(&self) -> Option<f64> {
        self.delta_action
    }
}

/// Telemetry returned by delayed CDT proposal steps.
///
/// The sampler receives this compact record after a plan has been scored. It is
/// intended for diagnostics and measurement backends that need to report which
/// move family was proposed without exposing the private plan fields.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::action::ActionConfig;
/// use causal_triangulations::prelude::simulation::{
///     CdtProposal, CdtProposalError, CdtTriangulation,
/// };
/// use markov_chain_monte_carlo::DelayedProposal;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), CdtProposalError> {
/// let tri = CdtTriangulation::from_cdt_strip(4, 3).expect("valid CDT strip");
/// let mut proposal =
///     CdtProposal::with_seed(ActionConfig::default(), 7).expect("valid action config");
/// let mut rng = StdRng::seed_from_u64(11);
/// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
///     return Ok(());
/// };
///
/// let info = proposal.info(&plan);
/// assert_eq!(info.move_type, plan.move_type());
/// assert_eq!(info.delta_action.is_some(), plan.delta_action().is_some());
/// if let (Some(info_delta), Some(plan_delta)) = (info.delta_action, plan.delta_action()) {
///     approx::assert_relative_eq!(info_delta, plan_delta, epsilon = 1e-12);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CdtProposalInfo {
    /// Move type selected for the proposal.
    pub move_type: MoveType,
    /// Action before the proposal.
    pub action_before: f64,
    /// Action after the proposal if the count-level delta is valid.
    pub action_after: Option<f64>,
    /// Proposed action change.
    pub delta_action: Option<f64>,
}

/// Local-site rejection observed while trying to realize an accepted CDT proposal.
///
/// These rejections mean the move type was selected and accepted at the
/// count-action level, but the bounded random local-site search did not find a
/// concrete site where the move could be applied.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
enum CdtProposalSiteRejection {
    /// The selected local site would violate CDT causality.
    CausalityViolation,
    /// The selected local site was geometrically invalid.
    GeometricViolation,
    /// A move kernel rejected the selected local site with a typed CDT error.
    Kernel(CdtError),
}

impl fmt::Display for CdtProposalSiteRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CausalityViolation => {
                f.write_str("causality violation at selected application site")
            }
            Self::GeometricViolation => {
                f.write_str("geometric violation at selected application site")
            }
            Self::Kernel(err) => err.fmt(f),
        }
    }
}

impl Error for CdtProposalSiteRejection {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Kernel(err) => Some(err),
            Self::CausalityViolation | Self::GeometricViolation => None,
        }
    }
}

/// Error reported by delayed CDT proposal planning or commit.
///
/// No-site outcomes are ordinary proposal absence and are reported from
/// [`DelayedProposal::propose_plan`] as `Ok(None)`, matching the upstream
/// delayed-commit contract. `ApplicationFailed` represents a hard backend or
/// invariant failure while constructing or committing a concrete proposal, and
/// preserves the typed [`CdtError`] that caused the failed application.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::moves::MoveType;
/// use causal_triangulations::prelude::simulation::CdtProposalError;
/// use causal_triangulations::CdtError;
///
/// let err = CdtProposalError::ApplicationFailed {
///     move_type: MoveType::Move13Add,
///     attempt: 2,
///     source: CdtError::BackendMutationFailed {
///         operation: "insert_vertex".to_string(),
///         target: "face FaceKey(3)".to_string(),
///         detail: "backend rejected mutation".to_string(),
///     },
/// };
/// assert!(err.to_string().contains("Move13Add"));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CdtProposalError {
    /// Constructing or applying a concrete proposal hit a hard backend or invariant failure.
    ApplicationFailed {
        /// Accepted move type.
        move_type: MoveType,
        /// Local-site attempt that hit the hard failure.
        attempt: usize,
        /// Typed lower-level failure observed while committing the accepted move.
        source: CdtError,
    },
}

impl fmt::Display for CdtProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplicationFailed {
                move_type,
                attempt,
                source,
            } => write!(
                f,
                "failed to apply {move_type:?} on attempt {attempt}: {source}"
            ),
        }
    }
}

impl Error for CdtProposalError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ApplicationFailed { source, .. } => Some(source),
        }
    }
}

/// Delayed-commit CDT proposal distribution.
///
/// This adapter exposes CDT's accept-before-mutation move ordering through the
/// [`DelayedProposal`] API. Use the same [`ActionConfig`] as the matching
/// [`CdtTarget`] or [`MetropolisAlgorithm`] so that proposal planning and target
/// scoring agree.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::action::ActionConfig;
/// use causal_triangulations::prelude::simulation::{
///     CdtProposal, CdtProposalError, CdtTriangulation,
/// };
/// use markov_chain_monte_carlo::DelayedProposal;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), CdtProposalError> {
/// let tri = CdtTriangulation::from_cdt_strip(4, 3).expect("valid CDT strip");
/// let mut proposal = CdtProposal::new(ActionConfig::default()).expect("valid action config");
/// let mut rng = StdRng::seed_from_u64(7);
///
/// let plan = proposal.propose_plan(&tri, &mut rng)?;
/// if let Some(plan) = plan {
///     assert!(plan.action_before().is_finite());
/// }
/// # Ok(())
/// # }
/// ```
pub struct CdtProposal {
    action_config: ActionConfig,
    moves: ErgodicsSystem,
}

impl CdtProposal {
    /// Creates a new unseeded delayed CDT proposal distribution.
    ///
    /// Delayed scoring is delegated to the target passed to
    /// [`DelayedProposal::proposed_log_prob`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidConfiguration`] if the action couplings are
    /// non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::action::ActionConfig;
    /// use causal_triangulations::prelude::simulation::CdtProposal;
    ///
    /// let _proposal = CdtProposal::new(ActionConfig::default())?;
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn new(action_config: ActionConfig) -> CdtResult<Self> {
        action_config.validate()?;
        Ok(Self {
            action_config,
            moves: ErgodicsSystem::new(),
        })
    }

    /// Creates a seeded delayed CDT proposal distribution.
    ///
    /// The seed controls the internal move-family selector. The `rng` passed to
    /// [`DelayedProposal::propose_plan`] is still accepted for compatibility
    /// with generic MCMC drivers.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidConfiguration`] if the action couplings are
    /// non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::action::ActionConfig;
    /// use causal_triangulations::prelude::simulation::CdtProposal;
    ///
    /// let _proposal = CdtProposal::with_seed(ActionConfig::default(), 42)?;
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn with_seed(action_config: ActionConfig, seed: u64) -> CdtResult<Self> {
        action_config.validate()?;
        Ok(Self {
            action_config,
            moves: ErgodicsSystem::with_seed(seed),
        })
    }
}

impl DelayedProposal<CdtTriangulation2D> for CdtProposal {
    type Plan = CdtProposalPlan;
    type Info = CdtProposalInfo;
    type Error = CdtProposalError;

    fn propose_plan<R: Rng + ?Sized>(
        &mut self,
        state: &CdtTriangulation2D,
        _rng: &mut R,
    ) -> Result<Option<Self::Plan>, Self::Error> {
        let move_type = self.moves.select_random_move();
        let action_before = action_for(&self.action_config, state);
        if proposed_delta_action(&self.action_config, simplex_counts(state), move_type).is_none() {
            cold_path();
            return Ok(None);
        }

        let mut proposed_state = state.clone();
        let action_after = match apply_accepted_move(
            &mut proposed_state,
            &mut self.moves,
            &self.action_config,
            move_type,
            action_before,
        ) {
            Ok(AcceptedMoveResult::Applied { action_after }) => action_after,
            Ok(AcceptedMoveResult::NoApplicableSite { .. }) => {
                cold_path();
                return Ok(None);
            }
            Err(err) => {
                cold_path();
                return Err(CdtProposalError::ApplicationFailed {
                    move_type,
                    attempt: err.attempt,
                    source: err.source,
                });
            }
        };
        let delta_action = action_after - action_before;

        Ok(Some(CdtProposalPlan {
            move_type,
            action_before,
            action_after: Some(action_after),
            delta_action: Some(delta_action),
            proposed_state,
        }))
    }

    fn proposed_log_prob<T: Target<CdtTriangulation2D>>(
        &self,
        _state: &CdtTriangulation2D,
        plan: &Self::Plan,
        target: &T,
    ) -> Result<f64, Self::Error> {
        Ok(plan
            .action_after
            .map_or(f64::NEG_INFINITY, |_| target.log_prob(&plan.proposed_state)))
    }

    fn info(&self, plan: &Self::Plan) -> Self::Info {
        CdtProposalInfo {
            move_type: plan.move_type,
            action_before: plan.action_before,
            action_after: plan.action_after,
            delta_action: plan.delta_action,
        }
    }

    fn commit<R: Rng + ?Sized>(
        &mut self,
        state: &mut CdtTriangulation2D,
        plan: Self::Plan,
        _rng: &mut R,
    ) -> Result<(), Self::Error> {
        *state = plan.proposed_state;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Metropolis algorithm
// ---------------------------------------------------------------------------

/// Resumable checkpoint for a CDT Metropolis-Hastings run.
///
/// The embedded [`ChainCheckpoint`] stores the current triangulation and
/// accepted/rejected chain counters using the shared MCMC crate's portable
/// checkpoint type. CDT adds the domain-specific runtime state needed for
/// scientific continuation: action/config metadata, accumulated telemetry,
/// both RNG streams, and the ergodic move system.
///
/// Resuming a serialized checkpoint continues from the stored chain counters
/// and RNG streams. The triangulation is restored through its invariant-checked
/// serde representation, so callers should rely on CDT observables and
/// validation contracts rather than byte-for-byte backend identity.
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
///         MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
///         ActionConfig::default(),
///     );
///     let checkpoint = algorithm.run_to_checkpoint(tri)?;
///
///     assert_eq!(checkpoint.current_step(), 2);
///     assert_eq!(checkpoint.steps().len(), 2);
///     assert_eq!(checkpoint.chain().total_steps(), 2);
///     assert_eq!(checkpoint.config().steps, 2);
///     assert_eq!(checkpoint.action_config(), &ActionConfig::default());
///     assert!(checkpoint.current_action().is_finite());
///     assert_eq!(checkpoint.move_stats().total_attempted(), 2);
///     assert_eq!(checkpoint.measurements().len(), 3);
///     Ok(())
/// }
/// ```
#[derive(Clone, Serialize, Deserialize)]
pub struct CdtMcmcCheckpoint {
    chain: ChainCheckpoint<CdtTriangulation2D>,
    config: MetropolisConfig,
    action_config: ActionConfig,
    current_step: u32,
    current_action: f64,
    move_stats: MoveStatistics,
    steps: Vec<MonteCarloStep>,
    measurements: Vec<Measurement>,
    elapsed_time: Duration,
    acceptance_rng: Xoshiro256PlusPlus,
    ergodics: ErgodicsSystem,
}

impl CdtMcmcCheckpoint {
    /// Returns the generic MCMC chain checkpoint.
    pub const fn chain(&self) -> &ChainCheckpoint<CdtTriangulation2D> {
        &self.chain
    }

    /// Returns the Metropolis configuration used when the checkpoint was made.
    #[must_use]
    pub const fn config(&self) -> &MetropolisConfig {
        &self.config
    }

    /// Returns the action configuration used when the checkpoint was made.
    #[must_use]
    pub const fn action_config(&self) -> &ActionConfig {
        &self.action_config
    }

    /// Returns the last completed Monte Carlo step.
    #[must_use]
    pub const fn current_step(&self) -> u32 {
        self.current_step
    }

    /// Returns the action of the checkpointed triangulation.
    #[must_use]
    pub const fn current_action(&self) -> f64 {
        self.current_action
    }

    /// Returns accumulated move statistics through the checkpoint step.
    #[must_use]
    pub const fn move_stats(&self) -> &MoveStatistics {
        &self.move_stats
    }

    /// Returns accumulated step telemetry through the checkpoint step.
    #[must_use]
    pub fn steps(&self) -> &[MonteCarloStep] {
        &self.steps
    }

    /// Returns accumulated measurements through the checkpoint step.
    #[must_use]
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Converts the checkpoint into a complete simulation result snapshot.
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
    ///         MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
    ///         ActionConfig::default(),
    ///     )
    ///     .run_to_checkpoint(tri)?;
    ///
    ///     let results = checkpoint.into_results();
    ///     assert_eq!(results.steps.len(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn into_results(self) -> SimulationResultsBackend {
        let (triangulation, _, _) = self.chain.into_parts();
        SimulationResultsBackend {
            config: self.config,
            action_config: self.action_config,
            move_stats: self.move_stats,
            steps: self.steps,
            measurements: self.measurements,
            elapsed_time: self.elapsed_time,
            triangulation,
        }
    }
}

struct MetropolisRunState {
    triangulation: CdtTriangulation2D,
    current_step: u32,
    current_action: f64,
    acceptance_rng: Xoshiro256PlusPlus,
    ergodics: ErgodicsSystem,
    move_stats: MoveStatistics,
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
    /// let config = MetropolisConfig::new(1.0, 10, 2, 1);
    /// let _algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
    /// ```
    #[must_use]
    pub const fn new(config: MetropolisConfig, action_config: ActionConfig) -> Self {
        Self {
            config,
            action_config,
        }
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
    ///     let config = MetropolisConfig::new(1.0, 2, 1, 1).with_seed(7);
    ///     let results = MetropolisAlgorithm::new(config, ActionConfig::default()).run(tri)?;
    ///     assert_eq!(results.steps.len(), 2);
    ///     Ok(())
    /// }
    /// ```
    pub fn run(&self, triangulation: CdtTriangulation2D) -> CdtResult<SimulationResultsBackend> {
        Ok(self.run_to_checkpoint(triangulation)?.into_results())
    }

    /// Run the simulation and return both the final results and checkpoint.
    ///
    /// The checkpoint can be serialized and later resumed with
    /// [`Self::resume_from_checkpoint`] without losing the CDT RNG streams.
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
    ///         MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
    ///         ActionConfig::default(),
    ///     );
    ///     let (results, checkpoint) = algorithm.run_with_checkpoint(tri)?;
    ///
    ///     assert_eq!(results.steps.len(), checkpoint.steps().len());
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
    /// [`ChainCheckpoint`] and stores CDT-specific proposal state, telemetry,
    /// and RNG streams beside it.
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
    ///         MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
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
        self.config.validate()?;
        self.action_config.validate()?;

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
    ///         MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
    ///         action.clone(),
    ///     );
    ///     let checkpoint = prefix.run_to_checkpoint(tri)?;
    ///
    ///     let resumed = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
    ///         action,
    ///     )
    ///     .resume_from_checkpoint(checkpoint)?;
    ///
    ///     assert_eq!(resumed.steps.len(), 4);
    ///     assert_eq!(resumed.config.steps, 4);
    ///     Ok(())
    /// }
    /// ```
    pub fn resume_from_checkpoint(
        &self,
        checkpoint: CdtMcmcCheckpoint,
    ) -> CdtResult<SimulationResultsBackend> {
        self.config.validate()?;
        self.action_config.validate()?;
        validate_resume_compatible(self, &checkpoint)?;

        let mut result_config = checkpoint.config.clone();
        result_config.steps = checkpoint
            .current_step
            .checked_add(self.config.steps)
            .ok_or_else(|| {
                checkpoint_resume_failed(
                    CheckpointResumeReason::StepCountOverflow,
                    "resumed step count exceeds u32::MAX",
                )
            })?;

        let mut state = MetropolisRunState::from_checkpoint(checkpoint)?;
        self.run_steps(&mut state, self.config.steps)?;
        state
            .into_checkpoint(result_config, self.action_config.clone())
            .map(CdtMcmcCheckpoint::into_results)
    }

    fn initial_state(&self, mut triangulation: CdtTriangulation2D) -> MetropolisRunState {
        let current_action = action_for(&self.action_config, &triangulation);
        let measurements = vec![measurement_for(0, current_action, &triangulation)];
        triangulation.record_event(SimulationEvent::MeasurementTaken {
            step: 0,
            action: current_action,
        });

        MetropolisRunState {
            triangulation,
            current_step: 0,
            current_action,
            acceptance_rng: simulation_rng(self.config.seed),
            ergodics: self.config.seed.map_or_else(ErgodicsSystem::new, |seed| {
                ErgodicsSystem::with_seed(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
            }),
            move_stats: MoveStatistics::new(),
            steps: Vec::new(),
            measurements,
            elapsed_time: Duration::ZERO,
        }
    }

    fn run_steps(&self, state: &mut MetropolisRunState, additional_steps: u32) -> CdtResult<()> {
        let start = Instant::now();
        for _ in 0..additional_steps {
            let step = state.current_step.checked_add(1).ok_or_else(|| {
                checkpoint_resume_failed(
                    CheckpointResumeReason::StepCountOverflow,
                    "resumed step count exceeds u32::MAX",
                )
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
            checkpoint.config.temperature,
        )
        .map_err(|err| {
            checkpoint_resume_failed(
                CheckpointResumeReason::CheckpointTargetConfiguration,
                err.to_string(),
            )
        })?;
        let chain = Chain::from_checkpoint(checkpoint.chain, &target).map_err(|err| {
            checkpoint_resume_failed(CheckpointResumeReason::McmcChainRestore, err.to_string())
        })?;
        let triangulation = chain.into_state();
        triangulation.validate_evolved_cdt().map_err(|err| {
            checkpoint_resume_failed(
                CheckpointResumeReason::TriangulationInvariants,
                err.to_string(),
            )
        })?;
        let actual_action = action_for(&checkpoint.action_config, &triangulation);
        if !actions_match(actual_action, checkpoint.current_action) {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::ActionMismatch,
                format!(
                    "checkpoint action mismatch: stored {}, recomputed {}",
                    checkpoint.current_action, actual_action
                ),
            ));
        }

        Ok(Self {
            triangulation,
            current_step: checkpoint.current_step,
            current_action: checkpoint.current_action,
            acceptance_rng: checkpoint.acceptance_rng,
            ergodics: checkpoint.ergodics,
            move_stats: checkpoint.move_stats,
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
        Ok(CdtMcmcCheckpoint {
            chain: ChainCheckpoint::new(self.triangulation, accepted, rejected),
            config,
            action_config,
            current_step: self.current_step,
            current_action: self.current_action,
            move_stats: self.move_stats,
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
            move_type: format!("{move_type:?}"),
            step: step.into(),
        });

    let action_before = state.current_action;
    let delta_action = proposed_delta_action(
        &algorithm.action_config,
        simplex_counts(&state.triangulation),
        move_type,
    );

    let mut accepted = false;
    let mut action_after = None;
    if let Some(delta) = delta_action
        && metropolis_accept(
            delta,
            algorithm.config.temperature,
            &mut state.acceptance_rng,
        )
    {
        match apply_accepted_move(
            &mut state.triangulation,
            &mut state.ergodics,
            &algorithm.action_config,
            move_type,
            action_before,
        ) {
            Ok(AcceptedMoveResult::Applied {
                action_after: applied_action,
            }) => {
                accepted = true;
                action_after = Some(applied_action);
                state.current_action = applied_action;
                state.move_stats.record_success(move_type);
                state
                    .triangulation
                    .record_event(SimulationEvent::MoveAccepted {
                        move_type: format!("{move_type:?}"),
                        step: step.into(),
                        action_change: applied_action - action_before,
                    });
                validate_evolved_cdt_if_due(state)?;
            }
            Ok(AcceptedMoveResult::NoApplicableSite { .. }) => {
                // A move type can be Metropolis-accepted even when bounded
                // random local-site selection finds no realizable site. That
                // is an ordinary proposal rejection, not a fatal simulation error.
            }
            Err(err) => {
                return Err(accepted_move_error(
                    step,
                    move_type,
                    err.attempt,
                    err.source.to_string(),
                ));
            }
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

    if step.is_multiple_of(algorithm.config.measurement_frequency) {
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

/// Builds a structured checkpoint-resume error.
fn checkpoint_resume_failed(reason: CheckpointResumeReason, detail: impl Into<String>) -> CdtError {
    CdtError::CheckpointResumeFailed {
        reason,
        detail: detail.into(),
    }
}

/// Verifies that a checkpoint can be resumed by the requested algorithm.
///
/// Resume accepts a different fresh seed because serialized checkpoints carry
/// their own RNG streams, but rejects physics and sampling schedule changes
/// that would make the cumulative chain scientifically ambiguous.
fn validate_resume_compatible(
    algorithm: &MetropolisAlgorithm,
    checkpoint: &CdtMcmcCheckpoint,
) -> CdtResult<()> {
    if algorithm.action_config != checkpoint.action_config {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::IncompatibleActionConfiguration,
            "action configuration differs from checkpoint",
        ));
    }
    if algorithm.config.temperature.to_bits() != checkpoint.config.temperature.to_bits() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::IncompatibleTemperature,
            "temperature differs from checkpoint",
        ));
    }
    if algorithm.config.thermalization_steps != checkpoint.config.thermalization_steps {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::IncompatibleThermalizationSchedule,
            "thermalization schedule differs from checkpoint",
        ));
    }
    if algorithm.config.measurement_frequency != checkpoint.config.measurement_frequency {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::IncompatibleMeasurementFrequency,
            "measurement frequency differs from checkpoint",
        ));
    }
    validate_checkpoint_counters(checkpoint)
}

/// Checks that serialized chain counters and CDT telemetry agree.
///
/// The generic checkpoint, move statistics, current step, and step telemetry
/// are redundant by design; this catches tampered or partially written
/// checkpoint payloads before any resumed sampling occurs.
fn validate_checkpoint_counters(checkpoint: &CdtMcmcCheckpoint) -> CdtResult<()> {
    checkpoint.config.validate().map_err(|err| {
        checkpoint_resume_failed(
            CheckpointResumeReason::CheckpointConfiguration,
            err.to_string(),
        )
    })?;
    checkpoint.action_config.validate().map_err(|err| {
        checkpoint_resume_failed(
            CheckpointResumeReason::CheckpointActionConfiguration,
            err.to_string(),
        )
    })?;

    let (accepted, rejected) = chain_counters(&checkpoint.move_stats)?;
    if checkpoint.chain.accepted() != accepted || checkpoint.chain.rejected() != rejected {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::ChainCounterMismatch,
            "chain counters do not match move statistics",
        ));
    }
    if checkpoint.chain.total_steps()
        != usize::try_from(checkpoint.current_step).unwrap_or(usize::MAX)
    {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::ChainStepMismatch,
            "chain step count does not match checkpoint step",
        ));
    }
    if checkpoint.steps.len() != checkpoint.chain.total_steps() {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::StepTelemetryMismatch,
            "step telemetry length does not match chain step count",
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
            CheckpointResumeReason::StepTelemetryMismatch,
            format!(
                "accepted step count mismatch: got {}, expected {}",
                accepted_steps,
                checkpoint.chain.accepted()
            ),
        ));
    }

    for (index, step) in checkpoint.steps.iter().enumerate() {
        let expected_step = u32::try_from(index + 1).map_err(|_| {
            checkpoint_resume_failed(
                CheckpointResumeReason::StepTelemetryOverflow,
                "step telemetry index exceeds u32::MAX",
            )
        })?;
        if step.step != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::StepTelemetryMismatch,
                format!(
                    "step telemetry must be sequential: got step {}, expected {}",
                    step.step, expected_step
                ),
            ));
        }
        if !step.action_before.is_finite() {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::StepTelemetryMismatch,
                format!("step {} has non-finite action_before", step.step),
            ));
        }
        if let Some(delta_action) = step.delta_action
            && !delta_action.is_finite()
        {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::StepTelemetryMismatch,
                format!("step {} has non-finite delta_action", step.step),
            ));
        }
        if step.accepted && step.delta_action.is_none() {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::StepTelemetryMismatch,
                format!("accepted step {} is missing delta_action", step.step),
            ));
        }
        match (step.accepted, step.action_after) {
            (true, Some(action_after)) if action_after.is_finite() => {
                if let Some(delta_action) = step.delta_action
                    && !actions_match(action_after, step.action_before + delta_action)
                {
                    return Err(checkpoint_resume_failed(
                        CheckpointResumeReason::StepTelemetryMismatch,
                        format!(
                            "step {} action_after does not match delta_action",
                            step.step
                        ),
                    ));
                }
            }
            (true, Some(_)) => {
                return Err(checkpoint_resume_failed(
                    CheckpointResumeReason::StepTelemetryMismatch,
                    format!("step {} has non-finite action_after", step.step),
                ));
            }
            (true, None) => {
                return Err(checkpoint_resume_failed(
                    CheckpointResumeReason::StepTelemetryMismatch,
                    format!("accepted step {} is missing action_after", step.step),
                ));
            }
            (false, Some(_)) => {
                return Err(checkpoint_resume_failed(
                    CheckpointResumeReason::StepTelemetryMismatch,
                    format!("rejected step {} unexpectedly has action_after", step.step),
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
        u64::from(checkpoint.current_step) / u64::from(checkpoint.config.measurement_frequency) + 1,
    )
    .map_err(|_| {
        checkpoint_resume_failed(
            CheckpointResumeReason::MeasurementTelemetryOverflow,
            "scheduled measurement count exceeds usize::MAX",
        )
    })?;
    if checkpoint.measurements.len() != expected_measurements {
        return Err(checkpoint_resume_failed(
            CheckpointResumeReason::MeasurementTelemetryMismatch,
            format!(
                "scheduled measurement count mismatch: got {}, expected {}",
                checkpoint.measurements.len(),
                expected_measurements
            ),
        ));
    }

    for (index, measurement) in checkpoint.measurements.iter().enumerate() {
        let expected_step = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(u64::from(checkpoint.config.measurement_frequency)))
            .and_then(|step| u32::try_from(step).ok())
            .ok_or_else(|| {
                checkpoint_resume_failed(
                    CheckpointResumeReason::MeasurementTelemetryOverflow,
                    "scheduled measurement step exceeds u32::MAX",
                )
            })?;
        if measurement.step != expected_step {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::MeasurementTelemetryMismatch,
                format!(
                    "measurement telemetry must follow the sampling schedule: got step {}, expected {}",
                    measurement.step, expected_step
                ),
            ));
        }
        if !measurement.action.is_finite() {
            return Err(checkpoint_resume_failed(
                CheckpointResumeReason::MeasurementTelemetryMismatch,
                format!(
                    "measurement at step {} has non-finite action",
                    measurement.step
                ),
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
fn chain_counters(move_stats: &MoveStatistics) -> CdtResult<(usize, usize)> {
    let attempted = move_stats.total_attempted();
    let accepted = move_stats.total_accepted();
    let rejected = attempted.checked_sub(accepted).ok_or_else(|| {
        checkpoint_resume_failed(
            CheckpointResumeReason::MoveStatisticsInvariant,
            "accepted move count exceeds attempted move count",
        )
    })?;
    Ok((
        usize::try_from(accepted).map_err(|_| {
            checkpoint_resume_failed(
                CheckpointResumeReason::CounterConversionOverflow,
                "accepted move count exceeds usize::MAX",
            )
        })?,
        usize::try_from(rejected).map_err(|_| {
            checkpoint_resume_failed(
                CheckpointResumeReason::CounterConversionOverflow,
                "rejected move count exceeds usize::MAX",
            )
        })?,
    ))
}

/// Builds the RNG used only for Metropolis acceptance draws.
///
/// This keeps acceptance randomness separate from move-site selection, so seeded
/// simulations are reproducible while unseeded simulations still draw fresh entropy.
fn simulation_rng(seed: Option<u64>) -> Xoshiro256PlusPlus {
    seed.map_or_else(rand::make_rng, Xoshiro256PlusPlus::seed_from_u64)
}

/// Reads simplex counts through the CDT wrapper for action and measurement code.
///
/// Centralizing these conversions keeps cached query paths authoritative and
/// makes integer saturation explicit at the simulation boundary.
fn simplex_counts(triangulation: &CdtTriangulation2D) -> SimplexCounts {
    SimplexCounts {
        vertices: saturating_usize_to_u32(triangulation.vertex_count()),
        edges: saturating_usize_to_u32(triangulation.edge_count()),
        triangles: saturating_usize_to_u32(triangulation.face_count()),
    }
}

/// Computes the current action from live simplex counts.
///
/// The Metropolis loop calls this only after state is known to be current, which
/// avoids trusting stale values across backend mutations or rollback.
fn action_for(action_config: &ActionConfig, triangulation: &CdtTriangulation2D) -> f64 {
    let counts = simplex_counts(triangulation);
    action_config.calculate_action(counts.vertices, counts.edges, counts.triangles)
}

/// Captures a measurement from the live triangulation state.
///
/// Keeping measurement construction in one helper ensures recorded actions and
/// simplex counts use the same query path at every measurement step.
fn measurement_for(step: u32, action: f64, triangulation: &CdtTriangulation2D) -> Measurement {
    let counts = simplex_counts(triangulation);
    Measurement {
        step,
        action,
        vertices: counts.vertices,
        edges: counts.edges,
        triangles: counts.triangles,
        volume_profile: triangulation.volume_profile(),
    }
}

/// Computes the count-level action change before mutating the triangulation.
///
/// This is the core proposal-before-mutation calculation: Metropolis acceptance
/// must be based on the selected move type's known simplex-count delta, not on a
/// speculative backend edit that may need rollback.
fn proposed_delta_action(
    action_config: &ActionConfig,
    before: SimplexCounts,
    move_type: MoveType,
) -> Option<f64> {
    let after = match move_type {
        MoveType::Move22 | MoveType::EdgeFlip => before,
        MoveType::Move13Add => SimplexCounts {
            vertices: before.vertices.checked_add(1)?,
            edges: before.edges.checked_add(3)?,
            triangles: before.triangles.checked_add(2)?,
        },
        MoveType::Move31Remove => SimplexCounts {
            vertices: before.vertices.checked_sub(1)?,
            edges: before.edges.checked_sub(3)?,
            triangles: before.triangles.checked_sub(2)?,
        },
    };

    let action_before =
        action_config.calculate_action(before.vertices, before.edges, before.triangles);
    let action_after = action_config.calculate_action(after.vertices, after.edges, after.triangles);
    Some(action_after - action_before)
}

/// Applies the Metropolis acceptance rule to a proposed action change.
///
/// Factoring this out keeps the probability rule isolated from move selection
/// and makes deterministic unit tests possible with a seeded RNG.
fn metropolis_accept<R: Rng + ?Sized>(delta_action: f64, temperature: f64, rng: &mut R) -> bool {
    delta_action <= 0.0 || rng.random::<f64>() < (-delta_action / temperature).exp()
}

/// Compares action values with a scale-aware tolerance for checkpoint validation.
fn actions_match(left: f64, right: f64) -> bool {
    if !(left.is_finite() && right.is_finite()) {
        return false;
    }
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= f64::EPSILON * scale * 8.0
}

/// Applies an already-accepted move, rolling back and retrying failed sites.
///
/// Retry exhaustion means the move type did not bind to a realizable local site
/// in the bounded random search, so callers record a normal rejection. Hard
/// backend failures still return a structured error.
#[derive(Debug, Clone, PartialEq)]
enum AcceptedMoveResult {
    Applied {
        action_after: f64,
    },
    NoApplicableSite {
        last_rejection: Option<CdtProposalSiteRejection>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct MoveApplicationError {
    attempt: usize,
    source: CdtError,
}

fn apply_accepted_move(
    triangulation: &mut CdtTriangulation2D,
    moves: &mut ErgodicsSystem,
    action_config: &ActionConfig,
    move_type: MoveType,
    action_before: f64,
) -> Result<AcceptedMoveResult, MoveApplicationError> {
    let mut last_rejection = None;
    for attempt in 1..=ACCEPTED_MOVE_RETRIES {
        let counts_before = simplex_counts(triangulation);
        let result = attempt_move(moves, move_type, triangulation);
        let rejection = match result {
            MoveResult::Success => {
                let action_after = action_for(action_config, triangulation);
                return Ok(AcceptedMoveResult::Applied { action_after });
            }
            MoveResult::HardFailure(err) => {
                return Err(MoveApplicationError {
                    attempt,
                    source: err,
                });
            }
            MoveResult::CausalityViolation => CdtProposalSiteRejection::CausalityViolation,
            MoveResult::GeometricViolation => CdtProposalSiteRejection::GeometricViolation,
            MoveResult::Rejected(err) => CdtProposalSiteRejection::Kernel(err),
        };
        debug_assert_eq!(
            simplex_counts(triangulation),
            counts_before,
            "failed move kernels must roll back simplex counts before returning"
        );
        last_rejection = Some(rejection);
    }

    debug_assert!(
        actions_match(action_for(action_config, triangulation), action_before),
        "failed accepted move retries must leave the triangulation rolled back"
    );
    Ok(AcceptedMoveResult::NoApplicableSite { last_rejection })
}

/// Builds the simulation-level error for an accepted move that could not be applied.
///
/// The move kernels keep causal, geometric, and backend failures orthogonal; this
/// wrapper adds the Metropolis step, move type, and retry context callers need to
/// debug a failed accepted application.
const fn accepted_move_error(
    step: u32,
    move_type: MoveType,
    attempts: usize,
    last_failure: String,
) -> CdtError {
    CdtError::MetropolisMoveApplicationFailed {
        step,
        move_type,
        attempts,
        last_failure,
    }
}

/// Dispatches one selected move type to the ergodic move system.
///
/// Keeping dispatch behind this helper lets the Metropolis loop work with move
/// proposals uniformly while the move module retains ownership of each kernel.
fn attempt_move(
    moves: &mut ErgodicsSystem,
    move_type: MoveType,
    triangulation: &mut CdtTriangulation2D,
) -> MoveResult {
    match move_type {
        MoveType::Move22 => moves.attempt_22_move(triangulation),
        MoveType::Move13Add => moves.attempt_13_move(triangulation),
        MoveType::Move31Remove => moves.attempt_31_move(triangulation),
        MoveType::EdgeFlip => moves.attempt_edge_flip(triangulation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::triangulation::CdtTriangulation;
    use crate::geometry::traits::TriangulationQuery;
    use approx::assert_relative_eq;
    use markov_chain_monte_carlo::Chain;
    use rand::rngs::StdRng;
    use serde_json::{from_str, to_string, to_value};
    use std::num::NonZeroUsize;

    fn assert_optional_relative_eq(left: Option<f64>, right: Option<f64>) {
        match (left, right) {
            (Some(left), Some(right)) => assert_relative_eq!(left, right, epsilon = 1e-12),
            (None, None) => {}
            other => panic!("expected matching optional floats, got {other:?}"),
        }
    }

    fn short_checkpoint() -> CdtMcmcCheckpoint {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
            ActionConfig::default(),
        )
        .run_to_checkpoint(triangulation)
        .expect("short prefix run should checkpoint")
    }

    fn empty_run_state(triangulation: CdtTriangulation2D) -> MetropolisRunState {
        MetropolisRunState {
            triangulation,
            current_step: 0,
            current_action: 0.0,
            acceptance_rng: simulation_rng(Some(1)),
            ergodics: ErgodicsSystem::with_seed(2),
            move_stats: MoveStatistics::new(),
            steps: Vec::new(),
            measurements: Vec::new(),
            elapsed_time: Duration::ZERO,
        }
    }

    fn assert_checkpoint_resume_failed(
        result: CdtResult<SimulationResultsBackend>,
        expected_reason: CheckpointResumeReason,
        expected_detail: &str,
    ) {
        let Err(CdtError::CheckpointResumeFailed { reason, detail }) = result else {
            panic!("expected checkpoint resume failure");
        };
        assert_eq!(reason, expected_reason);
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
        let config = MetropolisConfig::new(2.0, 500, 50, 5);
        assert_relative_eq!(config.temperature, 2.0);
        assert_relative_eq!(config.beta(), 0.5);
        assert_eq!(config.steps, 500);
        assert!(config.seed.is_none());

        let seeded = config.with_seed(123);
        assert_eq!(seeded.seed, Some(123));
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
        let config = MetropolisConfig::new(1.0, 10, 2, 2).with_seed(42);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        let triangulation =
            CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed to create");
        let results = algorithm
            .run(triangulation)
            .expect("simulation should run with real move loop");

        assert_eq!(results.steps.len(), 10);
        assert_relative_eq!(
            results.move_stats.total_acceptance_rate(),
            results.acceptance_rate()
        );
        assert!(results.measurements.iter().all(|measurement| {
            measurement.action.is_finite()
                && measurement.vertices > 0
                && measurement.edges > 0
                && measurement.triangles > 0
        }));
    }

    #[test]
    fn run_with_checkpoint_returns_matching_results_and_checkpoint() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 3, 0, 1).with_seed(13),
            ActionConfig::default(),
        );

        let (results, checkpoint) = algorithm
            .run_with_checkpoint(triangulation)
            .expect("checkpointed run should complete");

        assert_eq!(checkpoint.current_step(), 3);
        assert_eq!(results.steps.len(), checkpoint.steps().len());
        assert_eq!(&results.config, checkpoint.config());
        let checkpoint_results = checkpoint.into_results();
        assert_eq!(
            results.triangulation.vertex_count(),
            checkpoint_results.triangulation.vertex_count()
        );
        checkpoint_results
            .triangulation
            .validate()
            .expect("checkpoint triangulation should satisfy evolved invariants");
    }

    #[test]
    fn serialized_checkpoint_resumes_from_stored_rng_state() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let action_config = ActionConfig::default();
        let prefix = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 4, 0, 1).with_seed(13),
            action_config.clone(),
        );
        let checkpoint = prefix
            .run_to_checkpoint(triangulation)
            .expect("prefix run should checkpoint");
        let checkpoint_json = to_string(&checkpoint).expect("checkpoint should serialize");
        let checkpoint: CdtMcmcCheckpoint =
            from_str(&checkpoint_json).expect("checkpoint should deserialize");
        let alternate_checkpoint: CdtMcmcCheckpoint =
            from_str(&checkpoint_json).expect("checkpoint should deserialize again");
        let first_resume_algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 6, 0, 1).with_seed(999),
            action_config.clone(),
        );
        let first_resumed = first_resume_algorithm
            .resume_from_checkpoint(checkpoint)
            .expect("resume should complete");
        let second_resume_algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 6, 0, 1).with_seed(123),
            action_config,
        );
        let second_resumed = second_resume_algorithm
            .resume_from_checkpoint(alternate_checkpoint)
            .expect("resume should ignore fresh seed and use checkpoint RNG state");

        assert_eq!(first_resumed.config.steps, 10);
        assert_eq!(first_resumed.steps.len(), 10);
        assert_eq!(first_resumed.steps[4].step, 5);
        first_resumed
            .triangulation
            .validate_topology()
            .expect("resumed triangulation should preserve topology");
        first_resumed
            .triangulation
            .validate_foliation()
            .expect("resumed triangulation should preserve foliation");
        first_resumed
            .triangulation
            .validate_causality()
            .expect("resumed triangulation should preserve causality");
        first_resumed
            .triangulation
            .validate_cell_classification()
            .expect("resumed triangulation should preserve cell classification");
        assert_eq!(
            to_value(&first_resumed.steps).expect("steps should serialize"),
            to_value(&second_resumed.steps).expect("steps should serialize")
        );
        assert_eq!(
            to_value(&first_resumed.measurements).expect("measurements should serialize"),
            to_value(&second_resumed.measurements).expect("measurements should serialize")
        );
        assert_eq!(
            to_value(&first_resumed.move_stats).expect("stats should serialize"),
            to_value(&second_resumed.move_stats).expect("stats should serialize")
        );
        assert_eq!(
            first_resumed.triangulation.vertex_count(),
            second_resumed.triangulation.vertex_count()
        );
        assert_eq!(
            first_resumed.triangulation.edge_count(),
            second_resumed.triangulation.edge_count()
        );
        assert_eq!(
            first_resumed.triangulation.face_count(),
            second_resumed.triangulation.face_count()
        );
        assert_eq!(
            first_resumed.triangulation.slice_sizes(),
            second_resumed.triangulation.slice_sizes()
        );
    }

    #[test]
    fn resume_rejects_incompatible_action_config() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::new(2.0, 1.0, 0.1),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::IncompatibleActionConfiguration,
            "action configuration",
        );
    }

    #[test]
    fn resume_rejects_incompatible_sampling_schedule() {
        let checkpoint = short_checkpoint();
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 2).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::IncompatibleMeasurementFrequency,
            "measurement frequency",
        );
    }

    #[test]
    fn resume_rejects_inconsistent_checkpoint_counters() {
        let mut checkpoint = short_checkpoint();
        checkpoint.move_stats.record_attempt(MoveType::Move22);
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::ChainCounterMismatch,
            "chain counters",
        );
    }

    #[test]
    fn resume_rejects_inconsistent_step_telemetry() {
        let mut checkpoint = short_checkpoint();
        checkpoint.steps.pop();
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::StepTelemetryMismatch,
            "step telemetry length",
        );
    }

    #[test]
    fn resume_rejects_nonsequential_step_telemetry() {
        let mut checkpoint = short_checkpoint();
        checkpoint.steps[0].step = 2;
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::StepTelemetryMismatch,
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
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::StepTelemetryMismatch,
            "accepted step count mismatch",
        );
    }

    #[test]
    fn resume_rejects_missing_scheduled_measurement() {
        let mut checkpoint = short_checkpoint();
        checkpoint.measurements.pop();
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::MeasurementTelemetryMismatch,
            "scheduled measurement count mismatch",
        );
    }

    #[test]
    fn resume_rejects_checkpoint_action_mismatch() {
        let mut checkpoint = short_checkpoint();
        checkpoint.current_action += 1.0;
        let algorithm = MetropolisAlgorithm::new(
            MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
            ActionConfig::default(),
        );

        assert_checkpoint_resume_failed(
            algorithm.resume_from_checkpoint(checkpoint),
            CheckpointResumeReason::ActionMismatch,
            "checkpoint action mismatch",
        );
    }

    #[test]
    fn checkpoint_restore_maps_invalid_checkpoint_config_to_resume_failure() {
        let mut checkpoint = short_checkpoint();
        checkpoint.config.temperature = f64::NAN;
        let Err(CdtError::CheckpointResumeFailed { reason, detail }) =
            MetropolisRunState::from_checkpoint(checkpoint)
        else {
            panic!("expected checkpoint configuration failure");
        };

        assert_eq!(reason, CheckpointResumeReason::CheckpointConfiguration);
        assert!(detail.contains("temperature"));
    }

    #[test]
    fn chain_counters_rejects_accepted_above_attempted() {
        let stats = MoveStatistics {
            moves_22_accepted: 1,
            ..MoveStatistics::new()
        };
        let Err(CdtError::CheckpointResumeFailed { reason, detail }) = chain_counters(&stats)
        else {
            panic!("expected impossible move statistics to fail");
        };

        assert_eq!(reason, CheckpointResumeReason::MoveStatisticsInvariant);
        assert!(detail.contains("accepted move count exceeds attempted move count"));
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
            let config = MetropolisConfig::new(1.0, 20, 5, 5).with_seed(seed);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
            algorithm.run(tri).expect("seeded simulation should run")
        };

        let first = run(123);
        let second = run(123);

        assert_eq!(first.steps.len(), second.steps.len());
        for (first, second) in first.steps.iter().zip(second.steps.iter()) {
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
            -2.7,
            epsilon = 1e-12
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move31Remove)
                .expect("3,1 delta should be finite"),
            2.7,
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
    fn accepted_move_retry_exhaustion_is_rejection_result_and_rolls_back() {
        let mut triangulation =
            CdtTriangulation::from_seeded_points(3, 1, 2, 53).expect("Failed to create");
        let action_config = ActionConfig::default();
        let counts_before = simplex_counts(&triangulation);
        let action_before = action_for(&action_config, &triangulation);
        let mut moves = ErgodicsSystem::with_seed(7);

        let result = apply_accepted_move(
            &mut triangulation,
            &mut moves,
            &action_config,
            MoveType::Move31Remove,
            action_before,
        )
        .expect("site retry exhaustion is an ordinary rejection result");

        let AcceptedMoveResult::NoApplicableSite { last_rejection } = result else {
            panic!("Expected NoApplicableSite");
        };
        assert!(matches!(
            last_rejection,
            Some(
                CdtProposalSiteRejection::GeometricViolation | CdtProposalSiteRejection::Kernel(_)
            )
        ));
        assert_eq!(simplex_counts(&triangulation), counts_before);
        assert_relative_eq!(action_for(&action_config, &triangulation), action_before);
    }

    #[test]
    fn run_rejects_zero_frequency() {
        let config = MetropolisConfig::new(1.0, 10, 2, 0);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm.run(tri).unwrap_err();
        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "measurement_frequency");
                assert_eq!(provided_value, "0");
                assert_eq!(expected, "≥ 1");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_bad_temperature() {
        for bad_temp in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let config = MetropolisConfig::new(bad_temp, 10, 2, 2);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

            let err = algorithm.run(tri).unwrap_err();
            match err {
                CdtError::InvalidSimulationConfiguration {
                    setting, expected, ..
                } => {
                    assert_eq!(setting, "temperature", "T={bad_temp}");
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
        let err = MetropolisConfig::new(1.0, 19, 15, 10)
            .validate()
            .expect_err(
                "Configuration should require at least one post-thermalization measurement",
            );

        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "measurement schedule");
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
            .validate()
            .expect_err(
                "Configuration should reject schedules without a reachable post-thermalization measurement",
            );

        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "measurement schedule");
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
        let config = MetropolisConfig::new(1.0, 20, 15, 10).with_seed(42);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let results = algorithm.run(tri).expect("valid schedule should run");
        assert_eq!(results.steps.len(), 20);
        assert!(
            results
                .measurements
                .iter()
                .any(|measurement| measurement.step >= 15)
        );
    }

    #[test]
    fn run_validates_action_config() {
        let config = MetropolisConfig::new(1.0, 20, 15, 10).with_seed(42);
        let action_config = ActionConfig::new(f64::INFINITY, 1.0, 0.1);
        let algorithm = MetropolisAlgorithm::new(config, action_config);
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm
            .run(tri)
            .expect_err("invalid action config should be reported before simulation");
        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "coupling_0");
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
                    assert_eq!(setting, "temperature");
                    assert_eq!(expected, "finite and positive");
                }
                other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
            }
        }
    }

    #[test]
    fn cdt_target_rejects_invalid_action_config() {
        let Err(err) = CdtTarget::new(ActionConfig::new(f64::NAN, 1.0, 0.0), 1.0) else {
            panic!("invalid action config should be rejected");
        };

        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value: _,
                expected,
            } => {
                assert_eq!(setting, "coupling_0");
                assert_eq!(expected, "finite");
            }
            other => panic!("Expected InvalidConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn cdt_proposal_rejects_invalid_action_config() {
        let action_config = ActionConfig::new(1.0, f64::NEG_INFINITY, 0.0);
        let Err(err) = CdtProposal::new(action_config.clone()) else {
            panic!("invalid action config should be rejected");
        };

        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value: _,
                expected,
            } => {
                assert_eq!(setting, "coupling_2");
                assert_eq!(expected, "finite");
            }
            other => panic!("Expected InvalidConfiguration, got {other:?}"),
        }

        assert!(CdtProposal::with_seed(action_config, 7).is_err());
    }

    #[test]
    fn unseeded_config_uses_random_rng() {
        let config = MetropolisConfig::new(1.0, 5, 1, 1); // no seed
        assert!(config.seed.is_none());

        let mut rng = simulation_rng(config.seed);
        let draw = rng.random::<f64>();
        assert!((0.0..1.0).contains(&draw));
    }

    #[test]
    fn cdt_proposal_scores_delayed_plan() {
        let action_config = ActionConfig::default();
        let target =
            CdtTarget::new(action_config.clone(), 1.0).expect("valid target configuration");
        let mut proposal =
            CdtProposal::with_seed(action_config, 7).expect("valid proposal configuration");
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
    fn cdt_proposal_scores_impossible_plan_as_negative_infinity() {
        let action_config = ActionConfig::default();
        let target =
            CdtTarget::new(action_config.clone(), 1.0).expect("valid target configuration");
        let proposal =
            CdtProposal::with_seed(action_config, 7).expect("valid proposal configuration");
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
        let plan = CdtProposalPlan {
            move_type: MoveType::Move31Remove,
            action_before: 1.0,
            action_after: None,
            delta_action: None,
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
        let mut proposal =
            CdtProposal::with_seed(action_config, 7).expect("valid proposal configuration");
        let mut rng = StdRng::seed_from_u64(11);

        let step = chain
            .step_delayed(&target, &mut proposal, &mut rng)
            .expect("ordinary no-site outcomes must be delayed-step rejections, not errors");

        assert_eq!(step.proposed, step.info.is_some());
        assert!(!step.accepted || step.log_prob_after.is_some());
    }

    #[test]
    fn cdt_proposal_commit_applies_concrete_planned_state() {
        let action_config = ActionConfig::default();
        let mut proposal = CdtProposal::with_seed(action_config.clone(), 11)
            .expect("valid proposal configuration");
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
            operation: "set_vertex_data_by_key".to_string(),
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

        let site_rejection = CdtProposalSiteRejection::Kernel(source.clone());
        assert_eq!(
            Error::source(&site_rejection).map(ToString::to_string),
            Some(source.to_string())
        );
    }
}
