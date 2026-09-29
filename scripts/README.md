# Tooling integration tests

Generic maintenance uses the published `research-repo-tools==0.1.7` package.
This directory contains only consumer integration tests; CDT no longer builds a Python support package or maintains local maintenance CLIs.

Bootstrap from the repository root:

```bash
uv run --locked --only-group tooling research-repo-tools setup
just check
just test-python
```

The tests exercise CDT's configuration and command wiring: review/update/release recipes, Python fixture coverage, release metadata, and benchmark/example
contracts. The report-to-README integration verifies absolute timings, confidence bounds, and relative changes together. They never contact CodeRabbit.

Generic parser, renderer, subprocess, and notebook infrastructure tests belong to `research-repo-tools` and are not duplicated here.

Use `just --list` for maintained commands and [the command guide](../docs/dev/DEVELOPING.md) for details.
Changelog, release metadata, coverage, dependency/tool updates, notebooks, Semgrep fixtures, and performance evidence all use shared APIs.
CDT-specific workloads and output expectations live in `benches/`, `examples/`, `notebooks/`, and `tooling/`.

The owner chose to retire historical performance reports and legacy formats during the September 2026 migration.
[Performance testing](../docs/BENCHMARKING.md) describes the fresh baseline workflow and absolute timing tables.
