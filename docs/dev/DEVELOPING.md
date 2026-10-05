# Development Commands

Development commands and validation steps for the repository.

Agents must run appropriate checks after modifying code.

---

## Core Workflow

Typical development loop:

```bash
just check
just fix
just test
```

These commands ensure:

- formatting
- linting
- static analysis
- tests
- notebook execution and output hygiene

## Dependency And Tool Maintenance

```bash
just update
```

`just update` updates uv and declared Cargo tools first, runs shared setup, then refreshes Cargo/Python dependencies and lockfiles. Pins live in
`pyproject.toml`. Use `update-tools` or `update-dependencies` for an independent scope, and `tools-check` for non-mutating installed-version verification.
Review every resulting manifest, lockfile, and pin change before committing it.

To adopt a newer published shared package and its Python baseline, preview `just shared-python-plan VERSION`, then apply the reviewed plan with
`just shared-python-update VERSION`. Supply the package version, not a Python version. These commands run outside the old environment.
`just tools-clean` previews unused managed-cache entries; `just tools-clean --apply` removes them.

Release metadata is a separate deterministic transaction:

```bash
TAG=vX.Y.Z
just update-version "$TAG" --date "$RELEASE_DATE" --previous-release "$PREVIOUS_TAG"
```

An explicit previous tag keeps preparation offline; omitting it discovers the latest published stable GitHub release. The updater synchronizes Cargo/CFF
versions and owned active-documentation references; Python remains a dependency-only environment. It uses the declared date, rejects version-specific DOI
identifiers, validates the complete candidate tree
before replacement, and rolls back byte-for-byte on a caught publication failure. It does not update dependencies, generate the changelog, or run benchmarks.

Release performance is a separate evidence transaction:

```bash
just bench-latest
just bench-latest-vs-last
just performance-release vX.Y.Z vA.B.C
just performance-doc
just performance-readme tooling/performance-readme.toml
```

`bench-latest` runs the correctness gate before `ci_performance_suite`. `performance-release` requires two explicit fresh-series tags and measures
in temporary worktrees before retaining and promoting shared evidence. It requires an explicit user request because it mutates Git state.
The first post-migration tag establishes a baseline; comparisons and README tables wait until a second compatible measurement exists.
`performance-doc` and `performance-readme CONFIG` render retained evidence without measurement. See [performance testing](../BENCHMARKING.md).

## Justfile Usage

This repository standardizes development tasks through the `justfile`.
Run bare `just` for the complete public recipe list with arguments and descriptions. The explicit private default runs `just --list`; there is no separate
hand-maintained help list. Recipe definitions stay lexicographically sorted and `just justfile-fmt-check` checks formatting.

Agents should **prefer running `just` commands instead of invoking the underlying tools directly**. The justfile ensures the correct flags, configuration, and
tool ordering are used.

Examples:

- prefer `just fix` instead of running `cargo fmt` directly
- prefer `just check` instead of running `cargo clippy` directly
- prefer `just ci` instead of manually running multiple validation steps

Direct tool invocation should only be used when a corresponding `just` command does not exist.

Rust unit, integration, CLI, slow, example, and release test recipes run with `cargo nextest`. Documentation tests intentionally remain on `cargo test --doc`
because nextest does not run rustdoc doctests.

## Documentation And Command Policy

- README owns project evaluation, early API/model-scope guidance, and runnable Quickstart commands, including bare `just` for discovery.
- CONTRIBUTING owns contributor setup, checks, fixes, tests, security scans, and PR preparation. Dedicated guides own release, dependency-maintenance,
  benchmark, CLI, and cluster procedures; link to them instead of copying full command lists.
- REFERENCES owns bibliographic records, stable citation identifiers, and a topic-to-source index. `docs/scientific-basis.md` owns assumptions, conventions,
  method summaries, and evidence limits. Detailed foliation, move, and sampler contracts retain their own pages.
- Preserve Contents navigation, prerequisites before dependent methods, and meaningful deep links. Sort independent methods lexicographically within coherent
  groups; retain thematic reference ordering. Separate implemented behavior, validated evidence, planned capabilities, and implementation lineage.
- Use uppercase verbs or verb phrases, preferably gerunds, for task guides with execution instructions. Use lowercase descriptive names for policy,
  architecture, invariants, reference, analysis, and results. Preserve directory `README.md` indexes, conventional root names, historical archives, and hyphens.
- Use relative links inside the repository documentation tree. README links reused by package pages use explicit default-branch repository URLs for guides and
  `latest` API URLs for callable contracts. Release updates must not rewrite active navigation to a release tag; immutable evidence and citations keep their
  pins.
