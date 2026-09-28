# shellcheck disable=SC2148
# Justfile for causal-triangulations development workflow
# Install just: https://github.com/casey/just
# Usage: just <command> or just --list

# Use bash with strict error handling for all recipes
set positional-arguments

_tools := "uv run --locked --group dev research-repo-tools"
_run := _tools + " toolchain run --"
_notebooks := "uv run --locked --group dev --group notebooks research-repo-tools"

set shell := ["bash", "-euo", "pipefail", "-c"]


# Common cargo-llvm-cov arguments for all coverage runs.
# Excludes benches/examples from reports while allowing integration tests to
# exercise library code.
_coverage_base_args := '''--ignore-filename-regex '(^|/)(benches|examples)/' \
  --workspace --lib --tests \
  --verbose'''

# Internal helpers: ensure external tooling is installed
_ensure-jq:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v jq >/dev/null || { echo "❌ 'jq' not found. See 'just setup' or install: brew install jq"; exit 1; }

# GitHub Actions workflow validation
action-lint:
    # actionlint 1.7.12 predates $/ syntax (upstream issue #711); ignore only this exact valid reference.
    {{ _tools }} files run --include '.github/workflows/*.yml' --include '.github/workflows/*.yaml' -- actionlint -ignore '^specifying action "\$/\.github/actions/setup-toolchain" in invalid format because ref is missing\.'

# Benchmarks
allocation-check:
    {{ _run }} cargo bench --profile perf --bench allocation_profile

bench:
    {{ _run }} cargo bench --workspace

bench-ci: allocation-check
    {{ _run }} cargo bench --profile perf --bench ci_performance_suite

# Compare existing current Criterion output with a retained named baseline.
bench-compare baseline="cdt-v2-last":
    {{ _tools }} performance compare target/criterion target/criterion --baseline-sample "$1" --format markdown

# Compile benchmarks without running them, treating warnings as errors.
# This catches bench/release-profile-only warnings (e.g. debug_assertions-gated unused vars)
# that won't show up in normal debug-profile `cargo test` / `cargo clippy` runs.
bench-compile:
    CARGO_BUILD_WARNINGS=deny {{ _run }} cargo bench --workspace --no-run

# Run the correctness gate before producing release-signal measurements.
benchmark-input-check:
    {{ _run }} cargo test --locked --release --test integration_tests --test physics_integration

# Produce the correctness-gated current release signal.
bench-latest: benchmark-input-check
    {{ _run }} cargo bench --locked --profile perf --bench ci_performance_suite -- --noplot

# Produce the current release signal and compare it with the conventional local baseline.
bench-latest-vs-last: bench-latest
    {{ _tools }} performance compare target/criterion target/criterion --baseline-sample cdt-v2-last --format markdown

# Produce a named native Criterion baseline after validating benchmark inputs.
bench-save-baseline tag: benchmark-input-check
    {{ _run }} cargo bench --locked --profile perf --bench ci_performance_suite -- --save-baseline "$1" --noplot

# Refresh the conventional local comparison baseline.
bench-save-last: benchmark-input-check
    {{ _run }} cargo bench --locked --profile perf --bench ci_performance_suite -- --save-baseline cdt-v2-last --noplot

# Smoke-test benchmark harnesses with minimal samples; not for performance data.
bench-smoke:
    {{ _run }} cargo bench --workspace --bench cdt_benchmarks -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot
    {{ _run }} cargo bench --workspace --bench ci_performance_suite -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot

# Compile benchmarks and release-profile Rust test binaries without running.
bench-test-compile: bench-compile
    {{ _run }} cargo nextest run --tests --release --no-run

# Build commands
build:
    {{ _run }} cargo build

build-release:
    {{ _run }} cargo build --release

# Changelog management (git-cliff + post-processing + archiving + rumdl formatting)
changelog:
    {{ _run }} research-repo-tools changelog generate

changelog-tag version:
    just tag "$1"

changelog-unreleased tag date:
    {{ _run }} research-repo-tools changelog generate --tag "$1" --date "$2"

