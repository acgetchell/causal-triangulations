# causal-triangulations

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.20513229.svg)](https://doi.org/10.5281/zenodo.20513229)
[![Crates.io](https://img.shields.io/crates/v/causal-triangulations.svg)](https://crates.io/crates/causal-triangulations)
[![Downloads](https://img.shields.io/crates/d/causal-triangulations.svg)](https://crates.io/crates/causal-triangulations)
[![Docs.rs](https://docs.rs/causal-triangulations/badge.svg)](https://docs.rs/causal-triangulations)
[![CI][ci-badge]][ci-workflow]
[![rust-clippy analyze][clippy-badge]][clippy-workflow]
[![Codecov](https://codecov.io/gh/acgetchell/causal-triangulations/graph/badge.svg?token=CsbOJBypGC)](https://codecov.io/gh/acgetchell/causal-triangulations)
[![Audit dependencies][audit-badge]][audit-workflow]

Causal Dynamical Triangulations for quantum gravity in [Rust], built on fast [Delaunay triangulation] primitives
and composable, adaptable Metropolis-Hastings sampling via [`markov-chain-monte-carlo`].

## Contents

- [Introduction](#-introduction)
- [Project Status](#-project-status)
- [Ensemble And Volume Behavior](#-ensemble-and-volume-behavior)
- [Features](#-features)
- [Documentation Map](#-documentation-map)
- [Requirements](#-requirements)
- [Running The Binary](#-running-the-binary)
- [Ecosystem](#-ecosystem)
- [Benchmarking](#-benchmarking)
- [Roadmap](#-roadmap)
- [How to Contribute](#-how-to-contribute)
- [References](#-references)
- [AI Agents](#-ai-agents)
- [License](#-license)

## 🌌 Introduction

This library implements **Causal Dynamical Triangulations (CDT)** in [Rust]. CDT is a non-perturbative approach to quantum gravity defining the gravitational
path integral over causally triangulated spacetimes and evaluating it using Markov Chain Monte Carlo. For an introduction to CDT, see Ambjørn and Loll (2001),
[“Non-perturbative Lorentzian quantum gravity, causality and topology change”](https://arxiv.org/abs/hep-th/0105267). The library leverages high-performance
[Delaunay triangulation] backends and provides a foundational toolkit for CDT research and exploration.

## 🚧 Project Status

**v0.1.0 foundation release** — The validated 1+1 CDT foundation is in place, including toroidal volume-changing moves, nonuniform initial volume profiles,
upstream-backed MCMC sampler mechanics, trace/summary exports, resumable checkpoints, and CI-aligned validation tooling.

The library currently supports validated 1+1 CDT construction, foliation checks, Metropolis sampling, resumable checkpoints, and core observables.
Higher-dimensional CDT support, visualization/export workflows, and advanced ensemble-analysis helpers remain roadmap work.

Current scope:

- Supported now: validated 1+1 open-boundary and toroidal CDT construction, local CDT move proposals, Metropolis sampling, trace/summary output, and core
  observables.
- Not supported yet: 2+1 or 3+1 CDT, production volume fixing, automated λ scans, visualization/export workflows, and full ensemble-analysis tooling.
- Planned next: 1+1 refinement work for finite-volume scans, release binaries, analysis ergonomics, and explicit topology tracks for higher-dimensional CDT.

For a copy-paste local run written for physicists rather than Rust contributors, start with the [Quick Start](docs/QUICKSTART.md).

See [`docs/roadmap.md`](docs/roadmap.md) for current direction, near-term candidates, and non-goals.

## ⚖️ Ensemble And Volume Behavior

Current simulations do not apply volume fixing. Volume-changing moves may change the total number of vertices and simplices during a run, so the sampled
ensemble is the grand-canonical, unfixed-volume ensemble defined by the configured CDT action and Metropolis-Hastings proposal rules. This is intentional for
now: in 1+1 CDT, unfixed-volume simulations controlled by the cosmological constant are a standard toy-model setting, as in Israel and Lindner,
[Quantum gravity on a laptop: 1+1 Dimensional Causal Dynamical Triangulation simulation](https://doi.org/10.1016/j.rinp.2012.10.001).

In a grand-canonical CDT ensemble, the cosmological constant is the coupling that controls volume growth or shrinkage because it is conjugate to the lattice
volume term in the action. Use `--cosmological-constant` to tune that behavior. Values too far from the useful finite-volume regime can drive runs toward
minimum-volume configurations or toward rapid growth; this is expected physics for the unfixed-volume ensemble, not volume fixing.
Automated λ-scan utilities for finding practical finite-volume windows are planned as [#143](https://github.com/acgetchell/causal-triangulations/issues/143);
for v0.1.0, tune `--cosmological-constant` manually and inspect the reported volume and acceptance diagnostics.

The default 1+1 action constants use `κ0 = 0`, `κ2 = 0`, and an edge-count cosmological constant `(2 / 3) ln 2`, mapping the crate's `λ N1` convention to the
standard 2D CDT critical triangle-volume coupling `λc = ln 2` for closed 1+1 triangulations, where `N1 = 3 N2 / 2`. Open-boundary strips have boundary-count
corrections, so the same default should be treated as a practical baseline rather than an exact open-boundary critical value. The Delaunay backend supplies
construction and validation infrastructure for a well-formed initial PL triangulation; the simulation ensemble is defined by CDT moves, constraints, action,
and Metropolis-Hastings acceptance, not by maintaining the Delaunay condition after every move.

Higher-dimensional CDT studies often use explicit approximate volume fixing for finite-size numerical work. For example, Ambjørn et al. discuss quadratic
volume fixing in [The Semiclassical Limit of Causal Dynamical Triangulations](https://arxiv.org/abs/1102.3929), and the toroidal phase-structure study uses
quadratic volume fixing in [The phase structure of Causal Dynamical Triangulations with toroidal spatial topology](https://arxiv.org/abs/1802.10434). This
crate may add such a mode later, but it should be opt-in because it samples a modified action rather than the current bare unfixed-volume ensemble.

## ✨ Features

- Delaunay-built 1+1 CDT strip and periodic toroidal S¹×S¹ constructors with foliation invariants
- Foliation-aware topology, causality, and simplex-classification validation
- Proposal-before-mutation Metropolis-Hastings simulation with rollback on failed accepted moves
- Regge action calculation with configurable coupling constants
- Alexander/Pachner-style local move proposals with causal constraints
- Volume-profile, Hausdorff-dimension, and spectral-dimension observables for CDT analysis
- Trace CSV simulation output for external analysis workflows; JSON summary/metadata for CLI/config export
- Resumable serde-backed CDT/MCMC checkpoints for durable chain continuation
- Focused public preludes for simulation, triangulation, geometry, action, and observables
- Command-line interface, examples, Criterion benchmarks, and CI-aligned validation tooling
- Cross-platform compatibility: Linux, macOS, Windows

See [CHANGELOG.md](CHANGELOG.md) for release history.

## 🗺️ Documentation Map

- [Quick Start](docs/QUICKSTART.md) — first local 1+1 CDT run, parameter meanings, output files, and troubleshooting
- [CLI Examples](docs/CLI_EXAMPLES.md) — advanced command-line usage and output workflows
- [Metropolis](docs/metropolis.md) — proposal-before-mutation ordering, detailed balance, trace semantics, and sampler/backend boundaries
- [Moves](docs/moves.md) — CDT local move semantics, proposal ratios, rollback behavior, and action calibration
- [Foliation](docs/foliation.md) — time labels, spacelike/timelike classification, causality validation, and toroidal time handling
- [Roadmap](docs/roadmap.md) — near-term work, higher-dimensional topology tracks, and non-goals
- [Code Organization](docs/code_organization.md) — module layout, backend boundaries, and architecture notes
- [References](REFERENCES.md) — physics, numerical, and computational-geometry citations

## ⚙️ Requirements

- Rust 1.96.0 or newer (pinned by `Cargo.toml` and `rust-toolchain.toml`)

**Why Rust for CDT?**

- **Memory safety** for large-scale simulations
- **Zero-cost abstractions** for performance-critical geometry operations
- **Validation tooling** for tests, documentation, benchmarks, and CI parity

## 💻 Running The Binary

The crate installs a `cdt` binary. Use it to construct an initial 1+1 CDT triangulation, optionally run the Metropolis move loop, and write analysis-friendly
trace CSV output plus JSON summary/metadata.

```bash
cargo install causal-triangulations
cdt --help

cdt \
  --dimension 2 \
  --vertices-per-slice 4 \
  --timeslices 5 \
  --steps 100 \
  --thermalization-steps 10 \
  --measurement-frequency 10 \
  --seed 105 \
  --simulate \
  --output-csv cdt-runs/quickstart/trace.csv \
  --output-json cdt-runs/quickstart/summary.json
```

For a fully annotated first run, see [Quick Start](docs/QUICKSTART.md). For advanced CLI usage, topology examples, and logging/output patterns, see
[`docs/CLI_EXAMPLES.md`](docs/CLI_EXAMPLES.md).

The `examples/scripts/` directory contains ready-to-use research workflows:

- **`basic_simulation.sh`** - Simple simulation command
- **`parameter_sweep.sh`** - Temperature sweep setup
- **`performance_test.sh`** - Construction and simulation timing across system sizes

For detailed documentation, sample output, and usage instructions for each script, see [examples/scripts/README.md](examples/scripts/README.md).

## 🧩 Ecosystem

This crate is part of a broader Rust ecosystem for computational geometry and simulation:

- [`delaunay`](https://crates.io/crates/delaunay) — geometric primitives and triangulations
- [`la-stack`](https://crates.io/crates/la-stack) — linear algebra utilities
- [`markov-chain-monte-carlo`](https://crates.io/crates/markov-chain-monte-carlo) — composable MCMC traits, including plan-before-commit proposals for CDT
  move ordering

The design separates geometry, sampling, and CDT-specific physics. Within this crate, `src/geometry/` is the backend interface layer over `delaunay`,
`src/cdt/` is the CDT domain layer, and `src/cdt/metropolis/` contains the thin adapters and runner code that consume `markov-chain-monte-carlo`.

- **Foliation‑aware data model**: explicit time labels; space‑like vs time‑like edges encoded in types.
- **Testing**: unit, integration, and property-based tests for topology, causality, foliation, and simulation invariants.

## 📈 Benchmarking

Comprehensive performance benchmarks using [Criterion]:

```bash
# Run all benchmarks
cargo bench

# Specific benchmark categories
cargo bench triangulation_creation
cargo bench metropolis_simulation
cargo bench action_calculations

# CI regression benchmark contract
just bench-ci

# Performance regression testing
just perf-check          # Check for performance regressions
just perf-baseline       # Save performance baseline
just perf-report         # Generate detailed performance report
just perf-trends 7       # Analyze performance trends over 7 days
```

See [`benches/README.md`](benches/README.md) for benchmark details and [`docs/PERFORMANCE_TESTING.md`](docs/PERFORMANCE_TESTING.md) for comprehensive
performance testing workflow documentation.

## 🛣️ Roadmap

The high-level roadmap, including 1+1 maturity work, future 2+1 and 3+1 CDT topology tracks, observables, dual/Voronoi geometry, visualization, and non-goals,
lives in [`docs/roadmap.md`](docs/roadmap.md).

## 🤝 How to Contribute

We welcome contributions. Here's a short local workflow:

```bash
# Clone and setup
git clone https://github.com/acgetchell/causal-triangulations.git
cd causal-triangulations

# Traditional approach
cargo build && cargo test

# Modern approach (recommended) - install just command runner
cargo install just
just setup           # Complete environment setup
just check           # Run all linters/validators
just fix             # Apply formatters/auto-fixes
just --list          # See all available development commands

# Run examples
just run-example     # Basic simulation
./examples/scripts/basic_simulation.sh      # Shell script example
./examples/scripts/parameter_sweep.sh       # Temperature sweep setup
./examples/scripts/performance_test.sh      # Performance benchmarking across system sizes
```

`just setup` installs or verifies Cargo-hosted tools such as `dprint`, `rumdl`, `taplo-cli`, `typos-cli`, `cargo-nextest`, `cargo-llvm-cov`, and `zizmor`.
It also prints a checklist for external tools such as `uv`, `actionlint`, `shfmt`, `shellcheck`, and `jq`.

**Just Workflows:**

- `just check` - Run linters/validators (non-mutating)
- `just fix` - Apply formatters/auto-fixes (mutating)
- `just ci` - CI parity (mirrors GitHub Actions workflow [`ci.yml`](.github/workflows/ci.yml))
- `just commit-check` - Comprehensive pre-commit validation

**Repository tooling (via `just`):**

- `just changelog` - Regenerate `CHANGELOG.md`
- `just changelog-unreleased v0.1.0` - Generate a release changelog before the final tag exists
- `just tag v0.1.0` - Create an annotated git tag from changelog content
- `just perf-help` - Show performance analysis commands (`perf-baseline`, `perf-check`, etc.)

For comprehensive guidelines on contributing, development environment setup, testing, and code organization, please see [CONTRIBUTING.md](CONTRIBUTING.md).

This includes information about:

- Building and testing the library
- Running benchmarks and performance analysis
- Code style and standards
- Submitting changes and pull requests
- Code organization and development tools

## 📚 References

For a comprehensive list of academic references and bibliographic citations used throughout the library, see [REFERENCES.md](REFERENCES.md).

This includes foundational work on:

- Causal Dynamical Triangulations theory
- Monte Carlo methods in quantum gravity
- Computational geometry and Delaunay triangulations
- Discrete approaches to general relativity

## 🤖 AI Agents

AI coding assistants should read [AGENTS.md](AGENTS.md) before proposing or applying changes. See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup,
testing, benchmarks, style, and pull-request guidance.

## 📝 License

This project's license is specified in [LICENSE](LICENSE).

---

[Rust]: https://rust-lang.org
[Delaunay triangulation]: https://crates.io/crates/delaunay
[`markov-chain-monte-carlo`]: https://crates.io/crates/markov-chain-monte-carlo
[Criterion]: https://github.com/bheisler/criterion.rs
[ci-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml/badge.svg
[ci-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml
[clippy-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml/badge.svg
[clippy-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml
[audit-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml/badge.svg
[audit-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml
