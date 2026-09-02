# Scripts Directory

This directory contains Python and shell tooling used by the CDT repository. Where possible, we keep these scripts aligned with the newer versions in the
`delaunay` repo so both projects can eventually share a single PyPI package (runnable via `uvx`).

## Prerequisites

- Python 3.14+
- `uv`

Install dev dependencies:

```bash
uv sync --group dev
```

## CLI entrypoints (recommended)

These are exposed via `pyproject.toml` so you can run them with `uv run ...`. All commands support `--help`.

### Changelog utilities

```bash
just changelog
just changelog-unreleased v0.1.0
uv run postprocess-changelog --help
uv run archive-changelog --help
uv run check-release-metadata --help
uv run update-release-version --help
uv run tag-release v0.1.0 --help
just tag v0.1.0
```

`just changelog` runs `git-cliff`, applies markdown hygiene, and archives completed minor release series under `docs/archive/changelog/`. Use
`just update-version vX.Y.Z` followed by `just changelog-unreleased vX.Y.Z` while preparing a release PR before the final tag exists.
`just release-metadata-check` validates synchronized package/CFF versions, the permanent concept DOI, and active documentation references;
`just release-version-check` additionally requires the matching dated changelog heading.

Shared subprocess wrappers apply a five-minute default timeout. Benchmark paths that may run longer pass their own explicit benchmark-specific timeout.

### Dependency and tool updates

```bash
just update
```

`update-python-dev-pins` resolves exact entries in `dependency-groups.dev` as one compatible set, leaves ranged requirements unchanged, applies all exact
pin changes in one uv transaction, and restores `pyproject.toml` and `uv.lock` if the mutation fails or changes unrelated manifest content.
`update-tool-pins` reconciles the root justfile with the Cargo-installed tools managed by `just setup` and the active uv version. The aggregate recipe also
updates Cargo requirements and lockfiles, upgrades those managed Cargo tools, refreshes the uv lock, and syncs the development environment.

### Benchmark utilities

`benchmark-utils` is a shared baseline/compare tool (ported from `delaunay`). It’s safe to use in CDT, but some subcommands assume baseline formats and
benchmark layouts that are still being unified across repos.

```bash
uv run benchmark-utils generate-baseline
uv run benchmark-utils compare --baseline baseline-artifact/baseline_results.txt
```

Release-to-release evidence uses the stricter `release-performance` entrypoint through canonical recipes:

```bash
just bench-latest
just bench-save-last
just bench-latest-vs-last
just performance-local
just performance-release v0.1.1 v0.1.0
just performance-doc
just performance-readme
just performance-github-assets v0.1.1 v0.1.0
```

`performance_artifacts.py` defines the fixed CSV schema, provenance binding, strict reload checks, and rollback-safe multi-file replacement.
`release_performance.py` owns stable-tag resolution, isolated worktrees, Criterion parsing, native archive safety, and rendering. The retained pair lives under
`target/bench-reports/`; report and README commands refuse to write when a member is missing or the pair does not match.

### Hardware utilities

```bash
uv run hardware-utils info
uv run hardware-utils kv
uv run hardware-utils info --json
```

### CDT-specific helpers

```bash
just coverage-ci
uv run performance-analysis --help
uv run coverage-report --help
uv run notebook-check --help

# Backwards-compatible alias
uv run coverage_report --help
```

`coverage-report` summarizes the Cobertura XML produced by `just coverage-ci`.

The notebook checker discovers `notebooks/**/*.ipynb` when no paths are supplied, validates notebook JSON, compiles code cells with cell-aware diagnostics,
rejects committed outputs/execution counts, and runs Ruff and ty over extracted code:

```bash
just notebook-lint
just notebook-check
just notebook-check-slow
just notebook-output-check
uv run --group notebooks notebook-check --summary --repo-root .
```

`just notebook-check` executes only the fast notebook set and writes executed notebooks/artifacts under `target/notebooks`; the slow recipe adds the heavier
analysis-cache notebook. `notebook-output-check` reuses the same JSON and output-hygiene implementation while skipping extracted-code tools.

## Shell helpers

```bash
./scripts/run_all_examples.sh
```

## Linting and tests

```bash
uv run ruff check scripts/ --fix
uv run ruff format scripts/

uv run pytest
```
