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
//! 4. Random-move workloads with a fixed attempt budget equal to ten initial
//!    sweeps, so Criterion throughput matches the timed work exactly.
//! 5. Short Metropolis runs sized as ten initial sweeps.
//! 6. Public proposal-site iteration paths used by move attempts and one-step
//!    Metropolis proposal planning.

#[path = "support/or_abort.rs"]
mod benchmark_support;

use causal_triangulations::prelude::action::ActionConfig;
use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
use causal_triangulations::prelude::simulation::{
    CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError, CdtProposalPolicyView, MetropolisAlgorithm,
    MetropolisConfig,
};
use causal_triangulations::prelude::triangulation::CdtTriangulation2D;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hint::black_box;
use std::time::Duration;

use benchmark_support::OrAbort;

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
    move_seed: u64,
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
    BuildSuccessfulInsertionFixture,
    BuildSuccessfulRemovalFixture,
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
            Self::BuildSuccessfulInsertionFixture => {
                formatter.write_str("build a fixture with a successful forward volume move")
            }
            Self::BuildSuccessfulRemovalFixture => {
                formatter.write_str("build a fixture with a successful inverse volume move")
            }
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

const SUCCESSFUL_REMOVAL_FIXTURES: &[CdtFixture] = &[
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
    CdtFixture {
        name: "toroidal_large",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 16,
        time_slices: 12,
    },
];

/// State-dependent policy that reacts to current volume and offered support.
#[derive(Clone, Copy)]
struct VolumeResponsivePolicy;

impl CdtMoveFamilyPolicy for VolumeResponsivePolicy {
    fn family_weight(
        &self,
        view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError> {
        let volume_is_large = view.slice_sizes().iter().sum::<usize>() > 150;
        let supported = view.offered_site_count() != 0;
        let weight = match (view.family(), volume_is_large, supported) {
            (_, _, false) => 0.0,
            (MoveType::Move13Add, false, true) | (MoveType::Move31Remove, true, true) => 3.0,
            _ => 1.0,
        };
        Ok(weight)
    }
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
        result.or_abort(SetupOperation::BuildCdtFixture)
    }
}

/// Attempts one selected move type through the public move API.
fn selected_move_result(
    ergodics: &mut ErgodicsSystem,
    move_type: MoveType,
    triangulation: &mut CdtTriangulation2D,
) -> MoveResult {
    match move_type {
        MoveType::Move22 => ergodics.attempt_22_move(triangulation),
        MoveType::Move13Add => ergodics.attempt_13_move(triangulation),
        MoveType::Move31Remove => ergodics.attempt_31_move(triangulation),
        MoveType::EdgeFlip => ergodics.attempt_edge_flip(triangulation),
    }
}

/// Attempts one selected move type and keeps its result observable to Criterion.
fn attempt_selected_move(
    ergodics: &mut ErgodicsSystem,
    move_type: MoveType,
    triangulation: &mut CdtTriangulation2D,
) {
    black_box(selected_move_result(ergodics, move_type, triangulation));
}

/// Finds a deterministic seed whose first selected move succeeds on this exact state.
fn successful_move_seed(
    triangulation: &CdtTriangulation2D,
    move_type: MoveType,
    operation: SetupOperation,
) -> u64 {
    (0_u64..=4_096)
        .find(|&seed| {
            let mut probe = triangulation.clone();
            let mut ergodics = ErgodicsSystem::with_seed(seed);
            selected_move_result(&mut ergodics, move_type, &mut probe) == MoveResult::Success
        })
        .or_abort(operation)
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
        move_seed: BENCH_SEED,
    }
}

/// Creates and verifies a fixture whose next forward-volume move succeeds.
fn prepare_successful_insertion_fixture(fixture: CdtFixture) -> PreparedFixture {
    let mut prepared = prepare_fixture(fixture);
    prepared.move_seed = successful_move_seed(
        &prepared.triangulation,
        MoveType::Move13Add,
        SetupOperation::BuildSuccessfulInsertionFixture,
    );
    prepared
}

