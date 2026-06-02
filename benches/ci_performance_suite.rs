#![forbid(unsafe_code)]

//! CI performance suite for CDT regression checks.
//!
//! This harness is the small, durable performance contract for CDT workflows. It
//! complements the broader `cdt_benchmarks` target by focusing on operations that
//! should stay fast across releases:
//!
//! 1. Open-boundary and toroidal CDT triangulation construction.
//! 2. Evolved-state validation on generated triangulations.
//! 3. Individual ergodic move attempts on fresh fixtures.
//! 4. Ten-sweep random-move workloads, where each sweep attempts one move per
//!    current simplex.
//! 5. Short Metropolis runs sized as ten initial sweeps.
//! 6. Public proposal-site iteration paths used by move attempts and one-step
//!    Metropolis proposal planning.

use causal_triangulations::prelude::action::ActionConfig;
use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveStatistics, MoveType};
use causal_triangulations::prelude::simulation::{MetropolisAlgorithm, MetropolisConfig};
use causal_triangulations::prelude::triangulation::CdtTriangulation2D;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hint::black_box;
use std::time::Duration;

const SWEEP_COUNT: u32 = 10;
const BENCH_SEED: u64 = 0xCDA7_2026;

#[derive(Clone, Copy)]
enum TopologyFixture {
    OpenStrip,
    Toroidal,
}

#[derive(Clone, Copy)]
struct CdtFixture {
    name: &'static str,
    topology: TopologyFixture,
    vertices_per_slice: u32,
    time_slices: u32,
}

struct PreparedFixture {
    fixture: CdtFixture,
    triangulation: CdtTriangulation2D,
    vertices: usize,
    simplices: usize,
}

#[derive(Clone, Copy)]
enum SetupOperation {
    BuildCdtFixture,
    RunSingleMetropolisProposal,
    ConvertBenchmarkSize,
    ConvertSweepStepCount,
    ConvertSweepCount,
    ComputeSweepStepCount,
    ValidateRandomSweepWorkload,
    RunTenSweepMetropolis,
}

impl Display for SetupOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::BuildCdtFixture => formatter.write_str("build CDT benchmark fixture"),
            Self::RunSingleMetropolisProposal => {
                formatter.write_str("run single-step Metropolis proposal workload")
            }
            Self::ConvertBenchmarkSize => formatter.write_str("convert benchmark size"),
            Self::ConvertSweepStepCount => {
                formatter.write_str("convert benchmark sweep step count")
            }
            Self::ConvertSweepCount => formatter.write_str("convert sweep count"),
            Self::ComputeSweepStepCount => {
                formatter.write_str("compute benchmark sweep step count")
            }
            Self::ValidateRandomSweepWorkload => {
                formatter.write_str("validate random sweep workload")
            }
            Self::RunTenSweepMetropolis => formatter.write_str("run ten-sweep Metropolis workload"),
        }
    }
}

const GENERATION_FIXTURES: &[CdtFixture] = &[
    CdtFixture {
        name: "open_strip_small",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 12,
        time_slices: 8,
    },
    CdtFixture {
        name: "open_strip_medium",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 20,
        time_slices: 10,
    },
    CdtFixture {
        name: "open_strip_large",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 28,
        time_slices: 12,
    },
    CdtFixture {
        name: "toroidal_small",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 8,
        time_slices: 8,
    },
    CdtFixture {
        name: "toroidal_medium",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 12,
        time_slices: 10,
    },
];

const SWEEP_FIXTURES: &[CdtFixture] = &[
    CdtFixture {
        name: "open_strip_tiny",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 4,
        time_slices: 3,
    },
    CdtFixture {
        name: "open_strip_small",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 6,
        time_slices: 4,
    },
    CdtFixture {
        name: "toroidal_tiny",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 4,
        time_slices: 3,
    },
];

const PROPOSAL_FIXTURES: &[CdtFixture] = &[
    CdtFixture {
        name: "open_strip_medium",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 20,
        time_slices: 10,
    },
    CdtFixture {
        name: "toroidal_medium",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 12,
        time_slices: 10,
    },
    CdtFixture {
        name: "toroidal_probe",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 32,
        time_slices: 16,
    },
];

/// Fails fast when benchmark fixture setup cannot satisfy its invariant.
fn require_result<T>(result: Result<T, impl Display>, operation: SetupOperation) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{operation}: {error}"),
    }
}

/// Fails fast when benchmark fixture setup cannot produce a required value.
fn require_option<T>(value: Option<T>, operation: SetupOperation) -> T {
    let Some(value) = value else {
        panic!("{operation}");
    };
    value
}

