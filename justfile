# shellcheck disable=SC2148
# Justfile for causal-triangulations development workflow
# Install just: https://github.com/casey/just
# Usage: just <command> or just --list

# Use bash with strict error handling for all recipes
set shell := ["bash", "-euo", "pipefail", "-c"]

cargo_llvm_cov_version := "0.8.7"
cargo_nextest_version := "0.9.136"

# Common cargo-llvm-cov arguments for all coverage runs.
# Excludes benches/examples from reports while allowing integration tests to
# exercise library code.
_coverage_base_args := '''--ignore-filename-regex '(^|/)(benches|examples)/' \
  --workspace --lib --tests \
  --verbose'''

# Internal helpers: ensure external tooling is installed
_ensure-actionlint:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v actionlint >/dev/null || { echo "❌ 'actionlint' not found. See 'just setup' or https://github.com/rhysd/actionlint"; exit 1; }

_ensure-cargo-llvm-cov:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v cargo-llvm-cov >/dev/null; then
        echo "❌ 'cargo-llvm-cov' not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked cargo-llvm-cov --version {{cargo_llvm_cov_version}}"
        exit 1
    fi

_ensure-cargo-machete:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! cargo machete --version >/dev/null 2>&1; then
        echo "❌ 'cargo-machete' not found. Install with:"
        echo "   cargo install --locked cargo-machete"
        exit 1
    fi

_ensure-cargo-nextest:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! cargo nextest --version >/dev/null 2>&1; then
        echo "❌ 'cargo-nextest' not found. See 'just setup-tools' or install:"
        echo "   cargo install --locked cargo-nextest --version {{cargo_nextest_version}}"
        exit 1
    fi

_ensure-jq:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v jq >/dev/null || { echo "❌ 'jq' not found. See 'just setup' or install: brew install jq"; exit 1; }

_ensure-git-cliff:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v git-cliff >/dev/null || {
        echo "❌ 'git-cliff' not found. Install via Homebrew: brew install git-cliff"
        echo "   Or via Cargo: cargo install git-cliff"
        exit 1
    }

_ensure-dprint:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v dprint >/dev/null || { echo "❌ 'dprint' not found. See 'just setup' or install: brew install dprint"; exit 1; }

_ensure-rumdl:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v rumdl >/dev/null || { echo "❌ 'rumdl' not found. See 'just setup' or install: cargo install rumdl"; exit 1; }

_ensure-shellcheck:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v shellcheck >/dev/null || { echo "❌ 'shellcheck' not found. See 'just setup' or https://www.shellcheck.net"; exit 1; }

_ensure-shfmt:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v shfmt >/dev/null || { echo "❌ 'shfmt' not found. See 'just setup' or install: brew install shfmt"; exit 1; }

# Internal helper: ensure taplo is installed
_ensure-taplo:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v taplo >/dev/null || { echo "❌ 'taplo' not found. See 'just setup' or install: brew install taplo (or: cargo install taplo-cli)"; exit 1; }

# Internal helper: ensure typos-cli is installed
_ensure-typos:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v typos >/dev/null || { echo "❌ 'typos' not found. See 'just setup-tools' or install: cargo install typos-cli"; exit 1; }

# Internal helper: ensure uv is installed
_ensure-uv:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v uv >/dev/null || { echo "❌ 'uv' not found. See 'just setup' or https://github.com/astral-sh/uv"; exit 1; }

_ensure-yamllint:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v yamllint >/dev/null || { echo "❌ 'yamllint' not found. See 'just setup' or install: brew install yamllint (macOS), pip install yamllint, or uv tool install yamllint"; exit 1; }

# GitHub Actions workflow validation
action-lint: _ensure-actionlint
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '.github/workflows/*.yml' '.github/workflows/*.yaml')
    if [ "${#files[@]}" -gt 0 ]; then
        printf '%s\0' "${files[@]}" | xargs -0 actionlint
    else
        echo "No workflow files found to lint."
    fi

# Benchmarks
bench:
    cargo bench --workspace

bench-ci:
    cargo bench --profile perf --bench ci_performance_suite

# Smoke-test benchmark harnesses with minimal samples; not for performance data.
bench-smoke:
    cargo bench --workspace --bench cdt_benchmarks -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot
    cargo bench --workspace --bench ci_performance_suite -- --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot

