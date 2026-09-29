# Contributing to Causal Dynamical Triangulations

Thank you for your interest in contributing to [**causal-triangulations**][cdt-lib]. This project is a Rust library and command-line tool for validated
1+1 CDT simulations, with a strong emphasis on correctness, reproducibility, performance, and clear physics documentation.

## Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Style](#code-style)
- [Testing](#testing)
- [Security](#security)
- [Documentation](#documentation)
- [AI-Assisted Development](#ai-assisted-development)
- [Performance](#performance)
- [Pull Requests](#pull-requests)
- [Getting Help](#getting-help)

## Code of Conduct

Please keep discussion respectful and focused on advancing computational quantum-gravity research. Good contributions make the project easier to understand,
verify, reproduce, or extend.

## Getting Started

Prerequisites:

- Rust 1.98.1, pinned by `rust-toolchain.toml`; `Cargo.toml` also specifies `rust-version = "1.98.1"` as the required toolchain
- Git
- Platform C/C++ compiler and linker (Xcode command-line tools on macOS, a native build toolchain on Linux, or MSVC Build Tools on Windows)
- [Just] command runner, installed by shared setup below
- `uv` for Python support tooling
- GitHub CLI (`gh`) and `jq` for release and repository automation

Recommended setup:

```bash
git clone https://github.com/acgetchell/causal-triangulations.git
cd causal-triangulations
uv run --locked --managed-python --only-group tooling research-repo-tools setup
just check
```

Useful entry points:

- [README.md](README.md) — project overview and top-level documentation map
- [notebooks/00_quickstart.ipynb](notebooks/00_quickstart.ipynb) — first local CDT run
- [docs/scientific-basis.md](docs/scientific-basis.md) — CDT scope, validated invariants, current ensemble, and interpretation boundaries
- [docs/code-organization.md](docs/code-organization.md) — module layout and architecture boundaries
- [docs/dev/DEVELOPING.md](docs/dev/DEVELOPING.md) — authoritative local command guide
- [docs/dev/rust.md](docs/dev/rust.md) — Rust API, error, prelude, backend, and MCMC boundary rules
- [docs/dev/python.md](docs/dev/python.md) — Python support-script rules
- [docs/dev/testing.md](docs/dev/testing.md) — test expectations and validation workflow
- [docs/dev/tooling-alignment.md](docs/dev/tooling-alignment.md) — rationale for repository tooling choices

## Development Workflow

Common commands:

```bash
just                 # Show every public recipe, argument, and description
just setup           # Complete environment setup
just check           # Non-mutating validation
just fix             # Formatters and auto-fixes
just ci              # CI parity
just commit-check    # Full pre-commit validation
just run-example     # Basic simulation example
just bench-ci        # CI benchmark contract
just bench-save-last # Save a fresh local Criterion baseline
just bench-latest-vs-last # Compare absolute timings and relative changes
```

`just setup` repeats this shared setup. It installs Just, synchronizes the declared Rust/Cargo tools in a managed cache, and syncs the locked
Python development environment. Open a new shell if setup reports a PATH change. `just tools-check` verifies installed versions without installing tools;
`just setup-tools` repairs the managed toolchain. Rust and benchmark commands inherit that toolchain through the shared runner.

This command works on Linux, macOS, and Windows; the published setup contract requires no generated `bootstrap.sh` or `bootstrap.ps1` launchers.

Dependency and tool maintenance has a separate [update workflow](docs/dev/DEVELOPING.md#dependency-and-tool-maintenance).

Ready-to-use shell workflows live under `examples/scripts/`:

```bash
./examples/scripts/basic_simulation.sh      # Simple simulation command
./examples/scripts/parameter_sweep.sh       # Temperature sweep setup
./examples/scripts/performance_test.sh      # Performance benchmarking across system sizes
```

Release metadata, changelog generation, tagging, and publication procedures belong to [RELEASING](docs/RELEASING.md).

Prefer small focused branches. Branch names should follow `{type}/{issue}-descriptor-or-two`, for example:

```text
fix/307-topology-validation
perf/315-bench-profile
docs/187-quickstart
```

Before opening a PR:

- run the appropriate `just` checks;
- update tests and docs with the code change;
- update `docs/code-organization.md` when files or architecture-significant modules move;
- update `docs/dev/tooling-alignment.md` before changing repository tooling, workflows, or config policy;
- do not edit `CHANGELOG.md` directly; it is generated from commits.

## Code Style

Rust code uses:

- Rust 2024 edition
- MSRV 1.98.1
- `#![forbid(unsafe_code)]`
- `rustfmt` and strict Clippy
- narrow `CdtError` variants and `CdtResult<T>` for production errors

Architectural boundaries matter:

- `src/geometry/` is the backend interface layer over `delaunay`.
- `src/cdt/` owns CDT domain logic: foliation, topology, moves, action, observables, simulation, and results.
- `src/cdt/metropolis/` contains the thin adapters and runner code that consume `markov-chain-monte-carlo`.
- Direct `delaunay::` imports are restricted to the geometry layer and documented exceptions.

See [docs/dev/rust.md](docs/dev/rust.md) for the detailed rules.

## Testing

Use focused tests for narrow changes and broader validation for shared behavior. Common commands:

```bash
just test-unit
just test-integration
just test-cli
just test-doc
just bench-compile
just check
just ci
```

Testing expectations live in [docs/dev/testing.md](docs/dev/testing.md). Benchmarks and performance regression workflows live in
[benches/README.md](benches/README.md) and [docs/BENCHMARKING.md](docs/BENCHMARKING.md).

## Security

Run `just audit` to scan `Cargo.lock` and `uv.lock` through OSV, `just security-secrets` for the Gitleaks full-history and working-tree scan, or `just security`
for both. Shared setup installs and verifies the exact scanner versions declared in `pyproject.toml`. OSV needs network access; secrets scanning requires a
complete Git checkout. Findings, scanner failures, and incomplete reports fail the invoked recipe. JSON/SARIF reports are retained under `target/security/`
with secret values redacted. Dedicated GitHub workflows run both scans on main-branch pushes and pull requests, weekly, and on manual dispatch.
These scans are separate from local `check` and `ci`; see [the scanner contract](docs/dev/DEVELOPING.md#dependency-and-secret-scanning).

## Documentation

Documentation changes are first-class contributions. Keep prose accurate for both CDT specialists and readers who know physics or AI but not Rust.
The [documentation index](docs/README.md) maps content owners; the [documentation and command policy](docs/dev/DEVELOPING.md#documentation-and-command-policy)
defines naming, navigation, and where commands belong.

When editing docs:

- keep README high-level and link to deeper docs;
- keep [notebooks/00_quickstart.ipynb](notebooks/00_quickstart.ipynb) beginner-oriented and executable;
- keep [docs/scientific-basis.md](docs/scientific-basis.md) focused on scientific scope, validated invariants, and ensemble interpretation;
- keep [docs/RUNNING.md](docs/RUNNING.md) focused on scriptable CLI patterns;
- keep [docs/RUNNING-ON-HPC.md](docs/RUNNING-ON-HPC.md) focused on Slurm, Open OnDemand, and cluster execution;
- keep technical move/sampler details in [docs/moves.md](docs/moves.md) and [docs/metropolis.md](docs/metropolis.md);
- add or update citations in [REFERENCES.md](REFERENCES.md) when scientific claims depend on literature.

## AI-Assisted Development

[AGENTS.md](AGENTS.md) defines the canonical rules and invariants for AI coding assistants and autonomous agents working on this codebase.
AI tools are expected to read and follow it and its task-relevant linked guidance before proposing or applying changes.

Portions of this library were developed with the assistance of these tools:

- [ChatGPT](https://openai.com/chatgpt)
- [Claude](https://www.anthropic.com/claude)
- [CodeRabbit](https://coderabbit.ai/)
- [Codex](https://openai.com/codex/)

All AI-assisted work must be reviewed and validated by a human maintainer before it is merged.

For tool citation metadata, see [AI-Assisted Development Tools](REFERENCES.md#ai-assisted-development-tools) in [REFERENCES.md](REFERENCES.md).

## Performance

Performance is part of the scientific contract for this project. Use:

- [benches/README.md](benches/README.md) for benchmark inventory, Criterion usage, and adding new benchmarks;
- [docs/BENCHMARKING.md](docs/BENCHMARKING.md) for regression checks, baselines, CI behavior, and reporting.

Before large algorithmic changes, save or inspect a baseline:

```bash
just bench-ci
just bench-save-last
just bench-latest-vs-last
```

New performance evidence uses shared schemas. Historical reports and formats were retired in September 2026. Start with a fresh local baseline;
release comparisons require two explicit new-series tags. `just performance-doc` rerenders retained shared evidence, and `performance-readme CONFIG`
publishes absolute timings and relative changes together. See the performance guide for draft-release assets and publication configuration.

## Pull Requests

Optional local review requires an installed, authenticated CodeRabbit CLI:

```bash
just review                  # Committed branch changes plus local edits against origin/main
just review local/base       # Choose a locally available comparison ref
just review-uncommitted      # Uncommitted changes and untracked files
```

These commands pass `AGENTS.md` and `.coderabbit.yml` to the reviewer and preserve its failure status. They are outside `check` and `ci`; invoking them
explicitly sends the selected changes to CodeRabbit. Run the normal validators independently.

The default base is compared with live `origin/main`; missing/stale refs and failed remote lookups stop the review. Refresh a stale ref manually before
retrying. An explicit local base skips that remote check, and uncommitted review includes staged, unstaged, and nonignored untracked files without checking
remote freshness. Missing authentication, service failures, and allowance exhaustion mean review is unavailable. Agents need an explicit CodeRabbit request
before invocation; local validation does not authorize sending a review. See [review details](docs/dev/DEVELOPING.md#local-coderabbit-review).

Pull requests should be small enough to review. Include:

- what changed;
- why it changed;
- validation commands run;
- performance impact when relevant;
- links to issues or literature when the change is scientific or architectural.

Use conventional commit subjects when possible:

```text
docs: streamline cli documentation
fix: reject invalid toroidal profile metadata
perf: reduce proposal-site allocation
```

## Getting Help

- Use GitHub issues for bugs and feature requests.
- Use discussions for broader CDT, architecture, or workflow questions.
- Include command output, configuration, seeds, and platform information when reporting reproducibility or performance issues.

Thank you for helping make computational quantum-gravity tooling more reliable and understandable.

[cdt-lib]: https://github.com/acgetchell/causal-triangulations
[Just]: https://github.com/casey/just
