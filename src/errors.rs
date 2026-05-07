#![forbid(unsafe_code)]

//! Error types for the CDT library.

use crate::cdt::ergodic_moves::MoveType;
use crate::cdt::foliation::FoliationError;
use crate::config::CdtTopology;
use std::fmt;

/// Highest cumulative upstream Delaunay validation level being enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DelaunayValidationLevel {
    /// Validate Level 1 only.
    One,
    /// Validate Levels 1 through 2.
    Two,
    /// Validate Levels 1 through 3.
    Three,
    /// Validate Levels 1 through 4.
    Four,
}

impl fmt::Display for DelaunayValidationLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::One => formatter.write_str("Level 1"),
            Self::Two => formatter.write_str("Level 1-2"),
            Self::Three => formatter.write_str("Level 1-3"),
            Self::Four => formatter.write_str("Level 1-4"),
        }
    }
}

/// CDT validation check that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CdtValidationCheck {
    /// Generic backend geometry validation.
    Geometry,
    /// Foliation assignment from coordinates failed.
    FoliationAssignment,
    /// Causality validation failed.
    Causality,
    /// Strict CDT cell classification failed.
    CellClassification,
    /// Local ergodic move candidate geometry could not be interpreted.
    ErgodicMoveCandidateGeometry,
}

impl fmt::Display for CdtValidationCheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry => formatter.write_str("geometry"),
            Self::FoliationAssignment => formatter.write_str("foliation_assignment"),
            Self::Causality => formatter.write_str("causality"),
            Self::CellClassification => formatter.write_str("cell_classification"),
            Self::ErgodicMoveCandidateGeometry => {
                formatter.write_str("ergodic_move_candidate_geometry")
            }
        }
    }
}

/// Category explaining why a checkpoint could not be resumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CheckpointResumeReason {
    /// Resumed step count would overflow.
    StepCountOverflow,
    /// Checkpoint target reconstruction failed.
    CheckpointTargetConfiguration,
    /// Generic MCMC chain restoration failed.
    McmcChainRestore,
    /// Restored triangulation failed invariant validation.
    TriangulationInvariants,
    /// Stored action disagrees with recomputed action.
    ActionMismatch,
    /// Action configuration differs from the checkpoint.
    IncompatibleActionConfiguration,
    /// Temperature differs from the checkpoint.
    IncompatibleTemperature,
    /// Thermalization schedule differs from the checkpoint.
    IncompatibleThermalizationSchedule,
    /// Measurement frequency differs from the checkpoint.
    IncompatibleMeasurementFrequency,
    /// Checkpoint simulation configuration failed validation.
    CheckpointConfiguration,
    /// Checkpoint action configuration failed validation.
    CheckpointActionConfiguration,
    /// Generic MCMC chain counters disagree with CDT move statistics.
    ChainCounterMismatch,
    /// Generic MCMC chain step count disagrees with checkpoint step.
    ChainStepMismatch,
    /// Step telemetry is internally inconsistent.
    StepTelemetryMismatch,
    /// Step telemetry index conversion overflowed.
    StepTelemetryOverflow,
    /// Measurement telemetry count or step conversion overflowed.
    MeasurementTelemetryOverflow,
    /// Measurement telemetry is internally inconsistent.
    MeasurementTelemetryMismatch,
    /// Move statistics violate internal accounting invariants.
    MoveStatisticsInvariant,
    /// Accepted or rejected counter conversion overflowed.
    CounterConversionOverflow,
}

