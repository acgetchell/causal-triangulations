# causal-triangulations

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.20513228.svg)](https://doi.org/10.5281/zenodo.20513228)
[![Crates.io](https://badgen.net/crates/v/causal-triangulations)](https://crates.io/crates/causal-triangulations)
[![Downloads](https://badgen.net/crates/d/causal-triangulations)](https://crates.io/crates/causal-triangulations)
[![License](https://badgen.net/github/license/acgetchell/causal-triangulations)][repo-license]
[![Docs.rs](https://docs.rs/causal-triangulations/badge.svg)](https://docs.rs/causal-triangulations)
[![CI][ci-badge]][ci-workflow]
[![CodeQL][codeql-badge]][codeql-workflow]
[![zizmor][zizmor-badge]][zizmor-workflow]
[![rust-clippy analyze][clippy-badge]][clippy-workflow]
[![Codecov](https://codecov.io/gh/acgetchell/causal-triangulations/graph/badge.svg?token=CsbOJBypGC)](https://codecov.io/gh/acgetchell/causal-triangulations)
[![Audit dependencies][audit-badge]][audit-workflow]
[![OSV-Scanner][osv-badge]][osv-workflow]
[![Gitleaks][gitleaks-badge]][gitleaks-workflow]

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
- [AI Agents](#-ai-agents)
- [License](#-license)

## 🌌 Introduction

This library implements **Causal Dynamical Triangulations (CDT)** in [Rust]. CDT is a non-perturbative approach to quantum gravity defining the gravitational
path integral over causally triangulated spacetimes and evaluating it using Markov Chain Monte Carlo. For an introduction to CDT, see Ambjørn and Loll (1998),
[“Non-perturbative Lorentzian quantum gravity, causality and topology change”](https://arxiv.org/abs/hep-th/9805108). The library leverages high-performance
[Delaunay triangulation] backends and provides a foundational toolkit for CDT research and exploration.

## Use this crate when

Use this crate for 1+1 CDT simulations on strips or tori, with resumable Metropolis-Hastings runs and finite-lattice diagnostics through Rust, the CLI,
or notebooks.

## API and model scope

See the [CDT model][scientific-model] for supported geometries, boundary conditions, and initial-size requirements.

| Need | API entry point |
| --- | --- |
| Configure a run | [`prelude::config`][api-config] and `CdtConfig::into_validated` |
| Construct and inspect CDT states | [`prelude::triangulation`][api-triangulation] |
| Evaluate an action | [`prelude::action`][api-action] |
| Measure profiles and finite-graph observables | [`prelude::observables`][api-observables] |
| Run or resume a simulation | [`prelude::simulation`][api-simulation] |

The [API reference][api] describes the latest published crate; `just doc-check` builds the reference for this checkout.

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
- Versioned JSON checkpoints for exact MCMC continuation, using Delaunay's Level 4 snapshot with checked geometry and CDT state restoration;
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

The quickstart notebook runs the `cdt` engine and plots its CSV/JSON outputs. Rust 1.99.0 or newer is required; repository tooling and notebooks use the
declared uv environment. See [CLI workflows][repo-docs-running-md] for scriptable runs and [cluster workflows][repo-docs-running-on-hpc-md] for Slurm and Open
OnDemand.

## 🧪 Scientific Basis

The [scientific basis][repo-docs-scientific-basis-md] owns [action calibration][scientific-action], [ensemble and volume behavior][scientific-ensemble],
[validation limits][scientific-validation], and [downstream analysis responsibilities][scientific-responsibilities]. Move semantics and detailed-balance
notes live in [the move guide][repo-docs-moves-md] and [the sampler guide][repo-docs-metropolis-md].

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

See [geometry and sampling responsibilities][scientific-backends] for the boundaries between these crates.

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

<!-- Preserve links to the former AI-assisted Development heading. -->
<!-- markdownlint-disable-next-line MD033 -->
<a id="-ai-assisted-development"></a>

## 🤖 AI Agents

AI coding assistants should read [AGENTS.md][repo-agents-md] before proposing or applying changes. See [CONTRIBUTING.md][ai-development-guide] for the
repository's AI-assisted development note.

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
[ci-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml/badge.svg
[ci-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml
[codeql-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/codeql.yml/badge.svg
[codeql-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/codeql.yml
[zizmor-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/zizmor.yml/badge.svg
[zizmor-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/zizmor.yml
[clippy-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml/badge.svg
[clippy-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml
[audit-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml/badge.svg
[audit-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml
[osv-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/osv.yml/badge.svg
[osv-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/osv.yml
[gitleaks-badge]: https://github.com/acgetchell/causal-triangulations/actions/workflows/gitleaks.yml/badge.svg
[gitleaks-workflow]: https://github.com/acgetchell/causal-triangulations/actions/workflows/gitleaks.yml

[repo-license]: https://github.com/acgetchell/causal-triangulations/blob/main/LICENSE
[repo-docs-scientific-basis-md]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md
[scientific-model]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md#cdt-model
[scientific-action]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md#action-calibration
[scientific-ensemble]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md#ensemble-and-volume-behavior
[scientific-validation]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md#what-the-crate-validates
[scientific-responsibilities]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md#user-responsibilities
[scientific-backends]: https://github.com/acgetchell/causal-triangulations/blob/main/docs/scientific-basis.md#geometry-backend-role
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
[ai-development-guide]: https://github.com/acgetchell/causal-triangulations/blob/main/CONTRIBUTING.md#ai-assisted-development
[repo-codeofconduct-md]: https://github.com/acgetchell/causal-triangulations/blob/main/CODE_OF_CONDUCT.md
[repo-agents-md]: https://github.com/acgetchell/causal-triangulations/blob/main/AGENTS.md
[repo-citation-cff]: https://github.com/acgetchell/causal-triangulations/blob/main/CITATION.cff
