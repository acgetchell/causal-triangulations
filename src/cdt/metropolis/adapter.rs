#![forbid(unsafe_code)]

//! Adapter boundary between CDT state and `markov-chain-monte-carlo`.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveType, proposal_site_count};
use crate::errors::{CdtError, CdtResult, MetropolisMoveApplicationFailure};
use crate::geometry::CdtTriangulation2D;
use markov_chain_monte_carlo::{
    Chain, ChainCheckpoint, DelayedProposal, DiscreteProposalRatio, McmcError, Target,
};
use rand::Rng;
use std::error::Error;
use std::fmt;
use std::hint::cold_path;

use super::helpers::{action_for, proposed_delta_action, simplex_counts, validate_temperature};
use super::telemetry::{CdtProposalSiteRejection, ProposalStatistics};

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
    /// use causal_triangulations::prelude::simulation::{ActionConfig, CdtTarget};
    ///
    /// let _target = CdtTarget::new(ActionConfig::default(), 1.0)?;
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn new(action_config: ActionConfig, temperature: f64) -> CdtResult<Self> {
        action_config.validate();
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

/// Concrete CDT proposal plan selected before committing live state.
///
/// A plan records the selected [`MoveType`], the action before and after the
/// move, and a cloned triangulation containing the proposed mutation. Planning
/// may mutate that clone to realize a concrete local site, but it never mutates
/// the live simulation state. The sampler scores this plan with the
/// Metropolis-Hastings forward/reverse proposal-site ratio, then commits the
/// cloned state only if the Metropolis step accepts it. This planning boundary
/// is the natural hook for future adaptive or self-learning proposal selection.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal, MoveType,
/// };
/// use rand::{SeedableRng, rngs::StdRng};
/// use std::assert_matches;
///
/// # fn main() -> CdtResult<()> {
/// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
/// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
/// let mut rng = StdRng::seed_from_u64(11);
///
/// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
///     return Ok(());
/// };
/// assert_matches!(
///     plan.move_type(),
///     MoveType::Move22 | MoveType::Move13Add | MoveType::Move31Remove | MoveType::EdgeFlip
/// );
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
    pub(crate) move_type: MoveType,
    pub(crate) action_before: f64,
    pub(crate) action_after: Option<f64>,
    pub(crate) delta_action: Option<f64>,
    pub(crate) forward_site_count: usize,
    /// Reverse proposal-site denominator for the realized proposed state.
    ///
    /// This is the number of valid inverse-move local sites used to normalize
    /// the reverse proposal probability in the Metropolis-Hastings site-count
    /// ratio. It is a count of sites and must be greater than zero for a
    /// realized proposal to have finite reverse weight.
    pub(crate) reverse_site_count: usize,
    pub(crate) proposed_state: CdtTriangulation2D,
}

impl CdtProposalPlan {
    /// Returns the proposed move type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal, MoveType,
    /// };
    /// use rand::{SeedableRng, rngs::StdRng};
    /// use std::assert_matches;
    ///
    /// # fn main() -> CdtResult<()> {
    /// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
    ///     return Ok(());
    /// };
    /// assert_matches!(
    ///     plan.move_type(),
    ///     MoveType::Move22 | MoveType::Move13Add | MoveType::Move31Remove | MoveType::EdgeFlip
    /// );
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn move_type(&self) -> MoveType {
        self.move_type
    }

