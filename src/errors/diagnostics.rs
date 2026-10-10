#![forbid(unsafe_code)]

//! Typed instance values and constraints for validation diagnostics.

use crate::config::CdtTopology;
use std::fmt;

/// The original input rejected by a validator, separate from its error category.
///
/// Floating-point values retain their bits, including NaN payloads and signed
/// zero. Use predicates or [`f64::to_bits`] for those values rather than equality.
/// This type deliberately has no serialization format.
///
/// # Examples
///
/// ```
/// use causal_triangulations::{CdtConfig, CdtError, ConfigurationSetting,
///     ExpectedConstraint, ObservedValue};
/// use std::assert_matches;
///
/// let config = CdtConfig { vertices: 2, ..CdtConfig::new(36, 3) };
/// assert_matches!(config.into_validated(), Err(CdtError::InvalidConfiguration {
///     setting: ConfigurationSetting::Vertices,
///     provided_value: ObservedValue::Count(2),
///     expected: ExpectedConstraint::AtLeast { minimum: 3 },
/// }));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ObservedValue {
    /// An unsigned count or dimension, without narrowing platform-sized values.
    Count(u128),
    /// An unmodified floating-point input.
    Float(f64),
    /// Bounds supplied to a coordinate generator.
    CoordinateRange {
        /// Lower bound.
        min: f64,
        /// Upper bound.
        max: f64,
    },
    /// A coordinate and its location in the original input.
    VertexCoordinate {
        /// Zero-based vertex index.
        vertex_index: usize,
        /// Zero-based coordinate axis.
        axis: usize,
        /// Original coordinate.
        value: f64,
    },
    /// A rejected period in a toroidal domain.
    ToroidalPeriod {
        /// Zero-based domain axis.
        axis: usize,
        /// Original period.
        value: f64,
    },
    /// A domain rejected at the upstream geometry boundary.
    ToroidalDomain {
        /// Original periods in axis order.
        periods: [f64; 2],
        /// Opaque upstream diagnostic, not intended for machine parsing.
        detail: String,
    },
    /// Original per-slice vertex counts.
    SpatialVertexProfile(Vec<u32>),
    /// A rejected entry in a spatial vertex profile.
    ProfileSlice {
        /// Zero-based slice index.
        index: usize,
        /// Original vertex count.
        vertices: u32,
    },
    /// Operands of a count multiplication that overflowed.
    CountProduct {
        /// Left operand.
        left: u32,
        /// Right operand.
        right: u32,
    },
    /// Operands of a count addition that overflowed.
    CountSum {
        /// Left operand.
        left: u32,
        /// Right operand.
        right: u32,
    },
    /// Couplings whose joint arithmetic range was rejected.
    ActionCouplings {
        /// Bare inverse Newton coupling.
        coupling_0: f64,
        /// Curvature coupling.
        coupling_2: f64,
        /// Cosmological constant.
        cosmological_constant: f64,
    },
    /// Schedule whose combined settings cannot yield a measurement.
    MeasurementSchedule {
        /// Number of simulation steps.
        steps: u32,
        /// Number of thermalization steps.
        thermalization_steps: u32,
        /// Measurement cadence.
        measurement_frequency: u32,
    },
}

impl From<u32> for ObservedValue {
    fn from(value: u32) -> Self {
        Self::Count(u128::from(value))
    }
}

impl From<u8> for ObservedValue {
    fn from(value: u8) -> Self {
        Self::Count(u128::from(value))
    }
}

impl From<usize> for ObservedValue {
    fn from(value: usize) -> Self {
        Self::Count(value as u128)
    }
}