impl fmt::Display for CheckpointResumeReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StepCountOverflow => formatter.write_str("step count overflow"),
            Self::CheckpointTargetConfiguration => {
                formatter.write_str("checkpoint target configuration")
            }
            Self::McmcChainRestore => formatter.write_str("mcmc chain restore"),
            Self::TriangulationInvariants => formatter.write_str("triangulation invariants"),
            Self::ActionMismatch => formatter.write_str("action mismatch"),
            Self::IncompatibleActionConfiguration => {
                formatter.write_str("incompatible action configuration")
            }
            Self::IncompatibleTemperature => formatter.write_str("incompatible temperature"),
            Self::IncompatibleThermalizationSchedule => {
                formatter.write_str("incompatible thermalization schedule")
            }
            Self::IncompatibleMeasurementFrequency => {
                formatter.write_str("incompatible measurement frequency")
            }
            Self::CheckpointConfiguration => formatter.write_str("checkpoint configuration"),
            Self::CheckpointActionConfiguration => {
                formatter.write_str("checkpoint action configuration")
            }
            Self::ChainCounterMismatch => formatter.write_str("chain counter mismatch"),
            Self::ChainStepMismatch => formatter.write_str("chain step mismatch"),
            Self::StepTelemetryMismatch => formatter.write_str("step telemetry mismatch"),
            Self::StepTelemetryOverflow => formatter.write_str("step telemetry overflow"),
            Self::MeasurementTelemetryOverflow => {
                formatter.write_str("measurement telemetry overflow")
            }
            Self::MeasurementTelemetryMismatch => {
                formatter.write_str("measurement telemetry mismatch")
            }
            Self::MoveStatisticsInvariant => formatter.write_str("move statistics invariant"),
            Self::CounterConversionOverflow => formatter.write_str("counter conversion overflow"),
        }
    }
}

/// Main error type for CDT operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
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
    /// Upstream Delaunay validation rejected a geometry backend.
    #[error("Delaunay validation failed [{level}]: {detail}")]
    DelaunayValidationFailed {
        /// Cumulative upstream validation level being enforced.
        level: DelaunayValidationLevel,
        /// Upstream validation diagnostic.
        detail: String,
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
    /// Metropolis accepted a move, but a hard backend or invariant failure stopped application.
    #[error(
        "Metropolis accepted {move_type:?} at step {step}, but applying it failed after {attempts} attempts; last failure: {last_failure}"
    )]
    MetropolisMoveApplicationFailed {
        /// Monte Carlo step whose accepted move could not be applied.
        step: u32,
        /// Accepted move type being applied.
        move_type: MoveType,
        /// Number of application attempts made before failing.
        attempts: usize,
        /// Most specific lower-level rejection or failure observed.
        last_failure: String,
    },
    /// Constructed triangulation metadata is internally inconsistent.
    #[error(
        "Invalid triangulation metadata: {field} for {topology} (got: {provided_value}, expected: {expected})"
    )]
    InvalidTriangulationMetadata {
        /// Name of the invalid metadata field.
        field: String,
        /// Topology whose invariant was violated.
        topology: CdtTopology,
        /// Value stored in the triangulation metadata.
        provided_value: String,
        /// Expected constraint for the metadata field.
        expected: String,
    },
    /// Validation of a constructed triangulation failed
    #[error("Validation failed [{check}]: {detail}")]
    ValidationFailed {
        /// Validation check that failed.
        check: CdtValidationCheck,
        /// Human-readable description of the failure
        detail: String,
    },
    /// Topology metadata does not match the backend Euler characteristic.
    #[error(
        "Topology mismatch for {topology}: Euler characteristic χ={euler_characteristic}, expected one of {expected_euler_characteristics:?} (V={vertices}, E={edges}, F={faces})"
    )]
    TopologyMismatch {
        /// Topology requested by CDT metadata.
        topology: CdtTopology,
        /// Observed Euler characteristic from the backend.
        euler_characteristic: i32,
        /// Accepted Euler characteristics for the requested topology.
        expected_euler_characteristics: Vec<i32>,
        /// Backend vertex count at validation time.
        vertices: usize,
        /// Backend edge count at validation time.
        edges: usize,
        /// Backend face count at validation time.
        faces: usize,
    },
    /// Foliation construction or validation failed with a typed foliation error.
    #[error("Foliation validation failed: {0}")]
    Foliation(#[from] FoliationError),
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
    /// Backend mutation failed and restoring previously staged payloads also failed.
    #[error(
        "Backend mutation failed [{operation}] on {target}: {detail}; rollback failed: {rollback_errors}"
    )]
    BackendRollbackFailed {
        /// Mutation operation being attempted when the first failure occurred.
        operation: String,
        /// Human-readable target handle for the first failure.
        target: String,
        /// Primary mutation failure detail.
        detail: String,
        /// Rollback failure details for one or more payloads.
        rollback_errors: String,
    },
    /// Requested operation is part of the planned API surface but is not implemented yet.
    #[error("Unsupported operation [{operation}]: {reason}")]
    UnsupportedOperation {
        /// Operation being requested.
        operation: String,
        /// Human-readable explanation and migration/status detail.
        reason: String,
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
    /// Writing CSV/JSON simulation output failed.
    #[error("Failed to write {format} output to {path}: {detail}")]
    OutputWriteFailed {
        /// Target output path.
        path: String,
        /// Output format being written.
        format: String,
        /// Lower-level I/O or serialization error.
        detail: String,
    },
    /// Resolving a configured output path failed before writing began.
    #[error("Failed to resolve output path from base {base_path}: {detail}")]
    OutputPathResolutionFailed {
        /// Base path used for resolving configured output paths.
        base_path: String,
        /// Lower-level path resolution error.
        detail: String,
    },
    /// Configured CSV and JSON output paths resolve to the same file.
    #[error("CSV output path {csv_path} and JSON output path {json_path} resolve to the same file")]
    OutputPathConflict {
        /// Resolved CSV output path.
        csv_path: String,
        /// Resolved JSON output path.
        json_path: String,
    },
    /// Reading or decoding CSV/JSON simulation output failed.
    #[error("Failed to read {format} output from {path}: {detail}")]
    OutputReadFailed {
        /// Source output path.
        path: String,
        /// Output format being read.
        format: String,
        /// Lower-level I/O or decoding error.
        detail: String,
    },
    /// Serializing or deserializing a CDT or MCMC checkpoint failed.
    #[error("Failed to {operation} {target} checkpoint: {detail}")]
    CheckpointSerializationFailed {
        /// Checkpoint operation being attempted.
        operation: String,
        /// Human-readable checkpoint target, such as "final triangulation".
        target: String,
        /// Lower-level serialization error.
        detail: String,
    },
    /// Restoring or continuing an MCMC checkpoint failed before sampling resumed.
    #[error("Failed to resume MCMC checkpoint [{reason}]: {detail}")]
    CheckpointResumeFailed {
        /// Structured reason category for the resume failure.
        reason: CheckpointResumeReason,
        /// Human-readable reason resume could not proceed.
        detail: String,
    },
}

