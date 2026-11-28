#![forbid(unsafe_code)]

//! Artifact serialization, completion and integrity verification.

use super::{
    CONTRACT_VERSION, Configuration, IntrinsicCdtTriangulation, ValidationError, check_contract,
};
use crate::cdt::metropolis::intrinsic::MINIMUM_LOOP_LENGTH;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Duration;

const ARTIFACT_SCHEMA_VERSION: u32 = 1;
const ORACLE: &str = "periodic-labelled-inverse-automorphism/v1";
const SEED_POLICY: &str = "xoshiro256plusplus-seed-from-u64-single-stream/v1";
const CONFIGURATION: &str = "configuration.json";
const MEASUREMENTS: &str = "measurements.jsonl";
const TRANSITIONS: &str = "transitions.csv";
const METADATA: &str = "metadata.json";
const PARTIAL_METADATA: &str = ".metadata.json.partial";
const TRANSITION_HEADER: &[u8] = b"outcome\n";
const RELEASE_LOCK: &str = include_str!("../../../Cargo.lock");
const SAMPLER_DEPENDENCIES: [&str; 4] =
    ["delaunay", "la-stack", "markov-chain-monte-carlo", "rand"];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    contract_version: String,
    artifact_schema_version: u32,
    oracle_compatibility: String,
    seed_policy: String,
    configuration: Configuration,
    configuration_sha256: String,
    measurements_sha256: String,
    transitions_sha256: String,
    build: Build,
    timing_ns: Timing,
}

#[derive(Deserialize)]
struct MetadataVersion {
    contract_version: String,
    artifact_schema_version: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Build {
    crate_version: String,
    rustc: String,
    target: String,
    profile: String,
    opt_level: String,
    debug: String,
    release_lock_sha256: String,
    release_lock_dependencies: BTreeMap<String, String>,
}

impl Build {
    /// Identifies the build and the supplier lock shipped with its source.
    fn identify() -> Result<Self, ValidationError> {
        let mut dependencies = BTreeMap::new();
        // Cargo writes each package's name and version at the start of its
        // lockfile table. A duplicate sampler dependency would be ambiguous.
        for package in RELEASE_LOCK.split("[[package]]").skip(1) {
            let mut lines = package.lines().skip(1);
            let Some(name) = lines.next().and_then(|line| lock_value(line, "name = ")) else {
                continue;
            };
            if SAMPLER_DEPENDENCIES.contains(&name) {
                let version = lines
                    .next()
                    .and_then(|line| lock_value(line, "version = "))
                    .ok_or_else(|| {
                        ValidationError::Bundle(format!(
                            "no version for {name} in the release lock"
                        ))
                    })?;
                if dependencies
                    .insert(name.to_owned(), version.to_owned())
                    .is_some()
                {
                    return Err(ValidationError::Bundle(format!(
                        "ambiguous {name} version in the release lock"
                    )));
                }
            }
        }
        if dependencies.len() != SAMPLER_DEPENDENCIES.len() {
            return Err(ValidationError::Bundle(
                "incomplete sampler dependency list in the release lock".to_owned(),
            ));
        }
        Ok(Self {
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
            rustc: include_str!(concat!(env!("OUT_DIR"), "/finite_volume_rustc.txt"))
                .trim_end()
                .to_owned(),
            target: env!("CDT_BUILD_TARGET").to_owned(),
            profile: env!("CDT_BUILD_PROFILE").to_owned(),
            opt_level: env!("CDT_BUILD_OPT_LEVEL").to_owned(),
            debug: env!("CDT_BUILD_DEBUG").to_owned(),
            release_lock_sha256: hex(&Sha256::digest(RELEASE_LOCK.as_bytes())),
            release_lock_dependencies: dependencies,
        })
    }
}

/// Extracts a double-quoted value written by Cargo.
fn lock_value<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix)?
        .strip_prefix('"')?
        .strip_suffix('"')
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Timing {
    initialization: u128,
    warmup_transition: u128,
    sampling_transition: u128,
}

impl Timing {
    pub(super) const fn new(initialization: Duration) -> Self {
        Self {
            initialization: initialization.as_nanos(),
            warmup_transition: 0,
            sampling_transition: 0,
        }
    }

