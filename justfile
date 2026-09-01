# shellcheck disable=SC2148
# Justfile for causal-triangulations development workflow
# Install just: https://github.com/casey/just
# Usage: just <command> or just --list

# Use bash with strict error handling for all recipes
set shell := ["bash", "-euo", "pipefail", "-c"]

cargo_audit_version := "0.22.2"
cargo_edit_version := "0.13.13"
cargo_llvm_cov_version := "0.9.0"
cargo_machete_version := "0.9.2"
cargo_nextest_version := "0.9.143"
cargo_update_version := "22.1.1"
clippy_sarif_version := "0.8.0"
dprint_version := "0.57.0"
git_cliff_version := "2.13.1"
just_version := "1.58.0"
rumdl_version := "0.2.62"
sarif_fmt_version := "0.8.0"
taplo_version := "0.10.0"
typos_version := "1.50.0"
uv_version := "0.12.7"
zizmor_version := "1.30.0"

# Common cargo-llvm-cov arguments for all coverage runs.
# Excludes benches/examples from reports while allowing integration tests to
# exercise library code.
_coverage_base_args := '''--ignore-filename-regex '(^|/)(benches|examples)/' \
  --workspace --lib --tests \
  --verbose'''

# Internal helpers: ensure external tooling is installed
_ensure-actionlint: _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    uv run actionlint -version >/dev/null

_ensure-cargo-edit:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if cargo upgrade --version >/dev/null 2>&1; then
        installed_version="$(cargo upgrade --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ cargo_edit_version }}" ]]; then
        echo "❌ 'cargo-edit' {{ cargo_edit_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked cargo-edit --version {{ cargo_edit_version }}"
        exit 1
    fi

_ensure-cargo-install-update:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v cargo-install-update >/dev/null; then
        installed_version="$(cargo-install-update --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ cargo_update_version }}" ]]; then
        echo "❌ 'cargo-update' {{ cargo_update_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked cargo-update --version {{ cargo_update_version }}"
        exit 1
    fi

_ensure-cargo-llvm-cov:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v cargo-llvm-cov >/dev/null; then
        installed_version="$(cargo llvm-cov --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ cargo_llvm_cov_version }}" ]]; then
        echo "❌ 'cargo-llvm-cov' {{ cargo_llvm_cov_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked cargo-llvm-cov --version {{ cargo_llvm_cov_version }}"
        exit 1
    fi

_ensure-cargo-machete:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if cargo machete --version >/dev/null 2>&1; then
        installed_version="$(cargo machete --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ cargo_machete_version }}" ]]; then
        echo "❌ 'cargo-machete' {{ cargo_machete_version }} not found. Install with:"
        echo "   cargo install --locked cargo-machete --version {{ cargo_machete_version }}"
        exit 1
    fi

_ensure-cargo-nextest:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if cargo nextest --version >/dev/null 2>&1; then
        installed_version="$(cargo nextest --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ cargo_nextest_version }}" ]]; then
        echo "❌ 'cargo-nextest' {{ cargo_nextest_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked cargo-nextest --version {{ cargo_nextest_version }}"
        exit 1
    fi

_ensure-dprint:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v dprint >/dev/null; then
        installed_version="$(dprint --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ dprint_version }}" ]]; then
        echo "❌ 'dprint' {{ dprint_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked dprint --version {{ dprint_version }}"
        exit 1
    fi

_ensure-git-cliff:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v git-cliff >/dev/null; then
        installed_version="$(git-cliff --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ git_cliff_version }}" ]]; then
        echo "❌ 'git-cliff' {{ git_cliff_version }} not found. Install with:"
        echo "   cargo install --locked git-cliff --version {{ git_cliff_version }}"
        exit 1
    fi

_ensure-jq:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v jq >/dev/null || { echo "❌ 'jq' not found. See 'just setup' or install: brew install jq"; exit 1; }

_ensure-rumdl:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v rumdl >/dev/null; then
        installed_version="$(rumdl --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ rumdl_version }}" ]]; then
        echo "❌ 'rumdl' {{ rumdl_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked rumdl --version {{ rumdl_version }}"
        exit 1
    fi

_ensure-shellcheck:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v uv >/dev/null || { echo "❌ 'uv' not found. See 'just setup' or https://github.com/astral-sh/uv"; exit 1; }
    uv run shellcheck --version >/dev/null

_ensure-shfmt:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v uv >/dev/null || { echo "❌ 'uv' not found. See 'just setup' or https://github.com/astral-sh/uv"; exit 1; }
    uv run shfmt --version >/dev/null

# Internal helper: ensure taplo is installed
_ensure-taplo:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v taplo >/dev/null; then
        installed_version="$(taplo --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ taplo_version }}" ]]; then
        echo "❌ 'taplo' {{ taplo_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked taplo-cli --version {{ taplo_version }}"
        exit 1
    fi

# Internal helper: ensure typos-cli is installed
_ensure-typos:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v typos >/dev/null; then
        installed_version="$(typos --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ typos_version }}" ]]; then
        echo "❌ 'typos' {{ typos_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked typos-cli --version {{ typos_version }}"
        exit 1
    fi

# Internal helper: ensure uv is installed
_ensure-uv:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v uv >/dev/null; then
        installed_version="$(uv --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ uv_version }}" ]]; then
        echo "❌ 'uv' {{ uv_version }} not found. Install the pinned version and re-run: just setup-tools"
        echo "   https://docs.astral.sh/uv/getting-started/installation/"
        exit 1
    fi

_ensure-uv-available:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v uv >/dev/null || {
        echo "❌ 'uv' not found. Install it from https://github.com/astral-sh/uv" >&2
        exit 1
    }
    uv --version >/dev/null

_ensure-yamllint:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v uv >/dev/null || { echo "❌ 'uv' not found. See 'just setup' or https://github.com/astral-sh/uv"; exit 1; }
    uv run yamllint --version >/dev/null

