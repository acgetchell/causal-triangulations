# Code Organization Guide

This document lists the files checked into the repository and summarizes the architecture-sensitive module boundaries for the causal-triangulations crate.

## Project Structure

### Complete Directory Tree

> **Tip**: regenerate this tree from tracked files:
>
> ```bash
> git --no-pager ls-files | LC_ALL=C sort | \
>   LC_ALL=C tree -a --charset UTF-8 --dirsfirst --noreport -F --fromfile
> ```
>
> This keeps the directory tree synchronized with the files that GitHub will display.

```text
causal-triangulations/
├── .github/
│   ├── actions/
│   │   └── setup-just/
│   │       └── action.yml
│   ├── workflows/
│   │   ├── audit.yml
│   │   ├── ci.yml
│   │   ├── codecov.yml
│   │   ├── codeql.yml
│   │   ├── dependabot-auto-merge.yml
│   │   ├── performance.yml
│   │   ├── rust-clippy.yml
│   │   ├── semgrep-sarif.yml
│   │   └── zizmor.yml
│   ├── CODEOWNERS
│   └── dependabot.yml
├── benches/
│   ├── support/
│   │   └── or_abort.rs
│   ├── README.md
│   ├── cdt_benchmarks.rs
│   └── ci_performance_suite.rs
├── docs/
│   ├── assets/
│   │   ├── cdt_spacetime.png
│   │   └── cdt_spacetime.svg
│   ├── dev/
│   │   ├── commands.md
│   │   ├── python.md
│   │   ├── rust.md
│   │   ├── testing.md
│   │   └── tooling-alignment.md
│   ├── RELEASING.md
│   ├── cli-examples.md
│   ├── code-organization.md
│   ├── foliation.md
│   ├── hpc.md
│   ├── metropolis.md
│   ├── moves.md
│   ├── performance-testing.md
│   ├── roadmap.md
│   ├── scientific-basis.md
│   └── testing.md
├── examples/
│   ├── scripts/
│   │   ├── README.md
│   │   ├── basic_simulation.sh
│   │   ├── parameter_sweep.sh
│   │   └── performance_test.sh
│   ├── basic_cdt.rs
│   ├── find_good_seeds.rs
│   ├── observables.rs
│   └── output_and_checkpoint.rs
├── notebooks/
│   ├── 00_quickstart.ipynb
│   ├── 01_spacetime_visualization.ipynb
│   └── 02_analysis_caches.ipynb
├── proptest-regressions/
│   └── cdt/
│       └── triangulation.txt
├── scripts/
│   ├── tests/
│   │   ├── __init__.py
│   │   ├── conftest.py
│   │   ├── test_archive_changelog.py
│   │   ├── test_benchmark_models.py
│   │   ├── test_benchmark_utils.py
│   │   ├── test_coverage_report.py
│   │   ├── test_hardware_utils.py
│   │   ├── test_postprocess_changelog.py
│   │   ├── test_subprocess_utils.py
│   │   └── test_tag_release.py
│   ├── README.md
│   ├── archive_changelog.py
│   ├── benchmark_models.py
│   ├── benchmark_utils.py
│   ├── check_semgrep_fixtures.py
│   ├── coverage_report.py
│   ├── hardware_utils.py
│   ├── notebook_check.py
│   ├── performance_analysis.py
│   ├── postprocess_changelog.py
│   ├── run_all_examples.sh
│   ├── subprocess_utils.py
│   └── tag_release.py
├── src/
│   ├── cdt/
│   │   ├── action.rs
│   │   ├── ergodic_moves.rs
│   │   ├── foliation.rs
│   │   ├── metropolis/
│   │   │   ├── adapter.rs
│   │   │   ├── checkpoint.rs
│   │   │   ├── helpers.rs
│   │   │   ├── runner.rs
│   │   │   └── telemetry.rs
│   │   ├── observables.rs
│   │   ├── results.rs
│   │   └── triangulation/
│   │       ├── builders.rs
│   │       ├── foliation.rs
│   │       ├── moves.rs
│   │       ├── state.rs
│   │       └── validation.rs
│   ├── geometry/
│   │   ├── backends/
│   │   │   ├── delaunay.rs
│   │   │   └── mock.rs
│   │   ├── coordinates.rs
│   │   ├── generators.rs
│   │   ├── operations.rs
│   │   └── traits.rs
│   ├── config.rs
│   ├── errors.rs
│   ├── lib.rs
│   ├── main.rs
│   └── util.rs
├── tests/
│   ├── semgrep/
│   │   ├── .github/
│   │   │   └── workflows/
│   │   │       └── action_policy.yml
│   │   ├── docs/
│   │   │   └── command_order.sh
│   │   ├── doctests/
│   │   │   └── unwrap_expect.txt
│   │   ├── notebooks/
│   │   │   ├── clean.ipynb
│   │   │   └── dirty.ipynb
│   │   ├── scripts/
│   │   │   └── tests/
│   │   │       └── python_exceptions.py
│   │   └── src/
│   │       └── project_rules/
│   │           ├── bench_example_usage.rs
│   │           └── rust_style.rs
│   ├── cli.rs
│   ├── integration_tests.rs
│   ├── large_scale_debug.rs
│   ├── physics_integration.rs
│   ├── proptest_foliation.rs
│   ├── proptest_metropolis.rs
│   └── regressions.rs
├── .bencher.toml
├── .codecov.yml
├── .coderabbit.yml
├── .gitignore
├── .python-version
├── .taplo.toml
├── .yamllint
├── AGENTS.md
├── CHANGELOG.md
├── CODE_OF_CONDUCT.md
├── CONTRIBUTING.md
├── CITATION.cff
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── README.md
├── REFERENCES.md
├── SECURITY.md
├── cliff.toml
├── clippy.toml
├── dprint.json
├── justfile
├── pyproject.toml
├── rumdl.toml
├── rust-toolchain.toml
├── rustfmt.toml
├── semgrep.yaml
├── ty.toml
├── typos.toml
└── uv.lock
```

