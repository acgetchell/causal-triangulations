# Tooling Alignment

This note records the issue #112 comparison between `causal-triangulations` and the sibling `markov-chain-monte-carlo` repository. Keep it current when changing
repository tooling so future updates can be deliberate rather than copied wholesale.

## Current Parity

Both repositories now have explicit local configuration for the core Rust and Python support-tooling loop:

- `rust-toolchain.toml` pins the Rust toolchain and development components.
- `rustfmt.toml` records stable Rust formatting settings used by `cargo fmt`.
- `.taplo.toml` keeps TOML formatting conservative and Cargo-like.
- `ty.toml` scopes Ty to `scripts/` and pins the supported Python version.
- `pyproject.toml` owns Ruff, Ty, pytest, Semgrep, actionlint, yamllint, shellcheck, shfmt, uv packaging, and uv-managed development dependencies.
- `rumdl.toml`, `dprint.json`, `.yamllint`, `typos.toml`, and `clippy.toml` define documentation and configuration checks.
- `justfile` is the single local entry point for formatting, linting, tests, coverage, Semgrep, changelog, and setup commands.

Non-Rust repository tooling is normalized to a 160-column width: Ruff for Python support scripts, `rumdl` plus the Markdown raw line-length guard, `yamllint`,
`dprint`/`pretty_yaml`, and the generated changelog postprocessor all use 160 columns. Rust remains on the narrower `rustfmt` `max_width = 100` setting because
wide Rust signatures, trait bounds, and method chains become harder to scan at 160 columns.

The Markdown raw line-length guard counts bytes with `LC_ALL=C` so macOS, Linux, and Windows agree on UTF-8 punctuation near the 160-column boundary.

## Python Tooling

CDT's Python tooling is broader than MCMC's. MCMC currently has changelog and tag helpers; CDT also has benchmark, hardware, coverage, archive, and
performance-analysis scripts. The shared script patterns have already been ported here:

- secure subprocess wrappers in `scripts/subprocess_utils.py`;
- typed `subprocess.CompletedProcess[str]` helpers in tests;
- Ruff, Ty, pytest, and uv-managed development dependencies in `pyproject.toml`;
- actionlint, yamllint, shellcheck, and shfmt are installed through the uv development environment so the same `just ci` path can run on Linux, macOS, and
  Windows without Homebrew or runner-global tool assumptions;
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
  stabilization work a named path for heavier integration coverage. Issue #140 release validation runs those slow tests through CDT's local `perf` profile,
  matching the dedicated large-scale debug recipes instead of measuring default test-profile overhead.
- CDT has a repository rule SARIF workflow for the local Semgrep rules. A Codacy workflow was not ported because it depends on project-specific
  `CODACY_PROJECT_TOKEN` setup and would duplicate the existing repository-rule SARIF signal until Codacy is configured for this repository.
- CDT Semgrep rules include geometry-backend isolation, foliation/topology validation, focused prelude imports, doctest assertion-idiom enforcement,
  Python support-script discipline, and typed error policies. These are repository-specific and should not be weakened while porting generic rules. The
  `prefer-assert-matches-in-doctests` rule keeps public `///` examples on `std::assert_matches` (Rust 1.97.1) so documentation teaches the diagnostic-friendly
  idiom rather than `assert!(matches!(...))`.
- CDT requires Python `>=3.14` for repository-managed support tooling, matching the local `.python-version`, Ruff target, Ty environment, and CI setup.

## Ported Updates

The useful updates ported from MCMC are:

- explicit `rustfmt.toml` formatting configuration;
- explicit uv package mode and pytest 9 minimum in `pyproject.toml`;
- docs.rs package metadata with `all-features = true`, matching MCMC so rendered API documentation includes every public feature-gated surface;
- CodeQL analysis for GitHub Actions and Rust, using `build-mode: none` for Rust;
- the MCMC-style `cliff.toml` template and `just changelog-unreleased <version>` flow, adapted to keep CDT's changelog archive step and avoid temporary local
  release tags;
- a Semgrep rule that rejects `NaN` and infinity defaults after failed floating-point conversions, with a regression fixture under `tests/semgrep/`.
- production-only Rust Semgrep rules that reject bare `unwrap()` and explicit `panic!` in non-test `src/` code while preserving idiomatic fail-fast usage in
  unit and integration tests.
