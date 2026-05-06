# Rust Development Guidelines

Rust coding conventions for this repository.

Agents must follow these rules when modifying or adding Rust code.

---

## Core Principles

This project is a **Causal Dynamical Triangulations library** built on the `delaunay` crate for geometry.

Key goals:

- Correctness
- Predictable performance
- API stability
- Zero unsafe code

All design decisions should prioritize these goals.

---

## Safety

Unsafe Rust is forbidden.

The crate enforces:

```rust
#![forbid(unsafe_code)]
```

Agents must never introduce:

- `unsafe`
- `unsafe fn`
- `unsafe impl`
- `unsafe` blocks

---

## Borrowing and Ownership

Prefer **borrowing APIs** whenever possible.

### Function arguments

Prefer:

```rust
fn foo(points: &[Point<D>])
```

Instead of:

```rust
fn foo(points: Vec<Point<D>>)
```

### Return values

Prefer borrowed results:

```rust
fn vertex(&self, key: VertexKey) -> Option<&Vertex<D>>
```

Avoid unnecessary allocations and cloning in public APIs. Prefer returning references or iterators over internal data instead of cloning structures.

Only return owned values (`Vec`, `String`, etc.) when necessary.

Do not expose broad mutable access to invariant-heavy CDT wrappers. Prefer narrow mutation methods that perform one operation, invalidate derived caches/bookkeeping, and return a typed `Result`. Tests that need invalid legacy states should use local helpers inside the test module rather than test-only constructors or methods on production impl blocks.

---

## Error Handling

Public APIs must **not panic**.

Use explicit error propagation.

Production `src/` code must not use bare `unwrap()` or explicit `panic!`. Use `?`, typed errors, `Option`, or an intentional fallback instead. Tests, doctests, examples, and benchmark setup may fail fast when a broken fixture should stop execution immediately; prefer `expect("reason")` over bare `unwrap()` in user-facing examples so failures remain diagnosable.

### Fallible public functions

Return `Result`:

```rust
pub fn insert_vertex(...) -> Result<VertexKey, CdtError>
```

### Lookup functions

Return `Option`:

```rust
pub fn vertex(&self, key: VertexKey) -> Option<&Vertex<D>>
```

### Infallible APIs

Infallible functions **must not return `Result`**.

Examples:

- `len()`
- `is_empty()`
- iterators
- accessors
- builder setters

---

## Panic Policy

Panics should be avoided in library code.

Acceptable panic situations:

- internal invariants violated
- unreachable logic errors
- debugging assertions

Prefer returning `Result` or `Option` instead of panicking.

---

## Error Types

Errors should be defined **within the module where they are used**.

Avoid large centralized error enums.

Error variants must be narrow, orthogonal, and purpose-specific. Do not collapse distinct failure modes into one catch-all variant when callers, tests, or debugging would need to distinguish them. Prefer a new variant over stringly typed detail when the distinction is part of the recovery or diagnostic path.

Each variant should carry the structured context needed to debug the failure without parsing the `Display` string:

- the operation being attempted, when the same error can occur from multiple operations
- the relevant handle, key, index, coordinate, or configuration field
- the expected invariant and the actual value when reporting validation failures
- the source error as a typed source when possible, or as a string only at crate/backend boundaries where the upstream type is not part of this crate's public contract

Keep error layers orthogonal. Invalid input or handles, unsupported operations, topology/causality violations, backend mutation failures, and internal postcondition failures should use different variants. Wrapping is appropriate only when crossing abstraction layers, and wrappers must preserve the lower-level detail.

Public error enums must be `#[non_exhaustive]` so new variants remain additive.

Example:

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InsertError {
    #[error("duplicate vertex at index {index}")]
    DuplicateVertex { index: usize },
}
```

---

## Imports

Always import types at the top of the module rather than using fully‑qualified paths inline.

Group imports from the same module into a single `use` statement with braces.

If a test module already has `use super::*;`, do not re‑import items that are already brought into scope by the parent module's imports.

---

## Module Layout

Never use `mod.rs`.

Modules are declared from `src/lib.rs` (and `src/main.rs` for binaries), including nested modules via inline `pub mod foo { pub mod bar; }` when needed.

---

## Documentation

All public items must have documentation.

Public functions and methods should include a `# Examples` section with a runnable doctest demonstrating basic usage. Doctests serve as both documentation and regression tests.

Private helper functions should have a `///` doc comment that explains why the helper exists, especially when it encodes an invariant, isolates validation, or keeps a mutation path consistent.

After Rust changes, verify documentation builds:

```bash
just doc-check
```

---

## Re-exports

