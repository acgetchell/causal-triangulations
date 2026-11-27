#![forbid(unsafe_code)]

//! Periodic CDT gluings with a spatial origin on each oriented time slice.

use super::helpers::count_to_f64;
use crate::cdt::ergodic_moves::{MoveType, select_move_family_at};
use crate::cdt::proposal_policy::CdtMoveFamilyDistribution;
use crate::geometry::backends::delaunay::{GluingDraft, IntrinsicGluing};
use markov_chain_monte_carlo::{
    DelayedProposal, DiscreteProposalEndpoint, DiscreteProposalRatio, DiscreteProposalRatioError,
    Target,
};
use rand::{
    Rng,
    distr::{Distribution, Uniform},
};
use std::sync::Arc;
use thiserror::Error;

const MINIMUM_LOOP_LENGTH: usize = 3;
const MINIMUM_TIME_SLICES: usize = 3;

/// Failure to construct or sample a periodic intrinsic CDT gluing.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IntrinsicCdtError {
    /// Periodic time needs at least three slices to distinguish adjacent slabs.
    #[error("intrinsic periodic CDT requires at least three time slices, received {actual}")]
    TooFewTimeSlices {
        /// Requested number of slices.
        actual: usize,
    },
    /// The admissible spatial circles have at least three vertices.
    #[error("intrinsic CDT slice {slice} has length {length}; expected at least three")]
    ShortSpatialLoop {
        /// Zero-based time slice.
        slice: usize,
        /// Requested length of that slice.
        length: usize,
    },
    /// The proposed cell counts exceed the platform's addressable storage.
    #[error("intrinsic CDT cell counts exceed the platform's addressable storage")]
    CellCountOverflow,
    /// The geometry backend could not validate the generated torus.
    #[error("intrinsic CDT gluing validation failed: {detail}")]
    InvalidGluing {
        /// Geometry backend diagnostic.
        detail: String,
    },
    /// A plan was evaluated against a different marked state.
    #[error("intrinsic CDT plan does not belong to the current marked state")]
    SourceMismatch,
    /// The computed endpoints violated the upstream proposal-ratio contract.
    #[error("invalid intrinsic CDT proposal ratio: {0}")]
    ProposalRatio(#[from] DiscreteProposalRatioError),
}

/// One oriented annulus. Starting at the triangle on lower edge zero, visit
/// each Up triangle in lower-edge order and then its run of Down triangles.
/// The first Down edge begins at `upper_start` on the indexed upper circle.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slab {
    down_after_up: Vec<usize>,
    upper_start: usize,
}

impl Slab {
    /// Returns the index of the Down run containing this upper edge.
    fn run_for_upper_edge(&self, edge: usize, upper_length: usize) -> Option<usize> {
        let mut offset = (edge + upper_length - self.upper_start) % upper_length;
        self.down_after_up.iter().position(|&run| {
            if offset < run {
                true
            } else {
                offset -= run;
                false
            }
        })
    }
}

/// A coordinate-independent 1+1 CDT torus.
///
/// Each spatial circle has a fixed time label and a computational origin.
/// Parallel timelike edges retain separate identities in the checked gluing.
///
/// [`crate::CdtTarget`] gives the marked representatives weight proportional
/// to the Boltzmann factor divided by the product of their loop lengths.
/// Forgetting the origins gives the inverse-automorphism reference weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrinsicCdtTriangulation {
    slabs: Arc<[Slab]>,
    gluing: IntrinsicGluing,
}

