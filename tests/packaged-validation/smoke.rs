#![forbid(unsafe_code)]

//! Copy into a standalone Cargo project with a path dependency on the unpacked archive.

use causal_triangulations::prelude::simulation::validation;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CONTRACT: &str = "cdt/periodic-1p1-validation/v1";
const WARMUP: u64 = 16_384;
const CADENCE: u64 = 16;
const MEASUREMENTS: u64 = 64;
const ARTIFACTS: [&str; 3] = [
    "configuration.json",
    "measurements.jsonl",
    "transitions.csv",
];

struct PackagedSource {
    version: String,
    lock_sha256: String,
    sampler_dependencies: BTreeMap<String, String>,
}

fn main() {
    let [binary, archive, output, resolution]: [OsString; 4] =
        std::env::args_os().skip(1).collect::<Vec<_>>().try_into().expect(
            "supply the packaged binary, .crate archive, new smoke directory, and downstream Cargo metadata JSON",
        );
    let binary = Path::new(&binary);
    let archive = Path::new(&archive);
    let output = Path::new(&output);
    let resolution = read_json(Path::new(&resolution));
    let package = packaged_source(&resolution);
    let archive_name = archive
        .file_name()
        .expect("archive basename")
        .to_string_lossy();
    assert_eq!(
        archive_name,
        format!("causal-triangulations-{}.crate", package.version)
    );
    assert_eq!(validation::CONTRACT_VERSION, CONTRACT);
    fs::create_dir(output).expect("new smoke output directory");
    let mut conditions = Vec::new();
    let mut first = None;
    let mut totals = [0; 2];
    for (slices, target, first_seed) in [
        (4, 32, 320_101),
        (4, 40, 400_101),
        (4, 48, 480_101),
        (8, 64, 640_101),
    ] {
        for (class, peak, seed) in [
            ("flat", false, first_seed),
            ("nonuniform", true, first_seed + 4),
        ] {
            let name = format!("t{slices}-n{target}-{class}");
            let mut profile = vec![target / (2 * slices); slices];
            if peak {
                profile.fill(3);
                profile[0] += target / 2 - 3 * slices;
            }
            let config = config(slices, target, profile, &name, seed);
            let config_path = output.join(format!("{name}.json"));
            write_json(&config_path, &config);
            let directory = output.join(&name);
            if first.is_none() {
                // This call links the standalone project to the unpacked library.
                validation::run(&config_path, &directory).expect("public packaged runner");
                first = Some((config.clone(), config_path, directory.clone()));
            } else {
                invoke(binary, Some(&config_path), &directory, 0, "");
            }
            invoke(binary, None, &directory, 0, "");
            let result = inspect(&directory, &config, &package);
            totals[0] += number(&result["target_visits"]);
            totals[1] += number(&result["measurements"]) - number(&result["target_visits"]);
            conditions.push(result);
        }
    }
    assert!(totals.into_iter().all(|count| count > 0));
    let (config, config_path, original) = first.expect("flat condition");
    let replay = output.join("flat-replay");
    validation::run(&config_path, &replay).expect("replay with the same downstream build");
    invoke(binary, None, &replay, 0, "");
    inspect(&replay, &config, &package);
    for filename in ARTIFACTS {
        assert_eq!(
            read(&original.join(filename)),
            read(&replay.join(filename)),
            "{filename} differs on replay"
        );
    }
    let mut replay_metadata = read_json(&replay.join("metadata.json"));
    let mut original_metadata = read_json(&original.join("metadata.json"));
    for metadata in [&mut replay_metadata, &mut original_metadata] {
        metadata
            .as_object_mut()
            .expect("metadata object")
            .remove("timing_ns");
    }
    assert_eq!(replay_metadata, original_metadata);

    let failures = negative_cases(binary, output, &config_path, &config, &original);
    let report = json!({
        "contract_version": CONTRACT,
        "artifact_schema_version": 1,
        "package": {
            "file": archive_name,
            "version": package.version,
            "sha256": sha256(&read(archive)),
        },
        "cli_executable_sha256": sha256(&read(binary)),
        "consumer_executable_sha256": sha256(&read(&std::env::current_exe().expect("consumer executable path"))),
        "build": original_metadata["build"],
        "fixtures": conditions,
        "same_seed_replay": "configuration, measurements, transitions and untimed metadata match",
        "negative_cases": failures,
        "exact_volume_target_visits": totals[0],
        "off_target_measurements": totals[1],
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("report JSON")
    );
}

fn config(slices: usize, target: usize, profile: Vec<usize>, chain: &str, seed: u64) -> Value {
    json!({
        "contract_version": CONTRACT,
        "topology": "periodic_time_spatial_circle",
        "time_slices": slices,
        "minimum_loop_length": 3,
        "initial_spatial_loop_lengths": profile,
        "action": {
            "convention": "regge_counts_v1", "coupling_0": 0.0, "coupling_2": 0.0,
            "edge_cosmological_constant": 2.0 * std::f64::consts::LN_2 / 3.0,
        },
        "temperature": 1.0,
        "move_policy": "uniform_move_family",
        "volume_fixing": {"kind": "quadratic_triangles", "target_n2": target, "strength": 0.05},
        "warmup_steps": WARMUP,
        "measurement_cadence": CADENCE,
        "measurements": MEASUREMENTS,
        "run_id": "packaged-validation-smoke",
        "chain_id": chain,
        "seed": seed.to_string(),
    })
}

fn inspect(bundle: &Path, config: &Value, package: &PackagedSource) -> Value {
    let metadata = read_json(&bundle.join("metadata.json"));
    fields(
        &metadata,
        &[
            "contract_version",
            "artifact_schema_version",
            "oracle_compatibility",
            "seed_policy",
            "configuration",
            "configuration_sha256",
            "measurements_sha256",
            "transitions_sha256",
            "build",
            "timing_ns",
        ],
    );
    assert_eq!(metadata["contract_version"], CONTRACT);
    assert_eq!(metadata["artifact_schema_version"], 1);
    assert_eq!(
        metadata["oracle_compatibility"],
        "periodic-labelled-inverse-automorphism/v1"
    );
    assert_eq!(
        metadata["seed_policy"],
        "xoshiro256plusplus-seed-from-u64-single-stream/v1"
    );
    assert_eq!(&metadata["configuration"], config);
    let build = &metadata["build"];
    fields(
        build,
        &[
            "crate_version",
            "rustc",
            "target",
            "profile",
            "opt_level",
            "debug",
            "release_lock_sha256",
            "release_lock_dependencies",
        ],
    );
    assert_eq!(build["crate_version"], package.version);
    assert!(
        build["rustc"]
            .as_str()
            .expect("compiler string")
            .starts_with("rustc ")
    );
    assert!(!build["target"].as_str().expect("target string").is_empty());
    assert_eq!(build["profile"], "release");
    assert_eq!(build["opt_level"], "3");
    assert_eq!(build["debug"], "false");
    assert_eq!(
        build["release_lock_dependencies"],
        json!(package.sampler_dependencies),
        "seed the standalone project from the archived supplier Cargo.lock"
    );
    assert_eq!(build["release_lock_sha256"], package.lock_sha256);
    let timing = &metadata["timing_ns"];
    fields(
        timing,
        &["initialization", "warmup_transition", "sampling_transition"],
    );
    for key in ["initialization", "warmup_transition", "sampling_transition"] {
        assert!(
            timing[key].as_u64().is_some(),
            "{key} must be an unsigned timing"
        );
    }

    let mut contents = ARTIFACTS
        .map(|filename| {
            let contents = read(&bundle.join(filename));
            let field = format!(
                "{}_sha256",
                filename.split_once('.').expect("file extension").0
            );
            assert_eq!(metadata[field], sha256(&contents));
            assert_eq!(contents.last(), Some(&b'\n'));
            String::from_utf8(contents).expect("UTF-8 artifact")
        })
        .into_iter();
    let canonical_config: Value =
        serde_json::from_str(&contents.next().expect("configuration")).expect("configuration JSON");
    assert_eq!(&canonical_config, config);
    let measures = contents.next().expect("measurements");
    let transitions = contents.next().expect("transitions");
    let mut outcomes = transitions.lines();
    assert_eq!(outcomes.next(), Some("outcome"));
    let mut outcome_counts = BTreeMap::<&str, u64>::new();
    for outcome in outcomes {
        assert!(
            matches!(outcome, "A" | "R" | "N"),
            "unknown transition outcome"
        );
        *outcome_counts.entry(outcome).or_default() += 1;
    }
    assert_eq!(
        outcome_counts.values().sum::<u64>(),
        WARMUP + MEASUREMENTS * CADENCE
    );
    assert!(outcome_counts.contains_key("A") && outcome_counts.contains_key("R"));

    let mut target_histogram = BTreeMap::<Vec<u64>, u64>::new();
    let mut rows = 0;
    for line in measures.lines() {
        rows += 1;
        let row: Value = serde_json::from_str(line).expect("measurement JSON");
        fields(&row, &["step", "n0", "n1", "n2", "spatial_loop_lengths"]);
        assert_eq!(number(&row["step"]), WARMUP + rows * CADENCE);
        let lengths: Vec<_> = row["spatial_loop_lengths"]
            .as_array()
            .expect("spatial loops")
            .iter()
            .map(number)
            .collect();
        assert_eq!(
            u64::try_from(lengths.len()).expect("slice count"),
            number(&config["time_slices"])
        );
        assert!(lengths.iter().all(|&length| length >= 3));
        let vertices = lengths.iter().sum::<u64>();
        assert_eq!(number(&row["n0"]), vertices);
        assert_eq!(number(&row["n1"]), 3 * vertices);
        assert_eq!(number(&row["n2"]), 2 * vertices);
        if row["n2"] == config["volume_fixing"]["target_n2"] {
            *target_histogram.entry(lengths).or_default() += 1;
        }
    }
    assert_eq!(rows, MEASUREMENTS);
    json!({
        "chain_id": config["chain_id"],
        "seed": config["seed"],
        "start": config["initial_spatial_loop_lengths"],
        "target_n2": config["volume_fixing"]["target_n2"],
        "measurements": rows,
        "target_visits": target_histogram.values().sum::<u64>(),
        "profiles_observed_at_target": target_histogram.len(),
        "transition_outcomes": outcome_counts,
    })
}

fn negative_cases(
    binary: &Path,
    root: &Path,
    config_path: &Path,
    config: &Value,
    original: &Path,
) -> Value {
    let corrupted = root.join("corrupted.json");
    let missing = root.join("must-not-exist");
    fs::write(&corrupted, b"{").expect("write malformed config");
    invoke(
        binary,
        Some(&corrupted),
        &missing,
        2,
        "invalid_configuration",
    );
    assert!(!missing.exists());
    let mut changed = config.clone();
    changed["measurement_cadence"] = json!(0);
    write_json(&corrupted, &changed);
    invoke(
        binary,
        Some(&corrupted),
        &missing,
        2,
        "invalid_configuration",
    );
    assert!(!missing.exists());
    changed["contract_version"] = json!("cdt/periodic-1p1-validation/v2");
    write_json(&corrupted, &changed);
    invoke(binary, Some(&corrupted), &missing, 5, "incompatible_schema");
    assert!(!missing.exists());
    let mut bad_chain = config.clone();
    bad_chain["temperature"] = json!(1e-30);
    write_json(&corrupted, &bad_chain);
    invoke(
        binary,
        Some(&corrupted),
        &missing,
        3,
        "chain_initialization_failed",
    );
    assert!(!missing.exists());

    let partial = root.join("partial");
    fs::create_dir(&partial).expect("partial directory");
    invoke(binary, None, &partial, 4, "artifact_io_failed");
    let metadata = read(&original.join("metadata.json"));
    invoke(binary, Some(config_path), original, 4, "artifact_io_failed");
    assert_eq!(read(&original.join("metadata.json")), metadata);
    for filename in ARTIFACTS {
        fs::copy(original.join(filename), partial.join(filename)).expect("copy fixture artifact");
    }
    fs::write(partial.join("metadata.json"), &metadata).expect("copy completion record");
    let original_metadata = read_json(&partial.join("metadata.json"));
    let full_trace = read(&partial.join("measurements.jsonl"));
    let transitions = partial.join("transitions.csv");
    let full_transitions = read(&transitions);
    let transition_text = std::str::from_utf8(&full_transitions).expect("transition CSV");
    let remaining = transition_text
        .splitn(3, '\n')
        .nth(2)
        .expect("remaining transitions");
    let oversized_transition = format!("outcome\nNNNNNNNN\n{remaining}");
    for (file, field, valid, damaged) in [
        (
            "measurements.jsonl",
            "measurements_sha256",
            full_trace.as_slice(),
            &full_trace[..full_trace.len() - 1],
        ),
        (
            "transitions.csv",
            "transitions_sha256",
            full_transitions.as_slice(),
            oversized_transition.as_bytes(),
        ),
    ] {
        let mut invalid = original_metadata.clone();
        invalid[field] = json!(sha256(damaged));
        fs::write(partial.join(file), damaged).expect("write malformed trace");
        write_json(&partial.join("metadata.json"), &invalid);
        invoke(binary, None, &partial, 4, "invalid_bundle");
        fs::write(partial.join(file), valid).expect("restore valid trace");
        write_json(&partial.join("metadata.json"), &original_metadata);
    }
    let mut stale_checksum = original_metadata.clone();
    stale_checksum["measurements_sha256"] = json!("0".repeat(64));
    write_json(&partial.join("metadata.json"), &stale_checksum);
    invoke(binary, None, &partial, 4, "invalid_bundle");

    // Accept a proposal before the first measurement, then require the next
    // interval of self-loops to preserve that first profile.
    let steps = usize::try_from(WARMUP + MEASUREMENTS * CADENCE).expect("fixture step count");
    let second_interval_holds = format!("outcome\nA\n{}", "N\n".repeat(steps - 1));
    fs::write(&transitions, &second_interval_holds).expect("write transition fixture");
    let mut measurement_rows = std::str::from_utf8(&full_trace)
        .expect("measurement JSONL")
        .splitn(3, '\n');
    let first = measurement_rows.next().expect("first measurement");
    measurement_rows.next().expect("second measurement");
    let remaining = measurement_rows.next().expect("remaining measurements");
    let mut second: Value = serde_json::from_str(first).expect("first measurement JSON");
    second["step"] = json!(WARMUP + 2 * CADENCE);
    let length = &mut second["spatial_loop_lengths"][0];
    *length = json!(number(length) + 1);
    let vertices = number(&second["n0"]) + 1;
    second["n0"] = json!(vertices);
    second["n1"] = json!(3 * vertices);
    second["n2"] = json!(2 * vertices);
    let changing_profile = format!("{first}\n{second}\n{remaining}");
    fs::write(partial.join("measurements.jsonl"), &changing_profile)
        .expect("write measurement fixture");
    let mut inconsistent = original_metadata.clone();
    inconsistent["transitions_sha256"] = json!(sha256(second_interval_holds.as_bytes()));
    inconsistent["measurements_sha256"] = json!(sha256(changing_profile.as_bytes()));
    write_json(&partial.join("metadata.json"), &inconsistent);
    invoke(binary, None, &partial, 4, "invalid_bundle");
    fs::write(&transitions, full_transitions).expect("restore transition file");
    fs::write(partial.join("measurements.jsonl"), full_trace).expect("restore measurements");
    let mut unsupported = original_metadata;
    unsupported["artifact_schema_version"] = json!(2);
    write_json(&partial.join("metadata.json"), &unsupported);
    invoke(binary, None, &partial, 5, "incompatible_schema");
    json!({
        "corrupt_configuration": 2, "zero_cadence": 2, "configuration_schema": 5,
        "failed_chain_initialization": 3, "missing_completion_manifest": 4, "existing_output": 4,
        "truncated_trace": 4, "oversized_transition_row": 4, "inconsistent_self_loops": 4, "checksum_mismatch": 4,
        "artifact_schema": 5,
    })
}

fn invoke(binary: &Path, config: Option<&Path>, output: &Path, status: i32, code: &str) {
    let mut command = Command::new(binary);
    if let Some(config) = config {
        command.args(["run", "--config"]).arg(config);
    } else {
        command.arg("verify");
    }
    let result = command
        .arg("--output")
        .arg(output)
        .output()
        .expect("run packaged command");
    assert_eq!(
        result.status.code(),
        Some(status),
        "{command:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    if status == 0 {
        assert!(result.stderr.is_empty());
    } else {
        let diagnostic: Value =
            serde_json::from_slice(&result.stderr).expect("JSON error on stderr");
        fields(&diagnostic, &["contract_version", "code", "message"]);
        assert_eq!(diagnostic["contract_version"], CONTRACT);
        assert_eq!(diagnostic["code"], code);
        assert!(
            !diagnostic["message"]
                .as_str()
                .expect("diagnostic message")
                .is_empty()
        );
    }
}

fn packaged_source(resolution: &Value) -> PackagedSource {
    let packages = resolution["packages"].as_array().expect("Cargo packages");
    let package = packages
        .iter()
        .find(|package| package["name"] == "causal-triangulations")
        .expect("packaged dependency");
    assert!(package["source"].is_null(), "expected a path dependency");
    let manifest = PathBuf::from(
        package["manifest_path"]
            .as_str()
            .expect("path dependency manifest"),
    );
    assert!(
        manifest.with_file_name("Cargo.toml.orig").is_file(),
        "expected a Cargo archive"
    );
    let dependencies = resolution["resolve"]["nodes"]
        .as_array()
        .expect("Cargo resolve nodes")
        .iter()
        .find(|node| node["id"] == package["id"])
        .expect("packaged dependency node")["deps"]
        .as_array()
        .expect("packaged dependency edges");
    let sampler_dependencies = ["delaunay", "la-stack", "markov-chain-monte-carlo", "rand"]
        .into_iter()
        .map(|name| {
            let dependency = dependencies
                .iter()
                .find(|edge| {
                    edge["name"] == name.replace('-', "_")
                        && edge["dep_kinds"]
                            .as_array()
                            .expect("dependency kinds")
                            .iter()
                            .any(|kind| kind["kind"].is_null())
                })
                .expect("sampler dependency edge");
            let resolved = packages
                .iter()
                .find(|package| package["id"] == dependency["pkg"])
                .expect("resolved sampler package");
            assert_eq!(
                resolved["source"],
                "registry+https://github.com/rust-lang/crates.io-index"
            );
            (
                name.to_owned(),
                resolved["version"]
                    .as_str()
                    .expect("resolved version")
                    .to_owned(),
            )
        })
        .collect();
    PackagedSource {
        version: package["version"]
            .as_str()
            .expect("packaged version")
            .to_owned(),
        lock_sha256: sha256(&read(&manifest.with_file_name("Cargo.lock"))),
        sampler_dependencies,
    }
}

fn fields(object: &Value, expected: &[&str]) {
    let actual = object
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected.iter().copied().collect());
}

fn number(value: &Value) -> u64 {
    value.as_u64().expect("unsigned JSON integer")
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&read(path)).expect("valid JSON artifact")
}

fn write_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec(value).expect("serialize JSON");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("write fixture JSON");
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
