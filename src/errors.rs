#![forbid(unsafe_code)]

//! Error types for the CDT library.

use crate::cdt::ergodic_moves::MoveType;
use crate::cdt::foliation::FoliationError;
use crate::config::CdtTopology;
use markov_chain_monte_carlo::McmcError;
use std::fmt;

/// Highest cumulative upstream Delaunay validation level being enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
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

/// Identifies the top-level or simulation configuration setting that failed validation.
///
/// Use this with [`CdtError::InvalidConfiguration`] and
/// [`CdtError::InvalidSimulationConfiguration`] to inspect invalid settings
/// without parsing rendered error messages.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{CdtError, ConfigurationSetting};
/// use causal_triangulations::prelude::CdtConfig;
/// use std::assert_matches;
///
/// let config = CdtConfig {
///     vertices: 2,
///     ..CdtConfig::new(36, 3)
/// };
/// assert_matches!(
///     config.into_validated(),
///     Err(CdtError::InvalidConfiguration {
///         setting: ConfigurationSetting::Vertices,
///         ..
///     })
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConfigurationSetting {
    /// CDT dimensionality setting.
    Dimension,
    /// Total vertex count setting.
    Vertices,
    /// Number of time slices setting.
    Timeslices,
    /// Metropolis temperature setting.
    Temperature,
    /// Number of Metropolis steps setting.
    Steps,
    /// Number of thermalization steps setting.
    ThermalizationSteps,
    /// Measurement cadence setting.
    MeasurementFrequency,
    /// Combined measurement schedule constraint.
    MeasurementSchedule,
    /// Bare inverse Newton coupling setting.
    Coupling0,
    /// Curvature coupling setting.
    Coupling2,
    /// Cosmological constant setting.
    CosmologicalConstant,
    /// Explicit per-slice volume profile setting.
    VolumeProfile,
}

impl fmt::Display for ConfigurationSetting {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimension => formatter.write_str("dimension"),
            Self::Vertices => formatter.write_str("vertices"),
            Self::Timeslices => formatter.write_str("timeslices"),
            Self::Temperature => formatter.write_str("temperature"),
            Self::Steps => formatter.write_str("steps"),
            Self::ThermalizationSteps => formatter.write_str("thermalization_steps"),
            Self::MeasurementFrequency => formatter.write_str("measurement_frequency"),
            Self::MeasurementSchedule => formatter.write_str("measurement schedule"),
            Self::Coupling0 => formatter.write_str("coupling_0"),
            Self::Coupling2 => formatter.write_str("coupling_2"),
            Self::CosmologicalConstant => formatter.write_str("cosmological_constant"),
            Self::VolumeProfile => formatter.write_str("volume_profile"),
        }
    }
}

/// Identifies the issue category for invalid triangulation generation parameters.
///
/// Use this with [`CdtError::InvalidGenerationParameters`] when a constructor
/// rejects input before attempting triangulation.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{CdtError, GenerationParameterIssue};
/// use causal_triangulations::prelude::triangulation::CdtTriangulation;
/// use std::assert_matches;
///
/// let err = CdtTriangulation::from_random_points(2, 2, 2)
///     .expect_err("fewer than three vertices cannot form a triangulation");
///
/// assert_matches!(
///     err,
///     CdtError::InvalidGenerationParameters {
///         issue: GenerationParameterIssue::InsufficientVertexCount,
///         ..
///     }
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GenerationParameterIssue {
    /// Coordinate range has invalid ordering or bounds.
    InvalidCoordinateRange,
    /// Toroidal domain extents are invalid.
    InvalidToroidalDomain,
    /// A supplied vertex coordinate is NaN or infinite.
    NonFiniteVertexCoordinate,
    /// Total vertex count is below the constructor minimum.
    InsufficientVertexCount,
    /// Per-slice vertex count is below the constructor minimum.
    InsufficientVerticesPerSlice,
    /// Number of time slices is below the topology minimum.
    InsufficientNumberOfTimeSlices,
    /// A slice count was zero where at least one slice is required.
    NonPositiveSliceCount,
    /// Explicit volume profile has no slices.
    EmptyVolumeProfile,
    /// Explicit volume profile length cannot fit in supported counters.
    VolumeProfileLengthOverflow,
    /// A volume-profile slice has too few vertices.
    InsufficientVerticesInVolumeProfileSlice,
    /// Total vertex count cannot fit in supported counters.
    VertexCountOverflow,
    /// Simplex count cannot fit in supported counters.
    SimplexCountOverflow,
}

impl fmt::Display for GenerationParameterIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCoordinateRange => formatter.write_str("Invalid coordinate range"),
            Self::InvalidToroidalDomain => formatter.write_str("Invalid toroidal domain"),
            Self::NonFiniteVertexCoordinate => formatter.write_str("Non-finite vertex coordinate"),
            Self::InsufficientVertexCount => formatter.write_str("Insufficient vertex count"),
            Self::InsufficientVerticesPerSlice => {
                formatter.write_str("Insufficient vertices per slice")
            }
            Self::InsufficientNumberOfTimeSlices => {
                formatter.write_str("Insufficient number of time slices")
            }
            Self::NonPositiveSliceCount => formatter.write_str("Number of slices must be positive"),
            Self::EmptyVolumeProfile => formatter.write_str("Empty volume profile"),
            Self::VolumeProfileLengthOverflow => {
                formatter.write_str("Volume profile length overflow")
            }
            Self::InsufficientVerticesInVolumeProfileSlice => {
                formatter.write_str("Insufficient vertices in volume-profile slice")
            }
            Self::VertexCountOverflow => formatter.write_str("Vertex count overflow"),
            Self::SimplexCountOverflow => formatter.write_str("Simplex count overflow"),
        }
    }
}

/// Identifies the CDT triangulation metadata field that failed validation.
///
/// Use this with [`CdtError::InvalidTriangulationMetadata`] to distinguish
/// invalid metadata fields without relying on display text.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{CdtError, TriangulationMetadataField};
/// use causal_triangulations::prelude::triangulation::CdtTopology;
/// use std::assert_matches;
///
/// let metadata_error = CdtError::InvalidTriangulationMetadata {
///     field: TriangulationMetadataField::Timeslices,
///     topology: CdtTopology::Toroidal,
///     provided_value: "2".to_string(),
///     expected: "at least three time slices".to_string(),
/// };
///
/// assert_matches!(
///     metadata_error,
///     CdtError::InvalidTriangulationMetadata {
///         field: TriangulationMetadataField::Timeslices,
///         ..
///     }
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TriangulationMetadataField {
    /// Number of time slices recorded in triangulation metadata.
    Timeslices,
    /// Dimensionality recorded in triangulation metadata.
    Dimension,
}

