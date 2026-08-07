#![forbid(unsafe_code)]

//! Borrowed, invariant-safe views for 1+1 CDT proposal policies.

use crate::cdt::ergodic_moves::{MoveType, ProposalSite};
use crate::cdt::triangulation::CdtSimplexCounts;
use crate::config::CdtTopology;
use crate::errors::CdtResult;
use crate::geometry::CdtTriangulation2D;
use std::error::Error;
use std::fmt;
use std::iter::FusedIterator;
use std::ops::Range;

/// State-dependent policy over the four reversible 1+1 CDT move families.
///
/// CDT calls [`Self::family_weight`] once for each family in
/// [`MoveType::REVERSIBLE_1P1`], passing the invariant-safe borrowed view for
/// the current state. Returned values are nonnegative relative weights: CDT
/// validates and normalizes the complete four-family output before sampling a
/// family. A positive weight remains part of the proposal distribution even
/// when that family has no offered sites; selecting it produces an ordinary
/// self-loop rather than renormalizing over nonempty families.
///
/// Implementations should be deterministic functions of the supplied view and
/// externally managed immutable model state. CDT evaluates the policy again on
/// a realized proposed state to obtain the reverse-family probability.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError, CdtProposalPolicyView, MoveType,
/// };
///
/// struct PreferVolumeGrowth;
///
/// impl CdtMoveFamilyPolicy for PreferVolumeGrowth {
///     fn family_weight(
///         &self,
///         view: &CdtProposalPolicyView<'_>,
///     ) -> Result<f64, CdtMoveFamilyPolicyError> {
///         Ok(if view.family() == MoveType::Move13Add {
///             3.0
///         } else {
///             1.0
///         })
///     }
/// }
/// ```
pub trait CdtMoveFamilyPolicy {
    /// Returns a checked state-independent distribution when the policy is fixed.
    ///
    /// The default is `None`, so state-dependent implementations continue to
    /// receive one [`CdtProposalPolicyView`] per family through
    /// [`Self::family_weight`]. Fixed policies should return `Some` here so the
    /// proposal hot path can reuse their checked distribution without
    /// materializing unrelated family-site caches.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtMoveFamilyDistribution, CdtMoveFamilyPolicy,
    /// };
    ///
    /// let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])?;
    /// assert_eq!(policy.fixed_distribution(), Some(policy));
    /// # Ok::<(), causal_triangulations::CdtMoveFamilyPolicyError>(())
    /// ```
    #[must_use]
    fn fixed_distribution(&self) -> Option<CdtMoveFamilyDistribution> {
        None
    }

    /// Returns one nonnegative finite relative weight for the supplied family view.
    ///
    /// # Errors
    ///
    /// Returns [`CdtMoveFamilyPolicyError`] when policy evaluation itself
    /// cannot produce a weight. CDT separately rejects non-finite or negative
    /// successful outputs and complete distributions with empty support.
    fn family_weight(
        &self,
        view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError>;
}

impl<P> CdtMoveFamilyPolicy for &P
where
    P: CdtMoveFamilyPolicy + ?Sized,
{
    fn fixed_distribution(&self) -> Option<CdtMoveFamilyDistribution> {
        (**self).fixed_distribution()
    }

    fn family_weight(
        &self,
        view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError> {
        (**self).family_weight(view)
    }
}

/// Failure to evaluate or normalize an injected move-family policy.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CdtMoveFamilyPolicyError {
    /// The policy implementation could not evaluate one family.
    EvaluationFailed {
        /// Family whose weight could not be evaluated.
        family: MoveType,
        /// Opaque policy-specific diagnostic.
        detail: String,
    },
    /// One returned family weight was negative or non-finite.
    InvalidWeight {
        /// Family associated with the invalid output.
        family: MoveType,
        /// Rejected raw policy weight.
        weight: f64,
    },
    /// Every supported family had zero raw weight.
    EmptySupport,
    /// Finite component weights overflowed while their normalization total was computed.
    NonFiniteTotalWeight {
        /// Non-finite sum of the four raw weights.
        total_weight: f64,
    },
}

impl fmt::Display for CdtMoveFamilyPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvaluationFailed { family, detail } => write!(
                formatter,
                "move-family policy evaluation failed for {}: {detail}",
                family.identifier()
            ),
            Self::InvalidWeight { family, weight } => write!(
                formatter,
                "invalid move-family weight {weight} for {}: expected a nonnegative finite value",
                family.identifier()
            ),
            Self::EmptySupport => formatter.write_str(
                "move-family policy has empty support: expected at least one positive weight",
            ),
            Self::NonFiniteTotalWeight { total_weight } => write!(
                formatter,
                "invalid move-family normalization total {total_weight}: expected a positive finite sum",
            ),
        }
    }
}

