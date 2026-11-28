#![forbid(unsafe_code)]

//! Artifact serialization, completion and integrity verification.

use super::{
    CONTRACT_VERSION, Configuration, IntrinsicCdtTriangulation, ValidationError, check_contract,
    decode_object, deserialize_object,
};
use crate::cdt::metropolis::intrinsic::MINIMUM_LOOP_LENGTH;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
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
    #[serde(deserialize_with = "deserialize_object")]
    configuration: Configuration,
    configuration_sha256: String,
    measurements_sha256: String,
    transitions_sha256: String,
    #[serde(deserialize_with = "deserialize_object")]
    build: Build,
    #[serde(deserialize_with = "deserialize_object")]
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
        serde_json::to_writer(&mut self.measurements, &row).map_err(|error| {
            if error.is_io() {
                io_error("write measurements.jsonl")(error.into())
            } else {
                ValidationError::ArtifactJson(error)
            }
        })?;
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
    if !has_lf_terminator(&raw_metadata) {
        return Err(ValidationError::Bundle(
            "metadata.json must use LF line endings and end with LF".to_owned(),
        ));
    }
    let version: MetadataVersion = decode_object(&raw_metadata)?;
    check_contract(&version.contract_version)?;
    if version.artifact_schema_version != ARTIFACT_SCHEMA_VERSION {
        return Err(ValidationError::Schema(format!(
            "expected artifact schema {ARTIFACT_SCHEMA_VERSION}, received {}",
            version.artifact_schema_version
        )));
    }
    let metadata: Metadata = decode_object(&raw_metadata)?;
    let dependencies = &metadata.build.release_lock_dependencies;
    if dependencies.len() != SAMPLER_DEPENDENCIES.len()
        || !SAMPLER_DEPENDENCIES.iter().all(|name| {
            dependencies
                .get(*name)
                .is_some_and(|version| !version.trim().is_empty())
        })
    {
        return Err(ValidationError::Bundle(
            "metadata release lock must identify all four sampler dependencies and their versions"
                .to_owned(),
        ));
    }
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

    verify_traces(directory, &metadata, steps)
}

/// Checks both schedules together so self-loops cannot change the profile.
fn verify_traces(directory: &Path, metadata: &Metadata, steps: u32) -> Result<(), ValidationError> {
    let config = &metadata.configuration;
    let mut transitions = TraceReader::new(directory, TRANSITIONS)?;
    let mut measurements = TraceReader::new(directory, MEASUREMENTS)?;
    if transitions.line(TRANSITION_HEADER.len() as u64)? != TRANSITION_HEADER {
        return Err(ValidationError::Bundle(
            "invalid transition CSV header".to_owned(),
        ));
    }
    // Reserve up to 20 decimal digits per count and loop length, plus separators
    // and field names. The checked starting volume bounds the number of slices.
    let measurement_bytes = 160 + 21 * config.time_slices as u64;
    let mut previous = config.initial_spatial_loop_lengths.clone();
    let mut accepted_since_measurement = false;
    for step in 1..=steps {
        match transitions.line(2)? {
            b"A\n" => accepted_since_measurement = true,
            b"R\n" | b"N\n" => (),
            _ => {
                return Err(ValidationError::Bundle(format!(
                    "invalid transition outcome at step {step}"
                )));
            }
        }
        if config.measurement_due(step) {
            let row: Measurement = decode_object(measurements.line(measurement_bytes)?)?;
            row.validate(step, config.time_slices)?;
            if !accepted_since_measurement && row.spatial_loop_lengths != previous {
                return Err(ValidationError::Bundle(format!(
                    "profile changed without an accepted proposal at step {step}"
                )));
            }
            previous = row.spatial_loop_lengths;
            accepted_since_measurement = false;
        }
    }
    transitions.finish(&metadata.transitions_sha256)?;
    measurements.finish(&metadata.measurements_sha256)
}

/// Hashes exactly the bytes verified while enforcing the format's line limits.
struct TraceReader {
    name: &'static str,
    reader: BufReader<File>,
    sha: Sha256,
    buffer: Vec<u8>,
    line: u64,
}

impl TraceReader {
    fn new(directory: &Path, name: &'static str) -> Result<Self, ValidationError> {
        Ok(Self {
            name,
            reader: open(directory, name)?,
            sha: Sha256::new(),
            buffer: Vec::new(),
            line: 0,
        })
    }

    fn line(&mut self, max_bytes: u64) -> Result<&[u8], ValidationError> {
        self.buffer.clear();
        self.line += 1;
        self.reader
            .by_ref()
            .take(max_bytes)
            .read_until(b'\n', &mut self.buffer)
            .map_err(io_error(self.name))?;
        if !has_lf_terminator(&self.buffer) {
            return Err(ValidationError::Bundle(format!(
                "{} has an incomplete, oversized, or non-LF line {}",
                self.name, self.line
            )));
        }
        self.sha.update(&self.buffer);
        Ok(&self.buffer)
    }

    fn finish(mut self, expected_sha256: &str) -> Result<(), ValidationError> {
        if !self
            .reader
            .fill_buf()
            .map_err(io_error(self.name))?
            .is_empty()
        {
            return Err(ValidationError::Bundle(format!(
                "{} contains extra rows",
                self.name
            )));
        }
        check_sha256(self.name, expected_sha256, &hex(&self.sha.finalize()))
    }
}

/// Rejects raw carriage returns, which JSON strings must encode as escapes.
fn has_lf_terminator(bytes: &[u8]) -> bool {
    bytes.ends_with(b"\n") && !bytes.contains(&b'\r')
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
