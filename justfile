# shellcheck disable=SC2148
# Justfile for causal-triangulations development workflow
# Install just: https://github.com/casey/just
# Usage: just <command> or just --list

# Use bash with strict error handling for all recipes
set positional-arguments
set lazy

_tools := "uv run --locked --group dev research-repo-tools"
_native_tools := "--tool cargo-nextest --tool git-cliff --tool rumdl"
_run := _tools + " toolchain run" + (if env("CDT_CI_NATIVE_TOOLS", "") == "1" { " " + _native_tools } else { "" }) + " --"
_release_cargo := _tools + " toolchain run --tool cargo --"
_release_tools := "uvx --no-config --isolated --managed-python --python 3.15 --exclude-newer 2026-11-26T03:45:46Z --from research-repo-tools==0.3.3 research-repo-tools"
_release_tag := shell("cargo metadata --offline --locked --no-deps --format-version 1 | jq -er " + quote('.packages[] | select(.name == "causal-triangulations") | "v" + .version'))
_owner_dir := if env("CDT_RELEASE_OWNER", "") != "" { env("CDT_RELEASE_OWNER") } else { env("HOME", home_directory()) / ".local/state/causal-triangulations" / _release_tag }
_owner := quote(_owner_dir)
_checkpoint := "target/bench-reports/release-checkpoint"
_commit := '$(git --no-pager rev-parse --verify HEAD)'
_final := _owner + "/final/" + _commit
_notebooks := "uv run --locked --group dev --group notebooks research-repo-tools"
_scanner := "uv run --locked --no-active --group dev research-repo-tools"
_actions_inputs := quote(".github/workflows") + " " + quote(".github/actions/setup-toolchain/action.yml")

# Semgrep misreads Python 3.15's positive RLIM_INFINITY on Linux.
# Keep its inherited stack finite until https://github.com/semgrep/semgrep/issues/11979 is fixed.
_semgrep_stack_limit := if os() == "linux" { 'if [[ "$(ulimit -Hs)" == unlimited ]]; then ulimit -s 1048576; fi;' } else { "" }

set shell := ["bash", "-euo", "pipefail", "-c"]

# Common cargo-llvm-cov arguments for all coverage runs.
# Excludes benches/examples from reports while allowing integration tests to
# exercise library code.
_coverage_base_args := '''--ignore-filename-regex '(^|/)(benches|examples)/' \
  --workspace --lib --tests \
  --verbose'''

alias changelog-release := changelog-unreleased
alias python-fixtures-check := python-check
alias python-source-check := python-check
alias update-python-deps := update-python-dependencies

# Synchronize once before validators and independent runtime lanes.
[private]
_ci-environment:
    uv sync --locked --group dev --group notebooks

# Keep all shared Cargo outputs and scientific execution in one lane.
[private]
_ci-native:
    UV_NO_SYNC=1 just test-rust-ci test-doc
    UV_NO_SYNC=1 just notebook-execute-fast
    UV_NO_SYNC=1 just allocation-check examples-validate

# Preserve the complete static and compilation gate before runtime workloads.
[private]
_ci-policy: ci-repository-tooling _ci-compile

# Compile and lint all target-dependent code on each host.
[private]
_ci-compile: clippy-all-targets doc-check bench-compile

# Python tests use temporary outputs and the already synchronized environment.
[private]
_ci-python:
    UV_NO_SYNC=1 just test-python -m 'not semgrep_report'

# Independent consumers overlap without synchronizing their shared environment.
[parallel]
[private]
_ci-runtime: _ci-python _ci-native

# Internal helpers: ensure external tooling is installed
_ensure-jq:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v jq >/dev/null || { echo "❌ 'jq' not found. See 'just setup' or install: brew install jq"; exit 1; }

# GitHub Actions workflow validation
action-lint:
    # actionlint 1.7.12 predates $/ syntax (upstream issue #711); ignore only this exact valid reference.
    {{ _tools }} files run --include '.github/workflows/*.yml' --include '.github/workflows/*.yaml' -- actionlint -ignore '^specifying action "\$/\.github/actions/setup-toolchain" in invalid format because ref is missing\.'