impl Error for CdtMoveFamilyPolicyError {}

/// Checked, normalized, state-independent move-family distribution.
///
/// The array passed to [`Self::from_weights`] follows
/// [`MoveType::REVERSIBLE_1P1`] order. Inputs are nonnegative relative weights;
/// they need not already sum to one. Individual zero weights are supported,
/// while an all-zero distribution is rejected. Normalization quantizes the
/// effective probabilities to the proposal RNG's 53-bit categorical draw
/// space, preserving at least one draw value for every positive input weight.
///
/// This type also implements [`CdtMoveFamilyPolicy`], making it the fixed
/// weighted policy for [`CdtProposal`](crate::CdtProposal) and
/// [`MetropolisAlgorithm`](crate::MetropolisAlgorithm).
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     CdtMoveFamilyDistribution, MoveType,
/// };
///
/// let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])?;
/// assert_eq!(policy.probability(MoveType::Move13Add), 0.5);
/// # Ok::<(), causal_triangulations::CdtMoveFamilyPolicyError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CdtMoveFamilyDistribution {
    probabilities: [f64; 4],
    sample_masses: [u64; 4],
}

/// Number of equally likely integer outcomes used by one family-selection draw.
const FAMILY_SAMPLE_RESOLUTION: u64 = 1_u64 << 53;
/// Exact floating-point representation of [`FAMILY_SAMPLE_RESOLUTION`].
const FAMILY_SAMPLE_RESOLUTION_F64: f64 = 9_007_199_254_740_992.0;

impl CdtMoveFamilyDistribution {
    /// Creates the uniform distribution over all reversible 1+1 families.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{CdtMoveFamilyDistribution, MoveType};
    ///
    /// let policy = CdtMoveFamilyDistribution::uniform();
    /// assert_eq!(policy.probability(MoveType::Move22), 0.25);
    /// ```
    #[must_use]
    pub const fn uniform() -> Self {
        Self {
            probabilities: [0.25; 4],
            sample_masses: [FAMILY_SAMPLE_RESOLUTION / 4; 4],
        }
    }

    /// Validates and normalizes four relative family weights.
    ///
    /// # Errors
    ///
    /// Returns [`CdtMoveFamilyPolicyError::InvalidWeight`] for a negative or
    /// non-finite component, [`CdtMoveFamilyPolicyError::EmptySupport`] when all
    /// components are zero, or
    /// [`CdtMoveFamilyPolicyError::NonFiniteTotalWeight`] if their finite sum
    /// overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtMoveFamilyDistribution, MoveType,
    /// };
    ///
    /// let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 0.0, 2.0])?;
    /// assert_eq!(policy.probability(MoveType::Move13Add), 0.5);
    /// # Ok::<(), causal_triangulations::CdtMoveFamilyPolicyError>(())
    /// ```
    pub fn from_weights(weights: [f64; 4]) -> Result<Self, CdtMoveFamilyPolicyError> {
        for (family, weight) in MoveType::REVERSIBLE_1P1.into_iter().zip(weights) {
            if !weight.is_finite() || weight < 0.0 {
                return Err(CdtMoveFamilyPolicyError::InvalidWeight { family, weight });
            }
        }

        let total_weight = weights.into_iter().sum::<f64>();
        if total_weight == 0.0 {
            return Err(CdtMoveFamilyPolicyError::EmptySupport);
        }
        if !total_weight.is_finite() {
            return Err(CdtMoveFamilyPolicyError::NonFiniteTotalWeight { total_weight });
        }

        let sample_masses = quantized_sample_masses(weights, total_weight);
        let probabilities = sample_masses.map(sample_mass_probability);

        Ok(Self {
            probabilities,
            sample_masses,
        })
    }

    /// Returns the normalized probability of selecting `family`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{CdtMoveFamilyDistribution, MoveType};
    ///
    /// let policy = CdtMoveFamilyDistribution::uniform();
    /// assert_eq!(policy.probability(MoveType::Move31Remove), 0.25);
    /// ```
    #[must_use]
    pub const fn probability(&self, family: MoveType) -> f64 {
        self.probabilities[family_index(family)]
    }

