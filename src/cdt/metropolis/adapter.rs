#![forbid(unsafe_code)]

//! Adapter boundary between CDT state and `markov-chain-monte-carlo`.

use super::helpers::{
    action_for, count_to_f64, proposed_delta_action, simplex_counts, validate_temperature,
};
use super::intrinsic::{IntrinsicCdtError, IntrinsicCdtTriangulation};
use super::telemetry::{
    CdtProposalSiteRejection, NoPlanReason, ProposalDensity, ProposalKernelTelemetry,
    ProposalStatistics,
};
use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{
    ErgodicsSystem, MoveResult, MoveType, ProposalSite, ProposalSiteSelection,
};
use crate::cdt::proposal_policy::{
    CdtMoveFamilyDistribution, CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError,
    CdtProposalPolicyView, UniformCdtMoveFamilyPolicy,
};
use crate::cdt::triangulation::{CdtTriangulation2D, TriangulationRevision};
use crate::config::CdtTopology;
use crate::errors::{
    CdtError, CdtResult, ConfigurationSetting, ExpectedConstraint, MetropolisMoveApplicationFailure,
};
use crate::geometry::backends::delaunay::{DelaunayError, EmbeddedState};
use markov_chain_monte_carlo::{
    AdditiveTarget, Chain, ChainCheckpoint, DelayedProposal, DiscreteProposalEndpoint,
    DiscreteProposalRatio, DiscreteProposalRatioError, McmcError, Target,
};
use rand::Rng;
use std::error::Error;
use std::fmt;
use std::hint::cold_path;
use std::num::NonZeroUsize;

/// Target distribution for CDT: log-probability from the Regge action.
///
/// Computes `log_prob = -S / T` where `S` is the discrete Regge action
/// and `T` is the temperature. Intrinsic states also include the reference
/// weight for their computational spatial origins.
pub struct CdtTarget {
    action_config: ActionConfig,
    temperature: f64,
}

impl CdtTarget {
    /// Creates a new CDT target distribution.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if `temperature` is
    /// not finite and positive with a finite reciprocal, or if scaling the combined
    /// action or its separately evaluated additive terms can overflow (see
    /// [`ExpectedConstraint::FiniteLogProbability`]). [`ActionConfig`] has already
    /// validated its couplings before this constructor receives it.
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
        validate_temperature(temperature)?;
        if let Err(maximum_action_magnitude) = action_config
            .magnitude_bounds()
            .check_temperature(temperature)
        {
            return Err(CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::Temperature,
                provided_value: temperature.into(),
                expected: ExpectedConstraint::FiniteLogProbability {
                    maximum_action_magnitude,
                },
            });
        }
        Ok(Self {
            action_config,
            temperature,
        })
    }
}

impl Target<CdtTriangulation2D> for CdtTarget {
    fn log_prob(&self, state: &CdtTriangulation2D) -> f64 {
        if self.action_config.volume_fixing().is_some()
            && state.metadata().topology() != CdtTopology::Toroidal
        {
            return f64::NEG_INFINITY;
        }
        if self.action_config.volume_fixing().is_some() {
            return AdditiveTarget::new(
                CdtPhysicalLogWeight {
                    config: &self.action_config,
                    temperature: self.temperature,
                },
                CdtFixingLogWeight {
                    config: &self.action_config,
                    temperature: self.temperature,
                },
            )
            .log_prob(state);
        }
        let counts = simplex_counts(state);
        let action =
            self.action_config
                .calculate_action(counts.vertices, counts.edges, counts.triangles);
        -action / self.temperature
    }
}

