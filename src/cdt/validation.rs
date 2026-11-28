#![forbid(unsafe_code)]

//! Run and verify a versioned raw bundle for one periodic 1+1 CDT chain.
//!
//! The [external validation guide](https://github.com/acgetchell/causal-triangulations/blob/main/docs/VALIDATING.md)
//! defines the JSON and CSV formats and the released command's exit statuses.

mod bundle;

use crate::cdt::metropolis::intrinsic::{MINIMUM_LOOP_LENGTH, MINIMUM_TIME_SLICES};
use crate::cdt::metropolis::{Chain, DelayedStepError};
use crate::{
    ActionConfig, CdtError, CdtTarget, IntrinsicCdtError, IntrinsicCdtProposal,
    IntrinsicCdtTriangulation, McmcError, StepOutcome, VolumeFixing,
};
use rand::{SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Instant;
use thiserror::Error;

pub use bundle::verify;

/// Identifier shared by the runner configuration and bundle metadata.
pub const CONTRACT_VERSION: &str = "cdt/periodic-1p1-validation/v1";

/// Failure reported by the periodic validation runner or verifier.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ValidationError {
    /// The input could not be read or validated.
    #[error("invalid configuration: {0}")]
    Configuration(String),
    /// The requested contract or artifact schema is unsupported.
    #[error("incompatible validation schema: {0}")]
    Schema(String),
    /// The start profile could not be built as a valid intrinsic torus.
    #[error("initial triangulation: {0}")]
    InitialState(#[source] IntrinsicCdtError),
    /// The sampler could not evaluate the starting state.
    #[error("chain initialization: {0}")]
    Chain(#[source] McmcError),
    /// The intrinsic step failed before an outcome could be recorded.
    #[error("transition {step}: {source}")]
    Transition {
        /// One-based proposal step.
        step: u32,
        /// Sampler error.
        #[source]
        source: DelayedStepError<IntrinsicCdtError>,
    },
    /// The sampler returned an outcome this contract cannot represent.
    #[error("transition {0}: unsupported sampler outcome")]
    Outcome(u32),
    /// An output operation failed.
    #[error("validation artifact {operation}: {source}")]
    Io {
        /// Operation and artifact name.
        operation: &'static str,
        /// Filesystem error.
        #[source]
        source: io::Error,
    },
    /// A bundle is incomplete or inconsistent.
    #[error("invalid validation bundle: {0}")]
    Bundle(String),
    /// A bundle's JSON could not be read or written.
    #[error("invalid validation artifact JSON: {0}")]
    ArtifactJson(#[from] serde_json::Error),
}

impl ValidationError {
    /// Returns the stable process exit status for this failure.
    #[must_use]
    pub const fn exit_status(&self) -> u8 {
        match self {
            Self::Configuration(_) => 2,
            Self::InitialState(_) | Self::Chain(_) | Self::Transition { .. } | Self::Outcome(_) => {
                3
            }
            Self::Io { .. } | Self::Bundle(_) | Self::ArtifactJson(_) => 4,
            Self::Schema(_) => 5,
        }
    }

    /// Returns the stable machine-readable stderr code for this failure.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "invalid_configuration",
            Self::InitialState(_) | Self::Chain(_) => "chain_initialization_failed",
            Self::Transition { .. } | Self::Outcome(_) => "transition_failed",
            Self::Io { .. } => "artifact_io_failed",
            Self::Bundle(_) | Self::ArtifactJson(_) => "invalid_bundle",
            Self::Schema(_) => "incompatible_schema",
        }
    }
}

/// Read the version before interpreting the configuration fields.
#[derive(Deserialize)]
struct ContractProbe {
    contract_version: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    contract_version: String,
    topology: Topology,
    time_slices: usize,
    minimum_loop_length: usize,
    initial_spatial_loop_lengths: Vec<usize>,
    action: Action,
    temperature: f64,
    move_policy: MovePolicy,
    volume_fixing: Fixing,
    warmup_steps: u32,
    measurement_cadence: u32,
    measurements: u32,
    run_id: String,
    chain_id: String,
    seed: String,
}

#[derive(Deserialize, Serialize)]
enum Topology {
    #[serde(rename = "periodic_time_spatial_circle")]
    PeriodicTimeSpatialCircle,
}

#[derive(Deserialize, Serialize)]
enum MovePolicy {
    #[serde(rename = "uniform_move_family")]
    UniformMoveFamily,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "convention", deny_unknown_fields)]