- public-surface Rust Semgrep rules that reject `unwrap()` and `expect()` in public doctests, Cargo examples, and benchmark harnesses, with benchmarks using
  explicit fixture helpers so failed setup still reports the operation that failed.
- The `causal-triangulations.rust.no-unwrap-expect-in-doctests` rule now distinguishes code-like `.unwrap()` and `.expect(...)` calls from prose mentions, and
  `causal-triangulations.rust.no-unwrap-expect-in-benches-examples` keeps Semgrep fixture cross-matches scoped to the
  `tests/semgrep/src/project_rules/rust_style.rs` fixture shape. Before this refinement, prose such as "do not use `.unwrap()`" could be reported and Semgrep
  test mode needed broader name-based exclusions; after it, public-surface checks still reject panic-style calls while fixture-only exceptions are explicitly
  anchored.
- low-risk MCMC Semgrep rules for public `*_unchecked` Rust APIs, dynamic error erasure in examples/benchmarks, and dynamic error erasure in doctests. The CDT
  versions keep repository-owned rule IDs, explicit fixture paths, and typed-error guidance so user-facing examples continue to model `CdtResult` or concrete
  errors.
- an `examples-validate` recipe that runs Cargo examples and verifies stable output markers for the user-facing example contracts.

The useful Semgrep updates ported from the sibling `delaunay` repository are:

- a silent numeric-conversion fallback rule, adapted to CDT `src/` paths and cleaned up in observable/result aggregation code so conversion failures use
  explicit branches instead of `unwrap_or` sentinels;
- preventive Rust rules for `partial_cmp(...).unwrap_or(...)`, function-local imports in production source, and `#[allow(clippy::...)]` suppressions, each kept
  repository-owned with CDT rule IDs and fixtures.

The useful `justfile` updates ported from `delaunay` are:

- `ci-slow`, a named CI-plus-slow-tests workflow using CDT's existing `slow-tests` feature gate and local `perf` profile so large-scale toroidal probes validate
  release-relevant behavior within a bounded runtime;
- `cargo nextest` for runnable Rust unit, integration, CLI, slow, example, and release test recipes, while keeping rustdoc doctests on `cargo test --doc`
  because nextest does not execute doctests;
- single-pass release example builds in `scripts/run_all_examples.sh`, which preserve semantic output validation while avoiding repeated `cargo run --example`
  invocations;
- `bench-smoke`, adapted to CDT's `cdt_benchmarks` harness and Criterion's minimal sample settings so benchmark harnesses can be smoke-tested without producing
  baseline-quality numbers;
- `bench-test-compile`, which layers release-profile integration-test compilation on top of the existing warning-denying benchmark compile check;
- opt-in release hygiene recipes `unused-deps` and `publish-check`, kept outside the default `lint` and `ci` paths so they are available before releases without
  making routine validation slower or more tool-dependent;
- `citation-check`, using `cffconvert` through `uvx`, and inclusion of `CITATION.cff` in YAML/CFF formatting and linting so research-software citation metadata
  is schema-validated before release;
- `rumdl` Markdown checking, `dprint`/`pretty_yaml` YAML formatting, check/fix aliases, and a corrected rename/copy note in `spell-check`.
- repository-owned Semgrep rules that keep user-facing `just check` examples before `just fix` examples and enforce SHA-pinned, allowlisted GitHub Actions with
  readable version comments. Dependabot remains the update path for pinned external actions, with review focused on preserving both the SHA and readable version
  comment.
- Delaunay's non-Rust 160-column tooling policy, adapted for CDT while intentionally leaving Rust formatting at `rustfmt`'s 100-column width.
- MCMC-boundary Semgrep rules for issue #155, which reject production CDT-local Metropolis-Hastings `exp(log_alpha)` acceptance draws and manual
  accepted/rejected sampler counter increments. These are CDT-specific because this crate still owns proposal planning, proposal-site telemetry, and result
  translation, while `markov-chain-monte-carlo` owns the reusable sampler mechanics.
- A notebook hygiene Semgrep rule now rejects committed `.ipynb` execution counts and output objects in source notebooks. This complements the structured
  `just notebook-output-check` and `just notebook-check` recipes: Semgrep gives a fast repository-rule signal during normal linting, while the notebook recipe
  remains the authoritative JSON/output and headless-execution validation path.