impl CdtTarget {
    /// Computes the marked intrinsic log weight, including any total-volume bias.
    ///
    /// # Errors
    ///
    /// Returns [`IntrinsicCdtError::VolumeFixing`] if the fixing target is below
    /// six triangles per time slice, and [`IntrinsicCdtError::MarkingPrecision`]
    /// when even a one-vertex change in the longest loop is below the resolution
    /// of an absolute log weight. The upstream chain reports either failure as
    /// an invalid target value at initialization or proposal scoring.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{ActionConfig, CdtTarget, IntrinsicCdtTriangulation};
    /// let target = CdtTarget::new(ActionConfig::default(), 1.0)?;
    /// let weight = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3; 3])
    ///     .and_then(|state| target.intrinsic_log_prob(&state));
    /// assert!(weight.is_ok_and(f64::is_finite));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn intrinsic_log_prob(
        &self,
        state: &IntrinsicCdtTriangulation,
    ) -> Result<f64, IntrinsicCdtError> {
        let lengths = state.spatial_loop_lengths();
        if let Some(fixing) = self.action_config.volume_fixing() {
            fixing.ensure_toroidal_minimum(lengths.len() as u64)?;
        }
        let (vertices, edges, triangles) = state.counts();
        let action = self
            .action_config
            .calculate_action(vertices, edges, triangles);
        // The raw marking set has one choice per vertex on each slice.
        let mut longest = 0;
        let mut log_markings = 0.0;
        for length in lengths {
            longest = longest.max(length);
            log_markings += count_to_f64(length).ln();
        }
        let scaled_action = action / self.temperature;
        let log_weight = -scaled_action - log_markings;
        let magnitude = log_weight.abs().max(scaled_action.abs()).max(log_markings);
        let marking_step = count_to_f64(longest).recip().ln_1p();
        if magnitude.next_up() - magnitude >= marking_step {
            return Err(IntrinsicCdtError::MarkingPrecision {
                log_weight,
                marking_step,
            });
        }
        Ok(log_weight)
    }
}

impl Target<IntrinsicCdtTriangulation> for CdtTarget {
    fn log_prob(&self, state: &IntrinsicCdtTriangulation) -> f64 {
        self.intrinsic_log_prob(state).unwrap_or_else(|error| {
            // Target has no error channel. The chain treats NaN as a hard
            // scoring error before acceptance or mutation; -infinity would
            // instead assign zero mass and silently change the ensemble.
            log::error!("invalid intrinsic CDT target: {error}");
            f64::NAN
        })
    }
}

/// Thin physical-action component for the upstream additive-target owner.
struct CdtPhysicalLogWeight<'a> {
    config: &'a ActionConfig,
    temperature: f64,
}

impl Target<CdtTriangulation2D> for CdtPhysicalLogWeight<'_> {
    fn log_prob(&self, state: &CdtTriangulation2D) -> f64 {
        -self
            .config
            .physical_action(state.vertex_count(), state.edge_count(), state.face_count())
            / self.temperature
    }
}

/// Thin volume-bias component; upstream still owns generic target composition.
struct CdtFixingLogWeight<'a> {
    config: &'a ActionConfig,
    temperature: f64,
}

impl Target<CdtTriangulation2D> for CdtFixingLogWeight<'_> {
    fn log_prob(&self, state: &CdtTriangulation2D) -> f64 {
        -self.config.fixing_action(state.face_count()) / self.temperature
    }
}

/// Concrete CDT proposal plan selected before committing live state.
///
/// A plan records the selected [`MoveType`], the action before and after the
/// move, and a cloned triangulation containing the proposed mutation. Planning
/// may mutate that clone to realize a concrete local site, but it never mutates
/// the live simulation state. The sampler scores this plan with the
/// Metropolis-Hastings forward/reverse transition ratio, then commits the
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
/// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
/// let (forward, reverse) = plan.transition_probabilities();
/// assert!(forward > 0.0);
/// assert!(reverse >= 0.0);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CdtProposalPlan {
    pub(crate) source: TriangulationRevision,
    pub(crate) move_type: MoveType,
    pub(crate) action_before: f64,
    pub(crate) action_after: f64,
    pub(crate) delta_action: f64,
    pub(crate) density: DiscreteProposalRatio,
    pub(crate) endpoint_density: DiscreteProposalRatio,
    pub(crate) proposed_state: CdtTriangulation2D,
}