changelog-update: changelog
    @echo "📝 Changelog updated successfully!"
    @echo "To create a git tag with changelog content for a specific version, run:"
    @echo "  just tag <version>  # e.g., just tag v0.4.2"

# Check (non-mutating): run all linters/validators
check: lint
    @echo "✅ Checks complete!"

# Fast compile check (no binary produced)
check-fast:
    {{ _run }} cargo check

# CI simulation: flat union of GitHub-equivalent focused validators.
# All Cargo targets receive the same Clippy coverage as the SARIF workflow;
# runnable Rust tests and rustdoc doctests remain separate execution evidence.
ci: action-lint zizmor markdown-check spell-check validate-json toml-fmt-check toml-lint yaml-fmt-check yaml-lint citation-check python-check python-fixtures-check test-python notebook-check shell-check semgrep semgrep-test fmt-check clippy-all-targets doc-check test-rust-ci test-doc bench-compile allocation-check examples-validate
    @echo "🎯 CI checks complete!"

# CI with performance baseline
ci-baseline tag="ci":
    just ci
    just bench-save-baseline "$1"

# CI + feature-gated slow/stress tests.
ci-slow: ci test-slow
    @echo "✅ CI + slow tests passed!"

# Validate release-version/date synchronization, concept DOI, and environment metadata.
release-metadata-check:
    {{ _tools }} release check

# Require the generated current-version changelog heading for final release publication.
release-version-check:
    {{ _tools }} release check --final-release

# Validate CITATION.cff against the Citation File Format schema and release metadata gate.
citation-check: release-metadata-check
    uvx --from cffconvert==2.0.0 cffconvert --validate -i CITATION.cff

# Clean build artifacts
clean:
    {{ _run }} cargo clean
    rm -rf target/llvm-cov
    rm -rf coverage_report
    rm -rf coverage

# Fast production Rust linting used by `just check`.
clippy:
    {{ _run }} cargo clippy --workspace --all-features --lib --bins -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::cargo

# Full Cargo-target Clippy sweep used by `just ci` and the GitHub SARIF workflow.
clippy-all-targets:
    {{ _run }} cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::cargo

# Pre-commit workflow: comprehensive validation (checks + tests + release + benches)
commit-check: check test-all test-release bench-compile
    @echo "🚀 Ready to commit! All checks passed."

# Coverage analysis for local development (HTML output)
coverage:
    mkdir -p target/llvm-cov
    {{ _run }} cargo llvm-cov {{ _coverage_base_args }} --html --output-dir target/llvm-cov
    @echo "📊 Coverage report generated: target/llvm-cov/html/index.html"

# Coverage analysis for CI (Cobertura XML output for codecov/codacy)
coverage-ci:
    mkdir -p coverage
    {{ _run }} cargo llvm-cov {{ _coverage_base_args }} --cobertura --output-path coverage/cobertura.xml

coverage-report *args:
    {{ _tools }} coverage report "$@"

debug-large-scale-1p1 vertices="512" timeslices="16" sweeps="10" max_secs="1800" seed="0xCD710139":
    CDT_LARGE_DEBUG_VERTICES_1P1={{ vertices }} CDT_LARGE_DEBUG_TIMESLICES_1P1={{ timeslices }} CDT_LARGE_DEBUG_SWEEPS_1P1={{ sweeps }} CDT_LARGE_DEBUG_SEED_1P1={{ seed }} CDT_LARGE_DEBUG_MAX_RUNTIME_SECS={{ max_secs }} {{ _run }} cargo nextest run --cargo-profile perf --features slow-tests --test large_scale_debug debug_large_scale_1p1 -- --exact --nocapture

debug-large-scale-1p1-1024 max_secs="1800":
    just debug-large-scale-1p1 1024 32 1 {{ max_secs }}

debug-large-scale-1p1-512 max_secs="1800":
    just debug-large-scale-1p1 512 16 10 {{ max_secs }}

# Default recipe shows available commands
[default]
[private]
default:
    @just --list