impl fmt::Display for TriangulationMetadataField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeslices => formatter.write_str("timeslices"),
            Self::Dimension => formatter.write_str("dimension"),
        }
    }
}

/// Simulation output format for typed output read/write failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutputFormat {
    /// Comma-separated measurement output.
    Csv,
    /// JSON simulation summary output.
    Json,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Csv => formatter.write_str("CSV"),
            Self::Json => formatter.write_str("JSON"),
        }
    }
}

/// Checkpoint serialization operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CheckpointOperation {
    /// Serializing a checkpoint or checkpoint-like payload failed.
    Serialize,
    /// Deserializing a checkpoint or checkpoint-like payload failed.
    Deserialize,
}

impl fmt::Display for CheckpointOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize => formatter.write_str("serialize"),
            Self::Deserialize => formatter.write_str("deserialize"),
        }
    }
}

/// Backend mutation operation that failed while editing a CDT triangulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BackendMutationOperation {
    /// Write simplex payload by backend simplex key.
    SetSimplexDataByKey,
    /// Write vertex payload by backend vertex key.
    SetVertexDataByKey,
    /// Write vertex payload through a vertex handle.
    SetVertexData,
    /// Subdivide a face as part of a local move.
    SubdivideFace,
    /// Remove a vertex as part of a local move.
    RemoveVertex,
    /// Flip an edge as part of a local move.
    FlipEdge,
}

impl fmt::Display for BackendMutationOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetSimplexDataByKey => formatter.write_str("set_simplex_data_by_key"),
            Self::SetVertexDataByKey => formatter.write_str("set_vertex_data_by_key"),
            Self::SetVertexData => formatter.write_str("set_vertex_data"),
            Self::SubdivideFace => formatter.write_str("subdivide_face"),
            Self::RemoveVertex => formatter.write_str("remove_vertex"),
            Self::FlipEdge => formatter.write_str("flip_edge"),
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
    /// Strict CDT simplex classification failed.
    SimplexClassification,
    /// Local ergodic move candidate geometry could not be interpreted.
    ErgodicMoveCandidateGeometry,
}

impl fmt::Display for CdtValidationCheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry => formatter.write_str("geometry"),
            Self::FoliationAssignment => formatter.write_str("foliation_assignment"),
            Self::Causality => formatter.write_str("causality"),
            Self::SimplexClassification => formatter.write_str("simplex_classification"),
            Self::ErgodicMoveCandidateGeometry => {
                formatter.write_str("ergodic_move_candidate_geometry")
            }
        }
    }
}

/// Structured detail for crate-owned CDT validation failures.
///
/// This refines [`CdtError::ValidationFailed`] beyond a coarse
/// [`CdtValidationCheck`] so callers can inspect common CDT invariant failures
/// without parsing display text. Variants still keep string diagnostics where
/// the source is an upstream backend message or an opaque backend handle.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::CdtValidationFailure;
///
/// let failure = CdtValidationFailure::InvalidCdtTriangle {
///     face: "FaceKey(3v1)".to_string(),
///     spacelike_edges: 3,
///     timelike_edges: 0,
/// };
///
/// assert!(format!("{failure}").contains("spacelike=3"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CdtValidationFailure {
    /// Generic backend geometry validation failed with an upstream diagnostic.
    BackendGeometry {
        /// Upstream geometry validation diagnostic.
        detail: String,
    },
    /// Face vertices could not be resolved through the geometry backend.
    FaceVerticesUnavailable {
        /// Face being validated.
        face: String,
        /// Lower-level face-vertex lookup diagnostic.
        detail: String,
    },
    /// A face had the wrong number of vertices for a CDT triangle.
    FaceVertexCount {
        /// Face being validated.
        face: String,
        /// Number of vertices observed.
        actual: usize,
        /// Number of vertices expected.
        expected: usize,
    },
    /// A vertex in a foliated triangulation was missing its time label.
    MissingVertexTimeLabel {
        /// Vertex missing its time label.
        vertex: String,
    },
    /// A triangle had the wrong spacelike/timelike edge pattern.
    InvalidCdtTriangle {
        /// Face being validated.
        face: String,
        /// Number of spacelike edges observed.
        spacelike_edges: u8,
        /// Number of timelike edges observed.
        timelike_edges: u8,
    },
    /// Coordinate lookup failed while assigning foliation labels.
    VertexCoordinateReadFailed {
        /// Vertex whose coordinates could not be read.
        vertex: String,
        /// Lower-level coordinate lookup diagnostic.
        detail: String,
    },
    /// A vertex coordinate did not have enough dimensions for foliation assignment.
    VertexCoordinateDimension {
        /// Vertex whose coordinate dimensionality was invalid.
        vertex: String,
        /// Number of coordinates observed.
        actual: usize,
        /// Minimum number of coordinates expected.
        expected_minimum: usize,
    },
    /// A foliated face was not classifiable as a strict Up or Down CDT simplex.
    NonStrictSimplex {
        /// Face being classified.
        face: String,
    },
    /// Local ergodic-move candidate geometry failed a post-mutation invariant.
    ErgodicMoveCandidateGeometry {
        /// Diagnostic for the failed local candidate.
        detail: String,
    },
}

