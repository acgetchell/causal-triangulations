#![forbid(unsafe_code)]

//! Deterministic heap-allocation checks for CDT queries, proposals, and telemetry.

#[path = "support/or_abort.rs"]
mod benchmark_support;

use benchmark_support::OrAbort;
use causal_triangulations::prelude::geometry::TriangulationQuery;
use causal_triangulations::prelude::simulation::{
    ActionConfig, CdtMoveFamilyDistribution, CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError,
    CdtProposal, CdtProposalPlan, CdtProposalPolicyView, DelayedProposal, MetropolisAlgorithm,
    MetropolisConfig, MoveType,
};
use causal_triangulations::prelude::triangulation::CdtTriangulation2D;
use rand::{SeedableRng, rngs::StdRng};
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn main() {
    let triangulation =
        CdtTriangulation2D::from_cdt_strip(20, 10).or_abort("build allocation benchmark fixture");

    black_box(triangulation.edge_count());
    black_box(
        triangulation
            .slab_triangle_profile()
            .or_abort("prime slab-triangle-profile cache"),
    );

    let stats = measure_allocations("cached observables", || {
        black_box(triangulation.edge_count());
        black_box(
            triangulation
                .slab_triangle_profile()
                .or_abort("read cached profile"),
        );
    });
    assert_eq!(stats.total_blocks, 1);

    let edge = triangulation
        .geometry()
        .edges()
        .next()
        .or_abort("fixture edge");
    let stats = measure_allocations("100 checked edge endpoint queries", || {
        for _ in 0..100 {
            black_box(
                triangulation
                    .geometry()
                    .edge_endpoints(&edge)
                    .or_abort("live edge"),
            );
        }
    });
    assert_eq!(stats.total_blocks, 0);
    check_proposal_cache(triangulation);
    check_unchanged_trace();
}

/// Measures only the operation; diagnostics remain outside the allocation window.
fn measure_allocations(label: &str, operation: impl FnOnce()) -> dhat::HeapStats {
    let profiler = dhat::Profiler::builder().testing().build();
    operation();
    let stats = dhat::HeapStats::get();
    drop(profiler);
    println!(
        "{label}: {} allocations, {} bytes, {} live bytes",
        stats.total_blocks, stats.total_bytes, stats.curr_bytes
    );
    stats
}

/// Finds a concrete flip without committing; ordinary local rejections remain allowed.
fn concrete_flip<P: CdtMoveFamilyPolicy>(
    proposal: &mut CdtProposal<P>,
    state: &CdtTriangulation2D,
    rng: &mut StdRng,
) -> CdtProposalPlan {
    (0..64)
        .find_map(|_| {
            proposal
                .propose_plan(state, rng)
                .or_abort("plan concrete flip")
        })
        .or_abort("fixture must offer a concrete flip within the bounded search")
}

/// Both discarded and committed plans retain the matching owner-bound cache entry.
fn check_proposal_cache(mut triangulation: CdtTriangulation2D) {
    let policy =
        CdtMoveFamilyDistribution::from_weights([1.0, 0.0, 0.0, 0.0]).or_abort("flip policy");
    let mut proposal = CdtProposal::new(ActionConfig::default())
        .with_seed(7)
        .with_policy(policy);
    let mut rng = StdRng::seed_from_u64(11);
    let count = proposal
        .policy_view(&triangulation, MoveType::Move22)
        .offered_site_count();
    let stats = measure_allocations("warm live proposal view", || {
        assert_eq!(
            proposal
                .policy_view(&triangulation, MoveType::Move22)
                .offered_site_count(),
            count
        );
    });
    assert_eq!(stats.total_blocks, 0);
    for _ in 0..3 {
        drop(concrete_flip(&mut proposal, &triangulation, &mut rng));
        let stats = measure_allocations("live view after discarded flip", || {
            assert_eq!(
                proposal
                    .policy_view(&triangulation, MoveType::Move22)
                    .offered_site_count(),
                count
            );
        });
        assert_eq!(stats.total_blocks, 0);
    }
    let plan = concrete_flip(&mut proposal, &triangulation, &mut rng);
    let reverse_count = plan.reverse_site_count();
    proposal
        .commit(&mut triangulation, plan, &mut rng)
        .or_abort("commit flip");
    let stats = measure_allocations("live view after committed flip", || {
        assert_eq!(
            proposal
                .policy_view(&triangulation, MoveType::Move22)
                .offered_site_count(),
            reverse_count
        );
    });
    assert_eq!(stats.total_blocks, 0);
    check_state_dependent_cache(&triangulation);
}

/// Forces all family views to be evaluated while sampling only flips.
struct InspectAllFamilies;

impl CdtMoveFamilyPolicy for InspectAllFamilies {
    fn family_weight(
        &self,
        view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError> {
        Ok(if view.family() == MoveType::Move22 {
            1.0
        } else {
            0.0
        })
    }
}

/// Reverse policy evaluation must retain every live family, including empty ones.
fn check_state_dependent_cache(state: &CdtTriangulation2D) {
    let mut proposal = CdtProposal::new(ActionConfig::default())
        .with_seed(7)
        .with_policy(InspectAllFamilies);
    let counts = MoveType::REVERSIBLE_1P1
        .map(|family| proposal.policy_view(state, family).offered_site_count());
    let mut rng = StdRng::seed_from_u64(11);
    for _ in 0..3 {
        drop(concrete_flip(&mut proposal, state, &mut rng));
        let allocations =
            measure_allocations("all live family views after discarded policy plan", || {
                for (family, count) in MoveType::REVERSIBLE_1P1.into_iter().zip(counts) {
                    assert_eq!(
                        proposal.policy_view(state, family).offered_site_count(),
                        count
                    );
                }
            });
        assert_eq!(allocations.total_blocks, 0);
    }
}

/// A long unchanged trajectory must not retain a separate profile per trace/measurement row.
fn check_unchanged_trace() {
    let triangulation = CdtTriangulation2D::from_cdt_strip(4, 64).or_abort("long profile fixture");
    let config = MetropolisConfig::new(1.0, 1000, 0, 1)
        .or_abort("trace config")
        .with_seed(7);
    let policy =
        CdtMoveFamilyDistribution::from_weights([0.0, 0.0, 1.0, 0.0]).or_abort("removal policy");
    let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default()).with_policy(policy);
    let mut result = None;
    let stats = measure_allocations("1000 unchanged steps with 64 profile entries", || {
        result = Some(
            algorithm
                .run(triangulation)
                .or_abort("unchanged trajectory"),
        );
    });
    let result = result.or_abort("recorded result");
    assert_eq!(result.move_stats().total_accepted(), 0);
    assert_eq!(result.measurements().len(), 1001);
    // The old duplicate profiles alone retained over 512 KB. This budget leaves
    // room for scalar rows and counters on both 32-bit and 64-bit platforms.
    assert!(
        stats.curr_bytes < 500_000,
        "unchanged trace retained {} bytes",
        stats.curr_bytes
    );
    let profile = result.measurements()[0].slab_triangle_profile();
    assert!(
        result
            .measurements()
            .iter()
            .all(|measurement| measurement.slab_triangle_profile().as_ptr() == profile.as_ptr())
    );
}