Types that are part of the crate's **stable public API** (documented, intended for external consumption) should be re-exported from the crate root (`src/lib.rs`). Internal-use public types (e.g., backend-specific handles) should not be re-exported to avoid API bloat.

When adding a new public API type, add a corresponding `pub use` line in the re-export block at the top of `lib.rs`.

The broad `prelude::*` must stay small. It should cover common quick-start workflows such as CDT construction, basic configuration, simulation startup, query traits, and error handling. Do not use it as a dumping ground for every public type.

Focused preludes under `prelude::` must remain small, orthogonal, and purpose-specific. Use them in doctests, integration tests, examples, and benchmarks instead of deep module paths when demonstrating public workflows. Avoid duplicating specialized APIs across scoped preludes unless the overlap is deliberate and documented:

- `prelude::geometry` for backend construction, geometry generators, and geometry traits
- `prelude::triangulation` for CDT wrappers, foliation classification, topology metadata, and triangulation queries
- `prelude::moves` for local ergodic move kernels, move results, move types, and move statistics
- `prelude::action` for standalone action configuration and Regge action calculations
- `prelude::simulation` for Metropolis/action simulation workflows, proposal types, simulation result types, and telemetry needed to inspect or debug simulations
- `prelude::observables` for user-facing analysis APIs that measure triangulations or derived physical observables, such as volume profiles, Hausdorff-dimension estimators, and spectral-dimension estimators

Keep the simulation and observables boundaries crisp:

- Export user-facing observable estimators from `prelude::observables`, not `prelude::simulation`.
- Keep simulation telemetry, proposal adapters, and result containers in `prelude::simulation`; do not export them from `prelude::observables` merely because an observable can be computed from them.
- Examples and doctests for measurements should prefer `prelude::observables::*`; examples and doctests for running MCMC should prefer `prelude::simulation::*`.

---

## Integration Tests

Integration tests live in:

```text
tests/
```

Each integration test crate should include a crate‑level doc comment:

```rust
//! Integration tests for CDT simulation.
```

This satisfies `clippy::missing_docs` in CI.

---

## Logging and Diagnostics

Use `log` for runtime diagnostics. **Never use `eprintln!`** or `println!` for debug output in library code.

---

## Lint Suppression

When suppressing a lint, use `#[expect(...)]` instead of `#[allow(...)]`.

`expect` causes a compiler warning if the lint is no longer triggered, ensuring suppressions are removed when they become unnecessary.

Always include a `reason`:

```rust
#[expect(clippy::too_many_lines, reason = "test covers multiple cases")]
fn test_large_dataset_performance() { ... }
```

---

## Performance

Avoid unnecessary allocations.

Prefer:

- iterators
- slices
- stack arrays `[T; D]`
- fixed‑size containers

Avoid cloning large structures unless necessary.

---

## External Dependencies

Dependencies should be minimal.

Before adding a dependency, consider:

1. compile time impact
2. MSRV compatibility
3. maintenance status
4. dependency tree size

---

## Geometry Backend Isolation

`src/geometry/` is the backend interface layer. It is responsible for wrapping the upstream `delaunay` crate behind this crate's traits, opaque handles, generators, and backend adapters. `src/cdt/` is the CDT domain layer: it owns foliation, topology, causality, moves, action, simulation, results, and observables.

Direct `use delaunay::` imports are **restricted** to the `src/geometry/` subtree:

- `src/geometry/backends/delaunay.rs` — wraps `delaunay` crate types behind trait-based handles
- `src/geometry/generators.rs` — Delaunay triangulation generators (`generate_delaunay2`, `build_delaunay2_with_data`)

No module outside `src/geometry/` may import from the `delaunay` crate directly. Instead use:

- The `DelaunayBackend2D` type alias (defined in `src/lib.rs` geometry module)
- Handle types from `crate::geometry::backends::delaunay` (`DelaunayVertexHandle`, `DelaunayEdgeHandle`, `DelaunayFaceHandle`)
- Trait methods from `TriangulationQuery` / `TriangulationMut`
- Generator utilities from `crate::geometry::generators`

This ensures the `delaunay` crate can be upgraded or replaced without touching CDT logic.

---

## Formatting and Lints

Code must pass:

```bash
cargo fmt
cargo clippy
```

Typically run via:

```bash
just fix
just check
```

CI treats warnings as errors.

---

## API Stability

Agents must avoid:

- breaking public APIs
- renaming public types
- removing public functions

If an API change is necessary, prefer:

```rust
#[deprecated]
```

with migration guidance.

---

## Preferred Patch Style

When modifying Rust code:

- make **small focused changes**
- avoid large refactors
- maintain existing naming conventions
- preserve module boundaries