_ensure-zizmor:
    #!/usr/bin/env bash
    set -euo pipefail
    installed_version=""
    if command -v zizmor >/dev/null; then
        installed_version="$(zizmor --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    fi
    if [[ "$installed_version" != "{{ zizmor_version }}" ]]; then
        echo "❌ 'zizmor' {{ zizmor_version }} not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked zizmor --version {{ zizmor_version }}"
        exit 1
    fi

# GitHub Actions workflow validation
action-lint: _ensure-actionlint
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '.github/workflows/*.yml' '.github/workflows/*.yaml')
    if [ "${#files[@]}" -gt 0 ]; then
        # actionlint 1.7.12 predates $/ syntax; ignore only this valid self-repository reference.
        # Remove when https://github.com/rhysd/actionlint/issues/711 is released.
        printf '%s\0' "${files[@]}" | xargs -0 uv run actionlint \
            -ignore '^specifying action "\$/\.github/actions/setup-just" in invalid format because ref is missing\.'
    else
        echo "No workflow files found to lint."
    fi

# Benchmarks
allocation-check:
    cargo bench --profile perf --bench allocation_profile

bench:
    cargo bench --workspace

bench-ci: allocation-check
    cargo bench --profile perf --bench ci_performance_suite

# Compare existing current Criterion output with a retained named baseline.
bench-compare baseline="last": _ensure-uv
    uv run release-performance compare --baseline "{{ baseline }}"

# Compile benchmarks without running them, treating warnings as errors.
# This catches bench/release-profile-only warnings (e.g. debug_assertions-gated unused vars)
# that won't show up in normal debug-profile `cargo test` / `cargo clippy` runs.
bench-compile:
    CARGO_BUILD_WARNINGS=deny cargo bench --workspace --no-run

# Run the correctness gate before producing release-signal measurements.
benchmark-input-check:
    cargo test --locked --release --test integration_tests --test physics_integration

# Produce the correctness-gated current release signal.
bench-latest: benchmark-input-check
    cargo bench --locked --profile perf --bench ci_performance_suite -- --noplot

# Produce the current release signal and compare it with the conventional local baseline.
bench-latest-vs-last: bench-latest _ensure-uv
    uv run release-performance compare --baseline last

# Produce a named native Criterion baseline after validating benchmark inputs.
bench-save-baseline tag: benchmark-input-check
    cargo bench --locked --profile perf --bench ci_performance_suite -- --save-baseline "{{ tag }}" --noplot

# Refresh the conventional local comparison baseline.
bench-save-last: benchmark-input-check
    cargo bench --locked --profile perf --bench ci_performance_suite -- --save-baseline last --noplot

# Smoke-test benchmark harnesses with minimal samples; not for performance data.
bench-smoke:
    cargo bench --workspace --bench cdt_benchmarks -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot
    cargo bench --workspace --bench ci_performance_suite -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot

# Compile benchmarks and release-profile Rust test binaries without running.
bench-test-compile: bench-compile _ensure-cargo-nextest
    cargo nextest run --tests --release --no-run

# Build commands
build:
    cargo build

build-release:
    cargo build --release

