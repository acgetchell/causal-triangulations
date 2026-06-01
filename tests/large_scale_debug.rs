#![cfg_attr(not(feature = "slow-tests"), allow(dead_code))]
#![forbid(unsafe_code)]

//! Large-scale CDT debug harnesses.
//!
//! These tests are intended for manual performance and invariant debugging at
//! sizes larger than the default integration-test budget. They are gated behind
//! the `slow-tests` feature and should usually be run through the matching
//! `just debug-large-scale-*` and `just perf-large-scale-debug` recipes.

#[cfg(feature = "slow-tests")]
use std::time::Instant;
use std::{env, time::Duration};

use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
use causal_triangulations::prelude::simulation::{
    ActionConfig, CdtMcmcCheckpoint, CdtTopology, CdtTriangulation2D, MetropolisAlgorithm,
    MetropolisConfig, ProposalStatistics, TriangulationQuery,
};

const DEFAULT_TOTAL_VERTICES: u32 = 512;
const DEFAULT_TIMESLICES: u32 = 16;
const DEFAULT_SWEEPS: u32 = 10;
const DEFAULT_SEED: u64 = 0xCD71_0139;

#[derive(Clone, Copy, Default)]
struct MoveAcceptanceCounts {
    total: u64,
    hard_failures: u64,
    move_22: u64,
    move_13: u64,
    move_31: u64,
    edge_flip: u64,
}

#[derive(Clone, Copy, Default)]
struct ProposalOutcomeCounts {
    proposals: u64,
    accepted: u64,
    rejected: u64,
    no_site: u64,
    site_rejections: u64,
    metropolis_rejections: u64,
    hard_failures: u64,
}

impl ProposalOutcomeCounts {
    /// Captures cumulative proposal-kernel counters.
    const fn from_stats(stats: &ProposalStatistics) -> Self {
        let site_rejections = stats
            .site_causality_rejections()
            .saturating_add(stats.site_geometric_rejections())
            .saturating_add(stats.site_backend_rejections());
        let rejected = stats
            .no_site_proposals()
            .saturating_add(site_rejections)
            .saturating_add(stats.metropolis_rejections())
            .saturating_add(stats.hard_failures());
        Self {
            proposals: stats.move_family_proposals(),
            accepted: stats.accepted_transitions(),
            rejected,
            no_site: stats.no_site_proposals(),
            site_rejections,
            metropolis_rejections: stats.metropolis_rejections(),
            hard_failures: stats.hard_failures(),
        }
    }

    /// Returns per-sweep proposal deltas relative to the previous cumulative snapshot.
    const fn delta_since(self, previous: Self) -> Self {
        Self {
            proposals: self.proposals - previous.proposals,
            accepted: self.accepted - previous.accepted,
            rejected: self.rejected - previous.rejected,
            no_site: self.no_site - previous.no_site,
            site_rejections: self.site_rejections - previous.site_rejections,
            metropolis_rejections: self.metropolis_rejections - previous.metropolis_rejections,
            hard_failures: self.hard_failures - previous.hard_failures,
        }
    }
}

impl MoveAcceptanceCounts {
    /// Captures the accepted and hard-failure counters from cumulative move statistics.
    const fn from_stats(stats: &MoveStatistics) -> Self {
        Self {
            total: stats.total_accepted(),
            hard_failures: stats.total_hard_failures(),
            move_22: accepted_count(stats, MoveType::Move22),
            move_13: accepted_count(stats, MoveType::Move13Add),
            move_31: accepted_count(stats, MoveType::Move31Remove),
            edge_flip: accepted_count(stats, MoveType::EdgeFlip),
        }
    }

    /// Returns per-sweep counter deltas relative to the previous cumulative snapshot.
    const fn delta_since(self, previous: Self) -> Self {
        Self {
            total: self.total - previous.total,
            hard_failures: self.hard_failures - previous.hard_failures,
            move_22: self.move_22 - previous.move_22,
            move_13: self.move_13 - previous.move_13,
            move_31: self.move_31 - previous.move_31,
            edge_flip: self.edge_flip - previous.edge_flip,
        }
    }
}

/// Reads an unsigned 32-bit environment variable or returns the supplied default.
fn env_u32(name: &str, default: u32) -> u32 {
    env::var(name).ok().map_or(default, |value| {
        value
            .parse::<u32>()
            .unwrap_or_else(|_| panic!("{name} must be an unsigned 32-bit integer"))
    })
}

/// Reads a case-specific unsigned 32-bit variable before falling back to a generic one.
fn env_u32_prefer(specific_name: &str, generic_name: &str, default: u32) -> u32 {
    env::var(specific_name).ok().map_or_else(
        || env_u32(generic_name, default),
        |value| {
            value
                .parse::<u32>()
                .unwrap_or_else(|_| panic!("{specific_name} must be an unsigned 32-bit integer"))
        },
    )
}

/// Reads an unsigned 64-bit environment variable, accepting decimal or hexadecimal input.
fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .map_or(default, |value| parse_u64(name, &value))
}

