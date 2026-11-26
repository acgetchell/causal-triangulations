# Performance Testing

CDT keeps its scientific workloads in `benches/` and delegates measurement orchestration, evidence, comparisons, and report publication to
`research-repo-tools`. The September 2026 tooling migration deliberately retires historical reports and legacy formats. New measurements form a fresh series;
old GitHub Release assets remain remote historical records and are not selected by the new recipes.

## Local baseline and comparison

Run on the same quiet host, with the same toolchain and workload:

```bash
just bench-save-baseline last
# Make the intended change.
just bench-latest-vs-last
```

The first command runs integration and physics checks before saving the native Criterion baseline. The second reruns those gates and measurements,
then uses the shared comparison CLI to display absolute baseline/current median times in nanoseconds, recorded intervals, and relative changes.
A missing baseline fails instead of implying success. Local Criterion output lives under `target/criterion/` and is disposable.

`just bench-save-last` is shorthand for `just bench-save-baseline last`. To keep several baselines, pass the same name to `bench-save-baseline` and
`bench-latest-vs-last`. Use `just bench-latest-vs-last cdt-v2-last` for a baseline saved under CDT's previous default.
If the current run is already on disk, `just bench-compare NAME` compares it with the named baseline without measuring again.

`just bench-smoke` checks that harnesses run with small sample counts; it does not produce publishable timing evidence.
`just allocation-check` enforces deterministic budgets for cached observables, checked edge queries, proposal-cache reuse, and unchanged trace storage.
The [benchmark inventory](../benches/README.md) describes the fixtures and persistent-planner and slice-scaling groups.
The broader `just bench` and `just bench-ci` recipes retain their CDT workloads.

## Exploratory command timing

Run the four existing small, medium, large, and extra-large CLI simulations from the repository root:

```bash
just performance-command-time target/timings.json
```

The shared runner builds the release binary before timing, preserves the arguments in `tooling/command-timing.toml`, and records one elapsed-time sample
per case with no warmup. The report includes command, source, executable, toolchain, and host provenance; existing output files are rejected. Process
failures propagate and prevent a successful report. The optional `examples/scripts/performance_test.sh OUTPUT.json` shim invokes the same command.

These timings include process startup and the simulation's configured thermalization and measurements. Dividing a case's requested steps by elapsed
seconds gives only an exploratory end-to-end rate, not a kernel throughput or sampling-efficiency estimate. A single sample does not establish variance,
scaling, or a performance regression. Use the correctness-gated Criterion workflow for release evidence; these reports cannot serve as release baselines.

## Fresh release evidence

The v0.2.0 series starts after the historical v0.1.1 release. Its first report covers all 54 Criterion cases with absolute timings and recorded
uncertainty. A compatible successor can use paired trials. Review host, compiler, dependencies, build policy and workload coverage before interpreting
a comparison. The series declaration is in `benchmark-series/cdt-release-signal.toml`.

On the prepared release branch, use the external owner directory selected for the Cargo version. Keep it for the entire release.

```bash
just release-dependency-builds
just performance-release
just performance-readme --preview
just performance-readme
just performance-check
```

The release command checks published dependencies, exports the selected sources and runs the integration and physics gates before measurement.
It freezes the raw evidence and original provenance in the owner directory. If promotion fails after acquisition,
`just performance-capture` uses those bytes. For an interrupted measurement, `just performance-resume` resumes its checkpoint.
README rendering is independently repeatable.
Commit the generated outputs after reviewing
`docs/performance/current.md` and the retained run under `docs/performance/runs/`.

## README publication

The README configuration selects the state-dependent proposal median and its confidence bounds. Run `just performance-readme --check` to check the
table against the promoted report. `just performance-check` validates retained output without measuring.

After merge, follow the [release guide](RELEASING.md#wait-for-the-draft-and-review-its-assets). The release workflow receives the existing staged
archive by its original GitHub asset ID. Recovery uses the same selection and archive name, `causal-triangulations-TAG-cdt-series.tar.gz`.

Marginal timing intervals do not establish an interval for a speed ratio or statistical significance.
These measurements do not establish ergodicity, mixing, thermalization, or continuum behavior.

## CI evidence

The performance workflow runs the deterministic allocation check and correctness-gated Criterion suite, then uploads raw measurements for 30 days.
It does not restore legacy benchmark caches or claim a regression comparison against an unknown host.
Normal `just ci` compiles benchmarks and enforces the allocation contract; it does not run timing comparisons.

See [the benchmark inventory](../benches/README.md) for workload design and [development commands](dev/DEVELOPING.md) for the slow debugging probes.