    /// Returns all probabilities in [`MoveType::REVERSIBLE_1P1`] order.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::CdtMoveFamilyDistribution;
    ///
    /// assert_eq!(CdtMoveFamilyDistribution::uniform().probabilities(), [0.25; 4]);
    /// ```
    #[must_use]
    pub const fn probabilities(&self) -> [f64; 4] {
        self.probabilities
    }

    /// Returns the exact number of categorical draw atoms assigned to `family`.
    pub(crate) const fn sample_mass(&self, family: MoveType) -> u64 {
        self.sample_masses[family_index(family)]
    }
}

impl Default for CdtMoveFamilyDistribution {
    fn default() -> Self {
        Self::uniform()
    }
}

impl CdtMoveFamilyPolicy for CdtMoveFamilyDistribution {
    fn fixed_distribution(&self) -> Option<CdtMoveFamilyDistribution> {
        Some(*self)
    }

    fn family_weight(
        &self,
        view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError> {
        Ok(self.probability(view.family()))
    }
}

/// Built-in uniform policy used by conventional CDT simulations.
///
/// Its checked distribution uses the same sampler boundary as fixed and
/// state-dependent injected policies without refreshing family-site caches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UniformCdtMoveFamilyPolicy;

impl CdtMoveFamilyPolicy for UniformCdtMoveFamilyPolicy {
    fn fixed_distribution(&self) -> Option<CdtMoveFamilyDistribution> {
        Some(CdtMoveFamilyDistribution::uniform())
    }

    fn family_weight(
        &self,
        _view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError> {
        Ok(1.0)
    }
}

/// Converts one exact categorical mass to its effective selection probability.
fn sample_mass_probability(sample_mass: u64) -> f64 {
    sample_mass_to_f64(sample_mass) / FAMILY_SAMPLE_RESOLUTION_F64
}

/// Quantizes normalized weights onto the exact 53-bit family-selection grid.
///
/// Largest-remainder allocation keeps the effective distribution as close as
/// possible to the requested weights. A final support repair assigns one atom
/// to positive weights below the RNG resolution and removes the same mass from
/// the largest supported family, so sampling and Hastings telemetry remain
/// exactly aligned without silently dropping requested support.
fn quantized_sample_masses(weights: [f64; 4], total_weight: f64) -> [u64; 4] {
    let mut sample_masses = [0_u64; 4];
    let mut remainders = [0.0; 4];
    for index in 0..weights.len() {
        let ideal_mass = weights[index] / total_weight * FAMILY_SAMPLE_RESOLUTION_F64;
        sample_masses[index] = floor_sample_mass(ideal_mass);
        remainders[index] = ideal_mass - sample_mass_to_f64(sample_masses[index]);
    }

    let allocated = sample_masses.into_iter().sum::<u64>();
    if allocated < FAMILY_SAMPLE_RESOLUTION {
        let recipient = largest_remainder_index(&remainders, &weights);
        sample_masses[recipient] += FAMILY_SAMPLE_RESOLUTION - allocated;
    } else if allocated > FAMILY_SAMPLE_RESOLUTION {
        remove_sample_mass(
            &mut sample_masses,
            allocated - FAMILY_SAMPLE_RESOLUTION,
            [0; 4],
        );
    }

    let minimum_masses = weights.map(|weight| u64::from(weight > 0.0));
    let missing_support = (0..weights.len())
        .filter(|&index| minimum_masses[index] == 1 && sample_masses[index] == 0)
        .count() as u64;
    for index in 0..weights.len() {
        sample_masses[index] = sample_masses[index].max(minimum_masses[index]);
    }
    remove_sample_mass(&mut sample_masses, missing_support, minimum_masses);

    debug_assert_eq!(
        sample_masses.into_iter().sum::<u64>(),
        FAMILY_SAMPLE_RESOLUTION
    );
    sample_masses
}

/// Converts a categorical mass to `f64`; every accepted value is at most `2^53`.
#[expect(
    clippy::cast_precision_loss,
    reason = "integers through 2^53 are exactly representable by binary64"
)]
fn sample_mass_to_f64(sample_mass: u64) -> f64 {
    debug_assert!(sample_mass <= FAMILY_SAMPLE_RESOLUTION);
    sample_mass as f64
}

/// Floors a checked finite nonnegative ideal mass within the categorical range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the checked normalized mass is finite, nonnegative, and at most 2^53"
)]
fn floor_sample_mass(ideal_mass: f64) -> u64 {
    debug_assert!(ideal_mass.is_finite());
    debug_assert!((0.0..=FAMILY_SAMPLE_RESOLUTION_F64).contains(&ideal_mass));
    ideal_mass.floor() as u64
}