impl CdtFixture {
    /// Builds the requested CDT topology for a benchmark fixture.
    fn build(self) -> CdtTriangulation2D {
        let result = match self.topology {
            TopologyFixture::OpenStrip => {
                CdtTriangulation2D::from_cdt_strip(self.vertices_per_slice, self.time_slices)
            }
            TopologyFixture::Toroidal => {
                CdtTriangulation2D::from_toroidal_cdt(self.vertices_per_slice, self.time_slices)
            }
        };
        require_result(result, SetupOperation::BuildCdtFixture)
    }
}

/// Attempts one selected move type through the public move API.
fn attempt_selected_move(
    ergodics: &mut ErgodicsSystem,
    move_type: MoveType,
    triangulation: &mut CdtTriangulation2D,
) {
    let result = match move_type {
        MoveType::Move22 => ergodics.attempt_22_move(triangulation),
        MoveType::Move13Add => ergodics.attempt_13_move(triangulation),
        MoveType::Move31Remove => ergodics.attempt_31_move(triangulation),
        MoveType::EdgeFlip => ergodics.attempt_edge_flip(triangulation),
    };
    black_box(result);
}

/// Materializes a fixture once so benchmarks can use stable size metadata.
fn prepare_fixture(fixture: CdtFixture) -> PreparedFixture {
    let triangulation = fixture.build();
    let vertices = triangulation.vertex_count();
    let simplices = triangulation.face_count();
    PreparedFixture {
        fixture,
        triangulation,
        vertices,
        simplices,
    }
}

/// Runs one Metropolis proposal step through the public simulation driver.
fn run_single_metropolis_proposal(triangulation: CdtTriangulation2D) {
    let config = require_result(
        MetropolisConfig::new(1.0, 1, 0, 1),
        SetupOperation::RunSingleMetropolisProposal,
    )
    .with_seed(BENCH_SEED);
    let results = require_result(
        MetropolisAlgorithm::new(config, ActionConfig::default()).run(triangulation),
        SetupOperation::RunSingleMetropolisProposal,
    );
    black_box(results.proposal_stats());
}

/// Converts Criterion throughput element counts without silently truncating.
fn usize_to_u64(value: usize) -> u64 {
    require_result(u64::try_from(value), SetupOperation::ConvertBenchmarkSize)
}

/// Computes the Metropolis step budget for the ten-sweep CI workload.
fn ten_sweep_step_count(simplices: usize) -> u32 {
    require_result(
        u32::try_from(sweep_attempt_count(simplices)),
        SetupOperation::ConvertSweepStepCount,
    )
}

/// Encodes the CDT convention that one sweep attempts one move per simplex.
fn sweep_attempt_count(simplices: usize) -> usize {
    let sweeps = require_result(
        usize::try_from(SWEEP_COUNT),
        SetupOperation::ConvertSweepCount,
    );
    require_option(
        simplices.checked_mul(sweeps),
        SetupOperation::ComputeSweepStepCount,
    )
}

/// Runs ten random-move sweeps and validates the evolved triangulation.
fn run_random_move_sweeps(mut triangulation: CdtTriangulation2D, seed: u64) -> MoveStatistics {
    let mut ergodics = ErgodicsSystem::with_seed(seed);

    for _ in 0..SWEEP_COUNT {
        let attempts = triangulation.face_count();
        for _ in 0..attempts {
            let result = ergodics.attempt_random_move(&mut triangulation);
            black_box(result);
        }
    }

    require_result(
        triangulation.validate(),
        SetupOperation::ValidateRandomSweepWorkload,
    );
    black_box(triangulation.face_count());
    ergodics.stats().clone()
}

/// Runs a short Metropolis simulation sized to match the ten-sweep workload.
fn run_metropolis_ten_sweeps(triangulation: CdtTriangulation2D, simplices: usize) -> usize {
    let steps = ten_sweep_step_count(simplices);
    let config = require_result(
        MetropolisConfig::new(1.0, steps, 0, steps),
        SetupOperation::RunTenSweepMetropolis,
    )
    .with_seed(BENCH_SEED);
    let results = require_result(
        MetropolisAlgorithm::new(config, ActionConfig::default()).run(triangulation),
        SetupOperation::RunTenSweepMetropolis,
    );
    black_box(results.acceptance_rate());
    results.steps().len()
}