doc-check:
    RUSTDOCFLAGS='-D warnings' {{ _run }} cargo doc --workspace --no-deps --document-private-items

# Examples and validation
examples: examples-validate

examples-validate:
    {{ _run }} cargo build --locked --release --examples --target-dir target
    {{ _run }} research-repo-tools validation run tooling/examples.toml

# Fix (mutating): apply formatters/auto-fixes
fix: toml-fix fmt python-fix shell-fix markdown-fix yaml-fix
    @echo "✅ Fixes applied!"

fmt:
    {{ _run }} cargo fmt --all

fmt-check:
    {{ _run }} cargo fmt --all -- --check

# Help workflows
help-workflows:
    @echo "Common Just workflows:"
    @echo "  just check             # Run all non-mutating lints/validators"
    @echo "  just check-fast        # Fast compile check (cargo check)"
    @echo "  just ci                # Full CI run (checks + all tests + examples + bench compile)"
    @echo "  just ci-baseline       # CI + save performance baseline"
    @echo "  just ci-slow           # CI + feature-gated slow/stress tests"
    @echo "  just commit-check      # Comprehensive pre-commit validation"
    @echo "  just fix               # Apply formatters/auto-fixes (mutating)"
    @echo ""
    @echo "Testing:"
    @echo "  just coverage          # Generate coverage report (HTML)"
    @echo "  just coverage-ci       # Generate coverage for CI (XML)"
    @echo "  just examples          # Run all example scripts"
    @echo "  just examples-validate # Run examples and validate stable output markers"
    @echo "  just test              # Focused unit and doctest buckets"
    @echo "  just test-all          # Broad Rust and Python tooling tests"
    @echo "  just test-cli          # CLI integration tests only"
    @echo "  just test-examples     # Compile all examples as tests"
    @echo "  just test-integration  # Integration tests (tests/)"
    @echo "  just test-python       # Python tests only (pytest)"
    @echo "  just test-release      # All tests in release mode"
    @echo "  just test-rust         # Broad release Rust tests plus doctests"
    @echo "  just test-rust-ci      # Release unit and integration tests in one nextest pass"
    @echo "  just test-slow         # Feature-gated slow integration tests"
    @echo "  just test-unit         # Focused library unit tests"
    @echo ""
    @echo "Quality Check Groups:"
    @echo "  just lint          # All linting (code + docs + config)"
    @echo "  just lint-code     # Code linting (Rust, Python, Shell)"
    @echo "  just lint-config   # Configuration validation (JSON, TOML, Actions)"
    @echo "  just lint-docs     # Documentation linting (Markdown, Spelling)"
    @echo ""
    @echo "Benchmark System:"
    @echo "  just bench              # Run all benchmarks"
    @echo "  just allocation-check   # Run deterministic allocation assertions"
    @echo "  just bench-ci           # Run allocation assertions and CI regression benchmarks"
    @echo "  just bench-compare      # Compare current Criterion output with a named baseline"
    @echo "  just bench-compile      # Compile benchmarks without running"
    @echo "  just benchmark-input-check # Validate release benchmark inputs"
    @echo "  just bench-latest       # Produce the correctness-gated release signal"
    @echo "  just bench-latest-vs-last # Compare with the fresh 'cdt-v2-last' baseline"
    @echo "  just bench-save-baseline # Save a named native Criterion baseline"
    @echo "  just bench-save-last    # Refresh the 'cdt-v2-last' baseline"
    @echo "  just bench-smoke        # Smoke-test benchmark harnesses with minimal samples"
    @echo "  just bench-test-compile # Compile benches + release integration tests without running"
    @echo "  just debug-large-scale-1p1 # Run one toroidal 1+1 CDT debug case"
    @echo "  just perf-large-scale-debug # Run curated large-scale CDT debug cases"
    @echo ""
    @echo "Performance Analysis:"
    @echo "  just performance-doc # Render reports from retained shared evidence"
    @echo "  just performance-github-assets # Compare GitHub Release-native Criterion assets"
    @echo "  just performance-readme CONFIG # Update the owned README summary from retained evidence"
    @echo "  just performance-release # Measure, retain, reload, and publish release evidence"
    @echo ""
    @echo "Changelog:"
    @echo "  just changelog                   # Generate/update CHANGELOG.md"
    @echo "  just changelog-unreleased TAG DATE # Generate release changelog without a local tag"
    @echo "  just tag <ver>                   # Create git tag with changelog content"
    @echo ""
    @echo "Static Analysis:"
    @echo "  just citation-check      # Validate CFF schema and synchronized release metadata"
    @echo "  just publish-check       # Validate crates.io metadata and dry-run publish"
    @echo "  just release-metadata-check # Validate release versions, dates, and citation policy"
    @echo "  just review [base]       # Optional CodeRabbit branch review"
    @echo "  just review-uncommitted  # Optional CodeRabbit working-tree review"
    @echo "  just semgrep             # Run repository-owned Semgrep rules"
    @echo "  just semgrep-test        # Test repository-owned Semgrep rules"
    @echo "  just unused-deps         # Check for unused direct Cargo dependencies"
    @echo "  just zizmor              # GitHub Actions security analysis"
    @echo ""
    @echo "Running:"
    @echo "  just notebook         # Launch the quickstart notebook with uv-managed dependencies"
    @echo "  just notebook-lint    # Validate JSON, output hygiene, and native notebook Python"
    @echo "  just notebook-check   # Lint all notebooks and execute the fast notebook set"
    @echo "  just notebook-check-slow # Include the explicitly configured heavy notebook"
    @echo "  just notebook-clear-outputs     # Clear outputs from the quickstart notebook"
    @echo "  just notebook-clear-outputs-all # Clear outputs from every notebook"
    @echo "  just notebook-execute # Execute the quickstart notebook headlessly for CI/HPC"
    @echo "  just notebook-setup   # Install the uv notebook dependency group"
    @echo "  just run -- <args>  # Run with custom arguments"
    @echo "  just run-example    # Run with example arguments"
    @echo "  just run-simulation # Run basic_simulation.sh example script"
    @echo ""
    @echo "Note: Some recipes require external tools. Run 'just setup' for full environment setup."