impl IntrinsicCdtTriangulation {
    /// Constructs a torus with the specified loop lengths and evenly distributed
    /// Down triangles in each slab. The last slab meets the first slice.
    ///
    /// # Errors
    ///
    /// Returns [`IntrinsicCdtError::TooFewTimeSlices`] for fewer than three
    /// slices, [`IntrinsicCdtError::ShortSpatialLoop`] for a loop shorter than
    /// three, [`IntrinsicCdtError::CellCountOverflow`] for unaddressable cell
    /// counts, or [`IntrinsicCdtError::InvalidGluing`] on backend validation failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::IntrinsicCdtTriangulation;
    ///
    /// let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 5, 4])?;
    /// assert_eq!(state.spatial_loop_lengths().collect::<Vec<_>>(), [3, 5, 4]);
    /// assert_eq!(state.triangle_count(), 24);
    /// # Ok::<(), causal_triangulations::IntrinsicCdtError>(())
    /// ```
    pub fn from_spatial_loop_lengths(lengths: &[usize]) -> Result<Self, IntrinsicCdtError> {
        if lengths.len() < MINIMUM_TIME_SLICES {
            return Err(IntrinsicCdtError::TooFewTimeSlices {
                actual: lengths.len(),
            });
        }
        for (slice, &length) in lengths.iter().enumerate() {
            if length < MINIMUM_LOOP_LENGTH {
                return Err(IntrinsicCdtError::ShortSpatialLoop { slice, length });
            }
        }
        checked_vertex_count(lengths.iter().copied())?;
        let slabs = lengths
            .iter()
            .zip(lengths.iter().cycle().skip(1))
            .map(|(&lower, &upper)| Slab {
                down_after_up: (0..lower)
                    .map(|edge| upper / lower + usize::from(edge < upper % lower))
                    .collect(),
                upper_start: 0,
            })
            .collect();
        Self::from_slabs(slabs)
    }

    /// Iterates over the number of spacelike edges in each labeled slice.
    pub fn spatial_loop_lengths(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.slabs.iter().map(|slab| slab.down_after_up.len())
    }

    /// Counts triangle occurrences in the checked gluing.
    #[must_use]
    pub const fn triangle_count(&self) -> usize {
        self.gluing.counts().2
    }

    /// Returns the vertex, edge, and triangle counts used by the CDT action.
    pub(crate) const fn counts(&self) -> (usize, usize, usize) {
        self.gluing.counts()
    }

    /// Builds the validated geometry from a complete marked encoding.
    fn from_slabs(slabs: Vec<Slab>) -> Result<Self, IntrinsicCdtError> {
        let gluing = build_gluing(&slabs)?;
        Ok(Self {
            slabs: slabs.into(),
            gluing,
        })
    }
}

/// Bounds toroidal vertex, edge and face index arithmetic before construction.
fn checked_vertex_count(
    mut lengths: impl Iterator<Item = usize>,
) -> Result<usize, IntrinsicCdtError> {
    lengths
        .try_fold(0_usize, usize::checked_add)
        .filter(|&total| GluingDraft::torus_storage_fits(total))
        .ok_or(IntrinsicCdtError::CellCountOverflow)
}

/// Assigns one occurrence to every spoke in the cyclic walk. Only its final
/// spoke reconnects to the first by identity, even if endpoints repeat sooner.
fn build_gluing(slabs: &[Slab]) -> Result<IntrinsicGluing, IntrinsicCdtError> {
    let vertices = checked_vertex_count(slabs.iter().map(|slab| slab.down_after_up.len()))?;
    let mut draft = GluingDraft::new(vertices);
    let mut start = 0;
    let mut circles = Vec::with_capacity(slabs.len());
    for slab in slabs {
        let length = slab.down_after_up.len();
        let edges = (0..length)
            .map(|vertex| draft.edge(start + vertex, start + (vertex + 1) % length))
            .collect::<Vec<_>>();
        circles.push((start, edges));
        start += length;
    }
    let upper_circles = circles.iter().cycle().skip(1);
    for ((slab, (lower_start, lower_edges)), (upper_start, upper_edges)) in
        slabs.iter().zip(&circles).zip(upper_circles)
    {
        let mut lower = 0;
        let mut upper = slab.upper_start;
        let first_spoke = draft.edge(*lower_start, upper_start + upper);
        let mut spoke = first_spoke;
        let mut remaining = lower_edges.len() + upper_edges.len();
        for (&spatial, &run) in lower_edges.iter().zip(&slab.down_after_up) {
            lower = (lower + 1) % lower_edges.len();
            remaining -= 1;
            let next = if remaining == 0 {
                first_spoke
            } else {
                draft.edge(lower_start + lower, upper_start + upper)
            };
            draft.triangle([(spatial, false), (next, false), (spoke, true)]);
            spoke = next;
            for _ in 0..run {
                let spatial = upper_edges[upper];
                upper = (upper + 1) % upper_edges.len();
                remaining -= 1;
                let next = if remaining == 0 {
                    first_spoke
                } else {
                    draft.edge(lower_start + lower, upper_start + upper)
                };
                draft.triangle([(spoke, true), (next, false), (spatial, true)]);
                spoke = next;
            }
        }
    }
    draft
        .finish()
        .map_err(|detail| IntrinsicCdtError::InvalidGluing { detail })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Flip {
        slab: usize,
        run: usize,
        toward_previous: bool,
    },
    Insert {
        slice: usize,
        edge: usize,
    },
    Remove {
        slice: usize,
        vertex: usize,
    },
}

