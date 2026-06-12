# Contributing to Causal Dynamical Triangulations

Thank you for your interest in contributing to [**causal-triangulations**][cdt-lib]. This project is a Rust library and command-line tool for validated
1+1 CDT simulations, with a strong emphasis on correctness, reproducibility, performance, and clear physics documentation.

## Code of Conduct

Please keep discussion respectful and focused on advancing computational quantum-gravity research. Good contributions make the project easier to understand,
verify, reproduce, or extend.

## Getting Started

Prerequisites:

- Rust 1.96.0, pinned by `rust-toolchain.toml`; `Cargo.toml` also specifies `rust-version = "1.96.0"` as the required toolchain
- Git
- [Just] command runner: `cargo install just`
- `uv` for Python support tooling

Recommended setup:

```bash
git clone https://github.com/acgetchell/causal-triangulations.git
cd causal-triangulations
just setup
just check
```

Useful entry points:

- [README.md](README.md) — project overview and top-level documentation map
- [notebooks/00_quickstart.ipynb](notebooks/00_quickstart.ipynb) — first local CDT run
- [docs/scientific-basis.md](docs/scientific-basis.md) — CDT scope, validated invariants, current ensemble, and interpretation boundaries
- [docs/code-organization.md](docs/code-organization.md) — module layout and architecture boundaries
- [docs/dev/commands.md](docs/dev/commands.md) — authoritative local command guide
- [docs/dev/rust.md](docs/dev/rust.md) — Rust API, error, prelude, backend, and MCMC boundary rules
- [docs/dev/python.md](docs/dev/python.md) — Python support-script rules
- [docs/dev/testing.md](docs/dev/testing.md) — test expectations and validation workflow
- [docs/dev/tooling-alignment.md](docs/dev/tooling-alignment.md) — rationale for repository tooling choices

## Development Workflow

Common commands:

```bash
just --list          # Show available recipes
just setup           # Complete environment setup
just check           # Non-mutating validation
just fix             # Formatters and auto-fixes
just ci              # CI parity
just commit-check    # Full pre-commit validation
just run-example     # Basic simulation example
just bench-ci        # CI benchmark contract
just perf-check      # Local performance regression check
just perf-help       # Performance analysis command index
```

`just setup` installs or verifies Cargo-hosted tools such as `dprint`, `rumdl`, `taplo-cli`, `typos-cli`, `cargo-nextest`, `cargo-llvm-cov`, and `zizmor`.
It also prints a checklist for external tools such as `uv`, `actionlint`, `shfmt`, `shellcheck`, and `jq`.

Ready-to-use shell workflows live under `examples/scripts/`:

```bash
./examples/scripts/basic_simulation.sh      # Simple simulation command
./examples/scripts/parameter_sweep.sh       # Temperature sweep setup
./examples/scripts/performance_test.sh      # Performance benchmarking across system sizes
```

Release-support recipes are documented in [docs/RELEASING.md](docs/RELEASING.md). The most common entry points are:

```bash
just changelog                       # Regenerate CHANGELOG.md
just changelog-unreleased v0.1.0     # Generate a release changelog before the final tag exists
just tag v0.1.0                      # Create an annotated git tag from changelog content
```

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
- MSRV 1.96.0
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
cargo test --lib
cargo test --test integration_tests
cargo test --test cli
cargo test --doc
cargo bench --no-run
just check
just ci
```

Testing expectations live in [docs/dev/testing.md](docs/dev/testing.md). Benchmarks and performance regression workflows live in
[benches/README.md](benches/README.md) and [docs/performance-testing.md](docs/performance-testing.md).

## Documentation

Documentation changes are first-class contributions. Keep prose accurate for both CDT specialists and readers who know physics or AI but not Rust.

When editing docs:

- keep README high-level and link to deeper docs;
- keep [notebooks/00_quickstart.ipynb](notebooks/00_quickstart.ipynb) beginner-oriented and executable;
- keep [docs/scientific-basis.md](docs/scientific-basis.md) focused on scientific scope, validated invariants, and ensemble interpretation;
- keep [docs/cli-examples.md](docs/cli-examples.md) focused on scriptable CLI patterns;
- keep [docs/hpc.md](docs/hpc.md) focused on Slurm, Open OnDemand, and cluster execution;
- keep technical move/sampler details in [docs/moves.md](docs/moves.md) and [docs/metropolis.md](docs/metropolis.md);
- add or update citations in [REFERENCES.md](REFERENCES.md) when scientific claims depend on literature.

## Performance

Performance is part of the scientific contract for this project. Use:

- [benches/README.md](benches/README.md) for benchmark inventory, Criterion usage, and adding new benchmarks;
- [docs/performance-testing.md](docs/performance-testing.md) for regression checks, baselines, CI behavior, and reporting.

Before large algorithmic changes, save or inspect a baseline:

```bash
just bench-ci
just perf-baseline pre-change
just perf-check
```

## Pull Requests

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
