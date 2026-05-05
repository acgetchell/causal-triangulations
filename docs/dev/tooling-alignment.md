# Tooling Alignment

This note records the issue #112 comparison between `causal-triangulations` and the sibling `markov-chain-monte-carlo` repository. Keep it current when changing repository tooling so future updates can be deliberate rather than copied wholesale.

## Current Parity

Both repositories now have explicit local configuration for the core Rust and Python support-tooling loop:

- `rust-toolchain.toml` pins the Rust toolchain and development components.
- `rustfmt.toml` records stable Rust formatting settings used by `cargo fmt`.
- `.taplo.toml` keeps TOML formatting conservative and Cargo-like.
- `ty.toml` scopes Ty to `scripts/` and pins the supported Python version.
- `pyproject.toml` owns Ruff, Ty, pytest, uv packaging, and uv-managed development dependencies.
- `dprint.json`, `.yamllint`, `typos.toml`, and `clippy.toml` define documentation and configuration checks.
- `justfile` is the single local entry point for formatting, linting, tests, coverage, Semgrep, changelog, and setup commands.

## Python Tooling

CDT's Python tooling is broader than MCMC's. MCMC currently has changelog and tag helpers; CDT also has benchmark, hardware, coverage, archive, and performance-analysis scripts. The shared script patterns have already been ported here:

- secure subprocess wrappers in `scripts/subprocess_utils.py`;
- typed `subprocess.CompletedProcess[str]` helpers in tests;
- Ruff, Ty, pytest, and uv-managed development dependencies in `pyproject.toml`;
- Python Semgrep rules and fixtures for broad exception catches, raw `Exception` in tests, ad hoc subprocess mocks, and missing return annotations.

The broad-exception Semgrep rule is intentionally scoped to the smaller changelog, coverage, tag, subprocess, and model helper scripts in this pass. `scripts/benchmark_utils.py`, `scripts/hardware_utils.py`, and `scripts/performance_analysis.py` still contain broad recovery paths that need a separate cleanup before the rule can cover every script without creating noisy findings.

## Intentional Differences

Some differences remain because CDT has different workflows and project invariants:

- CDT runs examples through `scripts/run_all_examples.sh`, which discovers current examples dynamically and applies a timeout. The MCMC `validate-examples` recipe checks fixed seeded output markers for a smaller, stable example set; CDT should only add marker checks once example output contracts are intentionally stable.
- CDT keeps `archive-changelog` so completed release series move under `docs/archive/changelog/`; MCMC does not yet archive old changelog sections.
- CDT keeps a dedicated `performance.yml` workflow and local `perf-*` recipes. MCMC does not have matching CDT benchmark-baseline tooling.
- CDT has a repository rule SARIF workflow for the local Semgrep rules. A Codacy workflow was not ported because it depends on project-specific `CODACY_PROJECT_TOKEN` setup and would duplicate the existing repository-rule SARIF signal until Codacy is configured for this repository.
- CDT Semgrep rules include geometry-backend isolation, foliation/topology validation, focused prelude imports, Python support-script discipline, and typed error policies. These are repository-specific and should not be weakened while porting generic rules.
- CDT and MCMC both require Python `>=3.12` for repository-managed support tooling.

## Ported Updates

The useful updates ported from MCMC are:

- explicit `rustfmt.toml` formatting configuration;
- explicit uv package mode and pytest 9 minimum in `pyproject.toml`;
- CodeQL analysis for GitHub Actions and Rust, using `build-mode: none` for Rust;
- the MCMC-style `cliff.toml` template and `just changelog-unreleased <version>` flow, adapted to keep CDT's changelog archive step and avoid temporary local release tags;
- a Semgrep rule that rejects `NaN` and infinity defaults after failed floating-point conversions, with a regression fixture under `tests/semgrep/`.

## Deferred Updates

These were evaluated but not ported in this pass:

- `codacy.yml`: defer until the repository has an intentional Codacy project token and a decision about whether Codacy should upload repository-owned OpenGrep/Semgrep findings in addition to `.github/workflows/semgrep-sarif.yml`.
- `validate-examples`: defer until example outputs have stable, documented success markers. For now, `just examples` validates compilation and successful execution of all discovered examples.
- broad no-`unwrap`/`panic` Semgrep rules: defer because current production and doctest code intentionally uses many invariant checks and examples. These need a separate cleanup plan before becoming blocking policy.
- broad Python exception coverage for all scripts: defer until the benchmark, hardware, and performance-analysis scripts have typed recoverable exception boundaries.
