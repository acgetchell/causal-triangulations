#![forbid(unsafe_code)]

//! Comprehensive benchmarks for Causal Dynamical Triangulations operations.
//!
//! This benchmark suite measures the performance of key CDT operations including:
//! - Triangulation creation and initialization
//! - Geometry operations (edge counting, queries)
//! - Metropolis-Hastings simulation steps
//! - Action calculations
//! - Ergodic move operations

#[path = "support/or_abort.rs"]
mod benchmark_support;

use causal_triangulations::prelude::action::ActionConfig;
use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveType};
use causal_triangulations::prelude::simulation::{MetropolisAlgorithm, MetropolisConfig};
use causal_triangulations::prelude::triangulation::{CdtTriangulation2D, TriangulationQuery};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hint::black_box;

use benchmark_support::OrAbort;

const BENCH_SEED: u64 = 0xCD7_BEC4;

fn benchmark_seed(vertex_count: u32) -> u64 {
    BENCH_SEED.wrapping_add(u64::from(vertex_count))
}

#[derive(Clone, Copy)]
enum SetupOperation {
    CreateTriangulation,
    CreateTestTriangulation,
    BuildCdtBenchmarkStrip,
    CreateCdtStrip,
    RunSimulation,
    BuildSimulationResults,
    UpdateMetadata,
    ReadTriangulationAdjacency,
}

impl Display for SetupOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::CreateTriangulation => formatter.write_str("create triangulation"),
            Self::CreateTestTriangulation => formatter.write_str("create test triangulation"),
            Self::BuildCdtBenchmarkStrip => formatter.write_str("build CDT benchmark strip"),
            Self::CreateCdtStrip => formatter.write_str("create CDT strip"),
            Self::RunSimulation => formatter.write_str("run simulation"),
            Self::BuildSimulationResults => formatter.write_str("build simulation results"),
            Self::UpdateMetadata => formatter.write_str("update metadata"),
            Self::ReadTriangulationAdjacency => {
                formatter.write_str("read benchmark triangulation adjacency")
            }
        }
    }
}

