#![forbid(unsafe_code)]

//! Deterministic heap-allocation checks for cached CDT observables.

#[path = "support/or_abort.rs"]
mod benchmark_support;

use benchmark_support::OrAbort;
use causal_triangulations::prelude::triangulation::CdtTriangulation2D;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn main() {
    let triangulation =
        CdtTriangulation2D::from_cdt_strip(20, 10).or_abort("build allocation benchmark fixture");

    black_box(triangulation.edge_count());
    black_box(
        triangulation
            .slab_triangle_profile()
            .or_abort("prime slab-triangle-profile cache"),
    );

    let profiler = dhat::Profiler::builder().testing().build();
    black_box(triangulation.edge_count());
    black_box(
        triangulation
            .slab_triangle_profile()
            .or_abort("read cached slab-triangle profile"),
    );
    let stats = dhat::HeapStats::get();

    dhat::assert_eq!(stats.total_blocks, 1);
    println!(
        "cached observables: {} allocation, {} bytes",
        stats.total_blocks, stats.total_bytes
    );
    drop(profiler);
}