impl CdtProposalPlan {
    fn check_source(&self, state: &CdtTriangulation2D) -> Result<(), CdtProposalError> {
        if self.source == state.revision() {
            Ok(())
        } else {
            Err(CdtProposalError::SourceMismatch {
                move_type: self.move_type,
            })
        }
    }

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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert_eq!(plan.forward_family_probability(), 0.25);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn forward_family_probability(&self) -> f64 {
        self.density.forward_weight()
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert_eq!(plan.reverse_family_probability(), 0.25);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn reverse_family_probability(&self) -> f64 {
        self.density.reverse_weight()
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert!(plan.forward_site_count() > 0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn forward_site_count(&self) -> usize {
        self.density.forward_site_count()
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert!(plan.reverse_site_count() > 0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn reverse_site_count(&self) -> usize {
        self.density.reverse_site_count()
    }

    /// Returns the marginal forward and reverse probabilities of the realized transition.
    #[must_use]
    pub const fn transition_probabilities(&self) -> (f64, f64) {
        (
            self.endpoint_density.forward_weight(),
            self.endpoint_density.reverse_weight(),
        )
    }

    /// Returns the log proposal ratio for the realized transition.
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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
    /// let mut rng = StdRng::seed_from_u64(11);
    /// if let Some(plan) = proposal.propose_plan(&tri, &mut rng)? {
    ///     assert!(plan.log_proposal_ratio() < f64::INFINITY);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn log_proposal_ratio(&self) -> f64 {
        self.endpoint_density.log_q_ratio()
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
/// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
/// let mut rng = StdRng::seed_from_u64(11);
/// let Some(plan) = proposal.propose_plan(&tri, &mut rng)? else {
///     return Ok(());
/// };
///
/// let info = proposal.info(&plan);
/// assert_eq!(info.proposal_telemetry().selected_family(), plan.move_type());
/// assert_eq!(info.proposal_telemetry().reverse_family(), plan.reverse_move_type());
/// assert_eq!(info.proposal_telemetry().forward_site_count(), plan.forward_site_count());
/// assert_eq!(info.proposal_telemetry().reverse_site_count(), Some(plan.reverse_site_count()));
/// assert_eq!(info.proposal_telemetry().log_proposal_ratio(), Some(plan.log_proposal_ratio()));
/// assert!(info.delta_action().is_some());
/// if let Some(delta_action) = info.delta_action() {
///     approx::assert_relative_eq!(delta_action, plan.delta_action(), epsilon = 1e-12);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CdtProposalInfo {
    action_before: f64,
    family: MoveType,
    density: ProposalDensity<ProposedActions>,
}

/// Actions evaluated together on a realized move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ProposedActions {
    pub(super) after: f64,
    pub(super) delta: f64,
}

impl CdtProposalInfo {
    /// Retains both actions and both densities from one realized plan.
    pub(super) const fn concrete(
        family: MoveType,
        action_before: f64,
        actions: ProposedActions,
        selected: DiscreteProposalRatio,
        transition: DiscreteProposalRatio,
    ) -> Self {
        Self {
            action_before,
            family,
            density: ProposalDensity::Concrete {
                actions,
                selected,
                transition,
            },
        }
    }

    pub(super) const fn proposed_actions(self) -> Option<ProposedActions> {
        match self.density {
            ProposalDensity::Concrete { actions, .. } => Some(actions),
            ProposalDensity::NoPlan { .. } => None,
        }
    }

    /// Action on the source triangulation.
    #[must_use]
    pub const fn action_before(self) -> f64 {
        self.action_before
    }
    /// Action on the realized proposal, when available.
    #[must_use]
    pub const fn action_after(self) -> Option<f64> {
        match self.proposed_actions() {
            Some(actions) => Some(actions.after),
            None => None,
        }
    }
    /// Scored change in the action, when available.
    #[must_use]
    pub const fn delta_action(self) -> Option<f64> {
        match self.proposed_actions() {
            Some(actions) => Some(actions.delta),
            None => None,
        }
    }

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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    /// assert_eq!(telemetry.reverse_family(), telemetry.selected_family().reverse());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn proposal_telemetry(self) -> ProposalKernelTelemetry {
        ProposalKernelTelemetry::from_density(self.family, self.density)
    }
}

/// Error reported by planned CDT proposal planning or commit.
///
/// No-site outcomes are ordinary proposal absence and are reported from
/// [`DelayedProposal::propose_plan`] as `Ok(None)`, matching the upstream
/// plan-before-commit contract. [`CdtProposalError::ApplicationFailed`]
/// represents a hard backend or invariant failure while constructing or
/// committing a concrete proposal, and preserves the typed [`CdtError`] that
/// caused the failed application.
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
    /// The supplied triangulation differs from the owner or revision used to plan.
    SourceMismatch {
        /// Move whose source state changed.
        move_type: MoveType,
    },
    /// The injected family policy could not produce a checked distribution.
    Policy {
        /// Typed policy evaluation or normalization failure.
        source: CdtMoveFamilyPolicyError,
    },
    /// The upstream MCMC proposal-ratio boundary rejected planned components.
    ProposalRatio {
        /// Selected forward move family.
        move_type: MoveType,
        /// Typed upstream proposal-ratio construction failure.
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
            Self::SourceMismatch { move_type } => write!(
                f,
                "{move_type:?} proposal belongs to a different triangulation or revision"
            ),
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
            Self::SourceMismatch { .. } => None,
            Self::Policy { source } => Some(source),
            Self::ProposalRatio { source, .. } => Some(source),
            Self::ApplicationFailed { source, .. } => Some(source),
        }
    }
}

impl From<CdtProposalError> for CdtError {
    fn from(error: CdtProposalError) -> Self {
        match error {
            CdtProposalError::SourceMismatch { move_type } => {
                Self::ProposalSourceMismatch { move_type }
            }
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
        Self {
            action_config,
            moves: ErgodicsSystem::new(),
            policy: UniformCdtMoveFamilyPolicy,

            last_no_plan_info: None,
            last_proposal_stats: ProposalStatistics::new(),
        }
    }
}

impl<P> CdtProposal<P> {
    /// Returns this proposal planner with a deterministic internal RNG seed.
    ///
    /// The seed controls the internal move-family and site selector. The `rng`
    /// passed to [`DelayedProposal::propose_plan`] is still accepted for
    /// compatibility with generic MCMC drivers. Reseeding clears cached
    /// per-attempt telemetry so subsequent inspection cannot observe data from
    /// the earlier RNG stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{ActionConfig, CdtProposal};
    ///
    /// let _proposal = CdtProposal::new(ActionConfig::default()).with_seed(42);
    /// ```
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.moves = ErgodicsSystem::with_seed(seed);

