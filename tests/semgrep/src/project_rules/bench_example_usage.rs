#![forbid(unsafe_code)]
#![allow(dead_code)]

fn benchmark_and_example_fixture(result: Result<u32, &'static str>, value: Option<u32>) {
    // ruleid: causal-triangulations.rust.no-unwrap-expect-in-benches-examples, causal-triangulations.rust.no-bare-unwrap-in-src
    let _ = result.unwrap();

    // ruleid: causal-triangulations.rust.no-unwrap-expect-in-benches-examples
    let _ = value.expect("benchmarks and examples should avoid expect");

    // ok: causal-triangulations.rust.no-unwrap-expect-in-benches-examples
    let _ = result.unwrap_or(0);
}