# Compile benchmarks without running them, treating warnings as errors.
# This catches bench/release-profile-only warnings (e.g. debug_assertions-gated unused vars)
# that won't show up in normal debug-profile `cargo test` / `cargo clippy` runs.
bench-compile:
    RUSTFLAGS='-D warnings' cargo bench --workspace --no-run

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
    rumdl fmt --silent CHANGELOG.md "${archive_files[@]}"

changelog-unreleased version: _ensure-git-cliff _ensure-rumdl python-sync
    #!/usr/bin/env bash
    set -euo pipefail
    GIT_CLIFF_OFFLINE=true git-cliff --tag {{version}} -o CHANGELOG.md
    uv run postprocess-changelog
    uv run archive-changelog
    archive_files=()
    if [ -d docs/archive/changelog ]; then
        while IFS= read -r -d '' file; do
            archive_files+=("$file")
        done < <(find docs/archive/changelog -name '*.md' -print0)
    fi
    rumdl fmt --silent CHANGELOG.md "${archive_files[@]}"

tag version: python-sync
    uv run tag-release {{version}}

tag-force version: python-sync
    uv run tag-release {{version}} --force

changelog-tag version:
    just tag {{version}}

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

# CI simulation: comprehensive validation (matches .github/workflows/ci.yml).
# Rust unit/integration tests use nextest; doctests remain on cargo test because
# nextest does not execute rustdoc doctests.
ci: check bench-compile test-all examples-validate
    @echo "🎯 CI checks complete!"

# CI with performance baseline
ci-baseline tag="ci":
    just ci
    just perf-baseline {{tag}}

# CI + feature-gated slow/stress tests.
ci-slow: ci test-slow
    @echo "✅ CI + slow tests passed!"

# Clean build artifacts
clean:
    cargo clean
    rm -rf target/llvm-cov
    rm -rf coverage_report
    rm -rf coverage

# Code quality and formatting
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::cargo

# Pre-commit workflow: comprehensive validation (checks + tests + release + benches)
commit-check: check test-all test-release bench-compile
    @echo "🚀 Ready to commit! All checks passed."

# Coverage analysis for local development (HTML output)
coverage: _ensure-cargo-llvm-cov
    mkdir -p target/llvm-cov
    cargo llvm-cov {{_coverage_base_args}} --html --output-dir target/llvm-cov
    @echo "📊 Coverage report generated: target/llvm-cov/html/index.html"

# Coverage analysis for CI (Cobertura XML output for codecov/codacy)
coverage-ci: _ensure-cargo-llvm-cov
    mkdir -p coverage
    cargo llvm-cov {{_coverage_base_args}} --cobertura --output-path coverage/cobertura.xml

coverage-report *args: _ensure-uv
    uv run coverage_report {{args}}

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
    @echo "  just fix               # Apply formatters/auto-fixes (mutating)"
    @echo "  just check-fast        # Fast compile check (cargo check)"
    @echo "  just ci                # Full CI run (checks + all tests + examples + bench compile)"
    @echo "  just ci-baseline       # CI + save performance baseline"
    @echo "  just ci-slow           # CI + feature-gated slow/stress tests"
    @echo "  just commit-check      # Comprehensive pre-commit validation"
    @echo ""
    @echo "Testing:"
    @echo "  just test              # Lib and doc tests only (fast, used by CI)"
    @echo "  just test-integration  # Integration tests (tests/)"
    @echo "  just test-slow         # Feature-gated slow integration tests"
    @echo "  just test-all          # All tests (lib + doc + integration + Python)"
    @echo "  just test-python       # Python tests only (pytest)"
    @echo "  just test-release      # All tests in release mode"
    @echo "  just test-cli          # CLI integration tests only"
    @echo "  just test-examples     # Compile all examples as tests"
    @echo "  just examples          # Run all example scripts"
    @echo "  just examples-validate # Run examples and validate stable output markers"
    @echo "  just coverage          # Generate coverage report (HTML)"
    @echo "  just coverage-ci       # Generate coverage for CI (XML)"
    @echo ""
    @echo "Quality Check Groups:"
    @echo "  just lint          # All linting (code + docs + config)"
    @echo "  just lint-code     # Code linting (Rust, Python, Shell)"
    @echo "  just lint-docs     # Documentation linting (Markdown, Spelling)"
    @echo "  just lint-config   # Configuration validation (JSON, TOML, Actions)"
    @echo ""
    @echo "Benchmark System:"
    @echo "  just bench              # Run all benchmarks"
    @echo "  just bench-ci           # Run CI regression benchmarks with the perf profile"
    @echo "  just bench-smoke        # Smoke-test benchmark harnesses with minimal samples"
    @echo "  just bench-compile      # Compile benchmarks without running"
    @echo "  just bench-test-compile # Compile benches + release integration tests without running"
    @echo ""
    @echo "Performance Analysis:"
    @echo "  just perf-help     # Show performance analysis commands"
    @echo "  just perf-check    # Check for performance regressions"
    @echo "  just perf-baseline # Save current performance as baseline"
    @echo ""
    @echo "Changelog:"
    @echo "  just changelog                   # Generate/update CHANGELOG.md"
    @echo "  just changelog-unreleased <ver>  # Generate release changelog without a local tag"
    @echo "  just tag <ver>                   # Create git tag with changelog content"
    @echo ""
    @echo "Static Analysis:"
    @echo "  just semgrep             # Run repository-owned Semgrep rules"
    @echo "  just semgrep-test        # Test repository-owned Semgrep rules"
    @echo "  just unused-deps         # Check for unused direct Cargo dependencies"
    @echo "  just publish-check       # Validate crates.io metadata and dry-run publish"
    @echo ""
    @echo "Running:"
    @echo "  just run -- <args>  # Run with custom arguments"
    @echo "  just run-example    # Run with example arguments"
    @echo "  just run-simulation # Run basic_simulation.sh example script"
    @echo ""
    @echo "Note: Some recipes require external tools. Run 'just setup' for full environment setup."

