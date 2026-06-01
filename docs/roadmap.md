# Roadmap

This roadmap records likely directions for the `causal-triangulations` crate. It is not a stability promise; release scope depends on scientific need, API
maturity, and validation quality. Concrete implementation tasks should be tracked as GitHub issues; keep this document focused on release direction, candidate
themes, and non-goals.

## v0.1.0 CDT Foundations

The v0.1.0 foundation work focuses on making the crate a usable, validated 1+1 CDT simulation library:

- [x] Trait-based geometry backend boundary around the `delaunay` crate
- [x] Explicit open-boundary CDT strip construction
- [x] Explicit toroidal S¹×S¹ CDT construction with χ = 0 validation
- [x] Per-vertex foliation labels, causality checks, and strict Up/Down simplex classification
- [x] Real 2D ergodic move kernels over Delaunay backend edit operations
- [x] Concrete planned-proposal Metropolis-Hastings loop with forward/reverse local-site weighting
- [x] Toroidal Metropolis regression coverage requiring accepted periodic moves while preserving topology and foliation
- [x] CLI and configuration support for open-boundary and toroidal topology selection
- [x] Volume-profile, Hausdorff-dimension, and spectral-dimension observables on the combinatorial dual graph
- [x] Repository validation loop covering Rust, Python support scripts, Semgrep rules, documentation, examples, and benchmarks
- [ ] Support variable spatial volume profiles for 1+1 CDT initial geometries
  ([#141](https://github.com/acgetchell/causal-triangulations/issues/141))
- [ ] Track release-readiness gates for the first public release
  ([#140](https://github.com/acgetchell/causal-triangulations/issues/140))
- [ ] Audit public doctests for fallible error handling
  ([#151](https://github.com/acgetchell/causal-triangulations/issues/151))
- [x] Use chunked Metropolis sweeps in large-scale 1+1 CDT debug runs
  ([#152](https://github.com/acgetchell/causal-triangulations/issues/152)). The v0.1.0 scientific-utility bar requires the large-scale debug path to exercise
  Metropolis CDT behavior with literature-style sweeps sized from the current simplex count, not only the raw move kernel.
- [x] Align chunked Metropolis continuation with the upstream resumable sampler pattern
  ([#153](https://github.com/acgetchell/causal-triangulations/issues/153)). CDT chunk execution now drives the proposal-plan adapter through
  `markov-chain-monte-carlo` v0.4 sampler continuation while preserving CDT-owned measurements, proposal telemetry, and current-volume sweep sizing.
- [ ] Enforce the MCMC backend boundary
  ([#155](https://github.com/acgetchell/causal-triangulations/issues/155)). Because GitHub releases are archived by Zenodo, the DOI-backed `v0.1.0` release
  should not ship with generic Metropolis-Hastings mechanics duplicated in CDT-local code. Complete this after an updated `markov-chain-monte-carlo` release
  provides the needed upstream resumable-sampler and planned-step telemetry APIs
  ([markov-chain-monte-carlo#60](https://github.com/acgetchell/markov-chain-monte-carlo/issues/60),
  [markov-chain-monte-carlo#61](https://github.com/acgetchell/markov-chain-monte-carlo/issues/61)).

## 1+1 Maturity

Likely follow-up work before broadening the dimensional surface:

- Weight move-type selection by available application sites to reduce uniform-sampling bias
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
- Adopt the upstream lift-aware simplex barycenter helper once `delaunay#420` ships
  ([#147](https://github.com/acgetchell/causal-triangulations/issues/147)). This should replace the local `(1,3)` insertion-point policy while preserving the
  existing k=1 flip delegation, and is dependency-gated cleanup for move-kernel geometry ownership rather than a new CDT ensemble.
- Redesign public error shapes for ergonomic typed diagnostics
  ([#149](https://github.com/acgetchell/causal-triangulations/issues/149)). This should build on the current typed category enums by evaluating richer observed
  values, constraints, and matching patterns without blocking the v0.1.0 correctness work.
- Use exact Delaunay flip-feasibility validators for proposal-site accounting once `delaunay#419` ships
  ([#146](https://github.com/acgetchell/causal-triangulations/issues/146)). v0.1.0 should prove CDT's counted proposal sites match the sites sampled by the
  proposal kernel; backend-provided immutable feasibility checks are post-release dependency-gated tightening that avoids duplicating Delaunay dry-run policy.

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
