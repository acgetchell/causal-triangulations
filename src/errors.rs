//! Error types for the CDT library.

use crate::cdt::foliation::FoliationError;

/// Main error type for CDT operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CdtError {
    /// Invalid dimension specified
    #[error("Unsupported dimension: {0}. Only 2D is currently supported")]
    UnsupportedDimension(u32),
    /// Delaunay triangulation generation failed with detailed context
    #[error(
        "Delaunay triangulation generation failed: {vertex_count} vertices, range [{}, {}], attempt {attempt}: {underlying_error}",
        coordinate_range.0,
        coordinate_range.1
    )]
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
    #[error(
        "Invalid triangulation parameters: {issue} (got: {provided_value}, expected: {expected_range})"
    )]
    InvalidGenerationParameters {
        /// Description of the specific parameter issue
        issue: String,
        /// The actual value that was provided
        provided_value: String,
        /// The expected range or constraint for the parameter
        expected_range: String,
    },
    /// Top-level CDT configuration failed validation.
    #[error("Invalid configuration: {setting} (got: {provided_value}, expected: {expected})")]
    InvalidConfiguration {
        /// Name of the invalid configuration setting.
        setting: String,
        /// Value supplied for the setting.
        provided_value: String,
        /// Expected constraint for the setting.
        expected: String,
    },
    /// Metropolis / simulation configuration failed validation.
    #[error(
        "Invalid simulation configuration: {setting} (got: {provided_value}, expected: {expected})"
    )]
    InvalidSimulationConfiguration {
        /// Name of the invalid simulation setting.
        setting: String,
        /// Value supplied for the setting.
        provided_value: String,
        /// Expected constraint for the setting.
        expected: String,
    },
    /// Constructed triangulation metadata is internally inconsistent.
    #[error(
        "Invalid triangulation metadata: {field} for {topology} (got: {provided_value}, expected: {expected})"
    )]
    InvalidTriangulationMetadata {
        /// Name of the invalid metadata field.
        field: String,
        /// Topology whose invariant was violated.
        topology: String,
        /// Value stored in the triangulation metadata.
        provided_value: String,
        /// Expected constraint for the metadata field.
        expected: String,
    },
    /// Validation of a constructed triangulation failed
    #[error("Validation failed [{check}]: {detail}")]
    ValidationFailed {
        /// Name of the validation check that failed (e.g. "geometry", "topology", "Delaunay")
        check: String,
        /// Human-readable description of the failure
        detail: String,
    },
    /// Vertex construction failed during triangulation generation
    #[error("Vertex construction failed [{context}]: {underlying_error}")]
    VertexBuildFailed {
        /// Human-readable context (e.g., function name or vertex index)
        context: String,
        /// The underlying builder error message
        underlying_error: String,
    },
    /// Backend payload mutation failed due to an invalid or unavailable handle.
    #[error("Backend mutation failed [{operation}] on {target}: {detail}")]
    BackendMutationFailed {
        /// Mutation operation being attempted.
        operation: String,
        /// Human-readable target handle (e.g., "vertex `VertexKey`(..)").
        target: String,
        /// Additional failure detail.
        detail: String,
    },
    /// An edge violates the causal structure by spanning more than one time slice
    /// (or, on toroidal topology, more than one *circular* slice step).
    #[error("{}", format_causality_violation(*time_0, *time_1, *step_distance))]
    CausalityViolation {
        /// Time label of the first endpoint.
        time_0: u32,
        /// Time label of the second endpoint.
        time_1: u32,
        /// Topology-aware temporal step distance between the two labels.
        ///
        /// On `OpenBoundary` topology this equals `time_0.abs_diff(time_1)`.
        /// On `Toroidal` topology it is the circular distance
        /// `min(d, T − d)`, so the wrap-around edge between slice `T − 1`
        /// and slice `0` reads as `1` rather than `T − 1`.  This is the
        /// quantity that triggers the violation (`step_distance > 1`).
        step_distance: u32,
    },
    /// MCMC framework error (e.g. NaN in log-probability)
    #[error("MCMC error: {0}")]
    Mcmc(String),
}

fn format_causality_violation(time_0: u32, time_1: u32, step_distance: u32) -> String {
    let raw = time_0.abs_diff(time_1);
    if raw == step_distance {
        format!(
            "Causality violation: edge spans {step_distance} time-slice steps \
             (t={time_0} to t={time_1}), maximum allowed is 1"
        )
    } else {
        // Toroidal: the displayed step distance is the circular distance,
        // smaller than the raw label difference.
        format!(
            "Causality violation: edge spans {step_distance} time-slice steps \
             (t={time_0} to t={time_1}, |Δt|={raw} on the time circle), \
             maximum allowed is 1"
        )
    }
}

