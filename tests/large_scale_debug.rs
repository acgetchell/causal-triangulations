#![cfg_attr(not(feature = "slow-tests"), allow(dead_code))]
#![forbid(unsafe_code)]

//! Large-scale CDT debug harnesses.
//!
//! These tests are intended for manual performance and invariant debugging at
//! sizes larger than the default integration-test budget. They are gated behind
//! the `slow-tests` feature and should usually be run through the matching
//! `just debug-large-scale-*` and `just perf-large-scale-debug` recipes.

#[cfg(feature = "slow-tests")]
use causal_triangulations::prelude::moves::ErgodicsSystem;
use causal_triangulations::prelude::moves::{MoveStatistics, MoveType};
use causal_triangulations::prelude::triangulation::{
    CdtTopology, CdtTriangulation2D, TriangulationQuery,
};
use std::{env, time::Duration};
#[cfg(feature = "slow-tests")]
use std::{hint::black_box, time::Instant};

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
    assert_eq!(triangulation.metadata().topology, CdtTopology::Toroidal);
    assert_eq!(triangulation.geometry().euler_characteristic(), 0);
}

/// Reads the accepted counter for one move type.
const fn accepted_count(stats: &MoveStatistics, move_type: MoveType) -> u64 {
    match move_type {
        MoveType::Move22 => stats.moves_22_accepted,
        MoveType::Move13Add => stats.moves_13_accepted,
        MoveType::Move31Remove => stats.moves_31_accepted,
        MoveType::EdgeFlip => stats.edge_flips_accepted,
    }
}

#[cfg(feature = "slow-tests")]
#[test]
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
    let mut triangulation = CdtTriangulation2D::from_toroidal_cdt(vertices_per_slice, timeslices)
        .expect("large-scale toroidal CDT fixture should build");
    let mut ergodics = ErgodicsSystem::with_seed(seed);
    let initial_profile = triangulation.volume_profile();
    let mut expected_attempts = 0_u64;
    let mut previous_acceptance = MoveAcceptanceCounts::default();
    let mut previous_elapsed = Duration::ZERO;
    let expected_vertices =
        usize::try_from(total_vertices).expect("vertex count should fit in usize");

    assert_eq!(triangulation.vertex_count(), expected_vertices);
    assert_toroidal_invariants(&triangulation);

    println!(
        "1+1 toroidal CDT large-scale debug: vertices={total_vertices}, timeslices={timeslices}, \
         vertices_per_slice={vertices_per_slice}, initial_simplices={}, sweeps={sweeps}, seed=0x{seed:X}",
        triangulation.face_count()
    );

    for sweep in 1..=sweeps {
        let attempts = triangulation.face_count();
        expected_attempts += u64::try_from(attempts).expect("attempt count should fit in u64");

        for _ in 0..attempts {
            let result = ergodics.attempt_random_move(&mut triangulation);
            black_box(result);
        }

        assert_toroidal_invariants(&triangulation);
        let acceptance = MoveAcceptanceCounts::from_stats(&ergodics.stats);
        let sweep_acceptance = acceptance.delta_since(previous_acceptance);
        let elapsed = started_at.elapsed();
        let sweep_elapsed = elapsed.saturating_sub(previous_elapsed);
        println!(
            "sweep {sweep}/{sweeps}: sweep_attempts={attempts}, final_vertices={}, final_simplices={}, \
             sweep_accepted={} (Move22={}, Move13Add={}, Move31Remove={}, EdgeFlip={}), \
             sweep_hard_failures={}, total_accepted={}, total_attempted={}, total_hard_failures={}, \
             sweep_elapsed={sweep_elapsed:?}, total_elapsed={elapsed:?}",
            triangulation.vertex_count(),
            triangulation.face_count(),
            sweep_acceptance.total,
            sweep_acceptance.move_22,
            sweep_acceptance.move_13,
            sweep_acceptance.move_31,
            sweep_acceptance.edge_flip,
            sweep_acceptance.hard_failures,
            acceptance.total,
            expected_attempts,
            acceptance.hard_failures,
        );
        previous_acceptance = acceptance;
        previous_elapsed = elapsed;

        if let Some(cap) = runtime_cap {
            assert!(
                elapsed <= cap,
                "large-scale CDT debug run exceeded CDT_LARGE_DEBUG_MAX_RUNTIME_SECS={}",
                cap.as_secs()
            );
        }
    }

    assert_eq!(ergodics.stats.total_attempted(), expected_attempts);
    assert!(
        ergodics.stats.total_accepted() > 0,
        "large-scale random sweeps should accept at least one move"
    );
    assert!(
        ergodics.stats.moves_13_accepted > 0,
        "large-scale random sweeps should exercise toroidal Move13Add"
    );
    assert!(
        ergodics.stats.moves_31_accepted > 0,
        "large-scale random sweeps should exercise toroidal Move31Remove"
    );
    assert_ne!(
        triangulation.volume_profile(),
        initial_profile,
        "large-scale random sweeps should change the toroidal volume profile"
    );
}
