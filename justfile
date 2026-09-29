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

alias changelog-release := changelog-unreleased
alias update-python-deps := update-python-dependencies

# Internal helpers: ensure external tooling is installed
_ensure-jq:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v jq >/dev/null || { echo "❌ 'jq' not found. See 'just setup' or install: brew install jq"; exit 1; }

# GitHub Actions workflow validation
action-lint:
    # actionlint 1.7.12 predates $/ syntax (upstream issue #711); ignore only this exact valid reference.
    {{ _tools }} files run --include '.github/workflows/*.yml' --include '.github/workflows/*.yaml' -- actionlint -ignore '^specifying action "\$/\.github/actions/setup-toolchain" in invalid format because ref is missing\.'

# Run deterministic cached-observable allocation assertions.
allocation-check:
    {{ _run }} cargo bench --profile perf --bench allocation_profile

# Audit all maintained Rust and Python lockfiles through OSV.
[group('security')]
audit:
    {{ _tools }} security osv uv.lock Cargo.lock

# Run all Criterion benchmark workloads.
bench:
    {{ _run }} cargo bench --workspace

# Run allocation assertions and the CDT regression benchmark suite.
bench-ci: allocation-check
    {{ _run }} cargo bench --profile perf --bench ci_performance_suite

# Compare existing current Criterion output with a retained named baseline.
bench-compare baseline="cdt-v2-last":
    {{ _tools }} performance compare target/criterion target/criterion --baseline-sample "$1" --format markdown

# Compile all benchmark harnesses with warnings denied.
bench-compile:
    CARGO_BUILD_WARNINGS=deny {{ _run }} cargo bench --workspace --no-run

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
bench-save-last: (bench-save-baseline "cdt-v2-last")

# Smoke-test benchmark harnesses with minimal samples; not for performance data.
bench-smoke:
    {{ _run }} cargo bench --workspace --bench cdt_benchmarks -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot
    {{ _run }} cargo bench --workspace --bench ci_performance_suite -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot

# Compile benchmarks and release-profile Rust test binaries without running.
bench-test-compile: bench-compile
    {{ _run }} cargo nextest run --tests --release --no-run

# Run the correctness gate before producing release-signal measurements.
benchmark-input-check:
    {{ _run }} cargo test --locked --release --test integration_tests --test physics_integration

# Build the library and cdt binary.
build:
    {{ _run }} cargo build

# Build the optimized library and cdt binary.
build-release:
    {{ _run }} cargo build --release

# Changelog management (git-cliff + post-processing + archiving + rumdl formatting)
changelog:
    {{ _run }} research-repo-tools changelog generate

# Rotate completed minor versions into the shared archive layout.
changelog-archive:
    {{ _tools }} changelog archive

# Check existing release history and archives.
changelog-check:
    {{ _tools }} changelog check

# Preview a generated changelog without changing retained history.
changelog-preview *args:
    {{ _run }} research-repo-tools changelog generate --dry-run "$@"

# Generate prospective release notes using an explicit tag and date.
changelog-unreleased tag date:
    {{ _run }} research-repo-tools changelog generate --tag "$1" --date "$2"

# Check (non-mutating): run all linters/validators
check: lint-code lint-docs lint-config
    @echo "✅ Checks complete!"

# Fast compile check (no binary produced)
check-fast:
    {{ _run }} cargo check

# Run all GitHub-equivalent validators, tests, examples, and benchmark compilation.
ci: justfile-fmt-check action-lint zizmor markdown-check spell-check performance-check validate-json toml-fmt-check toml-lint yaml-fmt-check yaml-lint citation-check python-check test-python notebook-check shell-check semgrep semgrep-test fmt-check clippy-all-targets doc-check test-rust-ci test-doc bench-compile allocation-check examples-validate
    @echo "🎯 CI checks complete!"

# CI with performance baseline
ci-baseline tag="ci":
    just ci
    just bench-save-baseline "$1"

# CI + feature-gated slow/stress tests.
ci-slow: ci test-slow
    @echo "✅ CI + slow tests passed!"

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

# Summarize an existing Cobertura coverage report.
coverage-report *args:
    {{ _tools }} coverage report "$@"

