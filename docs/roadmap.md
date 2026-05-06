# Roadmap

This roadmap records likely directions for the `causal-triangulations` crate. It is not a stability promise; release scope depends on scientific need, API maturity, and validation quality. Concrete implementation tasks should be tracked as GitHub issues; keep this document focused on release direction, candidate themes, and non-goals.

## v0.1.0 CDT Foundations

The v0.1.0 foundation work focuses on making the crate a usable, validated 1+1 CDT simulation library:

- [x] Trait-based geometry backend boundary around the `delaunay` crate
- [x] Explicit open-boundary CDT strip construction
- [x] Explicit toroidal S1 x S1 CDT construction with chi = 0 validation
- [x] Per-vertex foliation labels, causality checks, and strict Up/Down cell classification
- [x] Real 2D ergodic move kernels over Delaunay backend edit operations
- [x] Proposal-before-mutation Metropolis loop with rollback and bounded local-site retries
- [x] Toroidal Metropolis regression coverage requiring at least 100 accepted moves while preserving topology and foliation
- [x] CLI and configuration support for open-boundary and toroidal topology selection
- [x] Repository validation loop covering Rust, Python support scripts, Semgrep rules, documentation, examples, and benchmarks

## Near-Term Candidates

Likely follow-up work:

- Weight move-type selection by available application sites to reduce uniform-sampling bias
- Weight or enumerate accepted move-site retries so proposals bind to concrete local moves before acceptance
- Broaden per-kernel toroidal tests around spatial and temporal wrap-around cells
- Accept fixed triangle cells directly in explicit-cell generator APIs to remove per-triangle `Vec` adaptation
- Add manual foliation assignment APIs with the same validation and synchronization guarantees as constructor-assigned labels
- Expand simulation observables and statistical output beyond the current action and simplex-count measurements
- Add tutorial-style examples for open-boundary strips, toroidal runs, and interpreting Metropolis acceptance behavior

## Longer-Term Ideas

Exploratory directions:

- Extend CDT construction, validation, and move kernels beyond 1+1 dimensions
- Support additional topology and boundary-condition families when geometry backend invariants can validate them cleanly
- Add visualization or mesh-export workflows for inspecting generated triangulations and sampled histories
- Add finite-size scaling and ensemble-analysis helpers for CDT research workflows
- Integrate parallel-chain workflows while keeping random-stream management explicit
- Explore alternative discrete gravity actions once the Regge-action path is well covered

## Non-Goals

The crate should remain a focused CDT physics library layered over a geometry backend. General-purpose mesh editing, a replacement Delaunay implementation, publication plotting, broad MCMC diagnostics, and domain-specific downstream analyses belong in separate crates or tools unless they are needed to validate core CDT behavior.
