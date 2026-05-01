#![allow(dead_code, unused_imports)]

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
use delaunay::prelude::DelaunayTriangulation;

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
pub use delaunay::prelude::VertexBuilder;

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
use delaunay::{core::DataType, geometry::kernel::AdaptiveKernel};

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
pub use delaunay::{core::edge::EdgeKey, core::tds::VertexKey};

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
pub(crate) use delaunay::prelude::Tds;

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
pub(super) use delaunay::{core::cell::CellKey, core::facet::Facet};

// ruleid: causal-triangulations.rust.no-direct-delaunay-imports-outside-geometry
use delaunay as dt;

pub fn production_stdio() {
    // ruleid: causal-triangulations.rust.no-stdio-diagnostics-in-src
    println!("debug output");

    // ruleid: causal-triangulations.rust.no-stdio-diagnostics-in-src
    eprintln!("debug output");
}

pub fn env_gated_stdio() {
    // ruleid: causal-triangulations.rust.no-env-gated-stdio-diagnostics
    if std::env::var_os("CDT_DEBUG").is_some() {
        // ruleid: causal-triangulations.rust.no-stdio-diagnostics-in-src
        println!("debug output");
    }
}

// ruleid: causal-triangulations.rust.expect-requires-reason
#[expect(clippy::too_many_lines)]
fn expect_without_reason_fixture() {}

// ok: causal-triangulations.rust.expect-requires-reason
#[expect(clippy::too_many_lines, reason = "fixture documents the suppression")]
fn expect_with_reason_fixture() {}

// ruleid: causal-triangulations.rust.no-box-dyn-error-in-src
type ProductionBoxedError = Box<dyn std::error::Error>;

trait ProductionDynamicErrors {
    // ruleid: causal-triangulations.rust.no-box-dyn-error-in-src
    fn boxed_error_result(&self) -> Result<(), Box<dyn std::error::Error>>;

    // ruleid: causal-triangulations.rust.no-box-dyn-error-in-src
    fn borrowed_error(&self, error: &dyn std::error::Error);
}

/// # Ok::<(), Box<dyn std::error::Error>>(())
fn doctest_style_error_is_ignored() {}
