# Performance Testing Guide

This document explains the regression and reporting workflow around CDT performance benchmarks. For the benchmark inventory and instructions for adding new
Criterion benchmarks, see [benches/README.md](../benches/README.md).

## Overview

The repository has two complementary performance tracks. Pull requests and main-branch development use Criterion output plus
`scripts/performance_analysis.py` to:

- compare current results with saved baselines;
- report regressions and improvements;
- generate Markdown reports for PR or release review;
- keep local and CI checks using the same benchmark contract.

Release preparation uses `scripts/release_performance.py` to compare isolated source states, retain a canonical CSV/provenance pair, and generate tracked
publication files. The release path never treats timestamped analyzer JSON or Criterion HTML as durable evidence.

The default CI contract combines `benches/ci_performance_suite.rs` with the deterministic assertions in `benches/allocation_profile.rs`. The Criterion suite is
intentionally smaller than the full exploratory benchmark suite so it can provide a stable regression signal across platforms; the allocation check is a
blocking correctness gate rather than a timing baseline.

## Local Commands

```bash
just allocation-check  # Run the deterministic allocation contract
just bench-ci          # Run the allocation contract and CI Criterion suite
just perf-check        # Compare current results against the latest baseline
just perf-check 5.0    # Use a stricter 5% regression threshold
just perf-baseline     # Save current results as a timestamped baseline
just perf-baseline tag # Save current results with a descriptive tag
just perf-report       # Generate a Markdown performance report
just perf-trends 7     # Summarize recent baseline trends
```

The correctness-gated release-signal commands are:

```bash
just benchmark-input-check       # Validate the release benchmark inputs
just bench-latest                # Validate inputs, then measure the current release signal
just bench-save-last             # Refresh the conventional local Criterion baseline
just bench-latest-vs-last        # Measure current and compare it with `last`
just bench-compare last          # Compare existing `new` output with a named sample
just bench-save-baseline v0.2.0  # Measure and save a version-named native sample
```

Useful direct analyzer commands:

```bash
uv run performance-analysis --no-run
uv run performance-analysis --no-run --threshold 5.0
uv run performance-analysis --compare performance_baselines/baseline_pre-change.json
uv run performance-analysis --report performance-report.md
```

`just perf-check` returns exit code `1` when regressions exceed the threshold. Local callers may treat that as blocking. In CI, benchmark noise is reported
but does not by itself fail the PR workflow.

## CI Behavior

The performance workflow:

- blocks on the deterministic allocation contract;
- runs the CI benchmark suite on pull requests;
- compares PR results with the main-branch baseline;
- comments with regressions, improvements, stable benchmarks, and new benchmarks;
- uploads report artifacts;
- saves updated baselines on main after successful merges.

Regression comments should be reviewed carefully, especially for changes touching geometry construction, move proposal enumeration, action calculation,
Metropolis simulation, output generation, or validation.

The separate `.github/workflows/release-benchmarks.yml` workflow runs when a GitHub Release is published. It checks out the exact tag, runs the benchmark
correctness gate, saves the tag-named native Criterion sample, packages only regular files under a `criterion/` archive root, and attaches the archive to
that release. The asset name is `causal-triangulations-vX.Y.Z-criterion-baseline.tar.gz`.

## Release Comparisons And Retained Evidence

Run the complete release transaction before committing a release PR:

```bash
just performance-release v0.1.1 v0.1.0
```

Both tags are optional. The current tag defaults to the Cargo package version and the baseline defaults to the newest published stable release older than
the current version. Explicit tags must use stable `vX.Y.Z` form; release publication rejects equal or reverse-ordered pairs. If the current tag already
exists, its commit and tracked source state must match `HEAD`, preventing unreleased changes from being published under an existing release label.

The command performs these operations in order:

1. Creates detached temporary worktrees for the baseline tag and current `HEAD`. Tracked current changes are copied byte-for-byte; untracked files do not
   enter the measured source state.
