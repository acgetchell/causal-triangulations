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