# Validate exact external Action identities in workflows and composites.
actions-allowlist-check:
    {{ _tools }} actions allowlist --policy tooling/selected-actions.json .github/workflows .github/actions

# Check coupled GitHub Actions pins offline using the provisioned environment.
actions-check:
    uv run --locked --no-sync --no-python-downloads --offline research-repo-tools actions check --policy .github/action-updates.toml {{ _actions_inputs }}

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
bench-compare baseline="last":
    {{ _tools }} performance compare target/criterion target/criterion --baseline-sample "$1" --format markdown

# Compile all benchmark harnesses with warnings denied.
bench-compile:
    CARGO_BUILD_WARNINGS=deny {{ _run }} cargo bench --workspace --no-run

# Produce the correctness-gated current release signal.
bench-latest: benchmark-input-check
    {{ _run }} cargo bench --locked --profile perf --bench ci_performance_suite -- --noplot

# Produce the current release signal and compare it with a saved local baseline.
bench-latest-vs-last baseline="last": bench-latest && (bench-compare baseline)

# Produce a named native Criterion baseline after validating benchmark inputs.
bench-save-baseline tag: benchmark-input-check
    {{ _run }} cargo bench --locked --profile perf --bench ci_performance_suite -- --save-baseline "$1" --noplot

# Refresh the conventional local comparison baseline.
bench-save-last: (bench-save-baseline "last")

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

# Build the public documentation from a packaged Cargo library.
cargo-docs package *args:
    {{ _run }} research-repo-tools validation cargo-docs "$@"

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

# Preview the complete changelog history rebuild, or apply it with --apply.
changelog-rebuild *args:
    {{ _run }} research-repo-tools changelog rebuild "$@"

# Generate prospective notes from the prepared citation date or an explicit override.
changelog-unreleased tag date="":
    {{ _run }} research-repo-tools changelog generate --tag "$1" ${2:+--date "$2"}

# Check (non-mutating): run all linters/validators
check: lint-code lint-docs lint-config
    @echo "✅ Checks complete!"

# Fast compile check (no binary produced)
check-fast:
    {{ _run }} cargo check

# Run all GitHub-equivalent validators, tests, examples, and benchmark compilation.
ci: _ci-environment _ci-policy _ci-runtime
    @echo "🎯 CI checks complete!"

# Hosted native compile, runtime and Python portability checks.
ci-native: _ci-environment _ci-compile _ci-runtime

# Source, dependency policy and formatting checks shared by the native runners.
ci-repository-tooling: justfile-fmt-check action-lint actions-allowlist-check actions-check zizmor markdown-check spell-check performance-check validate-json toml-fmt-check toml-lint yaml-fmt-check yaml-lint citation-check python-check notebook-lint shell-check semgrep semgrep-test fmt-check
    CDT_CI_SEMGREP_REPORT=target/security/semgrep uv run --locked --group dev pytest -q -m semgrep_report tests/tooling/test_shared_tooling.py

# CI + feature-gated slow/stress tests.
ci-slow: ci test-slow
    @echo "✅ CI + slow tests passed!"

# Validate CITATION.cff against the Citation File Format schema and release metadata gate.
citation-check: release-metadata-check
    uvx --from cffconvert==2.0.0 cffconvert --validate -i CITATION.cff

# Preview obsolete managed tools and retained coverage runs, or remove them with --apply.
clean *args:
    uv run --locked --no-sync --no-python-downloads research-repo-tools clean "$@"

# Remove local Cargo build artifacts and generated coverage reports.
clean-build:
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
commit-check: check bench-compile test-all test-release
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
default: help

# Preview a declared source change; pass --apply to reconcile its manifest and lock.
[positional-arguments]
dependency-source name *args:
    {{ _run }} research-repo-tools deps source "$@"

