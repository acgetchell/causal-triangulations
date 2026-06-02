# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### ⚠️ Breaking Changes

- Implement 2D CDT ergodic moves [#55](https://github.com/acgetchell/causal-triangulations/pull/55)
- Adopt delayed CDT proposals
- Adopt checked Delaunay v0.7.7 APIs
- Resolve periodic toroidal moves with Simplex APIs
- Complete 1+1 toroidal CDT sampling
- Add proposal-site accounting and profiled CDT starts
- Require validated CDT runtime configs [#163](https://github.com/acgetchell/causal-triangulations/pull/163)
- Harden foliation validation and unify backend payload APIs
- Split Metropolis into module tree
- Validate runtime invariants at parse boundaries
- Enforce simulation state invariants [#174](https://github.com/acgetchell/causal-triangulations/pull/174)
- Encode nonzero CDT invariants
- Run continuation through planned proposals [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
- Enforce CDT and backend invariants
- Harden CDT telemetry and profiled initialization
- Reject volume-profile override overflows
- Make Metropolis step telemetry nonzero [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
- Align tooling and CDT error APIs
- Align repository tooling with MCMC

### Merged Pull Requests

- Enforce simulation state invariants [#174](https://github.com/acgetchell/causal-triangulations/pull/174)
- Require validated CDT runtime configs [#163](https://github.com/acgetchell/causal-triangulations/pull/163)
- Tighten coverage and doctest assertion checks [#163](https://github.com/acgetchell/causal-triangulations/pull/163)
- Tighten Semgrep fixture coverage [#162](https://github.com/acgetchell/causal-triangulations/pull/162)
- Harden Semgrep and zizmor checks [#162](https://github.com/acgetchell/causal-triangulations/pull/162)
- Run continuation through planned proposals [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
- Make Metropolis step telemetry nonzero [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
- Add chunked Metropolis checkpoint sweeps [#152](https://github.com/acgetchell/causal-triangulations/pull/152)
- Support fallible public examples [#151](https://github.com/acgetchell/causal-triangulations/pull/151)
- Tighten unwrap Semgrep fixtures [#151](https://github.com/acgetchell/causal-triangulations/pull/151)
- Anchor unwrap fixture exclusions [#151](https://github.com/acgetchell/causal-triangulations/pull/151)
- Replace Node formatters with Rust-native checks [#129](https://github.com/acgetchell/causal-triangulations/pull/129)
- Add toroidal topology infrastructure [#61](https://github.com/acgetchell/causal-triangulations/pull/61)
- Implement foliation for 1+1 CDT [#57](https://github.com/acgetchell/causal-triangulations/pull/57)
- Harden foliation causality checks and backend data mutation [#57](https://github.com/acgetchell/causal-triangulations/pull/57)
- Wire Metropolis loop to real CDT moves [#56](https://github.com/acgetchell/causal-triangulations/pull/56)
- Implement 2D CDT ergodic moves [#55](https://github.com/acgetchell/causal-triangulations/pull/55)
- Migrate DelaunayBackend to upstream key-based handles [#54](https://github.com/acgetchell/causal-triangulations/pull/54)
- Enforce CDT validation guardrails [#8](https://github.com/acgetchell/causal-triangulations/pull/8)

### Added

- Add Crates.io and Docs.rs badges to README [`88e8f1a`](https://github.com/acgetchell/causal-triangulations/commit/88e8f1a64a5bf5fee6663fcf8247ce5d3579f099)

  Include badges for the Crates.io version, download statistics, and
  Docs.rs documentation to improve project visibility and provide quick
  access to package metadata and API docs.
- Migrate DelaunayBackend to upstream key-based handles [#54](https://github.com/acgetchell/causal-triangulations/pull/54)
  [`da3ba1d`](https://github.com/acgetchell/causal-triangulations/commit/da3ba1d48d17491b26c579cadf0818ec745ee3ea)

  - Replace UUID-based handles with upstream VertexKey/EdgeKey/CellKey
  - Own DelaunayTriangulation directly (remove Arc/RefCell/PhantomData)
  - Delete DelaunayEdgeCache and count_edges_in_tds; use upstream
    edges()/incident_edges()/vertex_coords()/cell_vertices()/
    cell_neighbors()/adjacent_cells() iterators

  - Remove stale thread-safety warning; add ThreadSafeBackend impl
  - Replace generic InvalidHandle(String) with typed error variants
    (InvalidVertex, InvalidEdge, InvalidFace) including the failing key

  - Add NotImplemented error variant for mutation stubs
  - Remove unused GeometryError variant and uuid dependency
  - Drop unnecessary [f64; D] serde bounds from all trait impl blocks
  - Simplify CoordinateScalar trait (remove bounds implied by Float)
  - Remove stale "Send + Sync requirements removed" note from
    GeometryBackend doc

  - Update and consolidate tests: 27 backend tests covering all query
    methods, invalid-handle error variants, dimension, Euler
    characteristic, face-neighbor symmetry, and thread safety
- Integrate markov-chain-monte-carlo crate for Metropolis-Hastings
  [`33b6ae5`](https://github.com/acgetchell/causal-triangulations/commit/33b6ae50c8fa93b39833e4cd2b4d2f590b57ab1a)

  - Add markov-chain-monte-carlo v0.1.0 dependency
  - Implement CdtTarget (Target trait) computing log_prob = -S/T from Regge action
  - Add placeholder CdtProposal (ProposalMut trait) returning None until #55
  - Rewrite MetropolisAlgorithm::run() using Chain::step_mut with automatic rollback
  - Add seed: Option&lt;u64&gt; to MetropolisConfig and CdtConfig (--seed CLI arg)
  - Seed both initial triangulation generation and Monte Carlo RNG for full
    reproducibility (closes #59)

  - Add CdtError::Mcmc variant with From&lt;McmcError&gt; conversion
  - run() now returns CdtResult&lt;SimulationResultsBackend&gt;
  - Update integration tests, examples, and benchmarks for new API
  - Add proptest for CdtTarget log_prob consistency across arbitrary couplings
  - Update docs/project.md with markov-chain-monte-carlo dependency
- Implement foliation for 1+1 CDT [#57](https://github.com/acgetchell/causal-triangulations/pull/57)
  [`7e1a751`](https://github.com/acgetchell/causal-triangulations/commit/7e1a7511ff2211b2a1b830f73a771482945c6431)

  - Add `Foliation` struct with `VertexSecondaryMap&lt;u32&gt;` for O(1)
    per-vertex time labels, stored in the CDT layer (not the geometry
    backend)

  - Add `EdgeType` enum (Spacelike / Timelike) and classification via
    `Foliation::classify_edge`

  - Add `from_foliated_cylinder` constructor: grid-based CDT with
    y-coordinate bucket labeling, spatial extent ≤ 1.0, concave √(t+1)
    boundary perturbation to guarantee causality

  - Add `assign_foliation_by_y_coordinate` for existing triangulations
  - Implement `validate_foliation()` (structural: label count, non-empty
    slices, sizes consistency)

  - Implement `validate_causality()` (edge-level: no edge spans &gt;1 time
    slice, uses y-coordinate bucketing for backend-agnostic check)

  - Add `CausalityViolation { time_0, time_1 }` error variant with
    structured fields for programmatic matching

  - Add safe numeric helpers in `util.rs`: `saturating_usize_to_i32`,
    `y_to_time_bucket`, `f64_band_to_u32`

  - Bump `delaunay` dependency to v0.7.3 (VertexSecondaryMap re-export,
    AdaptiveKernel as default builder kernel)

  - 50 new tests (7 foliation unit, 21 integration, 1 proptest, 11 util,
    1 error display, 9 pre-existing updated)
- Store time labels as vertex data, mirroring CDT++ vertex-&gt;info()
  [`1817018`](https://github.com/acgetchell/causal-triangulations/commit/18170189f1899aba1893077f5df8f882a5ad8cdf)

  - Change DelaunayBackend2D vertex data from () to u32 — time-slice
    labels are now stored directly on vertices via Vertex&lt;f64, u32, 2&gt;

  - Embed labels at construction: from_foliated_cylinder uses
    VertexBuilder::data(t) and DelaunayTriangulationBuilder::from_vertices

  - Simplify Foliation to bookkeeping only (slice_sizes + num_slices),
    remove VertexSecondaryMap — labels live on vertices, not a side map

  - Read labels from vertex data: time_label, edge_type, classify_edge,
    validate_causality_delaunay all use vertex_time_label() on the backend

  - Rebuild triangulation in assign_foliation_by_y_coordinate as
    workaround for missing set_vertex_data (blocked on delaunay#284)

  - Tighten re-export guidance in docs/dev/rust.md (stable API only,
    drop incorrect prelude terminology)

  - Update docs/foliation.md and docs/project.md architecture sections
- Add CellType classification for triangulation faces
  [`9b28642`](https://github.com/acgetchell/causal-triangulations/commit/9b28642011d11561c849c99db3998dd269fde1d5)

  - Introduce CellType enum (Up/Down) with i32 encoding in foliation module
  - Add classify_cell function to determine cell type from vertex time labels
  - Implement cell_type, cell_type_from_data, and classify_all_cells on
    CdtTriangulation for bulk cell classification with persistent storage

  - Add cell_key() accessor to DelaunayFaceHandle and cell data read/write
    support in the Delaunay backend

  - Tighten CdtTriangulation generics to require TriangulationQuery trait bound
  - Export CellType and TriangulationQuery from crate root
  - Update foliation and project documentation to reflect new cell data model
  - Bump delaunay dependency to 0.7.4
- Initialize foliated triangulations from labeled Delaunay backends
  [`90feaae`](https://github.com/acgetchell/causal-triangulations/commit/90feaae7e997f515eecc16df75b6f2c66565e474)

  Introduce a constructor to derive foliation structure from existing
  vertex time labels. This update also ensures Metropolis simulations
  capture the initial state at step 0, refines measurement boundary
  validation to prevent overflows, and improves the reliability of edge
  endpoint resolution for hand-built geometries.
- Add toroidal topology infrastructure [#61](https://github.com/acgetchell/causal-triangulations/pull/61)
  [`c0972a2`](https://github.com/acgetchell/causal-triangulations/commit/c0972a2f986153f9c4c9472d90422e70f41d1ede)

  - Add CdtTopology enum (OpenBoundary, Toroidal) with CLI --topology arg,
    CdtConfigOverrides support, and CdtMetadata.topology field

  - Add topology-aware validate_topology(): χ=1/2 for open boundary, χ=0
    for toroidal

  - Add CdtTriangulation::with_topology() constructor
  - Add build_explicit_delaunay2_with_topology() generator wrapping
    DelaunayTriangulationBuilder::from_vertices_and_cells()

  - Wire topology through run_simulation() to dispatch on CdtTopology
  - Add from_toroidal_cdt() placeholder pending delaunay#313
  - Update delaunay to 0.7.5, markov-chain-monte-carlo to 0.2.0
  - Migrate Chain field access to accessor methods (state(), log_prob(),
    into_state())
- Complete toroidal CDT generation with topology-aware validation
  [`8d5c6bf`](https://github.com/acgetchell/causal-triangulations/commit/8d5c6bf3992cfe4135fef6a5e2a78e05a7dde63b)
- [**breaking**] Implement 2D CDT ergodic moves [#55](https://github.com/acgetchell/causal-triangulations/pull/55)
  [`f359c2d`](https://github.com/acgetchell/causal-triangulations/commit/f359c2d017324c731edf0d0007016005fe4fc1ab)

  - Replace placeholder random move outcomes with Delaunay-backed k=2, k=1 insert, and k=1 remove operations.
  - Add causality-aware candidate selection, foliation resynchronization, and structured backend mutation errors.
  - Keep EdgeFlip as a 2D Move22 alias while preserving separate move statistics.
  - Add focused move preludes, doctests, benchmarks, Semgrep checks, and updated move documentation.
  - Mark public error enums non-exhaustive and document orthogonal error guidance.
- Wire Metropolis loop to real CDT moves [#56](https://github.com/acgetchell/causal-triangulations/pull/56)
  [`4d49422`](https://github.com/acgetchell/causal-triangulations/commit/4d49422da209f2982fd9dfb9bf7ca6b67f3dcaec)

  - Replace the zero-move Metropolis guardrail with real ergodic move execution.
  - Compute proposal action deltas before mutating and apply accepted moves with rollback-backed retry handling.
  - Add structured Metropolis move application failures for accepted moves that cannot be applied safely.
  - Record live simplex measurements, move statistics, and final triangulation state from simulation runs.
  - Update examples, benchmarks, docs, and tests for real simulation behavior and focused preludes.
- [**breaking**] Adopt delayed CDT proposals [`77c03d3`](https://github.com/acgetchell/causal-triangulations/commit/77c03d3d0e60706d984285fe5977faffe103de81)

  - Update `markov-chain-monte-carlo` to 0.3.0 and replace the legacy mutation-first CDT proposal adapter with delayed-commit planning.
  - Treat accepted moves that cannot find a realizable local site as ordinary proposal rejections while preserving hard backend failures as typed errors.
  - Add public delayed-proposal telemetry and error types, deterministic Metropolis benchmarking, stricter rustdoc link checks, and tolerant float assertions.
- Implement explicit CDT strip construction [`ed4b7e6`](https://github.com/acgetchell/causal-triangulations/commit/ed4b7e6c0f7a5a17a07706467b683dc362a5f625)

  - Replace the placeholder strip constructor with deterministic layered connectivity, direct time labels, overflow checks, and strict CDT validation.
  - Persist Up/Down cell classifications for explicit strip and toroidal constructors while preserving backend payloads on mutation failures.
  - Update foliation documentation for explicit open-boundary strips and toroidal meshes.
  - Tighten benchmark tooling diagnostics and Python exception boundaries so stdout remains data-only and broad support-script catches stay blocked.
- Feat!(cdt): add dimensional observables [`bf3ffb1`](https://github.com/acgetchell/causal-triangulations/commit/bf3ffb146cf511873c7f2107ebc7113f6e5a0218)
- Add simulation output and resumable checkpoints
  [`99a7ba9`](https://github.com/acgetchell/causal-triangulations/commit/99a7ba93cfff7289ae10f4f7a7addb89a15266db)

  - Add CSV measurement and JSON summary output paths for configured simulation runs.
  - Add serde-backed CDT triangulation and MCMC checkpoints with validated resume state, preserved RNG streams, and typed checkpoint errors.
  - Add an output-and-checkpoint example with semantic example validation through `just examples-validate`.
  - Document output files, resumable checkpoints, example validation, and Rust review guidance updates.
- [**breaking**] Adopt checked Delaunay v0.7.7 APIs
  [`ad0d31a`](https://github.com/acgetchell/causal-triangulations/commit/ad0d31a874efd82e9ea2f34f518ae4c1875af37e)

  - Upgrade to delaunay 0.7.7 and route backend construction through checked, topology-aware validation.
  - Harden CDT initialization, checkpoint restore, and mutation paths so invalid Delaunay state is rejected with typed errors.
  - Replace public simulation result fields with constructors and accessors, and add typed output/checkpoint/backend operation categories.
  - Move mock backend fixtures into the testing prelude and widen Euler characteristic arithmetic to i128.
- Feat!(tooling): add repository hygiene workflows
  [`b775761`](https://github.com/acgetchell/causal-triangulations/commit/b775761965b50dfadecab3bd032c6eb51d3174ad)
- [**breaking**] Resolve periodic toroidal moves with Simplex APIs
  [`82717b9`](https://github.com/acgetchell/causal-triangulations/commit/82717b9665e17da25635751004a128c46141c54d)

  - Update to delaunay 0.7.8 and use the upstream Simplex terminology across CDT geometry wrappers, foliation classification, docs, examples, and tests.
  - Finalize accepted local moves through evolved-CDT validation so invalid toroidal mutations roll back while offset-aware periodic moves can apply.
  - Harden release and benchmark support scripts around missing metadata, Windows path fallbacks, unsized benchmark sorting, and legacy baseline lookup.
- [**breaking**] Complete 1+1 toroidal CDT sampling
  [`c83b665`](https://github.com/acgetchell/causal-triangulations/commit/c83b6655c551589af0d73d1ac53b1365a1942031)

  - Support toroidal volume-changing CDT moves that preserve χ = 0, foliation, and closed spatial slices.
  - Plan concrete Metropolis-Hastings proposals before acceptance and weight forward/reverse local-site counts for detailed balance.
  - Add hard-failure move telemetry and typed validation/application failure details for sampler diagnostics.
  - Document binary usage, unfixed-volume ensemble behavior, large-scale debug workflows, and release-roadmap scope.
- [**breaking**] Add proposal-site accounting and profiled CDT starts
  [`1839f30`](https://github.com/acgetchell/causal-triangulations/commit/1839f30dbbea07813c231015a7ad1681301b2d4e)

  - Add explicit spatial volume profiles for CLI/configuration, initial CDT construction, simulation output, and documentation.
  - Plan Metropolis proposals through sampled local move sites and apply the Hastings correction from forward and reverse site counts.
  - Add proposal-kernel telemetry with saturating counters for no-site outcomes, site rejections, Metropolis rejections, accepted transitions, and hard
    failures.
  - Replace stringly validation and history fields with typed error categories and move enums.
  - Document detailed-balance semantics and add proposal-site benchmark coverage.
- Add chunked Metropolis checkpoint sweeps [#152](https://github.com/acgetchell/causal-triangulations/pull/152)
  [`5065bd2`](https://github.com/acgetchell/causal-triangulations/commit/5065bd253b75812ac691ee7ca1e4bf69e5ce3cbf)

  - Add resumable Metropolis checkpoint continuation so chunked drivers can inspect checkpointed triangulations between sweeps.
  - Route the large-scale 1+1 debug harness through checkpoint-preserving Metropolis chunks sized from the current simplex count.
  - Document the MCMC backend boundary and roadmap blockers for upstream markov-chain-monte-carlo continuation work.
- Support fallible public examples [#151](https://github.com/acgetchell/causal-triangulations/pull/151)
  [`9c10f0b`](https://github.com/acgetchell/causal-triangulations/commit/9c10f0b5daa1e6deea9c04c1f1af8474e0d26f28)

  - Convert public Rust doctests to CdtResult-based flows with typed error propagation.
  - Add Semgrep guardrails for unwrap/expect in doctests, benches, and examples.
  - Preserve standalone CDT proposal application failures as typed CdtError variants.
  - Re-export MCMC simulation traits from the simulation prelude for proposal workflows.
  - Replace benchmark fixture expect paths with operation-aware setup helpers.
- [**breaking**] Require validated CDT runtime configs [#163](https://github.com/acgetchell/causal-triangulations/pull/163)
  [`fbdcc41`](https://github.com/acgetchell/causal-triangulations/commit/fbdcc417583966faf6e7c68fb253b07197f3335f)

  - Add `ValidatedCdtConfig` and `ValidatedInitialVolume` so runtime construction consumes topology, volume, schedule, dimensionality, and coupling invariants
    after validation.
  - Move Metropolis/action runtime conversion behind validated configs and route `run_simulation` and examples through the proof-bearing API.
  - Report invalid Metropolis schedules and temperatures with `InvalidSimulationConfiguration` while keeping geometry, topology, and action failures on
    `InvalidConfiguration` .
  - Bump the Rust baseline to 1.96.0, update the MCMC backend to 0.4.0, and align docs.rs metadata, API docs, and test assertions with the new baseline.

### Changed

- Consolidate and refine crate keywords [`41a8212`](https://github.com/acgetchell/causal-triangulations/commit/41a8212fc18b13a2853d4ea02d2e5462d7a87d96)

  Combine "quantum" and "gravity" into "quantum-gravity" and remove the redundant "physics" tag to improve package discoverability and metadata accuracy.

- Use TDS key checks for handle validation and log stubs
  [`aa0ca36`](https://github.com/acgetchell/causal-triangulations/commit/aa0ca365a213b5373c78d110206092bf09b00192)

  Update handle validation in the Delaunay backend to check the topology
  data structure directly for vertex and cell keys. Remove the unused
  OperationFailed error variant and add warning logs to the unimplemented
  clear and reserve_capacity methods.

- Document mutation support and update changelog for backend keys
  [`974ba38`](https://github.com/acgetchell/causal-triangulations/commit/974ba3872b2bcb25e32f9be495f4e56a99cdeadf)

  Update the Delaunay backend documentation to clarify that mutation
  methods are currently unimplemented and return errors. Synchronize the
  changelog to reflect the migration from UUID-based handles to upstream
  key-based handles.

- Make performance regression check non-blocking in CI
  [`75eb4d2`](https://github.com/acgetchell/causal-triangulations/commit/75eb4d266f2266021b8544741661f6d181261d2f)

  Update the performance workflow to warn rather than fail when
  regressions are detected. Shared GitHub Actions runners often produce
  noisy benchmark results with significant variance, leading to false
  positives. This change ensures that statistical noise does not block
  pull request merges while maintaining visibility via PR comments.

- Improve foliation API correctness, error handling, and test coverage
  [`28c28da`](https://github.com/acgetchell/causal-triangulations/commit/28c28dabbaa41d3da8834625a99728e5fe5bf2b7)

  - Replace .unwrap() with Result propagation in from_foliated_cylinder and
    generate_delaunay2_with_context for VertexBuilder::build() errors

  - Add CdtError::VertexBuildFailed variant for vertex construction failures
    instead of misusing DelaunayGenerationFailed

  - Make validate_topology, validate_causality, validate_foliation public with
    doctests so crate users can run targeted validation checks

  - Simplify DelaunayBackend trait bounds: import DataType directly, remove
    redundant where clauses leveraging edition 2024 struct-level bounds

  - Add debug_assert in Foliation::new for out-of-range time labels
  - Fix docs/foliation.md heading levels (h4→h3) and add code block language
  - Add integration property tests (tests/proptest_foliation.rs) for cylinder
    invariants, edge classification completeness, and seed determinism

  - Add unit tests for acausal edge classification, new vertex labeling,
    single-slice foliation, and larger grid scaling

  - Update docs/dev/rust.md with doctest and re-export guidance

- Improve foliation robustness and add acausal edge classification
  [`1385793`](https://github.com/acgetchell/causal-triangulations/commit/1385793a88b72493b344f450d2ab6cc511644905)

  Explicitly classify edges spanning multiple time slices as acausal
  instead of treating them as invalid timelike edges. Replace debug
  assertions in Foliation construction with a formal Result-based API and
  improve error propagation during foliation assignment and causality
  validation in CdtTriangulation.

- Isolate delaunay crate behind geometry boundary, add prelude
  [`1c152b3`](https://github.com/acgetchell/causal-triangulations/commit/1c152b38178de0e8b09cbfda7e55493fb8b9d06e)

  - Move Delaunay generators from util.rs to new geometry/generators.rs
    module, establishing src/geometry/ as the sole delaunay crate boundary

  - Strip util.rs to pure numeric helpers (no delaunay imports)
  - Remove raw delaunay crate imports from cdt/triangulation.rs; use
    DelaunayBackend2D type alias and backend trait methods instead

  - Add prelude module with prelude::*, prelude::triangulation::*, and
    prelude::simulation::* sub-preludes; update all doctests, examples,
    integration tests, proptests, and benchmarks to use them

  - Shorten generator function names (generate_delaunay2_with_context →
    delaunay2_with_context, etc.) and assign_foliation_by_y_coordinate →
    assign_foliation_by_y

  - Expand FoliationError with LabelCountMismatch, EmptySlice, and
    SliceSizeSumMismatch variants; add From&lt;FoliationError&gt; for CdtError

  - Fix dropped backend error in assign_foliation_by_y (was map_err(|_|))
  - Add 9 new tests: causality violation detection, FoliationError Display
    and conversion, build_delaunay2_with_data edge cases

  - Document Geometry Backend Isolation rule in docs/dev/rust.md, AGENTS.md,
    and project.md

- Improve foliation consistency and specialize validation API
  [`c947e12`](https://github.com/acgetchell/causal-triangulations/commit/c947e12a333d26be5c1fa2661280c7257ac84832)

  Clear stale face classifications when updating time labels or
  re-classifying cells to ensure data consistency. Specialize CDT
  validation methods for the 2D Delaunay backend to facilitate direct
  vertex data access and restrict test-only triangulation generators to
  internal crate usage.

- Update triangulation metadata when assigning foliation
  [`fb12663`](https://github.com/acgetchell/causal-triangulations/commit/fb126634bbd2915a49ba501517039a93239b8168)

  Update the modification count and timestamp within assign_foliation_by_y
  to ensure the triangulation metadata accurately reflects the state
  change. Additionally, remove a redundant validation check for slice size
  sums that is already handled during foliation construction.

- Validate vertex time labels and improve causality test stability
  [`47f474a`](https://github.com/acgetchell/causal-triangulations/commit/47f474a2df1f2fa961d1ca735bd2a8ffd5c36922)

  Add explicit bounds checking for vertex time labels during foliation
  construction to ensure data consistency, returning an error if labels
  exceed the slice count. Refactor the causality violation test to use
  deterministic triangulation data for more reliable validation.

- Enhance causality validation in tests by using a foliated cylinder setup
  [`e61932f`](https://github.com/acgetchell/causal-triangulations/commit/e61932f4ffbc8fdf27d43e4b0bc84fb2d8f085ff)

- Reformat vertex time label access in triangulation tests
  [`37ed0e9`](https://github.com/acgetchell/causal-triangulations/commit/37ed0e9903907e2605eb3d2f8b6fd6af7abd3d48)

  Internal formatting update to improve code readability within the test
  suite.

- Invalidate cache when assigning foliation by y-coordinate
  [`090b5be`](https://github.com/acgetchell/causal-triangulations/commit/090b5be58e34b38f8e6de15b995d999f849aa4a6)

  Ensure cached properties are cleared when updating vertex time labels
  to prevent stale data from persisting after a foliation change.

- Refactor triangulation tests for deterministic causality validation
  [`20ff771`](https://github.com/acgetchell/causal-triangulations/commit/20ff7715d64c27eb2a7c8a4988097a8dc0c4d4c5)

  Refactor foliation and causality tests to use hand-built geometry and
  deterministic triangulation generators. This avoids issues with Delaunay
  tie-breaking in larger meshes, ensuring that causality violation
  detection is tested against a stable and predictable state.

- Implement structured error handling for configuration and generation
  [`1df0a5d`](https://github.com/acgetchell/causal-triangulations/commit/1df0a5d66faaa38676a16ad95142e2243934f162)

  Replace generic string-based error messages with structured CdtError
  variants to provide better diagnostic context for configuration and
  triangulation failures. This update centralizes simulation parameter
  validation and introduces stricter checks for measurement schedules to
  ensure at least one post-thermalization data point is recorded.

- [**breaking**] Harden foliation validation and unify backend payload APIs
  [`934f38c`](https://github.com/acgetchell/causal-triangulations/commit/934f38cce8064e61f7ed9e490f05bb2a35577cdc)

  - add live-label consistency checks with explicit missing, out-of-range, and slice-mismatch diagnostics
  - replace legacy payload mutation/read paths with key-based backend helpers across triangulation logic
  - centralize metadata mutation and cache invalidation bookkeeping behind dedicated update helpers
  - make the provisional point-set strip constructor internal and align strip property tests with the explicit-strip placeholder path

- Track foliation synchronization and harden time-slice assignment
  [`e1c3b8a`](https://github.com/acgetchell/causal-triangulations/commit/e1c3b8aa6b7afb10b1f177b7a4a6e2977b44092e)

  Introduce a synchronization counter to ensure foliation metadata remains
  synchronized with geometry changes. This update also enforces non-empty
  time slices and ensures that foliation assignment is atomic, preventing
  partial backend mutations if validation fails.

- Defer geometry cache invalidation until backend mutation occurs
  [`d912f8a`](https://github.com/acgetchell/causal-triangulations/commit/d912f8aa60de1dfd5c843ada0b2bde940e69e912)

  Modify the geometry mutation wrapper to distinguish between metadata
  updates (like simulation event recording) and actual backend modifications.
  This ensures that foliation metadata and geometry caches are only
  invalidated when the underlying triangulation is accessed for mutation.

  Additionally, specialize foliation validation for the Delaunay backend to
  eliminate dynamic downcasting and ensure cell type lookups correctly
  identify and handle stale states following geometry changes.

- Simplify geometry abstraction and remove redundant mesh module
  [`44bc7b8`](https://github.com/acgetchell/causal-triangulations/commit/44bc7b8ac6ea553312410e941c455e1962d37f94)

  Streamline the geometry interface by removing the unused `mesh` module and
  redundant traits (`ThreadSafeBackend`, `TriangulationFactory`). This
  refactor favors direct triangulation queries over intermediate mesh
  structures. Additionally, the update improves test determinism by using
  seeded point generation and adds verification for vertex-time labels and
  foliation re-assignment consistency.

- Import `rand::RngExt` trait for random number utilities
  [`ea094f2`](https://github.com/acgetchell/causal-triangulations/commit/ea094f239f7e4cbb06c0bf3777eea465ad9dfd28)

- Refactor!(cdt): remove mutable triangulation escape hatches
  [`a289361`](https://github.com/acgetchell/causal-triangulations/commit/a289361abff0b4c70606c207f07542279e614c60)

- Refactor!(cdt): split triangulation modules and harden invariants
  [`aa08007`](https://github.com/acgetchell/causal-triangulations/commit/aa08007cdb662d9e8863c4954a9294501f7dee0b)

- Refine Semgrep unwrap fixture exclusion patterns
  [`0f4fb0f`](https://github.com/acgetchell/causal-triangulations/commit/0f4fb0fc89bd62c0786b25482f6b58dd8e0e4fc2)

  Replace wildcard function arguments with explicit signatures for internal
  Semgrep unwrap/expect rule exclusions. This ensures more precise pattern
  matching and prevents unintended false negatives for doctests and internal
  utility functions.
  Refs: #151

- Update tooling documentation on Semgrep unwrap/expect rules
  [`8ac8398`](https://github.com/acgetchell/causal-triangulations/commit/8ac839845bbaeb8deaa35c59d5ef9530147a5902)

  Document refinements to Semgrep rules for `unwrap()` and `expect()`.
  This clarifies how rules distinguish code from prose mentions and
  anchor fixture exclusions, preventing false positives in doctests
  and ensuring accurate public-surface checks.

- [**breaking**] Split Metropolis into module tree
  [`3d602c5`](https://github.com/acgetchell/causal-triangulations/commit/3d602c5139fbfae35d7eec1ceb3d7f07697ae314)

  - Move CDT Metropolis adapter, runner, checkpoint, telemetry, and shared helper logic into an explicit `src/cdt/metropolis/` module tree.
  - Keep `adapter.rs` as the single boundary to `markov-chain-monte-carlo` while preserving existing public simulation re-exports.
  - Calibrate the default 1+1 CDT action constants to `kappa_0 = 0`, `kappa_2 = 0`, and `lambda_edge = (2 / 3) ln 2`.
  - Document the Metropolis module layout, Delaunay initialization role, and 1+1 CDT coupling calibration.

- [**breaking**] Validate runtime invariants at parse boundaries
  [`8a03da7`](https://github.com/acgetchell/causal-triangulations/commit/8a03da753747d6374ddf1da94f30ccb5b0194de3)

  - Require action, Metropolis, and top-level simulation configuration to enter runtime code through validated private-field types.
  - Validate result, checkpoint, move, and proposal telemetry before storage or deserialization so impossible counters and incoherent step records cannot be
    represented.
  - Move CDT triangulation state into the triangulation module tree and expose metadata, statistics, and constants through focused accessors and preludes.

- [**breaking**] Enforce simulation state invariants [#174](https://github.com/acgetchell/causal-triangulations/pull/174)
  [`583e5d4`](https://github.com/acgetchell/causal-triangulations/commit/583e5d485f35d9124cb85afd1bca4754d0537497)

  - Reject checkpoints and completed results whose proposal telemetry does not match recorded sampler steps, accepted transitions, or rejected transitions
  - Record Metropolis measurements only on the configured post-thermalization cadence and validate deserialized measurement streams against the same schedule
  - Resolve configured output paths before triangulation or sampling begins, and prevent CDT modification counters from wrapping into stale cache versions

- [**breaking**] Encode nonzero CDT invariants [`938d712`](https://github.com/acgetchell/causal-triangulations/commit/938d712175c35fe85aa893042f159afe0593f6ce)

  - Store Metropolis step counts, measurement cadence, foliation slice counts, and CDT time-slice metadata as NonZeroU32.
  - Preserve validated Metropolis configuration inside ValidatedCdtConfig instead of rebuilding from raw fields.
  - Move time-slice parsing into construction and deserialization boundaries so downstream accessors can stay infallible.
  - Widen measurement-count arithmetic before including the current step to avoid overflow.

- [**breaking**] Run continuation through planned proposals [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
  [`262518f`](https://github.com/acgetchell/causal-triangulations/commit/262518fee46949251b4f77003270e6eff990a4df)

  - Drive chunked Metropolis runs and checkpoint resume through the upstream sampler planned-step handoff while preserving CDT measurements, telemetry, and move
    history.
  - Carry validated config, volume, and foliation counts as NonZeroU32 so downstream construction can rely on nonzero invariants.
  - Add Semgrep guardrails around planned-step telemetry and sampler-state synchronization.

### Dependencies

- Bump actions/cache from 5.0.1 to 5.0.4 [`063c3fc`](https://github.com/acgetchell/causal-triangulations/commit/063c3fc7601935b9399be2105e5359e1a39fade9)
- Bump the dependencies group with 2 updates [`e19510f`](https://github.com/acgetchell/causal-triangulations/commit/e19510f251915ecb2eb02d44619cd6f0c56853bc)
- Bump taiki-e/install-action from 2.68.35 to 2.69.8
  [`3cc7c55`](https://github.com/acgetchell/causal-triangulations/commit/3cc7c556f6aaac7f959dd9df2e6130a118b49387)
- Bump codecov/codecov-action from 5.5.2 to 5.5.3
  [`69e2fca`](https://github.com/acgetchell/causal-triangulations/commit/69e2fca31bc9b6298a8dad8b9f48c84eba1c89ff)
- Bump codecov/codecov-action from 5.5.3 to 6.0.0
  [`57464a4`](https://github.com/acgetchell/causal-triangulations/commit/57464a4352252aea462850b5eab94ee7e9820386)
- Bump astral-sh/setup-uv from 7.6.0 to 8.0.0 [`c7e4472`](https://github.com/acgetchell/causal-triangulations/commit/c7e4472c72f73c7717b7639aae6f820a0c5cc266)
- Bump pygments from 2.19.2 to 2.20.0 [`5cc07e9`](https://github.com/acgetchell/causal-triangulations/commit/5cc07e9306ba51f990c01e4e5db4f5192b0379af)
- Bump taiki-e/install-action from 2.69.8 to 2.70.3
  [`387e96d`](https://github.com/acgetchell/causal-triangulations/commit/387e96dcbba58cac8bfdaaefcbb9d8367c965a90)
- Bump taiki-e/install-action from 2.70.3 to 2.75.0
  [`e3f4018`](https://github.com/acgetchell/causal-triangulations/commit/e3f40183c6778f252d46523234fcc1da58d932c4)
- Bump the dependencies group across 1 directory with 2 updates
  [`d843b90`](https://github.com/acgetchell/causal-triangulations/commit/d843b900b0940d4f79d101279164ab42d2549463)
- Bump astral-sh/setup-uv from 8.0.0 to 8.1.0 [`ba66738`](https://github.com/acgetchell/causal-triangulations/commit/ba66738ba2537cc3b1bb10bb9a0e93b4174d023b)
- Bump actions-rust-lang/setup-rust-toolchain [`e618486`](https://github.com/acgetchell/causal-triangulations/commit/e6184861c9b56396961a53846e5a26ac2f54f995)
- Bump taiki-e/install-action from 2.75.0 to 2.75.24
  [`b7e2413`](https://github.com/acgetchell/causal-triangulations/commit/b7e241317a2e9c1d0741bf80f3285503924daf28)
- Bump actions/github-script from 8.0.0 to 9.0.0
  [`2923849`](https://github.com/acgetchell/causal-triangulations/commit/292384985e8d336807b1c65a8a5a7d72306f3810)
- Bump actions/upload-artifact from 7.0.0 to 7.0.1
  [`c0ec7f6`](https://github.com/acgetchell/causal-triangulations/commit/c0ec7f66bebffed177731f16a1362980a98e15f0)
- Bump actions/cache from 5.0.4 to 5.0.5 [`74ff03a`](https://github.com/acgetchell/causal-triangulations/commit/74ff03a02a5d8d10a68846c92d1a2936512b8796)
- Bump taiki-e/install-action from 2.75.24 to 2.77.1
  [`8c5b755`](https://github.com/acgetchell/causal-triangulations/commit/8c5b7556dc41a49c5278abfb6fc5060d8da5a5b2)
- Bump taiki-e/install-action from 2.77.1 to 2.77.6
  [`718681b`](https://github.com/acgetchell/causal-triangulations/commit/718681b74f8da79895e8bad94cb0f5cad7707d50)
- Bump actions-rust-lang/setup-rust-toolchain [`effbbd4`](https://github.com/acgetchell/causal-triangulations/commit/effbbd42c174396063c3ec5f72a8565d22f7cf62)
- Bump assert_cmd in the dependencies group [`035bfcc`](https://github.com/acgetchell/causal-triangulations/commit/035bfcc63d3375ee09ed33185ec285e7f0f0dae5)
- Bump urllib3 in the uv group across 1 directory
  [`6d436aa`](https://github.com/acgetchell/causal-triangulations/commit/6d436aa50d86d7fe74caa0898765204aea45570f)
- Bump idna in the uv group across 1 directory [`df3480f`](https://github.com/acgetchell/causal-triangulations/commit/df3480f961e06c566dade7a3850e88b8d26c73bd)
- Bump taiki-e/install-action from 2.77.6 to 2.79.2
  [`6f611ff`](https://github.com/acgetchell/causal-triangulations/commit/6f611ff1fd8fcc00ffe9a8bf1835da777893aa28)
- Bump codecov/codecov-action from 6.0.0 to 6.0.1
  [`3847062`](https://github.com/acgetchell/causal-triangulations/commit/38470624f992c3fca087228d663fd6dcf8af864d)
- Bump serde_json [`1780261`](https://github.com/acgetchell/causal-triangulations/commit/1780261e6f13fc4f222f02ae220145cb0ca8c6a4)
- Bump log from 0.4.29 to 0.4.30 in the dependencies group
  [`ac3237f`](https://github.com/acgetchell/causal-triangulations/commit/ac3237fff81a4d1c0233cfc9125a87d7cbca56ed)
- Bump taiki-e/install-action from 2.79.2 to 2.79.9
  [`788eea8`](https://github.com/acgetchell/causal-triangulations/commit/788eea8b1cc1b9b91773c53d5c6a1cc172d1c369)

### Documentation

- Clarify CDT-owned mutation boundaries [`60730b6`](https://github.com/acgetchell/causal-triangulations/commit/60730b6a3e6d3a1b88bb08fdd309fd0ac5c2c118)

  - Describe foliation label writes through CDT-owned helper paths instead of backend mutation APIs.
  - Document rollback boundaries for mutating ergodic move helpers.
  - Cover legacy `std::f64` non-finite defaults in the Semgrep fixture.
- Clarify geometry backend move responsibilities
  [`6a0eb2b`](https://github.com/acgetchell/causal-triangulations/commit/6a0eb2b53494bc4bae594449f2f131e5a7e1930b)

  - Document that src/geometry/ wraps upstream Delaunay operations through crate-owned traits.
  - Spell out its conversion, validation, and error translation role between Delaunay and CDT geometry types.
- Clarify Metropolis proposal cache identity [`b3bfa4b`](https://github.com/acgetchell/causal-triangulations/commit/b3bfa4b58d8be05423dc305f00e08a377edc0698)

  - Document that proposal-site cache validity uses `(instance_id, modification_count)`.
  - Explain the reverse proposal-site denominator stored on CDT proposal plans.
- Wrap volume-fixing citation prose [`7cf8ffd`](https://github.com/acgetchell/causal-triangulations/commit/7cf8ffd6bdf19ce885359d5ce9df5c3baea9fc33)

  - Keep the Metropolis documentation within the raw Markdown line-length limit.
- Clarify saturated proposal telemetry merging [`3e218da`](https://github.com/acgetchell/causal-triangulations/commit/3e218da37312875587ca2970885324c5c4797bc0)

  - Document that ProposalStatistics::extend uses saturating counter addition.
  - Explain that saturated telemetry remains serializable while exact terminal-outcome partitions may be coarsened.

### Fixed

- Validate config in run(), record seed for provenance
  [`d9e713d`](https://github.com/acgetchell/causal-triangulations/commit/d9e713d851303ea9a04695bf3e5f7cbda520beb0)

  - Validate measurement_frequency &gt; 0 and temperature finite/positive
    at start of MetropolisAlgorithm::run() to prevent panics and NaN

  - Generate and record a random seed when none is provided, ensuring
    every simulation run is reproducible after the fact

  - Add #![forbid(unsafe_code)] to proptest integration test crate
  - Add tests for config validation and seed provenance recording
- Harden foliation causality checks and backend data mutation [#57](https://github.com/acgetchell/causal-triangulations/pull/57)
  [`58d4483`](https://github.com/acgetchell/causal-triangulations/commit/58d448323e89005614afe532ae5c410b902aac58)

  - make foliated cylinder generation fail fast when point-set Delaunay output is not a valid 1+1 CDT strip
  - add explicit backend mutation APIs for vertex and cell payloads, with structured backend-mutation error propagation
  - tighten causality validation to enforce the CDT per-triangle edge mix (one spacelike and two timelike)
  - update edge endpoint resolution to optional lookup semantics and optimize membership checks via local adjacency
  - add geometry-focused prelude exports and refresh doctests/examples for seeded triangulation plus foliation assignment workflows
  - convert foliation property coverage to known-limitation guards while explicit CDT strip construction is pending
- Report actual coordinate range in triangulation failure diagnostics
  [`6d23d2e`](https://github.com/acgetchell/causal-triangulations/commit/6d23d2edbd5b8a01d9cab71ef4aea29b3f64e632)

  Replace the hardcoded (0.0, 0.0) coordinate range in the Delaunay
  generation error with the computed bounds of the input vertices. This
  improves diagnostic visibility for failed triangulation attempts by
  providing the actual spatial extent of the processed data.
- Harden toroidal CDT validation and run_simulation semantics
  [`9240252`](https://github.com/acgetchell/causal-triangulations/commit/924025206f57276cb6879850a70a6a7181fe4854)

  - Tighten time_step_distance: out-of-range labels (t &gt;= T) bypass
    toroidal wrap-around so causality violations are reported with the
    raw step distance rather than being silently folded to 0 via min(T, 0).

  - Add FoliationError::MissingTemporalWrapAround variant (with Display
    and test) and validate_toroidal_temporal_wraparound() to distinguish
    a torus from a cylinder. chi = 0 alone is ambiguous; we now also
    require every slice to have timelike adjacency to (t-1) mod T and
    (t+1) mod T. Wired into validate_foliation alongside the existing
    spatial-S^1 check.

  - Refresh CdtTopology doc comments: remove stale 'blocked on delaunay#313'
    notes from OpenBoundary and Toroidal; document Toroidal as supported
    via CdtTriangulation::from_toroidal_cdt.

  - CdtConfig::validate: for Toroidal, require timeslices &gt;= 3, vertices
    divisible by timeslices, and vertices &gt;= 3 * timeslices. Tests cover
    each branch and confirm OpenBoundary is unaffected.

  - run_simulation: treat config.vertices as the TOTAL vertex count for
    both topologies. Toroidal branch now divides by timeslices to recover
    vertices_per_slice. Lib test asserts vertices=12, timeslices=3 yields
    a 12-vertex (not 36-vertex) toroidal triangulation.
- Harden CDT invariants and repository rules [`800f2fb`](https://github.com/acgetchell/causal-triangulations/commit/800f2fb99f3be4aaf834b6334a620d677694dd83)

  - Centralize toroidal time-slice validation through the metadata write path so foliation assignment cannot bypass topology invariants.
  - Make mock geometry mutations either update stored topology or return specific, debuggable errors.
  - Add focused coverage for Codecov gaps, move-statistics accounting, mock backend mutation paths, and toroidal foliation validation.
  - Expand public doctests and private-helper documentation guidance across CDT and geometry modules.
  - Tighten Semgrep/SARIF workflow handling, coverage-report docs, changelog line-length checks, and unsafe-code enforcement.
- Address review findings for changelog and geometry
  [`216252e`](https://github.com/acgetchell/causal-triangulations/commit/216252eb699642ce2c661045be148cdbef3af64d)

  - Harden changelog postprocessing, archive sorting, release staging, and Semgrep coverage for script tests.
  - Centralize toroidal time-slice validation and add unsafe-code guards where missing.
  - Improve mock backend mutation behavior, unordered facet equality, and saturating Euler arithmetic.
  - Strengthen seeded Delaunay determinism tests and re-export topology metadata through the crate API.
  - Remove panic-prone acceptance-rate conversions and broaden targeted regression coverage.
- Tighten geometry topology and tooling diagnostics
  [`9585d93`](https://github.com/acgetchell/causal-triangulations/commit/9585d937e566cca6ca961353b33accb516dc2d01)

  - Add unsafe-code guards to geometry backend and trait modules.
  - Preserve relative parent path prefixes during config path normalization.
  - Return concrete mock adjacency, incidence, and face-neighbor topology.
  - Re-export ToroidalConstructionMode through the geometry public facade.
  - Expand Semgrep direct-delaunay import detection for multiline grouped imports.
  - Include XML parse diagnostics in coverage report errors.
  - Include star bullets when injecting changelog summary sections.
  - Add regression coverage for path handling, mock topology, Semgrep fixtures, and Python tooling.
- Enforce CDT validation guardrails [#8](https://github.com/acgetchell/causal-triangulations/pull/8)
  [`15234c3`](https://github.com/acgetchell/causal-triangulations/commit/15234c3b4d4ca10be1b27753ee31338eb55af2a6)

  - Reject zero-move CDT simulations and placeholder ergodic moves with typed unsupported-operation errors.
  - Validate CDT metadata, topology, action couplings, and foliation mutation paths before committing state changes.
  - Add typed errors, property tests, doctests, and Semgrep coverage for validation and diagnostics.
  - Update examples, benchmarks, CLI docs, and preludes for the guarded simulation behavior.
- Enforce CDT validation guardrails [#8](https://github.com/acgetchell/causal-triangulations/pull/8)
  [`77650dc`](https://github.com/acgetchell/causal-triangulations/commit/77650dc2c3f9dd1e4e584ab3403f65e5efef5bfd)

  - Validate action couplings before MetropolisAlgorithm::run reaches the pending-moves guardrail.
  - Preserve Metropolis scaffolding while documenting and benchmarking the current unsupported-operation path.
  - Return typed toroidal time-slice metadata errors before staged foliation construction.
  - Add focused Metropolis coverage for aggregation helpers and the placeholder proposal contract.
  - Update CLI, example, benchmark, CodeRabbit, and Semgrep docs/config to match current behavior.
- Harden ergodic move validation [`657e0e6`](https://github.com/acgetchell/causal-triangulations/commit/657e0e6fd7f4b3619f98485f4c538c3b0bc0719e)

  - Add edge-adjacent face queries so causal edge flips avoid full face scans and hot-path candidate allocations.
  - Report post-mutation foliation sync failures as hard failures instead of ordinary rejections.
  - Keep foliation state unsynchronized when cell classification fails.
  - Update CDT benchmarks to exercise a real degree-3 vertex removal.
  - Strengthen Semgrep coverage for nested public error enums and tighten positive-path move tests.
- Compute toroidal face centroids correctly across periodic boundaries
  [`faf5b0e`](https://github.com/acgetchell/causal-triangulations/commit/faf5b0ef0f2617bbde8bb0282aa0fd909cd05e4c)

  Adds specialized centroid calculation for toroidal triangulations,
  unwrapping coordinates to ensure geometric accuracy for faces spanning
  domain boundaries. Also exposes the periodic domain from the geometry
  backend.

  Refines error reporting by introducing `HardFailure(CdtError)` for
  irreversible post-mutation failures, distinct from reversible rejections.

  Introduces and maintains an internal `interior_facets_by_edge` cache for
  Delaunay 2D backends to efficiently query adjacent facets for edges,
  improving performance and correctness of topological mutations.
  Refs: #55
- Validate stored foliation after backend mutation
  [`7040ff9`](https://github.com/acgetchell/causal-triangulations/commit/7040ff914c11d9e9d20be7ba904e79136bd5dba0)

  - Keep live foliation queries and cell classification active when mutable backend access invalidates sync bookkeeping without removing the foliation.
  - Clarify periodic and open-boundary CDT foliation docs, including `from_toroidal_cdt()` and `from_cdt_strip()` behavior.
- Harden from_cdt_strip generation [`cdbf363`](https://github.com/acgetchell/causal-triangulations/commit/cdbf3632412bf3e3939be426a10eeb6640b6f248)

  - Reserve explicit strip construction buffers fallibly and report CDT generation errors with strip context when allocation fails.
  - Keep strip triangles in fixed-size cells until the backend boundary and reject generated backends whose vertex or face counts differ from the requested
    strip.
- Update simulation history modification timestamps
  [`76cd209`](https://github.com/acgetchell/causal-triangulations/commit/76cd209888d19df2c983f5b5d9e05b7a8d94514e)

  - Refresh `last_modified` when recording simulation events so metadata reflects history-only CDT updates.
  - Warm triangulation caches explicitly in cache invalidation coverage.
  - Measure metadata cache invalidation without including triangulation construction cost.
- Preserve toroidal invariants during moves [`e4a398b`](https://github.com/acgetchell/causal-triangulations/commit/e4a398b9b3a261c8ddd3b9ef5eb4eff1183c1741)

  - Reject toroidal candidate edits that would break chi = 0 or the closed-S1
    foliation contract before recording move success.

  - Document the toroidal Metropolis invariant contract and refresh stale testing,
    move, foliation, and crate-level documentation.

  - Replace the retired TODO document with a high-level roadmap for release
    direction and follow-up work.
- [**breaking**] Enforce CDT and backend invariants
  [`3225000`](https://github.com/acgetchell/causal-triangulations/commit/32250003139e8bcbc95e3d88ad86fff58e1e9e7c)

  - Build open-boundary simulations from foliated CDT strips and validate regular per-slice configuration before simulation startup.
  - Validate topology in CdtTriangulation::with_topology before publishing wrapped backends.
  - Make TriangulationMut::clear and reserve_capacity fallible so unsupported or failed backend operations cannot silently succeed.
  - Add typed coordinate and reservation errors, and update docs and examples toward foliated CDT constructors.
- Harden output and checkpoint serialization [`45bc9e0`](https://github.com/acgetchell/causal-triangulations/commit/45bc9e0f8f1d2bfd7d3310da97cf655da64d55b5)

  - Reject invalid checkpoint state at serde boundaries, including empty foliation bookkeeping and invalid toroidal periods.
  - Prevent CSV and JSON outputs from resolving to the same file before writing either artifact.
  - Align serialized topology names with CLI vocabulary and report equilibrium-only average action in JSON summaries.
  - Avoid preallocating simulation step history from the full configured step count.
- Fix!(cdt): enforce Delaunay initialization invariants
  [`55cdbd4`](https://github.com/acgetchell/causal-triangulations/commit/55cdbd4e1d8a01823a6677f08f01580228db785b)
- Fix!(geometry): reject structurally invalid Delaunay edits
  [`3f5a857`](https://github.com/acgetchell/causal-triangulations/commit/3f5a8579bbc0ac0a31045d9883da313ade470765)
- Normalize CDT validation check tokens [`f7c3605`](https://github.com/acgetchell/causal-triangulations/commit/f7c3605934f76ef37907e3b626a6b6a0f9bcfb1c)

  - Emit `ergodic_move_candidate_geometry` consistently with the other
    `CdtValidationCheck` display identifiers.

  - Keep validation regression coverage deterministic for structural Delaunay
    rollback and checkpointed Metropolis runs.
- Harden move failures and result deserialization
  [`5685f1b`](https://github.com/acgetchell/causal-triangulations/commit/5685f1b0dca0deffbe1fcbaa1c17d339b9da656a)

  - Propagate post-mutation CDT invariant failures as hard move errors instead of candidate rejections.
  - Reject toroidal `(1,3)` subdivisions before mutation while preserving inverse `(3,1)` coverage.
  - Validate simulation result deserialization through the checked constructor so invalid configs or final triangulations fail to load.
  - Report inserted-vertex label failures with the matching backend mutation operation and document `adjacent_faces` query cost.
- Harden benchmark and changelog tooling [`9100099`](https://github.com/acgetchell/causal-triangulations/commit/910009968bd3c82e7e3f7be6d6a0a19f5ed799d4)

  - Prefer Criterion new estimates over stale base entries during fallback discovery so each benchmark id appears once.
  - Restrict changelog release-heading preservation to SemVer headings so bracketed commit-body headings are demoted.
  - Normalize non-Rust tooling to 160 columns with Markdown reflow and a raw line-length guard.
  - Document GitHub releases with the tag string as the release title.
- Harden toroidal proposal accounting [`9c6acb9`](https://github.com/acgetchell/causal-triangulations/commit/9c6acb9c806dabe40d2e9c2998d64f8512a58c76)

  - Count only move sites that can commit through the deterministic backend mutation path, and carry the forward site count into Metropolis-Hastings
    proposal-ratio scoring.
  - Reject resumable checkpoints with nonzero hard-failure counters and preserve structured Metropolis move failure sources across error conversions.
  - Document clone-plan-commit move ordering, unfixed-volume sampling, proposal-ratio correction, citation metadata, references, AI tooling, and release
    validation recipes.
- Correct 1+1 toroidal proposal accounting [`8f3f96b`](https://github.com/acgetchell/causal-triangulations/commit/8f3f96b5802c8205397c39008778dd27e4418a94)

  - Count only backend-committable CDT move sites when computing Metropolis-Hastings proposal ratios.
  - Keep toroidal grow and inverse local moves aligned with the same deterministic committability checks used by the mutating kernels.
  - Document manual cosmological-constant tuning for v0.1.0 until automated lambda-scan utilities land.
- [**breaking**] Harden CDT telemetry and profiled initialization
  [`f7f7244`](https://github.com/acgetchell/causal-triangulations/commit/f7f7244549822bf71581bd4619a84ef2139fdd73)

  - Preserve supplied proposal statistics when constructing simulation results.
  - Leave Metropolis step delta_action unset for self-loop proposals without a concrete plan.
  - Validate profiled open-boundary strip vertex and face counts before constructing foliation metadata.
  - Recompute vertex and timeslice counts when merging volume-profile overrides.
- [**breaking**] Reject volume-profile override overflows
  [`c5a3206`](https://github.com/acgetchell/causal-triangulations/commit/c5a32069aba33ee0e6bb0404259131d46d60cf6d)

  - Make CdtConfig::merge_with_override fail on volume-profile count overflow instead of clamping derived vertices and timeslices.
  - Preserve profile-derived configuration invariants before post-merge validation runs.
  - Track the v0.1.0 public doctest error-handling audit in the roadmap.
- Tighten unwrap Semgrep fixtures [#151](https://github.com/acgetchell/causal-triangulations/pull/151)
  [`2e44e17`](https://github.com/acgetchell/causal-triangulations/commit/2e44e1771d98ac03f62ebb7b8c09f785a7bd0831)

  - Avoid flagging doctest prose that merely mentions .unwrap() or .expect().
  - Keep bench/example unwrap checks scoped away from rust-style fixture helpers.
  - Remove stale bench/example annotations from the rust-style Semgrep fixture.
- Anchor unwrap fixture exclusions [#151](https://github.com/acgetchell/causal-triangulations/pull/151)
  [`5048842`](https://github.com/acgetchell/causal-triangulations/commit/50488420a926e795701938240cb5f8863ae043c1)

  - Tie Semgrep bench/example unwrap exclusions to the rust-style fixture path.
  - Mirror the fixture-path anchors beside the rust-style unwrap fixture helpers.
- Count multi-rule Semgrep fixtures [`657f478`](https://github.com/acgetchell/causal-triangulations/commit/657f478c14dda6d141df6696d50b7ba64e2c7408)

  - Split comma-separated Semgrep fixture annotations before comparing expected findings.
  - Keep Python fixture helpers typed and remove a stale unwrap expectation from the bench/example fixture.
- Require literal UTF-8 test fixture I/O [`759437b`](https://github.com/acgetchell/causal-triangulations/commit/759437bd5b5f6e83394124a536540356390bafda)

  - Tighten the Python test encoding Semgrep rule so Path.read_text and Path.write_text only pass with explicit encoding="utf-8".
- [**breaking**] Make Metropolis step telemetry nonzero [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
  [`f736b97`](https://github.com/acgetchell/causal-triangulations/commit/f736b970c194ec4f71422b3d2120b7aee84d5b48)

  - Store completed Metropolis step numbers as NonZeroU32 in public telemetry and resumable checkpoints.
  - Reject zero-valued completed-step telemetry while preserving step 0 for initial measurement records.
  - Derive accepted planned-step delta_action from the same action_after value used to update CDT state, including reconstructed action values.
  - Return a typed telemetry error when an accepted planned proposal lacks action evidence.

### Maintenance

- Bump pytest in the uv group across 1 directory
  [`eb26876`](https://github.com/acgetchell/causal-triangulations/commit/eb2687684c35811f39ba93843fb429011b4ddb8a)

  Bumps the uv group with 1 update in the / directory: [pytest](https://github.com/pytest-dev/pytest).

  Updates `pytest` from 9.0.2 to 9.0.3

  - [Release notes](https://github.com/pytest-dev/pytest/releases)
  - [Changelog](https://github.com/pytest-dev/pytest/blob/main/CHANGELOG.rst)
  - [Commits](https://github.com/pytest-dev/pytest/compare/9.0.2...9.0.3)

---

  updated-dependencies:

- dependency-name: pytest
  dependency-version: 9.0.3
  dependency-type: direct:development
  dependency-group: uv
  ...
- [**breaking**] Align tooling and CDT error APIs
  [`bc3630b`](https://github.com/acgetchell/causal-triangulations/commit/bc3630b212ea55d0c6a52073cf6cfc3e85a61527)

  - Port Delaunay-style changelog archive and postprocessing utilities.
  - Add Semgrep, coverage, Python, and workflow tooling updates.
  - Regenerate CHANGELOG.md with the new git-cliff pipeline.
  - Enforce toroidal time-slice metadata invariants through concrete CDT errors.
  - Simplify geometry trait result types and rename public Delaunay generator helpers.
  - Replace boxed example errors with CdtResult and derive CdtError with thiserror.
  - Update documentation.
- Add repository rule SARIF gate [`e91f4bb`](https://github.com/acgetchell/causal-triangulations/commit/e91f4bba5dd5c3153ae30b99ff9e18f538d1479e)

  - Add a native Semgrep SARIF workflow for repository-owned rules and document it in the project layout.
  - Harden Semgrep rules for direct delaunay imports, broad Python exceptions, and ad hoc subprocess-result mocks.
  - Standardize changelog scripts on the uv shebang and normalize generated archive links to POSIX paths.
  - Enforce the pinned cargo-llvm-cov version during setup.
  - Replace subprocess-result mocks in Python tests with typed CompletedProcess values.
  - Add focused coverage for mock geometry backend behavior and ergodic move statistics.
- Harden release tooling checks [`201fa76`](https://github.com/acgetchell/causal-triangulations/commit/201fa761fc0b5a048b2dff4da87161d07a745cc0)

  - Normalize changelog source paths before building truncated tag URLs.
  - Add regression coverage for Windows-style archive changelog paths.
  - Run Semgrep rule tests across the full fixture directory.
  - Restore Clippy SARIF pipefail behavior and cargo lint coverage.
- Harden coverage and release tooling [`e4f6ad1`](https://github.com/acgetchell/causal-triangulations/commit/e4f6ad1d7545b6b991b40796c5f69e2a2b5de556)

  - Skip Clippy SARIF uploads for forked pull requests to avoid permission failures.
  - Remove the Codacy coverage upload from the Codecov workflow while repo access is unavailable.
  - Handle git command timeouts in the release tag CLI error path.
  - Add regression coverage for release timeout handling and targeted geometry coverage gaps.
- [**breaking**] Align repository tooling with MCMC
  [`f7e6464`](https://github.com/acgetchell/causal-triangulations/commit/f7e6464eabac157b9378bd304462935b4221f014)

  - Add explicit CodeQL, rustfmt, Python, Semgrep, and changelog tooling configuration adapted for CDT.
  - Modernize changelog generation with the MCMC-style git-cliff template and no-temporary-tag release flow while preserving CDT release-series archiving.
  - Harden Python support scripts against malformed benchmark, coverage, Criterion, and release-tag inputs.
  - Update repository guidance, release docs, command docs, and tooling comparison notes for the new workflows.
- Replace Node formatters with Rust-native checks [#129](https://github.com/acgetchell/causal-triangulations/pull/129)
  [`40a8e74`](https://github.com/acgetchell/causal-triangulations/commit/40a8e744672b556a1a87fec312acca405eb11997)

  - Move Markdown checks to rumdl and keep legacy documentation exclusions explicit.
  - Limit dprint to YAML formatting with the pretty_yaml plugin and add check/fix aliases for TOML, YAML, and shell tooling.
  - Enforce workflow action pinning, allowlisting, and version comments with Semgrep fixtures.
  - Document the Rust-native tooling setup and check-before-fix workflow guidance.
- Tighten coverage and doctest assertion checks [#163](https://github.com/acgetchell/causal-triangulations/pull/163)
  [`36be83f`](https://github.com/acgetchell/causal-triangulations/commit/36be83f18d36d12d3fc079efa4b1f1fa091aea7d)

  - Include `src/lib.rs` in Codecov analysis so public API and `run_simulation` changes are visible in patch coverage.
  - Add a Semgrep rule that rejects `assert!(matches!(...))` in public Rust doctests.
  - Update public examples and development docs to use `std::assert_matches` for diagnostic-friendly enum and result assertions.
- Align security checks with shared Rust workflow
  [`3d5fbec`](https://github.com/acgetchell/causal-triangulations/commit/3d5fbecdf869df9d6d7e98de555eb14f360df9d7)

  - Run the CI matrix through pinned `just ci` tooling on Linux, macOS, and Windows.
  - Move workflow tool installation to cached Cargo installs and uv-managed Python tooling.
  - Add zizmor workflow coverage, SECURITY.md, and Dependabot cooldowns for security maintenance.
  - Expand repository Semgrep rules for GitHub Actions hardening, subprocess wrapper usage, and MCMC adapter boundaries.
  - Mirror pinned tool versions across workflows and justfile setup checks.
- Lock local Python tool installs [`9cf18d7`](https://github.com/acgetchell/causal-triangulations/commit/9cf18d76aa9d9e261593cd2a3b4593ed153dcbf1)

  - Require `setup-tools` to sync uv dependencies with the lockfile.
  - Pin pytest and ty in the dev dependency group so local installs match the locked tooling set.
- Make Semgrep fixture cleanup Windows-safe [`2bec72a`](https://github.com/acgetchell/causal-triangulations/commit/2bec72a7468fd6c250b23ea0e6cf254f42bbfb93)

  - Remove the temporary mirrored Semgrep config directory in one guarded cleanup step.
  - Avoid manual symlink unlinking and directory removal that fails on Windows runners.
- Tighten Semgrep fixture coverage [#162](https://github.com/acgetchell/causal-triangulations/pull/162)
  [`5ff5abe`](https://github.com/acgetchell/causal-triangulations/commit/5ff5abe26511f9e64d340c3f7e8c003a4b0b1376)

  - Port low-risk MCMC Semgrep rules for unchecked Rust APIs and dynamic error erasure in public examples.
  - Check repository Semgrep fixtures with direct annotation counts so path-sensitive rules stay covered.
  - Require explicit UTF-8 fixture I/O in benchmark utility tests to keep Windows runs portable.
- Harden Semgrep and zizmor checks [#162](https://github.com/acgetchell/causal-triangulations/pull/162)
  [`acbf1ad`](https://github.com/acgetchell/causal-triangulations/commit/acbf1ad1673372e05af9f0eb2e49b760d40d0298)

  - Avoid zizmor concurrency group collisions across refs and runs.
  - Validate Semgrep fixture checker inputs before comparing rule results.
  - Accept explicit UTF-8 encodings in fixture rules for both quote styles and the shared UTF8 constant.
- Enforce MCMC sampler boundary [`d523854`](https://github.com/acgetchell/causal-triangulations/commit/d523854737a2b77395741a761b707c85cbea52a8)

  - Add repository Semgrep rules that reject CDT-local Metropolis-Hastings acceptance draws and manual accepted/rejected sampler counters.
  - Cover the new guardrails with positive and negative Semgrep fixtures.
  - Document that CDT owns proposal planning and telemetry while markov-chain-monte-carlo owns reusable sampler mechanics

### Performance

- Speed up Rust validation workflows [`25ada46`](https://github.com/acgetchell/causal-triangulations/commit/25ada46c8ccc18561f439959dc15e1e65b3b8fbf)

  - Run runnable Rust unit, integration, CLI, slow, example, and release test recipes with cargo nextest while keeping rustdoc doctests on cargo test.
  - Build release examples once and execute the compiled binaries for validated example runs.
  - Install pinned cargo-nextest in CI and document the validation split.
- Add CDT CI performance suite [`f449c0b`](https://github.com/acgetchell/causal-triangulations/commit/f449c0be8318b5b9a3a99bfa0b667f56d5d9ce0b)

  - Add a perf-profile Criterion suite for CDT generation, validation, move attempts, ten-sweep random-move workloads, and short Metropolis runs.
  - Wire benchmark analysis and baseline comparison tooling to use stable Criterion benchmark IDs from the new suite.
  - Keep Linux, macOS, and Windows CI coverage while speeding Windows Rust validation with nextest.
  - Align coverage and changelog tooling with the Delaunay refresh, including cargo-llvm-cov 0.8.7 and archive heading normalization.
- Cache Metropolis proposal-site universes [`6b3bfbe`](https://github.com/acgetchell/causal-triangulations/commit/6b3bfbe8643816ec1cb87ff5664330c756adaa19)

  - Cache sampleable local sites per move family and triangulation instance so Metropolis sampling and proposal denominators share the same candidate set
    without repeatedly re-enumerating sites.
  - Give cloned and deserialized triangulations fresh transient identities so cached backend handles cannot leak across distinct states with matching
    modification counts.
  - Store reverse proposal-site counts on concrete CDT proposal plans to keep Hastings ratios tied to the proposed state that was actually planned.
  - Document cached proposal-site semantics and the v0.1.x roadmap split for chunked Metropolis sweeps.
  - Harden changelog formatting when no archived changelog files are present.

## [0.0.1] - 2026-03-23

### Added

- Add CODEOWNERS file and security audit workflow
  [`ead82d2`](https://github.com/acgetchell/causal-triangulations/commit/ead82d255ddcb4987d5e4a038e49a11d09b8abc3)

  - Added a new CODEOWNERS file to specify default owners for the repository.
  - Created a new security audit workflow that runs on push events for Cargo.toml and Cargo.lock files.
  - Updated the CI workflow to include macOS and Windows platforms in addition to Ubuntu.
  - Added a Kani CI workflow that runs on pull requests and pushes.
  - Updated cspell.json with additional words, including "acgetchell".

- Add test kani formal verification [`84edb8c`](https://github.com/acgetchell/causal-triangulations/commit/84edb8c62a17eed6aa725dca26cf43b17b7d6408)

- Add GitHub workflows for code coverage and Kani Rust Verifier
  [`eb07352`](https://github.com/acgetchell/causal-triangulations/commit/eb07352ade62f0a9212c6a307242958fbb472fba)

- Add Codecov badge to README.md [`d7adf30`](https://github.com/acgetchell/causal-triangulations/commit/d7adf306080f0fac4a1ecabcd2e7cc78bf503df6)

- Add verification module for Kani configuration
  [`39646c0`](https://github.com/acgetchell/causal-triangulations/commit/39646c09a8be071968cc32a89d88451efedc8090)

- Add command line argument for number of vertices
  [`0b89a88`](https://github.com/acgetchell/causal-triangulations/commit/0b89a88dec5218f2c3ffb082dc2df7b6a8e65f32)

  This commit adds a new command line argument `number_of_vertices` to the CLI. The user can now specify the number of vertices for the triangulation. The
  minimum value allowed is 3.

- Added useful and passing Kani verification [`a78c792`](https://github.com/acgetchell/causal-triangulations/commit/a78c79240114c3cb5f7dd5babf4295a30d7ae11c)

- Add support for timeslices in CLI [`a5c704d`](https://github.com/acgetchell/causal-triangulations/commit/a5c704d248010194851cb6ba7ca2a8a16d541e8e)

  This commit adds support for specifying the number of timeslices in the command-line interface (CLI). The `Config` struct now includes a `timeslices` field,
  and the `run` function accepts a `Config` object as an argument. The CLI tests have been updated to include the `-t/--timeslices` option.

- Add rand crate as a dependency [`fa4df1b`](https://github.com/acgetchell/causal-triangulations/commit/fa4df1bec0ff0c2ae7adf144c0cbccdcb8cd44c9)

  This commit adds the `rand` crate as a dependency in the `Cargo.toml` file. The version added is 0.8.5.

  The `rand` crate is used for generating random triangles for the triangulation.

- Add print statement to display the number of triangles
  [`de1ce9d`](https://github.com/acgetchell/causal-triangulations/commit/de1ce9db574e0308fc9aa7c23b74787af83d796f)

- Add clippy and kani CI badges to README.md [`78246de`](https://github.com/acgetchell/causal-triangulations/commit/78246def33d843ebd036426873e944f9312a46ef)

- Add generate_random_delaunay2 function [`c79e2b0`](https://github.com/acgetchell/causal-triangulations/commit/c79e2b0d5d0ba43638cb3dfac890ef4030cb5522)

  This commit adds the `generate_random_delaunay2` function to the codebase. This function generates a random Delaunay triangulation with a specified number of
  vertices. The function uses the `spade` library to perform the triangulation.

  The `generate_random_delaunay2` function takes in the number of vertices as an argument and returns a result containing the generated triangulation or an
  insertion error if there was a problem inserting points into the triangulation.

  The implementation of this function includes changes to multiple files, including `src/lib.rs` and `src/triangulation.rs`.

- Add triangulation2 [`628aff1`](https://github.com/acgetchell/causal-triangulations/commit/628aff1121305e5bea34b044c0ea58dfb5047baa)

- Add Security audit badge [`0cc6efd`](https://github.com/acgetchell/causal-triangulations/commit/0cc6efd41d83a8e3affc817bdc20e57f90b8f26f)

- Add .codecov.yml and update dependencies in Cargo.lock
  [`a2aa78a`](https://github.com/acgetchell/causal-triangulations/commit/a2aa78af4416890f1058ada9e22aadcff6ca9cab)

- Configures Dependabot for GitHub Actions and Cargo
  [`f072a82`](https://github.com/acgetchell/causal-triangulations/commit/f072a8283cc57039fa0afcfb1f0394a398fb9d94)

  Configures Dependabot to automatically create pull requests
  for outdated GitHub Actions and Cargo (Rust) dependencies,
  improving dependency management and security.

  Also, adds CI workflow and code coverage workflows. Sets up
  linting checks for Rust, Python, Shell, Markdown, and YAML
  files. Improves code quality and ensures consistency.

- Implements CDT core logic and simulation framework
  [`6a7c4be`](https://github.com/acgetchell/causal-triangulations/commit/6a7c4be28dd240f181a79892ea40c961c6d9c441)

  This commit introduces the core logic for Causal Dynamical
  Triangulations (CDT) including action calculation, ergodic moves,
  and a Metropolis-Hastings simulation framework.

  It adds modules for:

  - Calculating the 2D Regge Action (`cdt/action.rs`).
  - Implementing ergodic moves like (2,2), (1,3), and edge flips
    (`cdt/ergodic_moves.rs`).

  - Metropolis-Hastings algorithm for Monte Carlo sampling
    (`cdt/metropolis.rs`).

  - Causal triangulation data structures (`triangulations/causal.rs`)
  - Basic command line argument handling via `clap`.

  Also includes basic utility functions and integrates `approx` for
  testing. The `justfile` is updated to run toml validation from the
  `scripts` directory, and a python version constraint is set.

- Configures logging and error handling [`45df1b3`](https://github.com/acgetchell/causal-triangulations/commit/45df1b341a1becde7c0dd2226389ea2f4c3f94c5)

  Introduces `env_logger` for logging and a custom `CdtError` enum
  for structured error handling throughout the CDT library.

  This change enhances debuggability and provides a consistent way
  to manage and propagate errors. It also prepares the groundwork
  for more robust error reporting and handling in the future.

- Initial CDT property validation framework [`fb5da74`](https://github.com/acgetchell/causal-triangulations/commit/fb5da74a2fe9907bcb6d0f6d3fbca971676fb99f)

  Adds functions to validate CDT topology, causality, and
  foliation constraints. Placeholder implementations are used
  initially, with TODOs for full implementation. Also adds
  parameter validation to the Delaunay backend.

- Initial project files and structure [`2e40c54`](https://github.com/acgetchell/causal-triangulations/commit/2e40c545f1ebd53d21b979d7a8149eac376b8f68)

  Adds the initial files for the causal-dynamical-triangulations
  project, including code of conduct, contributing guidelines,
  project structure, and essential tooling setup with Just.
  This provides a foundation for community contribution
  and project maintainability.

- Performance analysis scripts and workflow [`e1cf749`](https://github.com/acgetchell/causal-triangulations/commit/e1cf7491e860fa98f94de2ed8782dcebf6897f18)

  Adds Python scripts and Justfile recipes for performance
  analysis, baselining, and reporting. Includes CI integration
  for performance monitoring and regression detection.
  Also adds property-based tests for core algorithms using proptest.

- Performance analysis scripts and workflow [`d6994db`](https://github.com/acgetchell/causal-triangulations/commit/d6994dbd92ab9ab08d4c10481ee1f2f825adfa2b)

  Adds performance analysis workflow using Criterion.rs benchmarks and a
  Python analysis script.

  The workflow:

  - Runs benchmarks on PRs and the main branch
  - Detects regressions with configurable thresholds
  - Generates reports and posts comments to PRs
  - Saves baselines for tracking performance over time

  Also includes changes to clippy workflow to install clippy-sarif
  tools with the toolchain and adds a script to find good seeds.

- Kani CI workflow and Python linting to justfile
  [`5fce5ee`](https://github.com/acgetchell/causal-triangulations/commit/5fce5ee6217288b5fe8312d40b47ea5afb92bd3e)

  Adds a Kani CI workflow for formal verification, including fast and
  full verification jobs. Also, introduces Python linting with Ruff to
  the justfile for code quality checks.

  Updates the setup process to include Python linting tools and
  configures the pyproject.toml file with Ruff dependencies. Enhances
  the README with Kani workflow behavior details.

- Convex hull and boundary edge computation. [`0c6c328`](https://github.com/acgetchell/causal-triangulations/commit/0c6c328b6cd6afdca200fe920e1c834910fddf85)

  Adds triangulation operations for computing the convex hull
  and boundary edges of a triangulation.

  Improves `delaunay` backend validation to avoid flakiness due
  to strict neighbor pointer checks. Now uses basic structural
  validation.

  Updates `delaunay` dependency to v0.6.2 and `arc-swap` to v1.8.0.

### Changed

- First commit [`9931dd3`](https://github.com/acgetchell/causal-triangulations/commit/9931dd3ee25483d7872dd79a0899fae3ddc20633)
- Create README.md [`9797804`](https://github.com/acgetchell/causal-triangulations/commit/9797804f4ea9328baeb079117e4771d75965a4d3)
- Create ci.yml [`238b474`](https://github.com/acgetchell/causal-triangulations/commit/238b474a553ae6e137e0799a344c0d1289d96672)
- Refactor Triangle struct and variable names [`9ae71ce`](https://github.com/acgetchell/causal-triangulations/commit/9ae71cef1c7ac2dd1284fdc0a94bb40a769fadf1)
- Create rust-clippy.yml [`7073fc0`](https://github.com/acgetchell/causal-triangulations/commit/7073fc063b8d61ea931424f1df058a36a2524227)
- Update codeql-action version and add CI badge to README
  [`64e4f1b`](https://github.com/acgetchell/causal-triangulations/commit/64e4f1bf1dc72aac588de335de765915d2f05c5a)
- Update Rust toolchain version and fix formatting in Triangle struct
  [`6e46ced`](https://github.com/acgetchell/causal-triangulations/commit/6e46cedc17b8b1fa568b232673462747486586c3)
- Update README.md [`296b984`](https://github.com/acgetchell/causal-triangulations/commit/296b984e6dae4d866d88d54f0d0a0d18f89aeca8)
- Separate libs from cli and add tests [`dd049b3`](https://github.com/acgetchell/causal-triangulations/commit/dd049b36e52c2a03062841a500d5b139fdb4a4e9)
- Refactor package and test names to use kebab-case
  [`4447215`](https://github.com/acgetchell/causal-triangulations/commit/4447215bc82e9b527ad3127298a0e7474625b93c)
- Refactor code and add proof for triangle center in circumcircle
  [`b12731d`](https://github.com/acgetchell/causal-triangulations/commit/b12731d077489b87a6d74286ed5670bcf7140e3a)
- Refactor codecov.yml and lib.rs [`b5464e3`](https://github.com/acgetchell/causal-triangulations/commit/b5464e3f7b1651e8ab2def6727309c21de4e19e0)
- Refactor GitHub workflow files for improved code coverage and caching
  [`6a23ed8`](https://github.com/acgetchell/causal-triangulations/commit/6a23ed8201e975bdaeaf88021777e7cb13db5810)
- Update Codecov workflow to upload coverage reports and archive code coverage results
  [`0f7c9bf`](https://github.com/acgetchell/causal-triangulations/commit/0f7c9bfe910e518f79a7eceea0b3cebf4dd9683b)
- Create LICENSE [`edffe14`](https://github.com/acgetchell/causal-triangulations/commit/edffe14a54601b46ea60d2c301827245ae946090)
- Refactor Delaunay triangulation code [`0564e2d`](https://github.com/acgetchell/causal-triangulations/commit/0564e2df714de4ea980067480e43d52a6bb113d4)
- Update syn version to 2.0.35, cspell.json with new words, and add a test for run function.
  [`76281a6`](https://github.com/acgetchell/causal-triangulations/commit/76281a6d5c3af80446056ca560dcecfad2dca5ed)
- Refactor Codecov workflow and remove unnecessary code
  [`2a6616a`](https://github.com/acgetchell/causal-triangulations/commit/2a6616a1756f1ba0f66a722f4c12787a6f3abbfb)
- Trying out using Spade [`8542cf4`](https://github.com/acgetchell/causal-triangulations/commit/8542cf4dd1736181fefe63fc8d7e1f4640a7eff2)
- Kani and spade don't get along [`3d084ea`](https://github.com/acgetchell/causal-triangulations/commit/3d084ea5096e71e5e658e7f14fd1e6241c2e1db5)
- Update README [`bae27db`](https://github.com/acgetchell/causal-triangulations/commit/bae27db4e42e4d2ccddb6de5a52454024d03dad5)
- Update Kani CI workflow and dependencies, print vertex positions and triangle positions.
  [`fa01d09`](https://github.com/acgetchell/causal-triangulations/commit/fa01d0913ef4ff6a53b9ad09e9e5b35946c7dac9)
- Update cargo [`28ef382`](https://github.com/acgetchell/causal-triangulations/commit/28ef38207129b36982fbf5d6270449308468dc90)
- Merge remote-tracking branch 'origin/main' into main
  [`3be06b4`](https://github.com/acgetchell/causal-triangulations/commit/3be06b48e4f6aaa3c61b299eaba7ff55da839dab)
- Update dependencies versions in Cargo.lock and add random point generation to triangulation.rs
  [`f534a5e`](https://github.com/acgetchell/causal-triangulations/commit/f534a5e153017c76471fd10a1f8a5fa18cf013d1)
- Refactor code to use external crates for command line arguments and 2D Delaunay triangulation
  [`dbd144c`](https://github.com/acgetchell/causal-triangulations/commit/dbd144c0416ae3178133e81c9adf0f064640d093)
- Update some dependencies [`9001a8d`](https://github.com/acgetchell/causal-triangulations/commit/9001a8d0e6c0dbdf7e32dafe0f1b859dd13ad539)
- Spade fixed. [`58a78a6`](https://github.com/acgetchell/causal-triangulations/commit/58a78a6dcfbdcaf849b7a5b7b62077390547d226)
- First pass at DCEL [`8d20c3b`](https://github.com/acgetchell/causal-triangulations/commit/8d20c3b9a2caccc5a1b93c9addceba7845f27246)
- Update dependencies in Cargo.toml and Cargo.lock
  [`a19ff71`](https://github.com/acgetchell/causal-triangulations/commit/a19ff715dad0bb9eab6a44f57b109fce044ade9f)
- Update package versions in Cargo.lock: [`22745d3`](https://github.com/acgetchell/causal-triangulations/commit/22745d37c77cc386c7b60d6a4e7dadacb2e53cdd)
- Update dependencies [`af94c11`](https://github.com/acgetchell/causal-triangulations/commit/af94c11f843592118a0a4713344db64fc304a69c)
- Refactors triangulation and setup process (internal)
  [`642e824`](https://github.com/acgetchell/causal-triangulations/commit/642e824666bc30c968e11a12786ca8f36f2de849)

  Refactors the project structure to use the `delaunay` crate for 2D
  triangulation, replacing `spade`. This change includes updates to
  Cargo.toml, removal of the old triangulation module, and
  implementation of a basic triangle generation function. Also adds a
  `.justfile` for development workflows and a `rust-toolchain.toml` for
  consistent Rust versions. This is an internal change to improve
  maintainability and prepare for more advanced triangulation
  features.
- Refactors triangulation and updates dependencies
  [`0601450`](https://github.com/acgetchell/causal-triangulations/commit/0601450132252508ddf3c6a4d3b698903eb5bdd9)

  Refactors the triangulation module to use the `delaunay` crate for
  Delaunay triangulation, providing a more robust and efficient
  implementation.

  Also, updates dependencies and fixes markdownlint settings.
  Adds words to the spell check dictionary.
  Removes the old triangulation files.
- Refactors Kani workflow and updates dependencies
  [`aab33e2`](https://github.com/acgetchell/causal-triangulations/commit/aab33e259e60cd4cf219575c2c4343f0ac75a270)

  Refactors the Kani verification workflow to use actions-rust-lang/setup-rust-toolchain.

  Updates Kani action to v1.1.

  Disables shell script linting in CI.
  This is an internal change to improve workflow maintainability.

  Adds a TODO document to track improvements and technical debt.
- Refactors triangulation and simulation architecture
  [`4d56e07`](https://github.com/acgetchell/causal-triangulations/commit/4d56e0734122c3dac90eb8f65637c386af116bbf)

  This commit significantly refactors the crate by separating
  geometric operations from physics constraints for improved
  layering and reusability. It introduces a smart wrapper for
  mutable TDS access with precise cache invalidation and enhances
  error context for triangulation generation. It also addresses
  an incomplete ergodic moves implementation, paving the way
  for a well-architected simulation. The changes include a new
  `moves.md` document detailing the new approach as well as
  integration tests.
- Refactors triangulation backend with trait-based geometry
  [`c05f637`](https://github.com/acgetchell/causal-triangulations/commit/c05f6378f63176003a4644c50d27ac825b5e4be8)

  Migrates triangulation to a trait-based geometry backend, providing
  better abstraction and testability by decoupling CDT algorithms
  from specific geometry implementations.

  This change includes:

  - Introduction of the `geometry` module with traits and backend implementations.
  - Restructuring of the `cdt` module to use the new trait-based backend.
  - Addition of Delaunay and Mock backend implementations.
  - Deprecation of the old `triangulations` module.

  No functional changes. Migration from legacy `CausalTriangulation`
  to new `CdtTriangulation` is recommended.
- Improves test coverage and adds Kani proofs [`16aae91`](https://github.com/acgetchell/causal-triangulations/commit/16aae91b8c09630f7c3670c6b7d9e898c981517f)

  Enhances the testing framework by adding a new testing guide,
  increasing test coverage, and introducing Kani proofs for action
  calculation properties. This ensures more robust and reliable
  code, especially in critical areas like the Regge action
  calculation. Also updates `.gitignore` and cspell to accommodate
  new test-related files and terms. (Internal change)
- 🔧 Build System: Integrates criterion for benchmarking
  [`9891b8b`](https://github.com/acgetchell/causal-triangulations/commit/9891b8bb423f4bafb25e3a65e076dabdda9e4266)
- Refactors triangulation creation and simulation runs
  [`ffa6e35`](https://github.com/acgetchell/causal-triangulations/commit/ffa6e3597375580abd37c0bf52c225c3dcaeb618)

  Refactors the triangulation creation process to use
  `from_random_points` for more clarity and consistency.

  Updates simulation runs to use the `run` method, streamlining
  the simulation execution and reducing redundancy.
  These changes enhance the overall structure and maintainability
  of the CDT simulation framework.
- Enhances testing and performance infrastructure
  [`fc513ac`](https://github.com/acgetchell/causal-triangulations/commit/fc513aca4bf90ca1f0c5d65a3d94141ee8604019)

  Improves performance analysis workflow with more robust
  benchmarking and reporting. Includes workflow updates
  with better compilation validation and error handling,
  adds coverage reporting, and includes new CI checks.
  Includes updates to `.gitignore` and `justfile`
  to support new tooling. (Internal change)
- Improves clippy-sarif tooling installation and cache
  [`4c455c9`](https://github.com/acgetchell/causal-triangulations/commit/4c455c93980883fc24ad33301268673417462c36)

  Refactors the clippy-sarif tooling installation process in the
  GitHub workflow to use a dedicated directory under $HOME and
  updates the PATH. This ensures tools are correctly cached and
  accessible. Also updates the spell-check recipe.

  This change also updates dependencies in `Cargo.lock` and uses `dirs`
  crate instead of env vars for resolving paths, which is more reliable.
- Pre-merge fixes [`0f6ec5a`](https://github.com/acgetchell/causal-triangulations/commit/0f6ec5ab36bd5b32b07eb37d35325f09eaddb59e)
- Improves changelog generation and performance analysis
  [`8e04d89`](https://github.com/acgetchell/causal-triangulations/commit/8e04d89260bf9c26172423ad9c53e9fa016a2412)

  Updates changelog generation to correctly identify PRs and adjusts
  the automatic changelog configuration to ignore test commits. Improves
  performance analysis reporting, including handling edge cases such as
  infinite ratios and refining trend analysis, and adds project root argument.
  These changes enhance the robustness and clarity of both changelog
  creation and performance assessment processes.
- Update tooling and backend compatibility [`5aa1ee8`](https://github.com/acgetchell/causal-triangulations/commit/5aa1ee82eebf822753a0668878f11c732407e77a)

  - Refactor delaunay backend for v0.5.1 API compatibility
    - Update vertex/cell access to use key-based lookups
    - Migrate from direct field access to method calls
    - Handle Phase 3A VertexKey and CellKey changes

  - Rename binary from cdt-rs to cdt for cleaner CLI
    - Update Cargo.toml, tests, docs, and examples
    - Simplify all command invocations

  - Improve justfile with robust patterns from delaunay
    - Add file existence checks for all lint operations
    - Reorganize into logical sections with comments
    - Add lint-code, lint-docs, lint-config groupings
    - Enhance spell-check to handle renamed files
    - Add run-simulation recipe for example script
    - Update help-workflows with better organization

  - Update development scripts
    - Copy improved enhance_commits.py from delaunay
    - Better commit categorization with explicit prefixes

  - Add sweep_results/ to gitignore
- Merge remote-tracking branch 'origin/main' [`c36740a`](https://github.com/acgetchell/causal-triangulations/commit/c36740ae85b5a1ded6556d8a4ecdc6cfa1401552)
- Update use of delaunay crate [`c4eef3c`](https://github.com/acgetchell/causal-triangulations/commit/c4eef3cb55537a4900fa56428ea456dbc276b895)

## Added

- Taplo-based TOML formatting/linting (`just toml-fmt`, `just toml-fmt-check`, `just toml-lint`) and wired into `lint-config`.
- Synced `scripts/` tooling and a full `scripts/tests/` suite (benchmarking, changelog utilities, hardware utils, etc.).

## Changed

- GitHub Actions `.github/workflows/ci.yml` now installs required tools and runs `just ci` on Linux/macOS (Windows keeps cargo build/test).
- `just ci` now includes JSON validation and taplo TOML checks to keep CI coverage up to date.
- Updated docs/config (README/CONTRIBUTING/cspell/pyproject/toolchain) to reflect the current workflows and tooling.

## Fixed

- Resolved delaunay backend and util.rs mismatches and updated to current version of delaunay crate

- Resolved Ruff/test issues in synced Python tooling; improved shell/yaml CI robustness.

- Improve triangulation backend and tooling [`ee00f97`](https://github.com/acgetchell/causal-triangulations/commit/ee00f970642157f57c517be745a06e16115a6f1c)

  Refactors the triangulation backend for better
  performance and maintainability, including adding a toolchain
  file for consistent Rust versions.

  Changes include:

  - Adding a taplo config to mirror Cargo TOML philosophy
  - Adding mypy to the python tooling
  - Adding more complete CI with fix and check commands

- Improve changelog tooling and cache invalidation
  [`6563a84`](https://github.com/acgetchell/causal-triangulations/commit/6563a8444880a780a3e8cd6961f1276f648b0f9f)

  - Refactor scripts/changelog_utils.py release-notes parsing/normalization and helpers
  - Add 125KB tag-message boundary coverage for changelog truncation
  - Add Delaunay edge-cache invalidation helper and clarify mutation/caching expectations
  - Tighten tooling/config (action-lint prep, remove ruff F841 ignore) and small script/test cleanups

- Install actionlint and harden scripts [`aed2f48`](https://github.com/acgetchell/causal-triangulations/commit/aed2f48de0247f02da22385a154d8d772eb4238f)

- CI: install actionlint (v1.7.9) on Linux/macOS runners

- changelog-utils: centralize tag-annotation size limit and add repo URL fallback for CHANGELOG links

- changelog-utils: allow overriding breaking-change API tokens via env

- hardware-utils: refactor hardware comparison to satisfy Ruff complexity rules

- performance-analysis: use explicit UTF-8 for file I/O

- mypy: relax overrides for test modules; update affected tests/docs

- Refactors performance analysis and adds type checking
  [`47a6f4d`](https://github.com/acgetchell/causal-triangulations/commit/47a6f4d6fcdb9a34fc82862381c1f2009bc91efe)

  Refactors performance analysis scripts to validate baseline data
  and improve robustness.

  Adds `ty` for type checking Python scripts, enhancing code
  quality and maintainability. Integrates `ty` into the `justfile`
  and CI workflow.

  Updates hardware info gathering on windows to be more robust.

  These changes improve the reliability and maintainability of
  the project's performance analysis and tooling.

- Pin shfmt version for CI consistency [`01d37fe`](https://github.com/acgetchell/causal-triangulations/commit/01d37fe1a1ed7e60aa728dc3f3976176a600c2ef)

  Pins the `shfmt` version in CI workflows to ensure consistent
  formatting checks across different operating systems (Linux and
  macOS). This change introduces an environment variable
  `SHFMT_VERSION` to define the pinned version and updates the
  installation scripts for both Linux and macOS to use this variable.
  Also refactors `hardware_utils.py` to consistently capture output.
  Refs: refactor/update-delaunay

- Updates actionlint version and installation process
  [`5341b14`](https://github.com/acgetchell/causal-triangulations/commit/5341b1494986b413049ff13aa161e8017616d529)

  Updates actionlint to version 1.7.10 and changes the installation
  process to directly download prebuilt binaries from the GitHub
  releases page to avoid potential cargo-binstall fallback failures.

  Also adds exception handling for several possible exceptions when
  retrieving memory information from the operating system.

- Improves CI workflow with version pinning and checksums
  [`e5c391e`](https://github.com/acgetchell/causal-triangulations/commit/e5c391e08246ff78e2e49f7c111e0feb5560d5d1)

  Updates the CI workflow to pin versions of `markdownlint-cli` and
  `cspell`.

  Adds checksum verification for `actionlint` and `shfmt` installations
  to enhance supply-chain security. This includes fetching checksum
  files and verifying the downloaded binaries against them.

  Refactors hardware utils to handle more exceptions.
  Addresses edge case where `baseline_mem_num` is 0 or negative.

- Updates CI workflow tool versions [`2aa55b2`](https://github.com/acgetchell/causal-triangulations/commit/2aa55b27d0c2b653ff2d8da95285524824140285)

  Updates the versions of markdownlint and cspell in the CI workflow
  to the latest versions.

  Also, adds a step to print the shfmt version in the CI workflow
  for both Linux and macOS to verify the installation.

- Updates tooling and CI configurations [`513847d`](https://github.com/acgetchell/causal-triangulations/commit/513847d38efb79fefaa3f0d33f73f684fb1d3fd6)

  Updates tooling configurations for linters,
  formatters, and CI workflows, improving code
  quality and maintainability (internal).

- Refactors CI workflow for improved tooling and audit
  [`995530d`](https://github.com/acgetchell/causal-triangulations/commit/995530d87cfa414ea848f38bea55a0ce2f7d2c69)

  - Updates CI configuration for enhanced tooling consistency.
  - Replaces cargo-audit action with direct cargo-audit execution for
    better control and reporting.

  - Removes cspell and adds typos-cli for unified spell checking.
  - Improves dependency auditing, Markdown/JSON formatting, and shell
    script validation.

  - Simplifies and consolidates installation of necessary tools.

- Improves CI/audit workflows and scripts [`7f7b32c`](https://github.com/acgetchell/causal-triangulations/commit/7f7b32cc77e01214bc421a5533511aba24bbe498)

  Updates CI and audit workflows for better reliability and tooling.

  Changes include:

  - Pinning actions to specific commit SHAs for reproducibility.
  - Updating `typos` version in CI.
  - Relaxing cargo audit failure to allow json output.
  - Adding `git-cliff` to the `setup` script
  - Moving static methods in changelog_utils
  - Fixing error message and raising RuntimeError

  These changes improve the reliability and maintainability of
  the CI and audit processes.

- Improves CI and audit workflows [`ab69abf`](https://github.com/acgetchell/causal-triangulations/commit/ab69abf97ec44b462e74af7530920d4f975b43ab)

  Updates the CI workflow to use a specific version of dprint and the
  audit workflow to use a specific version of cargo-audit, improving
  reproducibility. Modifies cargo audit to reuse cached advisory DB.
  Adds installation instructions for yamllint for macOS, pip, and uv.

  Updates benchmark comparison to allow configuration of the regression
  threshold. Adds CLI option for gh timeout.

  Updates docs to clarify the sampling method used by
  `select_random_move`.
  Updates `typos` spell-check script.
  Refs: feat/convex-hulls

- Improves performance baseline comparisons [`dcf64ec`](https://github.com/acgetchell/causal-triangulations/commit/dcf64ec660f469956e874e33f6b477c56629d963)

  Refactors the performance baseline comparison script to improve
  reliability and clarity.

  This commit includes changes to handle potential timeout errors
  during baseline fetching and makes the project root optional for
  more flexible usage. It also adjusts the sleep duration in the
  GitHub baseline fetcher to prevent exceeding deadlines.
  Additionally, it modifies the changelog script to better detect
  function declarations.

- Improves benchmark checks in CI workflow [`d86f9f0`](https://github.com/acgetchell/causal-triangulations/commit/d86f9f0c5206aa32c0554757298e186db3d3c32f)

  Updates the benchmark checks in the performance CI workflow to
  ensure both the `[[bench]]` configuration exists in `Cargo.toml`
  and that benchmark files are present in the `benches/` directory.
  This change provides more accurate warnings and prevents errors
  when benchmarks are not properly configured. Internal.

- Rename crate to causal-triangulations and upgrade to delaunay v0.7.2
  [`cbc5935`](https://github.com/acgetchell/causal-triangulations/commit/cbc59350f028a50a2e48b271271b3575404a1b8a)

- Remove Kani and update dev documentation [`a48c38a`](https://github.com/acgetchell/causal-triangulations/commit/a48c38afae9499bae184d0bee68b4a28075d5e8d)

  Remove Kani formal verification

  - Remove all `#[cfg(kani)]` proof modules from action.rs,
    ergodic_moves.rs, and config.rs

  - Delete `.github/workflows/kani.yml` CI workflow
  - Remove `kani` and `kani-fast` just recipes; drop from
    `commit-check` and `setup`

  - Remove `cfg(kani)` from Cargo.toml check-cfg
  - Clean Kani references from README, CONTRIBUTING, AGENTS,
    docs/RELEASING, and docs/dev/testing

  Kani's bundled nightly (rustc 1.93.0) is incompatible with the
  MSRV (1.94.0), and the proofs only covered pure arithmetic that
  proptest already handles. With `#![forbid(unsafe_code)]`, model
  checking adds maintenance cost without proportional benefit.

  Add developer documentation

  - Add docs/dev/commands.md, docs/dev/rust.md, docs/dev/testing.md
  - Expand AGENTS.md with detailed agent guidelines

  Improve scripts and tests

  - changelog_utils.py: raise ChangelogError with descriptive
    messages instead of calling sys.exit

  - hardware_utils.py: update TYPE_CHECKING imports
  - Add type hints to Python test functions
  - Adjust cross-platform skip logic in subprocess tests

  Fix geometry and test parameters

  - Use dt.validate() instead of dt.is_valid() in delaunay backend
  - Reduce proptest ranges in triangulation.rs to avoid Tarpaulin
    timeouts (vertices 3..30, timeslices 0..6)

- Improve error types, simplify trait bounds, and re-enable determinism test
  [`2fe2953`](https://github.com/acgetchell/causal-triangulations/commit/2fe29539004b9cd915b1bb534c16796b59284142)

  - Add ValidationFailed error variant with structured check/detail fields
    and diagnostic context (V/E/F counts) for geometry, topology, and
    Delaunay validation failures

  - Replace InvalidParameters with ValidationFailed in validate() and
    validate_topology() to distinguish runtime validation from bad input

  - Use InvalidGenerationParameters in factory functions (from_random_points,
    from_seeded_points) for consistency with generate_delaunay2_with_context

  - Remove unnecessary + 'static bounds from all DataType where clauses
    in DelaunayBackend (upstream DataType already implies owned data)

  - Merge two inherent impl blocks for DelaunayBackend into one by
    dropping the redundant serde bound on is_delaunay() and topology_kind()

  - Move function-scoped imports to module level in metropolis tests
  - Re-enable seeded_determinism_property proptest (RobustKernel +
    DelaunayTriangulationBuilder resolved the FastKernel non-determinism)

  - Update Cargo.lock from cargo update (51 packages)
  - Update justfile, Python scripts, and dev docs

- Separate is_valid from is_delaunay, add test coverage, harden test guards
  [`c0c8236`](https://github.com/acgetchell/causal-triangulations/commit/c0c82364cf202792033d6e545148037c9ae8d0a3)

  - Separate is_valid() (Levels 1-3 structural/topological via
    as_triangulation().validate()) from is_delaunay() (Levels 1-4
    including Delaunay property) so the two checks are distinct

  - Rename test_is_valid_checks_coherent_orientation to
    test_is_valid_runs_structural_validation with accurate comment

  - Add @pytest.mark.skipif guard on TestRunGitCommand for environments
    without git (consistent with existing TestRunCargoCommand pattern)

  - Add check_git_repo() guard in test_get_git_remote_url_returns_url
    before calling run_git_command(["remote"])

  - Port test_benchmark_models.py from delaunay repo (22 tests for
    benchmark data models, parsing, and formatting)

  - Port test_changelog_tag_size_limit.py from delaunay repo (7 tests
    for 125KB GitHub tag annotation limit handling, adapted to use
    synthetic content since CDT has no released versions yet)

- Harden and reorganize Python test suite [`2bbe623`](https://github.com/acgetchell/causal-triangulations/commit/2bbe623bc61aaff608f0e9c8a6a2c510bf7a6f47)

  Improve test isolation by mocking filesystem lookups and adding
  environment guards for git-dependent utilities. Reorganize benchmark
  model tests into appropriate classes and update mock call inspection
  logic in changelog tests for better maintainability.

- Update changelog for crate rename and delaunay v0.7.2 upgrade
  [`95998e0`](https://github.com/acgetchell/causal-triangulations/commit/95998e082bf4e3b997350b511d45982a817c9f5a)

  Synchronize CHANGELOG.md with recent milestones, including the crate
  rename to causal-triangulations, the upgrade to delaunay v0.7.2, and
  the removal of Kani verification. This internal update also modernizes
  Python test code to use parenthesized context managers.

### Dependencies

- Bump actions/cache from 4.2.4 to 4.3.0 [`7d3a7f2`](https://github.com/acgetchell/causal-triangulations/commit/7d3a7f26ecb64fe4a0596bd7b68dd9b1d31b191b)
- Bump actions/checkout from 4 to 5 [`ee667c9`](https://github.com/acgetchell/causal-triangulations/commit/ee667c91fae756ce6a29fd673211bb4f8e820c1e)
- Bump astral-sh/setup-uv from 6.7.0 to 6.8.0 [`db2daee`](https://github.com/acgetchell/causal-triangulations/commit/db2daee6eef0b670246a9bf68351b78f28890556)
- Bump actions/github-script from 7.0.1 to 8.0.0
  [`fbbd871`](https://github.com/acgetchell/causal-triangulations/commit/fbbd871a85670716736581836f3bca423463a91c)
- Bump astral-sh/setup-uv from 6.8.0 to 7.1.0 [`67bb4dc`](https://github.com/acgetchell/causal-triangulations/commit/67bb4dc19fe93aae5dab0980bac499bfde61c6bc)
- Bump actions-rust-lang/setup-rust-toolchain from 1.15.0 to 1.15.2
  [`76b881a`](https://github.com/acgetchell/causal-triangulations/commit/76b881aa421f250444a45e257bd97f41b03cc2cb)
- Bump astral-sh/setup-uv from 7.1.0 to 7.1.1 [`afceea3`](https://github.com/acgetchell/causal-triangulations/commit/afceea3dd8e181c2b4f6d6132eaf80ebd444b3b7)
- Bump clap from 4.5.49 to 4.5.50 in the dependencies group
  [`b6e1d89`](https://github.com/acgetchell/causal-triangulations/commit/b6e1d89d97b6ce4f377b22275c066183e0d2bfcb)
- Bump actions/setup-python from 6.0.0 to 6.1.0 [`31ba11c`](https://github.com/acgetchell/causal-triangulations/commit/31ba11c8d65b673d5b039079cb6ac61ed7921e4e)
- Bump actions/upload-artifact from 4.6.2 to 6.0.0
  [`5f5de78`](https://github.com/acgetchell/causal-triangulations/commit/5f5de78298d57a11d51386a4b53349f6f7bde413)
- Bump the dependencies group across 1 directory with 7 updates
  [`dc79541`](https://github.com/acgetchell/causal-triangulations/commit/dc795410c493878b1e9c86fa6934729251ae948a)
- Bump codecov/codecov-action from 5.5.1 to 5.5.2
  [`2cbe0aa`](https://github.com/acgetchell/causal-triangulations/commit/2cbe0aad5418ad5b04c304247d400d93d5af7264)
- Bump astral-sh/setup-uv from 7.1.1 to 7.1.6 [`fd22207`](https://github.com/acgetchell/causal-triangulations/commit/fd22207d48aed48e87eb6a608f836c2f9da2d2c8)
- Bump actions/cache from 4 to 5 [`08957fc`](https://github.com/acgetchell/causal-triangulations/commit/08957fcc7156ff56f08ec8898840cd86b4e5f1d6)
- Bump actions/checkout from 5.0.0 to 6.0.1 [`a58bb11`](https://github.com/acgetchell/causal-triangulations/commit/a58bb113b2bbaae002ef0e765be62ffa46d16013)
- Bump actions/setup-python from 6.1.0 to 6.2.0 [`3fa4378`](https://github.com/acgetchell/causal-triangulations/commit/3fa437896eac9f1b913efe8f28e2dab55a931084)
- Bump actions/checkout from 5.0.1 to 6.0.2 [`50a3a48`](https://github.com/acgetchell/causal-triangulations/commit/50a3a48ae5018e713ed62df96400fa73c219e033)
- Bump astral-sh/setup-uv from 7.1.6 to 7.3.0 [`d8b0667`](https://github.com/acgetchell/causal-triangulations/commit/d8b06675ffbfbfa73b102fdee6a7b39176532390)
- Bump taiki-e/install-action from 2.65.1 to 2.68.6
  [`025e514`](https://github.com/acgetchell/causal-triangulations/commit/025e514a58a355f3b1c60d45d43cd0ded41a3b52)
- Bump actions/upload-artifact from 6.0.0 to 7.0.0
  [`bd961ba`](https://github.com/acgetchell/causal-triangulations/commit/bd961ba1166b0bde1bebf9f0f517cdb29404d56c)
- Bump taiki-e/install-action from 2.68.6 to 2.68.35
  [`95bf128`](https://github.com/acgetchell/causal-triangulations/commit/95bf1287b125981b2976e6ff9d6bf04fd51c054a)
- Bump astral-sh/setup-uv from 7.3.0 to 7.6.0 [`c99861a`](https://github.com/acgetchell/causal-triangulations/commit/c99861ab3b2bdf608ad21721c09b6e3eb5577dcb)
- Bump actions-rust-lang/setup-rust-toolchain from 1.15.2 to 1.15.4
  [`902a210`](https://github.com/acgetchell/causal-triangulations/commit/902a21076017a223f53ec1f7a4c078fba7a27468)

### Documentation

- Update status badges [`000cba5`](https://github.com/acgetchell/causal-triangulations/commit/000cba5d81fc776a17b0098c5abda36ce8586bce)

### Fixed

- Fix Kani [`b36e6ae`](https://github.com/acgetchell/causal-triangulations/commit/b36e6ae7d43b48b82671003af384ef12d6f22467)
- Fix modules [`3d11b0c`](https://github.com/acgetchell/causal-triangulations/commit/3d11b0cb90c911f8d2a86b0bc5f3663506757c9d)
- Fix spade to version 2.2.1 [`cfb0cae`](https://github.com/acgetchell/causal-triangulations/commit/cfb0cae055c21087b402a6701daee9b4b8fcbbf7)
- Validates Delaunay property in CDT triangulation
  [`ebff8ac`](https://github.com/acgetchell/causal-triangulations/commit/ebff8ac6d9bb0c0af8f0219fe85965ba893eaa68)

  Validates the Delaunay property within the Constrained Delaunay
  Triangulation (CDT) implementation by leveraging the `is_delaunay`
  method in the Delaunay backend. This ensures that triangulations
  conform to the Delaunay criteria, improving the robustness and
  correctness of the triangulation process. Adds tests for Delaunay
  validation.
- Action linearity check overflow and Tds deprecation
  [`803ab24`](https://github.com/acgetchell/causal-triangulations/commit/803ab248babed019b6aa583100b20ca576a22774)

  Addresses potential integer overflows in the action linearity
  check by adding a pre-check. Now the linearity assertion is only
  performed if simplex counts are within safe bounds.

  Also, marks legacy Tds-based simulation code as deprecated,
  guiding users towards the trait-based backend API for better
  abstraction and maintainability. The legacy code remains for
  backward compatibility but its usage is discouraged.
- Validates config and handles CLI arg parsing errors
  [`cde9605`](https://github.com/acgetchell/causal-triangulations/commit/cde960597cd5a77f604f3847621da0e814c99a9e)

  Fixes the CLI argument parsing and adds comprehensive config validation to ensure simulation parameters are within acceptable ranges. This includes checks for
  positive measurement frequency, vertices count, and
  temperature. Catches `SystemExit` exceptions and returns corresponding
  error codes.

  Also updates auto-changelog config and benchmark names for clarity.
  The CI workflow is modified to target the `examples/scripts` directory
  for linting and formatting checks.
- Fix config.rs and other script errors [`916d103`](https://github.com/acgetchell/causal-triangulations/commit/916d1036db4d3d0293dbf09460ceeb2139557c3b)
- Fix changelog_utils and performance_analysis and update CHANGELOG.md
  [`38162bd`](https://github.com/acgetchell/causal-triangulations/commit/38162bdef4deacba9e5213088e06c867c14dcc16)
- Correctly identifies estimates files in criterion runs
  [`83f9de0`](https://github.com/acgetchell/causal-triangulations/commit/83f9de033d902cc1d12bec8cb96bb50e250973bf)

  Fixes an issue where the performance analyzer was not correctly
  identifying estimates files in criterion runs, specifically when
  located in the "new" directory.

  Updates the glob pattern to search for "estimates.json" in all
  directories and then filters to only include files under "new" or
  "base" directories. This ensures that the analyzer correctly
  parses the performance data from criterion benchmark results.
- Handles exceptions when getting CPU info, Kani, CI hardening
  [`da415a5`](https://github.com/acgetchell/causal-triangulations/commit/da415a534742de9bf2a405926b37513188aaa289)

  Corrects exception handling in hardware utils to specifically
  catch subprocess errors and other related exceptions when
  retrieving CPU information. This prevents the script from
  crashing when it fails to gather CPU details.

  Better job of checking Kani versions.

  Better way of installing ActionLint for CI.

### Maintenance

- Update crates [`cbdb784`](https://github.com/acgetchell/causal-triangulations/commit/cbdb78427663bfbf9d2438b2833e0d2b7205bed3)
- Update GitHub actions [`6b38039`](https://github.com/acgetchell/causal-triangulations/commit/6b380394a8044ce887d686cc0d4245040d83b65b)
- Fix libc [`9ff914d`](https://github.com/acgetchell/causal-triangulations/commit/9ff914dc19e3aff7e67a305400655c2465959d70)
- Update dependencies [`b191596`](https://github.com/acgetchell/causal-triangulations/commit/b1915961335a446601da417c790fbdd7e46e6268)
- Fix [`e7fc2b8`](https://github.com/acgetchell/causal-triangulations/commit/e7fc2b8632a787484a5a3fb3b3da90a627feb83f)
- Fix CodeCov [`ef8ae5b`](https://github.com/acgetchell/causal-triangulations/commit/ef8ae5b2a0991d87d8931b58acff6aa235558724)
- Ignore cargo kani for Codecov [`4732a05`](https://github.com/acgetchell/causal-triangulations/commit/4732a0544abb4f2a155f73290654a4270046c084)
- Ignore unused functions [`6c43277`](https://github.com/acgetchell/causal-triangulations/commit/6c4327750792c9fe11efb01f26a156115c3460b9)

  These should get replaced by https://github.com/acgetchell/d-delaunay
- Add missed unused function [`fe7670c`](https://github.com/acgetchell/causal-triangulations/commit/fe7670c5c403923b7cfcfce5b972edc0a228687c)
- Update dependencies [`5ed0203`](https://github.com/acgetchell/causal-triangulations/commit/5ed02038c76311c7a1582549608373759c75dc4a)
- Remove Node.js/markdownlint infrastructure and fix stale docs
  [`d9d74d9`](https://github.com/acgetchell/causal-triangulations/commit/d9d74d935e982ce036d06dcf43f63a28d01ce45f)

  Drop the `setup-node` + `npm install -g markdownlint-cli` steps from
  CI and all associated scaffolding; `dprint` has been the active
  markdown formatter for some time.

  - ci.yml: remove `setup-node` action, `npm install -g markdownlint-cli`
    step, and `MARKDOWNLINT_VERSION` env var

  - .markdownlint.json: delete orphaned config file
  - justfile: remove `_ensure-npx` helper (no recipe referenced it)
  - README.md: replace `just dev` with `just fix`/`just check`; remove
    Node.js/npx from tools checklist; fix MSRV 1.92.0 → 1.93.0

  - CONTRIBUTING.md: replace all `just dev` refs with `just fix`/`just
    check`/`just test`; fix MSRV 1.92.0 → 1.93.0 (4 sites); fix Edition
    2021 → 2024; fix Kani toolchain note MSRV

  - docs/CLI_EXAMPLES.md: fix `--simulate` default (true, not false);
    fix triangulation-only example to pass `--simulate false`

  - docs/TODO.md: tick off integration tests and CI/CD improvements as
    done with current counts; update date to 2026-02-21

  - docs/testing.md: update test counts (155 unit, 8 integration, 10 CLI,
    408 Python); fix coverage reference (`just coverage` → HTML,
    `cargo tarpaulin --out Json` → JSON for `just coverage-report`)
- Remove auto-changelog artifacts and restrict kani-full to manual
  [`6dc7269`](https://github.com/acgetchell/causal-triangulations/commit/6dc726979edeb200bb2000aea75f271effcab7c6)

  - .auto-changelog: delete legacy auto-changelog config (referenced the
    now-deleted docs/templates/changelog.hbs Handlebars template)

  - docs/templates/changelog.hbs: delete Handlebars template for
    auto-changelog; git-cliff with cliff.toml is the active tool

  - cliff.toml: remove stale comment referencing changelog.hbs
  - kani.yml: restrict kani-full job to workflow_dispatch only (was
    running on every push to main; move to manual trigger until all
    harnesses pass reliably)

  - CHANGELOG.md: regenerated via just changelog-update
- Fix review findings across docs, scripts, and backend
  [`6195b11`](https://github.com/acgetchell/causal-triangulations/commit/6195b111856931353273e89f60b0304708a04022)

  - Update CI Expectations in docs/dev/testing.md to match actual
    just ci recipe (check, bench-compile, test-all, examples)

  - Fix orphaned docstring fragment in changelog_utils.py main()
  - Add prettier/npx to setup-tools verification so "Tooling setup
    complete" guarantees yaml-fix will not hard-fail

  - Convert four classmethods to @staticmethod in changelog_utils.py
    (_strip_heading_like_emphasis, _convert_fenced_code_blocks_to_indented,
    _indented_block_looks_like_code, _format_indented_code_block_line)

  - Extract _contextual_patterns regex tuple to class-level constant
    _CONTEXTUAL_CODE_PATTERNS (compiled once, not per-call)

  - Add return type annotation to fake_run test helper
  - Fix is_valid comment: dt.validate() runs Levels 1-4, not 1-3
  - Apply dprint formatting to README.md

### Removed

- Remove DCEL [`45b2a17`](https://github.com/acgetchell/causal-triangulations/commit/45b2a17f0392934b2d8b1dcbc0abe5cb6ea3ab92)
- Legacy Kani workflow file [`6ab5176`](https://github.com/acgetchell/causal-triangulations/commit/6ab51760c34969547a1b5e49d44aa33353d2d19d)

  Removes the old Kani workflow file as it is no longer needed.

[Unreleased]: https://github.com/acgetchell/causal-triangulations/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/acgetchell/causal-triangulations/tree/v0.0.1