impl From<f64> for ObservedValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl fmt::Display for ObservedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(value) => value.fmt(f),
            Self::Float(value) => value.fmt(f),
            Self::CoordinateRange { min, max } => write!(f, "[{min}, {max}]"),
            Self::VertexCoordinate {
                vertex_index,
                axis,
                value,
            } => {
                write!(f, "vertex {vertex_index} axis {axis} = {value}")
            }
            Self::ToroidalPeriod { axis, value } => write!(f, "axis {axis} period {value}"),
            Self::ToroidalDomain { periods, detail } => write!(f, "{periods:?}: {detail}"),
            Self::SpatialVertexProfile(profile) => write!(f, "{profile:?}"),
            Self::ProfileSlice { index, vertices } => write!(f, "slice {index} has {vertices}"),
            Self::CountProduct { left, right } => write!(f, "{left} × {right}"),
            Self::CountSum { left, right } => write!(f, "{left} + {right}"),
            Self::ActionCouplings {
                coupling_0,
                coupling_2,
                cosmological_constant,
            } => {
                write!(f, "[{coupling_0}, {coupling_2}, {cosmological_constant}]")
            }
            Self::MeasurementSchedule {
                steps,
                thermalization_steps,
                measurement_frequency,
            } => {
                write!(
                    f,
                    "steps={steps}, thermalization_steps={thermalization_steps}, measurement_frequency={measurement_frequency}"
                )
            }
        }
    }
}

/// The concrete contract that a rejected input failed to satisfy.
///
/// These records describe the existing domain validators; they are not a
/// separate validation engine. Match the constraint with the error's category
/// and [`ObservedValue`] to choose a corrective action without parsing prose.
/// This type deliberately has no serialization format.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::{
///     CdtError, ConfigurationSetting, ExpectedConstraint, ObservedValue,
/// };
/// use causal_triangulations::prelude::simulation::MetropolisConfig;
/// use std::assert_matches;
///
/// assert_matches!(MetropolisConfig::new(1.0, 19, 15, 10),
///     Err(CdtError::InvalidSimulationConfiguration {
///         setting: ConfigurationSetting::MeasurementSchedule,
///         provided_value: ObservedValue::MeasurementSchedule {
///             steps: 19, thermalization_steps: 15, measurement_frequency: 10,
///         },
///         expected: ExpectedConstraint::PostThermalizationMeasurement,
///     }));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ExpectedConstraint {
    /// Inclusive count lower bound.
    AtLeast {
        /// Minimum allowed count.
        minimum: u128,
    },
    /// Exact required count or dimension.
    Exactly {
        /// Required value.
        value: u128,
    },
    /// Minimum number of slices required by a topology.
    AtLeastForTopology {
        /// Minimum allowed count.
        minimum: u32,
        /// Topology imposing the minimum.
        topology: CdtTopology,
    },
    /// Equal spatial slices must divide the total vertex count.
    DivisibleByTimeslices {
        /// Divisor.
        timeslices: u32,
        /// Topology of the slices.
        topology: CdtTopology,
    },
    /// Total vertices must cover the minimum size of every slice.
    MinimumTotalVertices {
        /// Minimum vertices per slice.
        vertices_per_slice: u32,
        /// Number of slices.
        timeslices: u32,
        /// Topology imposing the minimum.
        topology: CdtTopology,
    },
    /// The minimum total size must be representable before constructing slices.
    ProductFitsU32 {
        /// Multiplier applied to the observed count.
        factor: u32,
        /// Topology imposing the multiplier.
        topology: CdtTopology,
    },
    /// The count or arithmetic result must fit in `u32`.
    FitsU32,
    /// The count must fit in the platform's `usize`.
    FitsUsize,
    /// A profile must contain one entry per configured slice.
    ProfileLength {
        /// Configured slice count.
        timeslices: u32,
    },
    /// Every profile slice must satisfy the topology's minimum size.
    ProfileSliceMinimum {
        /// Minimum vertices per slice.
        minimum: u32,
        /// Topology imposing the minimum.
        topology: CdtTopology,
    },
    /// The sum of profile entries must fit in `u32`.
    ProfileSumFitsU32,
    /// The configured vertex count must equal the profile sum.
    ProfileSum {
        /// Sum of the profile entries.
        total: u32,
    },
    /// All intermediate checked open-strip face-count operations must fit `u32`.
    OpenStripFaceCountFitsU32,
    /// A floating-point value must be finite.
    Finite,
    /// Both coordinate bounds must be finite, with the minimum strictly smaller.
    FiniteIncreasingBounds,
    /// Each toroidal period must be finite and strictly positive.
    FinitePositivePeriods,
    /// Temperature must be finite, positive, and have a finite reciprocal.
    PositiveFiniteReciprocal,
    /// Combined couplings must keep the conservative action bound finite.
    FiniteAction {
        /// Largest representable simplex count used in the bound.
        maximum_simplex_count: usize,
    },
    /// Temperature must keep the conservative log-probability bound finite.
    FiniteLogProbability {
        /// Maximum action magnitude used by the target's existing check.
        maximum_action_magnitude: f64,
    },
    /// A schedule count cannot exceed the configured number of steps.
    AtMostSteps {
        /// Configured number of steps.
        steps: u32,
    },
    /// The schedule must yield at least one post-thermalization measurement.
    PostThermalizationMeasurement,
    /// Metadata dimension must agree with the geometry backend.
    BackendDimension {
        /// Actual backend dimension.
        dimension: usize,
    },
}