/// Selects the supported family with the largest fractional allocation remainder.
fn largest_remainder_index(remainders: &[f64; 4], weights: &[f64; 4]) -> usize {
    let mut selected = 0;
    for index in 1..remainders.len() {
        if weights[index] > 0.0 && remainders[index] > remainders[selected] {
            selected = index;
        }
    }
    selected
}

/// Removes quantization excess without violating per-family minimum support.
fn remove_sample_mass(sample_masses: &mut [u64; 4], mut excess: u64, minimum_masses: [u64; 4]) {
    while excess > 0 {
        let donor = (0..sample_masses.len())
            .max_by_key(|&index| sample_masses[index] - minimum_masses[index]);
        let Some(donor) = donor else {
            break;
        };
        let available = sample_masses[donor] - minimum_masses[donor];
        debug_assert!(available > 0);
        let removed = available.min(excess);
        sample_masses[donor] -= removed;
        excess -= removed;
    }
    debug_assert_eq!(excess, 0);
}

/// Maps a reversible move family to its stable policy-array position.
const fn family_index(family: MoveType) -> usize {
    match family {
        MoveType::Move22 => 0,
        MoveType::Move13Add => 1,
        MoveType::Move31Remove => 2,
        MoveType::EdgeFlip => 3,
    }
}

/// Opaque identifier for one canonical offered proposal site.
///
/// An identifier names one ordinal in one move family's deterministic site
/// ordering for a specific triangulation instance and modification version. It
/// deliberately does not expose Delaunay handles or mutation representation.
/// Identifiers may outlive the borrowed view that created them, but callers
/// must validate them against a fresh view before reuse. An accepted mutation
/// makes earlier IDs stale for that state; clones, deserialized values, and
/// replacement triangulations have a different identity and reject the IDs as
/// foreign.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
/// use causal_triangulations::prelude::triangulation::*;
///
/// # fn main() -> CdtResult<()> {
/// let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
/// let mut moves = ErgodicsSystem::with_seed(7);
/// let view = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
/// let Some(site) = view.offered_sites().next() else {
///     return Ok(());
/// };
/// assert_eq!(site.family(), MoveType::Move13Add);
/// assert_eq!(view.validate_site(site), Ok(()));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CdtProposalSiteId {
    family: MoveType,
    ordinal: usize,
    instance_id: u64,
    modification_count: u64,
}

impl CdtProposalSiteId {
    /// Returns the move family that owns this site identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// # fn main() -> CdtResult<()> {
    /// let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut moves = ErgodicsSystem::new();
    /// let view = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
    /// if let Some(site) = view.offered_sites().next() {
    ///     assert_eq!(site.family(), MoveType::Move13Add);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn family(self) -> MoveType {
        self.family
    }

    /// Returns this site's zero-based ordinal in its family view.
    ///
    /// Ordinals are deterministic only while the inspected triangulation stays
    /// unchanged. They are not persistent geometry identifiers.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// # fn main() -> CdtResult<()> {
    /// let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// let mut moves = ErgodicsSystem::new();
    /// let view = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
    /// let ordinals = view.offered_sites().map(|site| site.ordinal()).collect::<Vec<_>>();
    /// assert_eq!(ordinals, (0..view.offered_site_count()).collect::<Vec<_>>());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.ordinal
    }
}

/// Failure to use a proposal-site identifier with a policy view.
///
/// The variants distinguish cross-owner identifiers, stale versions,
/// cross-family use, and invalid ordinals so callers never need to parse error
/// strings when deciding whether to rebuild policy inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CdtProposalSiteIdError {
    /// The identifier belongs to a different triangulation instance.
    ForeignTriangulation {
        /// Family recorded by the rejected identifier.
        family: MoveType,
        /// Family-local ordinal recorded by the rejected identifier.
        ordinal: usize,
    },
    /// The triangulation was mutated after the identifier was issued.
    StaleState {
        /// Family recorded by the rejected identifier.
        family: MoveType,
        /// Family-local ordinal recorded by the rejected identifier.
        ordinal: usize,
        /// Modification version captured by the identifier.
        identifier_version: u64,
        /// Current modification version observed through the view.
        current_version: u64,
    },
    /// The identifier belongs to another move-family view.
    FamilyMismatch {
        /// Family selected by the current view.
        expected: MoveType,
        /// Family recorded by the rejected identifier.
        actual: MoveType,
    },
    /// The requested ordinal is outside the current offered-site set.
    OrdinalOutOfRange {
        /// Move family whose site set was indexed.
        family: MoveType,
        /// Rejected zero-based ordinal.
        ordinal: usize,
        /// Number of offered sites in the current view.
        offered_site_count: usize,
    },
}

