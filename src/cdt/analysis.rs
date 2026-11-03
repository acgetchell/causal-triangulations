#![forbid(unsafe_code)]

//! Dependence-aware analysis of complete cadence-aligned periodic CDT chains.
//!
//! Generic estimator mathematics is owned by `markov-chain-monte-carlo`.
//! A diagnostic verdict establishes information adequacy only, never agreement
//! with a physical ensemble. Exact-volume goodness-of-fit belongs to #238.
//!
//! # Examples
//!
//! Export a complete short chain through the public runner and JSON schema.
//! Such a short run is an interface example, not convergence evidence.
//!
//! ```
//! use causal_triangulations::prelude::config::{CdtConfig, CdtTopology};
//! use causal_triangulations::prelude::observables::ValidationChain;
//! use causal_triangulations::{CdtResult, run_simulation};
//!
//! fn main() -> CdtResult<()> {
//!     let config = CdtConfig {
//!         topology: CdtTopology::Toroidal, simulate: true, seed: Some(7),
//!         steps: 4, thermalization_steps: 0, measurement_frequency: 1,
//!         ..CdtConfig::new(16, 4)
//!     }.into_validated()?;
//!     let results = run_simulation(&config)?;
//!     let chain = ValidationChain::from_results("example", "chain-7", &config, &results)?;
//!     let mut json = Vec::new();
//!     chain.write_json(&mut json)?;
//!     let restored = ValidationChain::read_json(json.as_slice())?;
//!     assert_eq!(restored.data().measurements.len(), 5);
//!     Ok(())
//! }
//! ```

use crate::cdt::metropolis::helpers::{actions_match, expected_measurement_step};
use crate::cdt::metropolis::{
    Autocorrelation, BinningAnalysis, CombinedRhat, DiagnosticTiming, EssEstimate, EssEstimator,
    FoldedRankNormalizedSplitRhat, IntegratedAutocorrelationTime, MeanMcse, QuantileMcse,
    RankNormalizedSplitRhat, TailEss,
};
use crate::cdt::results::{Measurement, MeasurementWire, SimulationResultsBackend};
use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
use crate::errors::{CdtError, CdtResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Debug;
use std::io::{Read, Write};

/// First public cadence-aligned chain interchange schema.
pub const VALIDATION_CHAIN_SCHEMA_VERSION: u32 = 1;

/// Stream operation retained when chain/report JSON transport fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnalysisArtifactOperation {
    /// Decode and parse a complete chain artifact.
    #[error("read JSON")]
    Read,
    /// Encode a chain or diagnostic report.
    #[error("write JSON")]
    Write,
}

/// Invalid chain metadata, cadence, selection, or diagnostic policy.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum CdtAnalysisError {
    /// R-hat and pooled ESS require multiple independent chains.
    #[error("need at least two independent chains, got {provided}")]
    InsufficientChains {
        /// Number of chains supplied.
        provided: usize,
    },
    /// A periodic closed 1+1 row must satisfy the Euler/incidence count identities.
    #[error(
        "invalid closed-1+1 counts in chain {chain}, row {row}: V={vertices}, E={edges}, F={triangles}"
    )]
    Counts {
        /// Chain identity.
        chain: String,
        /// Measurement row.
        row: usize,
        /// Vertex count.
        vertices: u32,
        /// Edge count.
        edges: u32,
        /// Triangle count.
        triangles: u32,
    },
    /// A consumer cannot silently interpret an unknown schema.
    #[error("unsupported CDT chain schema {version}")]
    Schema {
        /// Rejected version.
        version: u32,
    },
    /// Reproducible analysis needs explicit run/chain identity and seed.
    #[error("missing chain identity or explicit RNG seed")]
    MissingIdentity,
    /// The requested interchange contract describes periodic 1+1 only.
    #[error("chain artifacts require periodic 1+1 CDT")]
    UnsupportedTopology,
    /// Metadata must agree with the simulation that produced the samples.
    #[error("chain {chain} has incompatible configuration or sampling semantics")]
    IncompatibleCondition {
        /// Chain identifier.
        chain: String,
    },
    /// Independent-chain analysis must not accept the same chain twice.
    #[error("duplicate chain identity or seed: {chain}")]
    DuplicateChain {
        /// Repeated chain.
        chain: String,
    },
    /// Missing, reordered, extra, or filtered rows change dependence semantics.
    #[error("chain {chain} has invalid cadence at row {row}: step {actual}, expected {expected:?}")]
    Cadence {
        /// Chain identifier.
        chain: String,
        /// Row position.
        row: usize,
        /// Recorded proposal step.
        actual: u32,
        /// Required proposal step, if representable.
        expected: Option<u32>,
    },
    /// A recorded row must contain the requested physical observable.
    #[error("chain {chain} has an invalid spatial profile at row {row}")]
    Profile {
        /// Chain identifier.
        chain: String,
        /// Measurement row.
        row: usize,
    },
    /// Policies are predeclared finite, positive precision/information contracts.
    #[error("invalid diagnostic policy field {field}")]
    Policy {
        /// Field rejected before estimator computation.
        field: DiagnosticPolicyField,
    },
    /// Empty or out-of-range observable selections are not silent zero series.
    #[error("invalid diagnostic observable selection")]
    Observable,
    /// Encoding and decoding failures cannot produce an apparently complete chain.
    #[error("chain artifact failed to {operation}: {detail}")]
    Serialization {
        /// Operation that failed at the JSON/I/O boundary.
        operation: AnalysisArtifactOperation,
        /// Original serde/I/O diagnostic at the transport boundary.
        detail: String,
    },
}