- Name equivalent workflows consistently across the research repositories. Checks validate without changing tracked sources, fixes apply changes, and
  run/example commands execute user workloads. Describe every public recipe; keep an explicit private default and sorted definitions. A library need not invent
  a binary.
- Update navigation, architecture inventories, generator inputs, configured output paths, and callers together when paths move. Regenerate owned outputs through
  their workflow; a filename change alone does not require new measurements.

## Dependency And Secret Scanning

```bash
just audit             # OSV: Cargo.lock and uv.lock
just security-secrets  # Gitleaks: all reachable history and current tracked/nonignored files
just security          # Both scans
```

The shared `research-repo-tools security` CLI owns scanning, report validation, and failure propagation. `pyproject.toml` declares exact `osv-scanner` and
`gitleaks` binary versions; `just setup-tools` installs them into the managed cache, and `just tools-check` verifies them. No scanner relies on an ambient
executable silently satisfying a managed pin. OSV scans explicit lockfiles without executing dependency code and needs network access. Secret scanning
requires full history and includes local edits and untracked nonignored files; shallow repositories fail instead of presenting partial history as clean.

Reports live under `target/security/`; secret values are redacted. Findings, execution errors, and absent or malformed reports fail the invoked command.
These explicit scans remain outside ordinary `check`/`ci`. Dedicated OSV and Gitleaks workflows invoke the same recipes on main-branch pushes and pull
requests, weekly, and by manual dispatch. They retain reports for seven days, including on scan failure. Gitleaks uses a full-history checkout.
The hosted cargo-audit, CodeQL, Semgrep, Clippy, and zizmor workflows remain separate.

## Local CodeRabbit Review

`just review [base]` and `just review-uncommitted` delegate to the pinned shared CLI. Install and authenticate CodeRabbit separately; neither recipe installs
it, enables paid credits, fetches Git refs, or retries failed reviews. Agents invoke a live review only on an explicit maintainer request for CodeRabbit.

Branch review defaults to `origin/main`, checks it against the live remote, and includes committed changes, staged/unstaged edits, and nonignored new files.
Missing/stale refs or failed remote lookups stop review. An explicit local base avoids the freshness check; uncommitted review avoids it and excludes committed
branch changes. The shared implementation discovers `AGENTS.md` and `.coderabbit.yml`, streams review output, and preserves interruptions and failure status.
Authentication, service, or allowance failures mean unavailable review, never a clean result. Consumer tests use local stubs without contacting CodeRabbit.

---

## Formatting

Rust formatting:

```bash
cargo fmt
```

Typically run through:

```bash
just fix
```

Formatting must always be applied before committing changes.

---

## Linting

Lint checks include:

```bash
cargo clippy
semgrep
```

Warnings are treated as errors in CI.

Run via:

```bash
just check
```

---

## Documentation Validation

Documentation must build successfully.

Verify with:

```bash
just doc-check
```

---

## Full CI Validation

Before large changes, run the full CI command:

```bash
just ci
```

This runs:

- GitHub Actions, Markdown, JSON, TOML, YAML/CFF, Python, shell, and repository-owned Semgrep checks
- Rust formatting, all-target Clippy, and production documentation builds
- one release-profile nextest pass for library unit tests and integration-test crates
- a separate rustdoc doctest bucket
- notebook output hygiene, native notebook checks, and fast headless execution
- benchmark harness compilation, the deterministic allocation contract, and validated example runs

The `ci` recipe is a flat union of these focused validators. It does not depend on broad `check`, `lint`, or `test-all` bundles. Clippy covers every Cargo
target to match the GitHub SARIF workflow; the test, benchmark, and example buckets still own their runtime or compile-contract evidence because ordinary
compilation does not execute Clippy lints.

For heavier stabilization work, run the slow-test wrapper:

```bash
just ci-slow
```

This runs the normal CI command and then feature-gated slow/stress tests through the repository `perf` Cargo profile. The slow suite includes large-scale
toroidal debug probes, so optimized builds are part of the validation contract rather than an optional speed tweak.

## Fast Validation Cycle

Start with the smallest test selection that covers the change: a filtered library unit test, one rustdoc doctest filter, or one integration-test crate. Use
the focused `test-unit`, `test-doc`, and `test-integration` recipes when the whole changed surface is the useful boundary.