# Run one bounded toroidal CDT debug case with explicit size and seed.
debug-large-scale-1p1 vertices="512" timeslices="16" sweeps="10" max_secs="1800" seed="0xCD710139":
    CDT_LARGE_DEBUG_VERTICES_1P1={{ vertices }} CDT_LARGE_DEBUG_TIMESLICES_1P1={{ timeslices }} CDT_LARGE_DEBUG_SWEEPS_1P1={{ sweeps }} CDT_LARGE_DEBUG_SEED_1P1={{ seed }} CDT_LARGE_DEBUG_MAX_RUNTIME_SECS={{ max_secs }} {{ _run }} cargo nextest run --cargo-profile perf --features slow-tests --test large_scale_debug debug_large_scale_1p1 -- --exact --nocapture

# Run the 1024-vertex toroidal debug case.
debug-large-scale-1p1-1024 max_secs="1800":
    just debug-large-scale-1p1 1024 32 1 {{ max_secs }}

# Run the 512-vertex toroidal debug case.
debug-large-scale-1p1-512 max_secs="1800":
    just debug-large-scale-1p1 512 16 10 {{ max_secs }}

# Default recipe shows available commands
[default]
[private]
default:
    @just --list

# Build public and private API documentation with warnings denied.
doc-check:
    RUSTDOCFLAGS='-D warnings' {{ _run }} cargo doc --workspace --no-deps --document-private-items

# Build and run Cargo examples, checking their semantic output contracts.
examples-validate:
    {{ _run }} cargo build --locked --release --examples --target-dir target
    {{ _run }} research-repo-tools validation run tooling/examples.toml

# Fix (mutating): apply formatters/auto-fixes
fix: justfile-fmt toml-fix fmt python-fix shell-fix markdown-fix yaml-fix
    @echo "✅ Fixes applied!"

# Format Rust source files.
fmt:
    {{ _run }} cargo fmt --all

# Check Rust formatting without modifying sources.
fmt-check:
    {{ _run }} cargo fmt --all -- --check

# Apply canonical Justfile formatting.
justfile-fmt:
    just --fmt --unstable

# Check Justfile formatting without modifying it.
justfile-fmt-check:
    just --fmt --check --unstable

# Code linting: Rust (fmt-check, clippy, docs, Semgrep) + Python (ruff, ty) + Shell scripts
lint-code: fmt-check clippy doc-check semgrep semgrep-test python-check shell-check

# Configuration validation: JSON, TOML, YAML/CFF, GitHub Actions workflows
lint-config: justfile-fmt-check validate-json toml-check yaml-check citation-check action-lint zizmor

# Check Markdown, spelling, and retained performance reports.
lint-docs: markdown-check spell-check performance-check

# Check Markdown formatting and the 160-column line limit.
markdown-check:
    {{ _run }} research-repo-tools files run --include '*.md' --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- rumdl check
    {{ _tools }} files run --include '*.md' --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- research-repo-tools docs check-lines

# Apply Markdown formatting and lint fixes.
markdown-fix:
    {{ _run }} research-repo-tools files run --include '*.md' --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- rumdl check --fix

# Open a notebook in the managed JupyterLab environment.
notebook notebook="notebooks/00_quickstart.ipynb":
    #!/usr/bin/env bash
    set -euo pipefail
    notebook_cache="$(pwd)/target/notebooks"
    mkdir -p "$notebook_cache/.ipython" "$notebook_cache/.matplotlib"
    MPLBACKEND=Agg IPYTHONDIR="$notebook_cache/.ipython" MPLCONFIGDIR="$notebook_cache/.matplotlib" {{ _run }} uv run --locked --group notebooks jupyter lab --ServerApp.open_browser=True --LabApp.open_browser=True "{{ notebook }}"

# Lint notebooks and execute the fast notebook set.
notebook-check: notebook-lint notebook-execute-fast
    @echo "📓 Notebook checks complete!"

# Also execute the heavier analysis notebook.
notebook-check-slow: notebook-check notebook-execute-slow
    @echo "📓 Slow notebook checks complete!"

# Clear outputs and execution counts from one source notebook.
notebook-clear-outputs notebook="notebooks/00_quickstart.ipynb":
    {{ _notebooks }} notebooks clear "$1"

# Clear outputs and execution counts from all source notebooks.
notebook-clear-outputs-all:
    {{ _notebooks }} files run --include 'notebooks/*.ipynb' -- research-repo-tools notebooks clear

# Execute one notebook and save its output separately.
notebook-execute notebook="notebooks/00_quickstart.ipynb" output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} notebooks execute --output-dir "$2" "$1"

# Execute all notebooks and save their outputs separately.
notebook-execute-all output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} files run --include 'notebooks/*.ipynb' -- research-repo-tools notebooks execute --output-dir "$1"

# Execute the quickstart and visualization notebooks.
notebook-execute-fast output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} notebooks execute --output-dir "$1" notebooks/00_quickstart.ipynb notebooks/01_spacetime_visualization.ipynb