- The planned-step sampler-state sync rule now anchors `record_planned_step(...)` to call statements so it continues to catch missing
  `sampler.replace_state(...)` after recorded planned steps without false-positive matches on the helper function declaration when that helper contains fallible
  telemetry construction.
- The Markdown and spelling tool pins in the justfile match the current Cargo-installed sibling-repository tools while preserving the existing
  `rumdl.toml`, `typos.toml`, and raw 160-column guard.
- The Cargo manifest description changed from `Causal Dynamical Triangulations in d-dimensions` to
  `Validated 1+1 Causal Dynamical Triangulations for quantum gravity`. This mirrors a config/manifest change per the repository guideline and records why the
  new wording was chosen: it makes the validated 1+1 scope and quantum-gravity domain explicit in package metadata. This update fulfills the
  `{.github/**/*,*.yml,*.yaml,*.toml,*.json}` tooling-alignment requirement for the `Cargo.toml` description edit.
- Python support tooling now targets Python 3.14 across `.python-version`, `pyproject.toml`, `ty.toml`, and CI, and `just python-typecheck` runs
  `uv run ty check scripts/ --error all`. This closes the issue #182 follow-up by making strict Ty checking the default local and CI contract while keeping
  every declared Python-version surface aligned with the repository minimum.

## Issue #162 CI And Security Alignment

Issue #162 refreshed the CI and security baseline against `markov-chain-monte-carlo` after acgetchell/markov-chain-monte-carlo#68 and #57:

- The main CI matrix now installs Python tooling with `uv sync --locked --group dev` and runs `just ci` on Ubuntu, macOS, and Windows.
- Rust CLI tools used in PR-running workflows are installed through `taiki-e/cache-cargo-install-action@417450f3c33ee20393705369577571770643d4c7`
  (`v3.0.7`) instead of `taiki-e/install-action` or ad hoc `cargo install` cache scripts.
- The GitHub Actions Semgrep allowlist now permits `taiki-e/cache-cargo-install-action` and `zizmorcore/zizmor-action`, and no longer permits
  `taiki-e/install-action`.
- Repository-owned Semgrep rules now also guard checkout credential persistence, `pull_request_target`, direct `github-script` expression interpolation,
  unlocked workflow `uv sync`, direct Python `subprocess.run` bypasses, and direct MCMC imports outside the CDT Metropolis adapter boundary.
- Tool versions are pinned in `justfile` constants for `cargo-nextest`, `dprint`, `rumdl`, `taplo`, `typos-cli`, `zizmor`, `cargo-llvm-cov`, and `git-cliff`;
  workflows resolve them with `just --evaluate` after the repository-local bootstrap action installs the pinned `just` version.
- Local validation includes `just zizmor` through `lint-config`, while `.github/workflows/zizmor.yml` uploads the GitHub Actions security signal in CI.
- `SECURITY.md` documents private vulnerability reporting and the repository security check set.

Cold-cache PR runs after tool-version changes are expected to be slower while `taiki-e/cache-cargo-install-action` seeds tool caches. Warm-cache timing should
be measured after the first cache-warming run before making additional CI-shape changes.

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
and keeps Rust target coverage on all three platforms. Each matrix leg runs the
comprehensive `just ci` path for the runner's native host and separately checks
the library and CLI with all features for the declared Rust target. This matters
on the ARM64 `macos-latest` runner: native tests exercise
`aarch64-apple-darwin`, while the explicit target check preserves compile
coverage for `x86_64-apple-darwin`. The target check stays production-only
because cross-target tests, examples, and Criterion benchmarks may require
execution or a target-native C toolchain. A broader split that would run the
comprehensive repository/tooling path only on Linux was rejected because it
would weaken the current macOS build signal.

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

Codecov status thresholds now match Delaunay's stricter coverage policy:
project coverage targets 90% with a 1% threshold, and patch coverage targets
70%. CDT keeps its existing `src/main.rs` ignore because the crate has a binary
entry point, and also ignores `src/lib.rs` like Delaunay because the crate root
is primarily module wiring and public re-exports rather than behavior-bearing
implementation code.