For final validation of non-core changes, compose each applicable bucket once, such as `just markdown-check`, `just python-check test-python`, or
`just notebook-check`. For core Rust changes or an exact local simulation of GitHub validation, run `just ci` directly instead of first running overlapping
broad bundles that `ci` will repeat.

## Semgrep

Repository-owned Semgrep rules live in `semgrep.yaml`. They encode focused project invariants that are not already covered by Rust, Clippy, Ruff, or ShellCheck.

When adding or changing a Semgrep rule, add a matching fixture under `tests/semgrep/` and keep `just semgrep-test` passing. Rules ported from
`markov-chain-monte-carlo` must be adapted to CDT naming, paths, and architecture constraints rather than copied mechanically.

Commands:

```bash
just semgrep       # Run repository-owned rules
just semgrep-test  # Verify Semgrep rule fixtures
```

## Coverage

Coverage is generated with `cargo-llvm-cov`, matching the Codecov workflow.

Commands:

```bash
just coverage     # HTML report at target/llvm-cov/html/index.html
just coverage-ci  # Cobertura XML at coverage/cobertura.xml
```

---

## Examples

Example programs and scripts live in:

```text
examples/
examples/scripts/
```

Validate with:

```bash
just examples-validate
```

Examples must:

- compile
- run successfully
- demonstrate correct API usage

`just examples-validate` checks stable output markers for user-facing Cargo examples. Keep those markers semantic rather than exact numeric values
so simulation output can evolve without making the example contract brittle.

The example runner compiles all Cargo examples once with `cargo build --release --examples`, then executes the compiled binaries directly. This preserves
example coverage while avoiding repeated Cargo invocations for each example.

When adding or renaming a Cargo example, update `tooling/examples.toml` with stable semantic output markers. A consumer integration test checks
that the declared validation inventory covers every Cargo example. Shared validation enforces a 600-second per-example timeout on all platforms.

---

## Spell Checking

Documentation and comments are spell‑checked.

Run:

```bash
just spell-check
```

The `typos` binary is provided by the Cargo crate `typos-cli`. `just setup-tools` installs the exact version declared in `pyproject.toml`; the shared runner
selects it for `just spell-check`.

If a legitimate technical word fails, add it to `typos.toml` under:

```toml
[default.extend-words]
```

---

## TOML Formatting

TOML files should be validated and formatted using Taplo.

Commands:

```bash
just toml-check       # Non-mutating formatting and lint checks
just toml-fix         # Apply formatting fixes
```

Use `just toml-lint` or `just toml-fmt-check` for a single check.

---

## Markdown Formatting

Markdown files are checked and fixed with `rumdl`. The repository uses a
160-column Markdown width, and `just markdown-check` also runs a raw line-length
guard so constructs that `rumdl` exempts from MD013 still respect that limit.

Commands:

```bash
just markdown-check    # Non-mutating check
just markdown-fix      # Apply fixes
```

---

## Shell Script Validation

Shell scripts must pass:

```text
shfmt
shellcheck
```

Commands:

```bash
just shell-check       # Lint (non-mutating)
just shell-fix         # Format (mutating)
```

---

## YAML Validation

YAML and `CITATION.cff` files are checked with `yamllint` and formatted with
`dprint` using the Rust-native `pretty_yaml` plugin.

Commands:

```bash
just yaml-check        # Non-mutating formatting and lint checks
just yaml-fix          # Format (mutating)
```

Compatibility aliases remain available as granular recipes:
`just yaml-lint` and `just yaml-fmt-check`.

---

## CITATION.cff Validation

Citation metadata should pass YAML style linting, CFF schema validation, and the release metadata synchronization gate.

Run the metadata gate independently with:

```bash
just release-metadata-check
```

It requires exactly one top-level ISO `date-released` value, matches it to the generated current-package changelog heading when present, and validates the
Cargo/CFF version set, the permanent Zenodo concept DOI, and active dependency examples. The Python environment is not a releasable package. The broader
citation check includes this gate:

```bash
just citation-check
```

For final publication, require the generated current-version changelog heading as well:

```bash
just release-version-check
```

---

## JSON Validation

JSON files should be validated after edits.

```bash
just validate-json
```

Or directly:

```bash
jq empty file.json
```

---

## GitHub Actions Validation

Workflows must pass `actionlint`.

The repository has separate workflows for full CI, dependency audit, Codecov coverage, repository-rule SARIF upload, Clippy SARIF, performance checks, and
CodeQL analysis. Do not add another external analysis workflow unless it has a distinct signal and required secrets are configured for this repository.

