# Performance Testing

CDT keeps its scientific workloads in `benches/` and delegates measurement orchestration, evidence, comparisons, and report publication to
`research-repo-tools`. The September 2026 tooling migration deliberately retires historical reports and legacy formats. New measurements form a fresh series;
old GitHub Release assets remain remote historical records and are not selected by the new recipes.

## Local baseline and comparison

Run on the same quiet host, with the same toolchain and workload:

```bash
just bench-save-last
# Make the intended change.
just bench-latest-vs-last
```

The first command runs integration and physics checks before saving the native Criterion baseline. The second reruns those gates and measurements,
then uses the shared comparison CLI to display absolute baseline/current median times in nanoseconds, recorded intervals, and relative changes.
A missing baseline fails instead of implying success. Local Criterion output lives under `target/criterion/` and is disposable.

`just bench-smoke` checks that harnesses run with small sample counts; it does not produce publishable timing evidence.
`just allocation-check` enforces the deterministic cached-observable allocation contract.
The broader `just bench` and `just bench-ci` recipes retain their CDT workloads.

## Fresh release evidence

The next tagged release establishes the first new shared baseline. Create a draft GitHub Release for that tag, then dispatch
`.github/workflows/release-benchmarks.yml` with its `release_tag`.
The workflow validates the draft, measures the exact tag with read-only permissions and caches disabled, then attaches inert verified bytes in a separate
writer job. It leaves the release as a draft for review. The new asset name is `causal-triangulations-TAG-cdt-baseline-v2.tar.gz`.

To package an exact clean local tag with the same shared format:

```bash
just performance-baseline "$TAG"
```

After two fresh releases exist, compare their assets without running benchmarks:

```bash
just performance-github-assets "$TAG" "$PREVIOUS_TAG"
```

For a prospective release, the explicit comparison recipe creates temporary Git worktrees and runs the trusted benchmark command from
`tooling/benchmark.toml`:

```bash
just performance-release "$TAG" "$PREVIOUS_TAG"
```

Agents following this repository's Git restrictions must leave that mutating worktree command to the user. It is outside normal validation.
Measurements capture source/harness fingerprints, host identity, Rust, Criterion, Delaunay, la-stack, and MCMC versions.
Matching known host OS, architecture, and CPU are required for local pairs. Review dependency and workload changes before interpreting any ratio.

The recipe saves shared JSON comparison/evidence files under `target/bench-reports/`, then promotes the report and retained evidence into
`docs/performance/v2/`. Missing or mismatched evidence fails before publication.
Use `just performance-doc --payload PATH --manifest PATH` to promote an already reviewed pair, or `just performance-doc --check` to check retained output.
No current report is tracked until the first compatible new comparison exists.

## README publication

Copy `tooling/performance-readme.example.toml` to `tooling/performance-readme.toml` when a fresh retained pair is available.
Set its evidence paths, independently verified source commit/tag pins, and representative benchmark rows. Keep the references to the package version
and both report tags. Preview before publishing:

```bash
just performance-readme tooling/performance-readme.toml --preview
just performance-readme tooling/performance-readme.toml
just performance-readme tooling/performance-readme.toml --check
```

The table includes **absolute baseline and current times with units**, recorded confidence bounds, the baseline/current point ratio, and percent reduction.
This makes the cost of each operation visible rather than presenting relative speed alone. It also reports added/missing workload coverage.
The full report retains provenance and all comparable workloads. The template intentionally omits a ratio-only SVG.

Marginal timing intervals do not establish an interval for a speed ratio or statistical significance.
These measurements do not establish ergodicity, mixing, thermalization, or continuum behavior.

## CI evidence

The performance workflow runs the deterministic allocation check and correctness-gated Criterion suite, then uploads raw measurements for 30 days.
It does not restore legacy benchmark caches or claim a regression comparison against an unknown host.
Normal `just ci` compiles benchmarks and enforces the allocation contract; it does not run timing comparisons.

See [the benchmark inventory](../benches/README.md) for workload design and [development commands](dev/commands.md) for the slow debugging probes.
