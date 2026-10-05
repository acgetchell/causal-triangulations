pub fn forbidden_receiver_calls(left: f64, right: f64) -> [f64; 5] {
    [
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        left.algebraic_add(right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        left.algebraic_sub(right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        left.algebraic_mul(right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        left.algebraic_div(right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        left.algebraic_rem(right),
    ]
}

pub fn forbidden_associated_calls(left: f64, right: f64) -> [f64; 5] {
    [
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_add(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_sub(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_mul(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_div(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_rem(left, right),
    ]
}

pub fn forbidden_function_items() -> [fn(f64, f64) -> f64; 5] {
    [
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_add,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_sub,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_mul,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_div,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        f64::algebraic_rem,
    ]
}

pub fn forbidden_qualified_calls(left: f64, right: f64) -> [f64; 5] {
    [
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_add(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_sub(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_mul(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_div(left, right),
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_rem(left, right),
    ]
}

pub fn forbidden_qualified_items() -> [fn(f64, f64) -> f64; 5] {
    [
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_add,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_sub,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_mul,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_div,
        // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::algebraic_rem,
    ]
}

pub fn forbidden_callbacks(values: &[f64]) -> [Option<f64>; 5] {
    [
        values.iter().copied().reduce(
            // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
            f64::algebraic_add,
        ),
        values.iter().copied().reduce(
            // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
            f64::algebraic_sub,
        ),
        values.iter().copied().reduce(
            // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
            f64::algebraic_mul,
        ),
        values.iter().copied().reduce(
            // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
            f64::algebraic_div,
        ),
        values.iter().copied().reduce(
            // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
            f64::algebraic_rem,
        ),
    ]
}

pub fn permitted_operations(left: f64, right: f64) -> [f64; 8] {
    [
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        left + right,
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        left - right,
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        left * right,
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        left / right,
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        left % right,
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        left.mul_add(right, 1.0),
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        f64::mul_add(left, right, 1.0),
        // ok: causal-triangulations.rust.no-algebraic-f64-operations
        <f64>::mul_add(left, right, 1.0),
    ]
}

pub fn permitted_qualified_fma_item() -> fn(f64, f64, f64) -> f64 {
    // ok: causal-triangulations.rust.no-algebraic-f64-operations
    <f64>::mul_add
}

/// ```rust
/// // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
/// let value = 1.0_f64.algebraic_add(2.0);
/// ```
pub fn forbidden_receiver_doctest() {}

/// ```
/// // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
/// let value = f64::algebraic_sub(1.0, 2.0);
/// ```
pub fn forbidden_associated_doctest() {}

/// ```rust,no_run
/// // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
/// let multiply = <f64>::algebraic_mul;
/// ```
pub fn forbidden_function_item_doctest() {}

mod inner_doctest {
    //! ```no_run
    //! // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
    //! let value = <f64>::algebraic_div(1.0, 2.0);
    //! ```
}

/**
 * ```rust
 * // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
 * let value = 1.0_f64.algebraic_rem(2.0);
 * ```
 */
pub fn forbidden_block_doctest() {}

mod inner_block_doctest {
    /*!
    ```rust
    // ruleid: causal-triangulations.rust.no-algebraic-f64-operations
    let value = 1.0_f64.algebraic_add(2.0);
    ```
    */
}

/// Prose may discuss f64::algebraic_add without executing it.
/// ```rust
/// // ok: causal-triangulations.rust.no-algebraic-f64-operations
/// let value = 1.0_f64 + 2.0;
/// // ok: causal-triangulations.rust.no-algebraic-f64-operations
/// let value = 1.0_f64.mul_add(2.0, 3.0);
/// ```
/// Prose after a Rust fence may also discuss f64::algebraic_sub.
/// ```
/// // ok: causal-triangulations.rust.no-algebraic-f64-operations
/// let value = 1.0_f64 - 2.0;
/// ```
/// ```text
/// f64::algebraic_add is forbidden in Rust code.
/// ```
pub fn permitted_doctest() {}

/**
 * ```rust
 * // ok: causal-triangulations.rust.no-algebraic-f64-operations
 * let value = 1.0_f64 % 2.0;
 * ```
 * Prose outside the fence may discuss f64::algebraic_rem.
 * ```
 * // ok: causal-triangulations.rust.no-algebraic-f64-operations
 * let value = 1.0_f64 / 2.0;
 * ```
 */
pub fn permitted_block_doctest() {}

pub fn permitted_markdown_string() -> &'static str {
    // ok: causal-triangulations.rust.no-algebraic-f64-operations
    r"example text:
```rust
let value = 1.0_f64.algebraic_add(2.0);
```
"
}