/// Keeps causality error formatting centralized so open and toroidal distances stay consistent.
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

/// Result type for CDT operations.
pub type CdtResult<T> = Result<T, CdtError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

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
            topology: CdtTopology::Toroidal,
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
    fn test_delaunay_validation_failed_error() {
        let error = CdtError::DelaunayValidationFailed {
            level: DelaunayValidationLevel::Four,
            detail: "upstream validation failed".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Delaunay validation failed [Level 1-4]: upstream validation failed"
        );
    }

    #[test]
    fn validation_level_display_covers_all_levels() {
        assert_eq!(DelaunayValidationLevel::One.to_string(), "Level 1");
        assert_eq!(DelaunayValidationLevel::Two.to_string(), "Level 1-2");
        assert_eq!(DelaunayValidationLevel::Three.to_string(), "Level 1-3");
        assert_eq!(DelaunayValidationLevel::Four.to_string(), "Level 1-4");
    }

    #[test]
    fn validation_check_display_covers_all_categories() {
        assert_eq!(CdtValidationCheck::Geometry.to_string(), "geometry");
        assert_eq!(
            CdtValidationCheck::FoliationAssignment.to_string(),
            "foliation_assignment"
        );
        assert_eq!(CdtValidationCheck::Causality.to_string(), "causality");
        assert_eq!(
            CdtValidationCheck::CellClassification.to_string(),
            "cell_classification"
        );
        assert_eq!(
            CdtValidationCheck::ErgodicMoveCandidateGeometry.to_string(),
            "ergodic_move_candidate_geometry"
        );
    }

    #[test]
    fn checkpoint_resume_reason_display_covers_all_categories() {
        let cases = [
            (
                CheckpointResumeReason::StepCountOverflow,
                "step count overflow",
            ),
            (
                CheckpointResumeReason::CheckpointTargetConfiguration,
                "checkpoint target configuration",
            ),
            (
                CheckpointResumeReason::McmcChainRestore,
                "mcmc chain restore",
            ),
            (
                CheckpointResumeReason::TriangulationInvariants,
                "triangulation invariants",
            ),
            (CheckpointResumeReason::ActionMismatch, "action mismatch"),
            (
                CheckpointResumeReason::IncompatibleActionConfiguration,
                "incompatible action configuration",
            ),
            (
                CheckpointResumeReason::IncompatibleTemperature,
                "incompatible temperature",
            ),
            (
                CheckpointResumeReason::IncompatibleThermalizationSchedule,
                "incompatible thermalization schedule",
            ),
            (
                CheckpointResumeReason::IncompatibleMeasurementFrequency,
                "incompatible measurement frequency",
            ),
            (
                CheckpointResumeReason::CheckpointConfiguration,
                "checkpoint configuration",
            ),
            (
                CheckpointResumeReason::CheckpointActionConfiguration,
                "checkpoint action configuration",
            ),
            (
                CheckpointResumeReason::ChainCounterMismatch,
                "chain counter mismatch",
            ),
            (
                CheckpointResumeReason::ChainStepMismatch,
                "chain step mismatch",
            ),
            (
                CheckpointResumeReason::StepTelemetryMismatch,
                "step telemetry mismatch",
            ),
            (
                CheckpointResumeReason::StepTelemetryOverflow,
                "step telemetry overflow",
            ),
            (
                CheckpointResumeReason::MeasurementTelemetryOverflow,
                "measurement telemetry overflow",
            ),
            (
                CheckpointResumeReason::MeasurementTelemetryMismatch,
                "measurement telemetry mismatch",
            ),
            (
                CheckpointResumeReason::MoveStatisticsInvariant,
                "move statistics invariant",
            ),
            (
                CheckpointResumeReason::CounterConversionOverflow,
                "counter conversion overflow",
            ),
        ];

        for (reason, expected) in cases {
            assert_eq!(reason.to_string(), expected);
        }
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
            check: CdtValidationCheck::Geometry,
            detail: "backend reported invalid triangulation structure".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Validation failed [geometry]: backend reported invalid triangulation structure"
        );
    }

    #[test]
    fn test_topology_mismatch_error() {
        let error = CdtError::TopologyMismatch {
            topology: CdtTopology::Toroidal,
            euler_characteristic: 1,
            expected_euler_characteristics: vec![0],
            vertices: 3,
            edges: 3,
            faces: 1,
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Topology mismatch for toroidal: Euler characteristic χ=1, expected one of [0] (V=3, E=3, F=1)"
        );
    }

    #[test]
    fn test_foliation_error_variant() {
        let error = CdtError::Foliation(FoliationError::EmptySlice { slice: 3 });
        let display = format!("{error}");
        assert_eq!(
            display,
            "Foliation validation failed: time slice 3 is empty"
        );
    }

    #[test]
    fn test_vertex_build_failed_error() {
        let error = CdtError::VertexBuildFailed {
            context: "explicit CDT vertex 7".to_string(),
            underlying_error: "Missing required field: `point`".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Vertex construction failed [explicit CDT vertex 7]: Missing required field: `point`"
        );
    }

    #[test]
    fn test_backend_rollback_failed_error() {
        let error = CdtError::BackendRollbackFailed {
            operation: "set_vertex_data_by_key".to_string(),
            target: "vertex VertexKey(123v1)".to_string(),
            detail: "backend reported invalid vertex key".to_string(),
            rollback_errors: "vertex VertexKey(7v1): backend reported invalid vertex key"
                .to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Backend mutation failed [set_vertex_data_by_key] on vertex VertexKey(123v1): backend reported invalid vertex key; rollback failed: vertex VertexKey(7v1): backend reported invalid vertex key"
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
    fn test_unsupported_operation_error() {
        let error = CdtError::UnsupportedOperation {
            operation: "MetropolisAlgorithm::run".to_string(),
            reason: "real CDT ergodic moves are not implemented yet".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Unsupported operation [MetropolisAlgorithm::run]: real CDT ergodic moves are not implemented yet"
        );
    }

    #[test]
    fn test_metropolis_move_application_failed_error() {
        let error = CdtError::MetropolisMoveApplicationFailed {
            step: 17,
            move_type: MoveType::Move31Remove,
            attempts: 8,
            last_failure: "no geometrically valid candidate site found".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Metropolis accepted Move31Remove at step 17, but applying it failed after 8 attempts; last failure: no geometrically valid candidate site found"
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
    fn test_output_write_failed_error() {
        let error = CdtError::OutputWriteFailed {
            path: "measurements.csv".to_string(),
            format: "CSV".to_string(),
            detail: "permission denied".to_string(),
        };
        let CdtError::OutputWriteFailed {
            path,
            format,
            detail,
        } = &error
        else {
            panic!("expected OutputWriteFailed variant");
        };
        assert_eq!(path, "measurements.csv");
        assert_eq!(format, "CSV");
        assert_eq!(detail, "permission denied");
        let display = format!("{error}");
        assert_eq!(
            display,
            "Failed to write CSV output to measurements.csv: permission denied"
        );
    }

    #[test]
    fn test_output_path_resolution_failed_error() {
        let error = CdtError::OutputPathResolutionFailed {
            base_path: ".".to_string(),
            detail: "No such file or directory".to_string(),
        };
        let CdtError::OutputPathResolutionFailed { base_path, detail } = &error else {
            panic!("expected OutputPathResolutionFailed variant");
        };
        assert_eq!(base_path, ".");
        assert_eq!(detail, "No such file or directory");
        let display = format!("{error}");
        assert_eq!(
            display,
            "Failed to resolve output path from base .: No such file or directory"
        );
    }

    #[test]
    fn test_output_path_conflict_error() {
        let error = CdtError::OutputPathConflict {
            csv_path: "output/results".to_string(),
            json_path: "output/results".to_string(),
        };
        let CdtError::OutputPathConflict {
            csv_path,
            json_path,
        } = &error
        else {
            panic!("expected OutputPathConflict variant");
        };
        assert_eq!(csv_path, "output/results");
        assert_eq!(json_path, "output/results");
        assert_eq!(
            format!("{error}"),
            "CSV output path output/results and JSON output path output/results resolve to the same file"
        );
    }

    #[test]
    fn test_output_read_failed_error() {
        let error = CdtError::OutputReadFailed {
            path: "summary.json".to_string(),
            format: "JSON".to_string(),
            detail: "expected value at line 1 column 1".to_string(),
        };
        let CdtError::OutputReadFailed {
            path,
            format,
            detail,
        } = &error
        else {
            panic!("expected OutputReadFailed variant");
        };
        assert_eq!(path, "summary.json");
        assert_eq!(format, "JSON");
        assert_eq!(detail, "expected value at line 1 column 1");
        let display = format!("{error}");
        assert_eq!(
            display,
            "Failed to read JSON output from summary.json: expected value at line 1 column 1"
        );
    }

    #[test]
    fn test_checkpoint_serialization_failed_error() {
        let error = CdtError::CheckpointSerializationFailed {
            operation: "deserialize".to_string(),
            target: "final triangulation".to_string(),
            detail: "missing field `geometry`".to_string(),
        };
        let CdtError::CheckpointSerializationFailed {
            operation,
            target,
            detail,
        } = &error
        else {
            panic!("expected CheckpointSerializationFailed variant");
        };
        assert_eq!(operation, "deserialize");
        assert_eq!(target, "final triangulation");
        assert_eq!(detail, "missing field `geometry`");
        let display = format!("{error}");
        assert_eq!(
            display,
            "Failed to deserialize final triangulation checkpoint: missing field `geometry`"
        );
    }

    #[test]
    fn test_checkpoint_resume_failed_error() {
        let error = CdtError::CheckpointResumeFailed {
            reason: CheckpointResumeReason::IncompatibleTemperature,
            detail: "temperature differs from checkpoint".to_string(),
        };
        let CdtError::CheckpointResumeFailed { reason, detail } = &error else {
            panic!("expected CheckpointResumeFailed variant");
        };
        assert_eq!(*reason, CheckpointResumeReason::IncompatibleTemperature);
        assert_eq!(detail, "temperature differs from checkpoint");
        assert_eq!(
            format!("{error}"),
            "Failed to resume MCMC checkpoint [incompatible temperature]: temperature differs from checkpoint"
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
        let error = CdtError::InvalidConfiguration {
            setting: "temperature".to_string(),
            provided_value: "NaN".to_string(),
            expected: "finite and positive".to_string(),
        };
        let _: &dyn Error = &error;
        // If this compiles, the trait is implemented correctly
    }
}