    pub(super) const fn record(&mut self, warmup: bool, elapsed: Duration) {
        if warmup {
            self.warmup_transition += elapsed.as_nanos();
        } else {
            self.sampling_transition += elapsed.as_nanos();
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Measurement {
    step: u32,
    n0: usize,
    n1: usize,
    n2: usize,
    spatial_loop_lengths: Vec<usize>,
}

impl Measurement {
    /// Checks the CDT bookkeeping invariants before export or offline use.
    fn validate(&self, step: u32, time_slices: usize) -> Result<(), ValidationError> {
        let lengths = &self.spatial_loop_lengths;
        let loop_sum = lengths
            .iter()
            .copied()
            .try_fold(0_usize, usize::checked_add);
        if self.step != step
            || lengths.len() != time_slices
            || lengths.iter().any(|&length| length < MINIMUM_LOOP_LENGTH)
            || loop_sum != Some(self.n0)
            || self.n0.checked_mul(3) != Some(self.n1)
            || self.n0.checked_mul(2) != Some(self.n2)
        {
            return Err(ValidationError::Bundle(format!(
                "invalid cadence or torus counts at step {step}"
            )));
        }
        Ok(())
    }
}

pub(super) struct Writer<'a> {
    directory: &'a Path,
    measurements: BufWriter<File>,
    transitions: BufWriter<File>,
    configuration_sha256: String,
    time_slices: usize,
    build: Build,
}

impl<'a> Writer<'a> {
    pub(super) fn new(
        directory: &'a Path,
        config: &Configuration,
    ) -> Result<Self, ValidationError> {
        let build = Build::identify()?;
        fs::create_dir(directory).map_err(io_error("create output directory"))?;
        let configuration_sha256 = write_json(directory, CONFIGURATION, config)?;
        let measurements = create(directory, MEASUREMENTS)?;
        let mut transitions = create(directory, TRANSITIONS)?;
        transitions
            .write_all(TRANSITION_HEADER)
            .map_err(io_error("write transitions.csv"))?;
        Ok(Self {
            directory,
            measurements,
            transitions,
            configuration_sha256,
            time_slices: config.time_slices,
            build,
        })
    }

    pub(super) fn transition(&mut self, outcome: u8) -> Result<(), ValidationError> {
        self.transitions
            .write_all(&[outcome, b'\n'])
            .map_err(io_error("write transitions.csv"))
    }

    pub(super) fn measure(
        &mut self,
        step: u32,
        state: &IntrinsicCdtTriangulation,
    ) -> Result<(), ValidationError> {
        let (n0, n1, n2) = state.counts();
        let row = Measurement {
            step,
            n0,
            n1,
            n2,
            spatial_loop_lengths: state.spatial_loop_lengths().collect(),
        };
        row.validate(step, self.time_slices)?;
        serde_json::to_writer(&mut self.measurements, &row)?;
        self.measurements
            .write_all(b"\n")
            .map_err(io_error("write measurements.jsonl"))
    }

    /// Flushes and hashes the traces before making their completion visible.
    pub(super) fn finish(
        mut self,
        configuration: Configuration,
        timing: Timing,
    ) -> Result<(), ValidationError> {
        self.measurements
            .flush()
            .map_err(io_error("flush measurements.jsonl"))?;
        self.transitions
            .flush()
            .map_err(io_error("flush transitions.csv"))?;
        let metadata = Metadata {
            contract_version: CONTRACT_VERSION.to_owned(),
            artifact_schema_version: ARTIFACT_SCHEMA_VERSION,
            oracle_compatibility: ORACLE.to_owned(),
            seed_policy: SEED_POLICY.to_owned(),
            configuration,
            configuration_sha256: self.configuration_sha256,
            measurements_sha256: file_sha256(self.directory, MEASUREMENTS)?,
            transitions_sha256: file_sha256(self.directory, TRANSITIONS)?,
            build: self.build,
            timing_ns: timing,
        };
        write_json(self.directory, PARTIAL_METADATA, &metadata)?;
        fs::rename(
            self.directory.join(PARTIAL_METADATA),
            self.directory.join(METADATA),
        )
        .map_err(io_error("complete metadata.json"))
    }
}

/// Verifies a completed bundle's version, configuration, cadence and checksums.
///
/// # Errors
///
/// Returns a [`ValidationError`] if the bundle is unsupported, incomplete,
/// unreadable, or internally inconsistent.
pub fn verify(directory: &Path) -> Result<(), ValidationError> {
    let raw_metadata =
        fs::read(directory.join(METADATA)).map_err(io_error("read metadata.json"))?;
    let version: MetadataVersion = serde_json::from_slice(&raw_metadata)?;
    check_contract(&version.contract_version)?;
    if version.artifact_schema_version != ARTIFACT_SCHEMA_VERSION {
        return Err(ValidationError::Schema(format!(
            "expected artifact schema {ARTIFACT_SCHEMA_VERSION}, received {}",
            version.artifact_schema_version
        )));
    }
    let metadata: Metadata = serde_json::from_slice(&raw_metadata)?;
    check_contract(&metadata.configuration.contract_version)?;
    if metadata.oracle_compatibility != ORACLE || metadata.seed_policy != SEED_POLICY {
        return Err(ValidationError::Schema(
            "unsupported oracle or seed policy identifier".to_owned(),
        ));
    }
    let config = &metadata.configuration;
    let (_, steps, _) = config
        .validate()
        .map_err(|error| ValidationError::Bundle(error.to_string()))?;
    let raw_config =
        fs::read(directory.join(CONFIGURATION)).map_err(io_error("read configuration.json"))?;
    let expected_config = json_line(config)?;
    if raw_config != expected_config {
        return Err(ValidationError::Bundle(
            "configuration.json differs from metadata".to_owned(),
        ));
    }
    check_sha256(
        CONFIGURATION,
        &metadata.configuration_sha256,
        &hex(&Sha256::digest(&raw_config)),
    )?;

    verify_lines(
        directory,
        TRANSITIONS,
        &metadata.transitions_sha256,
        u64::from(steps) + 1,
        |index, line| {
            if (index == 0 && line == TRANSITION_HEADER)
                || (index > 0 && matches!(line, b"A\n" | b"R\n" | b"N\n"))
            {
                Ok(())
            } else {
                Err(ValidationError::Bundle(format!(
                    "invalid transition CSV at line {}",
                    index + 1
                )))
            }
        },
    )?;
    verify_lines(
        directory,
        MEASUREMENTS,
        &metadata.measurements_sha256,
        u64::from(config.measurements),
        |index, line| {
            let step = (index + 1) * u64::from(config.measurement_cadence)
                + u64::from(config.warmup_steps);
            let step = u32::try_from(step)
                .map_err(|_| ValidationError::Bundle("invalid measurement clock".to_owned()))?;
            let row: Measurement = serde_json::from_slice(line)?;
            row.validate(step, config.time_slices)
        },
    )
}

/// Reads and validates the same bytes that feed each artifact's digest.
fn verify_lines(
    directory: &Path,
    name: &'static str,
    expected_sha256: &str,
    count: u64,
    mut validate_line: impl FnMut(u64, &[u8]) -> Result<(), ValidationError>,
) -> Result<(), ValidationError> {
    let mut reader = open(directory, name)?;
    let mut sha = Sha256::new();
    let mut line = Vec::new();
    for index in 0..count {
        line.clear();
        reader
            .read_until(b'\n', &mut line)
            .map_err(io_error(name))?;
        if !line.ends_with(b"\n") {
            return Err(ValidationError::Bundle(format!(
                "{name} is incomplete at line {}",
                index + 1
            )));
        }
        validate_line(index, &line)?;
        sha.update(&line);
    }
    if !reader.fill_buf().map_err(io_error(name))?.is_empty() {
        return Err(ValidationError::Bundle(format!(
            "{name} contains extra rows"
        )));
    }
    check_sha256(name, expected_sha256, &hex(&sha.finalize()))
}

fn check_sha256(name: &str, expected: &str, actual: &str) -> Result<(), ValidationError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ValidationError::Bundle(format!("{name} SHA-256 mismatch")))
    }
}

