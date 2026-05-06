# Roadmap

This roadmap records likely directions for the `causal-triangulations` crate. It is not a stability promise; release scope depends on scientific need, API maturity, and validation quality. Concrete implementation tasks should be tracked as GitHub issues; keep this document focused on release direction, candidate themes, and non-goals.

## v0.1.0 CDT Foundations

The v0.1.0 foundation work focuses on making the crate a usable, validated 1+1 CDT simulation library:

- [x] Trait-based geometry backend boundary around the `delaunay` crate
- [x] Explicit open-boundary CDT strip construction
- [x] Explicit toroidal S¹×S¹ CDT construction with χ = 0 validation
- [x] Per-vertex foliation labels, causality checks, and strict Up/Down cell classification
- [x] Real 2D ergodic move kernels over Delaunay backend edit operations
- [x] Proposal-before-mutation Metropolis loop with rollback and bounded local-site retries
- [x] Toroidal Metropolis regression coverage requiring at least 100 accepted moves while preserving topology and foliation
- [x] CLI and configuration support for open-boundary and toroidal topology selection
- [x] Volume-profile, Hausdorff-dimension, and spectral-dimension observables on the combinatorial dual graph
- [x] Repository validation loop covering Rust, Python support scripts, Semgrep rules, documentation, examples, and benchmarks

## 1+1 Maturity

Likely follow-up work before broadening the dimensional surface:

- Weight move-type selection by available application sites to reduce uniform-sampling bias
- Weight or enumerate accepted move-site retries so proposals bind to concrete local moves before acceptance
- Broaden per-kernel toroidal tests around spatial and temporal wrap-around cells
- Accept fixed triangle cells directly in explicit-cell generator APIs to remove per-triangle `Vec` adaptation
- Add manual foliation assignment APIs with the same validation and synchronization guarantees as constructor-assigned labels
- Add tutorial-style examples for open-boundary strips, toroidal runs, observables, and interpreting Metropolis acceptance behavior

## Higher-Dimensional CDT Tracks

The next CDT dimensions should advance as explicit topology tracks rather than a generic higher-dimensional bucket:

- 2+1 CDT with spherical spatial slices (S²) and toroidal spatial slices (T²), including constructor fixtures, foliation validation, local move kernels, Metropolis sampling, and topology-specific regression tests
- 3+1 CDT with spherical spatial slices (S³) and toroidal spatial slices (T³), following the same staged path after the required geometry-backend operations and invariants are available
- Periodic-time variants where the topology contract is well defined and the backend can validate the corresponding Euler/Poincaré-style invariants cleanly
- Dimension-specific action terms, simplex-count bookkeeping, volume profiles, and acceptance diagnostics

## Observables and Dual Geometry

CDT observables should remain user-facing analysis APIs and should grow in lockstep with validation:

- Extend volume observables from 1+1 slice profiles to spatial-volume profiles in 2+1 and 3+1 dimensions
- Add geodesic-distance distributions, shell-volume curves, two-point functions, and finite-size scaling helpers
- Add curvature-oriented Regge observables when the local simplex data is sufficiently validated
- Keep the current Hausdorff- and spectral-dimension estimators available on combinatorial dual adjacency graphs
- Reuse Voronoi tessellation support from the `delaunay` crate when it lands, so observables can opt into full dual/Voronoi cells rather than rebuilding only face- or cell-adjacency graphs
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

The crate should remain a focused CDT physics library layered over a geometry backend. General-purpose mesh editing, a replacement Delaunay implementation, publication plotting, broad MCMC diagnostics, and domain-specific downstream analyses belong in separate crates or tools unless they are needed to validate core CDT behavior.