        self.last_no_plan_info = None;
        self.last_proposal_stats = ProposalStatistics::new();
        self
    }

    /// Returns this proposal planner with a caller-supplied family policy.
    ///
    /// The conversion preserves the current proposal RNG stream and clears
    /// cached per-attempt telemetry. Borrow `policy` when its model state or
    /// audit data must remain caller-owned.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtMoveFamilyDistribution, CdtProposal,
    /// };
    ///
    /// let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])?;
    /// let _proposal = CdtProposal::new(ActionConfig::default())
    ///     .with_seed(42)
    ///     .with_policy(policy);
    /// # Ok::<(), causal_triangulations::CdtMoveFamilyPolicyError>(())
    /// ```
    #[must_use]
    pub fn with_policy<Q>(self, policy: Q) -> CdtProposal<Q>
    where
        Q: CdtMoveFamilyPolicy,
    {
        CdtProposal {
            action_config: self.action_config,
            moves: self.moves,
            policy,

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
    /// let mut proposal = CdtProposal::new(ActionConfig::default()).with_seed(7);
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
    pub(crate) const fn from_ergodics_with_policy(
        action_config: ActionConfig,
        moves: ErgodicsSystem,
        policy: P,
    ) -> Self
    where
        P: CdtMoveFamilyPolicy,
    {
        Self {
            action_config,
            moves,
            policy,

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
pub(super) const fn no_plan_info(
    move_type: MoveType,
    action_before: f64,
    forward_family_probability: f64,
    forward_site_count: usize,
    reason: NoPlanReason,
) -> CdtProposalInfo {
    CdtProposalInfo {
        action_before,
        family: move_type,
        density: ProposalDensity::NoPlan {
            reason,
            probability: forward_family_probability,
            sites: forward_site_count,
        },
    }
}

/// Builds complete public telemetry from an invariant-bearing concrete plan.
const fn plan_info(plan: &CdtProposalPlan) -> CdtProposalInfo {
    CdtProposalInfo::concrete(
        plan.move_type,
        plan.action_before,
        ProposedActions {
            after: plan.action_after,
            delta: plan.delta_action,
        },
        plan.density,
        plan.endpoint_density,
    )
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
        self.last_no_plan_info = None;
        self.last_proposal_stats = ProposalStatistics::new();
        let distribution = evaluate_policy_distribution(&self.policy, &mut self.moves, state)
            .map_err(|source| CdtProposalError::Policy { source })?;
        let move_type = self.moves.select_move_family(&distribution);
        let forward_family_probability = distribution.probability(move_type);
        let action_before = action_for(&self.action_config, state);
        let mut proposal_stats = ProposalStatistics::new();
        let realized = match propose_concrete_plan(
            state,
            &mut self.moves,
            &mut proposal_stats,
            &self.action_config,
            move_type,
        ) {
            Ok(ConcretePlanAttempt::Planned(plan)) => plan,
            Ok(ConcretePlanAttempt::NoPlan {
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

                self.last_no_plan_info = Some(no_plan_info);
                self.last_proposal_stats = proposal_stats;
                cold_path();
                return Ok(None);
            }
            Err(err) => {
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
        let proposed_state = realized.proposed_state;
        let reverse_distribution =
            match evaluate_policy_distribution(&self.policy, &mut self.moves, &proposed_state) {
                Ok(distribution) => distribution,
                Err(source) => {
                    self.last_proposal_stats = proposal_stats;
                    return Err(CdtProposalError::Policy { source });
                }
            };
        let reverse_site_count = self
            .moves
            .proposal_policy_view(&proposed_state, move_type.reverse())
            .offered_site_count();
        let density = DiscreteProposalRatio::from_endpoints(
            DiscreteProposalEndpoint::new(
                forward_family_probability,
                1.0,
                realized.forward_site_count.get(),
            ),
            DiscreteProposalEndpoint::new(
                reverse_distribution.probability(move_type.reverse()),
                1.0,
                reverse_site_count,
            ),
        );
        self.last_proposal_stats = proposal_stats;
        let density =
            density.map_err(|source| CdtProposalError::ProposalRatio { move_type, source })?;

        let endpoint_density = score_embedded_transition(
            &mut self.moves,
            move_type,
            state,
            &proposed_state,
            &distribution,
            &reverse_distribution,
            &realized.selected_site,
        )
        .inspect_err(|error| {
            if matches!(error, CdtProposalError::ApplicationFailed { .. }) {
                self.last_proposal_stats.record_hard_failure();
            }
        })?;
        Ok(Some(CdtProposalPlan {
            source: state.revision(),
            move_type,
            action_before,
            action_after: realized.action_after,
            delta_action: realized.delta_action,
            density,
            endpoint_density,
            proposed_state,
        }))
    }

    fn no_plan_info(&mut self) -> Option<Self::Info> {
        self.last_no_plan_info.take()
    }

    fn proposed_log_prob<T: Target<CdtTriangulation2D> + ?Sized>(
        &self,
        state: &CdtTriangulation2D,
        plan: &Self::Plan,
        target: &T,
    ) -> Result<f64, Self::Error> {
        plan.check_source(state)?;
        Ok(target.log_prob(&plan.proposed_state))
    }

    fn log_q_ratio(
        &self,
        state: &CdtTriangulation2D,
        plan: &Self::Plan,
    ) -> Result<f64, Self::Error> {
        plan.check_source(state)?;
        Ok(plan.log_proposal_ratio())
    }

    fn info(&self, plan: &Self::Plan) -> Self::Info {
        plan_info(plan)
    }

    fn commit<R: Rng + ?Sized>(
        &mut self,
        state: &mut CdtTriangulation2D,
        mut plan: Self::Plan,
        _rng: &mut R,
    ) -> Result<(), Self::Error> {
        plan.check_source(state)?;
        // Validation cadence is mutable run policy outside the geometry revision.
        plan.proposed_state
            .set_delaunay_check_interval(state.geometry().delaunay_check_interval());
        *state = plan.proposed_state;
        Ok(())
    }
}

/// Hard proposal-planning failure preserved for public error conversion.
///
/// This carrier keeps the attempt number and structured CDT error intact so
/// [`DelayedProposal::propose_plan`] can expose
/// [`CdtProposalError::ApplicationFailed`] without flattening the underlying
/// failure into text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MoveApplicationError {
    /// One-based proposal attempt that produced the hard failure.
    pub(crate) attempt: usize,
    /// Structured CDT failure reported by the move kernel.
    pub(crate) source: CdtError,
}

impl From<DelaunayError> for MoveApplicationError {
    fn from(error: DelaunayError) -> Self {
        Self {
            attempt: 1,
            source: CdtError::ProposalGeometry(error),
        }
    }
}

/// Result of attempting to realize one selected family on a cloned state.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "a successful attempt moves its triangulation directly into the returned plan without a heap allocation"
)]
pub(crate) enum ConcretePlanAttempt {
    Planned(RealizedCdtProposal),
    NoPlan {
        planning_outcome: NoPlanReason,
        forward_site_count: usize,
    },
}

/// Validated outcome of realizing one site, before endpoint-mass scoring.
#[derive(Debug)]
pub(crate) struct RealizedCdtProposal {
    pub(crate) forward_site_count: NonZeroUsize,
    pub(crate) action_after: f64,
    pub(crate) delta_action: f64,
    pub(crate) proposed_state: CdtTriangulation2D,
    selected_site: ProposalSite,
}

/// Plans one concrete CDT proposal without mutating the live chain state.
///
/// The helper samples a local site, applies it to a cloned triangulation, and
/// records the offered forward count and the realized destination. Ordinary
/// no-site, causality, geometry, and recoverable
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
) -> Result<ConcretePlanAttempt, MoveApplicationError> {
    let Some(delta_action) = proposed_delta_action(action_config, simplex_counts(state), move_type)
    else {
        proposal_stats.record_move_family(0);
        proposal_stats.record_no_site();
        return Ok(ConcretePlanAttempt::NoPlan {
            planning_outcome: NoPlanReason::InvalidCountDelta,
            forward_site_count: 0,
        });
    };
    let selection = moves.select_proposal_site(state, move_type);
    proposal_stats.record_move_family(selection.site_count());
    let ProposalSiteSelection::Sampled {
        site,
        count: forward_site_count,
    } = selection
    else {
        proposal_stats.record_no_site();
        return Ok(ConcretePlanAttempt::NoPlan {
            planning_outcome: NoPlanReason::NoOfferedSite,
            forward_site_count: 0,
        });
    };

    let proposed_state = match realize_site(state, move_type, &site)? {
        SiteRealization::Applied(draft) => draft,
        SiteRealization::Rejected(rejection) => {
            proposal_stats.record_site_rejection(&rejection);
            let planning_outcome = match rejection {
                CdtProposalSiteRejection::CausalityViolation => NoPlanReason::CausalityViolation,
                CdtProposalSiteRejection::GeometricViolation => NoPlanReason::GeometricViolation,
                CdtProposalSiteRejection::Kernel(_) => NoPlanReason::KernelRejected,
            };
            return Ok(ConcretePlanAttempt::NoPlan {
                planning_outcome,
                forward_site_count: forward_site_count.get(),
            });
        }
    };
    let action_after = action_for(action_config, &proposed_state);
    Ok(ConcretePlanAttempt::Planned(RealizedCdtProposal {
        forward_site_count,
        action_after,
        delta_action,
        proposed_state,
        selected_site: site,
    }))
}

/// Only an applied edit supplies a candidate state to endpoint scoring.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "move the speculative mesh into the plan without allocating for every scored site"
)]
enum SiteRealization {
    Applied(CdtTriangulation2D),
    Rejected(CdtProposalSiteRejection),
}

/// Applies an atom in isolation. Only an ordinary candidate rejection carries
/// zero probability for a distinct endpoint.
fn realize_site(
    source: &CdtTriangulation2D,
    family: MoveType,
    site: &ProposalSite,
) -> Result<SiteRealization, MoveApplicationError> {
    let mut draft = source.clone();
    let site = site.remap_for_clone(source, &draft)?;
    let result = ErgodicsSystem::apply_proposal_site_to_draft(&mut draft, family, site);
    classify_site_result(result, draft)
}

/// Separates ordinary rejection mass from failures that invalidate scoring.
fn classify_site_result(
    result: MoveResult,
    draft: CdtTriangulation2D,
) -> Result<SiteRealization, MoveApplicationError> {
    let rejection = match result {
        MoveResult::Success => return Ok(SiteRealization::Applied(draft)),
        MoveResult::CausalityViolation => CdtProposalSiteRejection::CausalityViolation,
        MoveResult::GeometricViolation => CdtProposalSiteRejection::GeometricViolation,
        MoveResult::Rejected(source) => CdtProposalSiteRejection::Kernel(source),
        MoveResult::HardFailure(source) => return Err(MoveApplicationError { attempt: 1, source }),
    };
    Ok(SiteRealization::Rejected(rejection))
}

/// Counts successful realizations with the destination's exact embedded charts.
fn embedded_transition_probability(
    moves: &mut ErgodicsSystem,
    source: &CdtTriangulation2D,
    target: &EmbeddedState,
    family: MoveType,
    distribution: &CdtMoveFamilyDistribution,
    realized_site: Option<&ProposalSite>,
) -> Result<f64, MoveApplicationError> {
    let probability = match family {
        MoveType::Move22 | MoveType::EdgeFlip => {
            distribution.probability(MoveType::Move22)
                + distribution.probability(MoveType::EdgeFlip)
        }
        MoveType::Move13Add | MoveType::Move31Remove => distribution.probability(family),
    };
    if probability == 0.0 {
        return Ok(0.0);
    }
    let view = moves.proposal_policy_view(source, family);
    let offered = view.offered_site_count();
    if offered == 0 {
        return Ok(0.0);
    }
    let mut candidates = Vec::new();
    let mut matching = 0;
    for site in (0..offered).filter_map(|ordinal| view.site_at(ordinal)) {
        // This descriptor already produced the destination in this direction.
        // Equal descriptors, if present, are separate offered atoms.
        if Some(site) == realized_site {
            matching += 1;
        } else if site.could_reach(source, target)? {
            candidates.push(site.clone());
        }
    }
    for site in &candidates {
        if let SiteRealization::Applied(draft) = realize_site(source, family, site)?
            && draft.geometry().embedded_state()? == *target
        {
            matching += 1;
        }
    }
    Ok(probability * (count_to_f64(matching) / count_to_f64(offered)))
}

/// Marginalizes actual embedded atoms, including both flip names.
fn score_embedded_transition(
    moves: &mut ErgodicsSystem,
    family: MoveType,
    source: &CdtTriangulation2D,
    target: &CdtTriangulation2D,
    forward_distribution: &CdtMoveFamilyDistribution,
    reverse_distribution: &CdtMoveFamilyDistribution,
    selected_site: &ProposalSite,
) -> Result<DiscreteProposalRatio, CdtProposalError> {
    let to_application =
        |move_type, failure: MoveApplicationError| CdtProposalError::ApplicationFailed {
            move_type,
            attempt: failure.attempt,
            source: failure.source,
        };
    let source_state = source
        .geometry()
        .embedded_state()
        .map_err(|error| to_application(family, error.into()))?;
    let reverse_family = family.reverse();
    let target_state = target
        .geometry()
        .embedded_state()
        .map_err(|error| to_application(reverse_family, error.into()))?;
    let forward = embedded_transition_probability(
        moves,
        source,
        &target_state,
        family,
        forward_distribution,
        Some(selected_site),
    )
    .map_err(|failure| to_application(family, failure))?;
    let reverse = embedded_transition_probability(
        moves,
        target,
        &source_state,
        reverse_family,
        reverse_distribution,
        None,
    )
    .map_err(|failure| to_application(reverse_family, failure))?;
    DiscreteProposalRatio::from_endpoints(
        DiscreteProposalEndpoint::new(forward, 1.0, 1),
        DiscreteProposalEndpoint::new(reverse, 1.0, 1),
    )
    .map_err(|source| CdtProposalError::ProposalRatio {
        move_type: family,
        source,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{CdtValidationCheck, CdtValidationFailure};
    use crate::geometry::backends::delaunay::DelaunayHandleKind;
    use std::assert_matches;

    #[test]
    fn endpoint_scoring_retains_typed_geometry_source() {
        let stale = DelaunayError::StaleHandle {
            kind: DelaunayHandleKind::Edge,
            handle_generation: 1,
            current_generation: 2,
        };
        let failure = MoveApplicationError::from(stale.clone());
        assert_eq!(failure.attempt, 1);
        let failure = MetropolisMoveApplicationFailure::from(failure.source);
        assert_eq!(
            failure
                .source()
                .and_then(|e| e.downcast_ref::<DelaunayError>()),
            Some(&stale)
        );
    }

    #[test]
    fn realized_site_preserves_hard_failure_instead_of_counting_rejection() {
        let source = CdtError::ValidationFailed {
            check: CdtValidationCheck::Geometry,
            failure: CdtValidationFailure::BackendGeometry {
                detail: "injected incidence failure".to_owned(),
            },
        };
        let draft = CdtTriangulation2D::from_cdt_strip(4, 3).expect("valid candidate fixture");
        let failure = classify_site_result(MoveResult::HardFailure(source.clone()), draft.clone())
            .expect_err("hard failure must abort endpoint scoring");
        assert_eq!(failure, MoveApplicationError { attempt: 1, source });
        assert_matches!(
            classify_site_result(MoveResult::GeometricViolation, draft),
            Ok(SiteRealization::Rejected(
                CdtProposalSiteRejection::GeometricViolation
            ))
        );
    }
}
