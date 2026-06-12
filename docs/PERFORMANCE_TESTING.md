# Performance Testing Guide

This document explains the regression and reporting workflow around CDT performance benchmarks. For the benchmark inventory and instructions for adding new
Criterion benchmarks, see [benches/README.md](../benches/README.md).

## Overview

The performance workflow uses Criterion benchmark output plus `scripts/performance_analysis.py` to:

- compare current results with saved baselines;
- report regressions and improvements;
- generate Markdown reports for PR or release review;
- keep local and CI checks using the same benchmark contract.

The default CI contract is `benches/ci_performance_suite.rs`. It is intentionally smaller than the full exploratory benchmark suite so it can provide a stable
regression signal across platforms.

## Local Commands

```bash
just bench-ci          # Run the CI regression benchmark contract
just perf-check        # Compare current results against the latest baseline
just perf-check 5.0    # Use a stricter 5% regression threshold
just perf-baseline     # Save current results as a timestamped baseline
just perf-baseline tag # Save current results with a descriptive tag
just perf-report       # Generate a Markdown performance report
just perf-trends 7     # Summarize recent baseline trends
```

Useful direct analyzer commands:

```bash
uv run performance-analysis --no-run
uv run performance-analysis --no-run --threshold 5.0
uv run performance-analysis --compare performance_baselines/baseline_pre-change_*.json
uv run performance-analysis --report performance-report.md
```

`just perf-check` returns exit code `1` when regressions exceed the threshold. Local callers may treat that as blocking. In CI, benchmark noise is reported
but does not by itself fail the PR workflow.

## CI Behavior

The performance workflow:

- runs the CI benchmark suite on pull requests;
- compares PR results with the main-branch baseline;
- comments with regressions, improvements, stable benchmarks, and new benchmarks;
- uploads report artifacts;
- saves updated baselines on main after successful merges.

Regression comments should be reviewed carefully, especially for changes touching geometry construction, move proposal enumeration, action calculation,
Metropolis simulation, output generation, or validation.

## Benchmark Categories

The CI suite focuses on release-relevant CDT paths:

- open-boundary and toroidal triangulation construction;
- topology, foliation, causality, and simplex-classification validation;
- individual ergodic move attempts;
- proposal-site iteration and single-step Metropolis proposal planning;
- short random-move sweeps;
- short Metropolis simulations.

The broader Criterion suite includes exploratory groups for geometry queries, cache behavior, action calculations, simulation analysis, and validation. Those
are documented in [benches/README.md](../benches/README.md).

## Performance Workflow

Before a performance-sensitive change:

```bash
just bench-ci
just perf-baseline pre-change
```

During development:

```bash
just perf-check 15.0
```

Before review:

```bash
just perf-check
just perf-report
```

For optimization PRs, include a short performance summary:

```markdown
## Performance Impact

- proposal-site enumeration: 18% faster on the CI suite
- short Metropolis runs: stable within threshold
- memory allocation: no new persistent allocations in the hot path
```

## Baselines

- Main-branch baselines are saved by CI.
- Feature baselines can be saved locally with descriptive tags.
- Release baselines should use version tags.
- The analyzer keeps recent baselines and report artifacts for comparison.

Keep baseline names descriptive enough to recover the comparison later:

```bash
just perf-baseline pre-proposal-cache
just perf-baseline v0.1.1
```

## Troubleshooting

`No benchmark results found`

: Run `just bench-ci` first, or run `uv run performance-analysis --no-run` only after Criterion JSON output exists.

`No baseline found for comparison`

: Save one with `just perf-baseline initial`, or compare directly against a known baseline file.

High variance

: Close other CPU-heavy work, rerun the benchmark, and compare trends rather than one noisy sample. Treat large PR comments as investigation prompts, not
  automatic proof of a regression.

Need deeper timing data

: Run a focused Criterion group from [benches/README.md](../benches/README.md), inspect the HTML report under `target/criterion/`, or use platform-specific
  profilers. Memory profiling is not exposed as a Cargo feature in this crate; use external profilers or targeted benchmark instrumentation.

## Components

- `benches/ci_performance_suite.rs`: stable CI regression contract
- `benches/cdt_benchmarks.rs`: broader Criterion benchmark groups
- `scripts/performance_analysis.py`: baseline comparison and report generation
- `.github/workflows/performance.yml`: CI performance workflow
- `performance_baselines/`: saved local and CI baselines