# All linting: code + documentation + configuration
lint: lint-code lint-docs lint-config

# Code linting: Rust (fmt-check, clippy, docs, Semgrep) + Python (ruff, ty) + Shell scripts
lint-code: fmt-check clippy doc-check semgrep semgrep-test python-lint shell-lint

# Configuration validation: JSON, TOML, YAML, GitHub Actions workflows
lint-config: validate-json toml-check yaml-check action-lint

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
        files+=("$file")
    done < <(git ls-files -z '*.md')
    if [ "${#files[@]}" -gt 0 ]; then
        printf '%s\0' "${files[@]}" | xargs -0 -n100 rumdl check
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
        files+=("$file")
    done < <(git ls-files -z '*.md')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "📝 rumdl check --fix (${#files[@]} files)"
        printf '%s\0' "${files[@]}" | xargs -0 -n100 rumdl check --fix
    else
        echo "No markdown files found to format."
    fi

markdown-lint: markdown-check

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

perf-baseline tag="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    tag_value="{{tag}}"
    if [ -n "$tag_value" ]; then
        uv run performance-analysis --save-baseline --tag "$tag_value"
    else
        uv run performance-analysis --save-baseline
    fi

perf-check threshold="10.0": _ensure-uv
    uv run performance-analysis --threshold {{threshold}}

perf-compare file: _ensure-uv
    uv run performance-analysis --compare "{{file}}"

perf-help:
    @echo "Performance Analysis Commands:"
    @echo "  just perf-baseline [tag]    # Save current performance as baseline (optionally tagged)"
    @echo "  just perf-check [threshold] # Check for regressions (default: 10% threshold)"
    @echo "  just perf-report [file]     # Generate performance report"
    @echo "  just perf-trends [days]     # Analyze trends over N days (default: 7)"
    @echo "  just perf-compare <file>    # Compare with specific baseline file"
    @echo ""
    @echo "Examples:"
    @echo "  just perf-baseline v1.0.0   # Save tagged baseline"
    @echo "  just perf-check 5.0         # Check with 5% threshold"
    @echo "  just perf-report my_report.md # Save report to specific file"
    @echo "  just perf-trends 30         # Analyze last 30 days"

perf-report file="": _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    file_value="{{file}}"
    if [ -n "$file_value" ]; then
        uv run performance-analysis --report "$file_value"
    else
        timestamp=$(date +"%Y%m%d_%H%M%S")
        uv run performance-analysis --report "performance_report_${timestamp}.md"
        echo "📄 Report saved to: performance_report_${timestamp}.md"
    fi

perf-trends days="7": _ensure-uv
    uv run performance-analysis --trends {{days}}

python-sync: _ensure-uv
    uv sync --group dev

python-check: _ensure-uv
    uv run ruff format --check scripts/
    uv run ruff check scripts/
    just python-typecheck

# Python code quality
python-fix: _ensure-uv
    uv run ruff check scripts/ --fix
    uv run ruff format scripts/

python-lint: python-check

python-typecheck: _ensure-uv
    uv run ty check scripts/

