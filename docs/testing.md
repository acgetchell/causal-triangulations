# Testing Overview

This document summarizes the repository's current test coverage and the main gaps to keep in mind when adding features. Command details and CI expectations live
in `docs/dev/testing.md` and `docs/dev/DEVELOPING.md`.

## Current Coverage

- **Library unit tests**: `cargo test --lib` currently runs the core Rust test suite across CDT, geometry, configuration, and utility modules.
- **Integration tests**: `tests/integration_tests.rs`, `tests/cli.rs`, `tests/proposal_policy.rs`, `tests/proptest_foliation.rs`,
  `tests/proptest_metropolis.rs`, and `tests/proptest_config.rs` cover public workflows, CLI validation, proposal endpoint probabilities, foliation
  invariants, Metropolis scoring, and property-test configuration precedence.
- **Python tooling tests**: `tests/tooling/` covers the installed shared tooling and CDT configuration, including benchmark, example, notebook, and
  release contracts.
- **Documentation tests**: public doctests run through `just test-doc` and as part of the broader CI path.
- **Examples and benchmark compilation**: `just ci` compiles benchmarks and runs all example programs.

The issue #105 toroidal regression is covered by `tests/integration_tests.rs::test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology`, which
runs a seeded S¹×S¹ Metropolis simulation, requires at least one accepted periodic move, and verifies topology, foliation, causality, simplex classification,
and χ = 0 at the end.

Open-boundary simulation entrypoints are covered by crate-root tests that require `run_simulation()` to build a foliated regular CDT strip, preserve
adjacent-slice causality, classify simplices, and report a non-empty slab-triangle profile. Configuration tests also reject open-boundary totals that cannot be
split into equal-size spatial slices.

The v0.1.1 capstone evidence includes focused spatial- and temporal-seam applications for both k=2 identifiers, exact `(1,3)`/`(3,1)` seam-local round trips,
full action recomputation for every successful move family under ordinary and extreme finite couplings, serialization-equivalent rollback evidence including
derived profiles and action, and family/site score algebra for uniform, unequal fixed, and state-dependent family policies. The original algebra checks
assumed matching return atoms.

The current suite replays the [#316 counterexample](scientific-basis.md#move-and-sampler-contract) and requires zero embedded return probability. Fixed-mixture
tests sum the two policy components `Move22` and `EdgeFlip` when they realize the same edit. Intrinsic cyclic-word tests independently enumerate small-torus
transition rows and detailed-balance fluxes. The Hilbert-order regression compares family order after exact geometry reconstruction and reversed site
discovery. The [statistical ensemble gate](roadmap.md#v020-analytic-acceptance-criteria) remains separate.

## Property-Test Case Controls

The expensive foliation and Metropolis property suites use eight generated cases during ordinary local runs. Set Proptest's standard `PROPTEST_CASES`
environment variable to raise or lower both suites for CI, stress runs, or reproduction:

```bash
PROPTEST_CASES=64 just test-integration
```

The repository helper only supplies the eight-case fallback when `PROPTEST_CASES` is absent. Proptest remains responsible for parsing valid and invalid values
and for every other configuration default.

## Remaining Gaps

### Ergodic Move Sampling

- Exercise more negative cases where a geometrically editable site must be rejected because it would break CDT invariants.

### Geometry Trait Operations

The Delaunay and mock backends have direct tests for mutation and query paths, but additional integration tests would be useful around trait-level contracts:

- `flip_edge()`
- `subdivide_face()`
- `remove_vertex()`
- edge and face adjacency after mutation
- cache invalidation and topology metadata after repeated edits

### Validation and Error Paths

- Expand tests for invalid parameter ranges, overflow boundaries, and generation failures with specific error contexts.
- Keep adding regression tests for distinct `CdtError` variants rather than relying on string matching.
- Exercise more stale-foliation and live-label mismatch paths after backend mutation.

### Metropolis Edge Cases

- Zero-step and measurement-boundary schedules.
- Long seeded runs that assert aggregate behavior without becoming slow or brittle.

### Documentation and Examples

- Continue adding runnable doctests for public APIs that expose stable workflows.
- Add tutorial examples for open-boundary strips, toroidal runs, and acceptance-rate interpretation.
- Keep `docs/roadmap.md` high level; concrete test gaps should be tracked as GitHub issues when they become actionable.

## Coverage Workflow

Generate coverage data with the repository recipes:

```bash
just coverage-ci
just coverage-report
```

For an HTML report:

```bash
just coverage
```

The HTML output is written to `target/llvm-cov/html/index.html`.
