# Tooling Alignment

## October 2026 MCMC 0.5.2 And Rust 1.99.1

Adopt the latest published, non-yanked `markov-chain-monte-carlo` 0.5.2 release, verified against Cargo's sparse registry index on 24 October 2026.
Its published metadata requires Rust 1.99.1, matching the sibling MCMC checkout. Update the dependency requirement and targeted lock entry, and align
`Cargo.toml`, `rust-toolchain.toml`, Clippy's MSRV and active user/contributor/agent guidance with that baseline. Preserve the existing staged lockfile
updates and the completed Python test-layout migration. CI already reads the declared toolchain, so it needs no workflow-local version edit.

The upstream release fixes subnormal binning midpoint precision and adopts deliberate fused arithmetic in adaptive warmup and normal quantiles. CDT uses
the sampler backend through its existing adapter; retain the serde feature and geometry/MCMC ownership boundaries. Validate the published dependency on
Rust 1.99.1 with the complete native CI gate, including seeded physics, checkpoint continuation, allocation, notebook and example contracts. This raises
CDT's minimum compiler version; historical changelogs and prior performance evidence remain unchanged.

## Issue #313 Python Tooling Test Layout

Move CDT's consumer integration tests from `tooling/tests/` to `tests/tooling/`, matching the checked-out MCMC and Delaunay test layouts. Keep `tooling/`
for consumer configuration and publication contracts; generic infrastructure tests remain owned by `research-repo-tools`.

Update pytest discovery, Ruff source roots and precise test exceptions, CodeRabbit guidance, Semgrep path filters and the repository inventory together.
The directory depth is unchanged, so both test modules retain their existing repository-root calculation. Preserve assertions, fixtures, release/security
boundaries and command behavior; the scanner regression now probes the new path. Shared Python discovery and Just/CI commands already select the complete
inventory, and Ty and Rust coverage have no old-path selection to migrate. Compare collected test identities before and after the move and run the Python,
Semgrep fixture/production, configuration and documentation validators.

Validation exposed an existing local downgrade to Semgrep 1.136.0, whose telemetry imports fail with the patched OpenTelemetry family and whose lock omits
MCP. Follow MCMC's unified Python 3.15 setup: restore its Semgrep 1.182.1 pin in the root development group, retain the existing exact MCP/OpenTelemetry
and Windows-wheel overrides, and refresh the root lock/environment. Keep the root scanner launcher and Linux stack guard; no separate scanner project,
interpreter or lockfile is needed.

## October 2026 Scientific Correctness And Regression Preservation

Align `AGENTS.md` with Delaunay's explicit scientific-correctness rule: passing tests alone do not establish validity, and legitimate regression failures
require production corrections that preserve the original inputs and intended behavior. Place the rule at the start of Core Rules so it governs repair
decisions before validation procedures are selected.

Reject weakened assertions, easier fixtures, reduced coverage, skipped tests, or accepted failures used only to make checks pass. A test-contract correction
requires independent evidence that the mathematics, fixture, or expectation is wrong, an explanation of that evidence, and meaningful retained coverage.
Typed errors and rollback remain safeguards; they do not replace supported successful behavior. This alignment changes agent guidance only.

## October 2026 Unified Python 3.15 Semgrep Environment

Follow Delaunay's root Python 3.15 environment for Semgrep 1.182.1. Its Windows dependency override selects `pywin32` 312, which supplies Python 3.15
wheels and removes the blocker that motivated CDT's separate Python 3.14 project. Move the scanner pin and patched MCP/OpenTelemetry dependencies into
the root manifest and lock, retain wheel-only Semgrep installation, and remove the independent scanner manifest, lock and interpreter selector.

