#![forbid(unsafe_code)]
#![allow(dead_code)]

fn benchmark_and_example_fixture(result: Result<u32, &'static str>, value: Option<u32>) {
    // ruleid: causal-triangulations.rust.no-unwrap-expect-in-benches-examples
    let _ = result.unwrap();

    // ruleid: causal-triangulations.rust.no-unwrap-expect-in-benches-examples
    let _ = value.expect("benchmarks and examples should avoid expect");

    // ok: causal-triangulations.rust.no-unwrap-expect-in-benches-examples
    let _ = result.unwrap_or(0);
}

// ruleid: causal-triangulations.rust.no-box-dyn-error-in-examples-benches
fn erased_benchmark_error() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

// ruleid: causal-triangulations.rust.no-box-dyn-error-in-examples-benches
fn borrowed_example_error(error: &dyn std::error::Error) {
    let _ = error.to_string();
}

// ok: causal-triangulations.rust.no-box-dyn-error-in-examples-benches
fn typed_example_error() -> causal_triangulations::CdtResult<()> {
    Ok(())
}