fn write_json(
    directory: &Path,
    name: &'static str,
    value: &impl Serialize,
) -> Result<String, ValidationError> {
    let bytes = json_line(value)?;
    let mut writer = create(directory, name)?;
    writer.write_all(&bytes).map_err(io_error(name))?;
    writer.flush().map_err(io_error(name))?;
    Ok(hex(&Sha256::digest(&bytes)))
}

fn json_line(value: &impl Serialize) -> Result<Vec<u8>, ValidationError> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn file_sha256(directory: &Path, name: &'static str) -> Result<String, ValidationError> {
    let mut reader = open(directory, name)?;
    let mut hash = Sha256::new();
    loop {
        let bytes = reader.fill_buf().map_err(io_error(name))?;
        if bytes.is_empty() {
            return Ok(hex(&hash.finalize()));
        }
        hash.update(bytes);
        let count = bytes.len();
        reader.consume(count);
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|&byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn create(directory: &Path, name: &'static str) -> Result<BufWriter<File>, ValidationError> {
    File::create_new(directory.join(name))
        .map(BufWriter::new)
        .map_err(io_error(name))
}

fn open(directory: &Path, name: &'static str) -> Result<BufReader<File>, ValidationError> {
    File::open(directory.join(name))
        .map(BufReader::new)
        .map_err(io_error(name))
}

const fn io_error(operation: &'static str) -> impl FnOnce(io::Error) -> ValidationError {
    move |source| ValidationError::Io { operation, source }
}
