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