impl Site {
    /// Identifies the time slice or slab whose change distinguishes a transition.
    const fn time(self) -> usize {
        match self {
            Self::Flip { slab, .. } => slab,
            Self::Insert { slice, .. } | Self::Remove { slice, .. } => slice,
        }
    }
}

/// Lists the sites before realization. All non-origin vertices above the
/// minimum are offered to removal, including ones that fail the collapse check.
fn offered_sites(slabs: &[Slab], family: MoveType) -> Vec<Site> {
    let mut sites = Vec::new();
    for (time, slab) in slabs.iter().enumerate() {
        let length = slab.down_after_up.len();
        match family {
            MoveType::Move22 | MoveType::EdgeFlip => {
                for (run, &count) in slab.down_after_up.iter().enumerate() {
                    if count > 0 {
                        sites.push(Site::Flip {
                            slab: time,
                            run,
                            toward_previous: true,
                        });
                        sites.push(Site::Flip {
                            slab: time,
                            run,
                            toward_previous: false,
                        });
                    }
                }
            }
            MoveType::Move13Add => {
                sites.extend((0..length).map(|edge| Site::Insert { slice: time, edge }));
            }
            MoveType::Move31Remove if length > MINIMUM_LOOP_LENGTH => {
                sites.extend((1..length).map(|vertex| Site::Remove {
                    slice: time,
                    vertex,
                }));
            }
            MoveType::Move31Remove => {}
        }
    }
    sites
}

/// Cyclic predecessor for an index already known to be in bounds.
const fn previous(index: usize, length: usize) -> usize {
    if index == 0 { length - 1 } else { index - 1 }
}

/// Realizes one local edit in the marked encoding. A rejected collapse leaves
/// the offered-site denominator intact.
fn apply_site(slabs: &mut [Slab], site: Site) -> Option<()> {
    match site {
        Site::Flip {
            slab,
            run,
            toward_previous,
        } => {
            let upper_length = slabs[(slab + 1) % slabs.len()].down_after_up.len();
            let slab = &mut slabs[slab];
            let runs = &mut slab.down_after_up;
            let target = if toward_previous {
                if run == 0 {
                    slab.upper_start = (slab.upper_start + 1) % upper_length;
                }
                previous(run, runs.len())
            } else {
                let target = (run + 1) % runs.len();
                if target == 0 {
                    slab.upper_start = previous(slab.upper_start, upper_length);
                }
                target
            };
            runs[run] -= 1;
            runs[target] += 1;
        }
        Site::Insert { slice, edge } => {
            let upper_length = slabs[slice].down_after_up.len();
            let incoming = previous(slice, slabs.len());
            let run = slabs[incoming].run_for_upper_edge(edge, upper_length)?;
            let incoming = &mut slabs[incoming];
            incoming.down_after_up[run] += 1;
            incoming.upper_start += usize::from(incoming.upper_start > edge);
            slabs[slice].down_after_up.insert(edge, 0);
        }
        Site::Remove { slice, vertex } => {
            let upper_length = slabs[slice].down_after_up.len();
            let incoming = previous(slice, slabs.len());
            let lower = &slabs[incoming];
            let run = lower.run_for_upper_edge(vertex - 1, upper_length)?;
            if slabs[slice].down_after_up[vertex - 1] != 0
                || lower.upper_start == vertex
                || lower.run_for_upper_edge(vertex, upper_length) != Some(run)
            {
                return None;
            }
            let incoming = &mut slabs[incoming];
            incoming.down_after_up[run] -= 1;
            incoming.upper_start -= usize::from(incoming.upper_start > vertex);
            slabs[slice].down_after_up.remove(vertex - 1);
        }
    }
    Some(())
}