## Architecture Layers

The crate is split into two intentionally different layers:

- `src/geometry/` is the backend interface layer. It is the only layer that talks directly to the `delaunay` crate, wrapping upstream types behind crate-owned
  traits, opaque handles, generators, and backend adapters.
- `src/cdt/` is the CDT domain layer. It owns causal triangulation semantics: foliation, topology and causality checks, ergodic moves, Regge action, Metropolis
  sampling adapters, measurements, and observables.

Code outside `src/geometry/` must not import `delaunay::` directly. CDT modules should depend on `TriangulationQuery` / `TriangulationMut`, crate-owned Delaunay
handle wrappers, `DelaunayBackend2D`, and generator functions from `crate::geometry`.

Generic MCMC mechanics should be delegated to `markov-chain-monte-carlo` through thin CDT adapters. CDT owns domain state, proposal-site enumeration,
foliation/topology validation, measurements, and result translation; the upstream MCMC crate owns Metropolis-Hastings acceptance, proposal-ratio application,
chain counters, planned-proposal commit ordering, and reusable sampler continuation behavior. Repository-owned Semgrep rules enforce the issue #155 boundary
against new CDT-local generic acceptance draws or manual accepted/rejected sampler counters.

## Repository Areas

- `src/` — core Rust library modules and the `cdt` binary entry point. The detailed source file map is below.
- `examples/` — runnable public API examples and shell workflows for simulation, observables, output, checkpoints, parameter sweeps, and performance checks.
- `notebooks/` — notebook front ends and analysis consumers for CLI-generated artifacts such as trace CSV and JSON summary files. Notebook code may run the
  binary for tutorials, but simulation logic stays in Rust.
- `tests/` — CLI, integration, physics, property-based, regression, slow-debug, and project-rule tests.
- `benches/` — Criterion benchmark harnesses and CI performance suites, with shared fail-fast fixture setup support under `benches/support/`.
- `docs/` — user guides, architecture notes, development rules, and release/testing/performance documentation.
- `scripts/` — Python and shell support tooling for benchmarks, coverage, changelog/release work, examples, and validation.

## Key Modules

### `cdt/foliation.rs` — Foliation

Assigns each vertex to a discrete time slice, enabling classification of edges as spacelike or timelike and triangles as up or down. See `docs/foliation.md` for
design details.

