//! Comprehensive benchmarks for Causal Dynamical Triangulations operations.
//!
//! This benchmark suite measures the performance of key CDT operations including:
//! - Triangulation creation and initialization
//! - Geometry operations (edge counting, queries)
//! - Metropolis-Hastings simulation steps
//! - Action calculations
//! - Ergodic move operations

#![allow(missing_docs)] // Allow missing docs for criterion-generated functions

use causal_triangulations::{
    cdt::{
        action::ActionConfig,
        ergodic_moves::{ErgodicsSystem, MoveType},
        metropolis::{MetropolisAlgorithm, MetropolisConfig},
    },
    geometry::{CdtTriangulation2D, traits::TriangulationQuery},
};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

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
                    let triangulation = CdtTriangulation2D::from_random_points(
                        black_box(vertex_count),
                        black_box(1),
                        black_box(2),
                    )
                    .expect("Failed to create triangulation");
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
        .iter()
        .filter_map(|&size| {
            CdtTriangulation2D::from_random_points(size, 1, 2)
                .ok()
                .map(|tri| (size as usize, tri))
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
    let triangulation = CdtTriangulation2D::from_random_points(50, 1, 2)
        .expect("Failed to create test triangulation");

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
            let vertices: Vec<_> = geometry.vertices().collect();
            black_box(vertices)
        });
    });

    // Benchmark edge iteration
    group.bench_function("iterate_edges", |b| {
        b.iter(|| {
            let edges: Vec<_> = geometry.edges().collect();
            black_box(edges)
        });
    });

    // Benchmark face iteration
    group.bench_function("iterate_faces", |b| {
        b.iter(|| {
            let faces: Vec<_> = geometry.faces().collect();
            black_box(faces)
        });
    });

    group.finish();
}

/// Benchmark action calculations
fn bench_action_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("action_calculations");

    let config = ActionConfig::default();

    // Test different triangulation sizes
    let test_cases = [
        (10, 15, 6),     // Small triangulation
        (50, 140, 92),   // Medium triangulation
        (100, 290, 192), // Large triangulation
    ];

    for (vertices, edges, faces) in test_cases {
        group.throughput(Throughput::Elements(u64::from(vertices)));
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

    let seed_triangulation = vec![vec![0, 1, 2], vec![1, 2, 3]]; // Simple test data

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
                    || (ErgodicsSystem::new(), seed_triangulation.clone()),
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
            ErgodicsSystem::new,
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
            || (ErgodicsSystem::new(), seed_triangulation.clone()),
            |(mut ergodics, mut triangulation)| {
                let result = ergodics.attempt_random_move(&mut triangulation);
                black_box(result)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark Metropolis-Hastings simulation steps
fn bench_metropolis_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("metropolis_simulation");

    // Test different step counts
    for steps in [10, 50, 100] {
        group.throughput(Throughput::Elements(u64::from(steps)));
        group.bench_with_input(
            BenchmarkId::new("simulation_steps", steps),
            &steps,
            |b, &steps| {
                b.iter(|| {
                    let triangulation = CdtTriangulation2D::from_random_points(20, 1, 2)
                        .expect("Failed to create triangulation");

                    let config = MetropolisConfig::new(1.0, steps, 5, 5);
                    let action_config = ActionConfig::default();
                    let algorithm = MetropolisAlgorithm::new(config, action_config);

                    let results = algorithm
                        .run(black_box(triangulation))
                        .expect("Simulation should succeed");
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
        CdtTriangulation2D::from_random_points(15, 1, 2).expect("Failed to create triangulation");

    let config = MetropolisConfig::new(1.0, 100, 10, 5);
    let action_config = ActionConfig::default();
    let algorithm = MetropolisAlgorithm::new(config, action_config);

    let results = algorithm
        .run(triangulation)
        .expect("Simulation should succeed");

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
            let mut triangulation = CdtTriangulation2D::from_random_points(50, 1, 2)
                .expect("Failed to create triangulation");
            triangulation.refresh_cache();
            black_box(triangulation)
        });
    });

    group.bench_function("cache_invalidation", |b| {
        b.iter(|| {
            let mut triangulation = CdtTriangulation2D::from_random_points(50, 1, 2)
                .expect("Failed to create triangulation");
            // Invalidate cache by getting mutable reference
            let _geometry_mut = triangulation.geometry_mut();
            black_box(triangulation)
        });
    });

    group.finish();
}

/// Benchmark triangulation validation
fn bench_validation(c: &mut Criterion) {
    let triangulation =
        CdtTriangulation2D::from_random_points(30, 1, 2).expect("Failed to create triangulation");

    let mut group = c.benchmark_group("validation");

    group.bench_function("validate", |b| {
        b.iter(|| {
            let result = triangulation.validate();
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