/// Sums all atoms giving one marked destination. A flip changes only its own
/// slab and a volume change changes one specific loop length, so sites at other
/// times cannot reach that same destination.
fn transition_probability(
    source: &[Slab],
    destination: &[Slab],
    sites: &[Site],
    time: usize,
    family_probability: f64,
) -> f64 {
    if sites.is_empty() || family_probability == 0.0 {
        return 0.0;
    }
    let incoming = previous(time, source.len());
    let mut draft = source.to_vec();
    let matching = sites
        .iter()
        .filter(|&&site| {
            if site.time() != time {
                return false;
            }
            let matches = apply_site(&mut draft, site).is_some()
                && draft[time] == destination[time]
                && draft[incoming] == destination[incoming];
            draft[time].clone_from(&source[time]);
            draft[incoming].clone_from(&source[incoming]);
            matches
        })
        .count();
    family_probability * (count_to_f64(matching) / count_to_f64(sites.len()))
}

/// A concrete transition and its marginal proposal mass in each direction.
#[derive(Debug, Clone)]
pub struct IntrinsicCdtPlan {
    source: Arc<[Slab]>,
    proposed: IntrinsicCdtTriangulation,
    family: MoveType,
    ratio: DiscreteProposalRatio,
}

impl IntrinsicCdtPlan {
    /// Returns the proposed state, which has already passed geometry validation.
    #[must_use]
    pub const fn proposed_state(&self) -> &IntrinsicCdtTriangulation {
        &self.proposed
    }

    /// Returns `(q(proposed | source), q(source | proposed))` for the marked states.
    ///
    /// Each probability sums all offered atoms that realize the endpoint.
    #[must_use]
    pub const fn transition_probabilities(&self) -> (f64, f64) {
        (self.ratio.forward_weight(), self.ratio.reverse_weight())
    }

    /// Returns `log q(source | proposed) - log q(proposed | source)`.
    ///
    /// The correction is negative infinity when the source cannot be proposed
    /// from the destination.
    #[must_use]
    pub fn log_proposal_ratio(&self) -> f64 {
        self.ratio.log_q_ratio()
    }

    /// Verifies that the plan is evaluated or committed at its source.
    fn check_source(&self, state: &IntrinsicCdtTriangulation) -> Result<(), IntrinsicCdtError> {
        if self.source == state.slabs {
            Ok(())
        } else {
            Err(IntrinsicCdtError::SourceMismatch)
        }
    }
}

/// Planned local proposals on coordinate-independent periodic CDT states.
///
/// Use with [`crate::CdtTarget`] and the upstream delayed sampler. A fixed
/// family distribution supplies the family weights and sites are sampled
/// uniformly within the selected family. The default gives all four families
/// equal weight.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     ActionConfig, CdtTarget, Chain, IntrinsicCdtProposal, IntrinsicCdtTriangulation,
/// };
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # use causal_triangulations::{CdtError, IntrinsicCdtError, McmcError};
/// # use markov_chain_monte_carlo::DelayedStepError;
/// # #[derive(Debug, thiserror::Error)]
/// # enum ExampleError {
/// #     #[error(transparent)]
/// #     Construction(#[from] IntrinsicCdtError),
/// #     #[error(transparent)]
/// #     Target(#[from] CdtError),
/// #     #[error(transparent)]
/// #     Chain(#[from] McmcError),
/// #     #[error(transparent)]
/// #     Step(#[from] DelayedStepError<IntrinsicCdtError>),
/// # }
/// # fn main() -> Result<(), ExampleError> {
/// let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[4, 4, 4, 4])?;
/// let target = CdtTarget::new(ActionConfig::default(), 1.0)?;
/// let mut proposal = IntrinsicCdtProposal::default();
/// let mut rng = StdRng::seed_from_u64(316);
/// let mut chain = Chain::new(state, &target)?;
/// chain.step_delayed(&target, &mut proposal, &mut rng)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct IntrinsicCdtProposal {
    distribution: CdtMoveFamilyDistribution,
    no_plan_family: Option<MoveType>,
}

