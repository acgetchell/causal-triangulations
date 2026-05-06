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
use crate::errors::{CdtError, CdtResult};
use crate::geometry::CdtTriangulation2D;
use crate::util::saturating_usize_to_u32;
use markov_chain_monte_carlo::{DelayedProposal, Target};
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
use std::error::Error;
use std::fmt;
use std::hint::cold_path;
use std::time::Instant;

const ACCEPTED_MOVE_RETRIES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SimplexCounts {
    vertices: u32,
    edges: u32,
    triangles: u32,
}

/// Configuration for the Metropolis-Hastings algorithm.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
/// use causal_triangulations::prelude::simulation::{CdtProposal, CdtTriangulation};
/// use markov_chain_monte_carlo::DelayedProposal;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53)?;
/// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7)?;
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
/// use causal_triangulations::prelude::simulation::{CdtProposal, CdtTriangulation};
/// use markov_chain_monte_carlo::DelayedProposal;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53)?;
/// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7)?;
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
/// use causal_triangulations::prelude::simulation::{CdtProposal, CdtTriangulation};
/// use markov_chain_monte_carlo::DelayedProposal;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53)?;
/// let mut proposal = CdtProposal::new(ActionConfig::default())?;
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
    /// configuration is invalid, [`CdtError::MetropolisMoveApplicationFailed`]
    /// if an accepted move causes a hard backend mutation failure, or a
    /// validation error for unrecoverable triangulation failures. Accepted move
    /// types that cannot find a realizable local site after bounded retries are
    /// recorded as rejected proposals.
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
    pub fn run(
        &self,
        mut triangulation: CdtTriangulation2D,
    ) -> CdtResult<SimulationResultsBackend> {
        // Validate configuration to fail fast before any work
        self.config.validate()?;
        self.action_config.validate()?;

        let mut rng = simulation_rng(self.config.seed);
        let mut moves = self.config.seed.map_or_else(ErgodicsSystem::new, |seed| {
            ErgodicsSystem::with_seed(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
        });
        let start = Instant::now();
        let mut move_stats = MoveStatistics::new();
        let mut steps = Vec::with_capacity(usize::try_from(self.config.steps).unwrap_or(0));
        let mut measurements = Vec::new();

        let mut current_action = action_for(&self.action_config, &triangulation);
        measurements.push(measurement_for(0, current_action, &triangulation));
        triangulation.record_event(SimulationEvent::MeasurementTaken {
            step: 0,
            action: current_action,
        });

        for step in 1..=self.config.steps {
            let move_type = moves.select_random_move();
            move_stats.record_attempt(move_type);
            triangulation.record_event(SimulationEvent::MoveAttempted {
                move_type: format!("{move_type:?}"),
                step: step.into(),
            });

            let action_before = current_action;
            let delta_action = proposed_delta_action(
                &self.action_config,
                simplex_counts(&triangulation),
                move_type,
            );

            let mut accepted = false;
            let mut action_after = None;
            if let Some(delta) = delta_action
                && metropolis_accept(delta, self.config.temperature, &mut rng)
            {
                match apply_accepted_move(
                    &mut triangulation,
                    &mut moves,
                    &self.action_config,
                    move_type,
                    action_before,
                ) {
                    Ok(AcceptedMoveResult::Applied {
                        action_after: applied_action,
                    }) => {
                        accepted = true;
                        action_after = Some(applied_action);
                        current_action = applied_action;
                        move_stats.record_success(move_type);
                        triangulation.record_event(SimulationEvent::MoveAccepted {
                            move_type: format!("{move_type:?}"),
                            step: step.into(),
                            action_change: applied_action - action_before,
                        });
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

            steps.push(MonteCarloStep {
                step,
                move_type,
                accepted,
                action_before,
                action_after,
                delta_action,
            });

            if step.is_multiple_of(self.config.measurement_frequency) {
                measurements.push(measurement_for(step, current_action, &triangulation));
                triangulation.record_event(SimulationEvent::MeasurementTaken {
                    step: step.into(),
                    action: current_action,
                });
            }
        }

        Ok(SimulationResultsBackend {
            config: self.config.clone(),
            action_config: self.action_config.clone(),
            move_stats,
            steps,
            measurements,
            elapsed_time: start.elapsed(),
            triangulation,
        })
    }
}

/// Builds the RNG used only for Metropolis acceptance draws.
///
/// This keeps acceptance randomness separate from move-site selection, so seeded
/// simulations are reproducible while unseeded simulations still draw fresh entropy.
fn simulation_rng(seed: Option<u64>) -> StdRng {
    seed.map_or_else(rand::make_rng, StdRng::seed_from_u64)
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
fn metropolis_accept(delta_action: f64, temperature: f64, rng: &mut StdRng) -> bool {
    delta_action <= 0.0 || rng.random::<f64>() < (-delta_action / temperature).exp()
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
        let snapshot = triangulation.clone();
        let result = attempt_move(moves, move_type, triangulation);
        let rejection = match result {
            MoveResult::Success => {
                let action_after = action_for(action_config, triangulation);
                return Ok(AcceptedMoveResult::Applied { action_after });
            }
            MoveResult::HardFailure(err) => {
                *triangulation = snapshot;
                return Err(MoveApplicationError {
                    attempt,
                    source: err,
                });
            }
            MoveResult::CausalityViolation => CdtProposalSiteRejection::CausalityViolation,
            MoveResult::GeometricViolation => CdtProposalSiteRejection::GeometricViolation,
            MoveResult::Rejected(err) => CdtProposalSiteRejection::Kernel(err),
        };
        *triangulation = snapshot;
        last_rejection = Some(rejection);
    }

    debug_assert!(
        (action_for(action_config, triangulation) - action_before).abs() < f64::EPSILON,
        "failed accepted move retries must leave the triangulation rolled back"
    );
    Ok(AcceptedMoveResult::NoApplicableSite { last_rejection })
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
    last_failure: String,
) -> CdtError {
    CdtError::MetropolisMoveApplicationFailed {
        step,
        move_type: format!("{move_type:?}"),
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

    fn assert_optional_relative_eq(left: Option<f64>, right: Option<f64>) {
        match (left, right) {
            (Some(left), Some(right)) => assert_relative_eq!(left, right, epsilon = 1e-12),
            (None, None) => {}
            other => panic!("expected matching optional floats, got {other:?}"),
        }
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
    fn explicit_cdt_volume_profiles_count_time_slabs() {
        let strip = CdtTriangulation::from_cdt_strip(4, 3).expect("create explicit strip");
        assert_eq!(strip.volume_profile(), vec![6, 6, 0]);

        let torus = CdtTriangulation::from_toroidal_cdt(3, 3).expect("create explicit torus");
        assert_eq!(torus.volume_profile(), vec![6, 6, 6]);
    }

    #[test]
    fn measurement_records_volume_profile_for_foliated_triangulation() {
        let triangulation = CdtTriangulation::from_cdt_strip(4, 3).expect("create explicit strip");
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