impl fmt::Display for ExpectedConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtLeast { minimum } => write!(f, "≥ {minimum}"),
            Self::Exactly { value } => value.fmt(f),
            Self::AtLeastForTopology { minimum, topology } => {
                write!(f, "≥ {minimum} for {topology} topology")
            }
            Self::DivisibleByTimeslices {
                timeslices,
                topology,
            } => write!(
                f,
                "divisible by timeslices ({timeslices}) for {topology} topology"
            ),
            Self::MinimumTotalVertices {
                vertices_per_slice,
                timeslices,
                topology,
            } => write!(
                f,
                "≥ {vertices_per_slice} · timeslices ({}) for {topology} topology",
                u64::from(*vertices_per_slice) * u64::from(*timeslices)
            ),
            Self::ProductFitsU32 { factor, topology } => write!(
                f,
                "{factor} · timeslices must fit in u32 for {topology} topology"
            ),
            Self::FitsU32 => f.write_str("must fit in u32"),
            Self::FitsUsize => f.write_str("must fit in usize"),
            Self::ProfileLength { timeslices } => {
                write!(f, "{timeslices} entries for configured timeslices")
            }
            Self::ProfileSliceMinimum { minimum, topology } => {
                write!(f, "each slice ≥ {minimum} for {topology} topology")
            }
            Self::ProfileSumFitsU32 => f.write_str("sum must fit in u32"),
            Self::ProfileSum { total } => write!(f, "sum of spatial_vertex_profile ({total})"),
            Self::OpenStripFaceCountFitsU32 => f.write_str("open-strip face count must fit in u32"),
            Self::Finite => f.write_str("finite"),
            Self::FiniteIncreasingBounds => f.write_str("finite min < max"),
            Self::FinitePositivePeriods => f.write_str("finite and positive periods"),
            Self::PositiveFiniteReciprocal => {
                f.write_str("finite and positive with a finite reciprocal")
            }
            Self::FiniteAction {
                maximum_simplex_count,
            } => write!(
                f,
                "joint magnitude that keeps action evaluation finite for simplex counts up to {maximum_simplex_count}"
            ),
            Self::FiniteLogProbability {
                maximum_action_magnitude,
            } => write!(
                f,
                "large enough to keep -action / temperature finite for action magnitude up to {maximum_action_magnitude}"
            ),
            Self::AtMostSteps { steps } => write!(f, "≤ steps ({steps})"),
            Self::PostThermalizationMeasurement => {
                f.write_str("at least one post-thermalization measurement")
            }
            Self::BackendDimension { dimension } => write!(f, "backend dimension ({dimension})"),
        }
    }
}