# All linting: code + documentation + configuration
lint: lint-code lint-docs lint-config

# Code linting: Rust (fmt-check, clippy, docs, Semgrep) + Python (ruff, ty) + Shell scripts
lint-code: fmt-check clippy doc-check semgrep semgrep-test python-check shell-lint

# Configuration validation: JSON, TOML, YAML/CFF, GitHub Actions workflows
lint-config: validate-json toml-check yaml-check citation-check action-lint zizmor

# Documentation linting: Markdown + spell checking
lint-docs: markdown-check spell-check

markdown-check:
    {{ _run }} research-repo-tools files run --include '*.md' --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- rumdl check
    {{ _tools }} files run --include '*.md' --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- research-repo-tools docs check-lines

# Markdown and YAML: apply auto-fixes (mutating)
markdown-fix:
    {{ _run }} research-repo-tools files run --include '*.md' --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- rumdl check --fix

markdown-lint: markdown-check

notebook notebook="notebooks/00_quickstart.ipynb":
    #!/usr/bin/env bash
    set -euo pipefail
    notebook_cache="$(pwd)/target/notebooks"
    mkdir -p "$notebook_cache/.ipython" "$notebook_cache/.matplotlib"
    MPLBACKEND=Agg IPYTHONDIR="$notebook_cache/.ipython" MPLCONFIGDIR="$notebook_cache/.matplotlib" {{ _run }} uv run --locked --group notebooks jupyter lab --ServerApp.open_browser=True --LabApp.open_browser=True "{{ notebook }}"

notebook-check: notebook-lint notebook-execute-fast
    @echo "📓 Notebook checks complete!"

notebook-check-slow: notebook-check notebook-execute-slow
    @echo "📓 Slow notebook checks complete!"

notebook-clear-outputs notebook="notebooks/00_quickstart.ipynb":
    {{ _notebooks }} notebooks clear "$1"

