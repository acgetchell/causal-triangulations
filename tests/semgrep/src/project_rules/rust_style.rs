#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]

use num_traits::cast::NumCast;

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

fn safe_f64(_value: u64) -> Option<f64> {
    Some(1.0)
}

fn silent_conversion_fallback_fixture(value: u64) -> Option<f64> {
    // ruleid: causal-triangulations.rust.no-silent-conversion-fallbacks
    let _ = NumCast::from(value).unwrap_or(0.0);

    // ruleid: causal-triangulations.rust.no-silent-conversion-fallbacks
    let _ = num_traits::cast::<u64, f64>(value).unwrap_or_else(|| 0.0);

    // ruleid: causal-triangulations.rust.no-silent-conversion-fallbacks
    let _ = safe_f64(value).unwrap_or(0.0);

    // ok: causal-triangulations.rust.no-silent-conversion-fallbacks
    NumCast::from(value)
}

fn partial_cmp_ordering_default_fixture(left: f64, right: f64) -> std::cmp::Ordering {
    // ruleid: causal-triangulations.rust.no-partial-cmp-ordering-defaults
    left.partial_cmp(&right)
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn function_local_use_fixture() {
    // ruleid: causal-triangulations.rust.no-function-local-use-in-src
    use std::cmp::Ordering;

    let _ = Ordering::Equal;
}

mod tests {
    fn test_helper() {
        // ok: causal-triangulations.rust.no-function-local-use-in-src
        use std::cmp::Ordering;

        let _ = Ordering::Equal;
    }
}

fn production_unwrap_and_panic_fixture(result: Result<u32, &'static str>, value: Option<u32>) {
    // ruleid: causal-triangulations.rust.no-bare-unwrap-in-src
    let _ = result.unwrap();

    // ruleid: causal-triangulations.rust.no-bare-unwrap-in-src
    let _ = value.unwrap();

    // ok: causal-triangulations.rust.no-bare-unwrap-in-src
    let _ = result.unwrap_or(0);

    // ruleid: causal-triangulations.rust.no-panic-in-src
    panic!("production code should return a typed error");
}

#[cfg(test)]
fn test_only_unwrap_and_panic_fixture(result: Result<u32, &'static str>) {
    // ok: causal-triangulations.rust.no-bare-unwrap-in-src
    let _ = result.unwrap();

    // ok: causal-triangulations.rust.no-panic-in-src
    panic!("tests may fail fast");
}

#[cfg(test)]
mod prop_tests {
    fn helper(result: Result<u32, &'static str>) {
        // ok: causal-triangulations.rust.no-bare-unwrap-in-src
        let _ = result.unwrap();

        // ok: causal-triangulations.rust.no-panic-in-src
        panic!("tests may fail fast");
    }
}

// ruleid: causal-triangulations.rust.no-clippy-allow-lints
#[allow(clippy::too_many_lines)]
fn clippy_allow_fixture() {}

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