impl fmt::Display for CdtProposalSiteIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignTriangulation { family, ordinal } => write!(
                formatter,
                "proposal site {}[{ordinal}] belongs to another triangulation",
                family.identifier()
            ),
            Self::StaleState {
                family,
                ordinal,
                identifier_version,
                current_version,
            } => write!(
                formatter,
                "proposal site {}[{ordinal}] is stale: identifier version {identifier_version}, current version {current_version}",
                family.identifier()
            ),
            Self::FamilyMismatch { expected, actual } => write!(
                formatter,
                "proposal site family mismatch: expected {}, received {}",
                expected.identifier(),
                actual.identifier()
            ),
            Self::OrdinalOutOfRange {
                family,
                ordinal,
                offered_site_count,
            } => write!(
                formatter,
                "proposal site {}[{ordinal}] is outside the {offered_site_count}-site offered set",
                family.identifier()
            ),
        }
    }
}

impl Error for CdtProposalSiteIdError {}

/// Allocation-free iterator over opaque offered-site identifiers.
///
/// Identifiers are yielded in ascending family-local ordinal order. The order
/// is deterministic for the unchanged triangulation version borrowed by the
/// parent [`CdtProposalPolicyView`].
#[derive(Debug, Clone)]
pub struct CdtProposalSiteIds {
    family: MoveType,
    instance_id: u64,
    modification_count: u64,
    ordinals: Range<usize>,
}

impl Iterator for CdtProposalSiteIds {
    type Item = CdtProposalSiteId;

    fn next(&mut self) -> Option<Self::Item> {
        self.ordinals.next().map(|ordinal| CdtProposalSiteId {
            family: self.family,
            ordinal,
            instance_id: self.instance_id,
            modification_count: self.modification_count,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.ordinals.size_hint()
    }
}

impl DoubleEndedIterator for CdtProposalSiteIds {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.ordinals.next_back().map(|ordinal| CdtProposalSiteId {
            family: self.family,
            ordinal,
            instance_id: self.instance_id,
            modification_count: self.modification_count,
        })
    }
}

impl ExactSizeIterator for CdtProposalSiteIds {}
impl FusedIterator for CdtProposalSiteIds {}

/// Immutable policy view for one reversible 1+1 CDT move family.
///
/// The view borrows both the canonical triangulation and the selected family's
/// versioned offered-site cache. Rust therefore prevents mutating either owner
/// while the view is alive. Creating the view does not clone the triangulation;
/// after cache synchronization, counting and iterating IDs allocate nothing.
/// An **offered site** passed the deterministic pre-mutation guards and belongs
/// to the conventional sampler's proposal denominator. A later composite
/// backend edit, allocation failure, or post-mutation CDT validation can still
/// reject the sampled site as an ordinary self-loop proposal.
///
/// An **eligible site**, also called an executable site, would satisfy the
/// stronger contract that, for an unchanged state, all deterministic backend
/// mutation preconditions and CDT postconditions needed by the move are known
/// before sampling. This view exposes offered sites, not eligible sites, and
/// therefore does not provide an executable-only action mask or guarantee that
/// execution will succeed.
///
/// A view is family-scoped so conventional sampling can inspect only the chosen
/// family. External policies can iterate [`MoveType::REVERSIBLE_1P1`] and create
/// one short-lived view per family without introducing a second site-enumeration
/// implementation.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
/// use causal_triangulations::prelude::triangulation::*;
///
/// # fn main() -> CdtResult<()> {
/// let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
/// let mut moves = ErgodicsSystem::new();
/// for family in MoveType::REVERSIBLE_1P1 {
///     let view = moves.proposal_policy_view(&triangulation, family);
///     assert_eq!(view.offered_sites().len(), view.offered_site_count());
/// }
/// # Ok(())
/// # }
/// ```
#[must_use = "a proposal-policy view must be inspected before its cache borrow is released"]
pub struct CdtProposalPolicyView<'a> {
    triangulation: &'a CdtTriangulation2D,
    family: MoveType,
    sites: &'a [ProposalSite],
}

