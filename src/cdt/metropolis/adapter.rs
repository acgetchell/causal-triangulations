#![forbid(unsafe_code)]

//! Adapter boundary between CDT state and `markov-chain-monte-carlo`.

use super::helpers::{action_for, proposed_delta_action, simplex_counts, validate_temperature};
use super::telemetry::{
    CdtProposalPlanningOutcome, CdtProposalSiteRejection, ProposalKernelTelemetry,
    ProposalStatistics,
};
use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveType};
use crate::cdt::proposal_policy::{
    CdtMoveFamilyDistribution, CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError,
    CdtProposalPolicyView, UniformCdtMoveFamilyPolicy,
};
use crate::errors::{CdtError, CdtResult, MetropolisMoveApplicationFailure};
use crate::geometry::CdtTriangulation2D;
use markov_chain_monte_carlo::{
    Chain, ChainCheckpoint, DelayedProposal, DiscreteProposalRatio, DiscreteProposalRatioError,
    McmcError, Target,
};
use rand::Rng;
use std::error::Error;
use std::fmt;
use std::hint::cold_path;

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
/// cloned state only if the Metropolis step accepts it. Injected family policies
/// evaluate through invariant-safe borrowed views at this planning boundary.
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
/// assert_eq!(plan.reverse_move_type(), plan.move_type().reverse());
/// assert_eq!(plan.forward_family_probability(), 0.25);
/// assert_eq!(plan.reverse_family_probability(), 0.25);
/// assert!(plan.forward_site_count() > 0);
/// assert!(plan.reverse_site_count() > 0);
/// assert!(plan.action_before().is_finite());
/// approx::assert_relative_eq!(
///     plan.action_after(),
///     plan.action_before() + plan.delta_action(),
///     epsilon = 1e-12
/// );
/// approx::assert_relative_eq!(
///     plan.log_proposal_ratio(),
///     plan.log_family_probability_ratio() + plan.log_site_count_ratio(),
///     epsilon = 1e-12
/// );
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CdtProposalPlan {
    pub(crate) move_type: MoveType,
    pub(crate) action_before: f64,
    pub(crate) action_after: f64,
    pub(crate) delta_action: f64,
    pub(crate) forward_family_probability: f64,
    pub(crate) reverse_family_probability: f64,
    pub(crate) forward_site_count: usize,
    /// Reverse proposal-site denominator for the realized proposed state.
    ///
    /// This is the number of valid inverse-move local sites used to normalize
    /// the reverse proposal probability in the Metropolis-Hastings site-count
    /// ratio. It is a count of sites and must be greater than zero for a
    /// realized proposal to have finite reverse weight.
    pub(crate) reverse_site_count: usize,
    pub(crate) log_proposal_ratio: f64,
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

    /// Returns the action of the concrete proposed state.
    ///
    /// Concrete plans are only constructed after a selected move has been
    /// realized and scored.
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
    /// assert!(plan.action_after().is_finite());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn action_after(&self) -> f64 {
        self.action_after
    }

    /// Returns the concrete proposal action change.
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
    /// approx::assert_relative_eq!(plan.action_after(), plan.action_before() + plan.delta_action());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn delta_action(&self) -> f64 {
        self.delta_action
    }

    /// Returns the family that proposes the inverse transition.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert_eq!(plan.reverse_move_type(), plan.move_type().reverse());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn reverse_move_type(&self) -> MoveType {
        self.move_type.reverse()
    }

    /// Returns `p(m | x)`, evaluated on the pre-move state.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert_eq!(plan.forward_family_probability(), 0.25);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn forward_family_probability(&self) -> f64 {
        self.forward_family_probability
    }

    /// Returns `p(reverse(m) | y)`, evaluated on the planned post-move state.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert_eq!(plan.reverse_family_probability(), 0.25);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn reverse_family_probability(&self) -> f64 {
        self.reverse_family_probability
    }

    /// Returns the canonical offered-site count `|S_m(x)|`.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert!(plan.forward_site_count() > 0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn forward_site_count(&self) -> usize {
        self.forward_site_count
    }

    /// Returns the canonical reverse offered-site count `|S_reverse(m)(y)|`.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert!(plan.reverse_site_count() > 0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn reverse_site_count(&self) -> usize {
        self.reverse_site_count
    }

    /// Returns `log(p(reverse(m) | y) / p(m | x))`.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert_eq!(plan.log_family_probability_ratio(), 0.0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn log_family_probability_ratio(&self) -> f64 {
        DiscreteProposalRatio::new(
            self.forward_family_probability,
            1,
            self.reverse_family_probability,
            1,
        )
        .map_or(f64::NEG_INFINITY, DiscreteProposalRatio::log_q_ratio)
    }

    /// Returns `log(|S_m(x)| / |S_reverse(m)(y)|)`.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert!(plan.log_site_count_ratio().is_finite());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn log_site_count_ratio(&self) -> f64 {
        DiscreteProposalRatio::from_counts(self.forward_site_count, self.reverse_site_count)
            .map_or(f64::NEG_INFINITY, DiscreteProposalRatio::log_q_ratio)
    }

    /// Returns the complete family-plus-site Hastings correction.
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
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     approx::assert_relative_eq!(
    ///         plan.log_proposal_ratio(),
    ///         plan.log_family_probability_ratio() + plan.log_site_count_ratio(),
    ///         epsilon = 1e-12
    ///     );
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn log_proposal_ratio(&self) -> f64 {
        self.log_proposal_ratio
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
/// assert_eq!(info.reverse_move_type, plan.reverse_move_type());
/// assert_eq!(info.forward_site_count, plan.forward_site_count());
/// assert_eq!(info.reverse_site_count, Some(plan.reverse_site_count()));
/// assert_eq!(info.log_proposal_ratio, Some(plan.log_proposal_ratio()));
/// assert!(info.delta_action.is_some());
/// if let Some(delta_action) = info.delta_action {
///     approx::assert_relative_eq!(delta_action, plan.delta_action(), epsilon = 1e-12);
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
    /// Family needed to propose the inverse transition.
    pub reverse_move_type: MoveType,
    /// Forward family probability evaluated on the pre-move state.
    pub forward_family_probability: f64,
    /// Reverse-family probability evaluated on the planned post-move state.
    pub reverse_family_probability: Option<f64>,
    /// Canonical offered-site count for the selected family in the pre-move state.
    pub forward_site_count: usize,
    /// Canonical offered-site count for the reverse family in the post-move state.
    pub reverse_site_count: Option<usize>,
    /// Family-probability contribution to the log Hastings correction.
    pub log_family_probability_ratio: Option<f64>,
    /// Offered-site-count contribution to the log Hastings correction.
    pub log_site_count_ratio: Option<f64>,
    /// Complete family-plus-site log Hastings correction.
    pub log_proposal_ratio: Option<f64>,
    /// CDT-local result of attempting to realize the selected family and site.
    pub planning_outcome: CdtProposalPlanningOutcome,
}

impl CdtProposalInfo {
    /// Returns the policy and proposal-density audit record for this attempt.
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
    /// let plan = proposal.propose_plan(&tri, &mut rng)?;
    /// let info = match &plan {
    ///     Some(plan) => proposal.info(plan),
    ///     None => {
    ///         let Some(info) = proposal.no_plan_info() else {
    ///             return Ok(());
    ///         };
    ///         info
    ///     }
    /// };
    /// let telemetry = info.proposal_telemetry();
    /// assert_eq!(telemetry.selected_family(), info.move_type);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn proposal_telemetry(self) -> ProposalKernelTelemetry {
        ProposalKernelTelemetry::new(
            self.move_type,
            self.reverse_move_type,
            self.forward_family_probability,
            self.reverse_family_probability,
            self.forward_site_count,
            self.reverse_site_count,
            self.planning_outcome,
        )
    }
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
    /// The injected family policy could not produce a checked distribution.
    Policy {
        /// Typed policy evaluation or normalization failure.
        source: CdtMoveFamilyPolicyError,
    },
    /// The upstream MCMC proposal-ratio boundary rejected planned components.
    ProposalRatio {
        /// Selected forward move family.
        move_type: MoveType,
        /// Typed upstream family/site ratio construction failure.
        source: DiscreteProposalRatioError,
    },
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
            Self::Policy { source } => write!(f, "CDT move-family policy failed: {source}"),
            Self::ProposalRatio { move_type, source } => write!(
                f,
                "failed to construct the proposal ratio for {move_type:?}: {source}"
            ),
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
            Self::Policy { source } => Some(source),
            Self::ProposalRatio { source, .. } => Some(source),
            Self::ApplicationFailed { source, .. } => Some(source),
        }
    }
}