    /// Returns the current action used to score this proposal.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal,
    /// };
    /// use rand::{SeedableRng, rngs::StdRng};
    ///
    /// # fn main() -> CdtResult<()> {
    /// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
    ///     return Ok(());
    /// };
    /// assert!(plan.action_before().is_finite());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn action_before(&self) -> f64 {
        self.action_before
    }

    /// Returns the action of the concrete proposed state, if one was realized.
    ///
    /// A value of `None` means the selected move could not be scored or
    /// realized, so [`DelayedProposal::proposed_log_prob`] treats the plan as
    /// impossible.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal,
    /// };
    /// use rand::{SeedableRng, rngs::StdRng};
    ///
    /// # fn main() -> CdtResult<()> {
    /// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
    ///     return Ok(());
    /// };
    /// assert_eq!(plan.action_after().is_some(), plan.delta_action().is_some());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn action_after(&self) -> Option<f64> {
        self.action_after
    }

    /// Returns the concrete proposal action change, if it can be evaluated.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal,
    /// };
    /// use rand::{SeedableRng, rngs::StdRng};
    ///
    /// # fn main() -> CdtResult<()> {
    /// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
    ///     return Ok(());
    /// };
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
    #[must_use]
    pub const fn delta_action(&self) -> Option<f64> {
        self.delta_action
    }
}