impl<'a> CdtProposalPolicyView<'a> {
    /// Creates a public view over an already synchronized canonical site set.
    pub(crate) const fn new(
        triangulation: &'a CdtTriangulation2D,
        family: MoveType,
        sites: &'a [ProposalSite],
    ) -> Self {
        Self {
            triangulation,
            family,
            sites,
        }
    }

    /// Returns the selected move family.
    #[must_use]
    pub const fn family(&self) -> MoveType {
        self.family
    }

    /// Returns the family needed to reverse a realized transition.
    #[must_use]
    pub const fn reverse_family(&self) -> MoveType {
        self.family.reverse()
    }

    /// Returns the current CDT topology without exposing geometry internals.
    #[must_use]
    pub const fn topology(&self) -> CdtTopology {
        self.triangulation.metadata().topology()
    }

    /// Returns the invariant-bearing simplex counts for the inspected state.
    ///
    /// # Errors
    ///
    /// Returns the underlying typed count error if a backend reports a zero
    /// simplex count instead of a constructed CDT state.
    pub fn simplex_counts(&self) -> CdtResult<CdtSimplexCounts> {
        self.triangulation.simplex_counts()
    }

    /// Returns the borrowed per-time-slice vertex counts.
    ///
    /// The slice is empty for an unfoliated state. It borrows the same
    /// triangulation as the policy view and cannot outlive it.
    #[must_use]
    pub fn slice_sizes(&self) -> &[usize] {
        self.triangulation.slice_sizes()
    }

    /// Returns the number of sites offered by this proposal family.
    ///
    /// This is the conventional sampler's proposal denominator, not a promise
    /// that every later backend edit and post-mutation validation will succeed.
    #[must_use]
    pub const fn offered_site_count(&self) -> usize {
        self.sites.len()
    }

    /// Iterates opaque identifiers for every canonical offered site.
    ///
    /// Empty families return an empty exact-size iterator. Iteration allocates
    /// nothing and follows deterministic ascending ordinal order.
    #[must_use]
    pub const fn offered_sites(&self) -> CdtProposalSiteIds {
        CdtProposalSiteIds {
            family: self.family,
            instance_id: self.triangulation.instance_id(),
            modification_count: self.triangulation.metadata().modification_count(),
            ordinals: 0..self.sites.len(),
        }
    }

    /// Returns the identifier at one family-local ordinal.
    ///
    /// # Errors
    ///
    /// Returns [`CdtProposalSiteIdError::OrdinalOutOfRange`] when `ordinal` is
    /// not smaller than [`Self::offered_site_count`].
    pub const fn site_id(
        &self,
        ordinal: usize,
    ) -> Result<CdtProposalSiteId, CdtProposalSiteIdError> {
        if ordinal >= self.sites.len() {
            return Err(CdtProposalSiteIdError::OrdinalOutOfRange {
                family: self.family,
                ordinal,
                offered_site_count: self.sites.len(),
            });
        }
        Ok(CdtProposalSiteId {
            family: self.family,
            ordinal,
            instance_id: self.triangulation.instance_id(),
            modification_count: self.triangulation.metadata().modification_count(),
        })
    }

    /// Validates that an identifier still names a site in this exact view.
    ///
    /// # Errors
    ///
    /// Returns a typed error for foreign triangulations, stale modification
    /// versions, family mismatches, or an invalid ordinal.
    pub fn validate_site(&self, site: CdtProposalSiteId) -> Result<(), CdtProposalSiteIdError> {
        if site.instance_id != self.triangulation.instance_id() {
            return Err(CdtProposalSiteIdError::ForeignTriangulation {
                family: site.family,
                ordinal: site.ordinal,
            });
        }
        let current_version = self.triangulation.metadata().modification_count();
        if site.modification_count != current_version {
            return Err(CdtProposalSiteIdError::StaleState {
                family: site.family,
                ordinal: site.ordinal,
                identifier_version: site.modification_count,
                current_version,
            });
        }
        if site.family != self.family {
            return Err(CdtProposalSiteIdError::FamilyMismatch {
                expected: self.family,
                actual: site.family,
            });
        }
        if site.ordinal >= self.sites.len() {
            return Err(CdtProposalSiteIdError::OrdinalOutOfRange {
                family: self.family,
                ordinal: site.ordinal,
                offered_site_count: self.sites.len(),
            });
        }
        Ok(())
    }

    /// Clones one private site descriptor at a checked cache ordinal.
    pub(crate) fn site_at(&self, ordinal: usize) -> Option<ProposalSite> {
        self.sites.get(ordinal).cloned()
    }
}
