# Testing Overview

This document summarizes the repository's current test coverage and the main gaps to keep in mind when adding features. Command details and CI expectations live in `docs/dev/testing.md` and `docs/dev/commands.md`.

## Current Coverage

- **Library unit tests**: `cargo test --lib` currently runs the core Rust test suite across CDT, geometry, configuration, and utility modules.
- **Integration tests**: `tests/integration_tests.rs`, `tests/cli.rs`, `tests/proptest_foliation.rs`, and `tests/proptest_metropolis.rs` cover public workflows, CLI validation, foliation invariants, and Metropolis scoring.
- **Python support-script tests**: `scripts/tests/` covers benchmark, changelog, coverage, hardware, tag, and subprocess utilities.
- **Documentation tests**: public doctests run through `just test-doc` and as part of the broader CI path.
- **Examples and benchmark compilation**: `just ci` compiles benchmarks and runs all example programs.

The issue #105 toroidal regression is covered by `tests/integration_tests.rs::test_toroidal_metropolis_preserves_topology_after_many_accepted_moves`, which runs a seeded S¹×S¹ Metropolis simulation, requires at least 100 accepted moves, and verifies topology, foliation, causality, cell classification, and χ = 0 at the end.

Open-boundary simulation entrypoints are covered by crate-root tests that require `run_simulation()` to build a foliated regular CDT strip, preserve adjacent-slice causality, classify cells, and report a non-empty volume profile. Configuration tests also reject open-boundary totals that cannot be split into equal-size spatial slices.

## Remaining Gaps

### Ergodic Move Sampling

- Weight move-type selection by available application sites instead of sampling move types uniformly.
- Weight or enumerate accepted move-site retries so proposals bind to concrete local moves before Metropolis acceptance.
- Add focused per-kernel toroidal fixtures for spatial and temporal wrap-around cells.
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

- Temperature extremes near zero and very large values.
- Zero-step and measurement-boundary schedules.
- Long seeded runs that assert aggregate behavior without becoming slow or brittle.
- Additional checks that rollback leaves action and simplex counts unchanged after failed accepted applications.

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
