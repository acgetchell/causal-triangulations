# causal-triangulations

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.20513228.svg)](https://doi.org/10.5281/zenodo.20513228)
[![Crates.io](https://badgen.net/crates/v/causal-triangulations)](https://crates.io/crates/causal-triangulations)
[![Downloads](https://badgen.net/crates/d/causal-triangulations)](https://crates.io/crates/causal-triangulations)
[![License](https://badgen.net/github/license/acgetchell/causal-triangulations)](LICENSE)
[![Docs.rs](https://docs.rs/causal-triangulations/badge.svg)](https://docs.rs/causal-triangulations)
[![CI][ci-badge]][ci-workflow]
[![rust-clippy analyze][clippy-badge]][clippy-workflow]
[![Codecov](https://codecov.io/gh/acgetchell/causal-triangulations/graph/badge.svg?token=CsbOJBypGC)](https://codecov.io/gh/acgetchell/causal-triangulations)
[![Audit dependencies][audit-badge]][audit-workflow]

Causal Dynamical Triangulations for quantum gravity in [Rust], built on fast [Delaunay triangulation] primitives and composable, adaptable
[Metropolis-Hastings sampling].

## Contents

- [Introduction](#-introduction)
- [Features](#-features)
- [Quickstart](#-quickstart)
- [Scientific Basis](#-scientific-basis)
- [Documentation Map](#-documentation-map)
- [Ecosystem](#-ecosystem)
- [Benchmarking](#-benchmarking)
- [Roadmap](#-roadmap)
- [Contributing](#-contributing)
- [Citation](#-citation)
- [References](#-references)
- [AI-assisted Development](#-ai-assisted-development)
- [License](#-license)

## 🌌 Introduction

This library implements **Causal Dynamical Triangulations (CDT)** in [Rust]. CDT is a non-perturbative approach to quantum gravity defining the gravitational
path integral over causally triangulated spacetimes and evaluating it using Markov Chain Monte Carlo. For an introduction to CDT, see Ambjørn and Loll (1998),
[“Non-perturbative Lorentzian quantum gravity, causality and topology change”](https://arxiv.org/abs/hep-th/9805108). The library leverages high-performance
[Delaunay triangulation] backends and provides a foundational toolkit for CDT research and exploration.

## ✨ Features

- Alexander/Pachner-style local move proposals with causal constraints
- Command-line interface, examples, Criterion benchmarks, and CI-aligned validation tooling
- Cross-platform compatibility: Linux, macOS, Windows
- Delaunay-built 1+1 CDT strip and periodic toroidal S¹×S¹ constructors with foliation invariants
- Focused public preludes for simulation, triangulation, geometry, action, and observables
- Foliation-aware topology, causality, and simplex-classification validation
- Notebook-first quickstart for physicists, AI/ML users, and Rust contributors
- Proposal-before-mutation Metropolis-Hastings simulation with rollback on failed accepted moves
- Regge action calculation with configurable coupling constants
- Versioned, CDT-owned JSON checkpoints for exact MCMC continuation across compatible crate and dependency upgrades, with checked geometry and state restore;
  see the [checkpoint compatibility policy](docs/metropolis.md#serialized-checkpoint-compatibility)
- Trace CSV simulation output for external analysis workflows; JSON summary/metadata for CLI/config export
- Spatial-vertex input profiles, slab-triangle output profiles, and explicitly finite-window effective dimensional observables

See [CHANGELOG.md](CHANGELOG.md) for release history and [`docs/roadmap.md`](docs/roadmap.md) for current direction, near-term candidates, and non-goals.

## 🚀 Quickstart

For most users, start with the notebook-first local run:

```bash
just notebook-setup
just notebook
```

`just notebook-setup` installs the uv-managed notebook dependency group, and `just notebook` launches JupyterLab with
[`notebooks/00_quickstart.ipynb`](notebooks/00_quickstart.ipynb) loaded. The recipes are defined in the `justfile`; inspect that file if you want to see
exactly what they run.

The notebook uses the `cdt` binary as the engine, then loads the trace CSV and JSON summary into plots. It also explains setup, installation expectations,
parameters, outputs, and small first experiments.

### Requirements

- Rust 1.98.0 or newer (pinned by `Cargo.toml` and `rust-toolchain.toml`)
- `uv` for the notebook environment and repository-managed Python tooling

Rust keeps the simulation engine memory-safe and fast while preserving validation tooling for tests, documentation, benchmarks, and CI parity.

For headless CI or batch execution, use:

```bash
just notebook-execute
```

For Slurm and Open OnDemand workflows, see [`docs/hpc.md`](docs/hpc.md).

Before committing edited notebooks, clear generated outputs and execution counts:

```bash
just notebook-clear-outputs-all
```

Use the binary directly when you want a scriptable run. For CLI usage, topology examples, and logging/output patterns, see
[`docs/cli-examples.md`](docs/cli-examples.md).

## 🧪 Scientific Basis

CDT approximates the gravitational path integral by summing over discrete, foliated spacetime geometries and sampling them with Markov Chain Monte Carlo. This
crate currently implements a validated 1+1-dimensional CDT foundation: it builds open-boundary and toroidal initial triangulations, checks foliation,
topology, causality, and simplex classification invariants, and runs local CDT move proposals through a Metropolis-Hastings sampler.

The validation is computational and ensemble-specific. The crate can check that generated and simulated triangulations satisfy the implemented discrete CDT
contract, that accepted moves preserve the configured topology and foliation constraints, and that proposal asymmetry is handled through the sampler's
Hastings correction. It does not prove continuum-limit physics, chain mixing, finite-size scaling, or suitability of a particular parameter choice for a
scientific study.

Current simulations are grand-canonical, unfixed-volume runs. Volume-changing `(1,3)` and `(3,1)` moves may grow or shrink the lattice, and the cosmological
constant controls that behavior through the action. This is intentional for the 1+1 foundation release; production volume fixing, automated λ scans, and
higher-dimensional CDT remain future work.

For the detailed scientific contract, ensemble scope, backend role, and parameter interpretation, see
[`docs/scientific-basis.md`](docs/scientific-basis.md). Move semantics and detailed-balance notes live in [`docs/moves.md`](docs/moves.md) and
[`docs/metropolis.md`](docs/metropolis.md).

## 🗺️ Documentation Map

- [CDT Spacetime Visualization notebook](notebooks/01_spacetime_visualization.ipynb) — example 1+1 CDT mesh visualization generator
- [CLI Examples](docs/cli-examples.md) — command-line usage and output workflows
- [Code Organization](docs/code-organization.md) — module layout, backend boundaries, and architecture notes
- [Example Scripts](examples/scripts/README.md) — maintained shell workflows for simulations, sweeps, and timing checks
- [Foliation](docs/foliation.md) — time labels, spacelike/timelike classification, causality validation, and toroidal time handling
- [HPC Notebook Workflows](docs/hpc.md) — Slurm, Open OnDemand, and cluster cache setup
- [Metropolis](docs/metropolis.md) — proposal-before-mutation ordering, detailed balance, trace semantics, and sampler/backend boundaries
- [Moves](docs/moves.md) — CDT local move semantics, proposal ratios, rollback behavior, and action calibration
- [Polars Analysis Caches notebook](notebooks/02_analysis_caches.ipynb) — local Parquet caches and diagnostic plots for debugging CDT CSV/JSON outputs
- [Quickstart notebook](notebooks/00_quickstart.ipynb) — notebook-first local 1+1 CDT run, parameter meanings, output files, and troubleshooting
- [References](REFERENCES.md) — physics, numerical, and computational-geometry citations
- [Roadmap](docs/roadmap.md) — near-term work, higher-dimensional topology tracks, and non-goals
- [Scientific Basis](docs/scientific-basis.md) — CDT scope, validated invariants, current ensemble, and interpretation boundaries

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

Performance validation uses [Criterion] benchmark suites plus repository recipes for repeatable local and CI checks. Run `just bench-ci` for the CI benchmark
contract and `just perf-check` for a local regression check. Release comparisons use `just performance-release`, which retains a schema-validated CSV and
matching provenance before updating the tracked report and summary.

<!-- performance-summary:start -->
Latest retained comparison: `v0.1.1` against `v0.1.0`.

| Comparable benchmarks | Current only | Baseline only |
| ---: | ---: | ---: |
| 29 | 9 | 5 |

![Release benchmark comparison](docs/assets/performance-comparison.svg)

[Tag-pinned full report](https://github.com/acgetchell/causal-triangulations/blob/v0.1.1/docs/PERFORMANCE.md) ·
[Native Criterion baseline](https://github.com/acgetchell/causal-triangulations/releases/download/v0.1.1/causal-triangulations-v0.1.1-criterion-baseline.tar.gz)
<!-- performance-summary:end -->

See [`benches/README.md`](benches/README.md) for benchmark details and [`docs/performance-testing.md`](docs/performance-testing.md) for comprehensive
performance testing workflow documentation.

## 🛣️ Roadmap

The high-level roadmap, including 1+1 maturity work, future 2+1 and 3+1 CDT topology tracks, observables, dual/Voronoi geometry, visualization, and non-goals,
lives in [`docs/roadmap.md`](docs/roadmap.md).

## 🤝 Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full contributor guide: project layout, development workflow, code style, testing, documentation layout,
performance/benchmarking, and release support. Community expectations live in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). AI assistants should follow
[AGENTS.md](AGENTS.md).

Quick local workflow: run `just setup` once, then run `just check` before opening a pull request. For the full command list, run `just --list`.

## 📚 Citation

If you use this software in academic work or downstream research software, cite the Zenodo DOI and include the software metadata from
[CITATION.cff](CITATION.cff).

- DOI: <https://doi.org/10.5281/zenodo.20513228>
- Citation metadata: [CITATION.cff](CITATION.cff)

```bibtex
@software{getchell_causal_triangulations,
  author = {Adam Getchell},
  title = {causal-triangulations: A Causal Dynamical Triangulation library for quantum gravity research},
  doi = {10.5281/zenodo.20513228},
  url = {https://github.com/acgetchell/causal-triangulations}
}
```

For release-specific fields such as version, release date, and ORCID, prefer [CITATION.cff](CITATION.cff).

## 🔎 References

For a comprehensive list of academic references and bibliographic citations used throughout the library, see [REFERENCES.md](REFERENCES.md).

This includes foundational work on:

- Causal Dynamical Triangulations theory
- Monte Carlo methods in quantum gravity
- Computational geometry and Delaunay triangulations
- Discrete approaches to general relativity

## 🤖 AI-assisted Development

This repository contains an [AGENTS.md](AGENTS.md) file, which defines the rules and invariants for AI coding assistants and autonomous agents working on this
codebase.

Portions of this library were developed with the assistance of AI tools including [ChatGPT], [Claude], [Codex], and [CodeRabbit].

All accepted code and documentation changes are reviewed, edited, and validated by the author.

For tool citation metadata, see the [AI-assisted development tools](REFERENCES.md#ai-assisted-development-tools) section of [REFERENCES.md](REFERENCES.md).

## 📜 License

This project is licensed under the [BSD 3-Clause License](LICENSE).

---

[Rust]: https://rust-lang.org
[Delaunay triangulation]: https://crates.io/crates/delaunay
[`markov-chain-monte-carlo`]: https://crates.io/crates/markov-chain-monte-carlo
[Metropolis-Hastings sampling]: https://crates.io/crates/markov-chain-monte-carlo
[Criterion]: https://github.com/bheisler/criterion.rs
[ChatGPT]: https://openai.com/chatgpt
[Claude]: https://www.anthropic.com/claude
[Codex]: https://openai.com/codex
[CodeRabbit]: https://coderabbit.ai/
[ci-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml/badge.svg
[ci-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml
[clippy-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml/badge.svg
[clippy-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml
[audit-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml/badge.svg
[audit-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml
