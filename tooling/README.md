# Tooling integration tests

Generic maintenance uses the published `research-repo-tools==0.2.3` package.
This directory contains CDT's tooling configuration and consumer integration tests under `tests/`; CDT maintains no local maintenance CLIs.
The independent `semgrep/` project retains Python 3.14, the exact scanner pin, and its MCP security override while root tooling and notebooks use Python 3.15.
`selected-actions.json` declares exact approved workflow Action identities for the shared structured allowlist; Zizmor and Semgrep retain SHA and readable
version-comment checks. See the [ownership catalog](../docs/dev/tooling-alignment.md#shared-tooling-retirement-catalog) for retained policy and upstream gaps.

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