notebook-clear-outputs-all:
    {{ _notebooks }} files run --include 'notebooks/*.ipynb' -- research-repo-tools notebooks clear

notebook-execute notebook="notebooks/00_quickstart.ipynb" output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} notebooks execute --output-dir "$2" "$1"

notebook-execute-all output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} files run --include 'notebooks/*.ipynb' -- research-repo-tools notebooks execute --output-dir "$1"

notebook-execute-fast output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} notebooks execute --output-dir "$1" notebooks/00_quickstart.ipynb notebooks/01_spacetime_visualization.ipynb

notebook-execute-slow output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} notebooks execute --timeout 1800 --output-dir "$1" notebooks/02_analysis_caches.ipynb

notebook-lint:
    {{ _notebooks }} files run --include '*.ipynb' --exclude 'tests/semgrep/**' -- research-repo-tools notebooks lint

notebook-output-check:
    {{ _notebooks }} files run --include '*.ipynb' --exclude 'tests/semgrep/**' -- research-repo-tools notebooks check

notebook-setup:
    uv run --locked --managed-python --only-group tooling research-repo-tools notebooks sync

# Run a shared performance command against explicit fresh evidence.
performance *args:
    {{ _run }} research-repo-tools performance "$@"

# Package a fresh tagged baseline in the shared format.
performance-baseline tag:
    {{ _run }} research-repo-tools performance baseline tooling/benchmark.toml "$1" "causal-triangulations-$1-cdt-baseline-v2.tar.gz"

# Compare two explicit new-format release assets without measuring.
performance-github-assets current baseline:
    {{ _tools }} performance assets "$1" "$2" --repository acgetchell/causal-triangulations --asset-template 'causal-triangulations-{tag}-cdt-baseline-v2.tar.gz' --payload target/bench-reports/assets.comparison.json --manifest target/bench-reports/assets.evidence.json --report target/bench-reports/assets.md

# Measure and retain a comparison; explicit tags avoid selecting discarded history.
# This creates temporary Git worktrees and is a user-invoked release operation.
performance-release current baseline:
    {{ _run }} research-repo-tools performance measure tooling/benchmark.toml "$1" "$2" --allow-git-mutations --payload target/bench-reports/release.comparison.json --manifest target/bench-reports/release.evidence.json --report target/bench-reports/release.md
    {{ _tools }} performance promote tooling/performance-report.toml --payload target/bench-reports/release.comparison.json --manifest target/bench-reports/release.evidence.json

# Promote supplied new evidence, or rerender the retained report offline.
performance-doc *args:
    {{ _tools }} performance promote tooling/performance-report.toml "$@"

# Publish absolute timing columns and relative changes from independently pinned evidence.
performance-readme configuration *args:
    {{ _tools }} performance publish "$1" "${@:2}"

perf-large-scale-debug max_secs="1800":
    just debug-large-scale-1p1-512 {{ max_secs }}
    just debug-large-scale-1p1-1024 {{ max_secs }}

publish-check:
    {{ _run }} research-repo-tools validation cargo-metadata
    {{ _run }} cargo publish --locked --allow-dirty --dry-run

python-check: python-source-check python-fixtures-check

# Python code quality
python-fix:
    {{ _tools }} files run --include '*.py' --include '*.pyi' -- ruff check --fix --no-force-exclude
    {{ _tools }} files run --include '*.py' --include '*.pyi' -- ruff format --no-force-exclude

python-lint: python-check

python-sync:
    uv sync --locked --group dev

python-typecheck:
    {{ _tools }} files run --include '*.py' --include '*.pyi' -- ty check --no-force-exclude --error all

# Running the binary
run *args:
    {{ _run }} cargo run --bin cdt "$@"

run-example:
    {{ _run }} cargo run --bin cdt -- -v 32 -t 3

run-release *args:
    {{ _run }} cargo run --release --bin cdt "$@"

# Run example simulation script
run-simulation:
    {{ _run }} ./examples/scripts/basic_simulation.sh