# Execute the heavier analysis-cache notebook.
notebook-execute-slow output_dir="target/notebooks":
    {{ _run }} {{ _notebooks }} notebooks execute --timeout 1800 --output-dir "$1" notebooks/02_analysis_caches.ipynb

# Validate notebook structure, output hygiene, and native Python.
notebook-lint:
    {{ _notebooks }} files run --include '*.ipynb' --exclude 'tests/semgrep/**' -- research-repo-tools notebooks lint

# Check source notebooks for outputs and execution counts.
notebook-output-check:
    {{ _notebooks }} files run --include '*.ipynb' --exclude 'tests/semgrep/**' -- research-repo-tools notebooks check

# Synchronize notebook dependencies and the managed kernel.
notebook-setup:
    uv run --locked --managed-python --only-group tooling research-repo-tools notebooks sync

# Run both curated large-scale toroidal debug cases.
perf-large-scale-debug max_secs="1800":
    just debug-large-scale-1p1-512 {{ max_secs }}
    just debug-large-scale-1p1-1024 {{ max_secs }}

# Run a shared performance command against explicit fresh evidence.
performance *args:
    {{ _run }} research-repo-tools performance "$@"

# Package a fresh tagged baseline in the shared format.
performance-baseline tag:
    {{ _run }} research-repo-tools performance baseline tooling/benchmark.toml "$1" "causal-triangulations-$1-cdt-baseline-v2.tar.gz"