The follow-up benchmark alignment introduced a CDT-specific
`ci_performance_suite` rather than copying Delaunay's dimensional construction
contract. CDT's suite covers generated open-boundary and toroidal
triangulations, validation, individual ergodic move attempts, ten-sweep
random-move workloads, and ten-sweep Metropolis runs. The suite uses a local
`perf` profile and feeds the existing performance-analysis scripts, while the
larger Delaunay profiling and same-machine comparison helpers remain deferred.

## July 2026 Tool And Dependency Refresh

The July 2026 refresh compares the shared command, dependency, and workflow surfaces against both `delaunay` and `la-stack`. The newest available release wins
when the sibling repositories differ: `la-stack` supplies the newer `rumdl`, `uv`, and Python-tool pins, while `delaunay` supplies the newer workflow bootstrap
and Dependabot review/merge sequence. Registry and Homebrew metadata confirm the selected managed CLI versions.

- The `justfile` is the single source of truth for cargo-audit, cargo-llvm-cov, cargo-machete, cargo-nextest, dprint, git-cliff, just, rumdl, sarif tooling,
  Taplo, typos-cli, uv, and zizmor. Workflows bootstrap `just` through the repository-local composite action, then resolve every remaining tool version with
  `just --evaluate` instead of duplicating workflow `env` pins. Multi-version exports validate every lookup before writing any step outputs, preventing failed
  or empty evaluations from being masked by successful `echo` commands.
- The dependency-audit workflow runs a human-readable `cargo audit` as its blocking verdict. A separate `if: always()` step writes the JSON artifact without
  suppressing that command's exit status, and the upload step still runs after either audit command reports a known advisory. Neither audit step may use
  `continue-on-error` or shell-level failure suppression, because that would let an audit invocation appear successful when the lockfile is vulnerable.
- Shared Rust dependencies move to their current stable releases, including `delaunay` 0.8.0, Clap 4.6.5, rand 0.10.2, serde 1.0.229, serde_json 1.0.151,
  thiserror 2.0.19, env_logger 0.11.11, and log 0.4.33. The `delaunay` upgrade requires Rust 1.97.1, so `Cargo.toml`, `rust-toolchain.toml`, and current MSRV
  documentation move together; historical release notes remain unchanged.
- The benchmark compile gate uses Cargo 1.97's `CARGO_BUILD_WARNINGS=deny` setting instead of `RUSTFLAGS='-D warnings'`. This keeps warnings fatal for local
  benchmark targets without changing compiler flags and invalidating otherwise reusable Cargo build artifacts; Clippy and rustdoc retain their dedicated
  warning flags because they enforce separate lint and documentation surfaces.
- Shared Python tooling aligns with `la-stack` at pytest 9.1.1, Ruff 0.16.1, Semgrep 1.172.0, and Ty 0.0.65. Notebook and build dependencies take the newer
  applicable sibling minimums without independently changing this repository's Python support baseline; the 6 August follow-up below raises that baseline.
- Semgrep 1.172.0 still pins the MCP Python SDK to vulnerable version 1.23.3. Until Semgrep publishes its pending 1.28.1 dependency bump, uv overrides that
  transitive pin to MCP 1.28.1, matching Semgrep's upstream remediation and clearing all three affected transport and task-handler advisories. Remove the
  override when a released Semgrep version carries the fixed pin directly.
- The stronger `delaunay` Dependabot workflow explicitly waits for a CodeRabbit approval on the triggering head SHA and for all required checks before merging
  that exact approved head. Live ruleset alignment uses its one-approval, resolved-thread, and CodeRabbit-app binding policy while preserving this repository's
  deletion/non-fast-forward rules, bypass actor, merge methods, strict checking, and complete required-check set.

The 31 July Delaunay follow-up is also portable to CDT. Shared workflows use setup-uv 9.0.0 with cache pruning, CodeQL Action 4.37.4, and zizmor-action 0.6.1,
while Dependabot adds grouped uv updates. CodeRabbit keeps review progress disabled and uses the legacy commit-status surface proven by Delaunay's unattended
Dependabot merges. The live Actions policy mirrors Delaunay's selected-action allowlist because it covers
every third-party action used by CDT while keeping GitHub-owned actions available. Delaunay's main-branch review rules remain the policy target, including
binding `CodeRabbit` to the CodeRabbit GitHub App, but issue #216 deliberately applies them only after the checked-in Dependabot automation reaches `main`;
until then, CDT preserves its existing live ruleset.