impl fmt::Display for CdtValidationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendGeometry { detail } | Self::ErgodicMoveCandidateGeometry { detail } => {
                formatter.write_str(detail)
            }
            Self::FaceVerticesUnavailable { face, detail } => {
                write!(
                    formatter,
                    "failed to resolve vertices for face {face}: {detail}"
                )
            }
            Self::FaceVertexCount {
                face,
                actual,
                expected,
            } => write!(
                formatter,
                "face {face} has {actual} vertices, expected {expected}"
            ),
            Self::MissingVertexTimeLabel { vertex } => write!(
                formatter,
                "vertex {vertex} has no time label in a foliated triangulation"
            ),
            Self::InvalidCdtTriangle {
                face,
                spacelike_edges,
                timelike_edges,
            } => write!(
                formatter,
                "invalid CDT triangle at face {face}: spacelike={spacelike_edges}, timelike={timelike_edges}"
            ),
            Self::VertexCoordinateReadFailed { vertex, detail } => {
                write!(
                    formatter,
                    "failed to read coordinates for vertex {vertex}: {detail}"
                )
            }
            Self::VertexCoordinateDimension {
                vertex,
                actual,
                expected_minimum,
            } => write!(
                formatter,
                "vertex {vertex} has {actual} coordinates, expected ≥ {expected_minimum}"
            ),
            Self::NonStrictSimplex { face } => write!(
                formatter,
                "face {face} is not a strict CDT simplex (expected Up or Down)"
            ),
        }
    }
}

/// Move-statistics counter category used in checkpoint resume diagnostics.
///
/// Use this with [`CheckpointResumeFailure::MoveCounterOverflow`] and
/// [`CheckpointResumeFailure::CounterConversionOverflow`] to distinguish which
/// aggregated counter could not be represented without parsing display text.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::CheckpointMoveCounter;
///
/// assert_eq!(CheckpointMoveCounter::Attempted.to_string(), "attempted");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CheckpointMoveCounter {
    /// Attempted move counter.
    Attempted,
    /// Accepted move counter.
    Accepted,
    /// Rejected move counter.
    Rejected,
}

impl fmt::Display for CheckpointMoveCounter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attempted => formatter.write_str("attempted"),
            Self::Accepted => formatter.write_str("accepted"),
            Self::Rejected => formatter.write_str("rejected"),
        }
    }
}

