# Development Commands

Development commands and validation steps for the repository.

Agents must run appropriate checks after modifying code.

---

## Core Workflow

Typical development loop:

```bash
just fix
just check
just test
```

These commands ensure:

- formatting
- linting
- static analysis
- tests

## Justfile Usage

This repository standardizes development tasks through the `justfile`.

Agents should **prefer running `just` commands instead of invoking the underlying tools directly**. The justfile ensures the correct flags, configuration, and tool ordering are used.

Examples:

- prefer `just fix` instead of running `cargo fmt` directly
- prefer `just check` instead of running `cargo clippy` directly
- prefer `just ci` instead of manually running multiple validation steps

Direct tool invocation should only be used when a corresponding `just` command does not exist.

Rust unit, integration, CLI, slow, example, and release test recipes run with `cargo nextest`. Documentation tests intentionally remain on `cargo test --doc` because nextest does not run rustdoc doctests.

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

- formatting checks
- lint checks
- repository-owned Semgrep rules
- unit tests
- integration tests
- documentation builds
- validated example runs
- benchmark compilation

For heavier stabilization work, run the slow-test wrapper:

```bash
just ci-slow
```

This runs the normal CI command and then feature-gated slow/stress tests.

## Semgrep

Repository-owned Semgrep rules live in `semgrep.yaml`. They encode focused project invariants that are not already covered by Rust, Clippy, Ruff, or ShellCheck.

When adding or changing a Semgrep rule, add a matching fixture under `tests/semgrep/` and keep `just semgrep-test` passing. Rules ported from `markov-chain-monte-carlo` must be adapted to CDT naming, paths, and architecture constraints rather than copied mechanically.

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

`just examples-validate` additionally checks stable output markers for user-facing Cargo examples. Keep those markers semantic rather than exact numeric values so simulation output can evolve without making the example contract brittle.

The example runner compiles all Cargo examples once with `cargo build --release --examples`, then executes the compiled binaries directly. This preserves example coverage while avoiding repeated Cargo invocations for each example.

When adding or renaming a Cargo example, update `scripts/run_all_examples.sh` `validate_example_output()` with stable semantic output markers, or intentionally document why success-only validation is sufficient for that example.

---

## Spell Checking

Documentation and comments are spell‑checked.

Run:

```bash
just spell-check
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
just toml-lint
just toml-fmt
just toml-fmt-check
```

---

## Markdown Formatting

Markdown files are formatted with dprint.

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
just shell-fmt         # Format (mutating)
```

---

## YAML Validation

YAML files are validated with yamllint and formatted with prettier.

Commands:

```bash
just yaml-lint         # Lint (non-mutating)
just yaml-fix          # Format (mutating)
```

`just yaml-fix` accepts either a globally installed `prettier` or `npx` fallback.

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

The repository has separate workflows for full CI, dependency audit, Codecov coverage, repository-rule SARIF upload, Clippy SARIF, performance checks, and CodeQL analysis. Do not add another external analysis workflow unless it has a distinct signal and required secrets are configured for this repository.

Run with:

```bash
just action-lint
```

---

## Python Validation

Python scripts are linted and type-checked:

```bash
just python-lint       # ruff format + ruff check
just python-fix        # ruff check --fix + ruff format
just python-typecheck  # ty check (blocking)
just test-python       # pytest
```

## Benchmark And Release Hygiene

Benchmark harnesses can be smoke-tested without producing baseline-quality performance data:

```bash
just bench-smoke
```

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
| Format code           | `just fix`               |
| Run lints             | `just check`             |
| Run unit tests        | `just test`              |
| Run integration tests | `just test-integration`  |
| Run slow tests        | `just test-slow`         |
| Run all tests         | `just test-all`          |
| Run Python tests      | `just test-python`       |
| Run examples          | `just examples`          |
| Validate examples     | `just examples-validate` |
| Run full CI           | `just ci`                |
| Pre-commit check      | `just commit-check`      |

---

## Testing by File Type

| Changed files | Command                                |
| ------------- | -------------------------------------- |
| `tests/`      | `just test-integration` (or `just ci`) |
| `examples/`   | `just examples-validate`               |
| `benches/`    | `just bench-compile`                   |
| `src/`        | `just test`                            |
| `scripts/`    | `just test-python`                     |
| Any Rust      | `just doc-check`                       |

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

This runs `git-cliff`, applies the Python postprocessor, and archives completed minor release series under `docs/archive/changelog/`.

For release PRs, generate the changelog for a version before the final tag exists with:

```bash
just changelog-unreleased v0.1.0
```

Create annotated release tags from the generated changelog after the release PR is merged with:

```bash
just tag v0.1.0
```
