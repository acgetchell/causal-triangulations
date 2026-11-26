# Tooling configuration

The `research-repo-tools` pin in `pyproject.toml` supplies generic maintenance and the shared pytest, Ruff, and Ty versions.
This directory contains CDT's tooling configuration. RRT checks all three CodeQL subpaths against the reviewed tag and commit in
`.github/action-updates.toml`.
Consumer integration tests live under [`tests/tooling/`](../tests/tooling/).
`release-evidence.toml` declares the required main-push jobs and signed Cargo archive. `draft-receipt.toml` identifies the original tag workflow.
`release-assets.toml` binds staging to the CDT repository and expected draft title. `performance-release.toml` selects the measurement,
report and README declarations for the series in `benchmark-series/cdt-release-signal.toml`.
`verify_registry_candidate.py` joins CDT's operator dispatch and published-event receipt to its four original release assets.
The installed RRT package authenticates the artifact, signed crate, benchmark archive, series history and source binding.
[RRT #285](https://github.com/acgetchell/research-repo-tools/issues/285) tracks moving the published-event verification upstream.
The [release guide](../docs/RELEASING.md) describes package verification, draft review and recovery.
Tooling, notebooks and Semgrep share the root Python 3.15 environment. RRT's Semgrep extra supplies the supported range and compatibility overrides.
The root manifest applies those overrides and requires a scanner wheel. The lock selects the exact version.
`selected-actions.json` lists approved workflow and composite Actions. Zizmor and Semgrep check commit pins and readable version comments. See the
[ownership catalog](../docs/dev/tooling-alignment.md#shared-tooling-retirement-catalog) for retained policy and upstream gaps.

Bootstrap from the repository root:

```bash
uv run --locked --only-group tooling research-repo-tools setup
just check
just test-python
```

The tests exercise CDT's configuration and command wiring: review/update/release recipes, Python fixture coverage, release metadata, and benchmark/example
contracts. [`test_release_benchmarks.py`](../tests/tooling/test_release_benchmarks.py) runs workflow shell and installed release commands with local GitHub
responses to check the original draft ID, staged asset IDs and raw archive bytes. Its synthetic Criterion samples exercise the scientific workload inventory
and report wiring.
The report-to-README integration verifies absolute timings, confidence bounds, and relative changes together. Tests never contact CodeRabbit.

Generic parser, renderer, subprocess, and notebook infrastructure tests belong to `research-repo-tools` and are not duplicated here.

`test_shared_adoption.py` exercises the published package with CDT's notebook selector, exact Action allowlist, inherited Python tool profile, four
exploratory timing workloads, and serial/parallel CI graph. `examples.toml` adds semantic output contracts to Cargo-discovered examples;
`command-timing.toml` owns the exploratory simulation arguments. The report initialization policy permits an empty first-use inventory and rejects
orphaned evidence or a README publication config before the first retained comparison.

Use `just --list` for maintained commands and [the command guide](../docs/dev/DEVELOPING.md) for details.
Changelog, release metadata, coverage, dependency/tool updates, notebooks, Semgrep fixtures, and performance evidence all use shared APIs.
CDT-specific workloads and output expectations live in `benches/`, `examples/`, `notebooks/`, and `tooling/`.

The owner chose to retire historical performance reports and legacy formats during the September 2026 migration.
[Performance testing](../docs/BENCHMARKING.md) describes the fresh baseline workflow and absolute timing tables.