External GitHub Actions must use full commit-SHA pins, stay within the
repository-owned allowlist in `semgrep.yaml`, and keep a readable version
comment next to each pin. Dependabot remains configured for the
`github-actions` ecosystem; its update PRs should preserve both the SHA pin and
the adjacent human-readable version comment.

Zizmor owns full-SHA pin validation. Semgrep owns the CDT action allowlist and readable version-comment policy.

### Dependabot Approval and Auto-Merge

The Dependabot caller delegates to the SHA-pinned `research-repo-tools` approval workflow, independently of the Python package version.
It approves eligible Cargo, uv, and GitHub Actions updates and enables native squash auto-merge. GitHub still requires current checks, the CodeRabbit status,
resolved review threads, and an approval for the current head. The caller executes no PR code and passes no personal tokens.

Keep its exact file allowlists synchronized with Cargo/uv manifests and locks and all workflow/composite-action paths. The consumer test checks this inventory.
The `pull_request_target` exception applies only to this reviewed caller, which contains one shared job and no local execution steps.

Activation requires these separate live settings, in addition to merging the caller into `main`:

- Allow auto-merge and squash merging.
- Allow Actions to create and approve pull requests, retaining read-only default workflow permissions.
- Allow `dependabot/fetch-metadata@*` and `acgetchell/research-repo-tools/.github/workflows/dependabot-approve.yml@*` in selected Actions.
- Enable stale-review dismissal in the active main ruleset, preserving required approvals, resolved threads, strict checks, and existing required check names.

All required live settings were applied and verified on 28 September 2026, preserving read-only defaults and existing required checks and bypasses.
The shared workflow refuses approval without the required branch protections. Verify a new eligible Dependabot event after deployment; rerunning an old
workflow run does not load the new caller. The old `CODERABBIT_REVIEW_TOKEN` is unused after migration and can be removed once the old workflow is retired.
See the [shared approval contract][approval].

[approval]: https://github.com/acgetchell/research-repo-tools/blob/cbb2ea6dee8866b3f0547bca935aef48fdd71707/docs/AUTOMATING_DEPENDABOT.md

### Workflow Validators

Run with:

```bash
just action-lint
```

GitHub Actions security analysis runs with `zizmor` locally and in `.github/workflows/zizmor.yml`:

```bash
just zizmor
```

The local recipe runs zizmor's online audits when `ZIZMOR_GITHUB_TOKEN` or `GH_TOKEN` is set, or when `gh auth token` can provide an authenticated token. It
falls back to an explicit offline scan when no token is available. The SARIF workflow uses the same scanner pin in `pyproject.toml`, so GitHub Advanced
Security and authenticated local runs apply the same audit implementation.

---

## Python Validation

Python scripts are linted and type-checked:

```bash
just python-check      # Full Ruff/format/Ty policy, including negative fixtures
just python-fix        # ruff check --fix + ruff format
just python-typecheck  # strict ty check (blocking)
just test-python       # pytest
```

## Notebook Validation

Notebook source files must have unique stable cell IDs and no generated outputs or execution counts. The shared CLI runs native Ruff and ty against the
notebooks, preserving cell-aware diagnostics and supported IPython syntax. Package-install cells are prohibited. Use:

```bash
just notebook-output-check  # Non-mutating output and execution-count hygiene check
just notebook-check         # Output hygiene, native notebook checks, and fast headless notebook execution
just notebook-check-slow    # Also execute heavier run-debugging notebooks
```

`just ci` includes `notebook-check`, which executes the quickstart and visualization notebooks but only lints heavier analysis notebooks. Headless execution
writes executed notebooks under `target/notebooks/` and leaves the source notebooks unchanged. To clear source notebook outputs before committing, run:

```bash
just notebook-clear-outputs-all
```

## Benchmark And Release Hygiene

Benchmark harnesses can be smoke-tested without producing baseline-quality performance data:

```bash
just bench-smoke
```

The deterministic query, proposal-cache, observable, and trace allocation contracts run in `just ci`.
Check them independently through the `perf` profile:

```bash
just allocation-check
```

The smaller CI benchmark contract runs the allocation check and the stable Criterion regression suite:

```bash
just bench-ci
```

For manual large-scale toroidal 1+1 CDT move-kernel debugging, run the parameterized slow-test harness through the `perf` Cargo profile:

```bash
just debug-large-scale-1p1 512 16 10
```

Named scale probes are also available:

```bash
just debug-large-scale-1p1-512
just debug-large-scale-1p1-1024
```