## Issue #216 Dependabot Review And Auto-Merge

Issue #216 adds repository-specific Dependabot automation without allowing GitHub Actions to approve pull requests. This keeps CodeRabbit as the independent
reviewer required by the `main` ruleset instead of copying automation patterns that self-approve dependency updates.

- CodeRabbit's failing review state is promoted to a failing commit status so a skipped, failed, or change-requesting review cannot satisfy
  the protected-branch gate.
- GitHub Actions updates are grouped like the existing Cargo updates, reducing review and workflow noise while preserving the separate ecosystem labels.
- A `pull_request` workflow runs only for Dependabot-authored pull requests in this repository, requests CodeRabbit through an owner-scoped fine-grained PAT,
  waits for CodeRabbit to approve the triggering head SHA, waits for every required check, and squash-merges only that exact approved head.
- CodeRabbit's submitted `APPROVED` review satisfies the ruleset's single required approval; GitHub Actions does not approve the pull request, and no maintainer
  approval is required for a qualifying grouped Dependabot update.
- Review requests are deduplicated only by an exact head-SHA marker authored by `acgetchell`, so a synchronized Dependabot branch receives a fresh review while
  overlapping runs for the same pull request are canceled.
- The workflow checks out no pull-request content and invokes no external actions. Its explicit `contents: write` and `pull-requests: write` permissions are
  limited to observing the protected-branch gates and performing the guarded squash merge; the owner PAT is used only for the review-request comment.
- Routine GitHub Actions, Cargo, and uv version updates are grouped and staggered at 03:00, 04:00, and 05:00 America/Los_Angeles time each Thursday so this
  repository does not contend with the sibling repositories' CodeRabbit and CI update windows.
- Each ecosystem keeps wildcard version and security groups separate because Dependabot otherwise defaults groups to version updates and opens individual
  security-update pull requests.

The `CODERABBIT_REVIEW_TOKEN` Dependabot secret and the post-merge `main` ruleset update remain live repository settings rather than checked-in configuration.
Actions review approval stays disabled; the ruleset must continue to preserve strict status checks, existing bypasses and required checks, and the CodeRabbit
requirement while adding one required approval and review-thread resolution.

## August 2026 Dependency And Rust Refresh

The August refresh verified the published dependency boundary against crates.io and the upstream release notes. `delaunay` 0.8.0 remains the newest release,
while `markov-chain-monte-carlo` moves from 0.4.0 to 0.4.1. The MCMC release raises its MSRV to Rust 1.97.1 and makes `Step<Info>` telemetry fields private, so
the CDT adapter now reads proposal metadata, outcomes, and cached log probabilities through the upstream invariant-preserving accessors.

`Cargo.toml`, `rust-toolchain.toml`, contributor documentation, and the installed validation toolchain remain synchronized on Rust 1.97.1. The repository keeps
using Rust 1.97.0's Cargo-owned `build.warnings` control for warning-denying benchmark compilation and uses the already-stable, diagnostic-friendly
`assert_matches!` API consistently throughout tests and public examples. The new Rust 1.97 integer bit-isolation and bit-width APIs were audited, but CDT has
no manual highest/lowest-bit scans to simplify; adding a contrived use would not improve the numerical or topology code. Rust 1.97.1 itself is a compiler
correctness point release fixing an LLVM miscompilation rather than a source-language feature release.

Benchmark fixture failures now use one shared postfix `OrAbort` trait with harness-local typed `SetupOperation` enums. Keeping only the genuinely shared
behavior in the support module avoids compiling one union enum whose harness-specific variants are dead in the other benchmark binary. Both Criterion
harnesses retain fail-fast setup semantics while avoiding nested prefix helpers and duplicated unwrap behavior.

The command-layer pins were also refreshed from installed tools and package-manager metadata: uv 0.12.1, cargo-nextest 0.9.143, rumdl 0.2.51, and zizmor
1.29.0. The `justfile` remains the single source of truth for these versions, and GitHub Actions continues to resolve them with `just --evaluate` rather than
carrying duplicate workflow literals. Local Homebrew uv and Cargo-installed cargo-nextest and zizmor were already current; the Cargo-installed rumdl binary
was upgraded from 0.2.50 to 0.2.51 before reconciling the repository pin.