impl From<markov_chain_monte_carlo::McmcError> for CdtError {
    fn from(err: markov_chain_monte_carlo::McmcError) -> Self {
        Self::Mcmc(err.to_string())
    }
}

impl From<FoliationError> for CdtError {
    fn from(err: FoliationError) -> Self {
        Self::ValidationFailed {
            check: "foliation".to_string(),
            detail: err.to_string(),
        }
    }
}

/// Result type for CDT operations.
pub type CdtResult<T> = Result<T, CdtError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_configuration_error() {
        let error = CdtError::InvalidConfiguration {
            setting: "vertices".to_string(),
            provided_value: "2".to_string(),
            expected: "≥ 3".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Invalid configuration: vertices (got: 2, expected: ≥ 3)"
        );
    }

    #[test]
    fn test_invalid_simulation_configuration_error() {
        let error = CdtError::InvalidSimulationConfiguration {
            setting: "temperature".to_string(),
            provided_value: "NaN".to_string(),
            expected: "finite and positive".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Invalid simulation configuration: temperature (got: NaN, expected: finite and positive)"
        );
    }

    #[test]
    fn test_invalid_triangulation_metadata_error() {
        let error = CdtError::InvalidTriangulationMetadata {
            field: "timeslices".to_string(),
            topology: "toroidal".to_string(),
            provided_value: "2".to_string(),
            expected: "≥ 3".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Invalid triangulation metadata: timeslices for toroidal (got: 2, expected: ≥ 3)"
        );
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
    fn test_unsupported_dimension_error() {
        let error = CdtError::UnsupportedDimension(3);
        let display = format!("{error}");
        assert_eq!(
            display,
            "Unsupported dimension: 3. Only 2D is currently supported"
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
    fn test_backend_mutation_failed_error() {
        let error = CdtError::BackendMutationFailed {
            operation: "set_vertex_data".to_string(),
            target: "vertex VertexKey(123v1)".to_string(),
            detail: "backend reported invalid vertex key".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Backend mutation failed [set_vertex_data] on vertex VertexKey(123v1): backend reported invalid vertex key"
        );
    }

    #[test]
    fn test_causality_violation_open_boundary_error() {
        // OpenBoundary topology: step_distance == |Δt|.
        let error = CdtError::CausalityViolation {
            time_0: 0,
            time_1: 3,
            step_distance: 3,
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Causality violation: edge spans 3 time-slice steps (t=0 to t=3), maximum allowed is 1"
        );
    }

    #[test]
    fn test_causality_violation_toroidal_error_reports_circular_distance() {
        // Toroidal T=10, t0=0, t1=8: raw |Δt|=8 but circular step distance is 2.
        let error = CdtError::CausalityViolation {
            time_0: 0,
            time_1: 8,
            step_distance: 2,
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Causality violation: edge spans 2 time-slice steps \
             (t=0 to t=8, |Δt|=8 on the time circle), maximum allowed is 1"
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
        let error1 = CdtError::InvalidConfiguration {
            setting: "steps".to_string(),
            provided_value: "0".to_string(),
            expected: "≥ 1".to_string(),
        };
        let error2 = CdtError::InvalidConfiguration {
            setting: "steps".to_string(),
            provided_value: "0".to_string(),
            expected: "≥ 1".to_string(),
        };
        let error3 = CdtError::InvalidConfiguration {
            setting: "steps".to_string(),
            provided_value: "10".to_string(),
            expected: "≥ 1".to_string(),
        };

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
        let error = CdtError::InvalidConfiguration {
            setting: "vertices".to_string(),
            provided_value: "2".to_string(),
            expected: "≥ 3".to_string(),
        };
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("InvalidConfiguration"));
        assert!(debug_str.contains("vertices"));
    }

    #[test]
    fn test_cdt_result_type() {
        let success: CdtResult<i32> = Ok(42);
        let failure: CdtResult<i32> = Err(CdtError::InvalidConfiguration {
            setting: "steps".to_string(),
            provided_value: "0".to_string(),
            expected: "≥ 1".to_string(),
        });

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
        use std::error::Error;

        let error = CdtError::InvalidConfiguration {
            setting: "temperature".to_string(),
            provided_value: "NaN".to_string(),
            expected: "finite and positive".to_string(),
        };
        let _: &dyn Error = &error;
        // If this compiles, the trait is implemented correctly
    }
}