# Build public and private API documentation with warnings denied.
doc-check:
    RUSTDOCFLAGS='-D warnings' {{ _run }} cargo doc --workspace --no-deps --document-private-items

# Build and run Cargo examples, checking their semantic output contracts.
examples-validate:
    {{ _run }} research-repo-tools validation cargo-examples tooling/examples.toml

# Fix (mutating): apply formatters/auto-fixes
fix: justfile-fmt toml-fix fmt python-fix shell-fix markdown-fix yaml-fix
    @echo "✅ Fixes applied!"

# Format Rust source files.
fmt:
    {{ _run }} cargo fmt --all

# Check Rust formatting without modifying sources.
fmt-check:
    {{ _run }} cargo fmt --all -- --check

# List available recipes and their arguments.
help:
    @just --justfile {{ quote(justfile()) }} --list

# Apply canonical Justfile formatting.
justfile-fmt:
    just --fmt --unstable

# Check Justfile formatting without modifying it.
justfile-fmt-check:
    just --fmt --check --unstable

# Code linting: Rust (fmt-check, clippy, docs, Semgrep) + Python (ruff, ty) + Shell scripts
lint-code: fmt-check clippy doc-check semgrep semgrep-test python-check shell-check

# Configuration validation: JSON, TOML, YAML/CFF, GitHub Actions workflows
lint-config: justfile-fmt-check validate-json toml-check yaml-check citation-check action-lint actions-allowlist-check actions-check zizmor

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
    MPLBACKEND=Agg {{ _notebooks }} notebooks launch "$1"

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
    {{ _run }} {{ _notebooks }} notebooks execute --output-dir "$1" notebooks/02_analysis_caches.ipynb

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

# Package a historical exact-tag baseline in the original shared format.
performance-baseline tag:
    {{ _run }} research-repo-tools performance baseline tooling/benchmark.toml "$1" "causal-triangulations-$1-cdt-baseline-v2.tar.gz"

# Check retained release reports, accepting an empty inventory before the first comparison.
performance-check:
    {{ _tools }} performance promote tooling/performance-report.toml --check
    {{ _tools }} performance promote tooling/series-report.toml --check

# Time the four exploratory CLI workloads, retaining every sample in a fresh JSON file.
performance-command-time output:
    {{ _run }} research-repo-tools performance command-time tooling/command-timing.toml --output "$1"

# Promote supplied new evidence, or rerender the retained report offline.
performance-doc *args:
    {{ _tools }} performance promote tooling/performance-report.toml "$@"

# Compare two explicit new-format release assets without measuring.
performance-github-assets current baseline:
    {{ _tools }} performance assets "$1" "$2" --repository acgetchell/causal-triangulations --asset-template 'causal-triangulations-{tag}-cdt-baseline-v2.tar.gz' --payload target/bench-reports/assets.comparison.json --manifest target/bench-reports/assets.evidence.json --report target/bench-reports/assets.md

# Freeze or recover the original measurement and regenerate its release report.
performance-capture owner=_owner_dir:
    {{ _run }} research-repo-tools performance capture tooling/performance-release.toml --owner "$1" --promote

# Select release history and validate the complete series declaration without measuring.
performance-prepare:
    {{ _run }} research-repo-tools performance prepare tooling/performance-release.toml --release-intent

# Render the release README from the captured run.
performance-readme *args:
    {{ _tools }} performance readme tooling/performance-release.toml "$@"

# Acquire the prepared release workload once and retain its evidence outside the checkout.
performance-release owner=_owner_dir: performance-prepare
    {{ _run }} research-repo-tools performance release tooling/performance-release.toml --release-intent --checkpoint {{ quote(_checkpoint) }} --owner "$1"

# Resume complete units against the original preparation in the acquisition checkout.
performance-resume checkpoint=_checkpoint owner=_owner_dir:
    {{ _run }} research-repo-tools performance release tooling/performance-release.toml --release-intent --resume --checkpoint "$1" --owner "$2"

