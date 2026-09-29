# causal-triangulations

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.20513228.svg)](https://doi.org/10.5281/zenodo.20513228)
[![Crates.io](https://badgen.net/crates/v/causal-triangulations)](https://crates.io/crates/causal-triangulations)
[![Downloads](https://badgen.net/crates/d/causal-triangulations)](https://crates.io/crates/causal-triangulations)
[![License](https://badgen.net/github/license/acgetchell/causal-triangulations)][repo-license]
[![Docs.rs](https://docs.rs/causal-triangulations/badge.svg)](https://docs.rs/causal-triangulations)
[![CI][ci-badge]][ci-workflow]
[![rust-clippy analyze][clippy-badge]][clippy-workflow]
[![Codecov](https://codecov.io/gh/acgetchell/causal-triangulations/graph/badge.svg?token=CsbOJBypGC)](https://codecov.io/gh/acgetchell/causal-triangulations)
[![Audit dependencies][audit-badge]][audit-workflow]

Causal Dynamical Triangulations for quantum gravity in [Rust], built on fast [Delaunay triangulation] primitives and composable, adaptable
[Metropolis-Hastings sampling].

## Contents

- [Introduction](#-introduction)
- [Use this crate when](#use-this-crate-when)
- [API and model scope](#api-and-model-scope)
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

## Use this crate when

- You need foliated 1+1 CDT strips or periodic toroidal triangulations with explicit topology and causality checks.
- You want configurable local-move Metropolis-Hastings simulations with trace output and resumable checkpoints.
- You want to inspect finite-lattice profiles and dimensional diagnostics through Rust, the CLI, or notebooks.

Higher-dimensional CDT and quantitative agreement with analytic ensembles remain planned work. Assess mixing, thermalization, finite-size effects, and
uncertainties for each scientific study; structural validation alone does not establish them.

## API and model scope

The supported simulation dimension is **2 (1+1 spacetime)**. Open-boundary strips have open spatial and temporal boundaries and Euler characteristic χ = 1;
toroidal runs are periodic in space and time, S¹×S¹, with χ = 0. The minimum initial sizes are four vertices per slice and two slices for strips, and three
vertices per slice and three slices for tori. Simulations use the unfixed-volume ensemble.

| Need | API entry point |
| --- | --- |
| Configure a run | [`prelude::config`][api-config] and `CdtConfig::into_validated` |
| Construct and inspect CDT states | [`prelude::triangulation`][api-triangulation] |
| Evaluate an action | [`prelude::action`][api-action] |
| Measure profiles and finite-graph observables | [`prelude::observables`][api-observables] |
| Run or resume a simulation | [`prelude::simulation`][api-simulation] |

The [API reference][api] describes the latest published crate; `just doc-check` builds the reference for this checkout. Geometry construction and structural
validation belong to [`delaunay`][geometry-api]; generic acceptance and chain mechanics belong to [`markov-chain-monte-carlo`][mcmc-api]. This crate owns CDT
foliation, moves, proposal probabilities, action, and ensemble conventions. See the [scientific contract][repo-docs-scientific-basis-md] for those boundaries.

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
- Spatial-vertex input profiles, slab-triangle output profiles, and explicitly finite-window effective dimensional observables
- Trace CSV simulation output for external analysis workflows; JSON summary/metadata for CLI/config export
- Versioned, CDT-owned JSON checkpoints for exact MCMC continuation across compatible crate and dependency upgrades, with checked geometry and state restore;
  see the [checkpoint compatibility policy][checkpoint-policy]

See [CHANGELOG.md][repo-changelog-md] for release history and [`docs/roadmap.md`][repo-docs-roadmap-md] for current direction, near-term candidates, and
non-goals.

## 🚀 Quickstart

From a repository checkout, follow [prerequisite setup][repo-contributing-mdgetting-started], then run a small seeded simulation:

```bash
just run -- --vertices-per-slice 4 --timeslices 5 --steps 100 \
  --thermalization-steps 10 --measurement-frequency 10 --seed 105 --simulate \
  --output-csv target/quickstart/trace.csv --output-json target/quickstart/summary.json
just  # Discover every public command, its arguments, and its purpose
```

This creates a 20-vertex open-boundary strip, attempts 100 Metropolis proposals, and writes one CSV trace row per step plus JSON measurements and final mesh.
Parent directories are created automatically. Accepted volume-changing moves can change the final size.

For interactive exploration:

```bash
just notebook-setup
just notebook
```

The quickstart notebook runs the `cdt` engine and plots its CSV/JSON outputs. Rust 1.98.1 or newer is required; repository tooling and notebooks use the
declared uv environment. See [CLI workflows][repo-docs-running-md] for scriptable runs and [cluster workflows][repo-docs-running-on-hpc-md] for Slurm and Open
OnDemand.

## 🧪 Scientific Basis

CDT approximates the gravitational path integral by summing over discrete, foliated spacetime geometries and sampling them with Markov Chain Monte Carlo. This
crate currently implements a validated 1+1-dimensional CDT foundation: it builds open-boundary and toroidal initial triangulations, checks foliation,
topology, causality, and simplex classification invariants, and runs local CDT move proposals through a Metropolis-Hastings sampler.

Current evidence concerns structural invariants and transition-kernel bookkeeping. Analytic ensemble validation, reference-implementation comparisons,
and continuum-limit physics are separate gates. The current action permits volume-changing `(1,3)` and `(3,1)` moves; production volume fixing and automated
coupling scans remain planned.

For the detailed scientific contract, ensemble scope, backend role, and parameter interpretation, see
[`docs/scientific-basis.md`][repo-docs-scientific-basis-md]. Move semantics and detailed-balance notes live in [`docs/moves.md`][repo-docs-moves-md] and
[`docs/metropolis.md`][repo-docs-metropolis-md].

## 🗺️ Documentation Map

The [documentation index][repo-docs-readme-md] maps user, scientific, and contributor guides.

- [CDT Spacetime Visualization notebook][repo-notebooks-01spacetimevisualization-ipynb] — example 1+1 CDT mesh visualization generator
- [CLI Examples][repo-docs-running-md] — command-line usage and output workflows
- [Code Organization][repo-docs-code-organization-md] — module layout, backend boundaries, and architecture notes
- [Example Scripts][repo-examples-scripts-readme-md] — maintained shell workflows for simulations, sweeps, and timing checks
- [Foliation][repo-docs-foliation-md] — time labels, spacelike/timelike classification, causality validation, and toroidal time handling
- [HPC Notebook Workflows][repo-docs-running-on-hpc-md] — Slurm, Open OnDemand, and cluster cache setup
- [Metropolis][repo-docs-metropolis-md] — proposal-before-mutation ordering, detailed balance, trace semantics, and sampler/backend boundaries
- [Moves][repo-docs-moves-md] — CDT local move semantics, proposal ratios, rollback behavior, and action calibration
- [Polars Analysis Caches notebook][repo-notebooks-02analysiscaches-ipynb] — local Parquet caches and diagnostic plots for debugging CDT CSV/JSON outputs
- [Quickstart notebook][repo-notebooks-00quickstart-ipynb] — notebook-first local 1+1 CDT run, parameter meanings, output files, and troubleshooting
- [References][repo-references-md] — physics, numerical, and computational-geometry citations
- [Roadmap][repo-docs-roadmap-md] — near-term work, higher-dimensional topology tracks, and non-goals
- [Scientific Basis][repo-docs-scientific-basis-md] — CDT scope, validated invariants, current ensemble, and interpretation boundaries

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

Performance validation uses [Criterion] workloads with shared measurement and reporting tools. Run `just bench-ci` for the CDT benchmark contract,
`just bench-save-last` for a fresh local baseline, and `just bench-latest-vs-last` for a comparison on the same host.

<!-- performance-summary:start -->

A fresh benchmark series starts with the September 2026 tooling update. No release comparison is published yet.
Future tables will show absolute baseline/current median times, units, confidence bounds, and relative changes together.

<!-- performance-summary:end -->

See [`benches/README.md`][repo-benches-readme-md] for benchmark details and [`docs/BENCHMARKING.md`][repo-docs-benchmarking-md] for comprehensive
performance testing workflow documentation.

## 🛣️ Roadmap

The high-level roadmap, including 1+1 maturity work, future 2+1 and 3+1 CDT topology tracks, observables, dual/Voronoi geometry, visualization, and non-goals,
lives in [`docs/roadmap.md`][repo-docs-roadmap-md].

## 🤝 Contributing

See [CONTRIBUTING.md][repo-contributing-md] for the full contributor guide: project layout, development workflow, code style, testing, documentation layout,
performance/benchmarking, and release support. Community expectations live in [CODE_OF_CONDUCT.md][repo-codeofconduct-md]. AI assistants should follow
[AGENTS.md][repo-agents-md].

The contributor guide owns setup, checks, fixes, tests, security scans, and PR preparation.

## 📚 Citation

If you use this software in academic work or downstream research software, cite the Zenodo DOI and include the software metadata from
[CITATION.cff][repo-citation-cff].

- DOI: <https://doi.org/10.5281/zenodo.20513228>
- Citation metadata: [CITATION.cff][repo-citation-cff]

```bibtex
@software{getchell_causal_triangulations,
  author = {Adam Getchell},
  title = {causal-triangulations: A Causal Dynamical Triangulation library for quantum gravity research},
  doi = {10.5281/zenodo.20513228},
  url = {https://github.com/acgetchell/causal-triangulations}
}
```

For release-specific fields such as version, release date, and ORCID, prefer [CITATION.cff][repo-citation-cff].

## 🔎 References

For a comprehensive list of academic references and bibliographic citations used throughout the library, see [REFERENCES.md][repo-references-md].

This includes foundational work on:

- Causal Dynamical Triangulations theory
- Monte Carlo methods in quantum gravity
- Computational geometry and Delaunay triangulations
- Discrete approaches to general relativity

## 🤖 AI-assisted Development

This repository contains an [AGENTS.md][repo-agents-md] file, which defines the rules and invariants for AI coding assistants and autonomous agents working on
this codebase.

Portions of this library were developed with the assistance of AI tools including [ChatGPT], [Claude], [Codex], and [CodeRabbit].

All accepted code and documentation changes are reviewed, edited, and validated by the author.

For tool citation metadata, see the [AI-assisted development tools][repo-references-mdai-assisted-development-tools] section of
[REFERENCES.md][repo-references-md].

## 📜 License

This project is licensed under the [BSD 3-Clause License][repo-license].

---

[Rust]: https://rust-lang.org
[Delaunay triangulation]: https://crates.io/crates/delaunay
[`markov-chain-monte-carlo`]: https://crates.io/crates/markov-chain-monte-carlo
[Metropolis-Hastings sampling]: https://crates.io/crates/markov-chain-monte-carlo
[Criterion]: https://github.com/bheisler/criterion.rs
[api]: https://docs.rs/causal-triangulations/latest/causal_triangulations/
[api-action]: https://docs.rs/causal-triangulations/latest/causal_triangulations/prelude/action/
[api-config]: https://docs.rs/causal-triangulations/latest/causal_triangulations/prelude/config/
[api-observables]: https://docs.rs/causal-triangulations/latest/causal_triangulations/prelude/observables/
[api-simulation]: https://docs.rs/causal-triangulations/latest/causal_triangulations/prelude/simulation/
[api-triangulation]: https://docs.rs/causal-triangulations/latest/causal_triangulations/prelude/triangulation/
[geometry-api]: https://docs.rs/delaunay/latest/delaunay/
[mcmc-api]: https://docs.rs/markov-chain-monte-carlo/latest/markov_chain_monte_carlo/
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

[repo-license]: https://github.com/acgetchell/causal-triangulations/blob/main/LICENSE
[repo-docs-scientific-basis-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md
[checkpoint-policy]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/metropolis.md#serialized-checkpoint-compatibility
[repo-changelog-md]: https://github.com/acgetchell/causal-triangulations/blob/main/CHANGELOG.md
[repo-docs-roadmap-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/roadmap.md
[repo-contributing-mdgetting-started]: https://github.com/acgetchell/causal-triangulations/blob/main/CONTRIBUTING.md#getting-started
[repo-docs-running-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/RUNNING.md
[repo-docs-running-on-hpc-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/RUNNING-ON-HPC.md
[repo-docs-moves-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/moves.md
[repo-docs-metropolis-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/metropolis.md
[repo-docs-readme-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/README.md
[repo-notebooks-01spacetimevisualization-ipynb]: https://github.com/acgetchell/causal-triangulations/blob/main/notebooks/01_spacetime_visualization.ipynb
[repo-docs-code-organization-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/code-organization.md
[repo-examples-scripts-readme-md]: https://github.com/acgetchell/causal-triangulations/blob/main/examples/scripts/README.md
[repo-docs-foliation-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/foliation.md
[repo-notebooks-02analysiscaches-ipynb]: https://github.com/acgetchell/causal-triangulations/blob/main/notebooks/02_analysis_caches.ipynb
[repo-notebooks-00quickstart-ipynb]: https://github.com/acgetchell/causal-triangulations/blob/main/notebooks/00_quickstart.ipynb
[repo-references-md]: https://github.com/acgetchell/causal-triangulations/blob/main/REFERENCES.md
[repo-benches-readme-md]: https://github.com/acgetchell/causal-triangulations/blob/main/benches/README.md
[repo-docs-benchmarking-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/BENCHMARKING.md
[repo-contributing-md]: https://github.com/acgetchell/causal-triangulations/blob/main/CONTRIBUTING.md
[repo-codeofconduct-md]: https://github.com/acgetchell/causal-triangulations/blob/main/CODE_OF_CONDUCT.md
[repo-agents-md]: https://github.com/acgetchell/causal-triangulations/blob/main/AGENTS.md
[repo-citation-cff]: https://github.com/acgetchell/causal-triangulations/blob/main/CITATION.cff
[repo-references-mdai-assisted-development-tools]: https://github.com/acgetchell/causal-triangulations/blob/main/REFERENCES.md#ai-assisted-development-tools