/// Diagnostic configuration field rejected at the analysis boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiagnosticPolicyField {
    /// ACF lag cap.
    #[error("maximum_lag")]
    MaximumLag,
    /// Completed-block floor.
    #[error("minimum_completed_blocks")]
    MinimumCompletedBlocks,
    /// Adjacent binning levels required for a plateau.
    #[error("stabilization_levels")]
    StabilizationLevels,
    /// Relative plateau tolerance.
    #[error("binning_relative_tolerance")]
    BinningRelativeTolerance,
    /// Independent chain count.
    #[error("minimum_chains")]
    MinimumChains,
    /// Required effective sample size.
    #[error("minimum_ess")]
    MinimumEss,
    /// Mean precision in original observable units.
    #[error("maximum_mean_mcse")]
    MaximumMeanMcse,
    /// Predeclared quantile probability or precision.
    #[error("quantiles")]
    Quantiles,
}

/// Complete raw chain transport. Configuration preserves start profile and conventions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationChainData {
    /// Interchange schema, independent of crate version.
    pub schema_version: u32,
    /// Producer crate version.
    pub crate_version: String,
    /// Spatial topology identifier, fixed to `S1` in this schema.
    pub spatial_topology: String,
    /// Temporal boundary identifier, fixed to `periodic` in this schema.
    pub temporal_boundary: String,
    /// Minimum admissible spacelike links per slice, currently three.
    pub minimum_loop_length: u32,
    /// Physical action convention, independent of the optional volume bias.
    pub action_convention: String,
    /// Experiment/run identifier.
    pub run_id: String,
    /// Independent-chain identifier; chunks retain the same identity.
    pub chain_id: String,
    /// Complete input configuration, including explicit seed and initial profile.
    pub configuration: CdtConfig,
    /// Every post-warmup scheduled measurement, including repeated states.
    pub measurements: Vec<Measurement>,
}

/// Defers measurement validation until after JSON decoding to preserve typed errors.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidationChainWire {
    schema_version: u32,
    crate_version: String,
    spatial_topology: String,
    temporal_boundary: String,
    minimum_loop_length: u32,
    action_convention: String,
    run_id: String,
    chain_id: String,
    configuration: CdtConfig,
    measurements: Vec<MeasurementWire>,
}

impl TryFrom<ValidationChainWire> for ValidationChainData {
    type Error = CdtError;

    fn try_from(wire: ValidationChainWire) -> CdtResult<Self> {
        Ok(Self {
            schema_version: wire.schema_version,
            crate_version: wire.crate_version,
            spatial_topology: wire.spatial_topology,
            temporal_boundary: wire.temporal_boundary,
            minimum_loop_length: wire.minimum_loop_length,
            action_convention: wire.action_convention,
            run_id: wire.run_id,
            chain_id: wire.chain_id,
            configuration: wire.configuration,
            measurements: wire
                .measurements
                .into_iter()
                .map(Measurement::try_from)
                .collect::<CdtResult<_>>()?,
        })
    }
}

/// Parsed periodic 1+1 chain with complete, uniform measurement cadence.
///
/// `try_new` reparses configurations and checks shape, counts, action terms,
/// profile/slab identities, and the complete schedule. Serialized input cannot
/// bypass these checks. Seeds and identities are declarations of independence;
/// callers remain responsible for generating truly independent trajectories.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "ValidationChainData", into = "ValidationChainData")]
pub struct ValidationChain {
    data: ValidationChainData,
}

impl TryFrom<ValidationChainData> for ValidationChain {
    type Error = CdtError;

    fn try_from(data: ValidationChainData) -> CdtResult<Self> {
        Self::try_new(data)
    }
}

impl From<ValidationChain> for ValidationChainData {
    fn from(chain: ValidationChain) -> Self {
        chain.data
    }
}

/// Parses source identity, supported model conventions, and runtime configuration.
fn validated_chain_metadata(data: &ValidationChainData) -> CdtResult<ValidatedCdtConfig> {
    if data.schema_version != VALIDATION_CHAIN_SCHEMA_VERSION {
        return Err(CdtAnalysisError::Schema {
            version: data.schema_version,
        }
        .into());
    }
    if data.run_id.is_empty()
        || data.chain_id.is_empty()
        || data.crate_version.is_empty()
        || data.configuration.seed.is_none()
    {
        return Err(CdtAnalysisError::MissingIdentity.into());
    }
    let config = data.configuration.clone().into_validated()?;
    if data.spatial_topology != "S1"
        || data.temporal_boundary != "periodic"
        || data.minimum_loop_length != 3
        || data.action_convention != "-kappa_0*N0-kappa_2*N2+lambda_edge*N1"
    {
        return Err(CdtAnalysisError::IncompatibleCondition {
            chain: data.chain_id.clone(),
        }
        .into());
    }
    if config.config().topology != CdtTopology::Toroidal {
        return Err(CdtAnalysisError::UnsupportedTopology.into());
    }
    Ok(config)
}