# Check retained release reports, accepting an empty inventory before the first comparison.
performance-check:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    artifacts=(
        docs/performance/v2/*.comparison.json
        docs/performance/v2/*.evidence.json
        docs/performance/v2/*.csv
        docs/performance/v2/*.svg
        docs/performance/v2/v*-vs-v*.md
    )
    if [[ -e docs/performance/v2/performance.md || -L docs/performance/v2/performance.md ]]; then
        {{ _tools }} performance promote tooling/performance-report.toml --check
    elif (( ${#artifacts[@]} )) || [[ -e tooling/performance-readme.toml || -L tooling/performance-readme.toml ]]; then
        echo "Release evidence exists without its current report; restore or promote the report." >&2
        exit 1
    else
        echo "No release comparison yet; the next tagged release establishes the baseline."
    fi

# Promote supplied new evidence, or rerender the retained report offline.
performance-doc *args:
    {{ _tools }} performance promote tooling/performance-report.toml "$@"

# Compare two explicit new-format release assets without measuring.
performance-github-assets current baseline:
    {{ _tools }} performance assets "$1" "$2" --repository acgetchell/causal-triangulations --asset-template 'causal-triangulations-{tag}-cdt-baseline-v2.tar.gz' --payload target/bench-reports/assets.comparison.json --manifest target/bench-reports/assets.evidence.json --report target/bench-reports/assets.md

# Publish absolute timing columns and relative changes from independently pinned evidence.
performance-readme configuration *args:
    {{ _tools }} performance publish "$1" "${@:2}"

# Measure and publish release evidence in temporary Git worktrees (maintainer operation).
performance-release current baseline:
    {{ _run }} research-repo-tools performance measure tooling/benchmark.toml "$1" "$2" --allow-git-mutations --payload target/bench-reports/release.comparison.json --manifest target/bench-reports/release.evidence.json --report target/bench-reports/release.md
    {{ _tools }} performance promote tooling/performance-report.toml --payload target/bench-reports/release.comparison.json --manifest target/bench-reports/release.evidence.json

# Check package metadata and dry-run crates.io publication.
publish-check:
    {{ _run }} research-repo-tools validation cargo-metadata
    {{ _run }} cargo publish --locked --allow-dirty --dry-run

# Check all Python source and negative fixtures with Ruff and Ty.
python-check: python-source-check python-fixtures-check

# Apply Ruff fixes and formatting to all Python files.
python-fix:
    {{ _tools }} files run --include '*.py' --include '*.pyi' -- ruff check --fix --no-force-exclude
    {{ _tools }} files run --include '*.py' --include '*.pyi' -- ruff format --no-force-exclude

# Apply the complete configured Python policy to negative fixtures as well.
python-fixtures-check:
    {{ _tools }} files run --include 'tests/semgrep/**/*.py' -- ruff check --no-fix --no-force-exclude
    {{ _tools }} files run --include 'tests/semgrep/**/*.py' -- ruff format --check --no-force-exclude
    {{ _tools }} files run --include 'tests/semgrep/**/*.py' -- ty check --no-force-exclude --error all

# Check all consumer Python outside the deliberate negative fixtures.
python-source-check:
    {{ _tools }} toolchain python-check
    {{ _tools }} files run --include '*.py' --include '*.pyi' --exclude 'tests/semgrep/**' -- ruff check --no-fix --no-force-exclude
    {{ _tools }} files run --include '*.py' --include '*.pyi' --exclude 'tests/semgrep/**' -- ruff format --check --no-force-exclude
    {{ _tools }} files run --include '*.py' --include '*.pyi' --exclude 'tests/semgrep/**' -- ty check --no-force-exclude --error all

# Synchronize the locked development environment.
python-sync:
    uv sync --locked --group dev

# Type-check all Python files with strict Ty diagnostics.
python-typecheck:
    {{ _tools }} files run --include '*.py' --include '*.pyi' -- ty check --no-force-exclude --error all

# Validate release-version/date synchronization, concept DOI, and environment metadata.
release-metadata-check:
    {{ _tools }} release check

# Print release notes from the active changelog or archives.
release-notes tag:
    {{ _tools }} changelog notes "$1"

# Inspect a published GitHub release and its crates.io package.
release-verify tag:
    gh release view "$1" --repo acgetchell/causal-triangulations --json tagName,isDraft,isPrerelease,body,assets | cat
    {{ _run }} cargo info "causal-triangulations@${1#v}" --registry crates-io

# Require the generated current-version changelog heading for final release publication.
release-version-check:
    {{ _tools }} release check --final-release

# Review branch and local changes against verified origin/main, or an explicit local base.
review base="origin/main":
    {{ _tools }} review branch --base="$1"

# Review staged, unstaged, and nonignored untracked changes; requires explicit maintainer intent.
review-uncommitted:
    {{ _tools }} review uncommitted

# Run cdt; pass binary arguments after --.
run *args:
    {{ _run }} cargo run --bin cdt "$@"

# Generate a small valid open-boundary CDT strip.
run-example:
    {{ _run }} cargo run --bin cdt -- --vertices-per-slice 4 --timeslices 3

# Run optimized cdt; pass binary arguments after --.
run-release *args:
    {{ _run }} cargo run --release --bin cdt "$@"

# Run example simulation script
run-simulation:
    {{ _run }} ./examples/scripts/basic_simulation.sh

# Run the dependency vulnerability and full-history secret scans.
[group('security')]
security: audit security-secrets

# Scan full Git history and current files with redacted Gitleaks reports.
[group('security')]
security-secrets:
    {{ _tools }} security secrets

# Repository-owned Semgrep rules for project-specific diagnostics.
semgrep:
    mkdir -p target/security
    rm -f target/security/semgrep.sarif
    {{ _tools }} files run --include '*.rs' --include '*.py' --include '*.yml' --include '*.yaml' --include '*.md' --include '*.sh' --include '*.ipynb' --exclude 'tests/semgrep/**' --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- semgrep scan --config semgrep.yaml --metrics off --disable-version-check --strict --disable-nosem --no-rewrite-rule-ids --no-git-ignore --max-target-bytes 0 --timeout 30 --error --sarif --output target/security/semgrep.sarif

# Validate annotated repository-owned Semgrep fixtures.
semgrep-test:
    {{ _tools }} semgrep check-fixtures

# Development setup
setup:
    uv run --locked --managed-python --only-group tooling research-repo-tools setup

# Install and verify the exact declared development tools.
setup-tools:
    uv run --locked --only-group tooling --inexact research-repo-tools toolchain sync

# Preview adoption of a published shared package and its Python baseline.
shared-python-plan version:
    uvx --no-config --isolated --managed-python --from "research-repo-tools==$1" research-repo-tools toolchain adopt --dry-run

# Adopt a published shared package and its Python environment and kernel.
shared-python-update version:
    uvx --no-config --isolated --managed-python --from "research-repo-tools==$1" research-repo-tools toolchain adopt --apply

# Shell scripts: lint/check (non-mutating)
shell-check:
    {{ _tools }} files run --include '*.sh' -- shellcheck -x
    {{ _tools }} files run --include '*.sh' -- shfmt -d

# Apply shell formatting fixes.
shell-fix:
    {{ _tools }} files run --include '*.sh' -- shfmt -w

# Spell check (typos)
spell-check:
    {{ _run }} research-repo-tools files run --exclude typos.toml --exclude CHANGELOG.md --exclude 'docs/archive/**' --exclude 'docs/archives/**' -- typos --config typos.toml --force-exclude

# Create an annotated local Git tag from release notes (maintainer operation).
tag version:
    {{ _tools }} changelog tag "$1"

# Replace a local Git tag from release notes (maintainer operation).
tag-force version:
    {{ _tools }} changelog tag "$1" --force

# Preview the annotated release tag without changing Git state.
tag-preview version:
    {{ _tools }} changelog tag "$1" --dry-run

# Focused local Rust buckets: unit tests plus rustdoc doctests.
test: test-unit test-doc

# Broad Rust correctness plus Python tooling tests.
test-all: test-rust test-python
    @echo "✅ All tests passed!"

# Run CLI integration tests.
test-cli:
    {{ _run }} cargo nextest run --test cli --verbose

# Doctests must stay on cargo test; nextest does not run rustdoc doctests.
test-doc:
    {{ _run }} cargo test --doc --verbose

# Compile and run the Cargo example test harnesses.
test-examples:
    {{ _run }} cargo nextest run --examples --verbose --no-tests pass

# Run integration-test targets and their library-unit prerequisite.
test-integration:
    {{ _run }} cargo nextest run --tests --verbose

# Run Python consumer integration tests.
test-python:
    {{ _run }} uv run --locked python -m pytest

# Run release-mode Rust tests and doctests.
test-release:
    {{ _run }} cargo nextest run --release --workspace
    {{ _run }} cargo test --doc --release

# Broad Rust test workflow; doctests remain a separate cargo-test bucket.
test-rust: test-rust-ci test-doc
    @echo "✅ Rust tests passed!"

# Broad release-profile Rust CI bucket: lib unit and integration tests together.
test-rust-ci:
    {{ _run }} cargo nextest run --release --profile ci --lib --tests --verbose

# Run bounded feature-gated stress tests through the perf profile.
test-slow:
    CDT_LARGE_DEBUG_MAX_RUNTIME_SECS=1800 {{ _run }} cargo nextest run --cargo-profile perf --tests --features slow-tests --verbose

# Focused library unit tests for changed-surface validation.
test-unit:
    {{ _run }} cargo nextest run --lib --verbose

# Check TOML formatting and syntax.
toml-check: toml-fmt-check toml-lint

# Apply TOML formatting fixes.
toml-fix:
    {{ _run }} research-repo-tools files run --include '*.toml' -- taplo fmt

# Check TOML formatting without modifying files.
toml-fmt-check:
    {{ _run }} research-repo-tools files run --include '*.toml' -- taplo fmt --check

# Check TOML syntax.
toml-lint:
    {{ _run }} research-repo-tools files run --include '*.toml' -- taplo lint

# Verify declared tools without installing or changing versions.
tools-check:
    uv run --locked --no-sync --no-python-downloads research-repo-tools toolchain check

# Preview managed-cache cleanup; pass --apply to remove unused entries.
tools-clean *args:
    uv run --locked --no-sync --no-python-downloads research-repo-tools toolchain clean "$@"

# Export verified managed paths for subsequent workflow steps.
tools-export:
    uv run --locked --no-sync --no-python-downloads research-repo-tools toolchain export

# Check for unused direct Cargo dependencies.
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

# Upgrade uv and managed tools before dependency changes.
update-tools: update-uv update-cargo-tools setup

# Upgrade uv with its installation owner and reconcile the pin.
update-uv:
    uv run --no-config --no-sync --no-python-downloads research-repo-tools deps update-uv

# Synchronize deterministic release metadata from one stable GitHub tag.
update-version version *args:
    {{ _tools }} release update "$@"

# File validation
validate-json: _ensure-jq
    {{ _tools }} files run --include '*.json' -- jq empty

# Check YAML/CFF formatting and lint rules.
yaml-check: yaml-fmt-check yaml-lint

# Format YAML and citation metadata.
yaml-fix:
    {{ _run }} research-repo-tools files run --include '*.yml' --include '*.yaml' --include CITATION.cff -- dprint fmt --incremental=false

# Check YAML/CFF formatting without modifying files.
yaml-fmt-check:
    {{ _run }} research-repo-tools files run --include '*.yml' --include '*.yaml' --include CITATION.cff -- dprint check --incremental=false

# Lint YAML and citation metadata.
yaml-lint:
    {{ _run }} research-repo-tools files run --include '*.yml' --include '*.yaml' --include CITATION.cff -- yamllint --strict -c .yamllint

# Audit GitHub Actions through the shared authentication policy.
zizmor *args:
    {{ _tools }} zizmor check "$@"
