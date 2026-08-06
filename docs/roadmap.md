# Roadmap

This roadmap records likely directions for the `causal-triangulations` crate. It is not a stability promise; release scope depends on scientific need, API
maturity, and validation quality. Concrete implementation tasks should be tracked as GitHub issues; keep this document focused on release direction, candidate
themes, and non-goals.

## v0.1.0 CDT Foundations

The v0.1.0 foundation release centers on validated 1+1 CDT construction, toroidal volume-changing moves, nonuniform initial volume profiles, upstream-backed
MCMC sampler mechanics, trace/summary exports, resumable checkpoints, and CI-aligned validation tooling.

Current scope: higher-dimensional CDT, production volume fixing, automated λ scans, visualization/export workflows, and full ensemble-analysis tooling remain
roadmap work.

Completed foundation work:

- [x] Trait-based geometry backend boundary around the `delaunay` crate.
- [x] Validated open-boundary and toroidal S¹×S¹ 1+1 CDT constructors, including explicit per-slice spatial volume profiles.
- [x] Per-vertex foliation labels, adjacent-slice causality checks, topology validation, and strict Up/Down simplex classification.
- [x] Real 2D local CDT move kernels over checked Delaunay edit operations, with rollback or rejection on invariant failures.
- [x] Causality-filtering Delaunay construction path inspired by CDT++ ([#192](https://github.com/acgetchell/causal-triangulations/issues/192)), with repeated
  removal of vertices incident to non-strict causal simplices until the strict violation count converges to zero.
- [x] Upstream Level 1-4 embedded/non-overlapping validation for evolved states
  ([#195](https://github.com/acgetchell/causal-triangulations/issues/195)), separated from optional Level 5 Delaunay validation and mandatory strict causal
  profiles ([#196](https://github.com/acgetchell/causal-triangulations/issues/196)).
- [x] Planned-proposal Metropolis execution through `markov-chain-monte-carlo`, including proposal-site weighting and resumable chunked sweeps.
- [x] CLI support, trace/summary exports, checkpoints, and core observables for downstream analysis.
- [x] Repository validation loop covering Rust, Python support scripts, Semgrep rules, documentation, notebooks, examples, and benchmarks.
- [x] Release-readiness work for Rust 1.96.0, public doctest fallibility, CI/security alignment, and the first DOI-backed foundation release.

## 1+1 Maturity

Likely follow-up work before broadening the dimensional surface:

- Evaluate move-family weighting by available application sites to improve proposal efficiency while preserving the documented Hastings correction
- Broaden per-kernel toroidal tests around spatial and temporal wrap-around simplices
- Accept fixed triangle simplices directly in explicit-simplex generator APIs to remove per-triangle `Vec` adaptation
- Add manual foliation assignment APIs with the same validation and synchronization guarantees as constructor-assigned labels
- Add tutorial-style examples for open-boundary strips, toroidal runs, observables, and interpreting Metropolis acceptance behavior

## v0.1.1 Maturity And Ensemble Controls

The post-v0.1.0 1+1 track should keep the default ensemble scientifically explicit while adding opt-in controls, dependency-gated cleanup, and public API
polish that are useful but not required for the first DOI-backed foundation release:

- Add optional CDT volume fixing for bounded large-scale simulations
  ([#142](https://github.com/acgetchell/causal-triangulations/issues/142)). Volume fixing should be opt-in and documented as a modified action, not the default
  unfixed-volume ensemble calibrated to the 1+1 CDT `lambda_c = ln 2` critical coupling.
- Add λ scan utilities for unfixed-volume 1+1 CDT runs
  ([#143](https://github.com/acgetchell/causal-triangulations/issues/143)). These should help users tune the cosmological constant, which is conjugate to the
  lattice volume term and controls volume growth or shrinkage in unfixed-volume runs.
- Added a Polars notebook workflow for CDT analysis caches
  ([#176](https://github.com/acgetchell/causal-triangulations/issues/176)). The notebook shows how to load the stable CSV/JSON outputs, reshape volume-profile
  data for exploratory analysis, and cache local Parquet files without adding plotting or Parquet dependencies to the Rust crate.
- Adopt the upstream lift-aware simplex barycenter helper once `delaunay#420` ships
  ([#147](https://github.com/acgetchell/causal-triangulations/issues/147)). This should replace the local `(1,3)` insertion-point policy while preserving the
  existing k=1 flip delegation, and is dependency-gated cleanup for move-kernel geometry ownership rather than a new CDT ensemble.
- Redesign public error shapes for ergonomic typed diagnostics
  ([#149](https://github.com/acgetchell/causal-triangulations/issues/149)). This should build on the current typed category enums by evaluating richer observed
  values, constraints, and matching patterns without blocking the v0.1.0 correctness work.
- [x] Use exact Delaunay flip-feasibility validators for proposal-site accounting after `delaunay#419` shipped
  ([#146](https://github.com/acgetchell/causal-triangulations/issues/146)). Representative open-boundary and toroidal regressions compare counted proposal
  sites with the sampler denominator, while finite-but-degenerate insertion and non-collapsible removal fixtures verify that immutable backend preflights reject
  candidates without adding clone-and-try work to production scans.

## Later Distribution Ergonomics

- Ship prebuilt `cdt` release binaries
  ([#169](https://github.com/acgetchell/causal-triangulations/issues/169)). This should make the command-line tool easier to try once the notebook and local
  source-build path are stable, while keeping `cargo install` and local release builds supported.

## Higher-Dimensional CDT Tracks

The next CDT dimensions should advance as explicit topology tracks rather than a generic higher-dimensional bucket:

- v0.2.0 should focus on 2+1 CDT with toroidal spatial slices (T²), including constructor fixtures, foliation validation, local move kernels, Metropolis
  sampling, and topology-specific regression tests. The current scoped issue is 2+1 toroidal CDT triangulations
  ([#144](https://github.com/acgetchell/causal-triangulations/issues/144)). This keeps the 2+1 milestone useful without blocking on backend spherical-topology
  support.
- v0.3.0 should add 2+1 CDT with spherical spatial slices (S²) after spherical topology lands in the `delaunay` crate. Treat this as a separate topology track
  rather than as a prerequisite for the first usable 2+1 toroidal release.
- 3+1 CDT with spherical spatial slices (S³) and toroidal spatial slices (T³), following the same staged path after the required geometry-backend operations
  and invariants are available
- Periodic-time variants where the topology contract is well defined and the backend can validate the corresponding Euler/Poincaré-style invariants cleanly
- Dimension-specific action terms, simplex-count bookkeeping, volume profiles, and acceptance diagnostics

## Observables and Dual Geometry

CDT observables should remain user-facing analysis APIs and should grow in lockstep with validation:

- Extend volume observables from 1+1 slice profiles to spatial-volume profiles in 2+1 and 3+1 dimensions
- Add geodesic-distance distributions, shell-volume curves, two-point functions, and finite-size scaling helpers
- Add curvature-oriented Regge observables when the local simplex data is sufficiently validated
- Keep the current Hausdorff- and spectral-dimension estimators available on combinatorial dual adjacency graphs
- Reuse Voronoi tessellation support from the `delaunay` crate when it lands, so observables can opt into full dual/Voronoi regions rather than rebuilding only
  face- or simplex-adjacency graphs
- Preserve a clear distinction between combinatorial dual graphs, geometric Voronoi tessellations, and visualization/export representations

## Visualization and Workflow Support

Visualization should help inspect CDT structure without turning this crate into a plotting package:

- Export mesh and graph data in common interchange formats for external visualization tools
- Provide lightweight examples for rendering foliated triangulations, slice volumes, dual graphs, and sampled histories
- Add optional diagnostic outputs for move acceptance, topology preservation, volume evolution, and diffusion-return curves
- Keep publication-quality plotting and broad downstream statistical analysis in companion tools unless needed for core CDT validation

## Longer-Term Ideas

Exploratory directions:

- Support additional topology and boundary-condition families when geometry backend invariants can validate them cleanly
- Add ensemble-analysis helpers for CDT research workflows beyond the core estimators
- Integrate parallel-chain workflows while keeping random-stream management explicit
- Explore alternative discrete gravity actions once the Regge-action path is well covered

## Non-Goals

The crate should remain a focused CDT physics library layered over a geometry backend. General-purpose mesh editing, a replacement Delaunay implementation,
publication plotting, broad MCMC diagnostics, and domain-specific downstream analyses belong in separate crates or tools unless they are needed to validate core
CDT behavior.