impl ValidationChain {
    /// Parses a raw artifact into a complete cadence-aligned periodic chain.
    ///
    /// # Errors
    /// Rejects incompatible versions, configurations, actions, profiles, or cadence.
    pub fn try_new(data: ValidationChainData) -> CdtResult<Self> {
        let config = validated_chain_metadata(&data)?;
        let action = config.to_action_config();
        let schedule = config.to_metropolis_config();
        let mut next = expected_measurement_step(
            0,
            schedule.thermalization_steps(),
            schedule.measurement_frequency(),
        );
        for (row, measurement) in data.measurements.iter().enumerate() {
            if Some(measurement.step()) != next || measurement.step() > schedule.steps().get() {
                return Err(CdtAnalysisError::Cadence {
                    chain: data.chain_id.clone(),
                    row,
                    actual: measurement.step(),
                    expected: next,
                }
                .into());
            }
            let profile =
                measurement
                    .spatial_loop_lengths()
                    .ok_or_else(|| CdtAnalysisError::Profile {
                        chain: data.chain_id.clone(),
                        row,
                    })?;
            if 2 * u64::from(measurement.vertices().get())
                != u64::from(measurement.triangles().get())
                || 2 * u64::from(measurement.edges().get())
                    != 3 * u64::from(measurement.triangles().get())
            {
                return Err(CdtAnalysisError::Counts {
                    chain: data.chain_id.clone(),
                    row,
                    vertices: measurement.vertices().get(),
                    edges: measurement.edges().get(),
                    triangles: measurement.triangles().get(),
                }
                .into());
            }
            if profile.len() != usize::try_from(config.timeslices().get()).unwrap_or(usize::MAX)
                || profile.len() != measurement.slab_triangle_profile().len()
                || profile.iter().enumerate().any(|(slice, &length)| {
                    u64::from(length) + u64::from(profile[(slice + 1) % profile.len()])
                        != u64::from(measurement.slab_triangle_profile()[slice])
                })
            {
                return Err(CdtAnalysisError::Profile {
                    chain: data.chain_id.clone(),
                    row,
                }
                .into());
            }
            let physical = action.physical_action(
                measurement.vertices().get() as usize,
                measurement.edges().get() as usize,
                measurement.triangles().get() as usize,
            );
            let fixing = action.fixing_action(measurement.triangles().get() as usize);
            if !actions_match(physical, measurement.physical_action())
                || !actions_match(fixing, measurement.fixing_action())
                || !actions_match(physical + fixing, measurement.action())
            {
                return Err(CdtAnalysisError::IncompatibleCondition {
                    chain: data.chain_id.clone(),
                }
                .into());
            }
            next = expected_measurement_step(
                row + 1,
                schedule.thermalization_steps(),
                schedule.measurement_frequency(),
            );
        }
        if data.measurements.is_empty() || next.is_some_and(|step| step <= schedule.steps().get()) {
            return Err(CdtAnalysisError::Cadence {
                chain: data.chain_id.clone(),
                row: data.measurements.len(),
                actual: schedule.steps().get(),
                expected: next,
            }
            .into());
        }
        Ok(Self { data })
    }

    /// Exports canonical results and their declared initialization configuration.
    ///
    /// Resumed results supply the cumulative schedule and original seed. The
    /// configuration argument retains the original start-profile declaration.
    ///
    /// # Errors
    /// Rejects detached configuration or incomplete result telemetry.
    pub fn from_results(
        run_id: &str,
        chain_id: &str,
        configuration: &ValidatedCdtConfig,
        results: &SimulationResultsBackend,
    ) -> CdtResult<Self> {
        let mut configuration = configuration.config().clone();
        if configuration.clone().into_validated()?.to_action_config() != *results.action_config()
            || configuration.topology != results.triangulation().metadata().topology()
            || configuration.timeslices != results.triangulation().time_slices().get()
            || configuration.vertices as usize
                != results.triangulation().metadata().initial_vertex_count()
        {
            return Err(CdtAnalysisError::IncompatibleCondition {
                chain: chain_id.to_owned(),
            }
            .into());
        }
        configuration.steps = results.config().steps().get();
        configuration.temperature = results.config().temperature();
        configuration.thermalization_steps = results.config().thermalization_steps();
        configuration.measurement_frequency = results.config().measurement_frequency().get();
        configuration.seed = results.config().seed();
        Self::try_new(ValidationChainData {
            schema_version: VALIDATION_CHAIN_SCHEMA_VERSION,
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
            spatial_topology: "S1".to_owned(),
            temporal_boundary: "periodic".to_owned(),
            minimum_loop_length: 3,
            action_convention: "-kappa_0*N0-kappa_2*N2+lambda_edge*N1".to_owned(),
            run_id: run_id.to_owned(),
            chain_id: chain_id.to_owned(),
            configuration,
            measurements: results.measurements().to_vec(),
        })
    }

    /// Borrows the validated transport, including original chain metadata.
    #[must_use]
    pub const fn data(&self) -> &ValidationChainData {
        &self.data
    }

    /// Writes versioned JSON without requiring Python, Parquet, or a notebook.
    ///
    /// # Errors
    /// Preserves encoding/I/O failure as a typed chain transport error.
    pub fn write_json(&self, writer: impl Write) -> CdtResult<()> {
        serde_json::to_writer_pretty(writer, &self.data).map_err(|error| {
            CdtAnalysisError::Serialization {
                operation: AnalysisArtifactOperation::Write,
                detail: error.to_string(),
            }
            .into()
        })
    }

    /// Reads JSON and reparses the complete chain contract.
    ///
    /// # Errors
    /// Rejects malformed, incompatible, truncated, or invalid chain artifacts.
    /// JSON and I/O failures use [`CdtAnalysisError::Serialization`]; invalid
    /// measurement values retain their structured [`CdtError`] variants.
    pub fn read_json(reader: impl Read) -> CdtResult<Self> {
        let wire: ValidationChainWire =
            serde_json::from_reader(reader).map_err(|error| CdtAnalysisError::Serialization {
                operation: AnalysisArtifactOperation::Read,
                detail: error.to_string(),
            })?;
        Self::try_new(wire.try_into()?)
    }

