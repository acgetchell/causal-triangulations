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

---

## Error Handling

Public APIs must **not panic**.

Use explicit error propagation.

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

Example:

```rust
#[derive(Debug, thiserror::Error)]
pub enum InsertError {
    #[error("duplicate vertex")]
    DuplicateVertex,
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

After Rust changes, verify documentation builds:

```bash
just doc-check
```

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
