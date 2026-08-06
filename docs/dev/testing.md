# Testing Guidelines

Testing rules for the causal-triangulations library.

Agents must follow these expectations when adding or modifying code.

---

## Testing Philosophy

This project is a **Causal Dynamical Triangulations library** for quantum gravity simulations.

Tests should verify:

- mathematical correctness (Regge action, coupling constants)
- geometric invariants (Delaunay property, triangulation validity)
- topological consistency (vertex/edge/face counts, Euler characteristic)
- Monte Carlo algorithm stability (ergodic moves, Metropolis acceptance)

When possible, prefer **property-based testing** over single-case tests.

Tests should focus on validating invariants rather than merely executing code.

---

## Test Types

### Unit Tests

Location:

```text
src/**
```

Defined inline using:

```rust
#[cfg(test)]
mod tests {
```

Unit tests validate:

- small internal algorithms
- helper utilities
- invariants within modules

They should be small, deterministic, and fast.

---

### Integration Tests

Location:

```text
tests/
```

Integration tests compile as **separate crates** and test the public API.

Each integration test crate should include a crate-level documentation comment:

```rust
//! Integration tests for CDT simulation.
```

This satisfies `clippy::missing_docs` in CI.

Integration tests should validate:

- full simulation construction
- public API behavior
- cross-module interactions (geometry ↔ CDT)

---

### Regression Tests

Location:

```text
tests/regressions.rs
```

Regression tests capture specific previously observed bugs or blocking upstream limitations. Each regression test should document:

- the issue, blocker, or failure mode it guards
- the user-visible symptom that exposed the bug
- how expectations should change when the underlying fix lands

---

### Python Tests

Location:

```text
scripts/tests/
```

Python tests use **pytest** (never unittest). Run via:

```bash
just test-python
```

All Python tests should:

- use type hints
- include `-> None` return annotations on test functions

### Rust Test Runner

Runnable Rust tests use `cargo nextest` through the repository `just` recipes. Rustdoc doctests remain on `cargo test --doc` because nextest does not execute
doctests. Use the `just` recipes instead of calling Cargo directly so this split stays consistent locally and in CI.

---

## Floating-Point Comparisons

Never compare floating-point values using `assert_eq!`.

Use the **approx** crate for tolerant comparisons:

```rust
use approx::assert_relative_eq;

assert_relative_eq!(a, b, epsilon = 1e-12);
```

---

## Deterministic Randomness

Tests must be deterministic.

If randomness is required, use a seeded RNG:

```rust
use rand::{SeedableRng, rngs::StdRng};

let rng = StdRng::seed_from_u64(1234);
```

Do **not** use `thread_rng()`. Deterministic seeds allow failures to be reproduced.

---

## Error Handling in Tests

Tests may freely use `unwrap()` or `expect()` when a failure should cause the test to fail immediately.

Explicit error handling is usually unnecessary in tests unless the test is specifically verifying error behavior.

---

## Test Commands

Run standard tests:

```bash
just test
```

This composes the focused library-unit and rustdoc-doctest buckets. Run only library unit tests with:

```bash
just test-unit
```

Run integration tests:

```bash
just test-integration
```

Run feature-gated slow integration tests:

```bash
just test-slow
```

Slow tests run through the repository `perf` Cargo profile and set a wall-clock cap for large-scale debug runs. This keeps stress probes aligned with
release/debug recipes and prevents default test-profile runtime from dominating slow validation.

Run all tests:

```bash
just test-all
```

`test-all` uses `test-rust` for broad Rust correctness and then runs the Python support-script suite. Broad runnable Rust tests use one release-profile nextest
pass for library unit tests plus integration-test crates; rustdoc doctests remain separate:

```bash
just test-rust-ci
just test-doc
```

Run Python tests:

```bash
just test-python
```

---

## Documentation Tests

Public documentation examples must compile and pass.

Validate with:

```bash
just test-doc
```

---

## Performance-Sensitive Tests

Tests should remain fast.

Avoid:

- extremely large random inputs
- quadratic or worse scaling test loops
- heavy allocations

Large-scale performance validation belongs in **benchmarks**, not tests.

---

## CI Expectations

All tests must pass under CI.

Before proposing patches agents should run:

```bash
just ci
```

The `ci` recipe directly composes the GitHub-equivalent leaf validators instead of depending on `check`, `lint`, or `test-all`. It enforces:

- **repository and configuration checks**: GitHub Actions, Markdown, spelling, JSON, TOML, YAML/CFF, Python, shell, and repository-owned Semgrep rules
- **core Rust checks**: formatting, production library/binary Clippy, and documentation builds
- **Rust correctness**: library unit tests and integration-test crates in one release-profile `test-rust-ci` nextest pass, plus separate rustdoc doctests
- **Python correctness**: pytest over support scripts, including the reusable notebook checker
- **notebooks**: source notebooks are output-clean, extracted code passes Ruff and ty, and the fast notebook set executes headlessly through the uv-managed
  environment
- **benchmarks and examples**: benchmark harnesses compile without warnings, and Cargo examples build once before running with stable output markers

For non-core changes, run the smallest relevant test or integration crate first and compose each focused final bucket once. For core Rust changes or exact
GitHub-equivalent evidence, run `just ci` directly rather than pre-running broad bundles that select the same tests.

---

## Preferred Test Style

Tests should be:

- deterministic
- focused
- invariant-driven
- easy to reproduce

Avoid large monolithic tests or tests that do not verify correctness.