    /// Selects exact target-volume observations offline, preserving original steps.
    ///
    /// This iterator is for conditional comparisons, not regular-cadence ACF
    /// input. Analyze full-cadence target/profile indicators for dependence.
    pub fn at_exact_triangles(&self, target: u32) -> impl Iterator<Item = &Measurement> {
        self.data
            .measurements
            .iter()
            .filter(move |measurement| measurement.triangles().get() == target)
    }
}

/// Scalar estimands derived from raw CDT measurements without changing cadence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "observable", rename_all = "kebab-case")]
pub enum CdtDiagnosticObservable {
    /// Combined action in action units.
    CombinedAction,
    /// Physical action in action units.
    PhysicalAction,
    /// Fixing action in action units.
    FixingAction,
    /// Total triangle volume.
    Triangles,
    /// Spacelike links in one fixed labeled slice.
    LoopLength {
        /// Slice index.
        slice: usize,
    },
    /// Largest spatial loop.
    MaximumLoopLength,
    /// Product for a second moment or covariance.
    LoopProduct {
        /// First slice.
        left: usize,
        /// Second slice.
        right: usize,
    },
    /// Full profile indicator at every scheduled measurement.
    ProfileIndicator {
        /// Exact labeled profile.
        profile: Vec<u32>,
    },
    /// Exact volume indicator at every scheduled measurement.
    TargetVolumeIndicator {
        /// Target triangle count.
        target: u32,
    },
    /// Joint profile/volume indicator for dependence-aware conditional ratios.
    TargetProfileIndicator {
        /// Target triangle count.
        target: u32,
        /// Exact labeled profile.
        profile: Vec<u32>,
    },
}

impl CdtDiagnosticObservable {
    /// Extracts one scalar with explicit bounds rather than inventing missing values.
    fn value(&self, measurement: &Measurement) -> CdtResult<f64> {
        let profile = measurement
            .spatial_loop_lengths()
            .ok_or(CdtAnalysisError::Observable)?;
        let at = |slice: usize| {
            profile
                .get(slice)
                .copied()
                .map(f64::from)
                .ok_or(CdtAnalysisError::Observable)
        };
        Ok(match self {
            Self::CombinedAction => measurement.action(),
            Self::PhysicalAction => measurement.physical_action(),
            Self::FixingAction => measurement.fixing_action(),
            Self::Triangles => f64::from(measurement.triangles().get()),
            Self::LoopLength { slice } => at(*slice)?,
            Self::MaximumLoopLength => f64::from(
                profile
                    .iter()
                    .copied()
                    .max()
                    .ok_or(CdtAnalysisError::Observable)?,
            ),
            Self::LoopProduct { left, right } => at(*left)? * at(*right)?,
            Self::ProfileIndicator { profile: expected } => {
                check_selected_profile(expected, profile.len())?;
                f64::from(u8::from(profile == expected))
            }
            Self::TargetVolumeIndicator { target } => {
                f64::from(u8::from(measurement.triangles().get() == *target))
            }
            Self::TargetProfileIndicator {
                target,
                profile: expected,
            } => {
                check_selected_profile(expected, profile.len())?;
                if expected.iter().map(|&value| u64::from(value)).sum::<u64>() * 2
                    != u64::from(*target)
                {
                    return Err(CdtAnalysisError::Observable.into());
                }
                f64::from(u8::from(
                    profile == expected && measurement.triangles().get() == *target,
                ))
            }
        })
    }
}

/// Rejects incompatible profile-indicator definitions before statistical analysis.
fn check_selected_profile(profile: &[u32], slices: usize) -> CdtResult<()> {
    if profile.len() != slices || profile.iter().any(|&length| length < 3) {
        return Err(CdtAnalysisError::Observable.into());
    }
    Ok(())
}

/// Predeclared quantile precision in original observable units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantileRequirement {
    /// Probability strictly inside `(0, 1)`.
    pub probability: f64,
    /// Largest acceptable quantile MCSE.
    pub maximum_mcse: f64,
}

/// Explicit analysis/precision policy, validated once at analysis entry.
///
/// There is no default effective-information or precision threshold. Release
/// analysis must predeclare these by estimand and use at least eight chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtDiagnosticPolicy {
    /// ACF lag cap; each trace uses at most `N - 1`.
    pub maximum_lag: usize,
    /// Minimum completed blocks at every selected stabilization level.
    pub minimum_completed_blocks: usize,
    /// Consecutive power-of-two binning levels needed to declare a plateau.
    pub stabilization_levels: usize,
    /// Maximum fractional change in blocked mean errors across the plateau.
    pub binning_relative_tolerance: f64,
    /// Whether unresolved binning prevents information adequacy.
    pub require_stable_binning: bool,
    /// Required independent chains, at least two; eight for the release matrix.
    pub minimum_chains: usize,
    /// Required mean, bulk, and both tail ESS values.
    pub minimum_ess: f64,
    /// Required mean MCSE in original observable units.
    pub maximum_mean_mcse: f64,
    /// Required quantile diagnostics; an empty list makes quantiles supplementary.
    pub quantiles: Vec<QuantileRequirement>,
}

