# Scientific Basis and Scope

This page records the scientific contract for `causal-triangulations`: what the crate implements and validates today, what belongs to downstream scientific
analysis, and where the current 1+1-dimensional foundation release deliberately stops.

For a notebook-first run, start with [`notebooks/00_quickstart.ipynb`](../notebooks/00_quickstart.ipynb). For command-line examples, see
[`docs/cli-examples.md`](cli-examples.md).

## CDT Model

Causal Dynamical Triangulations defines a gravitational path integral by summing over causally well-behaved triangulated spacetimes. The causal structure is
encoded by a discrete proper-time foliation: vertices carry time labels, edges are classified as spacelike or timelike, and simplices must respect the
allowed causal pattern.

This crate currently implements a validated 1+1-dimensional CDT foundation. It supports:

- open-boundary strip initial data;
- periodic S¹×S¹ toroidal initial data;
- local `(2,2)`, `(1,3)`, and `(3,1)` CDT move proposals;
- Regge-style action evaluation with configurable couplings;
- Metropolis-Hastings sampling through the `markov-chain-monte-carlo` backend;
- trace CSV and summary JSON exports for downstream analysis.

Higher-dimensional CDT, production volume fixing, automated λ scans, visualization/export workflows, and full ensemble-analysis tooling are outside the
current release scope. The roadmap tracks likely directions in [`docs/roadmap.md`](roadmap.md).

## What The Crate Validates

The validation contract is discrete and implementation-level. Constructors and simulation paths check:

- topology metadata and Euler-characteristic consistency for supported initial data;
- foliation labels and slice-size constraints;
- adjacent-slice causality, including periodic time distance for toroidal runs;
- strict Up/Down simplex classification in 1+1 dimensions;
- rollback or rejection of local move candidates that would break CDT invariants;
- proposal-before-mutation Metropolis ordering, with proposal asymmetry handled by the Hastings correction.

These checks make the implemented CDT state space and transition kernel explicit. They do not prove that a Markov chain has mixed, that a finite run is in an
asymptotic scaling regime, or that a chosen observable analysis supports a particular physical interpretation.

## Ensemble And Volume Behavior

Current simulations do not apply volume fixing. Volume-changing moves may change the total number of vertices and simplices during a run, so the sampled
ensemble is the grand-canonical, unfixed-volume ensemble defined by the configured CDT action and Metropolis-Hastings proposal rules.

This is intentional for now: in 1+1 CDT, unfixed-volume simulations controlled by the cosmological constant are a standard toy-model setting, as in Israel and
Lindner, [Quantum gravity on a laptop: 1+1 Dimensional Causal Dynamical Triangulation simulation](https://doi.org/10.1016/j.rinp.2012.10.001).

In a grand-canonical CDT ensemble, the cosmological constant is the coupling that controls volume growth or shrinkage because it is conjugate to the lattice
volume term in the action. Values too far from the useful finite-volume regime can drive the run toward minimum-volume configurations or toward rapid growth;
this is expected physics for the unfixed-volume ensemble, not an implementation failure.

Automated λ-scan utilities for finding practical finite-volume windows are planned as
[`causal-triangulations#143`](https://github.com/acgetchell/causal-triangulations/issues/143). For the current release, tune `--cosmological-constant`
manually and inspect volume, action, and acceptance diagnostics.

## Action Calibration

The default 1+1 action constants use:

```text
kappa_0 = 0
kappa_2 = 0
lambda_edge = (2 / 3) ln 2 ~= 0.46209812037329684
```

In pure 1+1 gravity, the curvature/Newton term is topological at fixed topology, so the default vertex and triangle couplings are zero. The exactly solved 2D
CDT model has critical cosmological coupling `lambda_c = ln 2` in the triangle-volume convention where configurations are weighted by `exp(-lambda N2)`.

This crate's historical action writes the cosmological term as `lambda_edge N1`. For closed toroidal 1+1 triangulations, `N1 = 3 N2 / 2`, so
`lambda_edge = (2 / 3) ln 2` maps the edge-count convention to the standard critical triangle-volume coupling. Open-boundary strips have boundary-count
corrections, so the same default should be treated as a practical baseline rather than an exact open-boundary critical value.

## Geometry Backend Role

The Delaunay backend is an implementation substrate, not the sampled physics ensemble. It gives the crate a robust way to construct an initial
piecewise-linear manifold, validate topology and adjacency, and perform checked local edit primitives.

After initialization, the simulation does not enforce the Delaunay condition as part of the Markov-chain state. The sampled ensemble is defined by CDT moves,
foliation/topology/causality constraints, the configured action, and Metropolis-Hastings acceptance.

## CDT++ Construction Lesson

The predecessor implementation, [`CDT-plusplus`](https://github.com/acgetchell/CDT-plusplus), used CGAL Delaunay triangulations as a geometric substrate and
then filtered the result through CDT causality rules. Its key reusable lesson is the separation between generic triangulation editing and CDT-domain validity:
start from a Delaunay-built PL manifold, classify cells by stored time labels, identify acausal or unclassified cells, delete selected offending vertices
through the geometry backend, and let the backend retriangulate the affected cavities before rechecking CDT invariants.

That approach is not treated as current validation evidence for this Rust crate. `CDT-plusplus` is referenced as implementation lineage and a source of design
experience; it may become a useful independent regression oracle if modernized enough to build and run representative fixtures. The planned Rust follow-up is
tracked as [`causal-triangulations#192`](https://github.com/acgetchell/causal-triangulations/issues/192): implement a causality-filtering Delaunay construction
path without moving generic PL-manifold editing into the CDT layer.

## Move And Sampler Contract

The CDT move layer owns domain-specific proposal sites and invariant checks. The generic Metropolis-Hastings mechanics are delegated to
`markov-chain-monte-carlo` through thin CDT adapters.

For details, see:

- [`docs/moves.md`](moves.md) for local move semantics, proposal-site definitions, rollback behavior, and volume-changing move interpretation;
- [`docs/metropolis.md`](metropolis.md) for proposal-before-mutation ordering, proposal-ratio correction, trace semantics, and sampler/backend boundaries;
- [`docs/foliation.md`](foliation.md) for time labels, spacelike/timelike classification, causality validation, and toroidal time handling.

## Volume Fixing

Higher-dimensional CDT studies often use explicit approximate volume fixing for finite-size numerical work. For example, Ambjørn et al. discuss quadratic
volume fixing in [The Semiclassical Limit of Causal Dynamical Triangulations](https://arxiv.org/abs/1102.3929), and the toroidal phase-structure study uses
quadratic volume fixing in [The phase structure of Causal Dynamical Triangulations with toroidal spatial topology](https://arxiv.org/abs/1802.10434).

This crate may add such a mode later, but it should be opt-in because it samples a modified action rather than the current bare unfixed-volume ensemble.

## User Responsibilities

Downstream analyses remain responsible for:

- choosing physically meaningful parameter ranges;
- running enough independent chains or diagnostics to assess mixing;
- estimating burn-in, autocorrelation, and finite-size effects;
- interpreting observables with appropriate uncertainty estimates;
- documenting when a modified ensemble, such as volume fixing, is being sampled.

The crate's job is to make the implemented discrete state, transition kernel, and exported diagnostics explicit enough that those scientific choices can be
reviewed rather than hidden.
