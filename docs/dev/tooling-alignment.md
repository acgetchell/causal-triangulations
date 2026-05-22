# Tooling Alignment

This note records the issue #112 comparison between `causal-triangulations` and the sibling `markov-chain-monte-carlo` repository. Keep it current when changing
repository tooling so future updates can be deliberate rather than copied wholesale.

## Current Parity

Both repositories now have explicit local configuration for the core Rust and Python support-tooling loop:

- `rust-toolchain.toml` pins the Rust toolchain and development components.
- `rustfmt.toml` records stable Rust formatting settings used by `cargo fmt`.
- `.taplo.toml` keeps TOML formatting conservative and Cargo-like.
- `ty.toml` scopes Ty to `scripts/` and pins the supported Python version.
- `pyproject.toml` owns Ruff, Ty, pytest, uv packaging, and uv-managed development dependencies.
- `rumdl.toml`, `dprint.json`, `.yamllint`, `typos.toml`, and `clippy.toml` define documentation and configuration checks.
- `justfile` is the single local entry point for formatting, linting, tests, coverage, Semgrep, changelog, and setup commands.

Non-Rust repository tooling is normalized to a 160-column width: Ruff for Python support scripts, `rumdl` plus the Markdown raw line-length guard, `yamllint`,
`dprint`/`pretty_yaml`, and the generated changelog postprocessor all use 160 columns. Rust remains on the narrower `rustfmt` `max_width = 100` setting because
wide Rust signatures, trait bounds, and method chains become harder to scan at 160 columns.

## Python Tooling

CDT's Python tooling is broader than MCMC's. MCMC currently has changelog and tag helpers; CDT also has benchmark, hardware, coverage, archive, and
performance-analysis scripts. The shared script patterns have already been ported here:

- secure subprocess wrappers in `scripts/subprocess_utils.py`;
- typed `subprocess.CompletedProcess[str]` helpers in tests;
- Ruff, Ty, pytest, and uv-managed development dependencies in `pyproject.toml`;
- Python Semgrep rules and fixtures for broad exception catches across all `scripts/**/*.py`, raw `Exception` in tests, ad hoc subprocess mocks, and missing
  return annotations.

The broad-exception Semgrep rule now covers the full Python support-script tree. `scripts/benchmark_utils.py`, `scripts/hardware_utils.py`, and
`scripts/performance_analysis.py` use typed recoverable exception boundaries, so new broad `except Exception` recovery paths are treated as tooling regressions
rather than accepted legacy cleanup.

## Intentional Differences

Some differences remain because CDT has different workflows and project invariants:

- CDT runs examples through `scripts/run_all_examples.sh`, which discovers current examples dynamically, builds release examples once, applies a timeout while
  running the compiled binaries, and checks stable semantic output markers in `--validate` mode without requiring exact numeric output.
- CDT keeps `archive-changelog` so completed release series move under `docs/archive/changelog/`; MCMC does not yet archive old changelog sections.
- CDT keeps a dedicated `performance.yml` workflow and local `perf-*` recipes. MCMC does not have matching CDT benchmark-baseline tooling.
- CDT exposes feature-gated long-running Rust checks through the `slow-tests` Cargo feature and the `just test-slow` recipe, keeping normal CI fast while giving
  stabilization work a named path for heavier integration coverage.
- CDT has a repository rule SARIF workflow for the local Semgrep rules. A Codacy workflow was not ported because it depends on project-specific
  `CODACY_PROJECT_TOKEN` setup and would duplicate the existing repository-rule SARIF signal until Codacy is configured for this repository.
- CDT Semgrep rules include geometry-backend isolation, foliation/topology validation, focused prelude imports, Python support-script discipline, and typed
  error policies. These are repository-specific and should not be weakened while porting generic rules.
- CDT and MCMC both require Python `>=3.12` for repository-managed support tooling.

## Ported Updates

The useful updates ported from MCMC are:

- explicit `rustfmt.toml` formatting configuration;
- explicit uv package mode and pytest 9 minimum in `pyproject.toml`;
- CodeQL analysis for GitHub Actions and Rust, using `build-mode: none` for Rust;
- the MCMC-style `cliff.toml` template and `just changelog-unreleased <version>` flow, adapted to keep CDT's changelog archive step and avoid temporary local
  release tags;
- a Semgrep rule that rejects `NaN` and infinity defaults after failed floating-point conversions, with a regression fixture under `tests/semgrep/`.
- production-only Rust Semgrep rules that reject bare `unwrap()` and explicit `panic!` in non-test `src/` code while preserving idiomatic fail-fast usage in
  tests, doctests, examples, and benchmark setup.
- an `examples-validate` recipe that runs Cargo examples and verifies stable output markers for the user-facing example contracts.

The useful Semgrep updates ported from the sibling `delaunay` repository are:

- a silent numeric-conversion fallback rule, adapted to CDT `src/` paths and cleaned up in observable/result aggregation code so conversion failures use
  explicit branches instead of `unwrap_or` sentinels;
- preventive Rust rules for `partial_cmp(...).unwrap_or(...)`, function-local imports in production source, and `#[allow(clippy::...)]` suppressions, each kept
  repository-owned with CDT rule IDs and fixtures.

The useful `justfile` updates ported from `delaunay` are:

- `ci-slow`, a named CI-plus-slow-tests workflow using CDT's existing `slow-tests` feature gate;
- `cargo nextest` for runnable Rust unit, integration, CLI, slow, example, and release test recipes, while keeping rustdoc doctests on `cargo test --doc`
  because nextest does not execute doctests;
- single-pass release example builds in `scripts/run_all_examples.sh`, which preserve semantic output validation while avoiding repeated `cargo run --example`
  invocations;
- `bench-smoke`, adapted to CDT's `cdt_benchmarks` harness and Criterion's minimal sample settings so benchmark harnesses can be smoke-tested without producing
  baseline-quality numbers;
- `bench-test-compile`, which layers release-profile integration-test compilation on top of the existing warning-denying benchmark compile check;
- opt-in release hygiene recipes `unused-deps` and `publish-check`, kept outside the default `lint` and `ci` paths so they are available before releases without
  making routine validation slower or more tool-dependent;
- `rumdl` Markdown checking, `dprint`/`pretty_yaml` YAML formatting, check/fix aliases, and a corrected rename/copy note in `spell-check`.
- repository-owned Semgrep rules that keep user-facing `just check` examples before `just fix` examples and enforce SHA-pinned, allowlisted GitHub Actions with
  readable version comments. Dependabot remains the update path for pinned external actions, with review focused on preserving both the SHA and readable version
  comment.
- Delaunay's non-Rust 160-column tooling policy, adapted for CDT while intentionally leaving Rust formatting at `rustfmt`'s 100-column width.

## CI Shape Evaluation

Issue #131 evaluated larger CI-shape changes after the lower-risk speedups from
issue #130. Recent successful PR CI runs on May 22, 2026 showed the macOS and
Windows matrix legs often determining wall-clock duration:

- `perf/130-ci-speedups` (#137): Ubuntu completed in about 3m31s, macOS in
  about 3m52s, and Windows in about 3m40s.
- `feat/checked-delaunay-api` (#128): Ubuntu completed in about 4m28s, macOS
  in about 4m56s, and Windows in about 6m10s.

The repository intentionally keeps the existing `build (ubuntu-latest)`,
`build (macos-latest)`, and `build (windows-latest)` required check contexts,
and keeps Rust target coverage on all three platforms. A broader split that
would run the comprehensive repository/tooling path only on Linux was rejected
because it would weaken the current macOS build signal.

The adopted change is smaller: the Windows leg now uses
`cargo nextest run --workspace --all-targets --no-run` instead of an initial
`cargo build --all-targets`, then runs lib, binary, example, and integration
test targets through one nextest invocation. That keeps the full Rust target
compile surface on Windows, including benchmark compilation, while avoiding
accidental Criterion benchmark execution in the test job. Rustdoc doctests
remain on `cargo test --doc` because nextest does not execute doctests.

The May 2026 Delaunay tooling refresh was reviewed for portable pieces. CDT
adopted `cargo-llvm-cov` 0.8.7 in both local setup and Codecov, added `rumdl`
formatting to generated changelog recipes, and ported archive heading
normalization so historical changelog archives are cleaned even when a release
only rewrites the active minor series. Delaunay's profiling, Codacy SARIF
filtering, and benchmark-comparison machinery remained deferred from that pass
because CDT needed a different benchmark contract and has no configured Codacy
workflow.

The follow-up benchmark alignment introduced a CDT-specific
`ci_performance_suite` rather than copying Delaunay's dimensional construction
contract. CDT's suite covers generated open-boundary and toroidal
triangulations, validation, individual ergodic move attempts, ten-sweep
random-move workloads, and ten-sweep Metropolis runs. The suite uses a local
`perf` profile and feeds the existing performance-analysis scripts, while the
larger Delaunay profiling and same-machine comparison helpers remain deferred.

## Deferred Updates

These were evaluated but not ported in this pass:

- `codacy.yml`: defer until the repository has an intentional Codacy project token and a decision about whether Codacy should upload repository-owned
  OpenGrep/Semgrep findings in addition to `.github/workflows/semgrep-sarif.yml`.
- Delaunay's hot-path `FastHashMap`/`FastHashSet` rule: defer because CDT does not currently define equivalent hash aliases or the same `src/core` hot-path
  layout.
- Delaunay's `profiling_suite` and same-machine baseline recipes: defer because CDT's `ci_performance_suite` now provides the portable regression contract,
  while deeper profiling still needs CDT-specific benchmark interpretation.