impl CdtDiagnosticPolicy {
    /// Establishes finite predeclared analysis parameters before inspecting draws.
    fn validate(&self) -> CdtResult<()> {
        let invalid = if self.maximum_lag == 0 {
            Some(DiagnosticPolicyField::MaximumLag)
        } else if self.minimum_completed_blocks < 2 {
            Some(DiagnosticPolicyField::MinimumCompletedBlocks)
        } else if self.stabilization_levels < 2 {
            Some(DiagnosticPolicyField::StabilizationLevels)
        } else if !self.binning_relative_tolerance.is_finite()
            || self.binning_relative_tolerance < 0.0
        {
            Some(DiagnosticPolicyField::BinningRelativeTolerance)
        } else if self.minimum_chains < 2 {
            Some(DiagnosticPolicyField::MinimumChains)
        } else if !self.minimum_ess.is_finite() || self.minimum_ess <= 0.0 {
            Some(DiagnosticPolicyField::MinimumEss)
        } else if !self.maximum_mean_mcse.is_finite() || self.maximum_mean_mcse <= 0.0 {
            Some(DiagnosticPolicyField::MaximumMeanMcse)
        } else if self.quantiles.iter().any(|requirement| {
            !requirement.probability.is_finite()
                || !(0.0..1.0).contains(&requirement.probability)
                || requirement.probability == 0.0
                || !requirement.maximum_mcse.is_finite()
                || requirement.maximum_mcse <= 0.0
        }) {
            Some(DiagnosticPolicyField::Quantiles)
        } else {
            None
        };
        if let Some(field) = invalid {
            return Err(CdtAnalysisError::Policy { field }.into());
        }
        Ok(())
    }
}

/// An estimator result never substitutes a sentinel for unavailable information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "kebab-case")]
pub enum DiagnosticEstimate<T> {
    /// The requested estimator was available.
    Available(T),
    /// The upstream estimator or declared policy could not establish the quantity.
    Unavailable {
        /// Original upstream diagnostic, including structured debug fields.
        reason: String,
    },
}

impl<T> DiagnosticEstimate<T> {
    /// Borrows an available estimate without discarding an unavailable reason.
    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        match self {
            Self::Available(value) => Some(value),
            Self::Unavailable { .. } => None,
        }
    }
}

/// Preserves the upstream failure identity rather than converting it to zero.
fn estimate<T, E: Debug>(value: Result<T, E>) -> DiagnosticEstimate<T> {
    match value {
        Ok(value) => DiagnosticEstimate::Available(value),
        Err(error) => DiagnosticEstimate::Unavailable {
            reason: format!("{error:?}"),
        },
    }
}

/// One upstream completed-block binning level; errors are in original units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtBinningLevel {
    /// Number of original samples per block.
    pub block_size: usize,
    /// Completed block count, excluding the incomplete tail.
    pub completed_blocks: usize,
    /// Mean of completed block means.
    pub mean: f64,
    /// Sample variance of completed block means.
    pub block_mean_variance: Option<f64>,
    /// Standard error of the mean, distinct from physical spread.
    pub blocked_mean_error: Option<f64>,
}

/// Per-chain dependence analysis, never across concatenated chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtChainDependence {
    /// Source chain identity.
    pub chain_id: String,
    /// Number of original post-warmup scheduled measurements.
    pub sample_count: usize,
    /// All requested ACF coefficients, beginning at lag zero.
    pub autocorrelation: DiagnosticEstimate<Vec<f64>>,
    /// Integrated time in recorded-measurement intervals.
    pub integrated_time: DiagnosticEstimate<f64>,
    /// Largest lag retained by the upstream initial-positive/monotone window.
    pub integrated_time_window: Option<usize>,
    /// Single-chain raw-mean ESS, distinct from multi-chain rank ESS.
    pub mean_ess: DiagnosticEstimate<f64>,
    /// Upstream power-of-two completed-block hierarchy.
    pub binning: DiagnosticEstimate<Vec<CdtBinningLevel>>,
    /// Selected plateau level index, or an explicit unresolved result.
    pub selected_binning_level: DiagnosticEstimate<usize>,
}

/// Multi-chain ESS with the upstream sample/splitting/regularization contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtEssSummary {
    /// Estimator kind, including a quantile probability where applicable.
    pub estimator: String,
    /// Effective samples.
    pub effective_samples: f64,
    /// ESS divided by retained split-chain draw count.
    pub relative_ess: f64,
    /// Original draw count before odd-middle omission.
    pub original_samples: usize,
    /// Retained split-chain draw count.
    pub retained_samples: usize,
    /// Whether upstream regularized the estimate.
    pub regularized: bool,
}

/// Retains count and estimator metadata from the public upstream ESS value.
fn ess_summary(value: EssEstimate) -> CdtEssSummary {
    CdtEssSummary {
        estimator: format!("{:?}", value.estimator()),
        effective_samples: value.value(),
        relative_ess: value.relative(),
        original_samples: value.original_sample_count(),
        retained_samples: value.sample_count(),
        regularized: value.is_regularized(),
    }
}

/// Original-unit Monte Carlo uncertainty with its matching ESS definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtMcseSummary {
    /// Original-unit standard error.
    pub standard_error: f64,
    /// Matching raw mean or quantile-indicator ESS.
    pub ess: CdtEssSummary,
    /// Type-7 sample quantile, when estimating a quantile.
    pub quantile: Option<f64>,
    /// Upstream order-statistic uncertainty interval; absent for means.
    pub interval: Option<[f64; 2]>,
}

/// Quantile diagnostic retaining probability and unsupported/tied-data outcomes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtQuantileDiagnostic {
    /// Predeclared quantile probability.
    pub probability: f64,
    /// Quantile MCSE and matching indicator ESS.
    pub mcse: DiagnosticEstimate<CdtMcseSummary>,
}

/// Information/convergence outcome, distinct from a physics-validation verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CdtDiagnosticVerdict {
    /// Required diagnostics and predeclared precision/information floors succeed.
    SufficientInformation,
    /// Available combined R-hat exceeds 1.01.
    NotConverged,
    /// A required quantity is unavailable or information/precision is inadequate.
    Inconclusive,
}

