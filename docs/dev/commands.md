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

## Justfile Usage

This repository standardizes development tasks through the `justfile`.

Agents should **prefer running `just` commands instead of invoking the underlying tools directly**. The justfile ensures the correct flags, configuration, and
tool ordering are used.

Examples:

- prefer `just fix` instead of running `cargo fmt` directly
- prefer `just check` instead of running `cargo clippy` directly
- prefer `just ci` instead of manually running multiple validation steps

Direct tool invocation should only be used when a corresponding `just` command does not exist.

Rust unit, integration, CLI, slow, example, and release test recipes run with `cargo nextest`. Documentation tests intentionally remain on `cargo test --doc`
because nextest does not run rustdoc doctests.

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
- notebook output hygiene, extracted-code checks, and fast headless execution
- benchmark harness compilation and validated example runs

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
just examples
just examples-validate
```

Examples must:

- compile
- run successfully
- demonstrate correct API usage

`just examples-validate` additionally checks stable output markers for user-facing Cargo examples. Keep those markers semantic rather than exact numeric values
so simulation output can evolve without making the example contract brittle.

The example runner compiles all Cargo examples once with `cargo build --release --examples`, then executes the compiled binaries directly. This preserves
example coverage while avoiding repeated Cargo invocations for each example.

When adding or renaming a Cargo example, update `scripts/run_all_examples.sh` `validate_example_output()` with stable semantic output markers, or intentionally
document why success-only validation is sufficient for that example.

---

## Spell Checking

Documentation and comments are spell‑checked.

Run:

```bash
just spell-check
```

The `typos` binary is provided by the Cargo crate `typos-cli`. `just setup-tools` installs the pinned version. To install it directly while still using the
repository pin, first ensure `just` is installed because this command resolves `typos_version` with `just --evaluate`:

```bash
cargo install --locked typos-cli --version "$(just --evaluate typos_version)"
```

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

Compatibility aliases remain available as granular recipes:
`just toml-lint`, `just toml-fmt-check`, and `just toml-fmt`.

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

`just shell-fmt` remains as a compatibility alias for the formatter.

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

Citation metadata should pass both YAML style linting and CFF schema validation.

Run:

```bash
just citation-check
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

Run with:

```bash
just action-lint
```

GitHub Actions security analysis runs with `zizmor` locally and in `.github/workflows/zizmor.yml`:

```bash
just zizmor
```

---

## Python Validation

Python scripts are linted and type-checked:

```bash
just python-lint       # ruff format + ruff check
just python-fix        # ruff check --fix + ruff format
just python-typecheck  # strict ty check (blocking)
just test-python       # pytest
```

## Notebook Validation

Notebook source files should not commit generated outputs or execution counts. Notebook code is also extracted and checked with Ruff and ty so `.ipynb` cells
follow the same Python standards as repository scripts. Use:

```bash
just notebook-output-check  # Non-mutating output and execution-count hygiene check
just notebook-check         # Output hygiene, extracted-code checks, and fast headless notebook execution
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

The smaller CI regression benchmark contract runs through the `perf` Cargo profile:

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
| Run examples          | `just examples`          |
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

This runs `git-cliff`, applies the Python postprocessor, archives completed minor release series under `docs/archive/changelog/`, and
formats generated changelog files with `rumdl`.

For release PRs, generate the changelog for a version before the final tag exists with:

```bash
just changelog-unreleased v0.1.0
```

Create annotated release tags from the generated changelog after the release PR is merged with:

```bash
just tag v0.1.0
```