Replace nested uv execution with one root development-environment invocation, retaining explicit `--no-active` selection for scanner commands. Port
Delaunay's Linux-only inherited stack limit for Semgrep's Python 3.15 `RLIM_INFINITY` handling
([semgrep#11979](https://github.com/semgrep/semgrep/issues/11979)); apply it to native scans, rule fixtures and the consumer test suite that invokes Semgrep.
Keep rule IDs, file selection, strict failure handling, aggregate JSON/SARIF reports and workflow upload gates intact.

Update the OSV inventory, declared Python update projects and Dependabot file inventories to match the single maintained Python lockfile. Dependabot keeps
its existing schedules while upstream Python 3.15 support is pending; do not introduce a second dependency definition for it. Consumer tests must verify
root interpreter/executable ownership, patched resolved dependencies and real scanner findings. Validate Python consumers, native scans/fixtures, notebooks,
security scans and the affected configuration/documentation gates. The independent-environment records below describe the superseded setup.

## Git Authorization Alignment With la-stack

Align `AGENTS.md` with la-stack's Git guidance: explicit user authorization permits scoped branch, index, commit, push, and tag operations, including necessary
supporting steps. Authorization persists for the task and requires clarification only when the scope changes. Preserve unrelated user changes, and present
manual commands when authorization is absent. Commit-message guidance likewise defers to an explicitly authorized commit instead of always requiring a
manual handoff. This documentation change grants no authorization to perform Git mutations itself.

## October 2026 Nested Scanner Environment Selection

The outer scanner `uv run` exports `VIRTUAL_ENV=tooling/semgrep/.venv`; the nested root tooling command correctly selects `.venv` but warns that it ignores
the inherited scanner environment. Follow `mcmc-learned-proposals`: explicitly use `--no-active` for both uv calls and `--project .` for the inner root
tooling command. Keep this launcher scoped to `semgrep` and `semgrep-test`; other tooling recipes, interpreter selectors, locks and scanner policy
are unchanged.

The outer call still supplies the isolated Python 3.14 scanner executable on PATH, while the shared CLI runs under the root Python 3.15 environment.
Consumer probes execute the real nested launcher with root, scanner and unrelated inherited environments and verify executable ownership and warning-free
selection. The existing native aggregate scan also rejects the mismatch warning. Validate the complete CI path to cover both production scans and fixtures.

## October 2026 OpenTelemetry Security Override

`just security` finds [GHSA-xcq7-p3pf-p88h](https://github.com/advisories/GHSA-xcq7-p3pf-p88h) in the isolated Semgrep lock: its
`opentelemetry-api` 1.37.0 is affected by unbounded inbound tracestate parsing. The first patched version is 1.45.1. Published Semgrep 1.182.1 still
restricts the API, SDK and HTTP exporter to 1.37, with instrumentation on 0.58b0; an ordinary lock refresh cannot select the fix.

Mirror `mcmc-learned-proposals` and `markov-chain-monte-carlo`: override API, SDK and HTTP exporter to `>=1.45.1,<2`, and requests/threading instrumentation
to `>=0.66b1,<1`. The resolver keeps their shared SDK/API/semantic-convention dependencies consistent. Retain Semgrep 1.182.1, the Python 3.14 environment,
wheel-only scanner installation and patched MCP override. Only the scanner manifest and lock require dependency changes; ordinary updates preserve these
overrides. The consumer contract checks both declarations and the resolved compatible telemetry set. Revalidate native Semgrep scans/fixtures, Python
consumer checks and both security scanners before accepting the override. Remove it when a published Semgrep dependency set supports the patched family.

Native validation on `aarch64-apple-darwin` resolves API/SDK/exporter 1.45.1 and instrumentation 0.66b1, retains Semgrep 1.182.1/MCP 1.30.0, and imports
Semgrep telemetry successfully. `just security` reports zero findings for all three lockfiles, full Git history and the working tree. All 68 consumer tests,
native Semgrep scans/fixtures, strict Python, TOML and documentation checks pass; both Python locks match their manifests. Logs are under
`target/issue-309-security-*`. Rust sources and runtime policies are unchanged, so this focused follow-up does not repeat the earlier full CI gate.

## Issue #309 Captured Benchmark Release Identity

The installed registry package `research-repo-tools==0.2.3` already supplies the release-target handoff introduced by upstream #94 in v0.1.9.
The existing workflow validates a draft before measurement but recaptures its identity during upload, leaving tag movement and release replacement across
jobs unchecked. Adopt `release-draft --commit` and `release-upload --target` without a dependency upgrade or another measurement policy.

Resolve the full tag commit through GitHub in the checkout-free preflight job and verify it with the published preflight command. Carry the captured JSON
and commit through explicit job outputs. Check out that commit, verify HEAD and the local tag before running checkout-owned tooling, and require the native
baseline's verified provenance to identify the captured commit and release. The writer reconstructs the target from preflight outputs outside the artifact
directory; measured checkout code and transported artifacts cannot supply replacement target/runtime data. Missing or malformed handoffs fail closed.

Retain registry-only writer jobs, exact published pins and cutoff, credential separation, disabled producer caches, existing permissions, non-canceling
concurrency, complete baseline assets, correctness/freshness gates and workload/sample policy. Release publication remains a maintainer operation.
Consumer regressions execute the workflow shell and installed CLI with local GitHub responses, including changed identities and missing handoffs;
they create no release and run no scientific timing workloads. Native workflow/security checks and consumer validation cover this adoption separately
from historical release measurements.

Validation on 23 October 2026 uses published 0.2.3, source `4085125526e21b2855eb7c61a52ee312782ae31a`, with the unchanged registry identities recorded
in the #299 adoption below. Both exact registry CLI help probes and the isolated Python baseline-reader import pass with the workflow's Python selector
and cutoff. A read-only GitHub request confirms the encoded full-tag-ref commit endpoint. On `aarch64-apple-darwin`, all 20 focused handoff cases and
the complete `just ci` gate pass: 68 consumer tests, 768 Rust tests, 333 doctests plus two additional rustdoc tests, notebook execution, allocation budgets,
benchmark compilation and four example contracts. Actionlint, the Actions allowlist, online Zizmor, strict Python checks and documentation checks pass.
Logs and source fingerprints are retained locally under `target/issue-309-*`. Hosted Linux/macOS/Windows results for a future PR head remain separate;
these local checks create no release and establish no fresh scientific timing evidence.

## Issue #299 Published Tooling Adoption

Adopt published `research-repo-tools` 0.2.3 through `shared-python-plan` and `shared-python-update`, including its Python 3.15 root/notebook baseline.
The first preview fails because Semgrep 1.182.1 pins Windows `pywin32` 311, which has no CPython 3.15 wheel. Follow la-stack's independent
`tooling/semgrep` project: retain Python `>=3.14,<3.15`, selector 3.14, exact Semgrep 1.182.1, wheel-only scanner installation, and the patched
`mcp>=1.30.0,<2` override. CDT previously had no independent scanner environment or active override; this explicitly adopts la-stack's security policy.
Keep the shared package in the root, with nested uv execution supplying the scanner executable to the root fixture harness.

Declare the scanner in `deps.python-projects` and delegate Python updates to `deps update-python --all-projects`. Root development pins advance while
scanner requirements, interpreter bounds, and overrides remain intact; both locks and environments refresh. OSV and Dependabot inventories must cover the
new project. Preserve the existing staged Cargo/Python dependency refresh and managed tool pins.

Move the remaining consumer tests and their README from `scripts/` into `tooling/`, updating pytest discovery, Ruff source roots, CodeRabbit instructions,
Semgrep production-path coverage, and architecture/navigation references. Remove the obsolete top-level directory; deliberate Semgrep fixture paths and
runnable `examples/scripts` stay put. Rule IDs, patterns, and positive/closest-compliant fixtures retain their existing contracts.

Issue #304 completes adoption of the available maintenance APIs in the same published package. Use canonical `python check/fix/typecheck` for the complete
Python inventory, including deliberate negative fixtures with their existing narrow exceptions. Keep compatibility names as aliases to the complete check,
and use public `just_inspect` helpers for native recipe inspection. Retain consumer-owned pytest 9.1.1, Ruff 0.17.1, and Ty 0.0.90 pins: the optional shared
`python-tools` extra would downgrade Ruff to 0.16.9 and Ty to 0.0.84. Tool installation and execution remain shared independently of this version decision.

Use shared aggregate `semgrep scan` with 50-file batches, paired JSON/SARIF reports under `target/security/semgrep/`, strict native errors, disabled inline
suppressions, one worker, and the existing 30-second target timeout. Explicitly exclude the five files with no applicable CDT rule: `.codecov.yml`,
`.coderabbit.yml`, `.github/actions/setup-toolchain/action.yml`, `.github/dependabot.yml`, and `semgrep.yaml`. They retain their owning YAML, workflow,
configuration, and fixture checks. A real multi-batch consumer regression verifies complete eligible-input coverage and the final SARIF rule inventory;
the SARIF workflow uploads that same aggregate report with its existing category and failure gates. Do not enable optional Rust-doc extraction.

Replace the regex Actions identity allowlist with shared structured `actions allowlist`, using `tooling/selected-actions.json` and the explicit workflow
directory. The installed parser rejects composite actions with `workflow requires jobs`; retain existing composite setup validation and track that upstream
gap with a matching adoption issue. Preserve every approved exact identity, including the single approved nested Dependabot workflow; no owner-wide or verified
action exemption is enabled. Keep readable-version comments in Semgrep, full-SHA enforcement in Zizmor, and all permissions, credentials, cache, and
privileged workflow policies. Run the allowlist before runtime workloads in `ci` and through `lint-config` in local checks.

Registry-only release jobs move to the same package version, uv pin, explicit Python baseline, and reviewed registry cutoff. They still execute no benchmark
checkout with a write token. No local wheel, sibling source, editable dependency, or temporary registry override belongs in committed configuration.

Trusted main-artifact reuse is applicable to future Cargo validation evidence, but this adoption does not implement a Cargo publisher or claim fresh
scientific measurements from earlier main unit checks. CDT has no signed main package-evidence producer or native crates.io workflow yet (#289).
Keep manual `cargo publish --locked`, Cargo package verification, `ci-slow`, the scientific claim checklist, geometry/CDT invariant gates, and fresh
correctness-gated baseline-v2 measurements. Any later promotion must bind the exact source, run attempt, validator jobs, package/artifact IDs and digests,
signatures, citation/source/harness provenance, and actual registry approval controls. Python wheel/sdist promotion commands do not establish that Cargo
contract. Existing immutable benchmark attachments remain unchanged.

Published identity, verified against [PyPI][rrt-023-pypi] and the [release attestation][rrt-023-attestation] on 22 October 2026:

- Package version 0.2.3, Python `>=3.15`, source commit `4085125526e21b2855eb7c61a52ee312782ae31a`.
- `research_repo_tools-0.2.3-py3-none-any.whl`: SHA-256 `da5f0259027662e7706b7f2b8a38d8e1f1d8a5dce314e2c996177b2947514fc6`.
- `research_repo_tools-0.2.3.tar.gz`: SHA-256 `13700b174656ffad265ecb926dd66013949853b7a39c088ab6d3852aba72df95`.
- Original main-push run `39683808521`, attempt 1; `distributions-1` artifact `12384819432`, ZIP SHA-256
  `bed8d086c4f1693924293c3049c3ede8a6a2fca5cf1a73dbf2e2c353a26aeaee`.
- Promotion run `39691177801`, attempt 1; `validated-evidence-1` artifact `12388710693`, ZIP SHA-256
  `9a7d15463bb8dab2791005b04bfbd696c0b4e5bca98d520ed59d917d84c69e88`.

Registry-only jobs use uv 0.13.5, Python 3.15, and cutoff `2026-10-22T08:29:37Z`. Root and scanner locks retain registry URLs and hashes.
Hosted Linux/macOS/Windows `just ci`, target checks, and SARIF workflows remain required for the final PR head; local macOS evidence cannot establish
their results. Cargo package verification and `ci-slow` remain additional release gates, independent of adoption checks.

Local acceptance on `aarch64-apple-darwin` passes `just tools-check`, both `uv lock --check` commands, and the complete `just ci` gate: 42 consumer tests,
768 Rust unit/integration tests, 333 doctests plus two additional rustdoc tests, strict Clippy/docs, Semgrep/fixtures, notebook hygiene/lint/execution,
benchmark compilation, deterministic allocation budgets, and all four example contracts. This is the #299 acceptance run before the #304 command migration.
The added real-scanner regression proves relocated Python violations are detected and adjacent compliant cases pass; the benchmark contract now checks
the loader's `MeasurementConfig` variant before accessing Criterion-specific fields.

The actual `update-python-dependencies` recipe also passes in a disposable consumer copy using registry packages. It refreshes both locks/environments,
advances compatible dependencies, and preserves the scanner manifest and selector byte-for-byte. The installed environments report Python 3.15.0 with
shared tooling 0.2.3, and Python 3.14.7 with Semgrep 1.182.1/MCP 1.30.0. The adopted project kernel targets the root `.venv` interpreter.
Both registry-only release command help probes pass with the workflow's exact pin, Python selector, and cutoff; no release is created or uploaded.
Local logs are retained under `target/issue-299-*.log`. These adoption checks do not produce or certify new release measurements.

Production maintenance Python remains zero lines. The #304 consumer-test change grows `test_shared_tooling.py` from 503 to 567 lines and collected cases
from 42 to 48; the one-line package docstring is unchanged. Additions protect the real multi-batch report/CI upload boundary, exact configured Action
identities and early failure propagation. Repeated dry-run subprocess calls now use shared Just inspection; no generic parser/aggregation engine is copied.

[rrt-023-pypi]: https://pypi.org/project/research-repo-tools/0.2.3/
[rrt-023-attestation]: https://github.com/acgetchell/research-repo-tools/releases/download/v0.2.3/release-attestation.json

### Shared Tooling Retirement Catalog

The complete upstream 16-row catalog is reconciled below against installed registry version 0.2.3, source
`4085125526e21b2855eb7c61a52ee312782ae31a`. Canonical Python commands and Just inspection first shipped in 0.1.8, Python-baseline inheritance in 0.1.7,
and shared Zizmor in 0.1.6; this consumer adopts them together through 0.2.3. Other rows use the supported 0.2.3 interface. No copied maintenance Python,
local wheel, editable dependency, production Python package, or local parser/renderer remains. Native tools and scientific workloads remain necessary.

| Catalog row | Disposition, command owner, and retained boundary |
| --- | --- |
| 1. Package/bootstrap | Migrated: `pyproject.toml`/`uv.lock` exact registry tooling and notebook extras; `setup` delegates installation. No application Python package needs preservation. |
| 2. Tool installation/version authority | Migrated: `setup`, `tools-check`, managed `toolchain run/export`; Just comes from `rust-just`. Retain Rust/MSRV, Cargo/binary declarations, system prerequisites, independent Semgrep compatibility, and newer consumer Ruff/Ty/pytest pins. |
| 3. Tool/dependency updates | Migrated: shared `deps`, `toolchain upgrade`, and `update-python --all-projects`. Retain scientific Cargo dependency policy and explicit scanner project/override. |
| 4. Python baseline/settings | Migrated: inherited Python 3.15 root/notebook baseline and supported mirrors; retain scanner Python 3.14 until its native wheel requirements permit an upgrade. |
| 5. Python checks/defaults | Migrated: canonical `python check/fix/typecheck`, complete discovery and strict Ty. Retain Ruff selections, typing policy, and exact negative-fixture exceptions; optional older shared tool versions are deliberately not adopted. |
| 6. Changelog/notes | Migrated: shared `changelog` generation, normalization, rotation and notes. Retain repository identity/format declarations and historical archives. |
| 7. Release metadata | Migrated: shared `release` transactions/checks. Retain version/citation/DOI rules and approval boundaries; native Cargo publication remains separately tracked in #289. |
| 8. Notebook infrastructure | Partially migrated: shared kernel sync, hygiene, lint, execution, output clearing. Retain scientific cells, fast/slow choice and timeout policy. Selected-file Lab launch adapter is blocked on upstream #131 → CDT #300. |
| 9. Performance acquisition/evidence | Partially migrated: shared Criterion measurement, provenance, assets and evidence. Retain native harnesses, correctness gates, case inventories and immutable evidence. Generic CLI timer retirement is blocked on #134 → #303. |
| 10. Reports/publication | Partially migrated: shared report promotion and README publication. Retain authored interpretation/selections; strict empty/orphan inventory adapter is blocked on #133 → #302. |
| 11. Discovery/native validators | Partially migrated: shared selection, fixture harness, aggregate scanner and Just inspection. Retain Semgrep rules/fixtures, native format/lint policy and thin Just graph. Cargo example stderr assertions keep the existing adapter until #132 → #301. |
| 12. Workflow security | Partially migrated: shared Zizmor, structured workflow allowlist and privileged Dependabot workflow. Retain exact Action identities, SHA/comments, permissions, triggers, cache/token boundaries and report gates. Composite allowlist extension is blocked on #135 → #305. |
| 13. Code review | Migrated: opt-in shared CodeRabbit commands. Retain `AGENTS.md`, `.coderabbit.yml` and scientific review policy; ordinary checks do not run a review. |
| 14. CI/SARIF/coverage | Migrated: managed exports, shared coverage summary and paired aggregate Semgrep reports. Retain native matrix, Cargo flags, Clippy SARIF pipeline, uploads, category/failure conditions, cache policy and required checks. |
| 15. Process/file infrastructure | Migrated: public process/evidence/publication APIs and Just inspection in consumer tests. No copied production executable discovery, subprocess engine, file transaction or batching implementation remains. |
| 16. Tests/dependencies/guidance | Migrated: `tests/tooling/` exercises installed-package integration and CDT policy; generic infrastructure tests stay upstream. Retain scientific tests/notebooks and native evidence. Top-level `scripts/` and obsolete one-batch SARIF guard are removed. |

Native validators remain configured in `pyproject.toml`, `ty.toml`, `semgrep.yaml`, `rust-toolchain.toml`, `Cargo.toml`, formatter/spelling configuration and
the Just graph. These declarations express consumer policy; shared ownership of invocation does not make them obsolete. The managed inventory includes
Cargo audit/edit/coverage/machete/nextest, Clippy SARIF, dprint, git-cliff, rumdl, SARIF formatting, Taplo, typos, Zizmor, Gitleaks and OSV. Native actionlint,
Ruff, Ty, pytest, Semgrep, Markdown/YAML/TOML/spelling, Rust tests/docs/format/Clippy, shell checks and scientific gates remain exercised.

Each unfinished adapter has an upstream capability issue and a matching CDT adoption issue with a native dependency:

- [Selected notebook launch #131](https://github.com/acgetchell/research-repo-tools/issues/131) →
  [CDT #300](https://github.com/acgetchell/causal-triangulations/issues/300).
- [Cargo example stderr #132](https://github.com/acgetchell/research-repo-tools/issues/132) →
  [CDT #301](https://github.com/acgetchell/causal-triangulations/issues/301).
- [Uninitialized report inventory #133](https://github.com/acgetchell/research-repo-tools/issues/133) →
  [CDT #302](https://github.com/acgetchell/causal-triangulations/issues/302).
- [External-command timing #134](https://github.com/acgetchell/research-repo-tools/issues/134) →
  [CDT #303](https://github.com/acgetchell/causal-triangulations/issues/303).
- [Composite Action references #135](https://github.com/acgetchell/research-repo-tools/issues/135) →
  [CDT #305](https://github.com/acgetchell/causal-triangulations/issues/305).

Retirement requires a published exact package release, preserved behavior, installed-consumer checks and native CI evidence. An upstream merge alone does
not replace the working adapter. Python-tool version inheritance and native/scientific policy are justified retained ownership, not missing shared APIs.

## 6 October 2026 Tooling Review Follow-Up

The changed-file tooling review checks the Rust 1.99 baseline, managed-tool pin propagation, local validation graph, Clippy/Semgrep enforcement, affected
workflow consumers, and active command documentation. `just tools-check` verifies the declared tools, including Rust 1.99.0 and typos-cli 1.51.0;
`uv lock --check --offline` verifies the existing Python resolution. This is a consistency review, not a claim that every installed tool is the latest release.

The local `ci` graph previously ran Python tests and fast notebooks before shell, Semgrep, Rust formatting, Clippy, and documentation checks. Move all static
checks and benchmark compilation before runtime workloads, and include `notebook-lint` early while retaining it on `notebook-check`. Just coalesces that
shared dependency so it runs once in CI; standalone notebook validation still lints before execution. Consumer regressions execute the real graph with
external-command stubs and verify that notebook-lint and Clippy failures stop before any runtime workload.

The `commit-check` graph previously selected release Rust tests through both `test-all` and `test-release`. Nextest inventories confirm that the two
selections contain the same 755 tests across 12 binaries. Make `test-release` depend on `test-rust-ci` and retain its release-profile doctests. Just then
coalesces the runnable Rust suite within `commit-check`, while debug and release doctests remain distinct evidence. A dry-run regression protects both
standalone release validation and pre-commit composition. The pre-commit gate also compiles benchmarks before runtime tests, so a benchmark compilation
failure stops before those workloads.

Both defects belong to CDT's consumer-owned aggregate recipes. The published `research-repo-tools` 0.1.7 template supplies a minimal `ci: check` wrapper and
does not own CDT's Rust test selections or notebook execution inventory, so these changes require no new upstream issue or shared-package update.
Retain the existing warning-denying Clippy groups, project-specific Semgrep rules and fixtures, pinned workflow setup/cache keys, and native platform matrix.
The core-workflow description is also corrected: notebook execution belongs to `ci`, rather than the smaller `check`, `fix`, and `test` cycle.

## 6 October 2026 Rust 1.99 Upgrade

The Rust baseline moves from 1.98.1 to the released 1.99.0 toolchain, following the repository policy that the contributor pin and crate MSRV match.
`Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, and active contributor/user/agent guidance move together. The shared setup action already reads the
toolchain file, keys its cache on that file, and installs the declared components, so no workflow-local version or dependency update is needed.
This raises the minimum compiler version for downstream users without changing the edition or public API.

The evaluation uses the final [Rust 1.99 release notes][rust-199], [release announcement][rust-199-announcement], and [Cargo changelog][cargo-199].
The installed compiler reports LLVM 23.1.1. Relevant improvements and adoption decisions are:

- Cargo's manifest/index parsing improvements, rustdoc's faster trait-implementation filtering, and improved inclusive-range iteration apply through the
  toolchain upgrade. CDT already uses inclusive ranges in seed searches and layered construction; their exhausted endpoints are not inspected.
  Upstream performance figures are not CDT measurements.
- Cargo disables incremental compilation by default when `CI` is set. The release/perf validation buckets are already nonincremental; the existing
  host/target CI matrix remains appropriate. The new `debug` profile currently behaves like `dev`, so changing recipes would bring no immediate benefit.
  Edition-2024 overrides of inherited dependency default features have no application to this single-crate manifest.
- Rust 1.99 checks statically zero-sized chunk/window calls through `unconditional_panic`, and rustdoc warns about unused footnote definitions.
  Existing warning-denying Clippy and documentation recipes pick these checks up without additional lint configuration.
- `String::from_utf8_lossy_owned` and `FromUtf8Error::into_utf8_lossy` help consumers that need owned decoded text. CDT's only lossy UTF-8 calls borrow
  subprocess stdout/stderr in a test failure message; consuming those buffers would not improve that path.
- `VecDeque::retain_back` has no queue-filtering call site: the observable breadth-first search queue uses push/pop operations. Boxed-array iteration,
  `StepBy`'s fused-iterator implementation, and filesystem timestamp setters likewise have no matching repository workflow.
- C variadics, raw-pointer layout access, and raw allocation reconstruction do not fit the crate's safe-Rust architecture. No allocation ownership or
  numerical algorithm changes are justified by these additions.
- Exact `doc(alias)` matches now help rustc suggest methods. Add aliases when a demonstrated caller-facing naming mismatch warrants them;
  the current audit found no existing alias or migration shim needing adjustment.

Compatibility checks cover deprecated integral-module paths, macro/attribute diagnostics, iterator use, all Cargo targets, and documentation examples.
The numerical policy continues to require ordinary IEEE arithmetic and deliberate FMA; compiler optimization changes are checked with the existing
seeded physics, checkpoint, property, and allocation contracts rather than adopting relaxed arithmetic.

The installed Clippy adds [`assert_is_empty`][assert-is-empty] diagnostics and detects additional [`suboptimal_flops`][suboptimal-flops] expressions.
Tests adopt diagnostic equality/inequality assertions for empty integer collections. Error-detail strings use `!is_empty()` with contextual failure messages.
Empty floating-point outputs use `is_empty()` with explicit failure messages showing the actual values, avoiding numerical equality comparisons while
retaining useful diagnostics.
The coordinate builders deliberately use `mul_add` for toroidal perturbations and open-strip interior coordinates, removing their lint exceptions.
A rational-arithmetic regression captures a toroidal sum whose fused result is exactly representable while separate operations round one ULP above it;
the existing open-strip analytical fixture also checks both signs of interior jitter. Toroidal coordinates can change in their last bit relative to the
previous arithmetic, so seed/checkpoint reproducibility is validated for the updated construction path rather than claiming cross-version bit identity.
The squared-deviation accumulation in `slab_triangle_fluctuations` also uses `mul_add`, removing its lint exception. An independent analytical fixture with
volumes `[1, 1, 2]` checks the correctly rounded standard deviation of `sqrt(1/3)`; separate multiplication and addition round one ULP higher in this case.

Validation on 6 October 2026 passes the complete `just ci` gate on `aarch64-apple-darwin`: 755 unit/integration tests, 333 doctests, 34 Python consumer tests,
warning-denying Clippy/docs, notebook execution, benchmark compilation, allocation budgets, and validated examples. Production library/CLI checks also pass
with all features for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`; native Linux/Windows execution remains the hosted CI matrix's responsibility.
The local `target/rust-1.99-results-fma-validation.md` ledger records the exact commands, toolchain, source fingerprint, profiles, instrumentation, and logs.

[rust-199]: https://doc.rust-lang.org/stable/releases.html#version-1990-2026-10-01
[rust-199-announcement]: https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/
[cargo-199]: https://doc.rust-lang.org/cargo/CHANGELOG.html#cargo-199-2026-10-01
[assert-is-empty]: https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#assert_is_empty
[suboptimal-flops]: https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#suboptimal_flops

## October 2026 Semgrep Ownership And Coverage

The installed Ruff policy already rejects missing function return annotations and raw `Exception` raises through ANN and TRY002. Zizmor 1.30.1's regular
persona already requires full action SHA pins, including quoted YAML references that the Semgrep regex misses. Remove those three duplicate Semgrep rules
and their exclusive fixtures and Ruff exceptions; the canonical `check` and `ci` recipes continue to invoke Ruff and Zizmor.

Consolidate environment-gated production diagnostics into the general stdio rule, preserving its test exemptions and covering qualified macros, `print!`,
`eprint!`, and `dbg!`. Remove the old planned-step sampler-state synchronization rule: the current runner keeps CDT telemetry beside the sampler-owned
triangulation, so its former `replace_state()` requirement no longer describes the continuation contract.

Keep CDT's scientific and architecture rules. Strengthen backend import checks for grouped root imports and preserve the approved geometry and Metropolis
adapter scopes. Extend the algebraic floating-point rule to doctest mirrors while retaining ordinary IEEE operations and deliberate FMA as compliant controls.
Allow borrowed dynamic errors inside standard `Error::source` implementations, while retaining the typed fallible-error boundary. Update the subprocess
diagnostic to name `research_repo_tools.process.run_safe_command`, which replaced the removed local wrapper.

Each changed rule retains its existing ID and gains positive and closest-compliant fixtures. Validation covers the rule schema, shared fixture harness, real
repository scan, Python consumer checks, and matching YAML, TOML, Markdown, and spelling checks. The accompanying dependency refresh updates development-tool
pins for uv, Ruff, Semgrep, dprint, and rumdl; workflow wiring does not change.

## Invariant-Preserving Performance Contracts

The repository-wide Rust performance audit extends the existing `allocation_profile` harness with checked edge queries, warm family-cache reuse after
discarded and committed concrete plans, state-dependent reverse-policy inspection, and bounded retained storage for unchanged traces.
The same `allocation-check` recipe remains blocking in local CI and performance workflows; no tool, dependency, workflow, or manifest changes are needed.
The existing cached-observable budget remains intact. New Criterion groups isolate persistent proposal planning and time-slice scaling while preserving
all existing benchmark workloads and identifiers. These local checks do not replace or publish release comparisons.

## Issue #268 Level 4 Adapter Cleanup

Delaunay 0.8.2 supplies the owner workflows requested by upstream #591. CDT delegates visualization and exact persistence to those APIs,
preserving the existing visualization schema and CDT producer field. Standalone backends and MCMC checkpoints share the upstream exact owner envelope
and retain CDT's optional Level 5 cadence. Serialization still requires only serialize-capable payloads.

General Euclidean insertion and cavity deletion now use upstream transactional Level 1–4 methods without strict promotion or redundant realization scans.
Toroidal general edits return a typed unsupported-topology error; local CDT bistellar evolution retains its existing rollback boundaries.
The filtered-Delaunay initial-state constructor explicitly requests upstream flip refinement between removal passes to preserve its Level 5 contract.
That construction-only refinement does not run during general edits or evolved-state restoration.
No dependency, tool, workflow, or manifest change is required: the repository already selects Delaunay 0.8.2 and Rust 1.98.1.

The maintainer explicitly chose to drop older checkpoint compatibility. Checkpoint format version 2 embeds the upstream exact owner envelope and restores
through `TriangulationSnapshot::try_into_triangulation`. The former index-based geometry records, hydration mirror, standalone compatibility reader, and
obsolete fixture and tests are removed. CDT retains metadata, RNG, duration, telemetry, and resume validation; older versions are rejected at the envelope
boundary. No upstream exact-restoration API work remains blocked.

## September 2026 MCMC Tooling Parity Follow-Up

The local MCMC comparison found matching shared-package, Rust, Cargo-tool, Python-tool, OSV Scanner, and Gitleaks pins. CDT already has its additional
shell/YAML validators and unused-dependency check. No version changes or extra cross-compilation targets are needed for this alignment.

Adopt MCMC's OSV and Gitleaks workflows with CDT's existing shared setup action, pinned external actions, read-only repository access, and seven-day report
retention. Both run for main-branch pushes and pull requests, weekly, and on manual dispatch; Gitleaks checks out full history. Add their paths to the existing
Dependabot workflow allowlist. README badges follow MCMC's exact order, including CodeQL and zizmor, whose workflows already exist here.

Add the applicable shared maintenance entry points: `tools-clean`, `shared-python-plan`, `shared-python-update`, `tag-preview`, and `release-verify`.
`performance-check` validates retained v2 reports while accepting the intentionally empty initial inventory; orphaned evidence still fails. It joins local
documentation checks and CI. Keep CDT's explicit two-tag performance commands and draft-only release publisher: MCMC's implicit latest-release measurement
could select retired CDT evidence. Existing setup and dependency-update recipes already cover MCMC's equivalent commands.

Consolidate redundant recipe pairs under `check`, `examples-validate`, `shell-fix`, and `toml-fix`; remove `lint`, `examples`, `shell-fmt`, and `toml-fmt`
after updating active callers. Retain `changelog-release` and `update-python-deps`, which are tested shared compatibility aliases. Delegate `bench-save-last`
to the parameterized baseline recipe and remove the redundant direct fixture-check dependency from `ci`; the fixture check remains part of `python-check`.

## September 2026 Documentation, Discovery, And Security Alignment

Issues #279 and #290 apply the documentation roles used in la-stack, Delaunay, and MCMC: README for choosing and running the project, references for
bibliographic provenance, scientific basis for model contracts and evidence limits, and CONTRIBUTING for validation and PR preparation. CDT retains its
hyphenated scientific-basis path, notebook and binary workflows, open-strip and toroidal 1+1 scope, and separate detailed foliation/move/sampler guides.
La-stack and MCMC are libraries with runnable examples; their entry points do not replace CDT's existing `run` recipe. Delaunay's broader geometry and
dimension support does not imply equivalent CDT support.

MCMC's generated Just help and security wrappers are the command reference. Recipe definitions are sorted, the private explicit default delegates to
`just --list`, and every public recipe has a description. The unused `help-workflows`, `changelog-tag`, `changelog-update`, `markdown-lint`, `python-lint`,
`shell-lint`, and `test-lib` wrappers are removed after checking active callers; distinct checks, fixes, workloads, and shared release/update aliases remain.
Just formatting joins configuration validation. [The command guide](DEVELOPING.md) owns the ongoing documentation and command-placement policy.

The active Markdown filename audit found four procedural guides to rename:

| Previous path | New path | Primary purpose |
| --- | --- | --- |
| `docs/cli-examples.md` | `docs/RUNNING.md` | Run the CLI and consume its outputs |
| `docs/hpc.md` | `docs/RUNNING-ON-HPC.md` | Run notebooks and simulations on clusters |
| `docs/performance-testing.md` | `docs/BENCHMARKING.md` | Measure, compare, and publish benchmark evidence |
| `docs/dev/commands.md` | `docs/dev/DEVELOPING.md` | Execute development and validation workflows |

`docs/RELEASING.md` already follows the task-guide convention. Architecture, foliation, moves, sampler contracts, roadmap, scientific basis, test-coverage
analysis, and the Rust/Python/testing policies retain descriptive lowercase names. Directory indexes retain `README.md`. Historical changelog paths are
unchanged; future generated performance reports retain `docs/performance/v2/performance.md` and their immutable evidence links. No measurements change.

Security adds MCMC's `audit`, `security-secrets`, and `security` recipes through the published shared CLI. Exact Gitleaks 8.30.1 and OSV Scanner 2.6.0 binary
pins use shared setup and verification. CDT scans its two maintained lockfiles (`Cargo.lock` and `uv.lock`); MCMC's separate diagnostic-benchmark lockfile
has no CDT counterpart. These network/full-history scans remain outside `check` and `ci`; the existing hosted cargo-audit, CodeQL, Semgrep, and zizmor
workflows keep their current ownership. Scanner failures and findings remain blocking when the security recipes are invoked.

The shared release-template work in [research-repo-tools#60](https://github.com/acgetchell/research-repo-tools/issues/60) remains open for v0.1.8.
CDT keeps the supported v0.1.7 metadata/changelog interfaces until that release is published; reviewed crates.io publication remains tracked in
[#289](https://github.com/acgetchell/causal-triangulations/issues/289). This cleanup does not invent future shared commands or change publication policy.

### Adoption Acceptance Review

The implementation of #281 and #285 landed in [PR #291](https://github.com/acgetchell/causal-triangulations/pull/291), whose head
`6f874425522881a3f0d118936f3d075e46ee891d` passed the hosted Linux, macOS, and Windows CI matrix. The follow-up local macOS validation installs the locked
PyPI package into a fresh environment, verifies managed executable paths and versions, and runs the consumer tests and full `just ci` gate.

For #285, actual `just review HEAD` and `just review-uncommitted` invocations use a local reviewer stub to verify instruction discovery, argument forwarding,
and success/failure propagation through the installed package. Generic freshness and subprocess policy stays upstream; live CodeRabbit invocation is separate.

Issue #281's follow-up adopts the shared changelog format explicitly. The old archive classified the historical commit beginning "Bump the dependencies
group" as Dependencies; the shared template classifies it as Changed. That content difference caused `conflicting retained release 0.0.1` during generation.
The maintainer approved regenerating history in the new format, so the one-time migration regenerates root and archive through the pinned shared generator.
Subsequent generation must be idempotent, preserve declared release dates, and support prospective minor-series rotation and archived-note lookup.

The issue's original generated `bootstrap.sh` / `bootstrap.ps1` requirement is superseded by the published v0.1.7 setup contract: one
`uv run --locked --managed-python --only-group tooling research-repo-tools setup` command works on all supported platforms. The package explicitly requires
no generated installers and exposes no `toolchain bootstrap` subcommand. CDT uses that supported command in contributor instructions and the shared CI
setup action; duplicating an installer locally would undermine the migration.

Update acceptance covers the actual merged Just recipes and installed PyPI package. Consumer tests execute all update recipes through a local uv stub,
checking scope, order, and failure short-circuiting at every step. Separate live macOS checks use a disposable consumer copy, an official standalone uv
installation, and an isolated Just tool environment:

- `update`, `update-tools`, `update-dependencies`, `update-cargo-dependencies`, `update-python-dependencies`, and `update-python-deps` all pass.
- Tool-only updates advance the isolated uv from 0.12.19 to 0.12.20, reconcile only its manifest pin, and preserve dependency requirements and both locks.
  Managed Cargo/scanner versions are already current and are verified at their managed paths, including cargo-edit 0.13.13.
- Deliberately older disposable requirements advance from clap 4.6.6 to 4.6.7 and pytest 9.0.2 to 9.1.1 through `update-dependencies`.
  Both locks refresh, the shared package stays at 0.1.7, and uv/Cargo/scanner pins remain unchanged. Explicit dev sync installs pytest even with
  `default-groups = []`; managed Rust is present on the synchronization path.
- The real checkout's dependency requirements, locks, uv installation, and managed declarations are unchanged by these update experiments.

CDT has one Cargo resolution root and no consumer exclusions; Git and native compiler/linker prerequisites remain system-owned.
Local `just ci` passes with 30 consumer tests, 742 Rust tests, and 333 doctests, plus notebook, example, and configuration checks. Logs are retained locally
under `target/issue-281-*.log`. Hosted Linux/macOS/Windows results from PR #291 establish the original setup baseline;
new checks require the next hosted CI run.
The CI checkout now includes full history and tags so the consumer changelog regression exercises the actual release boundaries on every matrix platform.

## September 2026 Shared Tooling Adoption

Issues #277, #281, and #285 consolidate maintenance on the published `research-repo-tools` 0.1.7 package, following the MCMC consumer migration.
The shared package owns tool installation and selection, dependency updates, changelog rotation, release metadata, coverage summaries, notebook
infrastructure, performance evidence, Semgrep fixture validation, and opt-in CodeRabbit review. CDT retains its scientific benchmark cases,
allocation checks, example output contracts, notebook workloads, and repository-owned static-analysis rules.

Tool declarations move from Just variables into `pyproject.toml`; uv is pinned in `[tool.uv]`, Rust in `rust-toolchain.toml`, and Just follows the shared
package's `rust-just` dependency. Managed setup verifies exact tools. Tool upgrades precede dependency updates, while dependency-only commands preserve
tool pins. CI uses the same managed commands as local development and retains its native Linux, macOS, and Windows matrix.

Rust moves from 1.98.0 to 1.98.1 to match the current upstream minimum. Delaunay 0.8.2 brings la-stack 0.4.6 through the geometry backend; MCMC moves to
0.5.0. The adapter boundaries remain authoritative: CDT does not add a direct la-stack dependency or duplicate upstream geometry or sampler mechanics.

Python validation discovers every tracked and nonignored Python file, including negative Semgrep fixtures, and applies the full configured Ruff and Ty
policy. Missing annotations and annotation-only runtime imports are blocking; intentional fixture violations have exact file/rule exceptions.
CodeRabbit excludes those deliberate fixtures and its duplicate docstring-percentage gate. Shared review recipes remain opt-in and outside `check`/`ci`.

Dependabot approval adopts the same reusable workflow commit as MCMC, `cbb2ea6dee8866b3f0547bca935aef48fdd71707`, independently of the PyPI package pin.
The trusted base-branch caller allows only CDT's Cargo/uv manifests and locks plus its enumerated Actions files. It passes no personal tokens and executes
no PR code. The shared implementation verifies signed Dependabot commits, the exact current head, and effective branch protections before approving and
enabling native squash auto-merge. Required checks, CodeRabbit status, and resolved threads remain merge gates.
The `pull_request_target` Semgrep exception is restricted to this caller path; a consumer test enforces its single pinned reusable job and file policy.
External-action rules now cover nested reusable-workflow paths and approve only this shared workflow path. GitHub settings activation is separate from
merging the caller; see the Dependabot section in [development commands](DEVELOPING.md#dependabot-approval-and-auto-merge).

This migration updates the setup, command, release, and file-inventory documentation needed to use the resulting tooling. The wider documentation
ownership, navigation, filename, and recipe-discovery cleanup is recorded in the #279/#290 follow-up above.

The owner explicitly chose to discard historical benchmark reports/assets and start fresh instead of maintaining legacy format adapters. All legacy
performance Python modules and their exclusive tests are removed. New comparisons use shared evidence and require explicit baseline/current tags;
old release assets are not selected implicitly. New release assets use a distinct `cdt-baseline-v2` name and a draft-only publisher. Ordinary CI retains
raw benchmark artifacts without claiming a regression comparison before a compatible new baseline exists.

The example shell runner is replaced by shared validation with the existing semantic output markers in `tooling/examples.toml`. A consumer test checks
that every Cargo example has a declared validation entry. Python is now a dependency-only environment, with no build backend or local console scripts.
The `basic_cdt` check uses a fixed POSIX-shell redirection to include its existing stderr log markers in the shared validator's stdout contract;
the other examples run directly. Setup already verifies the shell required by Just on every platform.

Consumer tests cover CDT configuration and command wiring only. Shared CodeRabbit subprocess behavior, notebook parsing, changelog archive parsing,
and standalone table rendering are tested in `research-repo-tools`; their duplicate synthetic tests are removed here. The configured report-to-README
integration retains absolute timing, confidence-bound, and relative-change assertions. The shared notebooks extra owns the kernel and execution dependencies;
CDT declares only its interactive frontend and scientific analysis dependencies alongside that extra.

Release documentation now uses explicit tag/date variables so checks stay offline and examples do not drift. There are no active non-performance
README source links requiring release rewriting; measured report links remain untouched. The configured CFF assertion rejects version-record identifiers
and fixes the concept DOI. Changelog archives move to the package convention, `docs/archives/changelog/`, without changing their contents.

The release-producer cache rule covers the shared setup action as well as direct setup-uv use. Zizmor runs through the declared toolchain in both local
checks and SARIF jobs; the obsolete action-specific scanner-version rule is replaced by a prohibition on bypassing that managed scanner.
Semgrep 1.178.0 declares MCP 1.29.0, so the temporary 1.28.1 override is removed.

Semgrep uses shared portable file selection and fixture validation, with one native batched scan for findings/SARIF. The shared per-file scan rejects
selected configuration files that have no applicable CDT rule, so it does not fit this mixed rule inventory. Native strict/error status remains blocking;
negative fixtures are excluded from the production scan and checked separately. No repository-local scanner implementation is retained.

PR #291's first Linux SARIF job exhausted its 20-minute budget during shared Cargo-tool setup before Semgrep started. This Python-only job now installs
the declared uv version and runs `uv run --locked --managed-python --group dev just semgrep`; the locked shared package supplies Just and file selection.
It does not need the full Rust/Cargo toolchain. The scan, strict failure status, and SARIF upload contract remain unchanged.

The sections below are historical decision records superseded where noted by the September 2026 adoption above.

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
  `prefer-assert-matches-in-doctests` rule keeps public `///` examples on `std::assert_matches` (Rust 1.98.0) so documentation teaches the diagnostic-friendly
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

## 31 August 2026 Rust And Backend Refresh

The end-of-August refresh raises the crate MSRV and contributor toolchain together to Rust 1.98.0, matching the published minimums of the selected backend
releases. The manifest now selects `delaunay` 0.8.1 and `markov-chain-monte-carlo` 0.4.2. `la-stack` remains an implementation dependency of `delaunay`, not a
direct CDT dependency; resolving `delaunay` 0.8.1 updates the lockfile to `la-stack` 0.4.5 without exposing that numerical backend in CDT's manifest or source.

The `delaunay` 0.8.1 boundary separates mutable Level 1–4 `Triangulation` ownership from certified Level 1–5 `DelaunayTriangulation` ownership and unifies
its proof-bearing builders. The CDT geometry adapter absorbs those upstream ownership changes so the domain layer continues to use crate-owned traits and
handles.
The MCMC 0.4.2 boundary re-scores current states before transitions, strengthens delayed-commit rollback and mismatch diagnostics, and corrects discrete
proposal ratios for state-dependent family normalizers. Focused adapter tests and the full repository CI path verify that CDT checkpoint continuation,
proposal accounting, geometry mutation, and validation semantics remain intact across both upgrades.

The same refresh advances the repository uv pin from 0.12.3 to Homebrew's current 0.12.7 and aligns stale Cargo-hosted validator pins with the installed
toolchain. CDT now shares the sibling repositories' `just update` contract: Cargo requirements and locks, exact Python development-tool pins, managed Cargo
CLI installations, the uv lock, and root justfile pins have explicit component recipes plus one aggregate entry point. The Python pin updaters are
transactional so a resolver failure or collateral manifest edit restores the prior `pyproject.toml` and `uv.lock`.

The first `just update` execution advances the direct Rust requirements for Clap 4.6.6, Log 0.4.34, and Thiserror 2.0.20 while retaining Delaunay 0.8.1 and
MCMC 0.4.2 as the latest compatible backend releases. Exact Python tool pins advance to Ruff 0.16.5, Semgrep 1.175.0, shfmt-py 4.1.0, and Ty 0.0.77; Cargo
and uv refresh their compatible transitive locks together. Every managed Cargo CLI already matches its reconciled justfile pin, so the command performs no
local Cargo tool reinstall on this workstation.

Zizmor 1.30 additionally requires GitHub's dedicated `$/...` syntax for repository-local actions. All workflows now use that spelling for the shared
`setup-just` action. Actionlint 1.7.12 does not parse the newer syntax yet, so its recipe ignores only the corresponding “missing ref” diagnostic; YAML,
action-policy, and Zizmor checks continue to validate the same workflow files.

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

## Issue #271 Stable uv Update Preflight

Issue #271 compares CDT's update workflow with la-stack's early stable-uv guard while preserving CDT's existing exact-pin policy for ordinary uv-backed
commands. The aggregate `just update` path validates that the active `uv --version` output contains one stable `X.Y.Z` version before dependency mutation,
and the direct `update-cargo-tools` path runs the same preflight before `cargo install-update`. Unlike the exact `_ensure-uv` guard, this update-only check
accepts a newer stable uv so the atomic tool-pin reconciler can advance `uv_version`. The CDT variant also rejects missing or ambiguous version output in
addition to prerelease and embedded versions, matching the reconciler's single-stable-version contract. Fake-executable regression tests cover the rejected
forms and Just dry-run tests preserve both pre-mutation ordering guarantees.

## Issue #249 Python Release Helper Hardening

Issue #249 ports the fail-closed release and fixture-validation patterns from `la-stack` while preserving CDT's destination-local, directory-fsynced changelog
publication design. Tag creation now parses `Cargo.toml` directly and rejects a requested tag/package-version mismatch before any Git tag lookup or mutation.
Changelog headings accept strict SemVer 2.0.0 only, reject duplicate `Unreleased` and release sections, render the complete output set before publication, and
expose one injectable replace boundary for deterministic publication and rollback tests. CDT retains its stronger recovery-copy behavior when a rollback itself
fails and reports the publication plus rollback errors together.

The new `release-metadata-check` recipe extends the existing citation gate. It requires one top-level ISO `date-released` value, matches that value
to the generated UTC date on the current Cargo package-version changelog heading when present, and verifies that the `causal-triangulations-scripts` package
ships `scripts/README.md` rather than the Rust crate README. The later issue #264 policy below supersedes this pass's version-record `identifiers` choice.

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

## August 2026 Release And Floating-Point Policy Alignment

Issues #264 and #265 compare the repository against `la-stack` while preserving CDT's scientific and publication boundaries.

- The release workflow uses a plan-before-write `update-version` transaction. It accepts one stable `vX.Y.Z` tag, obtains the previous stable version from
  published GitHub releases, excludes drafts and prereleases, synchronizes Rust/Python/CFF metadata and owned active-documentation references, and validates a
  temporary candidate tree before atomic publication. A caught partial write restores the original bytes and newline style.
- `CITATION.cff` now carries only the permanent Zenodo concept DOI. The release checker rejects top-level version-record `identifiers`, requires synchronized
  versions, and provides a stricter final-release gate for the generated changelog date. This intentionally supersedes issue #249's per-version DOI policy.
- The existing `just update` transaction already covers Cargo requirements and locks, exact Python development pins, managed Cargo tools, root tool pins,
  `uv.lock`, and environment synchronization. Release preparation therefore composes with that canonical command instead of duplicating its mutations.
- Issue #184 adds a CDT-specific release-performance path around the existing `ci_performance_suite`. Like the `la-stack` reference, it keeps a fixed CSV
  schema, digest-bound provenance, isolated source states, strict retained-pair reloads, native Criterion GitHub Release assets, render-only documentation
  commands, and rollback-safe tracked publication. CDT deliberately keeps the implementation smaller: the stable suite is already the correctness-valid
  release signal, so this does not import `la-stack`'s broader benchmark registry, project-specific Codacy filtering, or profiling-suite interpretation.
- The release-asset workflow pins and allowlists `actions/download-artifact` alongside the already approved upload action. It is used only for same-run
  transport into a separate `contents: write` publisher job; the durable public contract is the native `.tar.gz` attached to the GitHub Release.
- The existing `performance.yml` and `perf-*` commands remain the PR/main-branch regression and exploratory analyzer surface. The canonical `bench-latest`,
  `bench-compare`, `bench-save-*`, and `performance-*` commands form the independent durable release-evidence surface; neither is an alias for the other.
- Semgrep categorically rejects `f64::algebraic_add`, `algebraic_sub`, `algebraic_mul`, `algebraic_div`, and `algebraic_rem` across production Rust, tests,
  examples, and benchmarks, including receiver, associated, qualified, and function-item forms. Ordinary IEEE operators and deliberate `mul_add` calls
  remain allowed. The positive rule fixture in `tests/semgrep/src/project_rules/algebraic_float.rs` is the sole policy exception. Any future relaxed or
  fast-math facility requires a separate tracked scientific review.
- The release-asset workflow explicitly disables setup-uv caching because its benchmark job produces an artifact consumed by a separate write-privileged
  publisher. This remains explicit even though current setup-uv `auto` behavior excludes release events, keeping older GitHub Advanced Security zizmor audits
  and the checked-in workflow contract aligned.
- The zizmor SARIF workflow pins zizmor-action and the underlying zizmor tool instead of floating to the action's latest default. Authenticated local
  `just zizmor` runs use online audits through an environment or GitHub CLI token, while unauthenticated runs state that they are using the offline subset.
  Repository-owned Semgrep rules enforce the static release-cache precondition and require the workflow scanner version to match the repository's exact
  `1.30.0` pin; zizmor's online audit remains the authority for resolving whether an action SHA matches its human-readable version tag.
- The release workflow comparison covered `delaunay`, `markov-chain-monte-carlo`, and `la-stack`. All three use tag-keyed, non-canceling concurrency and bounded
  release jobs, so CDT adopts those controls; MCMC and la-stack also confirm the separate read-only producer and write-privileged publisher shape. Delaunay
  provides the directly applicable explicit setup-uv cache disablement. The siblings' zizmor workflows still float the scanner version and their local
  recipes remain offline, so CDT deliberately does not copy those two gaps.

## 1 September 2026 Rust 1.98 Final Release Audit

Issue #254 completes the post-publication audit against the final
[Rust 1.98.0 release notes](https://doc.rust-lang.org/stable/releases.html#version-1980-2026-08-20), the
[Rust 1.98 announcement](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/), and the
[Cargo 1.98 changelog](https://doc.rust-lang.org/cargo/CHANGELOG.html#cargo-198-2026-08-20). Rust 1.98.0 shipped on 20 August 2026, so the crate MSRV,
`rust-toolchain.toml`, contributor commands, CI matrix, and user-facing requirements now target a released stable toolchain rather than a beta candidate.
Cargo 1.98 contains fixes but no new feature surface that changes this repository's packaging contract.

The final standard-library audit records these decisions:

- `Path::is_empty`, `Option::map_or_default`, `Result::map_or_default`, `bool::ok_or`, and `bool::ok_or_else` did not appear in the final Rust 1.98 stabilized
  API list. Existing explicit path, fallback, and typed-error code therefore remains unchanged and stable-compatible.
- `NumBuffer` and integer `format_into` shipped, but no measured allocation-sensitive integer-formatting path exists in CDT. Diagnostics and artifact
  formatting keep their clearer owned-string implementation until benchmark evidence supports reusable-buffer complexity.
- The algebraic floating-point operations shipped, but their permitted reassociation and nondeterministic results conflict with reproducible scientific
  computation. The repository-wide Semgrep ban and its sole positive fixture remain the explicit policy; ordinary IEEE operators and deliberate `mul_add`
  calls remain allowed.
- `str::substr_range` and slice `subslice_range` have no manual pointer-range implementation to replace; `NonZero*::from_str_radix` has no invariant-bearing
  nonzero radix boundary; the UTF-16LE/BE string constructors have no matching input format; `strip_circumfix` has no clearer repository call site; and the
  new atomic slice-mutation helpers would add no value because CDT has no safe-global atomic storage boundary.
- `CommandArgs: Send + Sync` has no cross-thread command-builder use, and the legacy range module exists for macro migration rather than ordinary CDT
  ranges. No contrived adoption is warranted.

The compatibility notes were also checked against repository-owned Rust. CDT has no affected edition-2024 trait-object lifetime fallback, ambiguous macro or
glob import, transparent representation with multiple nontrivial fields, unsafe attribute, exported-symbol collision, unsupported target, or relied-upon
const-evaluation diagnostic. The MSRV change therefore requires no compatibility shim or source workaround beyond the already-completed dependency adapter
updates and explicit rejection of nondeterministic float APIs.

The capstone release render also exposed that multiline Rust provenance was encoded with HTML line breaks inside a generated Markdown table, while complete
hashes and toolchain details made the table exceed the repository line-length limit. The report renderer now gives the baseline and current provenance
separate sections, uses compact metadata lists, and preserves multiline toolchain output in fenced text blocks. Its fixture asserts that regenerated reports
contain both sections and the complete provenance without inline HTML. This keeps `performance-release`, render-only recovery, and the repository Markdown
gate consistent from one retained bundle.

## 26 October 2026 Sphere Audit Evidence

Issue #258 adds an opt-in external scientific workload under `docs/audits/2d-cdt_sphere/`. The C++ harness executes the pinned reference kernels; a small
independent Python census checks exported simplices. These domain algorithms belong to CDT, while the existing pinned `research-repo-tools` commands still
own Python discovery, linting, typing, and test execution. No timing runner, publication engine, dependency pin, or CI benchmark job is added.

Pytest discovery includes this audit's known-answer and retained-evidence tests alongside the existing tooling tests. Tests never download, compile, or run
the reference simulator. The audit tests receive the same assertion-only Ruff exception as existing pytest files. Full Python lint/type discovery already
covers the new package; no Python validator exclusion is added. Native regeneration remains an explicit command documented with its configuration and hashes.
Ty receives the audit directory as an additional module root because `2d-cdt_sphere` is a repository identifier, not a Python package identifier. This
resolves the checked `checks` package's relative imports without suppressing diagnostics or changing pytest's runtime import behavior.
The spelling checker excludes only the retained upstream `history.json`: an original commit message misspells the build filename, and correcting that quotation
would corrupt source evidence. Audit prose and all other new files remain checked.
Semgrep's native reports exposed a coverage gap: the existing Python rules only selected tooling/test directories, so the strict shared wrapper rejected
the unscanned audit inputs. The same applicable Python rules now include `docs/audits/**/*.py`, and audit text I/O explicitly uses UTF-8. This extends
the existing policy to the new source owner instead of excluding those files or relaxing report-completeness checks.