impl From<CdtProposalError> for CdtError {
    fn from(error: CdtProposalError) -> Self {
        match error {
            CdtProposalError::Policy { source } => Self::ProposalPolicyFailed { source },
            CdtProposalError::ProposalRatio { move_type, source } => {
                Self::ProposalRatioFailed { move_type, source }
            }
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
/// acceptance. Uniform, fixed weighted, and state-dependent family policies all
/// use this planner boundary instead of mutating the live chain before
/// acceptance.
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
pub struct CdtProposal<P = UniformCdtMoveFamilyPolicy> {
    action_config: ActionConfig,
    moves: ErgodicsSystem,
    policy: P,
    last_step_info: Option<CdtProposalInfo>,
    last_no_plan_info: Option<CdtProposalInfo>,
    last_proposal_stats: ProposalStatistics,
}

impl CdtProposal<UniformCdtMoveFamilyPolicy> {
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
            policy: UniformCdtMoveFamilyPolicy,
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
            policy: UniformCdtMoveFamilyPolicy,
            last_step_info: None,
            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }
}

impl<P> CdtProposal<P>
where
    P: CdtMoveFamilyPolicy,
{
    /// Creates an unseeded planner with an injected family policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMoveFamilyDistribution, CdtProposal,
    /// };
    ///
    /// let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])?;
    /// let _proposal = CdtProposal::with_policy(ActionConfig::default(), policy);
    /// # Ok::<(), causal_triangulations::CdtMoveFamilyPolicyError>(())
    /// ```
    #[must_use]
    pub fn with_policy(action_config: ActionConfig, policy: P) -> Self {
        action_config.validate();
        Self {
            action_config,
            moves: ErgodicsSystem::new(),
            policy,
            last_step_info: None,
            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }

    /// Creates a deterministically seeded planner with an injected family policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMoveFamilyDistribution, CdtProposal,
    /// };
    ///
    /// let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])?;
    /// let _proposal = CdtProposal::with_seed_and_policy(ActionConfig::default(), 42, policy);
    /// # Ok::<(), causal_triangulations::CdtMoveFamilyPolicyError>(())
    /// ```
    #[must_use]
    pub fn with_seed_and_policy(action_config: ActionConfig, seed: u64, policy: P) -> Self {
        action_config.validate();
        Self {
            action_config,
            moves: ErgodicsSystem::with_seed(seed),
            policy,
            last_step_info: None,
            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }

    /// Returns CDT's canonical borrowed policy view for one move family.
    ///
    /// This is the public inspection boundary for conventional and external
    /// family policies. The returned view borrows `state` and this proposal's
    /// versioned site cache, exposes no mutable geometry, and does not clone the
    /// triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtProposal, CdtResult, CdtTriangulation, MoveType,
    /// };
    ///
    /// # fn main() -> CdtResult<()> {
    /// let state = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
    /// let view = proposal.policy_view(&state, MoveType::Move13Add);
    /// assert_eq!(view.reverse_family(), MoveType::Move31Remove);
    /// assert_eq!(view.offered_sites().len(), view.offered_site_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn policy_view<'a>(
        &'a mut self,
        state: &'a CdtTriangulation2D,
        family: MoveType,
    ) -> CdtProposalPolicyView<'a> {
        self.moves.proposal_policy_view(state, family)
    }

    /// Rebuilds a proposal planner from checkpointed ergodic-move state.
    ///
    /// Resumed simulations use this to hand the upstream sampler the exact
    /// proposal RNG stream stored in a CDT checkpoint while resetting
    /// per-step telemetry caches.
    pub(crate) fn from_ergodics_with_policy(
        action_config: ActionConfig,
        moves: ErgodicsSystem,
        policy: P,
    ) -> Self {
        action_config.validate();
        Self {
            action_config,
            moves,
            policy,
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

/// Evaluates and normalizes one complete family distribution for `state`.
///
/// Fixed policies return their already checked distribution without touching
/// proposal-site caches. State-dependent policies receive every canonical
/// family view, including families with no offered sites. This deliberately
/// prevents availability-based renormalization from changing the proposal
/// kernel after policy evaluation.
fn evaluate_policy_distribution<P>(
    policy: &P,
    moves: &mut ErgodicsSystem,
    state: &CdtTriangulation2D,
) -> Result<CdtMoveFamilyDistribution, CdtMoveFamilyPolicyError>
where
    P: CdtMoveFamilyPolicy + ?Sized,
{
    if let Some(distribution) = policy.fixed_distribution() {
        return Ok(distribution);
    }

    let mut weights = [0.0; 4];
    for (index, family) in MoveType::REVERSIBLE_1P1.into_iter().enumerate() {
        let view = moves.proposal_policy_view(state, family);
        weights[index] = policy.family_weight(&view)?;
    }
    CdtMoveFamilyDistribution::from_weights(weights)
}

/// Builds typed telemetry for a selected family that produced no concrete plan.
const fn no_plan_info(
    move_type: MoveType,
    action_before: f64,
    forward_family_probability: f64,
    forward_site_count: usize,
    planning_outcome: CdtProposalPlanningOutcome,
) -> CdtProposalInfo {
    CdtProposalInfo {
        move_type,
        action_before,
        action_after: None,
        delta_action: None,
        reverse_move_type: move_type.reverse(),
        forward_family_probability,
        reverse_family_probability: None,
        forward_site_count,
        reverse_site_count: None,
        log_family_probability_ratio: None,
        log_site_count_ratio: None,
        log_proposal_ratio: None,
        planning_outcome,
    }
}

/// Builds complete public telemetry from an invariant-bearing concrete plan.
fn plan_info(plan: &CdtProposalPlan) -> CdtProposalInfo {
    CdtProposalInfo {
        move_type: plan.move_type,
        action_before: plan.action_before,
        action_after: Some(plan.action_after),
        delta_action: Some(plan.delta_action),
        reverse_move_type: plan.reverse_move_type(),
        forward_family_probability: plan.forward_family_probability,
        reverse_family_probability: Some(plan.reverse_family_probability),
        forward_site_count: plan.forward_site_count,
        reverse_site_count: Some(plan.reverse_site_count),
        log_family_probability_ratio: Some(plan.log_family_probability_ratio()),
        log_site_count_ratio: Some(plan.log_site_count_ratio()),
        log_proposal_ratio: Some(plan.log_proposal_ratio),
        planning_outcome: CdtProposalPlanningOutcome::ConcretePlan,
    }
}

impl<P> DelayedProposal<CdtTriangulation2D> for CdtProposal<P>
where
    P: CdtMoveFamilyPolicy,
{
    type Plan = CdtProposalPlan;
    type Info = CdtProposalInfo;
    type Error = CdtProposalError;

    fn propose_plan<R: Rng + ?Sized>(
        &mut self,
        state: &CdtTriangulation2D,
        _rng: &mut R,
    ) -> Result<Option<Self::Plan>, Self::Error> {
        self.last_step_info = None;
        self.last_no_plan_info = None;
        self.last_proposal_stats = ProposalStatistics::new();
        let distribution = evaluate_policy_distribution(&self.policy, &mut self.moves, state)
            .map_err(|source| CdtProposalError::Policy { source })?;
        let move_type = self.moves.select_move_family(&distribution);
        let forward_family_probability = distribution.probability(move_type);
        let action_before = action_for(&self.action_config, state);
        let mut proposal_stats = ProposalStatistics::new();
        let mut plan = match propose_concrete_plan(
            state,
            &mut self.moves,
            &mut proposal_stats,
            &self.action_config,
            move_type,
            action_before,
        ) {
            Ok(ConcretePlanAttempt {
                plan: Some(plan), ..
            }) => plan,
            Ok(ConcretePlanAttempt {
                plan: None,
                planning_outcome,
                forward_site_count,
            }) => {
                let no_plan_info = no_plan_info(
                    move_type,
                    action_before,
                    forward_family_probability,
                    forward_site_count,
                    planning_outcome,
                );
                self.last_step_info = Some(no_plan_info);
                self.last_no_plan_info = Some(no_plan_info);
                self.last_proposal_stats = proposal_stats;
                cold_path();
                return Ok(None);
            }
            Err(err) => {
                self.last_step_info = None;
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
        let reverse_distribution =
            match evaluate_policy_distribution(&self.policy, &mut self.moves, &plan.proposed_state)
            {
                Ok(distribution) => distribution,
                Err(source) => {
                    self.last_proposal_stats = proposal_stats;
                    return Err(CdtProposalError::Policy { source });
                }
            };
        plan.forward_family_probability = forward_family_probability;
        plan.reverse_family_probability = reverse_distribution.probability(move_type.reverse());
        plan.log_proposal_ratio = match DiscreteProposalRatio::new(
            plan.forward_family_probability,
            plan.forward_site_count,
            plan.reverse_family_probability,
            plan.reverse_site_count,
        ) {
            Ok(ratio) => ratio.log_q_ratio(),
            Err(source) => {
                self.last_proposal_stats = proposal_stats;
                return Err(CdtProposalError::ProposalRatio { move_type, source });
            }
        };
        self.last_step_info = Some(plan_info(&plan));
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
        Ok(target.log_prob(&plan.proposed_state))
    }

    fn log_q_ratio(
        &self,
        _state: &CdtTriangulation2D,
        plan: &Self::Plan,
    ) -> Result<f64, Self::Error> {
        Ok(concrete_log_q_ratio(plan))
    }

    fn info(&self, plan: &Self::Plan) -> Self::Info {
        plan_info(plan)
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

/// Result of attempting to realize one selected family on a cloned state.
#[derive(Debug)]
pub(crate) struct ConcretePlanAttempt {
    /// Concrete, validated proposed state, or `None` for an ordinary self-loop.
    pub(crate) plan: Option<CdtProposalPlan>,
    /// Typed result of attempting to realize the selected family and site.
    pub(crate) planning_outcome: CdtProposalPlanningOutcome,
    /// Original selected-family denominator in the pre-move state.
    pub(crate) forward_site_count: usize,
}

impl ConcretePlanAttempt {
    const fn self_loop(
        planning_outcome: CdtProposalPlanningOutcome,
        forward_site_count: usize,
    ) -> Self {
        Self {
            plan: None,
            planning_outcome,
            forward_site_count,
        }
    }

    const fn planned(plan: CdtProposalPlan) -> Self {
        let forward_site_count = plan.forward_site_count;
        Self {
            plan: Some(plan),
            planning_outcome: CdtProposalPlanningOutcome::ConcretePlan,
            forward_site_count,
        }
    }
}

/// Plans one concrete CDT proposal without mutating the live chain state.
///
/// The helper samples a local site, applies it to a cloned triangulation, and
/// records the forward and reverse proposal-site counts needed for the
/// Hastings correction. Ordinary no-site, causality, geometry, and recoverable
/// backend rejections return an attempt without a concrete plan so the public
/// planned-proposal API can expose them as typed self-loop proposals.
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
) -> Result<ConcretePlanAttempt, MoveApplicationError> {
    if proposed_delta_action(action_config, simplex_counts(state), move_type).is_none() {
        proposal_stats.record_move_family(0);
        proposal_stats.record_no_site();
        return Ok(ConcretePlanAttempt::self_loop(
            CdtProposalPlanningOutcome::InvalidCountDelta,
            0,
        ));
    }
    let selection = moves.select_proposal_site(state, move_type);
    let forward_site_count = selection.site_count;
    proposal_stats.record_move_family(forward_site_count);
    let Some(site) = selection.site else {
        proposal_stats.record_no_site();
        return Ok(ConcretePlanAttempt::self_loop(
            CdtProposalPlanningOutcome::NoOfferedSite,
            forward_site_count,
        ));
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
            return Ok(ConcretePlanAttempt::self_loop(
                CdtProposalPlanningOutcome::CausalityViolation,
                forward_site_count,
            ));
        }
        MoveResult::GeometricViolation => {
            proposal_stats.record_site_rejection(&CdtProposalSiteRejection::GeometricViolation);
            return Ok(ConcretePlanAttempt::self_loop(
                CdtProposalPlanningOutcome::GeometricViolation,
                forward_site_count,
            ));
        }
        MoveResult::Rejected(err) => {
            proposal_stats.record_site_rejection(&CdtProposalSiteRejection::Kernel(err));
            return Ok(ConcretePlanAttempt::self_loop(
                CdtProposalPlanningOutcome::KernelRejected,
                forward_site_count,
            ));
        }
    };
    let delta_action = action_after - action_before;
    let reverse_site_count = moves
        .proposal_policy_view(&proposed_state, move_type.reverse())
        .offered_site_count();
    let log_proposal_ratio =
        DiscreteProposalRatio::from_counts(forward_site_count, reverse_site_count)
            .map_or(f64::NEG_INFINITY, DiscreteProposalRatio::log_q_ratio);

    Ok(ConcretePlanAttempt::planned(CdtProposalPlan {
        move_type,
        action_before,
        action_after,
        delta_action,
        forward_family_probability: 1.0,
        reverse_family_probability: 1.0,
        forward_site_count,
        reverse_site_count,
        log_proposal_ratio,
        proposed_state,
    }))
}

/// Computes the Hastings proposal-density correction for a concrete plan.
///
/// The ratio uses the instantaneous forward and reverse local-site counts from
/// the selected move family. Zero denominators represent impossible proposal
/// weights and are scored as negative infinity rather than panicking.
pub(crate) const fn concrete_log_q_ratio(plan: &CdtProposalPlan) -> f64 {
    plan.log_proposal_ratio
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