# Changelog management (git-cliff + post-processing + archiving + rumdl formatting)
changelog: _ensure-git-cliff _ensure-rumdl python-sync
    #!/usr/bin/env bash
    set -euo pipefail
    GIT_CLIFF_OFFLINE=true git-cliff -o CHANGELOG.md
    uv run postprocess-changelog
    uv run archive-changelog
    archive_files=()
    if [ -d docs/archive/changelog ]; then
        while IFS= read -r -d '' file; do
            archive_files+=("$file")
        done < <(find docs/archive/changelog -name '*.md' -print0)
    fi
    if ((${#archive_files[@]})); then
        rumdl fmt --silent CHANGELOG.md "${archive_files[@]}"
    else
        rumdl fmt --silent CHANGELOG.md
    fi

changelog-tag version:
    just tag {{ version }}

changelog-unreleased version: _ensure-git-cliff _ensure-rumdl python-sync
    #!/usr/bin/env bash
    set -euo pipefail
    GIT_CLIFF_OFFLINE=true git-cliff --tag {{ version }} -o CHANGELOG.md
    uv run postprocess-changelog
    uv run update-release-version {{ version }} --sync-changelog-date
    uv run archive-changelog
    archive_files=()
    if [ -d docs/archive/changelog ]; then
        while IFS= read -r -d '' file; do
            archive_files+=("$file")
        done < <(find docs/archive/changelog -name '*.md' -print0)
    fi
    if ((${#archive_files[@]})); then
        rumdl fmt --silent CHANGELOG.md "${archive_files[@]}"
    else
        rumdl fmt --silent CHANGELOG.md
    fi

changelog-update: changelog
    @echo "📝 Changelog updated successfully!"
    @echo "To create a git tag with changelog content for a specific version, run:"
    @echo "  just tag <version>  # e.g., just tag v0.4.2"

# Check (non-mutating): run all linters/validators
check: lint
    @echo "✅ Checks complete!"

# Fast compile check (no binary produced)
check-fast:
    cargo check

# CI simulation: flat union of GitHub-equivalent focused validators.
# All Cargo targets receive the same Clippy coverage as the SARIF workflow;
# runnable Rust tests and rustdoc doctests remain separate execution evidence.
ci: action-lint zizmor markdown-check spell-check validate-json toml-fmt-check toml-lint yaml-fmt-check yaml-lint citation-check python-check test-python notebook-check shell-check semgrep semgrep-test fmt-check clippy-all-targets doc-check test-rust-ci test-doc bench-compile allocation-check examples-validate
    @echo "🎯 CI checks complete!"

# CI with performance baseline
ci-baseline tag="ci":
    just ci
    just perf-baseline {{ tag }}

# CI + feature-gated slow/stress tests.
ci-slow: ci test-slow
    @echo "✅ CI + slow tests passed!"

# Validate release-version/date synchronization, concept DOI, and support-package metadata.
release-metadata-check: _ensure-uv
    uv run check-release-metadata

# Require the generated current-version changelog heading for final release publication.
release-version-check: _ensure-uv
    uv run check-release-metadata --final-release

# Validate CITATION.cff against the Citation File Format schema and release metadata gate.
citation-check: release-metadata-check _ensure-uv
    uvx --from cffconvert==2.0.0 cffconvert --validate -i CITATION.cff

# Clean build artifacts
clean:
    cargo clean
    rm -rf target/llvm-cov
    rm -rf coverage_report
    rm -rf coverage

# Fast production Rust linting used by `just check`.
clippy:
    cargo clippy --workspace --all-features --lib --bins -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::cargo

# Full Cargo-target Clippy sweep used by `just ci` and the GitHub SARIF workflow.
clippy-all-targets:
    cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::cargo

# Pre-commit workflow: comprehensive validation (checks + tests + release + benches)
commit-check: check test-all test-release bench-compile
    @echo "🚀 Ready to commit! All checks passed."

# Coverage analysis for local development (HTML output)
coverage: _ensure-cargo-llvm-cov
    mkdir -p target/llvm-cov
    cargo llvm-cov {{ _coverage_base_args }} --html --output-dir target/llvm-cov
    @echo "📊 Coverage report generated: target/llvm-cov/html/index.html"

# Coverage analysis for CI (Cobertura XML output for codecov/codacy)
coverage-ci: _ensure-cargo-llvm-cov
    mkdir -p coverage
    cargo llvm-cov {{ _coverage_base_args }} --cobertura --output-path coverage/cobertura.xml

coverage-report *args: _ensure-uv
    uv run coverage_report {{ args }}

debug-large-scale-1p1 vertices="512" timeslices="16" sweeps="10" max_secs="1800" seed="0xCD710139": _ensure-cargo-nextest
    CDT_LARGE_DEBUG_VERTICES_1P1={{ vertices }} CDT_LARGE_DEBUG_TIMESLICES_1P1={{ timeslices }} CDT_LARGE_DEBUG_SWEEPS_1P1={{ sweeps }} CDT_LARGE_DEBUG_SEED_1P1={{ seed }} CDT_LARGE_DEBUG_MAX_RUNTIME_SECS={{ max_secs }} cargo nextest run --cargo-profile perf --features slow-tests --test large_scale_debug debug_large_scale_1p1 -- --exact --nocapture

debug-large-scale-1p1-1024 max_secs="1800":
    just debug-large-scale-1p1 1024 32 1 {{ max_secs }}

debug-large-scale-1p1-512 max_secs="1800":
    just debug-large-scale-1p1 512 16 10 {{ max_secs }}

# Default recipe shows available commands
default:
    @just --list

doc-check:
    RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items

# Examples and validation
examples:
    ./scripts/run_all_examples.sh

examples-validate:
    ./scripts/run_all_examples.sh --validate

# Fix (mutating): apply formatters/auto-fixes
fix: toml-fix fmt python-fix shell-fix markdown-fix yaml-fix
    @echo "✅ Fixes applied!"

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

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
    @echo "  just bench-latest-vs-last # Produce the release signal and compare with 'last'"
    @echo "  just bench-save-baseline # Save a named native Criterion baseline"
    @echo "  just bench-save-last    # Refresh the conventional 'last' baseline"
    @echo "  just bench-smoke        # Smoke-test benchmark harnesses with minimal samples"
    @echo "  just bench-test-compile # Compile benches + release integration tests without running"
    @echo "  just debug-large-scale-1p1 # Run one toroidal 1+1 CDT debug case"
    @echo "  just perf-large-scale-debug # Run curated large-scale CDT debug cases"
    @echo ""
    @echo "Performance Analysis:"
    @echo "  just perf-baseline # Save current performance as baseline"
    @echo "  just perf-check    # Check for performance regressions"
    @echo "  just perf-compare  # Compare with a specific baseline file"
    @echo "  just perf-help     # Show performance analysis commands"
    @echo "  just perf-report   # Generate performance report"
    @echo "  just perf-trends   # Analyze performance trends"
    @echo "  just performance-doc # Render reports from retained CSV/provenance only"
    @echo "  just performance-github-assets # Compare GitHub Release-native Criterion assets"
    @echo "  just performance-local # Measure HEAD against the latest stable release"
    @echo "  just performance-readme # Update the owned README summary from retained evidence"
    @echo "  just performance-release # Measure, retain, reload, and publish release evidence"
    @echo ""
    @echo "Changelog:"
    @echo "  just changelog                   # Generate/update CHANGELOG.md"
    @echo "  just changelog-unreleased <ver>  # Generate release changelog without a local tag"
    @echo "  just tag <ver>                   # Create git tag with changelog content"
    @echo ""
    @echo "Static Analysis:"
    @echo "  just citation-check      # Validate CFF schema and synchronized release metadata"
    @echo "  just publish-check       # Validate crates.io metadata and dry-run publish"
    @echo "  just release-metadata-check # Validate release dates and Python package README"
    @echo "  just semgrep             # Run repository-owned Semgrep rules"
    @echo "  just semgrep-test        # Test repository-owned Semgrep rules"
    @echo "  just unused-deps         # Check for unused direct Cargo dependencies"
    @echo "  just zizmor              # GitHub Actions security analysis"
    @echo ""
    @echo "Running:"
    @echo "  just notebook         # Launch the quickstart notebook with uv-managed dependencies"
    @echo "  just notebook-lint    # Validate JSON, output hygiene, and extracted Python"
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
lint-code: fmt-check clippy doc-check semgrep semgrep-test python-lint shell-lint

# Configuration validation: JSON, TOML, YAML/CFF, GitHub Actions workflows
lint-config: validate-json toml-check yaml-check citation-check action-lint zizmor

# Documentation linting: Markdown + spell checking
lint-docs: markdown-check spell-check

markdown-check: _ensure-rumdl
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        case "$file" in
            CHANGELOG.md|docs/archive/*) continue ;;
        esac
        [[ -f "$file" ]] || continue
        files+=("$file")
    done < <(git ls-files -z '*.md')
    if [ "${#files[@]}" -gt 0 ]; then
        printf '%s\0' "${files[@]}" | xargs -0 -n100 rumdl check
        violations=0
        for file in "${files[@]}"; do
            line_number=0
            while IFS= read -r line || [[ -n "$line" ]]; do
                line_number=$((line_number + 1))
                line_length="$(LC_ALL=C printf '%s' "$line" | wc -c | tr -d ' ')"
                if [ "$line_length" -gt 160 ]; then
                    printf '%s:%d: line length %d exceeds 160\n' "$file" "$line_number" "$line_length" >&2
                    violations=$((violations + 1))
                fi
            done < "$file"
        done
        if [ "$violations" -gt 0 ]; then
            echo "Markdown raw line-length check failed." >&2
            exit 1
        fi
    else
        echo "No markdown files found to check."
    fi

# Markdown and YAML: apply auto-fixes (mutating)
markdown-fix: _ensure-rumdl
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        case "$file" in
            CHANGELOG.md|docs/archive/*) continue ;;
        esac
        [[ -f "$file" ]] || continue
        files+=("$file")
    done < <(git ls-files -z '*.md')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "📝 rumdl check --fix (${#files[@]} files)"
        printf '%s\0' "${files[@]}" | xargs -0 -n100 rumdl check --fix
    else
        echo "No markdown files found to format."
    fi

markdown-lint: markdown-check

notebook notebook="notebooks/00_quickstart.ipynb": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    notebook_cache="$(pwd)/target/notebooks"
    mkdir -p "$notebook_cache/.ipython" "$notebook_cache/.matplotlib"
    MPLBACKEND=Agg IPYTHONDIR="$notebook_cache/.ipython" MPLCONFIGDIR="$notebook_cache/.matplotlib" uv run --group notebooks jupyter lab --ServerApp.open_browser=True --LabApp.open_browser=True "{{ notebook }}"

notebook-check: notebook-lint notebook-execute-fast
    @echo "📓 Notebook checks complete!"

notebook-check-slow: notebook-check notebook-execute-slow
    @echo "📓 Slow notebook checks complete!"

notebook-clear-outputs notebook="notebooks/00_quickstart.ipynb": _ensure-uv
    uv run --group notebooks jupyter nbconvert --clear-output --inplace "{{ notebook }}"

notebook-clear-outputs-all: _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    while IFS= read -r notebook; do
        uv run --group notebooks jupyter nbconvert --clear-output --inplace "$notebook"
    done < <(find notebooks -type f -name '*.ipynb' | sort)

notebook-execute notebook="notebooks/00_quickstart.ipynb" output_dir="target/notebooks": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    output_path="$(pwd)/{{ output_dir }}"
    mkdir -p "$output_path/.ipython" "$output_path/.matplotlib"
    MPLBACKEND=Agg IPYTHONDIR="$output_path/.ipython" MPLCONFIGDIR="$output_path/.matplotlib" uv run --group notebooks jupyter nbconvert --execute --ExecutePreprocessor.timeout=600 --ExecutePreprocessor.shutdown_kernel=immediate --to notebook --output-dir "{{ output_dir }}" "{{ notebook }}"

notebook-execute-all output_dir="target/notebooks": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    output_path="$(pwd)/{{ output_dir }}"
    mkdir -p "$output_path/.ipython" "$output_path/.matplotlib"
    found=0
    while IFS= read -r notebook; do
        found=1
        MPLBACKEND=Agg IPYTHONDIR="$output_path/.ipython" MPLCONFIGDIR="$output_path/.matplotlib" uv run --group notebooks jupyter nbconvert --execute --ExecutePreprocessor.timeout=600 --ExecutePreprocessor.shutdown_kernel=immediate --to notebook --output-dir "{{ output_dir }}" "$notebook"
    done < <(find notebooks -type f -name '*.ipynb' | sort)
    if [ "$found" -eq 0 ]; then
        echo "No notebooks found to execute."
    fi

notebook-execute-fast output_dir="target/notebooks": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    output_path="$(pwd)/{{ output_dir }}"
    mkdir -p "$output_path/.ipython" "$output_path/.matplotlib"
    for notebook in notebooks/00_quickstart.ipynb notebooks/01_spacetime_visualization.ipynb; do
        MPLBACKEND=Agg IPYTHONDIR="$output_path/.ipython" MPLCONFIGDIR="$output_path/.matplotlib" uv run --group notebooks jupyter nbconvert --execute --ExecutePreprocessor.timeout=600 --ExecutePreprocessor.shutdown_kernel=immediate --to notebook --output-dir "{{ output_dir }}" "$notebook"
    done

notebook-execute-slow output_dir="target/notebooks": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    output_path="$(pwd)/{{ output_dir }}"
    mkdir -p "$output_path/.ipython" "$output_path/.matplotlib"
    MPLBACKEND=Agg IPYTHONDIR="$output_path/.ipython" MPLCONFIGDIR="$output_path/.matplotlib" uv run --group notebooks jupyter nbconvert --execute --ExecutePreprocessor.timeout=1800 --ExecutePreprocessor.shutdown_kernel=immediate --to notebook --output-dir "{{ output_dir }}" notebooks/02_analysis_caches.ipynb

notebook-lint: _ensure-uv
    uv run --group notebooks notebook-check --lint --repo-root .

notebook-output-check: _ensure-uv
    uv run --group notebooks notebook-check --lint --repo-root . --no-ruff --no-format --no-ty

notebook-setup: _ensure-uv
    uv sync --group notebooks

perf-baseline tag="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    tag_value="{{ tag }}"
    if [ -n "$tag_value" ]; then
        uv run performance-analysis --save-baseline --tag "$tag_value"
    else
        uv run performance-analysis --save-baseline
    fi

perf-check threshold="10.0": _ensure-uv
    uv run performance-analysis --threshold {{ threshold }}

perf-compare file: _ensure-uv
    uv run performance-analysis --compare "{{ file }}"

perf-help:
    @echo "Performance Analysis Commands:"
    @echo "  just perf-baseline [tag]    # Save current performance as baseline (optionally tagged)"
    @echo "  just perf-check [threshold] # Check for regressions (default: 10% threshold)"
    @echo "  just perf-compare <file>    # Compare with specific baseline file"
    @echo "  just perf-report [file]     # Generate performance report"
    @echo "  just perf-trends [days]     # Analyze trends over N days (default: 7)"
    @echo ""
    @echo "Examples:"
    @echo "  just perf-baseline v1.0.0   # Save tagged baseline"
    @echo "  just perf-check 5.0         # Check with 5% threshold"
    @echo "  just perf-report my_report.md # Save report to specific file"
    @echo "  just perf-trends 30         # Analyze last 30 days"

perf-large-scale-debug max_secs="1800":
    just debug-large-scale-1p1-512 {{ max_secs }}
    just debug-large-scale-1p1-1024 {{ max_secs }}

perf-report file="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    file_value="{{ file }}"
    if [ -n "$file_value" ]; then
        uv run performance-analysis --report "$file_value"
    else
        timestamp=$(date +"%Y%m%d_%H%M%S")
        uv run performance-analysis --report "performance_report_${timestamp}.md"
        echo "📄 Report saved to: performance_report_${timestamp}.md"
    fi

perf-trends days="7": _ensure-uv
    uv run performance-analysis --trends {{ days }}

# Render tracked performance reports from the validated retained pair only.
performance-doc report_bundle="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    args=(document)
    if [[ -n "{{ report_bundle }}" ]]; then args+=(--bundle-dir "{{ report_bundle }}"); fi
    uv run release-performance "${args[@]}"

# Compare two GitHub Release-native Criterion assets without invoking Cargo.
performance-github-assets current_tag="" baseline_tag="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    args=(github-assets)
    if [[ -n "{{ current_tag }}" ]]; then args+=(--current-tag "{{ current_tag }}"); fi
    if [[ -n "{{ baseline_tag }}" ]]; then args+=(--baseline-tag "{{ baseline_tag }}"); fi
    uv run release-performance "${args[@]}"

# Compare the current tracked source state with the latest published stable release.
performance-local current_tag="" baseline_tag="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    args=(local)
    if [[ -n "{{ current_tag }}" ]]; then args+=(--current-tag "{{ current_tag }}"); fi
    if [[ -n "{{ baseline_tag }}" ]]; then args+=(--baseline-tag "{{ baseline_tag }}"); fi
    uv run release-performance "${args[@]}"

# Persist release evidence, reload it, and atomically publish reports and README assets.
performance-release current_tag="" baseline_tag="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    args=(release)
    if [[ -n "{{ current_tag }}" ]]; then args+=(--current-tag "{{ current_tag }}"); fi
    if [[ -n "{{ baseline_tag }}" ]]; then args+=(--baseline-tag "{{ baseline_tag }}"); fi
    uv run release-performance "${args[@]}"

# Update only the owned README summary and visual from the validated retained pair.
performance-readme report_bundle="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    args=(document --readme-only)
    if [[ -n "{{ report_bundle }}" ]]; then args+=(--bundle-dir "{{ report_bundle }}"); fi
    uv run release-performance "${args[@]}"

publish-check: _ensure-jq
    #!/usr/bin/env bash
    set -euo pipefail
    echo "🔍 Validating crates.io metadata..."
    errors=0

    metadata=$(cargo metadata --no-deps --format-version=1 2>/dev/null)
    keywords=$(echo "$metadata" | jq -r '.packages[0].keywords // [] | .[]')
    count=0
    while IFS= read -r kw; do
        [[ -z "$kw" ]] && continue
        count=$((count + 1))
        if (( ${#kw} > 20 )); then
            echo "  ❌ keyword '${kw}' exceeds 20-char limit (${#kw} chars)"
            errors=1
        fi
        if ! [[ "$kw" =~ ^[a-zA-Z0-9_-]+$ ]]; then
            echo "  ❌ keyword '${kw}' contains invalid characters"
            errors=1
        fi
    done <<< "$keywords"
    if (( count > 5 )); then
        echo "  ❌ too many keywords ($count > 5)"
        errors=1
    fi
    echo "  ✓ keywords ($count): $keywords"

    cat_count=$(cargo metadata --no-deps --format-version=1 2>/dev/null \
        | jq '.packages[0].categories | length')
    if (( cat_count > 5 )); then
        echo "  ❌ too many categories ($cat_count > 5)"
        errors=1
    fi
    echo "  ✓ categories ($cat_count)"

    desc=$(cargo metadata --no-deps --format-version=1 2>/dev/null \
        | jq -r '.packages[0].description // ""')
    if [[ -z "$desc" ]]; then
        echo "  ❌ description is missing"
        errors=1
    elif (( ${#desc} > 1000 )); then
        echo "  ❌ description exceeds 1000-char limit (${#desc} chars)"
        errors=1
    fi
    echo "  ✓ description (${#desc} chars)"

    if (( errors )); then
        echo ""
        echo "❌ Metadata validation failed. Fix Cargo.toml before publishing."
        exit 1
    fi

    echo ""
    echo "📦 Running cargo publish --dry-run..."
    cargo publish --locked --allow-dirty --dry-run
    echo ""
    echo "✅ Publish check passed!"

python-check: _ensure-uv
    uv run ruff format --check scripts/
    uv run ruff check scripts/
    just python-typecheck

# Python code quality
python-fix: _ensure-uv
    uv run ruff check scripts/ --fix
    uv run ruff format scripts/

python-lint: python-check

python-sync: _ensure-uv
    uv sync --group dev

python-typecheck: _ensure-uv
    uv run ty check scripts/ --error all

# Running the binary
run *args:
    cargo run --bin cdt {{ args }}

run-example:
    cargo run --bin cdt -- -v 32 -t 3

run-release *args:
    cargo run --release --bin cdt {{ args }}

# Run example simulation script
run-simulation:
    ./examples/scripts/basic_simulation.sh

# Repository-owned Semgrep rules for project-specific diagnostics.
semgrep: _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    uv run semgrep --metrics off --error --strict --timeout 30 --config semgrep.yaml .
    python_test_files=()
    while IFS= read -r file; do
        python_test_files+=("$file")
    done < <(git ls-files 'scripts/tests/*.py')
    if [[ "${#python_test_files[@]}" -gt 0 ]]; then
        uv run semgrep --metrics off --error --strict --timeout 30 --config semgrep.yaml "${python_test_files[@]}"
    fi

semgrep-test: _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail

    check_semgrep_fixture() {
        target="$1"
        json="$(uv run semgrep scan --metrics off --json --quiet --strict --config semgrep.yaml "$target")"
        SEMGREP_JSON="$json" uv run scripts/check_semgrep_fixtures.py "$target"
    }

    while IFS= read -r -d '' fixture; do
        check_semgrep_fixture "$fixture"
    done < <(find tests/semgrep -type f ! -name '*.fixed' -print0)

# cspell:ignore oldname newname

# Development setup
setup: setup-tools
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Setting up causal-triangulations development environment..."
    echo "Note: Rust toolchain and components managed by rust-toolchain.toml"
    echo ""
    echo "Installing Python tooling..."
    uv sync --group dev
    echo ""
    echo "Building project..."
    cargo build
    echo "✅ Setup complete! Run 'just help-workflows' to see available commands."

# Development tooling installation (best-effort)
setup-tools:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "🔧 Ensuring tooling required by just recipes is installed..."
    echo ""

    have() { command -v "$1" >/dev/null 2>&1; }
    for prerequisite in uv rustup cargo gh jq; do
        if ! have "$prerequisite"; then
            echo "❌ '$prerequisite' not found. Install it and re-run: just setup-tools"
            exit 1
        fi
    done
    uv_version="{{ uv_version }}"
    if [[ "$(uv --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$uv_version" ]]; then
        echo "❌ 'uv' ${uv_version} is required. Update uv and re-run: just setup-tools"
        exit 1
    fi

    echo "Ensuring Rust toolchain + components..."
    rustup component add clippy rustfmt rust-docs rust-src
    rustup component add llvm-tools-preview
    echo ""

    echo "Ensuring cargo tools..."
    cargo_update_version="{{ cargo_update_version }}"
    if ! have cargo-install-update || [[ "$(cargo-install-update --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$cargo_update_version" ]]; then
        echo "  ⏳ Installing cargo-update ${cargo_update_version} (cargo)..."
        cargo install --locked cargo-update --version "${cargo_update_version}"
    else
        echo "  ✓ cargo-update ${cargo_update_version}"
    fi

    cargo_edit_version="{{ cargo_edit_version }}"
    if ! cargo upgrade --version >/dev/null 2>&1 || [[ "$(cargo upgrade --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$cargo_edit_version" ]]; then
        echo "  ⏳ Installing cargo-edit ${cargo_edit_version} (cargo)..."
        cargo install --locked cargo-edit --version "${cargo_edit_version}"
    else
        echo "  ✓ cargo-edit ${cargo_edit_version}"
    fi

    cargo_audit_version="{{ cargo_audit_version }}"
    if ! cargo audit --version >/dev/null 2>&1 || [[ "$(cargo audit --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$cargo_audit_version" ]]; then
        echo "  ⏳ Installing cargo-audit ${cargo_audit_version} (cargo)..."
        cargo install --locked cargo-audit --version "${cargo_audit_version}"
    else
        echo "  ✓ cargo-audit ${cargo_audit_version}"
    fi

    just_version="{{ just_version }}"
    if ! have just || [[ "$(just --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$just_version" ]]; then
        echo "  ⏳ Installing just ${just_version} (cargo)..."
        cargo install --locked just --version "${just_version}"
    else
        echo "  ✓ just ${just_version}"
    fi

    dprint_version="{{ dprint_version }}"
    if ! have dprint || [[ "$(dprint --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$dprint_version" ]]; then
        echo "  ⏳ Installing dprint ${dprint_version} (cargo)..."
        cargo install --locked dprint --version "${dprint_version}"
    else
        echo "  ✓ dprint ${dprint_version}"
    fi

    rumdl_version="{{ rumdl_version }}"
    if ! have rumdl || [[ "$(rumdl --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$rumdl_version" ]]; then
        echo "  ⏳ Installing rumdl ${rumdl_version} (cargo)..."
        cargo install --locked rumdl --version "${rumdl_version}"
    else
        echo "  ✓ rumdl ${rumdl_version}"
    fi

    taplo_version="{{ taplo_version }}"
    if ! have taplo || [[ "$(taplo --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$taplo_version" ]]; then
        echo "  ⏳ Installing taplo-cli ${taplo_version} (cargo)..."
        cargo install --locked taplo-cli --version "${taplo_version}"
    else
        echo "  ✓ taplo ${taplo_version}"
    fi

    typos_version="{{ typos_version }}"
    if ! have typos || [[ "$(typos --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$typos_version" ]]; then
        echo "  ⏳ Installing typos-cli ${typos_version} (cargo)..."
        cargo install --locked typos-cli --version "${typos_version}"
    else
        echo "  ✓ typos ${typos_version}"
    fi

    git_cliff_version="{{ git_cliff_version }}"
    if ! have git-cliff || [[ "$(git-cliff --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$git_cliff_version" ]]; then
        echo "  ⏳ Installing git-cliff ${git_cliff_version} (cargo)..."
        cargo install --locked git-cliff --version "${git_cliff_version}"
    else
        echo "  ✓ git-cliff ${git_cliff_version}"
    fi

    cargo_llvm_cov_version="{{ cargo_llvm_cov_version }}"
    if ! have cargo-llvm-cov || [[ "$(cargo-llvm-cov --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$cargo_llvm_cov_version" ]]; then
        echo "  ⏳ Installing cargo-llvm-cov ${cargo_llvm_cov_version} (cargo)..."
        cargo install --locked cargo-llvm-cov --version "${cargo_llvm_cov_version}"
    else
        echo "  ✓ cargo-llvm-cov ${cargo_llvm_cov_version}"
    fi

    cargo_nextest_version="{{ cargo_nextest_version }}"
    if ! cargo nextest --version >/dev/null 2>&1 || [[ "$(cargo nextest --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$cargo_nextest_version" ]]; then
        echo "  ⏳ Installing cargo-nextest ${cargo_nextest_version} (cargo)..."
        cargo install --locked cargo-nextest --version "${cargo_nextest_version}"
    else
        echo "  ✓ cargo-nextest ${cargo_nextest_version}"
    fi

    cargo_machete_version="{{ cargo_machete_version }}"
    if ! cargo machete --version >/dev/null 2>&1 || [[ "$(cargo machete --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$cargo_machete_version" ]]; then
        echo "  ⏳ Installing cargo-machete ${cargo_machete_version} (cargo)..."
        cargo install --locked cargo-machete --version "${cargo_machete_version}"
    else
        echo "  ✓ cargo-machete ${cargo_machete_version}"
    fi

    zizmor_version="{{ zizmor_version }}"
    if ! have zizmor || [[ "$(zizmor --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)" != "$zizmor_version" ]]; then
        echo "  ⏳ Installing zizmor ${zizmor_version} (cargo)..."
        cargo install --locked zizmor --version "${zizmor_version}"
    else
        echo "  ✓ zizmor ${zizmor_version}"
    fi

    echo ""
    echo "Ensuring uv-managed Python tools..."
    uv sync --locked --group dev

    echo ""
    echo "Verifying required commands are available..."
    missing=0

    cmds=(uv gh jq cargo-install-update cargo-upgrade cargo-audit just taplo dprint rumdl git-cliff typos cargo-llvm-cov zizmor)
    for cmd in "${cmds[@]}"; do
        if have "$cmd"; then
            echo "  ✓ $cmd"
        else
            echo "  ✗ $cmd"
            missing=1
        fi
    done
    if cargo machete --version >/dev/null 2>&1; then
        echo "  ✓ cargo machete"
    else
        echo "  ✗ cargo machete"
        missing=1
    fi
    if cargo nextest --version >/dev/null 2>&1; then
        echo "  ✓ cargo nextest"
    else
        echo "  ✗ cargo nextest"
        missing=1
    fi
    if [ "$missing" -ne 0 ]; then
        echo ""
        echo "❌ Some required tools are still missing."
        echo "Install the missing uv, Cargo, or system tools manually, then re-run: just setup-tools"
        exit 1
    fi

    echo ""
    echo "✅ Tooling setup complete."

# Shell scripts: lint/check (non-mutating)
shell-check: _ensure-shellcheck _ensure-shfmt
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.sh')
    if [ "${#files[@]}" -gt 0 ]; then
        printf '%s\0' "${files[@]}" | xargs -0 -n4 uv run shellcheck -x
        printf '%s\0' "${files[@]}" | xargs -0 uv run shfmt -d
    else
        echo "No shell files found to check."
    fi

shell-fix: shell-fmt

# Shell scripts: format (mutating)
shell-fmt: _ensure-shfmt
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.sh')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "🧹 shfmt -w (${#files[@]} files)"
        printf '%s\0' "${files[@]}" | xargs -0 uv run shfmt -w
    else
        echo "No shell files found to format."
    fi
    # Note: justfiles are not shell scripts and are excluded from shellcheck

shell-lint: shell-check

# Spell check (typos)
spell-check: _ensure-typos
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    # Use -z for NUL-delimited output to handle filenames with spaces.
    #
    # Note: For renames/copies, `git status --porcelain -z` emits *two* NUL-separated paths.
    # The ordering can differ depending on the porcelain output, so we read both and
    # spell-check whichever one exists on disk.
    while IFS= read -r -d '' status_line; do
        status="${status_line:0:2}"
        filename="${status_line:3}"

        # For renames/copies, consume the second path token to keep parsing in sync.
        # Prefer the path that exists on disk to avoid passing stale paths to typos.
        if [[ "$status" == *"R"* || "$status" == *"C"* ]]; then
            if IFS= read -r -d '' other_path; then
                if [ ! -e "$filename" ] && [ -e "$other_path" ]; then
                    filename="$other_path"
                fi
            fi
        fi

        # Skip deletions (file may no longer exist).
        if [[ "$status" == *"D"* ]]; then
            continue
        fi

        files+=("$filename")
    done < <(git status --porcelain -z --ignored=no)
    if [ "${#files[@]}" -gt 0 ]; then
        # Exclude typos.toml itself: it intentionally contains allowlisted fragments.
        printf '%s\0' "${files[@]}" | xargs -0 -n100 typos --config typos.toml --force-exclude --exclude typos.toml --
    else
        echo "No modified files to spell-check."
    fi

tag version: python-sync
    uv run tag-release {{ version }}

tag-force version: python-sync
    uv run tag-release {{ version }} --force

# Focused local Rust buckets: unit tests plus rustdoc doctests.
test: test-unit test-doc

# Broad Rust correctness plus Python tooling tests.
test-all: test-rust test-python
    @echo "✅ All tests passed!"

test-cli: _ensure-cargo-nextest
    cargo nextest run --test cli --verbose

# Doctests must stay on cargo test; nextest does not run rustdoc doctests.
test-doc:
    cargo test --doc --verbose

test-examples: _ensure-cargo-nextest
    cargo nextest run --examples --verbose --no-tests pass

test-integration: _ensure-cargo-nextest
    cargo nextest run --tests --verbose

# Backward-compatible alias for the former recipe name.
test-lib: test-unit

# Broad Rust test workflow; doctests remain a separate cargo-test bucket.
test-rust: test-rust-ci test-doc
    @echo "✅ Rust tests passed!"

# Broad release-profile Rust CI bucket: lib unit and integration tests together.
test-rust-ci: _ensure-cargo-nextest
    cargo nextest run --release --profile ci --lib --tests --verbose

# Focused library unit tests for changed-surface validation.
test-unit: _ensure-cargo-nextest
    cargo nextest run --lib --verbose

test-python: _ensure-uv
    uv run python -m pytest

test-release: _ensure-cargo-nextest
    cargo nextest run --release --workspace
    cargo test --doc --release

test-slow: _ensure-cargo-nextest
    CDT_LARGE_DEBUG_MAX_RUNTIME_SECS=1800 cargo nextest run --cargo-profile perf --tests --features slow-tests --verbose

toml-check: toml-fmt-check toml-lint

toml-fix: toml-fmt

toml-fmt: _ensure-taplo
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.toml')

    if [ "${#files[@]}" -gt 0 ]; then
        taplo fmt "${files[@]}"
    else
        echo "No TOML files found to format."
    fi

toml-fmt-check: _ensure-taplo
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.toml')

    if [ "${#files[@]}" -gt 0 ]; then
        taplo fmt --check "${files[@]}"
    else
        echo "No TOML files found to check."
    fi

toml-lint: _ensure-taplo
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.toml')

    if [ "${#files[@]}" -gt 0 ]; then
        taplo lint "${files[@]}"
    else
        echo "No TOML files found to lint."
    fi

unused-deps: _ensure-cargo-machete
    cargo machete

# Update dependency requirements, locks, managed Cargo tools, and the active uv pin.
update: _ensure-cargo-install-update update-dependencies update-cargo-tools
    @echo "✅ Repository dependencies and tools updated."

# Advance Cargo dependency declarations and lockfile entries.
update-cargo-dependencies: _ensure-cargo-edit
    cargo upgrade --incompatible allow
    cargo update

# Update locally installed Cargo CLI tools and reconcile their pins plus the active uv version.
update-cargo-tools: _ensure-cargo-install-update _ensure-uv-available
    #!/usr/bin/env bash
    set -euo pipefail

    packages=(
        cargo-audit
        cargo-edit
        cargo-llvm-cov
        cargo-machete
        cargo-nextest
        cargo-update
        dprint
        git-cliff
        just
        rumdl
        taplo-cli
        typos-cli
        zizmor
    )
    cargo install-update --locked "${packages[@]}"
    uv run --locked update-tool-pins

# Advance Cargo and exact Python development requirements plus their lockfiles.
update-dependencies: _ensure-cargo-edit _ensure-uv-available update-cargo-dependencies update-python-dependencies

# Resolve latest exact Python development tools, retain ranged requirements, and sync.
update-python-dependencies: _ensure-uv-available
    uv run --locked update-python-dev-pins
    uv lock --upgrade
    uv sync --locked --group dev

# Synchronize deterministic release metadata from one stable GitHub tag.
update-version version: _ensure-uv
    uv run update-release-version {{ version }}

# File validation
validate-json: _ensure-jq
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.json')
    if [ "${#files[@]}" -gt 0 ]; then
        printf '%s\0' "${files[@]}" | xargs -0 -n1 jq empty
    else
        echo "No JSON files found to validate."
    fi

yaml-check: yaml-fmt-check yaml-lint

yaml-fix: _ensure-dprint
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.yml' '*.yaml' 'CITATION.cff')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "📝 dprint fmt (YAML/CFF, ${#files[@]} files)"
        dprint fmt --incremental=false "${files[@]}"
    else
        echo "No YAML files found to format."
    fi

yaml-fmt-check: _ensure-dprint
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.yml' '*.yaml' 'CITATION.cff')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "🔍 dprint check (YAML/CFF, ${#files[@]} files)"
        dprint check --incremental=false "${files[@]}"
    else
        echo "No YAML files found to check."
    fi

yaml-lint: _ensure-yamllint
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.yml' '*.yaml' 'CITATION.cff')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "🔍 yamllint (${#files[@]} YAML/CFF files)"
        uv run yamllint --strict -c .yamllint "${files[@]}"
    else
        echo "No YAML files found to lint."
    fi

zizmor: _ensure-zizmor
    #!/usr/bin/env bash
    set -euo pipefail
    zizmor_token="${ZIZMOR_GITHUB_TOKEN:-${GH_TOKEN:-}}"
    resolved_zizmor_token=""
    if [[ -z "$zizmor_token" ]] && command -v gh >/dev/null; then
        if resolved_zizmor_token="$(gh auth token 2>/dev/null)"; then
            zizmor_token="$resolved_zizmor_token"
        fi
    fi
    if [[ -n "$zizmor_token" ]]; then
        ZIZMOR_GITHUB_TOKEN="$zizmor_token" zizmor .github
    else
        echo "⚠️  No GitHub token available; zizmor online audits are disabled."
        zizmor --offline .github
    fi