/// Benchmark triangulation creation with different vertex counts
fn bench_triangulation_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("triangulation_creation");

    for vertex_count in [5, 10, 20, 50, 100] {
        group.throughput(Throughput::Elements(u64::from(vertex_count)));
        group.bench_with_input(
            BenchmarkId::new("delaunay_backend", vertex_count),
            &vertex_count,
            |b, &vertex_count| {
                b.iter(|| {
                    let triangulation = CdtTriangulation2D::from_seeded_points(
                        black_box(vertex_count),
                        black_box(1),
                        black_box(2),
                        black_box(benchmark_seed(vertex_count)),
                    )
                    .or_abort(SetupOperation::CreateTriangulation);
                    black_box(triangulation)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark edge counting performance
fn bench_edge_counting(c: &mut Criterion) {
    let mut group = c.benchmark_group("edge_counting");

    // Pre-create triangulations of different sizes
    let triangulations: Vec<(usize, CdtTriangulation2D)> = [10, 25, 50, 100, 200]
        .into_iter()
        .map(|size| {
            let triangulation =
                CdtTriangulation2D::from_seeded_points(size, 1, 2, benchmark_seed(size))
                    .or_abort(SetupOperation::CreateTriangulation);
            (size as usize, triangulation)
        })
        .collect();

    for (size, triangulation) in triangulations {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::new("uncached", size),
            &triangulation,
            |b, tri: &CdtTriangulation2D| {
                b.iter(|| {
                    let count = tri.geometry().edge_count();
                    black_box(count)
                });
            },
        );

        // Benchmark cached edge counting
        let mut cached_tri = triangulation;
        cached_tri.refresh_cache();

        group.bench_with_input(
            BenchmarkId::new("cached", size),
            &cached_tri,
            |b, tri: &CdtTriangulation2D| {
                b.iter(|| {
                    let count = tri.edge_count();
                    black_box(count)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark geometry query operations
fn bench_geometry_queries(c: &mut Criterion) {
    let triangulation = CdtTriangulation2D::from_seeded_points(50, 1, 2, benchmark_seed(50))
        .or_abort(SetupOperation::CreateTestTriangulation);

    let geometry = triangulation.geometry();

    let mut group = c.benchmark_group("geometry_queries");

    group.bench_function("vertex_count", |b| {
        b.iter(|| {
            let count = geometry.vertex_count();
            black_box(count)
        });
    });

    group.bench_function("face_count", |b| {
        b.iter(|| {
            let count = geometry.face_count();
            black_box(count)
        });
    });

    group.bench_function("euler_characteristic", |b| {
        b.iter(|| {
            let euler = geometry.euler_characteristic();
            black_box(euler)
        });
    });

    group.bench_function("is_valid", |b| {
        b.iter(|| {
            let valid = geometry.is_valid();
            black_box(valid)
        });
    });

    // Benchmark vertex iteration
    group.bench_function("iterate_vertices", |b| {
        b.iter(|| {
            let vertex_count = geometry.vertices().map(black_box).count();
            black_box(vertex_count)
        });
    });

    // Benchmark edge iteration
    group.bench_function("iterate_edges", |b| {
        b.iter(|| {
            let edge_count = geometry.edges().map(black_box).count();
            black_box(edge_count)
        });
    });

    // Benchmark face iteration
    group.bench_function("iterate_faces", |b| {
        b.iter(|| {
            let face_count = geometry.faces().map(black_box).count();
            black_box(face_count)
        });
    });

    group.finish();
}

/// Benchmark action calculations
fn bench_action_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("action_calculations");

    let config = ActionConfig::default();

    // Test different triangulation sizes
    let test_cases: [(usize, usize, usize, u64); 3] = [
        (10, 15, 6, 10),      // Small triangulation
        (50, 140, 92, 50),    // Medium triangulation
        (100, 290, 192, 100), // Large triangulation
    ];

    for (vertices, edges, faces, throughput_vertices) in test_cases {
        group.throughput(Throughput::Elements(throughput_vertices));
        group.bench_with_input(
            BenchmarkId::new("calculate_action", vertices),
            &(vertices, edges, faces),
            |b, &(v, e, f)| {
                b.iter(|| {
                    let action = config.calculate_action(black_box(v), black_box(e), black_box(f));
                    black_box(action)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark ergodic move operations
fn bench_ergodic_moves(c: &mut Criterion) {
    let mut group = c.benchmark_group("ergodic_moves");

    let seed_triangulation = || {
        CdtTriangulation2D::from_cdt_strip(4, 3).or_abort(SetupOperation::BuildCdtBenchmarkStrip)
    };

    // Benchmark different move types
    let move_types = [
        MoveType::Move22,
        MoveType::Move13Add,
        MoveType::Move31Remove,
        MoveType::EdgeFlip,
    ];

    for move_type in move_types {
        group.bench_with_input(
            BenchmarkId::new("move", format!("{move_type:?}")),
            &move_type,
            |b, &move_type| {
                b.iter_batched(
                    || (ErgodicsSystem::with_seed(BENCH_SEED), seed_triangulation()),
                    |(mut ergodics, mut triangulation)| {
                        let result = match move_type {
                            MoveType::Move22 => ergodics.attempt_22_move(&mut triangulation),
                            MoveType::Move13Add => ergodics.attempt_13_move(&mut triangulation),
                            MoveType::Move31Remove => ergodics.attempt_31_move(&mut triangulation),
                            MoveType::EdgeFlip => ergodics.attempt_edge_flip(&mut triangulation),
                        };
                        black_box(result)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    // Benchmark random move selection (stateless, no reset needed)
    group.bench_function("random_move_selection", |b| {
        b.iter_batched(
            || ErgodicsSystem::with_seed(BENCH_SEED),
            |mut ergodics| {
                let move_type = ergodics.select_random_move();
                black_box(move_type)
            },
            BatchSize::SmallInput,
        );
    });

    // Benchmark random move attempt (needs fresh triangulation each time)
    group.bench_function("random_move_attempt", |b| {
        b.iter_batched(
            || (ErgodicsSystem::with_seed(BENCH_SEED), seed_triangulation()),
            |(mut ergodics, mut triangulation)| {
                let result = ergodics.attempt_random_move(&mut triangulation);
                black_box(result)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark short Metropolis-Hastings simulations.
fn bench_metropolis_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("metropolis_simulation");

    for steps in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("metropolis_steps", steps),
            &steps,
            |b, &steps| {
                b.iter(|| {
                    let triangulation = CdtTriangulation2D::from_cdt_strip(4, 5)
                        .or_abort(SetupOperation::CreateCdtStrip);

                    let config = MetropolisConfig::new(1.0, steps, 5, 5)
                        .or_abort(SetupOperation::RunSimulation)
                        .with_seed(42);
                    let action_config = ActionConfig::default();
                    let algorithm = MetropolisAlgorithm::new(config, action_config);

                    let results = algorithm
                        .run(black_box(triangulation))
                        .or_abort(SetupOperation::RunSimulation);
                    black_box(results)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark simulation analysis operations
fn bench_simulation_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("simulation_analysis");

    // Create a sample simulation result
    let triangulation =
        CdtTriangulation2D::from_cdt_strip(5, 3).or_abort(SetupOperation::CreateCdtStrip);

    let config = MetropolisConfig::new(1.0, 100, 10, 5)
        .or_abort(SetupOperation::BuildSimulationResults)
        .with_seed(42);
    let results = MetropolisAlgorithm::new(config, ActionConfig::default())
        .run(triangulation)
        .or_abort(SetupOperation::BuildSimulationResults);

    group.bench_function("acceptance_rate", |b| {
        b.iter(|| {
            let rate = results.acceptance_rate();
            black_box(rate)
        });
    });

    group.bench_function("average_action", |b| {
        b.iter(|| {
            let avg = results.average_action();
            black_box(avg)
        });
    });

    group.bench_function("average_volume_profile", |b| {
        b.iter(|| {
            let profile = results.average_volume_profile();
            black_box(profile)
        });
    });

    group.bench_function("volume_fluctuations", |b| {
        b.iter(|| {
            let fluctuations = results.volume_fluctuations();
            black_box(fluctuations)
        });
    });

    group.bench_function("hausdorff_dimension_estimate", |b| {
        b.iter(|| {
            let estimate = results
                .hausdorff_dimension_estimate()
                .or_abort(SetupOperation::ReadTriangulationAdjacency);
            black_box(estimate)
        });
    });

    group.bench_function("spectral_dimension_estimate", |b| {
        b.iter(|| {
            let estimate = results
                .spectral_dimension_estimate()
                .or_abort(SetupOperation::ReadTriangulationAdjacency);
            black_box(estimate)
        });
    });

    group.bench_function("equilibrium_measurements", |b| {
        b.iter(|| {
            let measurements = results.equilibrium_measurements();
            black_box(measurements)
        });
    });

    group.finish();
}

/// Benchmark cache operations
fn bench_cache_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_operations");

    group.bench_function("refresh_cache", |b| {
        b.iter(|| {
            let mut triangulation =
                CdtTriangulation2D::from_cdt_strip(10, 5).or_abort(SetupOperation::CreateCdtStrip);
            triangulation.refresh_cache();
            black_box(triangulation)
        });
    });

    group.bench_function("metadata_cache_invalidation", |b| {
        b.iter_batched(
            || {
                let mut triangulation = CdtTriangulation2D::from_cdt_strip(10, 5)
                    .or_abort(SetupOperation::CreateCdtStrip);
                triangulation.refresh_cache();
                triangulation
            },
            |mut triangulation| {
                triangulation
                    .set_time_slices(2)
                    .or_abort(SetupOperation::UpdateMetadata);
                black_box(triangulation)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark embedding validation for an unfoliated geometry fixture.
fn bench_validation(c: &mut Criterion) {
    let triangulation = CdtTriangulation2D::from_seeded_points(30, 1, 2, benchmark_seed(30))
        .or_abort(SetupOperation::CreateTriangulation);

    let mut group = c.benchmark_group("validation");

    group.bench_function("validate_embedding", |b| {
        b.iter(|| {
            let result = triangulation.geometry().validate_embedding();
            black_box(result)
        });
    });

    group.finish();
}

// Registers all benchmarks
// Group all benchmarks
criterion_group!(
    benches,
    bench_triangulation_creation,
    bench_edge_counting,
    bench_geometry_queries,
    bench_action_calculations,
    bench_ergodic_moves,
    bench_metropolis_simulation,
    bench_simulation_analysis,
    bench_cache_operations,
    bench_validation
);
criterion_main!(benches);