/// Creates and verifies a fixture whose next inverse-volume move succeeds.
fn prepare_successful_removal_fixture(fixture: CdtFixture) -> PreparedFixture {
    let mut triangulation = fixture.build();
    let insertion_seed = successful_move_seed(
        &triangulation,
        MoveType::Move13Add,
        SetupOperation::BuildSuccessfulRemovalFixture,
    );
    let mut insertion = ErgodicsSystem::with_seed(insertion_seed);
    let insertion_result = insertion.attempt_13_move(&mut triangulation);
    (insertion_result == MoveResult::Success)
        .then_some(())
        .or_abort(SetupOperation::BuildSuccessfulRemovalFixture);
    let move_seed = successful_move_seed(
        &triangulation,
        MoveType::Move31Remove,
        SetupOperation::BuildSuccessfulRemovalFixture,
    );

    let vertices = triangulation.vertex_count();
    let simplices = triangulation.face_count();
    PreparedFixture {
        fixture,
        triangulation,
        vertices,
        simplices,
        move_seed,
    }
}

/// Runs one Metropolis proposal step through the public simulation driver.
fn run_single_metropolis_proposal(triangulation: CdtTriangulation2D) {
    let config = MetropolisConfig::new(1.0, 1, 0, 1)
        .or_abort(SetupOperation::RunSingleMetropolisProposal)
        .with_seed(BENCH_SEED);
    let results = MetropolisAlgorithm::new(config, ActionConfig::default())
        .run(triangulation)
        .or_abort(SetupOperation::RunSingleMetropolisProposal);
    black_box(results.proposal_stats());
}

/// Runs one proposal with family weights recomputed from the live state.
fn run_state_dependent_metropolis_proposal(triangulation: CdtTriangulation2D) {
    let config = MetropolisConfig::new(1.0, 1, 0, 1)
        .or_abort(SetupOperation::RunSingleMetropolisProposal)
        .with_seed(BENCH_SEED);
    let results = MetropolisAlgorithm::new(config, ActionConfig::default())
        .with_policy(VolumeResponsivePolicy)
        .run(triangulation)
        .or_abort(SetupOperation::RunSingleMetropolisProposal);
    black_box(results.proposal_stats());
}

/// Converts Criterion throughput element counts without silently truncating.
fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).or_abort(SetupOperation::ConvertBenchmarkSize)
}

/// Computes the Metropolis step budget for the ten-sweep CI workload.
fn ten_sweep_step_count(simplices: usize) -> u32 {
    u32::try_from(sweep_attempt_count(simplices)).or_abort(SetupOperation::ConvertSweepStepCount)
}

/// Encodes the CDT convention that one sweep attempts one move per simplex.
fn sweep_attempt_count(simplices: usize) -> usize {
    let sweeps = usize::try_from(SWEEP_COUNT).or_abort(SetupOperation::ConvertSweepCount);
    simplices
        .checked_mul(sweeps)
        .or_abort(SetupOperation::ComputeSweepStepCount)
}

/// Runs an exact random-move attempt budget and validates the evolved triangulation.
fn run_random_move_attempt_budget(
    mut triangulation: CdtTriangulation2D,
    seed: u64,
    attempts: usize,
) -> MoveStatistics {
    let mut ergodics = ErgodicsSystem::with_seed(seed);

    for _ in 0..attempts {
        let result = ergodics.attempt_random_move(&mut triangulation);
        black_box(result);
    }

    triangulation
        .validate()
        .or_abort(SetupOperation::ValidateRandomSweepWorkload);
    black_box(triangulation.face_count());
    ergodics.stats().clone()
}

