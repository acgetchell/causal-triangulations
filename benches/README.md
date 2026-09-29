# CDT Benchmarks

This document describes the Criterion benchmark suites. For regression workflows, baselines, CI behavior, and report generation, see
[`docs/BENCHMARKING.md`](../docs/BENCHMARKING.md).

## Running Benchmarks

```bash
cargo bench
just bench-ci
cargo bench --profile perf --bench ci_performance_suite
cargo bench --profile perf --bench allocation_profile
```

Run a focused group:

```bash
cargo bench triangulation_creation
cargo bench edge_counting
cargo bench geometry_queries
cargo bench action_calculations
cargo bench ergodic_moves
cargo bench metropolis_simulation
cargo bench simulation_analysis
cargo bench cache_operations
cargo bench validation
```

Criterion writes HTML reports under `target/criterion/`.

## CI Performance Suite

`ci_performance_suite` is the smaller benchmark contract used by the performance tooling. It runs with the Cargo `perf` profile and focuses on CDT workflows
that should stay comparable across releases:

- generating open-boundary and toroidal CDT triangulations;
- validating generated triangulations;
- attempting individual ergodic move types;
- scaling guaranteed-success inverse volume finalization across increasing toroidal CDT meshes;
- iterating proposal-site candidates through public move-attempt and single-step Metropolis proposal paths;
- separating cold/warm proposal views and 32-attempt persistent planner workloads that discard or commit concrete plans;
- varying time-slice count at a fixed 256-vertex base mesh for local inverse-move finalization;
- evaluating state-dependent family policies that inspect the complete offered-site views;
- executing fixed random-move attempt budgets equal to ten initial sweeps, so reported throughput exactly matches timed attempts;
- running short Metropolis simulations sized as ten initial sweeps.

Keep this suite stable and release-relevant. Exploratory or noisy benchmarks belong in `cdt_benchmarks.rs`.

`allocation_profile` is a deterministic heap-allocation contract for cached observables, checked edge queries, proposal caches, and trace storage.
It verifies that a cached edge-count read allocates nothing and an owned slab-triangle-profile read performs exactly one vector allocation.
Checked endpoint queries and warm family views allocate nothing, including after discarded and committed concrete flips and reverse state-dependent policy
evaluation. A 1,000-step unchanged trajectory with 64 profile entries must share measurement snapshots and retain less than 500 KB of measured allocations.
Fixture construction remains outside allocation measurement. The contract blocks `just ci`, `just bench-ci`, and the performance workflow.

## Benchmark Groups

### `triangulation_creation`

Measures Delaunay-backed CDT construction across small to larger initial configurations.

Use for:

- initial setup scaling;
- open-boundary and toroidal constructor regressions;
- backend construction changes.

### `edge_counting`

Compares cached and uncached edge-count queries.

Use for:

- cache effectiveness;
- repeated action/observable queries;
- validation and output hot paths.

### `geometry_queries`

Measures geometry interrogation operations such as vertex/edge/face iteration, Euler characteristic calculation, and structural validation.

Use for backend query regressions and geometry abstraction overhead.

### `action_calculations`

Measures CDT Regge-action evaluation:

```text
S = -κ₀ N0 - κ₂ N2 + λ N1
```

Use for Metropolis target and observable hot paths.

### `ergodic_moves`

Measures CDT move proposal and application paths:

- `Move22`: foliation-aware `(2,2)` edge flip;
- `Move13Add`: local volume-add proposal;
- `Move31Remove`: local inverse volume proposal;
- `EdgeFlip`: API-compatible alias for the 2D k=2 edge flip;
- random move selection and attempt paths.

The CI suite also includes `cdt_proposal_site_move_attempts_2d` and `cdt_single_metropolis_proposal_2d`. These exercise explicit proposal-site iteration,
cloned proposed-state mutation, and reverse-site counting for the Hastings ratio.

`cdt_persistent_proposals_2d` adds cold/warm family views and seeded 32-attempt batches with one planner per batch. Concrete plans are either discarded or
committed, followed by live-state inspection. Fixture construction and initial cache warming are outside the batch measurement; ordinary local rejections
remain part of the measured proposal workload. These cases isolate cache reuse from driver setup, trace output, and terminal validation.

`cdt_local_finalization_by_slices_2d` measures successful inverse volume moves at 8, 16, 32, and 64 time slices. Each fixture starts from 256 toroidal vertices
and performs one insertion during setup, so the measured removal sees 257 vertices at every slice count. Existing benchmark workloads remain unchanged.

### `metropolis_simulation`

Measures short Metropolis-Hastings runs over real CDT move kernels.

Includes configuration validation, proposal planning, accepted-move application, telemetry, and measurements.

### `simulation_analysis`

Measures post-run analysis such as acceptance rates, average action, and post-thermalization measurement extraction.

Use for output and analysis workflow regressions.

### `cache_operations`

Measures cache refresh and invalidation costs.

Use for repeated geometry-query and move-finalization paths.

### `validation`

Measures CDT validation, including topology, foliation, causality, and simplex classification.

Use for correctness-checking overhead.

## Adding Benchmarks

Use Criterion and keep setup outside the timed loop when setup is not the thing being measured:

```rust
use criterion::{Criterion, black_box};

fn bench_new_operation(c: &mut Criterion) {
    let mut group = c.benchmark_group("new_operation");
    let triangulation = build_fixture();

    group.bench_function("operation_name", |b| {
        b.iter(|| {
            let result = operation_to_benchmark(black_box(&triangulation));
            black_box(result)
        });
    });

    group.finish();
}
```

Guidelines:

- include enough sizes to show scaling;
- use seeded or deterministic fixtures where possible;
- name benchmarks by the behavior being measured;
- keep the CI suite stable and reasonably fast;
- document expected changes in PRs when benchmark behavior changes.

## Interpreting Results

Criterion reports means, confidence intervals, outliers, and change estimates when baselines are available. Small changes can be noise. Use
[`docs/BENCHMARKING.md`](../docs/BENCHMARKING.md) for thresholded regression checks and report generation.

Hardware, operating system, CPU load, and thermal behavior can change benchmark results. Prefer same-machine comparisons for local optimization work and CI
baselines for PR-level regression signals.