# Repository-owned Semgrep rules for project-specific diagnostics.
semgrep: _ensure-uv
    uv run semgrep --error --strict --timeout 30 --config semgrep.yaml .

semgrep-test: _ensure-uv
    #!/usr/bin/env bash
    set -euo pipefail
    config_dir="$(mktemp -d "${TMPDIR:-/tmp}/ct-semgrep-config.XXXXXX")"
    cleanup() {
        find "$config_dir" -type l -exec unlink {} \;
        find "$config_dir" -depth -type d -exec rmdir {} +
    }
    trap cleanup EXIT

    # Semgrep directory test mode maps fixture paths to config paths, so mirror
    # each fixture to the shared config while keeping semgrep.yaml authoritative.
    while IFS= read -r -d '' fixture; do
        rel="${fixture#tests/semgrep/}"
        config_path="$config_dir/${rel%.*}.yaml"
        mkdir -p "$(dirname "$config_path")"
        ln -s "$PWD/semgrep.yaml" "$config_path"
    done < <(find tests/semgrep -type f ! -name '*.fixed' -print0)

    uv run semgrep scan --test --strict --config "$config_dir" tests/semgrep

# Running the binary
run *args:
    cargo run --bin cdt {{args}}

run-example:
    cargo run --bin cdt -- -v 32 -t 3

run-release *args:
    cargo run --release --bin cdt {{args}}

# Run example simulation script
run-simulation:
    ./examples/scripts/basic_simulation.sh

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

    os="$(uname -s || true)"

    have() { command -v "$1" >/dev/null 2>&1; }

    install_with_brew() {
        local formula="$1"
        if brew list --versions "$formula" >/dev/null 2>&1; then
            echo "  ✓ $formula (brew)"
        else
            echo "  ⏳ Installing $formula (brew)..."
            HOMEBREW_NO_AUTO_UPDATE=1 brew install "$formula"
        fi
    }

    brew_available=0
    if have brew; then
        brew_available=1
        echo "Using Homebrew (brew) to install missing tools..."
        install_with_brew uv
        install_with_brew jq
        install_with_brew taplo
        install_with_brew dprint
        install_with_brew rumdl
        install_with_brew yamllint
        install_with_brew shfmt
        install_with_brew shellcheck
        install_with_brew actionlint
        echo ""
    else
        echo "⚠️  'brew' not found. Skipping Homebrew installs."
        if [[ "$os" == "Darwin" ]]; then
            echo "Install Homebrew from https://brew.sh, or ensure required tools are on PATH."
        else
            echo "Install required tools via your system package manager, or ensure they are on PATH."
        fi
        echo "Required tools: uv, jq, taplo, dprint, rumdl, yamllint, shfmt, shellcheck, actionlint, git-cliff, typos, cargo-llvm-cov, cargo-nextest, cargo-machete"
        echo ""
    fi

    echo "Ensuring Rust toolchain + components..."
    if ! have rustup; then
        echo "❌ 'rustup' not found. Install Rust via https://rustup.rs and re-run: just setup-tools"
        exit 1
    fi
    rustup component add clippy rustfmt rust-docs rust-src
    rustup component add llvm-tools-preview
    echo ""

    echo "Ensuring cargo tools..."
    dprint_version="0.54.0"
    if ! have dprint || [[ "$(dprint --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" != "$dprint_version" ]]; then
        echo "  ⏳ Installing dprint ${dprint_version} (cargo)..."
        cargo install --locked dprint --version "${dprint_version}"
    else
        echo "  ✓ dprint ${dprint_version}"
    fi

    rumdl_version="0.1.96"
    if ! have rumdl || [[ "$(rumdl --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" != "$rumdl_version" ]]; then
        echo "  ⏳ Installing rumdl ${rumdl_version} (cargo)..."
        cargo install --locked rumdl --version "${rumdl_version}"
    else
        echo "  ✓ rumdl ${rumdl_version}"
    fi

    typos_version="1.44.0"
    if ! have typos || [[ "$(typos --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" != "$typos_version" ]]; then
        echo "  ⏳ Installing typos-cli ${typos_version} (cargo)..."
        cargo install --locked typos-cli --version "${typos_version}"
    else
        echo "  ✓ typos ${typos_version}"
    fi

    git_cliff_version="2.12.0"
    if ! have git-cliff || [[ "$(git-cliff --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" != "$git_cliff_version" ]]; then
        echo "  ⏳ Installing git-cliff ${git_cliff_version} (cargo)..."
        cargo install --locked git-cliff --version "${git_cliff_version}"
    else
        echo "  ✓ git-cliff ${git_cliff_version}"
    fi

    cargo_llvm_cov_version="{{cargo_llvm_cov_version}}"
    if ! have cargo-llvm-cov || [[ "$(cargo-llvm-cov --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" != "$cargo_llvm_cov_version" ]]; then
        echo "  ⏳ Installing cargo-llvm-cov ${cargo_llvm_cov_version} (cargo)..."
        cargo install --locked cargo-llvm-cov --version "${cargo_llvm_cov_version}"
    else
        echo "  ✓ cargo-llvm-cov ${cargo_llvm_cov_version}"
    fi

    cargo_nextest_version="{{cargo_nextest_version}}"
    if ! cargo nextest --version >/dev/null 2>&1 || [[ "$(cargo nextest --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" != "$cargo_nextest_version" ]]; then
        echo "  ⏳ Installing cargo-nextest ${cargo_nextest_version} (cargo)..."
        cargo install --locked cargo-nextest --version "${cargo_nextest_version}"
    else
        echo "  ✓ cargo-nextest ${cargo_nextest_version}"
    fi

    if ! cargo machete --version >/dev/null 2>&1; then
        echo "  ⏳ Installing cargo-machete (cargo)..."
        cargo install --locked cargo-machete
    else
        echo "  ✓ cargo-machete"
    fi

    echo ""
    echo "Verifying required commands are available..."
    missing=0

    cmds=(uv jq taplo dprint rumdl yamllint shfmt shellcheck actionlint git-cliff typos cargo-llvm-cov)
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
        if [ "$brew_available" -ne 0 ]; then
            echo "Fix the installs above (brew) and re-run: just setup-tools"
        else
            if [[ "$os" == "Darwin" ]]; then
                echo "Install Homebrew (https://brew.sh) or install the missing tools manually, then re-run: just setup-tools"
            else
                echo "Install the missing tools via your system package manager, then re-run: just setup-tools"
            fi
        fi
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
        printf '%s\0' "${files[@]}" | xargs -0 -n4 shellcheck -x
        printf '%s\0' "${files[@]}" | xargs -0 shfmt -d
    else
        echo "No shell files found to check."
    fi

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
        printf '%s\0' "${files[@]}" | xargs -0 shfmt -w
    else
        echo "No shell files found to format."
    fi
    # Note: justfiles are not shell scripts and are excluded from shellcheck