- `Foliation` — aggregate bookkeeping (per-slice vertex counts, total slices)
- `EdgeType` — `Spacelike` (same slice) or `Timelike` (adjacent slices)
- `SimplexType` — `Up` (2,1) or `Down` (1,2) triangle classification, encoded as `i32` simplex data
- Time labels are stored directly as vertex data (`Vertex.data: Option<u32>`), mirroring CDT-plusplus’s `vertex->info()`

### `cdt/triangulation/` — CDT Triangulation State and Builders

This is CDT domain logic layered over the geometry backend interface. It may use `DelaunayBackend2D` and crate-owned Delaunay handles, but it does not reach
through to upstream `delaunay::` APIs directly.

- Owns the `CdtTriangulation` state, `CdtMetadata`, `SimulationEvent`, metadata validation, cached simplex-count accessors, and common backend-agnostic state
  methods
- `CdtSimplexCounts` carries the CDT-domain proof that constructed triangulations have positive vertex, edge, and triangle counts as `NonZeroUsize`, while
  raw geometry queries remain `usize` so backend construction, clearing, and collection-style count APIs can represent zero
- `from_cdt_strip(vertices_per_slice, num_slices)` — Delaunay-built open-boundary 1+1 CDT strip with strict Up/Down simplex classification and upstream Level
  1–4 Delaunay validation before wrapping
- `from_filtered_delaunay_strip(vertices_per_slice, num_slices)` — open-boundary 1+1 CDT initial-state constructor that starts from surplus labeled Delaunay
  points, removes vertices incident to non-strict CDT simplices through the backend `remove_vertex` path, and returns only after the strict causal simplex
  violation count converges to zero and full initial validation passes
- `from_cdt_strip_profile(volume_profile)` — open-boundary 1+1 CDT strip from explicit nonuniform per-slice vertex counts; builds labeled point data and
  delegates triangulation to the Delaunay constructor before strict initial validation
- `from_toroidal_cdt(vertices_per_slice, num_slices)` — periodic Delaunay S¹×S¹ toroidal CDT (χ = 0) with upstream Level 1–4 validation before wrapping;
  requires `vertices_per_slice ≥ 3` and `num_slices ≥ 3`
- `from_toroidal_cdt_profile(volume_profile)` — periodic S¹×S¹ toroidal CDT from explicit per-slice vertex counts, preserving closed spatial slices and
  periodic time
- `assign_foliation_by_y(num_slices)` — bin existing vertices into time slices
- Query methods: `time_label`, `edge_type`, `vertices_at_time`, `slice_sizes`, `has_foliation`, `strict_causal_simplex_violation_count`
- Validation: constructors require upstream Delaunay Level 1–4 validation for initial meshes; `validate()` is the post-move/final-state contract and requires
  upstream structural validity plus CDT topology, foliation, causality, and strict Up/Down simplex classification
- Mutable backend access is not exposed. CDT code mutates Delaunay state only through narrow crate-internal operations (`flip_edge`, `subdivide_face`,
  `remove_vertex`, `set_vertex_data`) that invalidate cached counts and foliation synchronization bookkeeping on success.

The implementation lives under `src/cdt/triangulation/` and is wired from `src/lib.rs` to avoid `mod.rs` files:

- `builders.rs` — Delaunay-backed random/seeded/labeled builders plus strip and periodic toroidal CDT builders
- `foliation.rs` — foliation assignment, slice and label queries, volume profiles, simplex/edge classification, and foliation synchronization
- `moves.rs` — narrow crate-internal Delaunay mutation hooks used by ergodic moves
- `state.rs` — module entry point, `CdtTriangulation`, `CdtMetadata`, `SimulationEvent`, serialization, cached geometry accessors, and backend-agnostic
  state methods
- `validation.rs` — full CDT validation and Delaunay-backed causality checks

### `config.rs` — `CdtTopology` enum

