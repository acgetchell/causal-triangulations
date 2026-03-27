//! Error types for the CDT library.

use std::fmt;

/// Main error type for CDT operations.
#[derive(Debug, Clone, PartialEq)]
pub enum CdtError {
    /// Invalid triangulation parameters
    InvalidParameters(String),
    /// Triangulation generation failed
    TriangulationGeneration(String),
    /// Ergodic move failed
    ErgodicsFailure(String),
    /// Invalid dimension specified
    UnsupportedDimension(u32),
    /// Action calculation error
    ActionCalculation(String),
    /// Delaunay triangulation generation failed with detailed context
    DelaunayGenerationFailed {
        /// Number of vertices requested for the triangulation
        vertex_count: u32,
        /// Coordinate range used for generation
        coordinate_range: (f64, f64),
        /// Attempt number when the failure occurred
        attempt: u32,
        /// Description of the underlying error that caused the failure
        underlying_error: String,
    },
    /// Invalid generation parameters detected before attempting triangulation
    InvalidGenerationParameters {
        /// Description of the specific parameter issue
        issue: String,
        /// The actual value that was provided
        provided_value: String,
        /// The expected range or constraint for the parameter
        expected_range: String,
    },
    /// Validation of a constructed triangulation failed
    ValidationFailed {
        /// Name of the validation check that failed (e.g. "geometry", "topology", "Delaunay")
        check: String,
        /// Human-readable description of the failure
        detail: String,
    },
    /// Vertex construction failed during triangulation generation
    VertexBuildFailed {
        /// Human-readable context (e.g., function name or vertex index)
        context: String,
        /// The underlying builder error message
        underlying_error: String,
    },
    /// An edge violates the causal structure by spanning more than one time slice
    CausalityViolation {
        /// Time label of the first endpoint
        time_0: u32,
        /// Time label of the second endpoint
        time_1: u32,
    },
    /// MCMC framework error (e.g. NaN in log-probability)
    Mcmc(String),
}

impl fmt::Display for CdtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParameters(msg) => write!(f, "Invalid parameters: {msg}"),
            Self::TriangulationGeneration(msg) => {
                write!(f, "Triangulation generation failed: {msg}")
            }
            Self::ErgodicsFailure(msg) => write!(f, "Ergodic move failed: {msg}"),
            Self::UnsupportedDimension(dim) => write!(
                f,
                "Unsupported dimension: {dim}. Only 2D is currently supported"
            ),
            Self::ActionCalculation(msg) => write!(f, "Action calculation error: {msg}"),
            Self::DelaunayGenerationFailed {
                vertex_count,
                coordinate_range,
                attempt,
                underlying_error,
            } => write!(
                f,
                "Delaunay triangulation generation failed: {vertex_count} vertices, range [{}, {}], attempt {attempt}: {underlying_error}",
                coordinate_range.0, coordinate_range.1
            ),
            Self::InvalidGenerationParameters {
                issue,
                provided_value,
                expected_range,
            } => write!(
                f,
                "Invalid triangulation parameters: {issue} (got: {provided_value}, expected: {expected_range})",
            ),
            Self::ValidationFailed { check, detail } => {
                write!(f, "Validation failed [{check}]: {detail}")
            }
            Self::VertexBuildFailed {
                context,
                underlying_error,
            } => write!(
                f,
                "Vertex construction failed [{context}]: {underlying_error}"
            ),
            Self::CausalityViolation { time_0, time_1 } => {
                let dt = time_0.abs_diff(*time_1);
                write!(
                    f,
                    "Causality violation: edge spans {dt} time slices (t={time_0} to t={time_1}), maximum allowed is 1"
                )
            }
            Self::Mcmc(msg) => write!(f, "MCMC error: {msg}"),
        }
    }
}

impl From<markov_chain_monte_carlo::McmcError> for CdtError {
    fn from(err: markov_chain_monte_carlo::McmcError) -> Self {
        Self::Mcmc(err.to_string())
    }
}

impl From<crate::cdt::foliation::FoliationError> for CdtError {
    fn from(err: crate::cdt::foliation::FoliationError) -> Self {
        Self::ValidationFailed {
            check: "foliation".to_string(),
            detail: err.to_string(),
        }
    }
}

impl std::error::Error for CdtError {}