/// Benchmarks deterministic construction for representative CDT topologies.
fn bench_cdt_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_generation_2d");

    for &fixture in GENERATION_FIXTURES {
        let prepared = prepare_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(prepared.vertices)));
        group.bench_with_input(
            BenchmarkId::new(fixture.name, prepared.vertices),
            &fixture,
            |b, &fixture| {
                b.iter(|| {
                    let triangulation = fixture.build();
                    black_box(triangulation)
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks full CDT validation on already-generated triangulations.
fn bench_cdt_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_validation_2d");

    for &fixture in GENERATION_FIXTURES {
        let prepared = prepare_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));
        group.bench_with_input(
            BenchmarkId::new(fixture.name, prepared.simplices),
            &prepared.triangulation,
            |b, triangulation| {
                b.iter(|| {
                    let result = triangulation.validate();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks individual ergodic move attempts on a common fresh fixture.
fn bench_cdt_move_attempts(c: &mut Criterion) {
    let prepared = prepare_fixture(CdtFixture {
        name: "open_strip_medium",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 20,
        time_slices: 10,
    });
    let mut group = c.benchmark_group("cdt_move_attempts_2d");
    group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));

    for move_type in [
        MoveType::Move22,
        MoveType::Move13Add,
        MoveType::Move31Remove,
        MoveType::EdgeFlip,
    ] {
        group.bench_with_input(
            BenchmarkId::new(format!("{move_type:?}"), prepared.simplices),
            &move_type,
            |b, &move_type| {
                b.iter_batched(
                    || {
                        (
                            ErgodicsSystem::with_seed(BENCH_SEED),
                            prepared.triangulation.clone(),
                        )
                    },
                    |(mut ergodics, mut triangulation)| {
                        attempt_selected_move(&mut ergodics, move_type, &mut triangulation);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmarks proposal-site iteration through public move-attempt APIs.
fn bench_cdt_proposal_site_move_attempts(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_proposal_site_move_attempts_2d");

    for &fixture in PROPOSAL_FIXTURES {
        let prepared = prepare_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));
        for move_type in [
            MoveType::Move22,
            MoveType::Move13Add,
            MoveType::Move31Remove,
            MoveType::EdgeFlip,
        ] {
            group.bench_with_input(
                BenchmarkId::new(
                    format!("{}_{move_type:?}", prepared.fixture.name),
                    prepared.simplices,
                ),
                &move_type,
                |b, &move_type| {
                    b.iter_batched(
                        || {
                            (
                                ErgodicsSystem::with_seed(BENCH_SEED),
                                prepared.triangulation.clone(),
                            )
                        },
                        |(mut ergodics, mut triangulation)| {
                            attempt_selected_move(&mut ergodics, move_type, &mut triangulation);
                        },
                        BatchSize::LargeInput,
                    );
                },
            );
        }
    }

    group.finish();
}

/// Benchmarks one public Metropolis proposal step, including cloned planning and reverse counts.
fn bench_cdt_single_metropolis_proposal(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_single_metropolis_proposal_2d");

    for &fixture in PROPOSAL_FIXTURES {
        let prepared = prepare_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));
        group.bench_with_input(
            BenchmarkId::new(prepared.fixture.name, prepared.simplices),
            &prepared,
            |b, prepared| {
                b.iter_batched(
                    || prepared.triangulation.clone(),
                    run_single_metropolis_proposal,
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmarks short random-move evolution over tiny CI-sized triangulations.
fn bench_cdt_random_move_sweeps(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_random_move_sweeps_2d");

    for &fixture in SWEEP_FIXTURES {
        let prepared = prepare_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(sweep_attempt_count(
            prepared.simplices,
        ))));
        group.bench_with_input(
            BenchmarkId::new(prepared.fixture.name, prepared.simplices),
            &prepared,
            |b, prepared| {
                b.iter_batched(
                    || prepared.triangulation.clone(),
                    |triangulation| {
                        let stats = run_random_move_sweeps(triangulation, BENCH_SEED);
                        black_box(stats.total_attempted())
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmarks the simulation driver with the same ten-sweep sizing contract.
fn bench_cdt_metropolis_ten_sweeps(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_metropolis_2d");

    for &fixture in &SWEEP_FIXTURES[..2] {
        let prepared = prepare_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(sweep_attempt_count(
            prepared.simplices,
        ))));
        group.bench_with_input(
            BenchmarkId::new(prepared.fixture.name, prepared.simplices),
            &prepared,
            |b, prepared| {
                b.iter_batched(
                    || prepared.triangulation.clone(),
                    |triangulation| {
                        let steps = run_metropolis_ten_sweeps(triangulation, prepared.simplices);
                        black_box(steps)
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(2))
        .warm_up_time(Duration::from_secs(1));
    targets =
        bench_cdt_generation,
        bench_cdt_validation,
        bench_cdt_move_attempts,
        bench_cdt_proposal_site_move_attempts,
        bench_cdt_single_metropolis_proposal,
        bench_cdt_random_move_sweeps,
        bench_cdt_metropolis_ten_sweeps
);
criterion_main!(benches);