2. Runs `benchmark-input-check`'s release tests and `ci_performance_suite` independently in each source state with separate Cargo target directories.
3. Parses Criterion median point estimates and confidence intervals, including current-only and baseline-only benchmark coverage.
4. Writes `target/bench-reports/performance.csv` and `target/bench-reports/performance.provenance.json` before removing temporary measurements.
5. Reloads the retained pair, verifies the CSV digest, release pair, schema, row count, source states, commands, toolchain, Criterion version, benchmark
   contract, and host metadata, then atomically updates tracked publication files.

The CSV is the sole quantitative source for report timings, percentages, confidence-interval classifications, and coverage notes. The matching provenance
binds the exact CSV bytes by SHA-256. A missing pair member tells the operator to rerun `just performance-release`; an existing but mismatched pair is
reported separately as an integrity failure.

The tracked publication set is deliberately narrow: `README.md`, `docs/PERFORMANCE.md`, `docs/assets/performance-comparison.svg`,
`docs/archive/performance/README.md`, and one release-pair archive report. Every destination is checked for repository containment and disjointness from the
retained pair. Promotion is transactional, so a later replacement failure restores earlier files.

After a successful measurement, these commands reuse the retained evidence without invoking Cargo or creating worktrees:

```bash
just performance-doc       # Rebuild the current report, archive entry, and visual
just performance-readme    # Rebuild only the owned README block and visual
```

Both recipes accept an optional bundle directory containing files named `performance.csv` and `performance.provenance.json`. This supports rendering a
copied retained pair while preserving the same strict validation and repository-contained publication destinations.

For audit or reconstruction after both native release assets exist:

```bash
just performance-github-assets v0.1.1 v0.1.0
```

This downloads both tag-pinned archives, rejects traversal paths, links, devices, and non-regular members, verifies the embedded source provenance, and
reconstructs the retained CSV/provenance pair without running benchmarks.

## Benchmark Categories

The CI suite focuses on release-relevant CDT paths:

- open-boundary and toroidal triangulation construction;
- topology, foliation, causality, and simplex-classification validation;
- individual ergodic move attempts;
- proposal-site iteration and single-step Metropolis proposal planning;
- fixed random-move attempt budgets sized as ten initial sweeps;
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
- GitHub Releases retain version-named native Criterion baselines for release reconstruction.
- `target/bench-reports/` retains the most recently generated canonical CSV/provenance pair locally; do not hand-edit either member.

`just clean` runs `cargo clean`, so it removes the retained local pair together with the rest of `target/`. Tracked reports remain, but subsequent
`performance-doc` or `performance-readme` runs require a copied bundle directory or a fresh `performance-release` transaction.

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

`retained performance artifact pair is incomplete`

: Run `just performance-release` to regenerate both members before running `performance-doc` or `performance-readme`. Do not copy a CSV or provenance file
  from a different comparison.

`retained CSV/provenance pair mismatch`

: Both files exist but fail their digest, row-count, schema, or release-pair contract. Preserve them for diagnosis, then rerun `just performance-release`.

High variance

: Close other CPU-heavy work, rerun the benchmark, and compare trends rather than one noisy sample. Treat large PR comments as investigation prompts, not
  automatic proof of a regression.

Need deeper timing data

: Run a focused Criterion group from [benches/README.md](../benches/README.md), inspect the HTML report under `target/criterion/`, or use platform-specific
  profilers. Memory profiling is not exposed as a Cargo feature in this crate; use external profilers or targeted benchmark instrumentation.

## Components

- `benches/ci_performance_suite.rs`: stable CI regression contract
- `benches/cdt_benchmarks.rs`: broader Criterion benchmark groups
- `benches/allocation_profile.rs`: deterministic cached-observable allocation counts
- `scripts/performance_analysis.py`: baseline comparison and report generation
- `scripts/performance_artifacts.py`: strict retained CSV/provenance schema and atomic multi-file replacement
- `scripts/release_performance.py`: tag resolution, isolated measurement, asset reconstruction, and tracked rendering
- `.github/workflows/performance.yml`: CI performance workflow
- `.github/workflows/release-benchmarks.yml`: release-native Criterion asset publication
- `performance_baselines/`: saved local and CI baselines
- `target/bench-reports/`: ignored retained release-comparison evidence used by deterministic render commands