impl IntrinsicCdtProposal {
    /// Sets the fixed family distribution. Empty selected families and rejected
    /// sites produce self-loops without reweighting any other site.
    #[must_use]
    pub const fn new(distribution: CdtMoveFamilyDistribution) -> Self {
        Self {
            distribution,
            no_plan_family: None,
        }
    }

    /// Both flip-family names sample the same set of local edits.
    fn transition_family_probability(&self, family: MoveType) -> f64 {
        match family {
            MoveType::Move22 | MoveType::EdgeFlip => {
                self.distribution.probability(MoveType::Move22)
                    + self.distribution.probability(MoveType::EdgeFlip)
            }
            _ => self.distribution.probability(family),
        }
    }

    /// Scores the sampled atom using the two complete endpoint site sets.
    fn plan_selected_site(
        &self,
        state: &IntrinsicCdtTriangulation,
        family: MoveType,
        sites: &[Site],
        site: Site,
    ) -> Result<Option<IntrinsicCdtPlan>, IntrinsicCdtError> {
        let mut destination = state.slabs.to_vec();
        if apply_site(&mut destination, site).is_none() {
            return Ok(None);
        }
        let reverse_family = family.reverse();
        let reverse_sites = offered_sites(&destination, reverse_family);
        let forward = transition_probability(
            &state.slabs,
            &destination,
            sites,
            site.time(),
            self.transition_family_probability(family),
        );
        let reverse = transition_probability(
            &destination,
            &state.slabs,
            &reverse_sites,
            site.time(),
            self.transition_family_probability(reverse_family),
        );
        let ratio = DiscreteProposalRatio::from_endpoints(
            DiscreteProposalEndpoint::new(forward, 1.0, 1),
            DiscreteProposalEndpoint::new(reverse, 1.0, 1),
        )?;
        let proposed = IntrinsicCdtTriangulation::from_slabs(destination)?;
        Ok(Some(IntrinsicCdtPlan {
            source: Arc::clone(&state.slabs),
            proposed,
            family,
            ratio,
        }))
    }
}

impl DelayedProposal<IntrinsicCdtTriangulation> for IntrinsicCdtProposal {
    type Plan = IntrinsicCdtPlan;
    type Info = MoveType;
    type Error = IntrinsicCdtError;

    fn propose_plan<R: Rng + ?Sized>(
        &mut self,
        state: &IntrinsicCdtTriangulation,
        rng: &mut R,
    ) -> Result<Option<Self::Plan>, Self::Error> {
        self.no_plan_family = None;
        let family = select_move_family_at(&self.distribution, rng.next_u64() >> 11);
        let sites = offered_sites(&state.slabs, family);
        let plan = match Uniform::new(0, sites.len()) {
            Ok(uniform) => {
                self.plan_selected_site(state, family, &sites, sites[uniform.sample(rng)])?
            }
            Err(_) => None,
        };
        self.no_plan_family = plan.is_none().then_some(family);
        Ok(plan)
    }

    fn no_plan_info(&mut self) -> Option<Self::Info> {
        self.no_plan_family.take()
    }

    fn proposed_log_prob<T: Target<IntrinsicCdtTriangulation> + ?Sized>(
        &self,
        state: &IntrinsicCdtTriangulation,
        plan: &Self::Plan,
        target: &T,
    ) -> Result<f64, Self::Error> {
        plan.check_source(state)?;
        Ok(target.log_prob(&plan.proposed))
    }

    fn log_q_ratio(
        &self,
        state: &IntrinsicCdtTriangulation,
        plan: &Self::Plan,
    ) -> Result<f64, Self::Error> {
        plan.check_source(state)?;
        Ok(plan.log_proposal_ratio())
    }

    fn info(&self, plan: &Self::Plan) -> Self::Info {
        plan.family
    }

    fn commit<R: Rng + ?Sized>(
        &mut self,
        state: &mut IntrinsicCdtTriangulation,
        plan: Self::Plan,
        _rng: &mut R,
    ) -> Result<(), Self::Error> {
        plan.check_source(state)?;
        *state = plan.proposed;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