# Check package metadata and dry-run crates.io publication.
publish-check: release-package

# Check all Python source and negative fixtures with Ruff and Ty.
python-check:
    {{ _tools }} python check

# Apply Ruff fixes and formatting to all Python files.
python-fix:
    {{ _tools }} python fix

# Synchronize the locked development environment.
python-sync:
    uv sync --locked --group dev

# Type-check all Python files with strict Ty diagnostics.
python-typecheck:
    {{ _tools }} python typecheck

# Verify the original staged release batch in the checkout-free tag workflow.
release-benchmarks tag: (release-draft tag)
    state={{ _final }}; sha=$(sha256sum < "$state/transfer.json"); gh workflow run release-benchmarks.yml --repo acgetchell/causal-triangulations --ref "$1" -f "tag=$1" -F "transfer=@$state/transfer.json" -f "digest=${sha%% *}"

# Verify one dispatched registry candidate using a private work directory on the runner.
[private]
_registry-candidate: release-dependency-builds
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "$RELEASE_TAG" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
    [[ "$RELEASE_COMMIT" == {{ _commit }} && "$REGISTRY_WORK" == /* ]]
    mkdir "$REGISTRY_WORK"
    {{ _release_cargo }} cargo package --locked --package causal-triangulations --no-verify --target-dir "$REGISTRY_WORK"
    cargo_version=$(cargo --version)
    rustc_version=$(rustc --version --verbose)
    jq -n --arg cargo "$cargo_version" --arg rustc "$rustc_version" '{cargo: $cargo, rustc: $rustc}' > "$REGISTRY_WORK/tools.json"
    uv run --locked --no-sync python -I tooling/verify_registry_candidate.py
    {{ _tools }} release registry "$RELEASE_TAG" --expect absent

# Populate the managed Cargo cache used by the locked release dependency proof.
release-cargo-fetch *args:
    {{ _release_cargo }} cargo fetch --locked "$@"

# Authenticate declared registry graphs and this machine's installed dependency bytes.
release-dependencies:
    {{ _release_cargo }} research-repo-tools release dependencies

# Prepare native source receipts and registry inputs before release provenance checks.
release-dependency-builds: python-sync release-cargo-fetch
    {{ _release_cargo }} research-repo-tools release dependency-builds

# Capture the original prepared draft and print the retained target path.
release-draft tag:
    state={{ _final }}; jq -e --arg tag "$1" --arg commit "{{ _commit }}" '.tag == $tag and .commit == $commit' "$state/policy.json" >/dev/null
    state={{ _final }}; digest=$(sha256sum < "$state/selection.json"); {{ _release_tools }} release assets retrieve tooling/draft-receipt.toml --tag "$1" --commit "{{ _commit }}" --expected-native-sha256 "${digest%% *}" --output "$state/draft"
    @printf '%s\n' {{ _final }}/draft/target.json

# Freeze the original Step 1 acquisition and signed native evidence for the clean release commit.
release-evidence: (_release-candidate "select")

# Revalidate the original benchmark and Cargo evidence without sampling or building.
[private]
_release-candidate mode:
    #!/usr/bin/env bash
    set -euo pipefail
    owner={{ _owner }}
    state={{ _final }}
    commit={{ _commit }}
    args=()
    output="$state/acceptance.json"
    if [[ "$1" != select || -e "$output" ]]; then
        args+=(--acceptance "$output")
        output="$state/revalidated.json"
    fi
    {{ _release_cargo }} {{ _release_tools }} release evidence candidate tooling/release-evidence.toml \
        --required-checks --checkout "$PWD" --commit "$commit" --policy "$owner/policy.json" --policy-output "$state/policy.json" \
        --payload "$owner/run.json" --manifest "$owner/evidence.json" --acquisition-sha256 "$(cat "$owner/acquisition.sha256")" \
        --publication-receipt "$owner/publication.json" --publication-sha256 "$(cat "$owner/publication.sha256")" \
        --native-output "$state/selection.json" --output "$output" --format summary ${args[@]+"${args[@]}"}

# Check the release PR's clean commit against Step 1 evidence before merge.
release-preflight commit="":
    owner={{ _owner }}; {{ _release_tools }} release evidence candidate-preflight --checkout "$PWD" --commit "${1:-{{ _commit }}}" --policy "$owner/policy.json" --payload "$owner/run.json" --manifest "$owner/evidence.json" --acquisition-sha256 "$(cat "$owner/acquisition.sha256")" --publication-receipt "$owner/publication.json" --publication-sha256 "$(cat "$owner/publication.sha256")"

# Validate release-version/date synchronization, concept DOI, and environment metadata.
release-metadata-check:
    {{ _tools }} release check

# Print release notes from the active changelog or archives.
release-notes tag:
    {{ _tools }} changelog notes "$1"

# Verify the clean default and all-feature packages without uploading.
release-package directory="":
    {{ _release_cargo }} research-repo-tools validation cargo-package --package causal-triangulations ${1:+--output "$1"}

# Publish the explicitly reviewed GitHub draft and verify the resulting asset identities.
release-publish tag: (_release-candidate "reviewed") (release-draft tag)
    state={{ _final }}; {{ _release_cargo }} {{ _release_tools }} release publish "$1" --approve --target "$state/draft/target.json" --asset-transfer "$state/transfer.json" --transfer-sha256 "$(cat "$state/reviewed.sha256")"

# Review the original draft and freeze its verified asset IDs and digests.
release-review tag: (_release-candidate "reviewed") (release-draft tag)
    #!/usr/bin/env bash
    set -euo pipefail
    state={{ _final }}
    digest=$(sha256sum < "$state/transfer.json")
    digest=${digest%% *}
    {{ _release_tools }} release assets receive --target "$state/draft/target.json" --transfer "$state/transfer.json" --transfer-sha256 "$digest" --output "$state/reviewed"
    cmp "$state/policy.json" "$state/reviewed/policy.json"
    cmp "$state/acceptance.json" "$state/reviewed/acceptance.json"
    cmp "$state/package/causal-triangulations-${1#v}.crate" "$state/reviewed/causal-triangulations-${1#v}.crate"
    {{ _release_tools }} release evidence archive verify --policy "$state/policy.json" --acceptance "$state/acceptance.json" \
        --archive "$state/reviewed/causal-triangulations-$1-cdt-series.tar.gz" --target "$state/draft/target.json"
    if [[ -e "$state/reviewed.sha256" ]]; then test "$(cat "$state/reviewed.sha256")" = "$digest"; else (set -C; printf '%s\n' "$digest" > "$state/reviewed.sha256"); fi
    jq '.target, .assets' "$state/transfer.json"

# Attach the retained signed Cargo package to the authenticated draft.
release-stage tag=_release_tag: (release-draft tag)
    #!/usr/bin/env bash
    set -euo pipefail
    owner={{ _owner }}
    commit={{ _commit }}
    state="$owner/final/$commit"
    asset="causal-triangulations-$1-cdt-series.tar.gz"
    {{ _release_cargo }} {{ _release_tools }} release evidence verify tooling/release-evidence.toml --checkout "$PWD" --commit "$commit" --version "${1#v}" \
        --expected "$state/selection.json" --output "$state/rechecked-native.json" --retain-crate "$state/package"
    {{ _release_tools }} --root "$owner" release evidence archive derive --payload run.json --manifest evidence.json \
        --policy "final/$commit/policy.json" --acceptance "final/$commit/acceptance.json" --output "final/$commit/$asset"
    {{ _release_cargo }} {{ _release_tools }} release assets stage tooling/release-assets.toml --checkout "$PWD" --tag "$1" --commit "$commit" \
        --target "$state/draft/target.json" --journal "$state/staging.json" --output "$state/transfer.json" --approve \
        --asset "$state/package/causal-triangulations-${1#v}.crate" --asset "$state/$asset" --asset "$state/policy.json" --asset "$state/acceptance.json"

# Inspect retained local evidence without accepting a candidate or mutating its release.
release-status:
    #!/usr/bin/env bash
    set -euo pipefail
    owner={{ _owner }}
    state={{ _final }}
    printf 'Release: %s\nOwner: %s\nCommit: %s\n' {{ quote(_release_tag) }} "$owner" "{{ _commit }}"
    for file in {{ quote(_checkpoint) }} target/bench-reports/{preparation,run,evidence}.json \
        "$owner"/{acquisition,publication}.sha256 "$state"/{selection,acceptance}.json \
        "$state/draft/target.json" "$state"/{staging,transfer}.json "$state/reviewed.sha256"; do
        if [[ -e "$file" ]]; then printf 'retained  %s\n' "$file"; else printf 'missing   %s\n' "$file"; fi
    done
    if [[ -f "$state/draft/target.json" ]]; then
        jq '{repository, tag, commit, release_id}' "$state/draft/target.json"
    fi

# Dispatch registry approval with the original reviewed assets and the successful published event.
release-upload tag:
    #!/usr/bin/env bash
    set -euo pipefail
    tag="$1"
    commit={{ _commit }}
    owner={{ _owner }}
    state={{ _final }}
    repo=acgetchell/causal-triangulations
    release_id=$(jq -er --arg tag "$tag" --arg sha "$commit" --arg repo "$repo" 'select(.tag == $tag and .commit == $sha and .repository == $repo) | .release_id' "$state/draft/target.json")
    prior=$(gh api --paginate --slurp "repos/$repo/actions/workflows/publish-crates.yml/runs?event=workflow_dispatch&head_sha=$commit&per_page=100" \
        | jq -r --arg title "Publish $tag to crates.io" '.[].workflow_runs[] | select(.display_title == $title) | .html_url')
    if [[ -n "$prior" ]]; then
        printf 'A registry dispatch already exists. Inspect it and run just release-verify %s before considering a full rerun:\n%s\n' "$tag" "$prior" >&2
        exit 1
    fi
    reviewed=$(cat "$state/reviewed.sha256")
    transfer=$(sha256sum < "$state/transfer.json")
    [[ "${transfer%% *}" == "$reviewed" ]]
    run=$(gh api --paginate --slurp "repos/$repo/actions/workflows/publish-crates.yml/runs?event=release&head_sha=$commit&per_page=100" \
        | jq -ce --arg title "Verify published release $tag (release $release_id)" '[.[].workflow_runs[] | select(.display_title == $title)] | if length == 1 then .[0] | select(.status == "completed" and .conclusion == "success") else error("expected one published-release gate") end')
    run_id=$(jq -er '.id' <<< "$run")
    attempt=$(jq -er '.run_attempt' <<< "$run")
    name="published-release-$run_id-$attempt"
    artifact=$(gh api --paginate --slurp "repos/$repo/actions/runs/$run_id/artifacts?per_page=100" \
        | jq -ce --arg name "$name" '[.[].artifacts[] | select(.name == $name)] | if length == 1 then .[0] | select(.expired == false) else error("expected one event receipt") end')
    accepted=$(sha256sum < "$state/acceptance.json")
    proof=$(jq -cn --argjson run "$run_id" --argjson attempt "$attempt" --argjson artifact "$artifact" --arg transfer "$reviewed" \
        --arg accepted "${accepted%% *}" --arg acquisition "$(cat "$owner/acquisition.sha256")" --arg publication "$(cat "$owner/publication.sha256")" \
        '{run:$run,attempt:$attempt,artifact:$artifact.id,digest:$artifact.digest,transfer:$transfer,acceptance:$accepted,acquisition:$acquisition,publication:$publication}')
    gh workflow run publish-crates.yml --repo "$repo" --ref "$tag" -f "tag=$tag" -F "transfer=@$state/transfer.json" -f "proof=$proof"

# Compare the immutable GitHub crate asset with the exact crates.io version.
release-verify tag:
    {{ _release_cargo }} {{ _release_tools }} release verify "$1" --target {{ _final }}/draft/target.json --require-crate-asset --attempts 7 --interval 10

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
semgrep output='target/security/semgrep':
    {{ _semgrep_stack_limit }} {{ _scanner }} semgrep scan --rust-docs --include '*.rs' --include '*.py' --include '*.yml' --include '*.yaml' --include '*.md' --include '*.sh' --include '*.ipynb' --exclude 'tests/semgrep/**' --exclude 'docs/archive/**' --exclude 'docs/archives/**' --exclude .codecov.yml --exclude .coderabbit.yml --exclude .github/actions/setup-toolchain/action.yml --exclude .github/dependabot.yml --exclude semgrep.yaml --output "$1"

# Validate annotated repository-owned Semgrep fixtures.
semgrep-test:
    {{ _semgrep_stack_limit }} {{ _scanner }} semgrep check-fixtures --rust-docs

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
[arg('commit', long)]
[arg('dry_run', long='dry-run', value='--dry-run')]
[arg('force', long, value='--force')]
tag version commit="" dry_run="" force="": (_release-candidate "reviewed")
    head={{ _commit }}; [[ -z "$2" || "$2" == "$head" ]]; jq -e --arg tag "$1" --arg commit "$head" '.tag == $tag and .commit == $commit' {{ _final }}/policy.json >/dev/null; {{ _release_cargo }} {{ _release_tools }} changelog tag "$1" --commit "$head" ${3:+"$3"} ${4:+"$4"}

# Replace a local Git tag from release notes (maintainer operation).
tag-force version *args:
    just tag "$@" --force

# Preview the annotated release tag without changing Git state.
tag-preview version *args:
    {{ _release_cargo }} research-repo-tools changelog tag "$@" --dry-run

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
test-python *args:
    {{ _semgrep_stack_limit }} {{ _run }} uv run --locked python -m pytest "$@"

# Run release-mode Rust tests and doctests.
test-release: test-rust-ci
    {{ _run }} cargo test --doc --release

# Broad Rust test workflow; doctests remain a separate cargo-test bucket.
test-rust: test-rust-ci test-doc
    @echo "✅ Rust tests passed!"

# Broad release-profile Rust CI bucket: lib unit and integration tests together.
test-rust-ci:
    {{ _run }} cargo nextest run --release --profile ci --lib --tests --verbose

# Run the bounded toroidal stress test through the perf profile.
test-slow:
    CDT_LARGE_DEBUG_MAX_RUNTIME_SECS=1800 {{ _run }} cargo nextest run --cargo-profile perf --test large_scale_debug --features slow-tests --verbose

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

# Reconcile coupled GitHub Actions pins; use --dry-run to preview.
update-actions *args:
    uv run --locked --only-group tooling --inexact research-repo-tools actions update --policy .github/action-updates.toml {{ _actions_inputs }} "$@"

# Advance Cargo dependency declarations and lockfile entries.
update-cargo-dependencies:
    if [ -f Cargo.toml ]; then uv run --locked --only-group tooling --inexact research-repo-tools toolchain run -- cargo upgrade --incompatible allow; fi
    if [ -f Cargo.toml ]; then uv run --locked --only-group tooling --inexact research-repo-tools toolchain run -- cargo update; fi

# Update locally installed Cargo CLI tools and reconcile their pins plus the active uv version.
update-cargo-tools:
    uv run --locked --only-group tooling --inexact research-repo-tools toolchain upgrade

# Advance Cargo and exact Python development requirements plus their lockfiles.
update-dependencies: update-cargo-dependencies update-python-dependencies

# Advance root dev pins, then refresh and sync the unified Python environment.
update-python-dependencies:
    uv run --isolated --locked --only-group tooling research-repo-tools deps update-python --all-projects --adopt-supplier tooling

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
