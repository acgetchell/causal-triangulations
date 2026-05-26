//! Proposal and step telemetry for CDT Metropolis sampling.

use crate::errors::CdtError;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

use crate::cdt::ergodic_moves::MoveType;

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

/// Local-site rejection observed while trying to realize an accepted CDT proposal.
///
/// These rejections mean the move type was selected and accepted at the
/// count-action level, but the bounded random local-site search did not find a
/// concrete site where the move could be applied.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CdtProposalSiteRejection {
    /// The selected local site would violate CDT causality.
    CausalityViolation,
    /// The selected local site was geometrically invalid.
    GeometricViolation,
    /// The selected local site was rejected by the backend mutation kernel.
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

/// Telemetry for concrete Metropolis proposal outcomes.
///
/// These counters describe the proposal kernel observed during a run. They are
/// diagnostic only: detailed balance is enforced by the per-step proposal
/// probability used in the Hastings ratio, not by accumulated empirical counts.
///
/// The struct is non-exhaustive so future releases can add proposal telemetry
/// without breaking downstream code. Construct empty accumulators with
/// [`Self::new`] or [`Default::default`], and inspect fields through shared
/// references returned by
/// [`checkpoint proposal stats`][super::CdtMcmcCheckpoint::proposal_stats] or
/// [`result proposal stats`][crate::cdt::results::SimulationResultsBackend::proposal_stats].
/// Counters saturate at `u64::MAX` instead of wrapping.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::ProposalStatistics;
///
/// let stats = ProposalStatistics::new();
/// assert_eq!(stats.move_family_proposals, 0);
/// assert_eq!(stats.accepted_transitions, 0);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProposalStatistics {
    /// Number of selected move-family proposals, saturating at `u64::MAX`.
    pub move_family_proposals: u64,
    /// Sum of sampleable forward-site denominators observed during planning,
    /// saturating at `u64::MAX`.
    pub observed_forward_sites: u64,
    /// Number of proposals with no sampleable local site, saturating at `u64::MAX`.
    pub no_site_proposals: u64,
    /// Number of sampled sites rejected by causal checks, saturating at `u64::MAX`.
    pub site_causality_rejections: u64,
    /// Number of sampled sites rejected by geometric checks, saturating at `u64::MAX`.
    pub site_geometric_rejections: u64,
    /// Number of sampled sites rejected by backend mutation errors, saturating at `u64::MAX`.
    pub site_backend_rejections: u64,
    /// Number of valid proposed transitions rejected by the Metropolis draw,
    /// saturating at `u64::MAX`.
    pub metropolis_rejections: u64,
    /// Number of proposed transitions committed to the chain, saturating at `u64::MAX`.
    pub accepted_transitions: u64,
    /// Number of proposal attempts that hit a hard failure, saturating at `u64::MAX`.
    pub hard_failures: u64,
}

impl ProposalStatistics {
    /// Creates an empty proposal telemetry accumulator.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats, ProposalStatistics::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            move_family_proposals: 0,
            observed_forward_sites: 0,
            no_site_proposals: 0,
            site_causality_rejections: 0,
            site_geometric_rejections: 0,
            site_backend_rejections: 0,
            metropolis_rejections: 0,
            accepted_transitions: 0,
            hard_failures: 0,
        }
    }

    /// Records one selected move family and the forward-site denominator observed for it.
    ///
    /// This is proposal-kernel telemetry only; the per-step Hastings ratio uses
    /// the instantaneous site count directly rather than accumulated statistics.
    pub(crate) fn record_move_family(&mut self, forward_sites: usize) {
        self.move_family_proposals = self.move_family_proposals.saturating_add(1);
        self.observed_forward_sites = self
            .observed_forward_sites
            .saturating_add(u64::try_from(forward_sites).unwrap_or(u64::MAX));
    }

    /// Records a move-family proposal with no concrete local site.
    pub(crate) const fn record_no_site(&mut self) {
        self.no_site_proposals = self.no_site_proposals.saturating_add(1);
    }

    /// Classifies a sampled-site rejection without changing chain state.
    ///
    /// These are ordinary self-loop proposal outcomes, not hard failures.
    pub(crate) const fn record_site_rejection(&mut self, rejection: &CdtProposalSiteRejection) {
        match rejection {
            CdtProposalSiteRejection::CausalityViolation => {
                self.site_causality_rejections = self.site_causality_rejections.saturating_add(1);
            }
            CdtProposalSiteRejection::GeometricViolation => {
                self.site_geometric_rejections = self.site_geometric_rejections.saturating_add(1);
            }
            CdtProposalSiteRejection::Kernel(_) => {
                self.site_backend_rejections = self.site_backend_rejections.saturating_add(1);
            }
        }
    }

    /// Records Metropolis rejection after a valid proposed transition was scored.
    pub(crate) const fn record_metropolis_rejection(&mut self) {
        self.metropolis_rejections = self.metropolis_rejections.saturating_add(1);
    }

    /// Records a proposed transition that was committed to the live chain.
    pub(crate) const fn record_accepted_transition(&mut self) {
        self.accepted_transitions = self.accepted_transitions.saturating_add(1);
    }

    /// Records an unexpected hard failure during proposal application.
    pub(crate) const fn record_hard_failure(&mut self) {
        self.hard_failures = self.hard_failures.saturating_add(1);
    }
}