Issue #223 completes the uv alignment by making actionlint reuse the shared exact-version guard instead of checking only for an executable named `uv`. The
notebook JSON boundary now also requires every cell `metadata` field to be an object and parses `nbformat` as an actual JSON integer equal to 4, so values such
as `4.0` and booleans cannot enter the trusted notebook model through Python's numeric equality and subclass behavior. Notebook discovery is captured before
linting so `find` failures remain fatal while a successful empty discovery still reports that no notebooks were found.

The 6 August follow-up reconciles the command layer with the installed Homebrew uv 0.12.2 release, raises the repository's Python minimum to 3.14, and refreshes
the repository-managed Python environment from live uv package metadata. `.python-version`, package metadata and classifiers, Ruff, Ty, CI's shared interpreter
file, script-facing documentation, and the uv lock all encode the same 3.14 baseline. The direct dependency updates are Packaging 26.3, Ty 0.0.69, and Polars
1.43.2; the remaining direct development and notebook requirements were already current. The Semgrep 1.172.0 override remains on MCP 1.28.1 because Semgrep
still has not published a compatible dependency update, while MCP 2.0.0 is a separate incompatible major release rather than a safe lockfile refresh. The
`justfile` remains the single uv version source consumed by workflows. Ruff's 3.14 target also normalizes deferred annotations in support scripts and tests.
Semgrep 1.172.0 cannot yet parse Python 3.14's optional parenthesis-free multi-exception handlers, so those handlers retain the valid parenthesized spelling
with narrow `# fmt: skip` pragmas; this keeps strict Semgrep scans complete without lowering the declared Python baseline or excluding source files. A same-day
Cargo tool refresh raises the repository's rumdl pin from 0.2.51 to the installed 0.2.52 release. CI continues to consume that single `justfile` variable
through the shared version resolver, so no workflow-local rumdl literal changes.

The 7 August follow-up advances the command-layer uv pin from 0.12.2 to 0.12.3 after the installed Homebrew version and Homebrew stable formula metadata moved
to 0.12.3. The `justfile` remains the single source of truth, and CI, performance, and Semgrep workflows continue to resolve it dynamically, so no duplicated
workflow literals or project dependency-lock changes are required.

The 12 August follow-up checks every exact command-layer and Python-tool pin against live crates.io, PyPI, and Homebrew metadata. Rumdl advances from 0.2.52
to 0.2.55, Ruff from 0.16.1 to 0.16.2, and Ty from 0.0.69 to 0.0.71; all other exact Cargo and Python tool pins and uv 0.12.3 remain current. The complete uv
lock is refreshed rather than limiting resolution to those exact requirements, allowing compatible direct and transitive packages such as JupyterLab 4.6.3
to advance together. Semgrep 1.172.0 still declares `mcp==1.23.3`, so the security override remains at compatible MCP 1.28.1 instead of crossing to MCP 2.0.0.
GitHub Actions continue to consume the single `justfile` rumdl pin through `just --evaluate`; no workflow-local rumdl literal needs updating. The independent
action-release audit advances `astral-sh/setup-uv` from 9.0.0 to 10.0.0 at its immutable release commit, while every other pinned action remains current.

## Issue #205 Orthogonal CI And Notebook Checker Alignment

Issue #205 compares CDT's validation shape with the completed `markov-chain-monte-carlo` issue #95 implementation while preserving CDT's stronger notebook
execution policy. The shared pieces are the flat GitHub-equivalent CI union, a release-profile nextest bucket for runnable Rust tests, focused changed-surface
recipes, and a reusable notebook parser/linter. Delaunay's current notebook bucket remains intentionally lint-only, whereas CDT continues to execute the
quickstart and visualization notebooks in routine CI and reserves the analysis-cache notebook for `notebook-check-slow`.

- `just ci` names each distinct validator directly instead of reaching them through `check`, `lint`, or `test-all`. This keeps Markdown, configuration,
  Python, notebooks, Rust linting, runnable Rust tests, doctests, benchmarks, and examples independently selectable and makes target-class overlap explicit.