/// Versioned analysis report; per-chain and pooled estimators remain distinct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtDiagnosticReport {
    /// Report schema version.
    pub schema_version: u32,
    /// Producer version.
    pub crate_version: String,
    /// Generic estimator API identity; actual resolution is retained in Cargo.lock.
    pub upstream_estimator_contract: String,
    /// Source metadata without duplicating canonical raw measurement storage.
    pub sources: Vec<CdtDiagnosticSource>,
    /// Selected estimand and its original-unit interpretation.
    pub observable: CdtDiagnosticObservable,
    /// Original observable units; mean errors and MCSE retain these units.
    pub observable_units: String,
    /// Predeclared precision and binning policy.
    pub policy: CdtDiagnosticPolicy,
    /// Per-chain ACF, integrated time, and binning evidence.
    pub dependence: Vec<CdtChainDependence>,
    /// Location-sensitive pooled-rank split component.
    pub rank_normalized_rhat: DiagnosticEstimate<f64>,
    /// Scale-sensitive folded pooled-rank split component.
    pub folded_rhat: DiagnosticEstimate<f64>,
    /// Maximum of both available components; unavailable if either is unavailable.
    pub combined_rhat: DiagnosticEstimate<f64>,
    /// Raw-scale mean ESS.
    pub mean_ess: DiagnosticEstimate<CdtEssSummary>,
    /// Rank-normalized bulk ESS.
    pub bulk_ess: DiagnosticEstimate<CdtEssSummary>,
    /// Lower 0.05 quantile-indicator ESS.
    pub lower_tail_ess: DiagnosticEstimate<CdtEssSummary>,
    /// Upper 0.95 quantile-indicator ESS.
    pub upper_tail_ess: DiagnosticEstimate<CdtEssSummary>,
    /// Original-unit mean uncertainty using raw-scale mean ESS.
    pub mean_mcse: DiagnosticEstimate<CdtMcseSummary>,
    /// Requested quantile uncertainties; collapsed intervals stay unavailable.
    pub quantiles: Vec<CdtQuantileDiagnostic>,
    /// Mean ESS per actual workload wall-clock second, supplementary to correctness.
    pub mean_ess_per_second: DiagnosticEstimate<f64>,
    /// Caller-supplied post-warmup workload wall time, excluding serialization/setup.
    pub timed_seconds: Option<f64>,
    /// Original chain count asserted by optional production timing metadata.
    pub timed_chain_count: Option<usize>,
    /// Original samples per chain asserted by optional production timing metadata.
    pub timed_samples_per_chain: Option<usize>,
    /// Information adequacy only; exact ensemble agreement is evaluated separately.
    pub verdict: CdtDiagnosticVerdict,
}

impl CdtDiagnosticReport {
    /// Writes a versioned JSON report with explicit unavailable estimator outcomes.
    ///
    /// # Errors
    /// Reports serialization and output failures through the chain transport error.
    pub fn write_json(&self, writer: impl Write) -> CdtResult<()> {
        serde_json::to_writer_pretty(writer, self).map_err(|error| {
            CdtAnalysisError::Serialization {
                operation: AnalysisArtifactOperation::Write,
                detail: error.to_string(),
            }
            .into()
        })
    }
}

/// Metadata identifying an analyzed chain without a second stored draw history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtDiagnosticSource {
    /// Producer crate version.
    pub crate_version: String,
    /// Run identity.
    pub run_id: String,
    /// Independent-chain identity.
    pub chain_id: String,
    /// Full configuration, including start profile, warmup, cadence, and seed.
    pub configuration: CdtConfig,
    /// Number of original scheduled measurements.
    pub sample_count: usize,
}