# Repository-owned Semgrep rules for project-specific diagnostics.
semgrep:
    mkdir -p target/security
    rm -f target/security/semgrep.sarif
    {{ _tools }} files run --include '*.rs' --include '*.py' --include '*.yml' --include '*.yaml' --include '*.md' --include '*.sh' --include '*.ipynb' --exclude 'tests/semgrep/**' --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- semgrep scan --config semgrep.yaml --metrics off --disable-version-check --strict --disable-nosem --no-rewrite-rule-ids --no-git-ignore --max-target-bytes 0 --timeout 30 --error --sarif --output target/security/semgrep.sarif

semgrep-test:
    {{ _tools }} semgrep check-fixtures

# cspell:ignore oldname newname

# Development setup
setup:
    uv run --locked --managed-python --only-group tooling research-repo-tools setup

# Install and verify the exact declared development tools.
setup-tools:
    uv run --locked --only-group tooling --inexact research-repo-tools toolchain sync

# Shell scripts: lint/check (non-mutating)
shell-check:
    {{ _tools }} files run --include '*.sh' -- shellcheck -x
    {{ _tools }} files run --include '*.sh' -- shfmt -d

shell-fix: shell-fmt

# Shell scripts: format (mutating)
shell-fmt:
    {{ _tools }} files run --include '*.sh' -- shfmt -w

shell-lint: shell-check

# Spell check (typos)
spell-check:
    {{ _run }} research-repo-tools files run --exclude typos.toml --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- typos --config typos.toml --force-exclude

tag version:
    {{ _tools }} changelog tag "$1"

tag-force version:
    {{ _tools }} changelog tag "$1" --force

# Focused local Rust buckets: unit tests plus rustdoc doctests.
test: test-unit test-doc

# Broad Rust correctness plus Python tooling tests.
test-all: test-rust test-python
    @echo "✅ All tests passed!"

test-cli:
    {{ _run }} cargo nextest run --test cli --verbose

# Doctests must stay on cargo test; nextest does not run rustdoc doctests.
test-doc:
    {{ _run }} cargo test --doc --verbose

test-examples:
    {{ _run }} cargo nextest run --examples --verbose --no-tests pass

test-integration:
    {{ _run }} cargo nextest run --tests --verbose

# Backward-compatible alias for the former recipe name.
test-lib: test-unit

# Broad Rust test workflow; doctests remain a separate cargo-test bucket.
test-rust: test-rust-ci test-doc
    @echo "✅ Rust tests passed!"

# Broad release-profile Rust CI bucket: lib unit and integration tests together.
test-rust-ci:
    {{ _run }} cargo nextest run --release --profile ci --lib --tests --verbose

# Focused library unit tests for changed-surface validation.
test-unit:
    {{ _run }} cargo nextest run --lib --verbose

test-python:
    {{ _run }} uv run --locked python -m pytest

test-release:
    {{ _run }} cargo nextest run --release --workspace
    {{ _run }} cargo test --doc --release

test-slow:
    CDT_LARGE_DEBUG_MAX_RUNTIME_SECS=1800 {{ _run }} cargo nextest run --cargo-profile perf --tests --features slow-tests --verbose

toml-check: toml-fmt-check toml-lint

toml-fix: toml-fmt

toml-fmt:
    {{ _run }} research-repo-tools files run --include '*.toml' -- taplo fmt

toml-fmt-check:
    {{ _run }} research-repo-tools files run --include '*.toml' -- taplo fmt --check

toml-lint:
    {{ _run }} research-repo-tools files run --include '*.toml' -- taplo lint

unused-deps:
    {{ _run }} cargo machete

# Update dependency requirements, locks, managed Cargo tools, and the active uv pin.
update: update-tools update-dependencies

# Advance Cargo dependency declarations and lockfile entries.
update-cargo-dependencies:
    if [ -f Cargo.toml ]; then uv run --locked --only-group tooling --inexact research-repo-tools toolchain run -- cargo upgrade --incompatible allow; fi
    if [ -f Cargo.toml ]; then uv run --locked --only-group tooling --inexact research-repo-tools toolchain run -- cargo update; fi