The arguments are total vertices, timeslices, and sweeps. The umbrella recipe runs the curated large-scale debug case set:

```bash
just perf-large-scale-debug
```

The umbrella recipe currently runs `512/16/10` and `1024/32/1` cases. It is intentionally manual and long-running; use the parameterized
`debug-large-scale-1p1` recipe directly when you only need a smaller smoke probe.

These debug recipes do not enable volume fixing. Volume drift in the reported final vertex and simplex counts is expected when `(1,3)` and `(3,1)` moves are
accepted. Treat these runs as unfixed-volume Metropolis debug sweeps, not as fixed-volume CDT production ensembles. Future fixed-volume recipes should be
explicitly labeled because quadratic volume fixing samples a modified ensemble; see Ambjørn et al.,
[The Semiclassical Limit of Causal Dynamical Triangulations](https://arxiv.org/abs/1102.3929), and
[The phase structure of Causal Dynamical Triangulations with toroidal spatial topology](https://arxiv.org/abs/1802.10434).

For unfixed-volume Metropolis debug runs, each sweep is sized from the current number of top-dimensional simplices at the start of that sweep, then executed as
one checkpoint-preserving Metropolis chunk. The cosmological constant is the volume-control coupling. If a large-scale recipe grows or shrinks too aggressively,
tune the action parameters rather than interpreting the run as a failed fixed-volume simulation.

The recipes set per-case environment variables, and the Rust harness also accepts these variables directly:

| Variable | Description |
| -------- | ----------- |
| `CDT_LARGE_DEBUG_VERTICES` / `CDT_LARGE_DEBUG_VERTICES_1P1` | Total vertex count |
| `CDT_LARGE_DEBUG_TIMESLICES` / `CDT_LARGE_DEBUG_TIMESLICES_1P1` | Number of periodic time slices |
| `CDT_LARGE_DEBUG_SWEEPS` / `CDT_LARGE_DEBUG_SWEEPS_1P1` | Sweep count; one sweep attempts one move per current simplex |
| `CDT_LARGE_DEBUG_SEED` / `CDT_LARGE_DEBUG_SEED_1P1` | RNG seed, decimal or `0x` hexadecimal |
| `CDT_LARGE_DEBUG_MAX_RUNTIME_SECS` | Optional wall-clock cap checked between sweeps; `0` disables it |

To compile benchmarks and release-profile integration tests without running them:

```bash
just bench-test-compile
```

Before release preparation, optional Cargo hygiene checks are available:

```bash
just unused-deps
just publish-check
```

---

## Recommended Command Matrix

| Task                  | Command                  |
| --------------------- | ------------------------ |
| Run lints             | `just check`             |
| Format code           | `just fix`               |
| Run unit tests        | `just test-unit`         |
| Run unit + doctests   | `just test`              |
| Run integration tests | `just test-integration`  |
| Run broad Rust tests  | `just test-rust`         |
| Run slow tests        | `just test-slow`         |
| Run all tests         | `just test-all`          |
| Run Python tests      | `just test-python`       |
| Validate examples     | `just examples-validate` |
| Validate notebooks    | `just notebook-check`    |
| Run full CI           | `just ci`                |
| Pre-commit check      | `just commit-check`      |

---

## Testing by File Type

| Changed files | Command                                |
| ------------- | -------------------------------------- |
| `tests/`      | `just test-integration` (or `just ci`)       |
| `examples/`   | `just examples-validate`                     |
| `benches/`    | `just bench-compile`                         |
| `src/`        | `just test-unit test-doc` (or `just ci`)     |
| `scripts/`    | `just python-check test-python`              |
| `notebooks/`  | `just notebook-check`                        |
| Any Rust      | Also run `just doc-check` with the applicable Rust path command |

---

## CI Expectations

CI enforces:

- formatting
- clippy lints
- documentation build
- tests
- validated examples

All warnings are treated as errors.

Agents must ensure changes pass CI locally before proposing patches.

---

## Changelog

The changelog is **auto-generated**.

Never edit manually.

Regenerate with:

```bash
just changelog
```

The shared CLI uses its bundled `git-cliff` template, applies Markdown hygiene, archives completed minor release series under `docs/archives/changelog/`,
and formats generated changelog files with `rumdl`. `just changelog-preview` validates a preview without changing history.

For release PRs, generate the changelog for a version before the final tag exists with:

```bash
just changelog-unreleased "$TAG" "$RELEASE_DATE"
```

Create annotated release tags from the generated changelog after the release PR is merged with:

```bash
just tag "$TAG"
```