/// Analyzes comparable independent CDT chains using published upstream estimators.
///
/// Timing, when supplied, must cover exactly these post-warmup measurements and
/// their generating transitions, excluding setup, serialization and plotting.
/// Parallel-chain timing is actual wall time, not the sum of runner durations.
/// No timing is inferred from whole-run duration or a subsequently filtered trace.
///
/// # Errors
/// Rejects incompatible conditions, duplicate identity/seeds, invalid policy,
/// single-chain requests, or invalid observable selections. Valid but unresolved
/// estimators remain explicit report outcomes.
///
/// # Examples
///
/// This small interface example declares a policy before generating independent
/// chains. Short traces may produce inconclusive diagnostics; neither an
/// adequate-information verdict nor this example establishes ensemble validity.
/// Release validation requires the larger predeclared chain matrix.
///
/// ```
/// use causal_triangulations::prelude::config::{CdtConfig, CdtTopology};
/// use causal_triangulations::prelude::observables::{
///     analyze_cdt_chains, CdtDiagnosticObservable, CdtDiagnosticPolicy, ValidationChain,
/// };
/// use causal_triangulations::{CdtResult, run_simulation};
///
/// fn main() -> CdtResult<()> {
///     let policy = CdtDiagnosticPolicy {
///         maximum_lag: 8, minimum_completed_blocks: 4, stabilization_levels: 2,
///         binning_relative_tolerance: 0.2, require_stable_binning: true,
///         minimum_chains: 2, minimum_ess: 100.0, maximum_mean_mcse: 0.1,
///         quantiles: Vec::new(),
///     };
///     let mut chains = Vec::new();
///     for seed in [131, 132] {
///         let config = CdtConfig {
///             topology: CdtTopology::Toroidal, simulate: true, seed: Some(seed),
///             steps: 32, thermalization_steps: 4, measurement_frequency: 1,
///             ..CdtConfig::new(16, 4)
///         }.into_validated()?;
///         let results = run_simulation(&config)?;
///         chains.push(ValidationChain::from_results(
///             "analysis-example", &format!("chain-{seed}"), &config, &results,
///         )?);
///     }
///     let report = analyze_cdt_chains(
///         &chains, CdtDiagnosticObservable::LoopLength { slice: 0 }, &policy, None,
///     )?;
///     println!("{report:?}");
///     Ok(())
/// }
/// ```
pub fn analyze_cdt_chains(
    chains: &[ValidationChain],
    observable: CdtDiagnosticObservable,
    policy: &CdtDiagnosticPolicy,
    timing: Option<&DiagnosticTiming>,
) -> CdtResult<CdtDiagnosticReport> {
    policy.validate()?;
    ensure_comparable(chains)?;
    let values = chains
        .iter()
        .map(|chain| {
            chain
                .data
                .measurements
                .iter()
                .map(|measurement| observable.value(measurement))
                .collect::<CdtResult<Vec<_>>>()
        })
        .collect::<CdtResult<Vec<_>>>()?;
    let slices: Vec<_> = values.iter().map(Vec::as_slice).collect();
    let dependence = chains
        .iter()
        .zip(&values)
        .map(|(chain, values)| per_chain_dependence(&chain.data.chain_id, values, policy))
        .collect();
    let combined = CombinedRhat::estimate(&slices);
    let rank_normalized_rhat = estimate(
        combined
            .and_then(CombinedRhat::rank_normalized)
            .map(RankNormalizedSplitRhat::value),
    );
    let folded_rhat = estimate(
        combined
            .and_then(CombinedRhat::folded)
            .map(FoldedRankNormalizedSplitRhat::value),
    );
    let combined_rhat = match combined {
        Ok(value) => value.value().map_or_else(
            || DiagnosticEstimate::Unavailable {
                reason: "CombinedRhat requires both location and folded components".to_owned(),
            },
            DiagnosticEstimate::Available,
        ),
        Err(error) => DiagnosticEstimate::Unavailable {
            reason: format!("{error:?}"),
        },
    };
    let mean = EssEstimate::estimate(&slices, EssEstimator::Mean);
    let tails = TailEss::estimate(&slices);
    let mean_mcse = estimate(MeanMcse::estimate(&slices).map(|value| CdtMcseSummary {
        standard_error: value.value(),
        ess: ess_summary(*value.effective_sample_size()),
        quantile: None,
        interval: None,
    }));
    let quantiles = policy
        .quantiles
        .iter()
        .map(|requirement| CdtQuantileDiagnostic {
            probability: requirement.probability,
            mcse: estimate(
                QuantileMcse::estimate(&slices, requirement.probability).map(|value| {
                    CdtMcseSummary {
                        standard_error: value.value(),
                        ess: ess_summary(*value.effective_sample_size()),
                        quantile: Some(value.quantile()),
                        interval: Some(value.interval()),
                    }
                }),
            ),
        })
        .collect();
    let observable_units = observable_units(&observable);
    let mut report = CdtDiagnosticReport {
        schema_version: 1,
        crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_estimator_contract: "markov-chain-monte-carlo 0.5.2 public diagnostics".to_owned(),
        sources: diagnostic_sources(chains),
        observable,
        observable_units,
        policy: policy.clone(),
        dependence,
        rank_normalized_rhat,
        folded_rhat,
        combined_rhat,
        mean_ess: estimate(mean.map(ess_summary)),
        bulk_ess: estimate(EssEstimate::estimate(&slices, EssEstimator::Bulk).map(ess_summary)),
        lower_tail_ess: estimate(tails.and_then(TailEss::lower).map(ess_summary)),
        upper_tail_ess: estimate(tails.and_then(TailEss::upper).map(ess_summary)),
        mean_mcse,
        quantiles,
        mean_ess_per_second: match mean {
            Ok(value) => estimate(value.per_second(timing)),
            Err(error) => DiagnosticEstimate::Unavailable {
                reason: format!("{error:?}"),
            },
        },
        timed_seconds: timing.map(|timing| timing.elapsed().as_secs_f64()),
        timed_chain_count: timing.map(|timing| timing.chain_count()),
        timed_samples_per_chain: timing.map(|timing| timing.samples_per_chain()),
        verdict: CdtDiagnosticVerdict::Inconclusive,
    };
    report.verdict = diagnostic_verdict(&report);
    Ok(report)
}

/// Copies source identity and configuration while borrowing canonical draw storage.
fn diagnostic_sources(chains: &[ValidationChain]) -> Vec<CdtDiagnosticSource> {
    chains
        .iter()
        .map(|chain| CdtDiagnosticSource {
            crate_version: chain.data.crate_version.clone(),
            run_id: chain.data.run_id.clone(),
            chain_id: chain.data.chain_id.clone(),
            configuration: chain.data.configuration.clone(),
            sample_count: chain.data.measurements.len(),
        })
        .collect()
}

/// Names the original units carried by Monte Carlo errors.
fn observable_units(observable: &CdtDiagnosticObservable) -> String {
    match observable {
        CdtDiagnosticObservable::CombinedAction
        | CdtDiagnosticObservable::PhysicalAction
        | CdtDiagnosticObservable::FixingAction => "action",
        CdtDiagnosticObservable::Triangles => "triangles",
        CdtDiagnosticObservable::LoopLength { .. } | CdtDiagnosticObservable::MaximumLoopLength => {
            "spacelike-links"
        }
        CdtDiagnosticObservable::LoopProduct { .. } => "spacelike-links-squared",
        _ => "indicator",
    }
    .to_owned()
}