- `OpenBoundary` (default) — finite strip with boundary, χ ∈ {1, 2}
- `Toroidal` — periodic in space and time, S¹×S¹, χ = 0
- Wired through `CdtConfig.topology`, `CdtConfigOverrides.topology`, the CLI `--topology` flag, and `CdtMetadata.topology`
- `run_simulation()` accepts `ValidatedCdtConfig` and dispatches on topology and profile mode: regular `Toroidal` → `from_toroidal_cdt`, regular
  `OpenBoundary` → `from_cdt_strip`, and explicit `CdtConfig.volume_profile` → the matching profile constructor; `vertices` is always the total vertex count

### `cdt/metropolis/` — Metropolis move ordering

`MetropolisAlgorithm::run()` is the current CDT production runner for proposal-before-mutation sampling. It proposes a move type, samples an explicit local
proposal site from the same universe used for proposal counts, plans that site on a cloned triangulation, computes `ΔS` and the forward/reverse
Metropolis-Hastings site-count ratio, accepts or rejects the concrete proposal, and only replaces the live triangulation after acceptance. Ordinary causal,
geometric, and backend edit failures are self-loop proposal outcomes recorded in `ProposalStatistics`; hard backend failures remain structured errors.
Toroidal move finalization rejects and rolls back candidate sites that would violate χ = 0 or the closed-S¹ per-slice foliation invariant.

The module tree is declared from `src/lib.rs` to avoid the `metropolis.rs` plus `metropolis/` layout pattern. `adapter.rs` is the single CDT adapter boundary
for `markov-chain-monte-carlo` proposal and target traits. `runner.rs` owns `MetropolisAlgorithm::run_steps`, which rebuilds the upstream `Chain`/`Sampler`
continuation view, delegates generic acceptance, proposal-ratio application, chain counters, and planned-proposal commit ordering to the upstream MCMC crate,
then records CDT-specific telemetry, measurements, and result state. `checkpoint.rs` owns resumable checkpoint state and resume validation, `telemetry.rs` owns
public step/proposal telemetry, and `helpers.rs` holds shared CDT-domain calculations.

