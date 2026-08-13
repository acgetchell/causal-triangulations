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
uv run tag-release v0.1.0 --help
just tag v0.1.0
```

`just changelog` runs `git-cliff`, applies markdown hygiene, and archives completed minor release series under `docs/archive/changelog/`. Use
`just changelog-unreleased vX.Y.Z` while preparing a release PR before the final tag exists. `just release-metadata-check` requires the CFF release date to
match the generated current-version changelog heading when present and verifies that the Python support package ships this README.

Shared subprocess wrappers apply a five-minute default timeout. Benchmark paths that may run longer pass their own explicit benchmark-specific timeout.

### Benchmark utilities

`benchmark-utils` is a shared baseline/compare tool (ported from `delaunay`). It’s safe to use in CDT, but some subcommands assume baseline formats and
benchmark layouts that are still being unified across repos.

```bash
uv run benchmark-utils generate-baseline
uv run benchmark-utils compare --baseline baseline-artifact/baseline_results.txt
```

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