/// Checks comparison metadata before any pooled computation.
fn ensure_comparable(chains: &[ValidationChain]) -> CdtResult<()> {
    let Some(first) = chains.first().filter(|_| chains.len() >= 2) else {
        return Err(CdtAnalysisError::InsufficientChains {
            provided: chains.len(),
        }
        .into());
    };
    let mut identities = HashSet::new();
    let mut seeds = HashSet::new();
    let reference = &first.data.configuration;
    for chain in chains {
        let config = &chain.data.configuration;
        if !identities.insert((&chain.data.run_id, &chain.data.chain_id))
            || !seeds.insert(config.seed)
        {
            return Err(CdtAnalysisError::DuplicateChain {
                chain: chain.data.chain_id.clone(),
            }
            .into());
        }
        if config.timeslices != reference.timeslices
            || config.topology != reference.topology
            || config.temperature.to_bits() != reference.temperature.to_bits()
            || config.coupling_0.to_bits() != reference.coupling_0.to_bits()
            || config.coupling_2.to_bits() != reference.coupling_2.to_bits()
            || config.cosmological_constant.to_bits() != reference.cosmological_constant.to_bits()
            || config.volume_fixing != reference.volume_fixing
            || config.thermalization_steps != reference.thermalization_steps
            || config.measurement_frequency != reference.measurement_frequency
            || config.steps != reference.steps
            || chain.data.crate_version != first.data.crate_version
            || chain.data.measurements.len() != first.data.measurements.len()
        {
            return Err(CdtAnalysisError::IncompatibleCondition {
                chain: chain.data.chain_id.clone(),
            }
            .into());
        }
    }
    Ok(())
}

/// Keeps each chain's time ordering and incomplete-block semantics intact.
fn per_chain_dependence(
    chain_id: &str,
    values: &[f64],
    policy: &CdtDiagnosticPolicy,
) -> CdtChainDependence {
    let acf = Autocorrelation::estimate(
        values,
        policy.maximum_lag.min(values.len().saturating_sub(1)),
    );
    let time = acf
        .as_ref()
        .map_err(Clone::clone)
        .and_then(Autocorrelation::integrated_time);
    let binning = BinningAnalysis::try_from_iter(values.iter().copied()).map(|analysis| {
        analysis
            .estimates()
            .map(|level| CdtBinningLevel {
                block_size: level.block_size(),
                completed_blocks: level.block_count(),
                mean: level.mean(),
                block_mean_variance: level.sample_variance(),
                blocked_mean_error: level.standard_error(),
            })
            .collect::<Vec<_>>()
    });
    let selected_binning_level = match &binning {
        Ok(levels) => select_binning_level(levels, policy),
        Err(error) => DiagnosticEstimate::Unavailable {
            reason: format!("{error:?}"),
        },
    };
    CdtChainDependence {
        chain_id: chain_id.to_owned(),
        sample_count: values.len(),
        autocorrelation: estimate(acf.map(|value| value.values().to_vec())),
        integrated_time: estimate(time.map(IntegratedAutocorrelationTime::estimate)),
        integrated_time_window: time.ok().map(IntegratedAutocorrelationTime::window),
        mean_ess: estimate(time.map(IntegratedAutocorrelationTime::effective_sample_size)),
        binning: estimate(binning),
        selected_binning_level,
    }
}

/// Selects a declared multi-level error plateau, never the coarsest level by default.
fn select_binning_level(
    levels: &[CdtBinningLevel],
    policy: &CdtDiagnosticPolicy,
) -> DiagnosticEstimate<usize> {
    let selected = levels
        .windows(policy.stabilization_levels)
        .enumerate()
        .filter(|(_, window)| {
            window.iter().all(|level| {
                level.completed_blocks >= policy.minimum_completed_blocks
                    && level.blocked_mean_error.is_some()
            }) && window.windows(2).all(|pair| {
                match (pair[0].blocked_mean_error, pair[1].blocked_mean_error) {
                    (Some(left), Some(right)) => {
                        (left - right).abs()
                            <= policy.binning_relative_tolerance * left.abs().max(right.abs())
                    }
                    _ => false,
                }
            })
        })
        .map(|(start, _)| start + policy.stabilization_levels - 1)
        .next_back();
    selected.map_or_else(
        || DiagnosticEstimate::Unavailable {
            reason: "InsufficientCompletedBlocksOrUnresolvedErrorPlateau".to_owned(),
        },
        DiagnosticEstimate::Available,
    )
}

/// Applies only predeclared information and convergence requirements.
fn diagnostic_verdict(report: &CdtDiagnosticReport) -> CdtDiagnosticVerdict {
    if report
        .combined_rhat
        .value()
        .is_some_and(|&value| value > 1.01)
    {
        return CdtDiagnosticVerdict::NotConverged;
    }
    let information = [
        &report.mean_ess,
        &report.bulk_ess,
        &report.lower_tail_ess,
        &report.upper_tail_ess,
    ]
    .iter()
    .all(|estimate| {
        estimate
            .value()
            .is_some_and(|value| value.effective_samples >= report.policy.minimum_ess)
    });
    let precision =
        report
            .mean_mcse
            .value()
            .is_some_and(|value| value.standard_error <= report.policy.maximum_mean_mcse)
            && report.quantiles.iter().zip(&report.policy.quantiles).all(
                |(diagnostic, required)| {
                    diagnostic.mcse.value().is_some_and(|value| {
                        value.standard_error <= required.maximum_mcse
                            && value.ess.effective_samples >= report.policy.minimum_ess
                    })
                },
            );
    let binning = !report.policy.require_stable_binning
        || report
            .dependence
            .iter()
            .all(|chain| chain.selected_binning_level.value().is_some());
    if report.sources.len() >= report.policy.minimum_chains
        && report.combined_rhat.value().is_some()
        && information
        && precision
        && binning
    {
        CdtDiagnosticVerdict::SufficientInformation
    } else {
        CdtDiagnosticVerdict::Inconclusive
    }
}