/// Runs a short Metropolis simulation sized to match the ten-sweep workload.
fn run_metropolis_ten_sweeps(triangulation: CdtTriangulation2D, simplices: usize) -> usize {
    let steps = ten_sweep_step_count(simplices);
    let config = MetropolisConfig::new(1.0, steps, 0, steps)
        .or_abort(SetupOperation::RunTenSweepMetropolis)
        .with_seed(BENCH_SEED);
    let results = MetropolisAlgorithm::new(config, ActionConfig::default())
        .run(triangulation)
        .or_abort(SetupOperation::RunTenSweepMetropolis);
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

/// Benchmarks individual ergodic move attempts on verified fresh fixtures.
fn bench_cdt_move_attempts(c: &mut Criterion) {
    let flip_fixture = prepare_fixture(CdtFixture {
        name: "open_strip_medium",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 20,
        time_slices: 10,
    });
    let insertion_fixture = prepare_successful_insertion_fixture(CdtFixture {
        name: "toroidal_medium",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 12,
        time_slices: 10,
    });
    let removal_fixture = prepare_successful_removal_fixture(CdtFixture {
        name: "toroidal_medium_after_insertion",
        topology: TopologyFixture::Toroidal,
        vertices_per_slice: 12,
        time_slices: 10,
    });
    let mut group = c.benchmark_group("cdt_move_attempts_2d");

    for move_type in [
        MoveType::Move22,
        MoveType::Move13Add,
        MoveType::Move31Remove,
        MoveType::EdgeFlip,
    ] {
        let prepared = match move_type {
            MoveType::Move13Add => &insertion_fixture,
            MoveType::Move31Remove => &removal_fixture,
            MoveType::Move22 | MoveType::EdgeFlip => &flip_fixture,
        };
        group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));
        group.bench_with_input(
            BenchmarkId::new(format!("{move_type:?}"), prepared.simplices),
            &move_type,
            |b, &move_type| {
                b.iter_batched(
                    || {
                        (
                            ErgodicsSystem::with_seed(prepared.move_seed),
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

/// Benchmarks policy evaluation that materializes state-dependent family views.
fn bench_cdt_state_dependent_policy(c: &mut Criterion) {
    let prepared = prepare_fixture(CdtFixture {
        name: "open_strip_medium",
        topology: TopologyFixture::OpenStrip,
        vertices_per_slice: 20,
        time_slices: 10,
    });
    let mut group = c.benchmark_group("cdt_state_dependent_policy_2d");
    group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));
    group.bench_function("single_metropolis_proposal", |b| {
        b.iter_batched(
            || prepared.triangulation.clone(),
            run_state_dependent_metropolis_proposal,
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

/// Benchmarks a guaranteed-success inverse move across increasing mesh sizes.
fn bench_cdt_successful_local_finalization(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_successful_local_finalization_2d");
    for &fixture in SUCCESSFUL_REMOVAL_FIXTURES {
        let prepared = prepare_successful_removal_fixture(fixture);
        group.throughput(Throughput::Elements(usize_to_u64(prepared.simplices)));
        group.bench_with_input(
            BenchmarkId::new(prepared.fixture.name, prepared.simplices),
            &prepared,
            |b, prepared| {
                b.iter_batched(
                    || {
                        (
                            ErgodicsSystem::with_seed(prepared.move_seed),
                            prepared.triangulation.clone(),
                        )
                    },
                    |(mut ergodics, mut triangulation)| {
                        let result = ergodics.attempt_31_move(&mut triangulation);
                        black_box(result);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

/// Benchmarks fixed random-move attempt budgets over tiny CI-sized triangulations.
fn bench_cdt_random_move_attempt_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("cdt_random_move_attempt_budget_2d");

    for &fixture in SWEEP_FIXTURES {
        let prepared = prepare_fixture(fixture);
        let attempt_budget = sweep_attempt_count(prepared.simplices);
        group.throughput(Throughput::Elements(usize_to_u64(attempt_budget)));
        group.bench_with_input(
            BenchmarkId::new(prepared.fixture.name, prepared.simplices),
            &prepared,
            |b, prepared| {
                b.iter_batched(
                    || prepared.triangulation.clone(),
                    |triangulation| {
                        let stats = run_random_move_attempt_budget(
                            triangulation,
                            BENCH_SEED,
                            attempt_budget,
                        );
                        assert_eq!(
                            stats.total_attempted(),
                            u64::try_from(attempt_budget)
                                .or_abort(SetupOperation::ConvertBenchmarkSize)
                        );
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
        bench_cdt_state_dependent_policy,
        bench_cdt_successful_local_finalization,
        bench_cdt_random_move_attempt_budget,
        bench_cdt_metropolis_ten_sweeps
);
criterion_main!(benches);