/// Reads a case-specific unsigned 64-bit variable before falling back to a generic one.
fn env_u64_prefer(specific_name: &str, generic_name: &str, default: u64) -> u64 {
    env::var(specific_name).ok().map_or_else(
        || env_u64(generic_name, default),
        |value| parse_u64(specific_name, &value),
    )
}

/// Parses decimal or `0x`-prefixed hexadecimal unsigned 64-bit values for debug seeds.
fn parse_u64(name: &str, value: &str) -> u64 {
    let trimmed = value.trim();
    trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .map_or_else(
            || {
                trimmed
                    .parse::<u64>()
                    .unwrap_or_else(|_| panic!("{name} must be an unsigned 64-bit integer"))
            },
            |hex| {
                u64::from_str_radix(hex, 16)
                    .unwrap_or_else(|_| panic!("{name} must be an unsigned 64-bit integer"))
            },
        )
}

/// Returns the optional wall-clock cap used to stop unexpectedly slow debug runs.
fn optional_runtime_cap() -> Option<Duration> {
    let seconds = env_u64("CDT_LARGE_DEBUG_MAX_RUNTIME_SECS", 0);
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

/// Converts total vertices into a regular per-slice count for the current constructor.
fn vertices_per_slice(total_vertices: u32, timeslices: u32) -> u32 {
    assert!(
        timeslices > 0,
        "CDT_LARGE_DEBUG_TIMESLICES must be positive"
    );
    assert!(
        total_vertices >= timeslices,
        "CDT_LARGE_DEBUG_VERTICES must be at least CDT_LARGE_DEBUG_TIMESLICES"
    );
    assert_eq!(
        total_vertices % timeslices,
        0,
        "CDT_LARGE_DEBUG_VERTICES must divide evenly by CDT_LARGE_DEBUG_TIMESLICES"
    );

    total_vertices / timeslices
}

/// Checks the toroidal invariants that every large-scale random sweep must preserve.
fn assert_toroidal_invariants(triangulation: &CdtTriangulation2D) {
    triangulation
        .validate()
        .expect("large-scale toroidal CDT should validate after random sweeps");
    assert_eq!(triangulation.metadata().topology(), CdtTopology::Toroidal);
    assert_eq!(triangulation.geometry().euler_characteristic(), 0);
}

/// Builds a chunk configuration for one unfixed-volume Metropolis debug sweep.
fn sweep_config(attempts: usize, seed: u64) -> MetropolisConfig {
    let steps = u32::try_from(attempts).expect("sweep attempt count should fit in u32");
    MetropolisConfig::new(1.0, steps, 0, 1)
        .expect("sweep config should be valid")
        .with_seed(seed)
}

/// Runs one Metropolis chunk and keeps resumable checkpoint state for the next sweep.
fn run_metropolis_sweep(
    checkpoint: Option<CdtMcmcCheckpoint>,
    triangulation: Option<CdtTriangulation2D>,
    attempts: usize,
    seed: u64,
    action_config: &ActionConfig,
) -> CdtMcmcCheckpoint {
    let algorithm = MetropolisAlgorithm::new(sweep_config(attempts, seed), action_config.clone());
    checkpoint.map_or_else(
        || {
            algorithm
                .run_to_checkpoint(
                    triangulation.expect("initial triangulation is required for the first sweep"),
                )
                .expect("large-scale Metropolis sweep should run")
        },
        |checkpoint| {
            algorithm
                .resume_to_checkpoint(checkpoint)
                .expect("large-scale Metropolis sweep should resume")
        },
    )
}

/// Reads the accepted counter for one move type.
const fn accepted_count(stats: &MoveStatistics, move_type: MoveType) -> u64 {
    stats.accepted(move_type)
}

#[cfg(feature = "slow-tests")]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "debug harness keeps setup, chunk execution, and printed telemetry together"
)]
fn debug_large_scale_1p1() {
    let total_vertices = env_u32_prefer(
        "CDT_LARGE_DEBUG_VERTICES_1P1",
        "CDT_LARGE_DEBUG_VERTICES",
        DEFAULT_TOTAL_VERTICES,
    );
    let timeslices = env_u32_prefer(
        "CDT_LARGE_DEBUG_TIMESLICES_1P1",
        "CDT_LARGE_DEBUG_TIMESLICES",
        DEFAULT_TIMESLICES,
    );
    let sweeps = env_u32_prefer(
        "CDT_LARGE_DEBUG_SWEEPS_1P1",
        "CDT_LARGE_DEBUG_SWEEPS",
        DEFAULT_SWEEPS,
    );
    let seed = env_u64_prefer(
        "CDT_LARGE_DEBUG_SEED_1P1",
        "CDT_LARGE_DEBUG_SEED",
        DEFAULT_SEED,
    );
    let vertices_per_slice = vertices_per_slice(total_vertices, timeslices);
    let runtime_cap = optional_runtime_cap();

    let started_at = Instant::now();
    let triangulation = CdtTriangulation2D::from_toroidal_cdt(vertices_per_slice, timeslices)
        .expect("large-scale toroidal CDT fixture should build");
    let initial_profile = triangulation.volume_profile();
    let mut expected_attempts = 0_u64;
    let mut previous_acceptance = MoveAcceptanceCounts::default();
    let mut previous_proposals = ProposalOutcomeCounts::default();
    let mut previous_elapsed = Duration::ZERO;
    let expected_vertices =
        usize::try_from(total_vertices).expect("vertex count should fit in usize");
    let action_config = ActionConfig::default();
    let mut checkpoint: Option<CdtMcmcCheckpoint> = None;
    let mut pending_initial_triangulation = Some(triangulation);

    let initial_triangulation = pending_initial_triangulation
        .as_ref()
        .expect("initial triangulation is available before the first sweep");
    assert_eq!(initial_triangulation.vertex_count(), expected_vertices);
    assert_toroidal_invariants(initial_triangulation);

    println!(
        "1+1 toroidal CDT large-scale Metropolis debug: vertices={total_vertices}, timeslices={timeslices}, \
         vertices_per_slice={vertices_per_slice}, initial_simplices={}, sweeps={sweeps}, seed=0x{seed:X}, \
         ensemble=unfixed-volume",
        initial_triangulation.face_count()
    );

    for sweep in 1..=sweeps {
        let attempts = checkpoint.as_ref().map_or_else(
            || {
                pending_initial_triangulation
                    .as_ref()
                    .expect("initial triangulation is available before the first sweep")
                    .face_count()
            },
            |checkpoint| checkpoint.triangulation().face_count(),
        );
        expected_attempts += u64::try_from(attempts).expect("attempt count should fit in u64");

        checkpoint = Some(run_metropolis_sweep(
            checkpoint,
            pending_initial_triangulation.take(),
            attempts,
            seed,
            &action_config,
        ));

        let checkpoint_ref = checkpoint
            .as_ref()
            .expect("Metropolis sweep should produce a checkpoint");
        let triangulation = checkpoint_ref.triangulation();
        assert_toroidal_invariants(triangulation);
        let acceptance = MoveAcceptanceCounts::from_stats(checkpoint_ref.move_stats());
        let sweep_acceptance = acceptance.delta_since(previous_acceptance);
        let proposals = ProposalOutcomeCounts::from_stats(checkpoint_ref.proposal_stats());
        let sweep_proposals = proposals.delta_since(previous_proposals);
        let elapsed = started_at.elapsed();
        let sweep_elapsed = elapsed.saturating_sub(previous_elapsed);
        println!(
            "sweep {sweep}/{sweeps}: sweep_proposals={}, final_vertices={}, final_edges={}, final_simplices={}, \
             final_volume_profile={:?}, \
             sweep_accepted={} (Move22={}, Move13Add={}, Move31Remove={}, EdgeFlip={}), \
             sweep_rejected={}, sweep_no_site={}, sweep_site_rejections={}, sweep_metropolis_rejections={}, \
             sweep_hard_failures={}, total_accepted={}, total_proposals={}, total_rejected={}, total_hard_failures={}, \
             sweep_elapsed={sweep_elapsed:?}, total_elapsed={elapsed:?}",
            sweep_proposals.proposals,
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
            triangulation.volume_profile(),
            sweep_acceptance.total,
            sweep_acceptance.move_22,
            sweep_acceptance.move_13,
            sweep_acceptance.move_31,
            sweep_acceptance.edge_flip,
            sweep_proposals.rejected,
            sweep_proposals.no_site,
            sweep_proposals.site_rejections,
            sweep_proposals.metropolis_rejections,
            sweep_acceptance.hard_failures,
            acceptance.total,
            proposals.proposals,
            proposals.rejected,
            acceptance.hard_failures,
        );
        previous_acceptance = acceptance;
        previous_proposals = proposals;
        previous_elapsed = elapsed;

        if let Some(cap) = runtime_cap {
            assert!(
                elapsed <= cap,
                "large-scale CDT debug run exceeded CDT_LARGE_DEBUG_MAX_RUNTIME_SECS={}",
                cap.as_secs()
            );
        }
    }

    let checkpoint = checkpoint.expect("at least one sweep should run");
    let triangulation = checkpoint.triangulation();
    assert_eq!(checkpoint.move_stats().total_attempted(), expected_attempts);
    assert_eq!(
        checkpoint.proposal_stats().move_family_proposals(),
        expected_attempts
    );
    assert!(
        checkpoint.move_stats().total_accepted() > 0,
        "large-scale Metropolis sweeps should accept at least one move"
    );
    assert!(
        checkpoint.move_stats().accepted(MoveType::Move13Add) > 0,
        "large-scale Metropolis sweeps should exercise toroidal Move13Add"
    );
    assert!(
        checkpoint.move_stats().accepted(MoveType::Move31Remove) > 0,
        "large-scale Metropolis sweeps should exercise toroidal Move31Remove"
    );
    assert_ne!(
        triangulation.volume_profile(),
        initial_profile,
        "large-scale Metropolis sweeps should change the toroidal volume profile"
    );
}
