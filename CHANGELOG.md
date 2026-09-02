# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-09-02

### ⚠️ Breaking Changes

- Adopt stable Delaunay mesh interchange
- Add lifecycle validation profiles [#196](https://github.com/acgetchell/causal-triangulations/pull/196)
- Add versioned checkpoint wire format [#218](https://github.com/acgetchell/causal-triangulations/pull/218)
- Enforce fallible observable and trace invariants
- Stop returning faces from vertex removal
- Tighten typed API workflows [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
- Propagate checkpoint result invariant failures
- Enforce exact move feasibility and Python 3.14 tooling
- Harden proposal accounting and restored state
- Enforce simulation invariants end to end [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
- Preserve exact layered CDT states
- Align dependencies and repository automation [#216](https://github.com/acgetchell/causal-triangulations/pull/216)
- Refresh Rust, backends, and update tooling
- Replace owned snapshots with borrowed views [#222](https://github.com/acgetchell/causal-triangulations/pull/222)

### Merged Pull Requests

- Bump astral-sh/setup-uv in the github-actions group [#266](https://github.com/acgetchell/causal-triangulations/pull/266)
- Bump the dependencies group with 2 updates [#263](https://github.com/acgetchell/causal-triangulations/pull/263)
- Bump the github-actions group with 3 updates [#262](https://github.com/acgetchell/causal-triangulations/pull/262)
- Tighten release and fixture validation [#261](https://github.com/acgetchell/causal-triangulations/pull/261)
- Preserve evolved checkpoint states [#253](https://github.com/acgetchell/causal-triangulations/pull/253)
- Bump the github-actions group with 5 updates [#251](https://github.com/acgetchell/causal-triangulations/pull/251)
- Strengthen proposal-policy contract coverage [#233](https://github.com/acgetchell/causal-triangulations/pull/233)
- Remove redundant proposal-policy closure [#233](https://github.com/acgetchell/causal-triangulations/pull/233)
- Preserve mock topology during vertex removal [#225](https://github.com/acgetchell/causal-triangulations/pull/225)
- Tighten typed API workflows [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
- Enforce simulation invariants end to end [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
- Replace owned snapshots with borrowed views [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
- Bound volume-move seed discovery [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
- Reject incompatible legacy toroidal mode [#219](https://github.com/acgetchell/causal-triangulations/pull/219)
- Add versioned checkpoint wire format [#218](https://github.com/acgetchell/causal-triangulations/pull/218)
- Align dependencies and repository automation [#216](https://github.com/acgetchell/causal-triangulations/pull/216)
- Add lifecycle validation profiles [#196](https://github.com/acgetchell/causal-triangulations/pull/196)
- Validate evolved straight-line embeddings [#195](https://github.com/acgetchell/causal-triangulations/pull/195)
- Harden summary mesh export handling [#194](https://github.com/acgetchell/causal-triangulations/pull/194)
- Delegate insertion barycenters to Delaunay [#147](https://github.com/acgetchell/causal-triangulations/pull/147)

### Added

- Add notebook-first CDT analysis workflows [`16d37d5`](https://github.com/acgetchell/causal-triangulations/commit/16d37d506e47ba82b4f514c882388f3b8c7a55b8)

  - Add uv-managed Jupyter notebooks for quickstart, spacetime visualization, and Polars/Parquet analysis caches.
  - Export stable final-triangulation mesh data with validated spacetime coordinates for notebook and downstream visualization consumers.
  - Tighten open-boundary foliation validation around spatial interval order, time-coordinate consistency, and noncrossing slab embeddings.
  - Move quickstart setup into the notebook path and reorganize docs around scientific scope, HPC notebook workflows, citation metadata, and lower-case
    documentation names.
  - Add notebook hygiene checks, Semgrep guards, and Python 3.13 notebook/tooling alignment.
- Add causal-filtered Delaunay strip construction
  [`c56e29c`](https://github.com/acgetchell/causal-triangulations/commit/c56e29c2d77dcbbe3e3e39677280a685076ecb0d)

  - Add `from_filtered_delaunay_strip` to build open-boundary CDT starts by removing vertices incident to non-strict causal simplices until the strict violation
    count reaches zero.
  - Expose `strict_causal_simplex_violation_count` as a current-foliation invariant check for detecting non-causal top-dimensional simplices.
  - Route Delaunay-backed vertex removal through the upstream `remove_vertex` API and document the temporary empty replacement-face return contract.
  - Document the CDT-plusplus influence and the distinction between Delaunay Level 4 validation and the CDT strict-causality invariant.
- [**breaking**] Adopt stable Delaunay mesh interchange
  [`1ab4d2e`](https://github.com/acgetchell/causal-triangulations/commit/1ab4d2e2c010ea6bab362d4f98a6c35b21fa5094)

  - Export final triangulations using versioned UUID identities and adjacency.
  - Preserve CDT foliation labels in a UUID-keyed sidecar.
  - Validate the interchange contract in the visualization workflow.
- Validate evolved straight-line embeddings [#195](https://github.com/acgetchell/causal-triangulations/pull/195)
  [`36a6949`](https://github.com/acgetchell/causal-triangulations/commit/36a6949c667786f36ee859d43f303cac11d005e3)

  - Separate Level 1-4 embedding checks from Level 1-5 Delaunay validation.
  - Reject overlapping or degenerate evolved geometries without requiring Delaunay-ness.
  - Preserve recoverable proposal rejections while avoiding duplicate whole-mesh validation.
- [**breaking**] Add lifecycle validation profiles [#196](https://github.com/acgetchell/causal-triangulations/pull/196)
  [`5b5e368`](https://github.com/acgetchell/causal-triangulations/commit/5b5e3680460a065b5b85498196473470dbc5ec9f)

  - Add initial, evolved, and strict-Delaunay profiles that separate required embedding checks from optional Delaunay validation.
  - Require current foliation and strict causal simplex validation for named CDT profiles while preserving a geometry-only path for raw experiments.
  - Reuse backend embedding evidence during move finalization and checkpoint creation to avoid duplicate global validation.
- Expose borrowed proposal-policy views [`c1fcb0d`](https://github.com/acgetchell/causal-triangulations/commit/c1fcb0d52f84d0e1a77f89a4f40faffeca74bdf7)

  - Add stable move-family identifiers and opaque, versioned site IDs for deterministic proposal-policy inspection.
  - Share the canonical offered-site universe with conventional and external policies without exposing mutable geometry.
  - Distinguish offered proposal support from stronger eligible-site guarantees and preserve failed offers as self-loops.
  - Reject foreign, stale, cross-family, and out-of-range site identifiers with typed errors.
- Add weighted move-family proposal policies [`71c822c`](https://github.com/acgetchell/causal-triangulations/commit/71c822c3d1720232b740406bbd006d0167c9d061)

  - Support checked fixed and state-dependent family weights with exact categorical sampling and forward/reverse Hastings corrections.
  - Add policy-aware simulation and checkpoint continuation while keeping policy persistence caller-owned.
  - Expose typed proposal-planning outcomes and per-step kernel telemetry for probability auditing.
  - Document policy configuration, detailed-balance semantics, and the public simulation workflow.
- Standardize release preparation and evidence [`a9c6ef6`](https://github.com/acgetchell/causal-triangulations/commit/a9c6ef6172986ee1a8175206414ee7c0313ee15c)

  - Add transactional release preparation that synchronizes package, citation, changelog, and documentation metadata.
  - Retain validated benchmark evidence, publish Criterion release assets, and render reproducible performance reports.
  - Guard floating-point reproducibility by rejecting algebraic operations while preserving deliberate FMA.
  - Correct scientific and software references across project documentation.
- [**breaking**] Add versioned checkpoint wire format [#218](https://github.com/acgetchell/causal-triangulations/pull/218)
  [`19e1bd2`](https://github.com/acgetchell/causal-triangulations/commit/19e1bd2e1ab3441e2b5f4a8b972a086c5dce1e88)

  - Store geometry, chain state, telemetry, elapsed time, and both RNG streams in a CDT-owned v1 JSON schema.
  - Restore exact Level 1–4 geometry before validating topology, foliation, causality, action state, and counters.
  - Report unsupported versions through a typed error and document checkpoint compatibility boundaries.

### Changed

- [**breaking**] Enforce fallible observable and trace invariants
  [`513140f`](https://github.com/acgetchell/causal-triangulations/commit/513140f1d2a22e505c212389c981c8fefacbc3d1)

  - Make CDT observable estimators and volume-profile APIs return typed errors instead of silently collapsing backend or overflow failures.
  - Store Metropolis proposal plans and scalar trace rows with invariant-bearing action and count evidence before serialization.
  - Validate checkpoint, result, move-statistics, and measurement reconstruction through typed CDT error paths.
  - Carry API docs, examples, benchmarks, citation metadata, and the lockfile through the stricter contracts.
- Pin rumdl to 0.2.14 across tooling [`329cabc`](https://github.com/acgetchell/causal-triangulations/commit/329cabcbc4f20bc0a8eeae0167a88d5628fa664f)

  This update applies to the CI workflow, local justfile commands,
  and relevant documentation, ensuring consistency across tooling
  and leveraging the latest improvements in the Markdown linter.
- Adapt notebook CDT binary lookup for Windows executables
  [`feb54e9`](https://github.com/acgetchell/causal-triangulations/commit/feb54e93a5b95ee0cc5a6a277b592a5140f25d0c)

  Modify the `get_cdt_binary_path` function to check for both `cdt` and
  `cdt.exe` binaries. This ensures notebooks can locate the correct
  executable for CDT operations across Unix and Windows environments.
  Refs: #176
- Cover causal filtering edge cases [`a5e712a`](https://github.com/acgetchell/causal-triangulations/commit/a5e712abc8351ae19255e511d0bd15fdffb16a37)

  - Add a reusable same-slice non-strict triangle fixture for filtering tests.
  - Exercise removal candidate selection, minimum slice-size refusal, strict no-op behavior, missing live labels, and pass-budget exhaustion.
- [**breaking**] Stop returning faces from vertex removal
  [`cf67f6b`](https://github.com/acgetchell/causal-triangulations/commit/cf67f6b62168e9b6fd353b98308fa99d33cccc2a)

  - Return unit after a validated removal instead of fabricating replacement-face handles.
  - Preserve specialized inverse k=1 and generic cavity-retriangulation paths.
  - Apply the contract consistently across CDT, Delaunay, and mock backends.
- Delegate insertion barycenters to Delaunay [#147](https://github.com/acgetchell/causal-triangulations/pull/147)
  [`1e2bc36`](https://github.com/acgetchell/causal-triangulations/commit/1e2bc360d84d0a5f98ac624473fb680349ee7e0a)

  - Use topology-aware simplex barycenters for open and toroidal insertion sites.
  - Remove duplicated periodic coordinate-selection logic from CDT move planning.
  - Preserve typed barycenter diagnostics and reinforce validation boundaries.
- Strengthen proposal-policy contract coverage [#233](https://github.com/acgetchell/causal-triangulations/pull/233)
  [`7f97141`](https://github.com/acgetchell/causal-triangulations/commit/7f971416570e9bc8109385bf595c09d3a576b4cf)

  - Verify policy facts and deterministic reverse site iteration.
  - Assert structured diagnostics for foreign, stale, cross-family, and out-of-range site identifiers.
- Remove redundant proposal-policy closure [#233](https://github.com/acgetchell/causal-triangulations/pull/233)
  [`bdd342a`](https://github.com/acgetchell/causal-triangulations/commit/bdd342a06acb179bf5b08ee4935badff632fea1e)
- [**breaking**] Tighten typed API workflows [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
  [`9702766`](https://github.com/acgetchell/causal-triangulations/commit/970276688e31c1478136ed108384fac16b2100cb)

  - Bind move-family policies once across run, checkpoint, and resume workflows.
  - Preserve structured failure sources and stage context across geometry, rollback, observables, and output handling.
  - Narrow geometry bounds and make CLI, notebook, benchmark, and tooling workflows deterministic.
- Cover invalid checkpoint validation intervals [`4a42d73`](https://github.com/acgetchell/causal-triangulations/commit/4a42d73627647d368e8cb32f2f29fc6b9f88953b)

  - Verify versioned checkpoints reject a zero Delaunay validation cadence.
  - Reuse deserialization errors by reference in checkpoint assertions.

### Dependencies

- Bump github/codeql-action from 4.36.0 to 4.36.1
  [`3bbf6e6`](https://github.com/acgetchell/causal-triangulations/commit/3bbf6e6697afac8a9d33961974ff92cc36bbf16a)

- Bump actions/checkout from 6.0.2 to 6.0.3 [`571c69c`](https://github.com/acgetchell/causal-triangulations/commit/571c69c7dbffb30eff26bfc3e8e4a61eb103edce)

- Bump starlette in the uv group across 1 directory
  [`dc3c325`](https://github.com/acgetchell/causal-triangulations/commit/dc3c325fa79985febc383b9ab884f778ceb15cb2)

- Bump the github-actions group with 5 updates [#251](https://github.com/acgetchell/causal-triangulations/pull/251)
  [`d081b1e`](https://github.com/acgetchell/causal-triangulations/commit/d081b1eb85c30562a7b4c381b822ee62a4a93a67)

- Bump the github-actions group with 3 updates [#262](https://github.com/acgetchell/causal-triangulations/pull/262)
  [`ef91ab3`](https://github.com/acgetchell/causal-triangulations/commit/ef91ab3a514cfdd3515a236057d08a83a81bde28)

- Bump the dependencies group with 2 updates [#263](https://github.com/acgetchell/causal-triangulations/pull/263)
  [`46bfb02`](https://github.com/acgetchell/causal-triangulations/commit/46bfb02157626b6406b781d6acaf1eb767a2c305)

- Bump astral-sh/setup-uv in the github-actions group [#266](https://github.com/acgetchell/causal-triangulations/pull/266)
  [`1d3b4a2`](https://github.com/acgetchell/causal-triangulations/commit/1d3b4a289dc826514f84a84342a85632263e3f77)

### Documentation

- Clarify saturated proposal telemetry deserialization
  [`e5a3efb`](https://github.com/acgetchell/causal-triangulations/commit/e5a3efb7adb25c53e7c5e11a325ded64d8ad2f77)

  - Document that ProposalStatistics deserialization only relaxes exact terminal-outcome partitioning after saturated telemetry merges when move-family
    proposals also saturated.
- Add physicist quickstart and streamline guides
  [`34b86f9`](https://github.com/acgetchell/causal-triangulations/commit/34b86f995cb5ef128e87a62d21ef9fd6a923b54b)

  - Add a non-programmer quickstart for running 1+1 CDT simulations, interpreting CSV/JSON output, and tuning the main physics knobs.
  - Refocus README around current 1+1 scope, grand-canonical volume behavior, the crate ecosystem, and reviewer-facing documentation entry points.
  - Streamline CLI, script, contributing, benchmark, and performance docs so each guide has a distinct maintenance role.
  - Document how Delaunay bistellar k-flips map onto foliation-preserving CDT/Pachner moves, including the planned interface revisit after delaunay#252.
  - Align Markdown and spelling tool pins with sibling repositories.
- Clarify Rust validation command composition [`50dc67f`](https://github.com/acgetchell/causal-triangulations/commit/50dc67f13aed0d54bff2394b8506c9137cbcef64)

### Fixed

- [**breaking**] Propagate checkpoint result invariant failures
  [`faf5ece`](https://github.com/acgetchell/causal-triangulations/commit/faf5ecedf8f80566c6241b952edf2f2ca944fa97)

  - Return CdtResult from CdtMcmcCheckpoint::into_results so result snapshot invariant failures propagate instead of panicking.
  - Preserve saturated move and proposal telemetry, and report concrete scalar trace outcomes during resume validation.
  - Avoid same-process staged output temp-file collisions and keep toroidal removal selection limited to sampleable inverse sites.
  - Align the Zenodo DOI badge and cover empty observable/profile boundaries.
- Harden checkpoint and adjacency invariants [`1795533`](https://github.com/acgetchell/causal-triangulations/commit/17955336be720cd3533db644af287f10e252db71)

  - Recompute resumed checkpoint actions into live state after tolerant validation so telemetry starts from the canonical action value.
  - Reject saturated or overflowed proposal-terminal counters unless the selected move-family counter saturated too.
  - Rotate triangulation instance epochs when modification counters saturate so proposal-site caches cannot collide with stale versions.
  - Cache Delaunay vertex-adjacency indexes between topology mutations while keeping mutation rollback and serialization cache boundaries explicit.
  - Add fallible MetropolisConfig runtime validation for runner entry points.
- Harden documentation review follow-ups [`6c4ed19`](https://github.com/acgetchell/causal-triangulations/commit/6c4ed191f54c9a0b5c045aafbbf54ceecb1fcabe)

  - Make pinned-tool version checks tolerate missing or nonstandard version output.
  - Record the Cargo manifest description rationale in the tooling-alignment notes.
  - Clarify the pinned Rust toolchain in contributor setup guidance.
  - Use an explicit performance baseline path in the compare example.
  - Have the parameter-sweep script report success from status marker files instead of grepping logs.
  - Regenerate the changelog from the repository pipeline.
- Harden open-boundary validation follow-ups [`ea8b781`](https://github.com/acgetchell/causal-triangulations/commit/ea8b781393c4da17d534185d7502c845a326d224)

  - Centralize post-mutation topology and foliation rejection policy on typed CDT errors.
  - Avoid truncating summary JSON outputs before final triangulation mesh summaries are built.
  - Speed up open-slab crossing detection while preserving missing-position and incident-edge handling.
  - Share open-strip coordinate generation between regular and profiled constructors.
  - Strengthen spacetime-coordinate docs and constructor coverage for notebook mesh consumers.
  - Align Codecov thresholds with the stricter Delaunay policy and document the rationale.
  - Keep notebook time-label parsing tolerant of numeric labels while rejecting nonnumeric values.
- Harden summary mesh export handling [#194](https://github.com/acgetchell/causal-triangulations/pull/194)
  [`aed1c2c`](https://github.com/acgetchell/causal-triangulations/commit/aed1c2c9ee7d5a2db20ff26f825cccddb7889eab)

  - Canonicalize summary mesh triangle indices so JSON exports stay deterministic across equivalent face orderings.
  - Preserve OutputWriteFailed when mesh summary construction fails during write_summary_json.
  - Reject non-integral notebook time labels and run notebook lint with notebook dependencies.
- Reject incompatible legacy toroidal mode [#219](https://github.com/acgetchell/causal-triangulations/pull/219)
  [`80e241e`](https://github.com/acgetchell/causal-triangulations/commit/80e241e586e2843b85146d526a0630abaef6f371)

  - Prevent legacy `Canonicalized` payloads from silently adopting different periodic-image semantics.
  - Preserve deserialization for supported `PeriodicImagePoint` and `Explicit` modes.
- Override Semgrep's vulnerable MCP dependency [`c018216`](https://github.com/acgetchell/causal-triangulations/commit/c0182167eec47b7c9d781382d897b5f32c04dec0)

  - Pin the transitive MCP SDK to patched version 1.28.1.
  - Document the temporary override and its removal condition.
- Refresh dependencies and harden validation [`634c427`](https://github.com/acgetchell/causal-triangulations/commit/634c4278d8a94807219d09b784ae7b8f52bb6b53)

  - Update markov-chain-monte-carlo to 0.4.1 and adopt its invariant-preserving Step telemetry accessors.
  - Refresh repository tool pins, fail fast on unresolved workflow versions, and separate staggered Dependabot security updates.
  - Reject malformed notebook metadata and non-integer nbformat values at the parser boundary.
  - Consolidate benchmark setup failures behind the shared postfix OrAbort helper and replace unreliable README badges.
- Propagate notebook discovery failures [`c3cd8d4`](https://github.com/acgetchell/causal-triangulations/commit/c3cd8d447703c3c31f515993ad3c2a428233ff2e)

  - Capture notebook discovery before linting so find failures remain fatal while successful empty searches stay explicit.
  - Document that direct pinned typos-cli installation requires just to resolve the configured version.
- Preserve mock topology during vertex removal [#225](https://github.com/acgetchell/causal-triangulations/pull/225)
  [`9634f98`](https://github.com/acgetchell/causal-triangulations/commit/9634f982ca4d106c12a0c7059496dabc2bb7027a)

  - Reject non-isolated mock vertices before deleting their storage.
  - Report incident edge and face counts through typed error context.
  - Keep rejected removals failure-atomic while retaining isolated removal.
- [**breaking**] Enforce exact move feasibility and Python 3.14 tooling
  [`68e4131`](https://github.com/acgetchell/causal-triangulations/commit/68e4131f89cc7eccbc4f2e7767fd6c9aa473e24a)

  - Exclude deterministically infeasible proposal sites using immutable Delaunay k=1 and k=2 preflights.
  - Extend geometry backends with exact subdivision and collapse feasibility checks while preserving CDT self-loop semantics.
  - Require Python 3.14, refresh uv-managed dependencies, and pin uv 0.12.2 and rumdl 0.2.52.
- Harden runtime typing and collapse preflights [`481412f`](https://github.com/acgetchell/causal-triangulations/commit/481412fcd61166d8d8326af789d314043bb5a424)

  - Keep argparse subparser generics static-only so benchmark CLI annotations remain runtime-safe.
  - Reject mock vertex collapses until exact inverse k=1 removal is modeled.
- [**breaking**] Harden proposal accounting and restored state
  [`bed1d8d`](https://github.com/acgetchell/causal-triangulations/commit/bed1d8df9646d2a209ba1786129968723e49682f)

  - Allocate quantized family mass only across supported move families and preserve path-conditioned Hastings probabilities.
  - Reconstruct persisted trajectories and reject actions, counts, outcomes, or volume profiles that disagree with restored geometry.
  - Lint every Cargo target in local before-push CI to reproduce GitHub security findings.
  - Narrow backend and policy bounds while keeping public preludes focused and orthogonal.
- [**breaking**] Enforce simulation invariants end to end [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
  [`da7dfe5`](https://github.com/acgetchell/causal-triangulations/commit/da7dfe55cb42e620a4dac6a99ecd197e3122cb86)

  - Make foliated volume moves rollback-safe and exactly reversible across open and toroidal geometries while delegating structural validation to Delaunay.
  - Validate bounded action and temperature arithmetic, checkpoint telemetry, resumed suffixes, and atomic multi-output publication.
  - Distinguish spatial-vertex initialization from slab-triangle measurements, expose finite-graph observable curves, and gate allocation regressions.
- [**breaking**] Preserve exact layered CDT states
  [`2ebd058`](https://github.com/acgetchell/causal-triangulations/commit/2ebd058102338daca2667a31081e5d8a2e89cd53)

  - Build open strips at exact integer time coordinates with Level 1–4 validated staircase connectivity.
  - Restore evolved and exact non-Delaunay checkpoints without requiring the optional Level 5 predicate.
  - Honor PROPTEST_CASES while retaining the eight-case local default.
  - Make changelog publication recoverable and reject invalid notebook numeric inputs.
  - Refresh rumdl, Ruff, Ty, setup-uv, and uv-managed packages.
  - Correct foundational CDT and release citation metadata.
- Harden changelog publication and numeric parsing
  [`2e583da`](https://github.com/acgetchell/causal-triangulations/commit/2e583da2e6284650b359a7658179c5ad61372153)

  - Reject overflowing analysis values through the finite-number error contract.
  - Fsync changelog replacement directories and retain actionable rollback recovery mappings.
- Harden Python release and fixture helpers [`b094bbf`](https://github.com/acgetchell/causal-triangulations/commit/b094bbf9346561705a2ea582bef34363d15a2909)

  - Reject mismatched tag versions, invalid release dates, and malformed or duplicate changelog headings before mutation.
  - Publish changelog outputs transactionally while preserving rollback diagnostics.
  - Match Semgrep fixture findings by rule and source span and reject malformed result payloads.
  - Bound shared subprocesses and make project-root discovery configurable.
  - Synchronize citation and support-package metadata through the release validation gate.
- Tighten release and fixture validation [#261](https://github.com/acgetchell/causal-triangulations/pull/261)
  [`fdafda6`](https://github.com/acgetchell/causal-triangulations/commit/fdafda6e1a946260a6db12ed5174a7840f70210b)

  - Require Python package versions to match authoritative Cargo metadata.
  - Prefer the shortest matching span for overlapping Semgrep findings.
- Make certified mutations failure-atomic [`f1802c5`](https://github.com/acgetchell/causal-triangulations/commit/f1802c53986d34a86617de02be7c4613376df880)

  - Report pre-existing Level 5 rejection separately from coordinate insertion failures.
  - Publish insertion and generic deletion candidates only after Level 4 validation.
  - Attribute mesh exports to this crate and consolidate explicit-builder diagnostics.
- Align release and security validation [`47b8951`](https://github.com/acgetchell/causal-triangulations/commit/47b895167f812b8bac98837648a06f4c611df918)

  - Pin local and SARIF zizmor scans to the same version and enable authenticated online audits.
  - Harden release workflows with bounded jobs, explicit cache policy, and Semgrep enforcement.
  - Preserve comments around citation identifiers and reject ambiguous metadata blocks during version updates.
  - Clarify release placeholders and the intentional algebraic-float fixture exception.
- Enforce exact zizmor scanner parity [`84eb4f9`](https://github.com/acgetchell/causal-triangulations/commit/84eb4f9357787b8cb70b8ad047e41000a18ebba6)

  - Require zizmor-action workflows to use the repository's 1.30.0 scanner pin.
  - Refresh git-cliff and uv pins with compatible Rust and Python lockfile updates.
- Preflight stable uv updates [`12cfc80`](https://github.com/acgetchell/causal-triangulations/commit/12cfc80a92cfa55d732c27e2023bf58563ce5774)

  - Reject malformed or unstable uv versions before dependency and Cargo tool mutations.
  - Accept newer stable releases so atomic tool-pin reconciliation can advance the repository pin.
- Preserve evolved checkpoint states [#253](https://github.com/acgetchell/causal-triangulations/pull/253)
  [`b97c5f2`](https://github.com/acgetchell/causal-triangulations/commit/b97c5f2d59aae89a98c5b027e03fa661d7c28bc9)

  - Restore topology-aware Levels 1–4 state without Level 5 repair or retriangulation.
  - Preserve periodic realizations, exact connectivity, adjacency, and payloads.
  - Reject invalid restored realizations before publishing checkpoint state.
  - Refresh the libredox dependency and typos tool pin.
- Reject inconsistent checkpoint slice metadata [`f391535`](https://github.com/acgetchell/causal-triangulations/commit/f3915353316df9c18e940473b77446ad219c8639)

  - Reject version 1 checkpoints when foliation and triangulation slice counts disagree.
  - Preserve operation-specific diagnostics across CDT and Delaunay checkpoint translation.

### Maintenance

- [**breaking**] Align dependencies and repository automation [#216](https://github.com/acgetchell/causal-triangulations/pull/216)
  [`50e74d8`](https://github.com/acgetchell/causal-triangulations/commit/50e74d8a3a0a8ef5a6698207f3ef5bccb82693c6)

  - Automate Dependabot squash merges after CodeRabbit approves the exact pull-request head and all required checks pass.
  - Centralize development-tool pins in the justfile and align workflows, actions, and dependency grouping with sibling repositories.
  - Upgrade Rust and Python dependencies, including Delaunay 0.8, and adapt CDT geometry interfaces accordingly.
  - Preserve failed edge mutations atomically, report toroidal construction retries accurately, and document version-bound checkpoints.
- Standardize orthogonal validation buckets [`37a4653`](https://github.com/acgetchell/causal-triangulations/commit/37a4653c2519d4aaefd4f279f9aabc5eb16f1b08)

  - Flatten CI into focused validators with one release-profile Rust test bucket.
  - Keep doctests and optional all-target Clippy checks independently selectable.
  - Reuse the notebook checker for discovery, output hygiene, and multi-file diagnostics.
  - Document focused validation workflows and fast versus slow notebook execution.
- [**breaking**] Refresh Rust, backends, and update tooling
  [`fe7745d`](https://github.com/acgetchell/causal-triangulations/commit/fe7745d2d4dabbd8086a975dd6476c9b7c505f1d)

  - Raise the MSRV to Rust 1.98.0 and update the Delaunay and MCMC integrations.
  - Adopt Delaunay's Level 1–4 mutable owner while preserving checkpoint and visualization compatibility.
  - Add transactional dependency and managed-tool upgrades through `just update`.
  - Align workflows and Python tooling with the refreshed validator versions.
  - Track temporary Delaunay compatibility work in #268 pending acgetchell/delaunay#591.

### Performance

- [**breaking**] Replace owned snapshots with borrowed views [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
  [`37805b5`](https://github.com/acgetchell/causal-triangulations/commit/37805b5d0a84378004831610a4640be0b7ee59b7)

  - Return borrowed coordinates and slices plus lazy topology, adjacency, foliation, and history iterators.
  - Scope hashable handles to their backend owner and topology generation, rejecting foreign or stale capabilities with typed errors.
  - Remove duplicate simulation history and topology cloning while making proposal and backend rollback boundaries explicit.
- Bound volume-move seed discovery [#222](https://github.com/acgetchell/causal-triangulations/pull/222)
  [`2e3f47d`](https://github.com/acgetchell/causal-triangulations/commit/2e3f47d25b045e5929ec62f25411f653562cde00)

  - Limit deterministic seed probing to a small fixture budget.
  - Abort with explicit context for unsupported move families.

## [0.1.0] - 2026-06-02

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
- Encode trace and count invariants [#164](https://github.com/acgetchell/causal-triangulations/pull/164)
- Enforce trace profile invariants [#164](https://github.com/acgetchell/causal-triangulations/pull/164)
- Enforce CDT and backend invariants
- Harden CDT telemetry and profiled initialization
- Reject volume-profile override overflows
- Make Metropolis step telemetry nonzero [#153](https://github.com/acgetchell/causal-triangulations/pull/153)
- Align tooling and CDT error APIs
- Align repository tooling with MCMC

### Merged Pull Requests

- Enforce simulation state invariants [#174](https://github.com/acgetchell/causal-triangulations/pull/174)
- Encode trace and count invariants [#164](https://github.com/acgetchell/causal-triangulations/pull/164)
- Enforce trace profile invariants [#164](https://github.com/acgetchell/causal-triangulations/pull/164)
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

- [**breaking**] Encode trace and count invariants [#164](https://github.com/acgetchell/causal-triangulations/pull/164)
  [`d843d16`](https://github.com/acgetchell/causal-triangulations/commit/d843d165d0392ad1e179e3d43a7984491faa5a18)

  - Represent constructed CDT simplex counts with invariant-bearing nonzero types while keeping raw backend geometry counts as usize.
  - Replace public measurement and Metropolis step fields with typed accessors and outcome-specific telemetry payloads.
  - Export simulation diagnostics through upstream scalar trace CSV rows instead of legacy measurement CSV rows.
  - Remove saturating telemetry downcasts in favor of checked conversions and structured count/trace error variants.
  - Update docs, examples, preludes, and tooling guidance for the new trace and count contracts.

- [**breaking**] Enforce trace profile invariants [#164](https://github.com/acgetchell/causal-triangulations/pull/164)
  [`f481bc2`](https://github.com/acgetchell/causal-triangulations/commit/f481bc2bdf9d1ad727a90b12e4c35f67a7c8bf9c)

  - Reject measurement and scalar trace volume profiles whose totals exceed the stored triangle count.
  - Preserve checkpoint seed metadata across resumed scalar trace rows and reject mismatched trace seeds during validation.
  - Clarify that CSV output carries trace rows while JSON output carries summary metadata and final triangulation data.

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
- Finalize v0.1.0 validation [`31a4cff`](https://github.com/acgetchell/causal-triangulations/commit/31a4cff63adf5598b517530d69dc7568db5dad50)

  - Store resumable checkpoint actions behind a finite-action boundary before exposing checkpoint state.
  - Reject non-finite and mismatched checkpoint actions during checkpoint construction and deserialization.
  - Run slow release validation through the perf profile with a bounded large-scale debug harness.
  - Align v0.1.0 release-readiness docs with upstream-backed Metropolis continuation and completed validation gates.

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

## Archives

Older releases are archived by minor series:

- [0.0.x](docs/archive/changelog/0.0.md)

[0.1.1]: https://github.com/acgetchell/causal-triangulations/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/acgetchell/causal-triangulations/compare/v0.0.1...v0.1.0
