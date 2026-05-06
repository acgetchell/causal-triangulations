#![forbid(unsafe_code)]
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
use delaunay::{
    core::triangulation::TopologyGuarantee,
    topology::traits::topological_space::GlobalTopology,
};

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

fn nonfinite_conversion_default_fixture(value: Option<f64>) {
    // ruleid: causal-triangulations.rust.no-nonfinite-unwrap-defaults
    let _ = value.unwrap_or(f64::INFINITY);

    // ruleid: causal-triangulations.rust.no-nonfinite-unwrap-defaults
    let _ = value.unwrap_or_else(|| f64::NAN);

    // ruleid: causal-triangulations.rust.no-nonfinite-unwrap-defaults
    let _ = value.unwrap_or_else(|| std::f64::NAN);

    // ok: causal-triangulations.rust.no-nonfinite-unwrap-defaults
    let _ = value.unwrap_or(f64::MAX);
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

enum CdtError {
    ValidationFailed { check: String, detail: String },
    TopologyMismatch,
    Foliation,
    UnsupportedOperation,
}

// ruleid: causal-triangulations.rust.public-error-enums-non-exhaustive
pub enum MissingNonExhaustiveError {
    InvalidInput,
}

mod nested_error_fixtures {
    #[derive(Debug)]
    // ruleid: causal-triangulations.rust.public-error-enums-non-exhaustive
    pub enum NestedMissingNonExhaustiveError {
        InvalidInput,
    }
}

// ok: causal-triangulations.rust.public-error-enums-non-exhaustive
#[non_exhaustive]
pub enum ExtensibleError {
    InvalidInput,
}

// ruleid: causal-triangulations.rust.prefer-focused-prelude-imports-in-public-usage
use causal_triangulations::geometry::DelaunayBackend2D;

// ok: causal-triangulations.rust.prefer-focused-prelude-imports-in-public-usage
use causal_triangulations::prelude::geometry::DelaunayBackend2D as PreludeDelaunayBackend2D;

fn stringly_domain_validation_errors() {
    // ruleid: causal-triangulations.rust.no-stringly-domain-validation-errors
    let _ = CdtError::ValidationFailed {
        check: "foliation".to_string(),
        detail: "missing label".to_string(),
    };

    // ruleid: causal-triangulations.rust.no-stringly-domain-validation-errors
    let _ = CdtError::ValidationFailed {
        detail: "bad Euler characteristic".to_string(),
        check: String::from("topology"),
    };

    // ruleid: causal-triangulations.rust.no-stringly-domain-validation-errors
    let _ = CdtError::ValidationFailed {
        check: "cdt_construction".into(),
        detail: "not implemented".to_string(),
    };

    // ruleid: causal-triangulations.rust.no-stringly-domain-validation-errors
    let _ = CdtError::ValidationFailed {
        detail: "slice count is invalid".to_string(),
        check: "foliation",
    };

    // ok: causal-triangulations.rust.no-stringly-domain-validation-errors
    let _ = CdtError::ValidationFailed {
        check: "geometry".to_string(),
        detail: "backend rejected structure".to_string(),
    };
}