/// Structured reason a CDT checkpoint could not be resumed.
///
/// [`CdtError::CheckpointResumeFailed`] wraps this enum for CDT-owned resume
/// invariants such as incompatible schedules, inconsistent telemetry, and
/// overflow while rebuilding upstream MCMC counters. Upstream framework,
/// configuration, and triangulation validation failures remain separate
/// [`CdtError`] variants so callers can keep matching the original typed error.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{
///     CdtError, CheckpointResumeFailure,
/// };
/// use std::assert_matches;
///
/// let err = CdtError::CheckpointResumeFailed {
///     failure: CheckpointResumeFailure::IncompatibleTemperature,
/// };
///
/// assert_matches!(
///     err,
///     CdtError::CheckpointResumeFailed {
///         failure: CheckpointResumeFailure::IncompatibleTemperature,
///     }
/// );
/// ```
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckpointResumeFailure {
    /// Resumed step count would overflow.
    #[error("resumed step count exceeds u32::MAX")]
    StepCountOverflow,
    /// Stored action disagrees with recomputed action.
    #[error("checkpoint action mismatch: stored {stored}, recomputed {recomputed}")]
    ActionMismatch {
        /// Serialized action stored in the checkpoint.
        stored: f64,
        /// Action recomputed from the restored triangulation.
        recomputed: f64,
    },
    /// Action configuration differs from the checkpoint.
    #[error("action configuration differs from checkpoint")]
    IncompatibleActionConfiguration,
    /// Temperature differs from the checkpoint.
    #[error("temperature differs from checkpoint")]
    IncompatibleTemperature,
    /// Thermalization schedule differs from the checkpoint.
    #[error("thermalization schedule differs from checkpoint")]
    IncompatibleThermalizationSchedule,
    /// Measurement frequency differs from the checkpoint.
    #[error("measurement frequency differs from checkpoint")]
    IncompatibleMeasurementFrequency,
    /// Generic MCMC chain counters disagree with CDT move statistics.
    #[error(
        "chain counters do not match move statistics: chain accepted={chain_accepted}, rejected={chain_rejected}; move accepted={move_accepted}, rejected={move_rejected}"
    )]
    ChainCounterMismatch {
        /// Accepted proposals recorded by the upstream MCMC chain.
        chain_accepted: usize,
        /// Rejected proposals recorded by the upstream MCMC chain.
        chain_rejected: usize,
        /// Accepted proposals reconstructed from CDT move statistics.
        move_accepted: usize,
        /// Rejected proposals reconstructed from CDT move statistics.
        move_rejected: usize,
    },
    /// Generic MCMC chain step count disagrees with checkpoint step.
    #[error(
        "chain step count does not match checkpoint step: chain steps={chain_steps}, checkpoint step={checkpoint_step}"
    )]
    ChainStepMismatch {
        /// Total steps recorded by the upstream MCMC chain.
        chain_steps: usize,
        /// Current CDT step stored in the checkpoint.
        checkpoint_step: u32,
    },
    /// Step telemetry length disagrees with the chain step count.
    #[error("step telemetry length mismatch: got {actual}, expected {expected}")]
    StepTelemetryLengthMismatch {
        /// Number of serialized step records.
        actual: usize,
        /// Expected number of step records from the chain counters.
        expected: usize,
    },
    /// Accepted-step telemetry count disagrees with the chain accepted count.
    #[error("accepted step count mismatch: got {actual}, expected {expected}")]
    StepTelemetryAcceptedCountMismatch {
        /// Accepted steps recorded in CDT telemetry.
        actual: usize,
        /// Accepted proposals recorded by the upstream MCMC chain.
        expected: usize,
    },
    /// Step telemetry index conversion overflowed.
    #[error("step telemetry index exceeds u32::MAX")]
    StepTelemetryIndexOverflow,
    /// Step telemetry records are not sequential.
    #[error("step telemetry must be sequential: got step {actual}, expected {expected}")]
    StepTelemetrySequenceMismatch {
        /// Serialized step value.
        actual: u32,
        /// Expected sequential step value.
        expected: u32,
    },
    /// Step telemetry contains a non-finite pre-move action.
    #[error("step {step} has non-finite action_before")]
    NonFiniteStepActionBefore {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Step telemetry contains a non-finite action delta.
    #[error("step {step} has non-finite delta_action")]
    NonFiniteStepDeltaAction {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Accepted step telemetry is missing the action delta.
    #[error("accepted step {step} is missing delta_action")]
    AcceptedStepMissingDeltaAction {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Accepted step telemetry has an action-after value inconsistent with the delta.
    #[error("step {step} action_after does not match delta_action")]
    StepActionAfterDeltaMismatch {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Accepted step telemetry contains a non-finite post-move action.
    #[error("step {step} has non-finite action_after")]
    NonFiniteStepActionAfter {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Accepted step telemetry is missing the post-move action.
    #[error("accepted step {step} is missing action_after")]
    AcceptedStepMissingActionAfter {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Rejected step telemetry unexpectedly contains a post-move action.
    #[error("rejected step {step} unexpectedly has action_after")]
    RejectedStepHasActionAfter {
        /// Step with invalid telemetry.
        step: u32,
    },
    /// Measurement count calculation overflowed.
    #[error("scheduled measurement count exceeds usize::MAX")]
    MeasurementCountOverflow,
    /// Measurement telemetry length disagrees with the configured schedule.
    #[error("scheduled measurement count mismatch: got {actual}, expected {expected}")]
    MeasurementCountMismatch {
        /// Number of serialized measurements.
        actual: usize,
        /// Expected measurement count from the sampling schedule.
        expected: usize,
    },
    /// Measurement step calculation overflowed.
    #[error("scheduled measurement step exceeds u32::MAX")]
    MeasurementStepOverflow,
    /// Measurement telemetry step disagrees with the configured schedule.
    #[error("measurement telemetry step mismatch: got {actual}, expected {expected}")]
    MeasurementStepMismatch {
        /// Serialized measurement step.
        actual: u32,
        /// Expected measurement step from the sampling schedule.
        expected: u32,
    },
    /// Measurement telemetry contains a non-finite action.
    #[error("measurement at step {step} has non-finite action")]
    NonFiniteMeasurementAction {
        /// Measurement step with invalid telemetry.
        step: u32,
    },
    /// A per-move counter sum overflowed.
    #[error("{counter} move count exceeds u64::MAX")]
    MoveCounterOverflow {
        /// Counter category that overflowed.
        counter: CheckpointMoveCounter,
    },
    /// Resumable checkpoints cannot contain hard-failure move counters.
    #[error("{move_type:?} hard-failure move count must be zero in resumable checkpoints")]
    MoveHardFailures {
        /// Move type with invalid hard-failure telemetry.
        move_type: MoveType,
    },
    /// Accepted move counter exceeds attempted move counter.
    #[error("{move_type:?} accepted move count exceeds attempted move count")]
    MoveAcceptedExceedsAttempted {
        /// Move type with impossible move telemetry.
        move_type: MoveType,
    },
    /// Total accepted move count exceeds total attempted move count.
    #[error("accepted move count exceeds attempted move count")]
    TotalAcceptedExceedsAttempted,
    /// Accepted or rejected counter conversion overflowed.
    #[error("{counter} move count exceeds usize::MAX")]
    CounterConversionOverflow {
        /// Counter category that could not fit in upstream chain counters.
        counter: CheckpointMoveCounter,
    },
}

/// Lower-level source for a Metropolis-accepted move that could not be applied.
///
/// [`CdtError::MetropolisMoveApplicationFailed`] uses this enum to preserve the
/// category and structured context of a hard failure after Metropolis has
/// accepted a move type. It is intentionally smaller than recursively storing a
/// full [`CdtError`] while still giving callers typed branches for backend,
/// validation, topology, foliation, and causality failures.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{
///     BackendMutationOperation, MetropolisMoveApplicationFailure,
/// };
///
/// let failure = MetropolisMoveApplicationFailure::BackendMutation {
///     operation: BackendMutationOperation::RemoveVertex,
///     target: "vertex VertexKey(7v1)".to_string(),
///     detail: "backend reported invalid vertex key".to_string(),
/// };
///
/// assert!(format!("{failure}").contains("remove_vertex"));
/// ```
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum MetropolisMoveApplicationFailure {
    /// A backend payload or topology edit failed while applying the accepted move.
    #[error("backend mutation failed [{operation}] on {target}: {detail}")]
    BackendMutation {
        /// Mutation operation being attempted.
        operation: BackendMutationOperation,
        /// Human-readable target handle.
        target: String,
        /// Additional failure detail.
        detail: String,
    },
    /// A backend mutation failed, then rollback of staged payloads also failed.
    #[error(
        "backend mutation failed [{operation}] on {target}: {detail}; rollback failed: {rollback_errors}"
    )]
    BackendRollback {
        /// Mutation operation being attempted when the first failure occurred.
        operation: BackendMutationOperation,
        /// Human-readable target handle for the first failure.
        target: String,
        /// Primary mutation failure detail.
        detail: String,
        /// Rollback failure details for one or more payloads.
        rollback_errors: String,
    },
    /// Upstream Delaunay validation rejected the evolved geometry.
    #[error("Delaunay validation failed [{level}]: {detail}")]
    DelaunayValidation {
        /// Cumulative upstream validation level being enforced.
        level: DelaunayValidationLevel,
        /// Upstream validation diagnostic.
        detail: String,
    },
    /// CDT validation rejected the evolved triangulation.
    #[error("validation failed [{check}]: {failure}")]
    Validation {
        /// Validation check that failed.
        check: CdtValidationCheck,
        /// Structured validation failure detail.
        failure: CdtValidationFailure,
    },
    /// Topology metadata did not match the evolved backend Euler characteristic.
    #[error(
        "topology mismatch for {topology}: Euler characteristic χ={euler_characteristic}, expected one of {expected_euler_characteristics:?} (V={vertices}, E={edges}, F={faces})"
    )]
    TopologyMismatch {
        /// Topology requested by CDT metadata.
        topology: CdtTopology,
        /// Observed Euler characteristic from the backend.
        euler_characteristic: i128,
        /// Accepted Euler characteristics for the requested topology.
        expected_euler_characteristics: Vec<i128>,
        /// Backend vertex count at validation time.
        vertices: usize,
        /// Backend edge count at validation time.
        edges: usize,
        /// Backend face count at validation time.
        faces: usize,
    },
    /// Foliation bookkeeping or validation failed.
    #[error("foliation validation failed: {0}")]
    Foliation(FoliationError),
    /// A post-mutation edge violated CDT causality.
    #[error("{}", format_causality_violation(*time_0, *time_1, *step_distance))]
    CausalityViolation {
        /// Time label of the first endpoint.
        time_0: u32,
        /// Time label of the second endpoint.
        time_1: u32,
        /// Topology-aware temporal step distance between the two labels.
        step_distance: u32,
    },
    /// A hard failure reached the Metropolis boundary through an unexpected error category.
    #[error("unexpected accepted-move failure: {detail}")]
    Unexpected {
        /// Lower-level error text retained for diagnostics.
        detail: String,
    },
}

impl From<CdtError> for MetropolisMoveApplicationFailure {
    fn from(error: CdtError) -> Self {
        match error {
            CdtError::BackendMutationFailed {
                operation,
                target,
                detail,
            } => Self::BackendMutation {
                operation,
                target,
                detail,
            },
            CdtError::BackendRollbackFailed {
                operation,
                target,
                detail,
                rollback_errors,
            } => Self::BackendRollback {
                operation,
                target,
                detail,
                rollback_errors,
            },
            CdtError::DelaunayValidationFailed { level, detail } => {
                Self::DelaunayValidation { level, detail }
            }
            CdtError::ValidationFailed { check, failure } => Self::Validation { check, failure },
            CdtError::TopologyMismatch {
                topology,
                euler_characteristic,
                expected_euler_characteristics,
                vertices,
                edges,
                faces,
            } => Self::TopologyMismatch {
                topology,
                euler_characteristic,
                expected_euler_characteristics,
                vertices,
                edges,
                faces,
            },
            CdtError::Foliation(error) => Self::Foliation(error),
            CdtError::CausalityViolation {
                time_0,
                time_1,
                step_distance,
            } => Self::CausalityViolation {
                time_0,
                time_1,
                step_distance,
            },
            CdtError::MetropolisMoveApplicationFailed { source, .. }
            | CdtError::ProposalApplicationFailed { source, .. } => source,
            unexpected @ (CdtError::UnsupportedDimension(_)
            | CdtError::DelaunayGenerationFailed { .. }
            | CdtError::InvalidGenerationParameters { .. }
            | CdtError::InvalidConfiguration { .. }
            | CdtError::InvalidSimulationConfiguration { .. }
            | CdtError::InvalidTriangulationMetadata { .. }
            | CdtError::VertexBuildFailed { .. }
            | CdtError::Mcmc(_)
            | CdtError::OutputWriteFailed { .. }
            | CdtError::OutputPathResolutionFailed { .. }
            | CdtError::OutputPathConflict { .. }
            | CdtError::OutputReadFailed { .. }
            | CdtError::CheckpointSerializationFailed { .. }
            | CdtError::CheckpointResumeFailed { .. }) => Self::Unexpected {
                detail: unexpected.to_string(),
            },
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
        /// Structured category for the rejected generation parameter.
        issue: GenerationParameterIssue,
        /// The actual value that was provided
        provided_value: String,
        /// The expected range or constraint for the parameter
        expected_range: String,
    },
    /// Top-level CDT configuration failed validation.
    #[error("Invalid configuration: {setting} (got: {provided_value}, expected: {expected})")]
    InvalidConfiguration {
        /// Structured category for the invalid configuration setting.
        setting: ConfigurationSetting,
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
        /// Structured category for the invalid simulation setting.
        setting: ConfigurationSetting,
        /// Value supplied for the setting.
        provided_value: String,
        /// Expected constraint for the setting.
        expected: String,
    },
    /// Metropolis accepted a move, but a hard backend or invariant failure stopped application.
    ///
    /// The [`Self::MetropolisMoveApplicationFailed::source`] field keeps the
    /// lower-level failure category as [`MetropolisMoveApplicationFailure`] so
    /// callers can distinguish backend mutation, validation, topology,
    /// foliation, and causality failures without parsing the rendered message.
    #[error(
        "Metropolis accepted {move_type:?} at step {step}, but applying it failed after {attempts} attempts; source: {source}"
    )]
    MetropolisMoveApplicationFailed {
        /// Monte Carlo step whose accepted move could not be applied.
        step: u32,
        /// Accepted move type being applied.
        move_type: MoveType,
        /// Number of application attempts made before failing.
        attempts: usize,
        /// Most specific lower-level rejection or failure observed.
        source: MetropolisMoveApplicationFailure,
    },
    /// Planning or committing a standalone delayed CDT proposal hit a hard failure.
    #[error("CDT proposal failed while applying {move_type:?} on attempt {attempt}: {source}")]
    ProposalApplicationFailed {
        /// Move type whose concrete proposal application failed.
        move_type: MoveType,
        /// Local-site attempt that hit the hard failure.
        attempt: usize,
        /// Most specific lower-level rejection or failure observed.
        source: MetropolisMoveApplicationFailure,
    },
    /// Constructed triangulation metadata is internally inconsistent.
    #[error(
        "Invalid triangulation metadata: {field} for {topology} (got: {provided_value}, expected: {expected})"
    )]
    InvalidTriangulationMetadata {
        /// Structured category for the invalid metadata field.
        field: TriangulationMetadataField,
        /// Topology whose invariant was violated.
        topology: CdtTopology,
        /// Value stored in the triangulation metadata.
        provided_value: String,
        /// Expected constraint for the metadata field.
        expected: String,
    },
    /// Validation of a constructed triangulation failed.
    ///
    /// The [`Self::ValidationFailed::check`] field identifies the broad
    /// validation phase, while [`Self::ValidationFailed::failure`] carries the
    /// typed invariant failure within that phase.
    #[error("Validation failed [{check}]: {failure}")]
    ValidationFailed {
        /// Validation check that failed.
        check: CdtValidationCheck,
        /// Structured validation failure detail.
        failure: CdtValidationFailure,
    },
    /// Topology metadata does not match the backend Euler characteristic.
    #[error(
        "Topology mismatch for {topology}: Euler characteristic χ={euler_characteristic}, expected one of {expected_euler_characteristics:?} (V={vertices}, E={edges}, F={faces})"
    )]
    TopologyMismatch {
        /// Topology requested by CDT metadata.
        topology: CdtTopology,
        /// Observed Euler characteristic from the backend.
        euler_characteristic: i128,
        /// Accepted Euler characteristics for the requested topology.
        expected_euler_characteristics: Vec<i128>,
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
        operation: BackendMutationOperation,
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
        operation: BackendMutationOperation,
        /// Human-readable target handle for the first failure.
        target: String,
        /// Primary mutation failure detail.
        detail: String,
        /// Rollback failure details for one or more payloads.
        rollback_errors: String,
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
    /// Upstream MCMC framework error, such as a non-finite log-probability.
    #[error("MCMC error: {0}")]
    Mcmc(#[from] McmcError),
    /// Writing CSV/JSON simulation output failed.
    #[error("Failed to write {format} output to {path}: {detail}")]
    OutputWriteFailed {
        /// Target output path.
        path: String,
        /// Output format being written.
        format: OutputFormat,
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
        format: OutputFormat,
        /// Lower-level I/O or decoding error.
        detail: String,
    },
    /// Serializing or deserializing a CDT or MCMC checkpoint failed.
    #[error("Failed to {operation} {target} checkpoint: {detail}")]
    CheckpointSerializationFailed {
        /// Checkpoint operation being attempted.
        operation: CheckpointOperation,
        /// Human-readable checkpoint target, such as "final triangulation".
        target: String,
        /// Lower-level serialization error.
        detail: String,
    },
    /// Restoring or continuing an MCMC checkpoint failed before sampling resumed.
    ///
    /// The [`CheckpointResumeFailure`] source is reserved for CDT-owned
    /// resume invariants. Upstream MCMC, configuration, and triangulation
    /// validation errors are reported through their more specific variants.
    #[error("Failed to resume MCMC checkpoint: {failure}")]
    CheckpointResumeFailed {
        /// Structured resume failure with typed context.
        #[source]
        failure: CheckpointResumeFailure,
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

/// Result type for CDT operations.
pub type CdtResult<T> = Result<T, CdtError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;
    use std::error::Error;

    #[test]
    fn test_invalid_configuration_error() {
        let error = CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::Vertices,
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
            setting: ConfigurationSetting::Temperature,
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
            field: TriangulationMetadataField::Timeslices,
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
            CdtValidationCheck::SimplexClassification.to_string(),
            "simplex_classification"
        );
        assert_eq!(
            CdtValidationCheck::ErgodicMoveCandidateGeometry.to_string(),
            "ergodic_move_candidate_geometry"
        );
    }

    #[test]
    fn configuration_setting_display_covers_all_settings() {
        let cases = [
            (ConfigurationSetting::Dimension, "dimension"),
            (ConfigurationSetting::Vertices, "vertices"),
            (ConfigurationSetting::Timeslices, "timeslices"),
            (ConfigurationSetting::Temperature, "temperature"),
            (ConfigurationSetting::Steps, "steps"),
            (
                ConfigurationSetting::ThermalizationSteps,
                "thermalization_steps",
            ),
            (
                ConfigurationSetting::MeasurementFrequency,
                "measurement_frequency",
            ),
            (
                ConfigurationSetting::MeasurementSchedule,
                "measurement schedule",
            ),
            (ConfigurationSetting::Coupling0, "coupling_0"),
            (ConfigurationSetting::Coupling2, "coupling_2"),
            (
                ConfigurationSetting::CosmologicalConstant,
                "cosmological_constant",
            ),
            (ConfigurationSetting::VolumeProfile, "volume_profile"),
        ];

        for (setting, expected) in cases {
            assert_eq!(setting.to_string(), expected);
        }
    }

    #[test]
    fn generation_parameter_issue_display_covers_all_issues() {
        let cases = [
            (
                GenerationParameterIssue::InvalidCoordinateRange,
                "Invalid coordinate range",
            ),
            (
                GenerationParameterIssue::InvalidToroidalDomain,
                "Invalid toroidal domain",
            ),
            (
                GenerationParameterIssue::NonFiniteVertexCoordinate,
                "Non-finite vertex coordinate",
            ),
            (
                GenerationParameterIssue::InsufficientVertexCount,
                "Insufficient vertex count",
            ),
            (
                GenerationParameterIssue::InsufficientVerticesPerSlice,
                "Insufficient vertices per slice",
            ),
            (
                GenerationParameterIssue::InsufficientNumberOfTimeSlices,
                "Insufficient number of time slices",
            ),
            (
                GenerationParameterIssue::NonPositiveSliceCount,
                "Number of slices must be positive",
            ),
            (
                GenerationParameterIssue::EmptyVolumeProfile,
                "Empty volume profile",
            ),
            (
                GenerationParameterIssue::VolumeProfileLengthOverflow,
                "Volume profile length overflow",
            ),
            (
                GenerationParameterIssue::InsufficientVerticesInVolumeProfileSlice,
                "Insufficient vertices in volume-profile slice",
            ),
            (
                GenerationParameterIssue::VertexCountOverflow,
                "Vertex count overflow",
            ),
            (
                GenerationParameterIssue::SimplexCountOverflow,
                "Simplex count overflow",
            ),
        ];

        for (issue, expected) in cases {
            assert_eq!(issue.to_string(), expected);
        }
    }

    #[test]
    fn triangulation_metadata_field_display_covers_all_fields() {
        assert_eq!(
            TriangulationMetadataField::Timeslices.to_string(),
            "timeslices"
        );
        assert_eq!(
            TriangulationMetadataField::Dimension.to_string(),
            "dimension"
        );
    }

    #[test]
    fn output_format_display_covers_all_formats() {
        assert_eq!(OutputFormat::Csv.to_string(), "CSV");
        assert_eq!(OutputFormat::Json.to_string(), "JSON");
    }

    #[test]
    fn checkpoint_operation_display_covers_all_operations() {
        assert_eq!(CheckpointOperation::Serialize.to_string(), "serialize");
        assert_eq!(CheckpointOperation::Deserialize.to_string(), "deserialize");
    }

    #[test]
    fn backend_mutation_operation_display_covers_all_operations() {
        let cases = [
            (
                BackendMutationOperation::SetSimplexDataByKey,
                "set_simplex_data_by_key",
            ),
            (
                BackendMutationOperation::SetVertexDataByKey,
                "set_vertex_data_by_key",
            ),
            (BackendMutationOperation::SetVertexData, "set_vertex_data"),
            (BackendMutationOperation::SubdivideFace, "subdivide_face"),
            (BackendMutationOperation::RemoveVertex, "remove_vertex"),
            (BackendMutationOperation::FlipEdge, "flip_edge"),
        ];

        for (operation, expected) in cases {
            assert_eq!(operation.to_string(), expected);
        }
    }

    #[test]
    fn checkpoint_move_counter_display_covers_all_categories() {
        let cases = [
            (CheckpointMoveCounter::Attempted, "attempted"),
            (CheckpointMoveCounter::Accepted, "accepted"),
            (CheckpointMoveCounter::Rejected, "rejected"),
        ];

        for (counter, expected) in cases {
            assert_eq!(counter.to_string(), expected);
        }
    }

    #[test]
    fn checkpoint_resume_failure_display_includes_structured_context() {
        let failure = CheckpointResumeFailure::ChainCounterMismatch {
            chain_accepted: 1,
            chain_rejected: 2,
            move_accepted: 3,
            move_rejected: 4,
        };

        assert_eq!(
            failure.to_string(),
            "chain counters do not match move statistics: chain accepted=1, rejected=2; move accepted=3, rejected=4"
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
            issue: GenerationParameterIssue::InsufficientVertexCount,
            provided_value: "2".to_string(),
            expected_range: "at least 3".to_string(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Invalid triangulation parameters: Insufficient vertex count (got: 2, expected: at least 3)"
        );
    }

    #[test]
    fn test_validation_failed_error() {
        let error = CdtError::ValidationFailed {
            check: CdtValidationCheck::Geometry,
            failure: CdtValidationFailure::BackendGeometry {
                detail: "backend reported invalid triangulation structure".to_string(),
            },
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Validation failed [geometry]: backend reported invalid triangulation structure"
        );
    }

    #[test]
    fn cdt_validation_failure_display_covers_structured_variants() {
        let cases = [
            (
                CdtValidationFailure::BackendGeometry {
                    detail: "backend rejected structure".to_string(),
                },
                "backend rejected structure",
            ),
            (
                CdtValidationFailure::FaceVerticesUnavailable {
                    face: "FaceKey(3v1)".to_string(),
                    detail: "backend reported invalid simplex key".to_string(),
                },
                "failed to resolve vertices for face FaceKey(3v1): backend reported invalid simplex key",
            ),
            (
                CdtValidationFailure::FaceVertexCount {
                    face: "FaceKey(3v1)".to_string(),
                    actual: 4,
                    expected: 3,
                },
                "face FaceKey(3v1) has 4 vertices, expected 3",
            ),
            (
                CdtValidationFailure::MissingVertexTimeLabel {
                    vertex: "VertexKey(7v1)".to_string(),
                },
                "vertex VertexKey(7v1) has no time label in a foliated triangulation",
            ),
            (
                CdtValidationFailure::InvalidCdtTriangle {
                    face: "FaceKey(3v1)".to_string(),
                    spacelike_edges: 3,
                    timelike_edges: 0,
                },
                "invalid CDT triangle at face FaceKey(3v1): spacelike=3, timelike=0",
            ),
            (
                CdtValidationFailure::VertexCoordinateReadFailed {
                    vertex: "VertexKey(7v1)".to_string(),
                    detail: "missing vertex".to_string(),
                },
                "failed to read coordinates for vertex VertexKey(7v1): missing vertex",
            ),
            (
                CdtValidationFailure::VertexCoordinateDimension {
                    vertex: "VertexKey(7v1)".to_string(),
                    actual: 1,
                    expected_minimum: 2,
                },
                "vertex VertexKey(7v1) has 1 coordinates, expected ≥ 2",
            ),
            (
                CdtValidationFailure::NonStrictSimplex {
                    face: "FaceKey(3v1)".to_string(),
                },
                "face FaceKey(3v1) is not a strict CDT simplex (expected Up or Down)",
            ),
            (
                CdtValidationFailure::ErgodicMoveCandidateGeometry {
                    detail: "candidate edge has no adjacent faces".to_string(),
                },
                "candidate edge has no adjacent faces",
            ),
        ];

        for (failure, expected) in cases {
            assert_eq!(failure.to_string(), expected);
        }
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
            operation: BackendMutationOperation::SetVertexDataByKey,
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
            operation: BackendMutationOperation::SetVertexData,
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
    fn test_metropolis_move_application_failed_error() {
        let source = MetropolisMoveApplicationFailure::BackendMutation {
            operation: BackendMutationOperation::SetVertexData,
            target: "vertex VertexKey(123v1)".to_string(),
            detail: "backend reported invalid vertex key".to_string(),
        };
        let error = CdtError::MetropolisMoveApplicationFailed {
            step: 17,
            move_type: MoveType::Move31Remove,
            attempts: 8,
            source: source.clone(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "Metropolis accepted Move31Remove at step 17, but applying it failed after 8 attempts; source: backend mutation failed [set_vertex_data] on vertex VertexKey(123v1): backend reported invalid vertex key"
        );
        assert_eq!(
            Error::source(&error).map(ToString::to_string),
            Some(source.to_string())
        );
    }

    #[test]
    fn test_proposal_application_failed_error() {
        let source = MetropolisMoveApplicationFailure::BackendMutation {
            operation: BackendMutationOperation::SetVertexDataByKey,
            target: "vertex VertexKey(123v1)".to_string(),
            detail: "backend reported invalid vertex key".to_string(),
        };
        let error = CdtError::ProposalApplicationFailed {
            move_type: MoveType::Move13Add,
            attempt: 2,
            source: source.clone(),
        };
        let display = format!("{error}");
        assert_eq!(
            display,
            "CDT proposal failed while applying Move13Add on attempt 2: backend mutation failed [set_vertex_data_by_key] on vertex VertexKey(123v1): backend reported invalid vertex key"
        );
        assert_eq!(
            Error::source(&error).map(ToString::to_string),
            Some(source.to_string())
        );
    }

    #[test]
    fn metropolis_move_application_failure_preserves_backend_mutation_fields() {
        let failure = MetropolisMoveApplicationFailure::from(CdtError::BackendMutationFailed {
            operation: BackendMutationOperation::RemoveVertex,
            target: "vertex VertexKey(7v1)".to_string(),
            detail: "backend reported invalid vertex key".to_string(),
        });

        let MetropolisMoveApplicationFailure::BackendMutation {
            operation,
            target,
            detail,
        } = failure
        else {
            panic!("expected backend mutation failure source");
        };

        assert_eq!(operation, BackendMutationOperation::RemoveVertex);
        assert_eq!(target, "vertex VertexKey(7v1)");
        assert_eq!(detail, "backend reported invalid vertex key");
    }

    #[test]
    fn metropolis_move_application_failure_preserves_validation_fields() {
        let validation_failure = CdtValidationFailure::InvalidCdtTriangle {
            face: "FaceKey(3v1)".to_string(),
            spacelike_edges: 3,
            timelike_edges: 0,
        };
        let failure = MetropolisMoveApplicationFailure::from(CdtError::ValidationFailed {
            check: CdtValidationCheck::Causality,
            failure: validation_failure.clone(),
        });

        let MetropolisMoveApplicationFailure::Validation { check, failure } = failure else {
            panic!("expected validation failure source");
        };

        assert_eq!(check, CdtValidationCheck::Causality);
        assert_eq!(failure, validation_failure);
    }

    #[test]
    fn metropolis_move_application_failure_preserves_structured_sources() {
        let cases = [
            (
                CdtError::BackendRollbackFailed {
                    operation: BackendMutationOperation::FlipEdge,
                    target: "edge EdgeKey(5v1)".to_string(),
                    detail: "flip failed".to_string(),
                    rollback_errors: "rollback failed".to_string(),
                },
                MetropolisMoveApplicationFailure::BackendRollback {
                    operation: BackendMutationOperation::FlipEdge,
                    target: "edge EdgeKey(5v1)".to_string(),
                    detail: "flip failed".to_string(),
                    rollback_errors: "rollback failed".to_string(),
                },
            ),
            (
                CdtError::DelaunayValidationFailed {
                    level: DelaunayValidationLevel::Three,
                    detail: "invalid triangulation".to_string(),
                },
                MetropolisMoveApplicationFailure::DelaunayValidation {
                    level: DelaunayValidationLevel::Three,
                    detail: "invalid triangulation".to_string(),
                },
            ),
            (
                CdtError::TopologyMismatch {
                    topology: CdtTopology::Toroidal,
                    euler_characteristic: 1,
                    expected_euler_characteristics: vec![0],
                    vertices: 3,
                    edges: 3,
                    faces: 1,
                },
                MetropolisMoveApplicationFailure::TopologyMismatch {
                    topology: CdtTopology::Toroidal,
                    euler_characteristic: 1,
                    expected_euler_characteristics: vec![0],
                    vertices: 3,
                    edges: 3,
                    faces: 1,
                },
            ),
            (
                CdtError::Foliation(FoliationError::EmptySlice { slice: 3 }),
                MetropolisMoveApplicationFailure::Foliation(FoliationError::EmptySlice {
                    slice: 3,
                }),
            ),
            (
                CdtError::CausalityViolation {
                    time_0: 0,
                    time_1: 2,
                    step_distance: 2,
                },
                MetropolisMoveApplicationFailure::CausalityViolation {
                    time_0: 0,
                    time_1: 2,
                    step_distance: 2,
                },
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(MetropolisMoveApplicationFailure::from(error), expected);
        }
    }

    #[test]
    fn metropolis_move_application_failure_from_wrapper_preserves_source() {
        let source = MetropolisMoveApplicationFailure::BackendMutation {
            operation: BackendMutationOperation::RemoveVertex,
            target: "vertex VertexKey(7v1)".to_string(),
            detail: "backend reported invalid vertex key".to_string(),
        };
        let failure =
            MetropolisMoveApplicationFailure::from(CdtError::MetropolisMoveApplicationFailed {
                step: 17,
                move_type: MoveType::Move31Remove,
                attempts: 8,
                source: source.clone(),
            });

        assert_eq!(failure, source);
    }

    #[test]
    fn metropolis_move_application_failure_unexpected_retains_diagnostic() {
        let failure = MetropolisMoveApplicationFailure::from(CdtError::UnsupportedDimension(3));

        let MetropolisMoveApplicationFailure::Unexpected { detail } = failure else {
            panic!("expected unexpected failure source");
        };

        assert_eq!(
            detail,
            "Unsupported dimension: 3. Only 2D is currently supported"
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
        let error = CdtError::Mcmc(McmcError::NanProposedLogProb);
        let display = format!("{error}");
        assert_eq!(
            display,
            "MCMC error: target returned NaN log-probability for a proposed state"
        );
    }

    #[test]
    fn test_mcmc_error_from_conversion() {
        let mcmc_err = McmcError::NanProposedLogProb;
        let cdt_err: CdtError = mcmc_err.into();
        assert_matches!(cdt_err, CdtError::Mcmc(McmcError::NanProposedLogProb));
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
            format: OutputFormat::Csv,
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
        assert_eq!(*format, OutputFormat::Csv);
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
            format: OutputFormat::Json,
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
        assert_eq!(*format, OutputFormat::Json);
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
            operation: CheckpointOperation::Deserialize,
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
        assert_eq!(*operation, CheckpointOperation::Deserialize);
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
            failure: CheckpointResumeFailure::IncompatibleTemperature,
        };
        let CdtError::CheckpointResumeFailed { failure } = &error else {
            panic!("expected CheckpointResumeFailed variant");
        };
        assert_eq!(*failure, CheckpointResumeFailure::IncompatibleTemperature);
        assert_eq!(
            format!("{error}"),
            "Failed to resume MCMC checkpoint: temperature differs from checkpoint"
        );
        assert_eq!(
            Error::source(&error).map(ToString::to_string),
            Some("temperature differs from checkpoint".to_string())
        );
    }

    #[test]
    fn test_error_equality() {
        let error1 = CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::Steps,
            provided_value: "0".to_string(),
            expected: "≥ 1".to_string(),
        };
        let error2 = CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::Steps,
            provided_value: "0".to_string(),
            expected: "≥ 1".to_string(),
        };
        let error3 = CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::Steps,
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
            setting: ConfigurationSetting::Vertices,
            provided_value: "2".to_string(),
            expected: "≥ 3".to_string(),
        };
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("InvalidConfiguration"));
        assert!(debug_str.contains("Vertices"));
    }

    #[test]
    fn test_cdt_result_type() {
        let success: CdtResult<i32> = Ok(42);
        let failure: CdtResult<i32> = Err(CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::Steps,
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
            setting: ConfigurationSetting::Temperature,
            provided_value: "NaN".to_string(),
            expected: "finite and positive".to_string(),
        };
        let _: &dyn Error = &error;
        // If this compiles, the trait is implemented correctly
    }
}