# Update locally installed Cargo CLI tools and reconcile their pins plus the active uv version.
update-cargo-tools:
    uv run --locked --only-group tooling --inexact research-repo-tools toolchain upgrade

# Advance Cargo and exact Python development requirements plus their lockfiles.
update-dependencies: update-cargo-dependencies update-python-dependencies

# Resolve latest exact Python development tools, retain ranged requirements, and sync.
update-python-dependencies:
    uv run --locked --only-group tooling --inexact research-repo-tools deps update-python
    uv lock --upgrade
    uv run --locked --no-sync --no-python-downloads research-repo-tools toolchain run -- uv sync --locked --managed-python --group dev

# Synchronize deterministic release metadata from one stable GitHub tag.
update-version version *args:
    {{ _tools }} release update "$@"

# File validation
validate-json: _ensure-jq
    {{ _tools }} files run --include '*.json' -- jq empty

yaml-check: yaml-fmt-check yaml-lint

yaml-fix:
    {{ _run }} research-repo-tools files run --include '*.yml' --include '*.yaml' --include CITATION.cff -- dprint fmt --incremental=false

yaml-fmt-check:
    {{ _run }} research-repo-tools files run --include '*.yml' --include '*.yaml' --include CITATION.cff -- dprint check --incremental=false

yaml-lint:
    {{ _run }} research-repo-tools files run --include '*.yml' --include '*.yaml' --include CITATION.cff -- yamllint --strict -c .yamllint

zizmor *args:
    {{ _tools }} zizmor check "$@"


# Check all consumer Python outside the deliberate negative fixtures.
python-source-check:
    {{ _tools }} toolchain python-check
    {{ _tools }} files run --include '*.py' --include '*.pyi' --exclude 'tests/semgrep/**' -- ruff check --no-fix --no-force-exclude
    {{ _tools }} files run --include '*.py' --include '*.pyi' --exclude 'tests/semgrep/**' -- ruff format --check --no-force-exclude
    {{ _tools }} files run --include '*.py' --include '*.pyi' --exclude 'tests/semgrep/**' -- ty check --no-force-exclude --error all

# Apply the complete configured Python policy to negative fixtures as well.
python-fixtures-check:
    {{ _tools }} files run --include 'tests/semgrep/**/*.py' -- ruff check --no-fix --no-force-exclude
    {{ _tools }} files run --include 'tests/semgrep/**/*.py' -- ruff format --check --no-force-exclude
    {{ _tools }} files run --include 'tests/semgrep/**/*.py' -- ty check --no-force-exclude --error all

# Review branch and local changes against verified origin/main, or an explicit local base.
review base="origin/main":
    {{ _tools }} review branch --base="$1"

# Review staged, unstaged, and nonignored untracked changes; requires explicit maintainer intent.
review-uncommitted:
    {{ _tools }} review uncommitted

# Preview a generated changelog without changing retained history.
changelog-preview *args:
    {{ _run }} research-repo-tools changelog generate --dry-run "$@"

# Rotate completed minor versions into the shared archive layout.
changelog-archive:
    {{ _tools }} changelog archive

# Check existing release history and archives.
changelog-check:
    {{ _tools }} changelog check

# Print release notes from the active changelog or archives.
release-notes tag:
    {{ _tools }} changelog notes "$1"

alias changelog-release := changelog-unreleased
alias update-python-deps := update-python-dependencies

# Verify declared tools without installing or changing versions.
tools-check:
    uv run --locked --no-sync --no-python-downloads research-repo-tools toolchain check

# Export verified managed paths for subsequent workflow steps.
tools-export:
    uv run --locked --no-sync --no-python-downloads research-repo-tools toolchain export

# Upgrade uv and managed tools before dependency changes.
update-tools: update-uv update-cargo-tools setup

# Upgrade uv with its installation owner and reconcile the pin.
update-uv:
    uv run --no-config --no-sync --no-python-downloads research-repo-tools deps update-uv