enum Action {
    #[serde(rename = "regge_counts_v1")]
    ReggeCounts {
        coupling_0: f64,
        coupling_2: f64,
        edge_cosmological_constant: f64,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Fixing {
    #[serde(rename = "quadratic_triangles")]
    QuadraticTriangles { target_n2: u32, strength: f64 },
}

impl Configuration {
    /// Uses the same proposal clock for both acquisition and verification.
    const fn measurement_due(&self, step: u32) -> bool {
        step > self.warmup_steps
            && (step - self.warmup_steps).is_multiple_of(self.measurement_cadence)
    }

    /// Establishes the invariants needed by the step clock and target.
    fn validate(&self) -> Result<(CdtTarget, u32, u64), ValidationError> {
        let invalid = |message: &str| ValidationError::Configuration(message.to_owned());
        if self.minimum_loop_length != MINIMUM_LOOP_LENGTH
            || self.time_slices < MINIMUM_TIME_SLICES
            || self.initial_spatial_loop_lengths.len() != self.time_slices
            || self
                .initial_spatial_loop_lengths
                .iter()
                .any(|&length| length < MINIMUM_LOOP_LENGTH)
        {
            return Err(invalid(
                "require at least three time slices and one loop of length at least three per slice, with minimum_loop_length = 3",
            ));
        }
        let Fixing::QuadraticTriangles {
            target_n2,
            strength,
        } = self.volume_fixing;
        let start_n2 = self
            .initial_spatial_loop_lengths
            .iter()
            .copied()
            .try_fold(0_usize, usize::checked_add)
            .and_then(|sum| sum.checked_mul(2));
        if start_n2 != Some(target_n2 as usize) {
            return Err(invalid(
                "target_n2 must equal twice the sum of the initial spatial loop lengths",
            ));
        }
        if self.run_id.trim().is_empty() || self.chain_id.trim().is_empty() {
            return Err(invalid("run_id and chain_id must be nonempty"));
        }
        let seed = self
            .seed
            .parse::<u64>()
            .ok()
            .filter(|seed| seed.to_string() == self.seed)
            .ok_or_else(|| invalid("seed must be a canonical unsigned 64-bit decimal string"))?;
        let steps = self.measurements.checked_mul(self.measurement_cadence)
            .filter(|&post_warmup| post_warmup > 0)
            .and_then(|post_warmup| self.warmup_steps.checked_add(post_warmup))
            .ok_or_else(|| invalid("require positive measurements and cadence and a total step count that fits u32"))?;
        let fixing = VolumeFixing::quadratic_triangles(target_n2, strength)
            .map_err(|error| ValidationError::Configuration(error.to_string()))?;
        let Action::ReggeCounts {
            coupling_0,
            coupling_2,
            edge_cosmological_constant,
        } = self.action;
        let target = ActionConfig::new(coupling_0, coupling_2, edge_cosmological_constant)
            .and_then(|action| action.with_volume_fixing(Some(fixing)))
            .and_then(|action| CdtTarget::new(action, self.temperature))
            .map_err(|error: CdtError| ValidationError::Configuration(error.to_string()))?;
        Ok((target, steps, seed))
    }
}

/// Runs one chain from a versioned JSON configuration into a new directory.
///
/// Configuration validation precedes output directory creation. A successful
/// run writes a completion manifest. A failed write can leave an incomplete
/// directory, which [`verify`] rejects. Restart a failed run from its original
/// configuration into another directory.
///
/// # Errors
///
/// Returns a [`ValidationError`] for invalid input, unsupported schemas,
/// sampler failures, or output failures.
pub fn run(config_path: &Path, output: &Path) -> Result<(), ValidationError> {
    let input = fs::read(config_path).map_err(|error| {
        ValidationError::Configuration(format!("{}: {error}", config_path.display()))
    })?;
    let config_error = |error: serde_json::Error| ValidationError::Configuration(error.to_string());
    let probe: ContractProbe = serde_json::from_slice(&input).map_err(config_error)?;
    check_contract(&probe.contract_version)?;
    let config: Configuration = serde_json::from_slice(&input).map_err(config_error)?;
    let (target, steps, seed) = config.validate()?;
    let setup_start = Instant::now();
    let state =
        IntrinsicCdtTriangulation::from_spatial_loop_lengths(&config.initial_spatial_loop_lengths)
            .map_err(ValidationError::InitialState)?;
    target
        .intrinsic_log_prob(&state)
        .map_err(ValidationError::InitialState)?;
    let mut chain = Chain::new(state, &target).map_err(ValidationError::Chain)?;
    let mut proposal = IntrinsicCdtProposal::default();
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut timing = bundle::Timing::new(setup_start.elapsed());
    let mut artifacts = bundle::Writer::new(output, &config)?;
    for step in 1..=steps {
        let start = Instant::now();
        let result = chain.step_delayed(&target, &mut proposal, &mut rng);
        let elapsed = start.elapsed();
        let outcome = result
            .map_err(|source| ValidationError::Transition { step, source })?
            .outcome();
        let code = match outcome {
            StepOutcome::Accepted => b'A',
            StepOutcome::RejectedProposal => b'R',
            StepOutcome::NoProposal => b'N',
            _ => return Err(ValidationError::Outcome(step)),
        };
        timing.record(step <= config.warmup_steps, elapsed);
        artifacts.transition(code)?;
        if config.measurement_due(step) {
            artifacts.measure(step, chain.state())?;
        }
    }
    artifacts.finish(config, timing)
}

/// Rejects an unknown contract version before interpreting its payload.
fn check_contract(version: &str) -> Result<(), ValidationError> {
    if version == CONTRACT_VERSION {
        Ok(())
    } else {
        Err(ValidationError::Schema(format!(
            "expected {CONTRACT_VERSION}, received {version}"
        )))
    }
}