- `test-rust-ci` runs library unit tests and integration-test crates together with
  `cargo nextest run --release --profile ci --lib --tests --verbose`; `test-rust` adds the separate rustdoc bucket because nextest does not execute doctests.
  `test-unit`, `test-doc`, and `test-integration` remain debug-profile-focused recipes for changed-surface work, with `test-lib` retained as a compatibility
  alias. `.config/nextest.toml` defines the named CI profile with no retries, non-fail-fast execution, immediate final failure output, and a bounded slow-test
  timeout so the command behaves consistently on every platform.
- The fast `clippy` recipe remains scoped to production library and binary targets, while `just ci` uses `clippy-all-targets` to match
  `.github/workflows/rust-clippy.yml`. PR #234 demonstrated that compiling tests, examples, and benchmarks in their focused buckets does not execute Clippy
  lints such as `clippy::ref_option` or `clippy::items_after_statements`. The all-target CI sweep is therefore distinct lint evidence, not redundant compile
  evidence, and prevents GitHub Advanced Security SARIF findings that cannot be reproduced by the documented local CI command.
- `scripts/notebook_check.py` now owns notebook discovery, JSON parsing, cell compilation, output hygiene, extracted-code Ruff/ty diagnostics, and multi-file
  failure reporting. `notebook-output-check` calls the same checker with external-code checks disabled instead of maintaining a second `jq` implementation.
  Just continues to own launch behavior, the fast/slow execution sets, output placement under `target/notebooks`, and explicit source-output cleanup.
- No workflow or tool-version changes are needed: the existing platform matrix already delegates to `just ci`, and the pinned cargo-nextest/uv environment
  provides every command used by the standardized buckets. The new nextest file supplies repository-owned profile policy rather than changing a tool pin.

## Issue #222 Deterministic Allocation Contract

Issue #222 treats cached-observable allocation counts as deterministic correctness assertions rather than noisy timing measurements. The canonical
`allocation-check` recipe executes those assertions through the `perf` profile; `just ci` runs it as part of the before-push contract, `just bench-ci` pairs it
with the stable Criterion suite, and the performance workflow invokes the same recipe before baseline analysis. Allocation failures are blocking on every path,
while statistically noisy Criterion regressions retain their existing report-only pull-request behavior.

## Issue #249 Python Release Helper Hardening

Issue #249 ports the fail-closed release and fixture-validation patterns from `la-stack` while preserving CDT's destination-local, directory-fsynced changelog
publication design. Tag creation now parses `Cargo.toml` directly and rejects a requested tag/package-version mismatch before any Git tag lookup or mutation.
Changelog headings accept strict SemVer 2.0.0 only, reject duplicate `Unreleased` and release sections, render the complete output set before publication, and
expose one injectable replace boundary for deterministic publication and rollback tests. CDT retains its stronger recovery-copy behavior when a rollback itself
fails and reports the publication plus rollback errors together.

The new `release-metadata-check` recipe extends the existing citation gate. It requires one top-level ISO `date-released` value, matches that value
to the generated UTC date on the current Cargo package-version changelog heading when present, and verifies that the `causal-triangulations-scripts` package
ships `scripts/README.md` rather than the Rust crate README. `CITATION.cff` keeps the Zenodo concept DOI at top level and records the v0.1.0 record DOI under
`identifiers`, preserving the CFF distinction between all-version and per-version identifiers.

The Semgrep fixture harness now validates the JSON container and each `check_id`, `start.line`, and `end.line`, then matches annotations by rule ID and source
line within the reported span. Mismatches go to stderr. Shared subprocess wrappers use a finite five-minute default timeout, while existing benchmark paths
retain explicit longer limits, and project-root discovery accepts an explicit file or directory start.

## Deferred Updates

These were evaluated but not ported in this pass:

- `codacy.yml`: defer until the repository has an intentional Codacy project token and a decision about whether Codacy should upload repository-owned
  OpenGrep/Semgrep findings in addition to `.github/workflows/semgrep-sarif.yml`.
- Delaunay's hot-path `FastHashMap`/`FastHashSet` rule: defer because CDT does not currently define equivalent hash aliases or the same `src/core` hot-path
  layout.
- Delaunay's `profiling_suite` and same-machine baseline recipes: defer because CDT's `ci_performance_suite` now provides the portable regression contract,
  while deeper profiling still needs CDT-specific benchmark interpretation.
