# CDT Benchmarking Guide

This document describes the comprehensive benchmarking suite for the Causal Dynamical Triangulations library.

## Overview

The benchmarking suite measures performance of key CDT operations using the [criterion](https://crates.io/crates/criterion) benchmarking framework. The
benchmarks are designed to track performance across different system sizes and identify performance regressions.

## Running Benchmarks

### All Benchmarks

```bash
cargo bench
```

### CI Performance Suite

```bash
just bench-ci
```

The `ci_performance_suite` target is the smaller regression contract used by
the performance tooling. It runs with the Cargo `perf` profile and focuses on
CDT workflows that should stay comparable across releases:

- generating open-boundary and toroidal CDT triangulations;
- validating generated triangulations;
- attempting individual ergodic move types;
- executing ten random-move sweeps, where each sweep attempts one move per
  current simplex;
- running short Metropolis simulations sized as ten initial sweeps.

### Specific Benchmark Groups

```bash
# Triangulation creation performance
cargo bench triangulation_creation

# Edge counting performance (cached vs uncached)
cargo bench edge_counting

# Geometry query operations
cargo bench geometry_queries

# Action calculations
cargo bench action_calculations

# Ergodic move operations
cargo bench ergodic_moves

# Metropolis-Hastings simulation loop
cargo bench metropolis_simulation

# Simulation analysis operations
cargo bench simulation_analysis

# Cache operations
cargo bench cache_operations

# Validation operations
cargo bench validation

# CI regression contract
cargo bench --profile perf --bench ci_performance_suite
```

### Output Format

```bash
# Generate HTML reports
cargo bench -- --output-format html

# Save benchmark results for comparison
cargo bench -- --save-baseline my_baseline
```

## Benchmark Categories

### 1. Triangulation Creation (`triangulation_creation`)

- **Purpose**: Measures time to create Delaunay triangulations of various sizes
- **Test Sizes**: 5, 10, 20, 50, 100 vertices
- **Metrics**: Time per triangulation, throughput (triangulations/second)
- **Use Case**: Identifying scaling behavior for initial setup

### 2. Edge Counting (`edge_counting`)

- **Purpose**: Compares cached vs uncached edge counting performance
- **Test Sizes**: 10, 25, 50, 100, 200 vertices
- **Variants**:
  - `uncached`: Direct computation every time (O(E))
  - `cached`: Using triangulation cache (O(1) when valid)
- **Use Case**: Optimizing frequent edge count queries in simulations

### 3. Geometry Queries (`geometry_queries`)

- **Purpose**: Measures basic geometry operations performance
- **Operations Tested**:
  - `vertex_count`: Count vertices
  - `face_count`: Count faces
  - `euler_characteristic`: Calculate V - E + F
  - `is_valid`: Validate triangulation
  - `iterate_vertices`: Iterate over all vertices
  - `iterate_edges`: Iterate over all edges
  - `iterate_faces`: Iterate over all faces
- **Use Case**: Profiling core geometry backend performance

### 4. Action Calculations (`action_calculations`)

- **Purpose**: Measures CDT action computation performance
- **Test Cases**: Small (10V), Medium (50V), Large (100V) triangulations
- **Formula**: S = -κ₀V - κ₂F + λE (2D Regge action)
- **Use Case**: Optimizing Monte Carlo step calculations

### 5. Ergodic Moves (`ergodic_moves`)

- **Purpose**: Measures performance of CDT move operations
- **Move Types**:
  - `Move22`: Edge flip between triangles
  - `Move13Add`: Add vertex by triangle subdivision
  - `Move31Remove`: Remove vertex by triangle merging
  - `EdgeFlip`: Standard Delaunay edge flip
- **Additional Tests**:
  - `random_move_selection`: Random move type selection
  - `random_move_attempt`: Complete random move attempt
- **Use Case**: Optimizing Monte Carlo move proposal

### 6. Metropolis Simulation (`metropolis_simulation`)

- **Purpose**: Measures short Metropolis-Hastings runs over the CDT move kernels
- **Test Configurations**: 10, 50, 100 requested MC steps
- **Includes**: Configuration validation, move proposal acceptance, accepted move application, and measurements
- **Use Case**: Tracking end-to-end simulation-loop overhead

### 7. Simulation Analysis (`simulation_analysis`)

- **Purpose**: Measures post-simulation analysis performance
- **Operations**:
  - `acceptance_rate`: Calculate move acceptance statistics
  - `average_action`: Calculate mean action over simulation
  - `equilibrium_measurements`: Extract post-thermalization data
- **Use Case**: Optimizing data analysis workflows

### 8. Cache Operations (`cache_operations`)

- **Purpose**: Measures triangulation caching performance
- **Operations**:
  - `refresh_cache`: Populate geometry cache
  - `cache_invalidation`: Cache invalidation cost
- **Use Case**: Optimizing repeated geometry queries

### 9. Validation (`validation`)

- **Purpose**: Measures triangulation validation performance
- **Operations**:
  - `validate_cdt_properties`: Full CDT property validation
- **Use Case**: Profiling debugging and verification overhead

## Performance Expectations

### Typical Performance Ranges (Debug Builds)

- **Triangulation Creation**: ~100μs - 10ms (size-dependent)
- **Cached Edge Count**: ~1μs - 10μs
- **Uncached Edge Count**: ~10μs - 1ms (size-dependent)
- **Action Calculation**: ~100ns - 1μs
- **Ergodic Move**: ~1μs - 100μs
- **MC Step**: ~10μs - 1ms

### Performance Optimization Targets

- **Edge counting**: Achieve >90% cache hit rate in simulations
- **Action calculations**: <1μs per calculation
- **MC steps**: <100μs per step for medium triangulations
- **Memory usage**: Linear scaling with vertex count

## Interpreting Results

### Key Metrics

- **Mean**: Average execution time
- **Std Dev**: Performance consistency
- **Throughput**: Operations per second
- **Regression**: Performance changes over time

### Performance Analysis

```bash
# Compare with previous baseline
cargo bench -- --baseline previous

# Generate detailed HTML report
cargo bench -- --output-format html

# Profile with perf integration (Linux)
cargo bench -- --profile-time=5
```

## Benchmark Configuration

### Criterion Settings

- **Sample Size**: Automatically determined by criterion
- **Measurement Time**: 5 seconds per benchmark
- **Warmup Time**: 3 seconds
- **Confidence Level**: 95%
- **Outlier Detection**: Automatic

### Custom Configurations

The benchmarks use criterion's default statistical analysis with:

- Throughput measurements where applicable
- HTML report generation enabled
- Automatic outlier detection and handling
- Cross-platform timing precision

## Adding New Benchmarks

### Template

```rust
/// Benchmark new CDT operation
fn bench_new_operation(c: &mut Criterion) {
    let mut group = c.benchmark_group("new_operation");

    // Setup test data
    let triangulation = CdtTriangulation2D::new_with_delaunay(20, 1, 2)
        .expect("Failed to create triangulation");

    group.bench_function("operation_name", |b| {
        b.iter(|| {
            let result = operation_to_benchmark(black_box(&triangulation));
            black_box(result)
        });
    });

    group.finish();
}

// Add to criterion_group! macro
criterion_group!(
    benches,
    // ... existing benchmarks ...
    bench_new_operation
);
```

### Best Practices

1. Use `black_box()` to prevent compiler optimizations
2. Create test data outside the benchmark loop when possible
3. Include multiple test sizes for scaling analysis
4. Add throughput measurements for size-dependent operations
5. Document expected performance ranges and optimization targets

## Continuous Integration

### Automated Benchmarking

```bash
# Run the CI regression benchmark contract
just bench-ci

# Performance regression detection
cargo bench -- --save-baseline main
```

### Performance Monitoring

- Track benchmark results over time
- Set up alerts for significant performance regressions
- Compare performance across different hardware configurations
- Profile memory usage alongside timing benchmarks

## Hardware Considerations

### Recommended Setup

- **CPU**: Modern multi-core processor (benchmarks are mostly single-threaded)
- **Memory**: 8GB+ RAM for large triangulation benchmarks
- **Storage**: SSD for faster compilation and test data I/O

### Platform Differences

- macOS: Uses `mach_absolute_time()` for high-precision timing
- Linux: Uses `clock_gettime()` with CLOCK_MONOTONIC
- Windows: Uses `QueryPerformanceCounter()`

Results may vary across platforms due to different timer precision and system overhead.