shell-lint: shell-check

shell-fix: shell-fmt

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

# Testing: fast tests (lib via nextest + rustdoc doctests via cargo test)
test: test-lib test-doc

# Testing: comprehensive suite (Rust runnable tests via nextest + doctests + Python)
test-all: test test-integration test-python
    @echo "✅ All tests passed!"

test-lib: _ensure-cargo-nextest
    cargo nextest run --lib --verbose

# Doctests must stay on cargo test; nextest does not run rustdoc doctests.
test-doc:
    cargo test --doc --verbose

test-integration: _ensure-cargo-nextest
    cargo nextest run -E 'kind(test)' --verbose

test-slow: _ensure-cargo-nextest
    cargo nextest run -E 'kind(test)' --features slow-tests --verbose

test-examples: _ensure-cargo-nextest
    cargo nextest run --examples --verbose --no-tests pass

test-cli: _ensure-cargo-nextest
    cargo nextest run --test cli --verbose

test-python: _ensure-uv
    uv run python -m pytest

test-release: _ensure-cargo-nextest
    cargo nextest run --release --workspace
    cargo test --doc --release

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

toml-check: toml-fmt-check toml-lint

toml-fix: toml-fmt

unused-deps: _ensure-cargo-machete
    cargo machete

yaml-check: yaml-fmt-check yaml-lint

yaml-fix: _ensure-dprint
    #!/usr/bin/env bash
    set -euo pipefail
    files=()
    while IFS= read -r -d '' file; do
        files+=("$file")
    done < <(git ls-files -z '*.yml' '*.yaml')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "📝 dprint fmt (YAML, ${#files[@]} files)"
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
    done < <(git ls-files -z '*.yml' '*.yaml')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "🔍 dprint check (YAML, ${#files[@]} files)"
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
    done < <(git ls-files -z '*.yml' '*.yaml')
    if [ "${#files[@]}" -gt 0 ]; then
        echo "🔍 yamllint (${#files[@]} files)"
        yamllint --strict -c .yamllint "${files[@]}"
    else
        echo "No YAML files found to lint."
    fi