/// Result type for CDT operations.
pub type CdtResult<T> = Result<T, CdtError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_parameters_error() {
        let error = CdtError::InvalidParameters("Test message".to_string());
        let display = format!("{error}");
        assert_eq!(display, "Invalid parameters: Test message");
    }

    #[test]
    fn test_triangulation_generation_error() {
        let error = CdtError::TriangulationGeneration("Generation failed".to_string());
        let display = format!("{error}");
        assert_eq!(
            display,
            "Triangulation generation failed: Generation failed"
        );
    }

    #[test]
    fn test_ergodics_failure_error() {
        let error = CdtError::ErgodicsFailure("Move rejected".to_string());
        let display = format!("{error}");
        assert_eq!(display, "Ergodic move failed: Move rejected");
    }

    #[test]
    fn test_unsupported_dimension_error() {
        let error = CdtError::UnsupportedDimension(3);
        let display = format!("{error}");
        assert_eq!(
            display,
            "Unsupported dimension: 3. Only 2D is currently supported"
        );
    }

    #[test]
    fn test_action_calculation_error() {
        let error = CdtError::ActionCalculation("NaN result".to_string());
        let display = format!("{error}");
        assert_eq!(display, "Action calculation error: NaN result");
    }

    #[test]
    fn test_delaunay_generation_failed_error() {
        let error = CdtError::DelaunayGenerationFailed {
            vertex_count: 10,
            coordinate_range: (-1.0, 1.0),
            attempt: 5,
            underlying_error: "Too many duplicate points".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Delaunay triangulation generation failed: 10 vertices, range [-1, 1], attempt 5: Too many duplicate points"
        );
    }

    #[test]
    fn test_invalid_generation_parameters_error() {
        let error = CdtError::InvalidGenerationParameters {
            issue: "Vertex count too small".to_string(),
            provided_value: "2".to_string(),
            expected_range: "at least 3".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Invalid triangulation parameters: Vertex count too small (got: 2, expected: at least 3)"
        );
    }

    #[test]
    fn test_validation_failed_error() {
        let error = CdtError::ValidationFailed {
            check: "topology".to_string(),
            detail: "Euler characteristic χ=3 unexpected (V=5, E=8, F=6)".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Validation failed [topology]: Euler characteristic χ=3 unexpected (V=5, E=8, F=6)"
        );
    }

    #[test]
    fn test_vertex_build_failed_error() {
        let error = CdtError::VertexBuildFailed {
            context: "from_foliated_cylinder vertex 7".to_string(),
            underlying_error: "Missing required field: `point`".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Vertex construction failed [from_foliated_cylinder vertex 7]: Missing required field: `point`"
        );
    }

    #[test]
    fn test_causality_violation_error() {
        let error = CdtError::CausalityViolation {
            time_0: 0,
            time_1: 3,
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Causality violation: edge spans 3 time slices (t=0 to t=3), maximum allowed is 1"
        );
    }

    #[test]
    fn test_mcmc_error() {
        let error = CdtError::Mcmc("NaN log-probability".to_string());
        let display = format!("{error}");
        assert_eq!(display, "MCMC error: NaN log-probability");
    }

    #[test]
    fn test_mcmc_error_from_conversion() {
        let mcmc_err = markov_chain_monte_carlo::McmcError::NanProposedLogProb;
        let cdt_err: CdtError = mcmc_err.into();
        let display = format!("{cdt_err}");
        assert!(
            display.contains("MCMC error"),
            "Should contain MCMC error prefix: {display}"
        );
        assert!(
            display.contains("NaN"),
            "Should contain NaN context: {display}"
        );
    }

    #[test]
    fn test_error_equality() {
        let error1 = CdtError::InvalidParameters("Test".to_string());
        let error2 = CdtError::InvalidParameters("Test".to_string());
        let error3 = CdtError::InvalidParameters("Different".to_string());

        assert_eq!(error1, error2);
        assert_ne!(error1, error3);
    }

    #[test]
    fn test_error_clone() {
        let error = CdtError::UnsupportedDimension(4);
        let cloned = error.clone();
        assert_eq!(error, cloned);
    }

    #[test]
    fn test_error_debug() {
        let error = CdtError::InvalidParameters("Debug test".to_string());
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("InvalidParameters"));
        assert!(debug_str.contains("Debug test"));
    }

    #[test]
    fn test_cdt_result_type() {
        let success: CdtResult<i32> = Ok(42);
        let failure: CdtResult<i32> = Err(CdtError::InvalidParameters("Test".to_string()));

        assert!(success.is_ok());
        assert!(failure.is_err());
        assert_eq!(success, Ok(42));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CdtError>();
    }

    #[test]
    fn test_std_error_trait() {
        let error = CdtError::InvalidParameters("Test error".to_string());
        let _: &dyn std::error::Error = &error;
        // If this compiles, the trait is implemented correctly
    }
}