/// Telemetry returned by planned CDT proposal steps.
///
/// The sampler receives this compact record after a plan has been scored. It is
/// intended for diagnostics and measurement backends that need to report which
/// move family was proposed without exposing the private plan fields.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal,
/// };
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> CdtResult<()> {
/// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
/// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
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

/// Error reported by planned CDT proposal planning or commit.
///
/// No-site outcomes are ordinary proposal absence and are reported from
/// [`DelayedProposal::propose_plan`] as `Ok(None)`, matching the upstream
/// plan-before-commit contract. `ApplicationFailed` represents a hard backend or
/// invariant failure while constructing or committing a concrete proposal, and
/// preserves the typed [`CdtError`] that caused the failed application.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{BackendMutationOperation, CdtError};
/// use causal_triangulations::prelude::simulation::{CdtProposalError, MoveType};
///
/// let err = CdtProposalError::ApplicationFailed {
///     move_type: MoveType::Move13Add,
///     attempt: 2,
///     source: CdtError::BackendMutationFailed {
///         operation: BackendMutationOperation::SetVertexDataByKey,
///         target: "vertex VertexKey(7)".to_string(),
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
        /// Move type whose concrete application failed.
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

impl From<CdtProposalError> for CdtError {
    fn from(error: CdtProposalError) -> Self {
        match error {
            CdtProposalError::ApplicationFailed {
                move_type,
                attempt,
                source,
            } => Self::ProposalApplicationFailed {
                move_type,
                attempt,
                source: MetropolisMoveApplicationFailure::from(source),
            },
        }
    }
}

/// Planned CDT proposal distribution.
///
/// This adapter exposes CDT's clone-plan-score-commit move ordering through the
/// upstream [`DelayedProposal`] API. It plans a concrete local move on a cloned
/// triangulation, scores the proposed state with the same [`ActionConfig`] as
/// the matching [`CdtTarget`] or [`MetropolisAlgorithm`](super::MetropolisAlgorithm), corrects for
/// forward/reverse proposal-site counts, and commits the clone only after
/// acceptance. Future self-learning Metropolis-Hastings proposal policies
/// should plug into this planner boundary instead of mutating the live chain
/// before acceptance.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, DelayedProposal,
/// };
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> CdtResult<()> {
/// let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
/// let mut proposal = CdtProposal::new(ActionConfig::default());
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
    last_step_info: Option<CdtProposalInfo>,
    last_no_plan_info: Option<CdtProposalInfo>,
    last_proposal_stats: ProposalStatistics,
}

impl CdtProposal {
    /// Creates a new unseeded CDT proposal planner.
    ///
    /// Proposed-state scoring is delegated to the target passed to
    /// [`DelayedProposal::proposed_log_prob`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{ActionConfig, CdtProposal};
    ///
    /// let _proposal = CdtProposal::new(ActionConfig::default());
    /// ```
    #[must_use]
    pub fn new(action_config: ActionConfig) -> Self {
        action_config.validate();
        Self {
            action_config,
            moves: ErgodicsSystem::new(),
            last_step_info: None,
            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }

    /// Creates a seeded CDT proposal planner.
    ///
    /// The seed controls the internal move-family selector. The `rng` passed to
    /// [`DelayedProposal::propose_plan`] is still accepted for compatibility
    /// with generic MCMC drivers.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{ActionConfig, CdtProposal};
    ///
    /// let _proposal = CdtProposal::with_seed(ActionConfig::default(), 42);
    /// ```
    #[must_use]
    pub fn with_seed(action_config: ActionConfig, seed: u64) -> Self {
        action_config.validate();
        Self {
            action_config,
            moves: ErgodicsSystem::with_seed(seed),
            last_step_info: None,
            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }

    /// Rebuilds a proposal planner from checkpointed ergodic-move state.
    ///
    /// Resumed simulations use this to hand the upstream sampler the exact
    /// proposal RNG stream stored in a CDT checkpoint while resetting
    /// per-step telemetry caches.
    pub(crate) fn from_ergodics(action_config: ActionConfig, moves: ErgodicsSystem) -> Self {
        action_config.validate();
        Self {
            action_config,
            moves,
            last_step_info: None,
            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }

    /// Extracts the ergodic-move state after upstream sampler execution.
    ///
    /// The caller writes the returned state back into the CDT checkpoint/run
    /// state so later chunks continue from the same proposal RNG stream.
    pub(crate) fn into_ergodics(self) -> ErgodicsSystem {
        self.moves
    }

    /// Returns telemetry recorded by the most recent planned proposal attempt.
    ///
    /// [`MetropolisAlgorithm`](super::runner::MetropolisAlgorithm) merges this
    /// snapshot into CDT-owned proposal counters after the upstream sampler
    /// reports the planned-step outcome.
    pub(crate) const fn last_proposal_stats(&self) -> &ProposalStatistics {
        &self.last_proposal_stats
    }

    /// Returns proposal metadata recorded by the most recent planned sampler step.
    ///
    /// CDT step history and move statistics depend on this metadata even for
    /// self-loop proposals, so missing values are translated into an explicit
    /// telemetry error during runner bookkeeping.
    pub(crate) const fn last_step_info(&self) -> Option<CdtProposalInfo> {
        self.last_step_info
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
        let no_plan_info = CdtProposalInfo {
            move_type,
            action_before,
            action_after: None,
            delta_action: None,
        };
        let mut proposal_stats = ProposalStatistics::new();
        let plan = match propose_concrete_plan(
            state,
            &mut self.moves,
            &mut proposal_stats,
            &self.action_config,
            move_type,
            action_before,
        ) {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                self.last_step_info = Some(no_plan_info);
                self.last_no_plan_info = Some(no_plan_info);
                self.last_proposal_stats = proposal_stats;
                cold_path();
                return Ok(None);
            }
            Err(err) => {
                self.last_step_info = Some(no_plan_info);
                self.last_no_plan_info = None;
                proposal_stats.record_hard_failure();
                self.last_proposal_stats = proposal_stats;
                cold_path();
                return Err(CdtProposalError::ApplicationFailed {
                    move_type,
                    attempt: err.attempt,
                    source: err.source,
                });
            }
        };
        self.last_step_info = Some(CdtProposalInfo {
            move_type: plan.move_type,
            action_before: plan.action_before,
            action_after: plan.action_after,
            delta_action: plan.delta_action,
        });
        self.last_no_plan_info = None;
        self.last_proposal_stats = proposal_stats;
        Ok(Some(plan))
    }

    fn no_plan_info(&mut self) -> Option<Self::Info> {
        self.last_no_plan_info.take()
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

    fn log_q_ratio(
        &self,
        state: &CdtTriangulation2D,
        plan: &Self::Plan,
    ) -> Result<f64, Self::Error> {
        Ok(concrete_log_q_ratio(state, plan))
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MoveApplicationError {
    pub(crate) attempt: usize,
    pub(crate) source: CdtError,
}

/// Plans one concrete CDT proposal without mutating the live chain state.
///
/// The helper samples a local site, applies it to a cloned triangulation, and
/// records the forward and reverse proposal-site counts needed for the
/// Hastings correction. Ordinary no-site, causality, geometry, and recoverable
/// backend rejections return `Ok(None)` so the public planned-proposal API can
/// expose them as self-loop proposals.
///
/// # Errors
///
/// Returns [`MoveApplicationError`] only for hard backend or invariant failures
/// that must surface through [`CdtProposalError::ApplicationFailed`].
pub(crate) fn propose_concrete_plan(
    state: &CdtTriangulation2D,
    moves: &mut ErgodicsSystem,
    proposal_stats: &mut ProposalStatistics,
    action_config: &ActionConfig,
    move_type: MoveType,
    action_before: f64,
) -> Result<Option<CdtProposalPlan>, MoveApplicationError> {
    if proposed_delta_action(action_config, simplex_counts(state), move_type).is_none() {
        proposal_stats.record_move_family(0);
        proposal_stats.record_no_site();
        return Ok(None);
    }
    let selection = moves.select_proposal_site(state, move_type);
    let forward_site_count = selection.site_count;
    proposal_stats.record_move_family(forward_site_count);
    let Some(site) = selection.site else {
        proposal_stats.record_no_site();
        return Ok(None);
    };

    let mut proposed_state = state.clone();
    let move_stats_before = moves.stats().clone();
    let result = moves.apply_proposal_site(&mut proposed_state, move_type, site);
    moves.replace_stats(move_stats_before);
    let action_after = match result {
        MoveResult::Success => action_for(action_config, &proposed_state),
        MoveResult::HardFailure(err) => {
            return Err(MoveApplicationError {
                attempt: 1,
                source: err,
            });
        }
        MoveResult::CausalityViolation => {
            proposal_stats.record_site_rejection(&CdtProposalSiteRejection::CausalityViolation);
            return Ok(None);
        }
        MoveResult::GeometricViolation => {
            proposal_stats.record_site_rejection(&CdtProposalSiteRejection::GeometricViolation);
            return Ok(None);
        }
        MoveResult::Rejected(err) => {
            proposal_stats.record_site_rejection(&CdtProposalSiteRejection::Kernel(err));
            return Ok(None);
        }
    };
    let delta_action = action_after - action_before;
    let reverse_site_count = proposal_site_count(&proposed_state, reverse_move_type(move_type));

    Ok(Some(CdtProposalPlan {
        move_type,
        action_before,
        action_after: Some(action_after),
        delta_action: Some(delta_action),
        forward_site_count,
        reverse_site_count,
        proposed_state,
    }))
}

/// Computes the Hastings proposal-density correction for a concrete plan.
///
/// The ratio uses the instantaneous forward and reverse local-site counts from
/// the selected move family. Zero denominators represent impossible proposal
/// weights and are scored as negative infinity rather than panicking.
pub(crate) fn concrete_log_q_ratio(_state: &CdtTriangulation2D, plan: &CdtProposalPlan) -> f64 {
    DiscreteProposalRatio::from_counts(plan.forward_site_count, plan.reverse_site_count)
        .map_or(f64::NEG_INFINITY, DiscreteProposalRatio::log_q_ratio)
}

/// Restores a checkpointed triangulation through the upstream MCMC chain type.
///
/// The conversion reuses `markov-chain-monte-carlo` target validation before
/// CDT resume logic rebuilds domain-specific run state.
///
/// # Errors
///
/// Returns an upstream checkpoint error when the checkpointed state is
/// incompatible with the supplied [`CdtTarget`].
pub(crate) fn restore_checkpoint_state(
    checkpoint: ChainCheckpoint<CdtTriangulation2D>,
    target: &CdtTarget,
) -> Result<CdtTriangulation2D, McmcError> {
    Chain::from_checkpoint(checkpoint, target).map(Chain::into_state)
}

/// Returns the inverse CDT move family used for reverse proposal accounting.
const fn reverse_move_type(move_type: MoveType) -> MoveType {
    match move_type {
        MoveType::Move22 => MoveType::Move22,
        MoveType::Move13Add => MoveType::Move31Remove,
        MoveType::Move31Remove => MoveType::Move13Add,
        MoveType::EdgeFlip => MoveType::EdgeFlip,
    }
}