See `docs/metropolis.md` for the current planned-proposal ordering and
[`causal-triangulations#155`](https://github.com/acgetchell/causal-triangulations/issues/155) for the remaining MCMC boundary follow-up.

### `cdt/results.rs` — Simulation outputs

- `Measurement` records per-step action, simplex counts, and optional per-slice volume profiles.
- `SimulationResultsBackend` owns the final triangulation, Monte Carlo step telemetry, upstream scalar trace rows, move statistics, and measurement history.
- Result methods summarize acceptance rate, average action, post-thermalization volume profiles, sample volume fluctuations, and final-state Hausdorff/spectral
  dimension estimates.
- `scalar_trace()` and `write_trace_csv()` expose step diagnostics through `markov-chain-monte-carlo::Trace`, using a rectangular CSV table suitable for Polars
  and other downstream dataframe tools.

### `cdt/observables.rs` — User-facing estimators

- `estimate_hausdorff_dimension` — estimates Hausdorff dimension from combinatorial dual-graph geodesic ball growth, returning `None` when the triangulation
  is too small or live face adjacency cannot be resolved
- `estimate_spectral_dimension` — estimates spectral dimension from dual-graph diffusion return probability, returning `None` when the graph is too small or
  lacks enough positive return-probability samples for a fit
- `CdtTriangulation::volume_profile` measures per-slice triangle counts on a triangulation; `SimulationResultsBackend` provides aggregate volume-profile
  summaries for simulation outputs
- Import triangulation-focused analysis APIs through `prelude::observables`; use `prelude::simulation` when constructing or inspecting simulation result
  containers

### `geometry/traits.rs` — Backend-neutral interface

- `GeometryBackend` defines associated coordinate, handle, and error types for a geometry implementation
- `TriangulationQuery` is the read-only surface used by CDT logic for counts, handles, adjacency, coordinates, face vertices, and validation
- `TriangulationMut` is the narrow mutation surface used by CDT-owned move kernels through CDT state mutation methods, not broad mutable backend exposure
- Result structs such as `FlipResult`, `EdgeAdjacentFaces`, and `SubdivisionResult` keep local topology operations backend-neutral
- Use `prelude::geometry` for real backend construction and geometry traits; use `prelude::testing` for mock-backend doctests or downstream fixture code

### `geometry/coordinates.rs` — Crate-Owned Coordinate Types

- `SpacetimeCoordinate` parses raw 2D backend coordinate vectors into finite 1+1 coordinates with named spatial and time accessors
- The type serializes as a two-element `[x, y]` array for notebook and JSON compatibility while keeping CDT code from depending on raw coordinate indexes
- `SpacetimeCoordinateError` reports wrong-dimensional and non-finite raw coordinate payloads at the boundary

### `geometry/backends/delaunay.rs` — Delaunay adapter

- Wraps the upstream `delaunay` triangulation in `DelaunayBackend`
- Defines crate-owned opaque handles (`DelaunayVertexHandle`, `DelaunayEdgeHandle`, `DelaunayFaceHandle`) so CDT code does not depend on upstream key types
- Translates upstream Delaunay operations and errors into this crate's trait contracts
- Together with `geometry/generators.rs`, this is the only place that directly imports from the `delaunay` crate

### `geometry/generators.rs` — Delaunay triangulation generators

- `generate_delaunay2` — builds a 2D Delaunay triangulation with optional seed
- `build_delaunay2_with_data` — builds from coordinate + vertex-data pairs
- `build_delaunay2_from_simplices` / `build_delaunay2_with_topology` — builds from explicit simplex connectivity (no Delaunay point insertion); the latter
  also accepts `TopologyGuarantee` and `GlobalTopology` metadata for supported explicit topologies
- `build_toroidal_delaunay2` — API-compatibility wrapper for explicit toroidal meshes; with `delaunay` v0.8 it validates the domain and reports the upstream
  explicit-toroidal topology limitation
- `build_periodic_toroidal_delaunay2` — builds true periodic toroidal Delaunay meshes through the upstream image-point constructor
- `random_delaunay2`, `seeded_delaunay2` — convenience wrappers
- `DelaunayTriangulation2D` — type alias for the concrete 2D triangulation type

Together with `backends/delaunay.rs`, this module is the only place that directly imports from the `delaunay` crate.

The toroidal CDT constructor builds from labeled lattice vertices and delegates to the upstream periodic image-point constructor, then validates the resulting
Delaunay triangulation before CDT foliation, causality, topology, and simplex-classification checks run.

### `util.rs` — Numeric helpers

- `saturating_usize_to_i32` — crate-internal usize→i32 conversion for Euler characteristic arithmetic
- Simulation action inputs use native `usize` topology counts; validated CDT count snapshots use `NonZeroUsize`; measurement and trace telemetry downcasts to
  `u32` are checked at construction boundaries
- `y_to_time_bucket` — f64→Option<u32> via round(), for time-slice assignment
- `f64_band_to_u32` — f64→u32 clamped, for y-coordinate binning

## Notebooks

Notebook files live in `notebooks/` and should wrap the CLI or consume generated artifacts rather than reimplementing CDT sampling logic:

- `notebooks/00_quickstart.ipynb` — builds or finds the `cdt` binary, runs a tiny quickstart simulation, reads the generated trace CSV and JSON summary, plots
  acceptance/action/volume diagnostics, and points users to safe first parameter changes.
- `notebooks/01_spacetime_visualization.ipynb` — runs a small 1+1 CDT simulation, reads the exported final triangulation mesh from the JSON summary, and
  generates a static example spacetime visualization under `target/notebooks/visualization/`.
- `notebooks/02_analysis_caches.ipynb` — runs moderate deterministic CDT simulations for debugging, reads stable trace CSV and summary JSON outputs with
  Polars, materializes local Parquet caches, flattens volume profiles into long-form tables with run metadata, and plots exploratory run diagnostics.

## Key Dependencies

- `delaunay` (v0.8) — geometry backend (Delaunay triangulations, vertex data for time labels, checked TDS reconstruction with topology context,
  `set_vertex_data_by_key` for O(1) label mutation)
- `markov-chain-monte-carlo` (v0.4.1) — MCMC framework (`DelayedProposal`, `Chain::step_delayed`, invariant-bearing `Step<Info>` telemetry, `Target`)
- `num-traits` — `ToPrimitive` and `NumCast` for checked or saturating numeric conversions
