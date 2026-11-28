#![forbid(unsafe_code)]

//! Command-line interface integration tests for the CDT-RS application.
//!
//! This module contains tests that verify the behavior of the command-line
//! interface, including argument validation, success scenarios, and error handling.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use serde_json::{Value, from_str, json};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};
use std::thread;

fn temp_output_dir(name: &str) -> PathBuf {
    let thread_name = safe_thread_name();
    env::temp_dir().join(format!(
        "causal-triangulations-cli-{name}-{}-{}",
        process::id(),
        thread_name
    ))
}

fn cdt_command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("cdt"))
}

mod validation {
    use super::{Value, fs, json, temp_output_dir};
    use assert_cmd::prelude::*;
    use causal_triangulations::prelude::action::VolumeFixingError;
    use causal_triangulations::prelude::simulation::{
        CdtError,
        validation::{CONTRACT_VERSION, ValidationError, run, verify},
    };
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const MANIFEST: &str = "metadata.json";
    const MEASUREMENTS: &str = "measurements.jsonl";
    const TRANSITIONS: &str = "transitions.csv";

    struct Fixture {
        root: PathBuf,
        config: PathBuf,
        output: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = temp_output_dir("validation");
            fs::create_dir(&root).expect("fresh validation directory");
            let fixture = Self {
                config: root.join("input.json"),
                output: root.join("bundle"),
                root,
            };
            write_json(&fixture.config, &configuration());
            fixture
        }

        fn acquire(&self) -> Value {
            run(&self.config, &self.output).expect("seeded validation chain");
            read_json(&self.output.join(MANIFEST))
        }

        fn metadata(&self, metadata: &Value) {
            write_json(&self.output.join(MANIFEST), metadata);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn configuration() -> Value {
        json!({
            "contract_version": CONTRACT_VERSION,
            "topology": "periodic_time_spatial_circle",
            "time_slices": 4,
            "minimum_loop_length": 3,
            "initial_spatial_loop_lengths": [4, 4, 4, 4],
            "action": {
                "convention": "regge_counts_v1",
                "coupling_0": 0.0,
                "coupling_2": 0.0,
                "edge_cosmological_constant": 0.462_098_120_373_296_84
            },
            "temperature": 1.0,
            "move_policy": "uniform_move_family",
            "volume_fixing": {"kind": "quadratic_triangles", "target_n2": 32, "strength": 0.05},
            "warmup_steps": 2,
            "measurement_cadence": 3,
            "measurements": 32,
            "run_id": "validation-ci",
            "chain_id": "flat",
            "seed": "320101"
        })
    }

    fn command() -> Command {
        Command::new(assert_cmd::cargo::cargo_bin!("cdt-validation"))
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).expect("read JSON record"))
            .expect("parse JSON record")
    }

    fn write_json(path: &Path, value: &Value) {
        let mut contents = serde_json::to_vec(value).expect("serialize fixture");
        contents.push(b'\n');
        fs::write(path, contents).expect("write JSON record");
    }

    fn assert_cli_failure(command: &mut Command, status: i32, code: &str) {
        let result = command.output().expect("run validation command");
        assert_eq!(result.status.code(), Some(status), "{:?}", result.stderr);
        assert_eq!(result.stdout.as_slice(), b"");
        let diagnostic: Value = serde_json::from_slice(&result.stderr).expect("JSON diagnostic");
        assert_eq!(diagnostic["contract_version"], CONTRACT_VERSION);
        assert_eq!(diagnostic["code"], code);
        assert!(
            diagnostic["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
    }

    #[test]
    fn validation_cli_emits_seeded_traces_and_rejects_overwrite() {
        let fixture = Fixture::new();
        command()
            .arg("--help")
            .assert()
            .success()
            .stdout(predicates::str::contains("verify"));
        assert_cli_failure(command().arg("run"), 2, "invalid_arguments");
        let mut invocation = command();
        invocation
            .arg("run")
            .arg("--config")
            .arg(&fixture.config)
            .arg("--output")
            .arg(&fixture.output);
        invocation.assert().success().stdout("").stderr("");
        command()
            .arg("verify")
            .arg("--output")
            .arg(&fixture.output)
            .assert()
            .success()
            .stdout("")
            .stderr("");
        let metadata = read_json(&fixture.output.join(MANIFEST));
        assert_eq!(metadata["artifact_schema_version"], 1);
        assert_eq!(metadata["configuration"], configuration());
        let transitions =
            fs::read_to_string(fixture.output.join(TRANSITIONS)).expect("proposal outcomes");
        assert_eq!(transitions.lines().count(), 99, "header and 98 proposals");
        for outcome in ['A', 'N', 'R'] {
            assert!(
                transitions
                    .lines()
                    .skip(1)
                    .any(|row| row.starts_with(outcome))
            );
        }
        let measurements =
            fs::read_to_string(fixture.output.join(MEASUREMENTS)).expect("cadence measurements");
        let rows: Vec<Value> = measurements
            .lines()
            .map(|row| serde_json::from_str(row).expect("measurement record"))
            .collect();
        assert_eq!(rows.len(), 32);
        assert_eq!(rows[0]["step"], 5);
        assert_eq!(rows[31]["step"], 98);
        assert!(rows.iter().any(|row| row["n2"] == 32));
        assert!(rows.iter().any(|row| row["n2"] != 32));
        assert_cli_failure(&mut invocation, 4, "artifact_io_failed");
        assert_eq!(read_json(&fixture.output.join(MANIFEST)), metadata);
        assert_eq!(
            fs::read_to_string(fixture.output.join(MEASUREMENTS)).expect("original measurements"),
            measurements
        );
    }

    #[test]
    fn validation_rejects_malformed_configuration_before_writing() {
        let fixture = Fixture::new();
        for (field, bad) in [
            ("initial_spatial_loop_lengths", json!([2, 4, 4, 4])),
            ("time_slices", json!(5)),
            ("run_id", json!(" ")),
            ("seed", json!("0320101")),
            ("measurement_cadence", json!(0)),
            ("warmup_steps", json!(u32::MAX)),
            ("unknown", json!(true)),
            ("topology", json!({"periodic_time_spatial_circle": null})),
            ("move_policy", json!({"uniform_move_family": null})),
            ("action", json!(["regge_counts_v1", 0.0, 0.0, 0.0])),
            ("volume_fixing", json!(["quadratic_triangles", 32, 0.05])),
        ] {
            let mut config = configuration();
            config[field] = bad;
            write_json(&fixture.config, &config);
            assert!(
                matches!(
                    run(&fixture.config, &fixture.output),
                    Err(ValidationError::Configuration(_))
                ),
                "{field}"
            );
            assert!(!fixture.output.exists(), "{field}");
        }
        for invalid in ["{", r#"["cdt/periodic-1p1-validation/v1"]"#] {
            fs::write(&fixture.config, invalid).expect("write malformed input");
            assert!(matches!(
                run(&fixture.config, &fixture.output),
                Err(ValidationError::Configuration(_))
            ));
        }
        let mut config = configuration();
        config["volume_fixing"]["target_n2"] = json!(34);
        write_json(&fixture.config, &config);
        assert!(matches!(
            run(&fixture.config, &fixture.output),
            Err(ValidationError::Configuration(_))
        ));
        config["contract_version"] = json!("cdt/periodic-1p1-validation/v2");
        write_json(&fixture.config, &config);
        assert_cli_failure(
            command()
                .arg("run")
                .arg("--config")
                .arg(&fixture.config)
                .arg("--output")
                .arg(&fixture.output),
            5,
            "incompatible_schema",
        );
        assert!(!fixture.output.exists());
    }

    #[test]
    fn validation_preserves_typed_configuration_failures() {
        let fixture = Fixture::new();
        for (field, value) in [
            ("temperature", json!(0.0)),
            (
                "action",
                json!({"convention":"regge_counts_v1", "coupling_0":1e300, "coupling_2":0.0, "edge_cosmological_constant":0.0}),
            ),
            (
                "volume_fixing",
                json!({"kind":"quadratic_triangles", "target_n2":32, "strength":-1.0}),
            ),
        ] {
            let mut config = configuration();
            config[field] = value;
            write_json(&fixture.config, &config);
            let error =
                run(&fixture.config, &fixture.output).expect_err("invalid numerical setting");
            assert_eq!(
                (error.exit_status(), error.code()),
                (2, "invalid_configuration")
            );
            assert!(matches!(
                (field, error),
                (
                    "temperature",
                    ValidationError::InvalidTarget(CdtError::InvalidSimulationConfiguration { .. })
                ) | (
                    "action",
                    ValidationError::InvalidAction(CdtError::InvalidConfiguration { .. })
                ) | (
                    "volume_fixing",
                    ValidationError::InvalidVolumeFixing(VolumeFixingError::InvalidStrength { .. })
                )
            ));
            assert!(!fixture.output.exists());
        }
        let mut config = configuration();
        config["temperature"] = json!(1e-30);
        write_json(&fixture.config, &config);
        assert_cli_failure(
            command()
                .arg("run")
                .arg("--config")
                .arg(&fixture.config)
                .arg("--output")
                .arg(&fixture.output),
            3,
            "chain_initialization_failed",
        );
        assert!(!fixture.output.exists());
    }

    #[test]
    fn validation_verifies_the_completion_manifest_schema() {
        let fixture = Fixture::new();
        let original = fixture.acquire();
        let manifest = fixture.output.join(MANIFEST);
        let bytes = fs::read(&manifest).expect("completed manifest");
        fs::write(
            &manifest,
            bytes.strip_suffix(b"\n").expect("LF-terminated manifest"),
        )
        .expect("remove final LF");
        assert!(matches!(
            verify(&fixture.output),
            Err(ValidationError::Bundle(_))
        ));
        let mut crlf = bytes.strip_suffix(b"\n").expect("manifest body").to_vec();
        crlf.extend_from_slice(b"\r\n");
        fs::write(&manifest, crlf).expect("write CRLF manifest");
        assert!(matches!(
            verify(&fixture.output),
            Err(ValidationError::Bundle(_))
        ));
        for field in ["build", "configuration", "timing_ns"] {
            let mut metadata = original.clone();
            let positional = metadata[field]
                .as_object()
                .expect("record fields")
                .values()
                .cloned()
                .collect();
            metadata[field] = Value::Array(positional);
            fixture.metadata(&metadata);
            assert!(
                matches!(
                    verify(&fixture.output),
                    Err(ValidationError::ArtifactJson(_))
                ),
                "{field}"
            );
        }
        for dependencies in [
            json!({}),
            json!({"delaunay":"0.8.5","la-stack":"0.5.4","markov-chain-monte-carlo":"0.5.3","unknown":"0.1.0"}),
            json!(["0.8.5", "0.5.4", "0.5.3", "0.10.4"]),
        ] {
            let mut metadata = original.clone();
            metadata["build"]["release_lock_dependencies"] = dependencies;
            fixture.metadata(&metadata);
            assert!(matches!(
                verify(&fixture.output),
                Err(ValidationError::ArtifactJson(_))
            ));
        }
        let manifest_text = std::str::from_utf8(&bytes).expect("UTF-8 manifest");
        let duplicate = manifest_text.replacen(
            r#""release_lock_dependencies":{"#,
            r#""release_lock_dependencies":{"delaunay":"0.8.6","#,
            1,
        );
        assert_ne!(duplicate, manifest_text);
        fs::write(&manifest, duplicate).expect("write duplicate sampler dependency");
        assert!(matches!(
            verify(&fixture.output),
            Err(ValidationError::ArtifactJson(_))
        ));
        for version in ["", " \t"] {
            let mut metadata = original.clone();
            metadata["build"]["release_lock_dependencies"]["delaunay"] = json!(version);
            fixture.metadata(&metadata);
            assert!(matches!(
                verify(&fixture.output),
                Err(ValidationError::Bundle(_))
            ));
        }
        let mut metadata = original.clone();
        metadata["build"]["release_lock_dependencies"]["delaunay"] = json!("0.8.6");
        fixture.metadata(&metadata);
        verify(&fixture.output).expect("accept provenance from a different supplier build");
        metadata["artifact_schema_version"] = json!(2);
        fixture.metadata(&metadata);
        assert!(matches!(
            verify(&fixture.output),
            Err(ValidationError::Schema(_))
        ));
        metadata = original;
        metadata["seed_policy"] = json!("unknown");
        fixture.metadata(&metadata);
        assert!(matches!(
            verify(&fixture.output),
            Err(ValidationError::Schema(_))
        ));
        fs::remove_file(manifest).expect("remove completion manifest");
        assert_cli_failure(
            command().arg("verify").arg("--output").arg(&fixture.output),
            4,
            "artifact_io_failed",
        );
    }

    #[test]
    fn validation_rejects_inconsistent_or_incomplete_traces() {
        let fixture = Fixture::new();
        let original = fixture.acquire();
        let transitions = fixture.output.join(TRANSITIONS);
        let valid_transitions = fs::read(&transitions).expect("valid transition file");
        let mut extra_transition = valid_transitions.clone();
        extra_transition.extend_from_slice(b"N\n");
        for invalid in [
            b"invalid\n".as_slice(),
            b"outcome\nX\n",
            b"outcome\nNNNN\n",
            &valid_transitions[..valid_transitions.len() - 1],
            &extra_transition,
        ] {
            fs::write(&transitions, invalid).expect("replace transition file");
            assert!(matches!(
                verify(&fixture.output),
                Err(ValidationError::Bundle(_))
            ));
        }
        let all_holds = format!("outcome\n{}", "N\n".repeat(98));
        let mut metadata = original.clone();
        metadata["transitions_sha256"] = json!(digest(all_holds.as_bytes()));
        fs::write(&transitions, all_holds).expect("replace all transitions with holds");
        fixture.metadata(&metadata);
        assert!(
            matches!(verify(&fixture.output), Err(ValidationError::Bundle(message)) if message.contains("profile changed"))
        );
        fs::write(&transitions, valid_transitions).expect("restore transition file");
        fixture.metadata(&original);
        let measurements = fixture.output.join(MEASUREMENTS);
        let valid_measurements = fs::read_to_string(&measurements).expect("valid measurements");
        let (first, remaining) = valid_measurements
            .split_once('\n')
            .expect("first measurement");
        let spaced = format!(" {first}\n{remaining}");
        fs::write(&measurements, &spaced).expect("write noncompact measurement");
        metadata = original.clone();
        metadata["measurements_sha256"] = json!(digest(spaced.as_bytes()));
        fixture.metadata(&metadata);
        verify(&fixture.output).expect("accept whitespace within the measurement line limit");
        let row: Value = serde_json::from_str(first).expect("first measurement JSON");
        let positional = json!([
            row["step"],
            row["n0"],
            row["n1"],
            row["n2"],
            row["spatial_loop_lengths"]
        ]);
        let bad_row = format!("{positional}\n{remaining}");
        fs::write(&measurements, &bad_row).expect("replace a measurement with a positional array");
        metadata = original.clone();
        metadata["measurements_sha256"] = json!(digest(bad_row.as_bytes()));
        fixture.metadata(&metadata);
        assert!(matches!(
            verify(&fixture.output),
            Err(ValidationError::ArtifactJson(_))
        ));
        let crlf = format!("{first}\r\n{remaining}");
        fs::write(&measurements, &crlf).expect("write CRLF measurement");
        metadata["measurements_sha256"] = json!(digest(crlf.as_bytes()));
        fixture.metadata(&metadata);
        assert!(
            matches!(verify(&fixture.output), Err(ValidationError::Bundle(message)) if message.contains("non-LF"))
        );
        let mut inconsistent = row;
        inconsistent["step"] = json!(6);
        fs::write(&measurements, format!("{inconsistent}\n{remaining}"))
            .expect("write off-cadence measurement");
        assert!(
            matches!(verify(&fixture.output), Err(ValidationError::Bundle(message)) if message.contains("invalid cadence"))
        );
        fs::write(&measurements, valid_measurements).expect("restore measurement file");
        assert!(
            matches!(verify(&fixture.output), Err(ValidationError::Bundle(message)) if message.contains("SHA-256 mismatch"))
        );
        fixture.metadata(&original);
        let config = fixture.output.join("configuration.json");
        write_json(&config, &json!({}));
        assert!(
            matches!(verify(&fixture.output), Err(ValidationError::Bundle(message)) if message.contains("differs from metadata"))
        );
    }

    fn digest(bytes: &[u8]) -> String {
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            write!(digest, "{byte:02x}").expect("format digest");
        }
        digest
    }
}

/// Returns the current test thread name with path separators and
/// reserved characters removed.
fn safe_thread_name() -> String {
    thread::current()
        .name()
        .unwrap_or("test")
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect()
}

#[test]
fn exit_success() {
    let mut cmd = cdt_command();
    cmd.arg("-v");
    cmd.arg("36");
    cmd.arg("-t");
    cmd.arg("3");
    cmd.assert().success();
}

#[test]
fn cdt_cli_args() {
    let mut cmd = cdt_command();

    cmd.arg("-v");
    cmd.arg("36");
    cmd.arg("-t");
    cmd.arg("3");
    cmd.env("RUST_LOG", "info");

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("faces"));
}

#[test]
fn cdt_cli_no_args() {
    let mut cmd = cdt_command();

    cmd.assert().failure().stderr(predicate::str::contains(
        "error: the following required arguments were not provided:",
    ));
}

#[test]
fn cdt_cli_help_documents_readme_usage() {
    let mut cmd = cdt_command();

    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "Run 1+1-dimensional Causal Dynamical Triangulations simulations",
        ))
        .stdout(predicate::str::contains(
            "Vertices per spatial slice; total vertices are computed",
        ))
        .stdout(predicate::str::contains("--vertices-per-slice"))
        .stdout(predicate::str::contains(
            "--spatial-vertex-profile <N0,N1,...>",
        ))
        .stdout(predicate::str::contains("--topology <TOPOLOGY>"))
        .stdout(predicate::str::contains("toroidal"))
        .stdout(predicate::str::contains("--output-json <PATH>"));
}

#[test]
fn cdt_cli_rejects_ambiguous_vertex_count_inputs() {
    let mut cmd = cdt_command();

    cmd.arg("--vertices").arg("12");
    cmd.arg("--vertices-per-slice").arg("4");
    cmd.arg("--timeslices").arg("3");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains(
            "the argument '--vertices <VERTICES>' cannot be used with '--vertices-per-slice <VERTICES_PER_SLICE>'",
        ));
}

#[test]
fn cdt_cli_invalid_args() {
    let mut cmd = cdt_command();

    cmd.arg("-v");
    cmd.arg("36");
    cmd.arg("-t");
    cmd.arg("3");
    cmd.arg("-d");
    cmd.arg("5");

    cmd.assert().failure().stderr(predicate::str::contains(
        "error: invalid value '5' for '--dimension <DIMENSION>': 5 is not in 2..3",
    ));
}

#[test]
fn cdt_cli_rejects_unimplemented_dimension_at_parse_time() {
    let mut cmd = cdt_command();

    cmd.arg("-v");
    cmd.arg("36");
    cmd.arg("-t");
    cmd.arg("3");
    cmd.arg("-d");
    cmd.arg("3");

    cmd.assert().failure().stderr(predicate::str::contains(
        "error: invalid value '3' for '--dimension <DIMENSION>': 3 is not in 2..3",
    ));
}

#[test]
fn cdt_cli_invalid_measurement_frequency_zero() {
    // Note: This would be caught by clap's range validation now,
    // but we test the error message for completeness
    let mut cmd = cdt_command();

    cmd.arg("--vertices-per-slice").arg("4");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--measurement-frequency").arg("0");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("0 is not in 1.."));
}

#[test]
fn cdt_cli_reports_invalid_measurement_frequency_when_logging_is_disabled() {
    let mut cmd = cdt_command();

    cmd.arg("--vertices-per-slice").arg("4");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("100");
    cmd.arg("--measurement-frequency").arg("200");
    cmd.arg("--simulate");
    cmd.env("RUST_LOG", "off");

    cmd.assert().failure().stderr(predicate::str::contains(
        "Invalid simulation configuration: measurement_frequency (got: 200, expected: ≤ steps (100))",
    ));
}

#[test]
fn cdt_cli_runs_simulation_with_real_moves() {
    let mut cmd = cdt_command();

    cmd.arg("--vertices-per-slice").arg("4");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("20");
    cmd.arg("--thermalization-steps").arg("15");
    cmd.arg("--measurement-frequency").arg("10");
    cmd.arg("--seed").arg("42");
    cmd.arg("--simulate");
    cmd.env("RUST_LOG", "error");

    cmd.assert().success();
}

#[test]
fn cdt_cli_writes_configured_outputs() {
    let output_dir = temp_output_dir("outputs");
    let csv_path = output_dir.join("trace.csv");
    let json_path = output_dir.join("summary.json");
    let mut cmd = cdt_command();

    cmd.arg("--vertices-per-slice").arg("4");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("4");
    cmd.arg("--thermalization-steps").arg("0");
    cmd.arg("--measurement-frequency").arg("1");
    cmd.arg("--seed").arg("13");
    cmd.arg("--simulate");
    cmd.arg("--output-csv").arg(&csv_path);
    cmd.arg("--output-json").arg(&json_path);
    cmd.env("RUST_LOG", "error");

    cmd.assert().success();

    let csv = fs::read_to_string(&csv_path).expect("CSV output should be readable");
    let json = fs::read_to_string(&json_path).expect("JSON output should be readable");
    let parsed: Value = from_str(&json).expect("summary should parse");
    fs::remove_dir_all(&output_dir).expect("temporary output directory should be removable");

    assert!(csv.starts_with(
        "chain_id,step,accepted,proposed,log_prob,action,vertices,edges,triangles,move_family"
    ));
    assert_eq!(parsed["config"]["vertices"], 12);
    assert_eq!(parsed["final_triangulation"]["time_slices"], 3);
}

#[test]
fn cdt_cli_accepts_nonuniform_spatial_vertex_profile_without_timeslices() {
    let output_dir = temp_output_dir("spatial-vertex-profile");
    let json_path = output_dir.join("summary.json");
    let mut cmd = cdt_command();

    cmd.arg("--spatial-vertex-profile").arg("4,6,5");
    cmd.arg("--steps").arg("4");
    cmd.arg("--thermalization-steps").arg("0");
    cmd.arg("--measurement-frequency").arg("1");
    cmd.arg("--output-json").arg(&json_path);
    cmd.env("RUST_LOG", "error");

    cmd.assert().success();

    let json = fs::read_to_string(&json_path).expect("JSON output should be readable");
    let parsed: Value = from_str(&json).expect("summary should parse");
    fs::remove_dir_all(&output_dir).expect("temporary output directory should be removable");

    assert_eq!(parsed["config"]["vertices"], 15);
    assert_eq!(parsed["config"]["timeslices"], 3);
    assert_eq!(parsed["config"]["spatial_vertex_profile"], json!([4, 6, 5]));
    assert_eq!(parsed["final_triangulation"]["vertices"], 15);
    assert_eq!(parsed["final_triangulation"]["time_slices"], 3);
}

#[test]
fn cdt_cli_rejects_spatial_vertex_profile_timeslice_mismatch() {
    let mut cmd = cdt_command();

    cmd.arg("--spatial-vertex-profile").arg("4,6,5");
    cmd.arg("--timeslices").arg("4");

    cmd.assert().failure().stderr(predicate::str::contains(
        "--timeslices (4) must match --spatial-vertex-profile entry count (3)",
    ));
}

#[test]
fn cdt_cli_readme_toroidal_simulation_command_writes_json() {
    let output_dir = temp_output_dir("readme-toroidal");
    let json_path = output_dir.join("toroidal-summary.json");
    let mut cmd = cdt_command();

    cmd.arg("--vertices-per-slice").arg("4");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--topology").arg("toroidal");
    cmd.arg("--steps").arg("20");
    cmd.arg("--thermalization-steps").arg("0");
    cmd.arg("--measurement-frequency").arg("5");
    cmd.arg("--temperature").arg("1.5");
    cmd.arg("--seed").arg("105");
    cmd.arg("--simulate");
    cmd.arg("--output-json").arg(&json_path);
    cmd.env("RUST_LOG", "error");

    cmd.assert().success();

    let json = fs::read_to_string(&json_path).expect("JSON output should be readable");
    let parsed: Value = from_str(&json).expect("summary should parse");
    fs::remove_dir_all(&output_dir).expect("temporary output directory should be removable");

    assert_eq!(parsed["config"]["vertices"], 12);
    assert_eq!(parsed["config"]["timeslices"], 3);
    assert_eq!(parsed["config"]["topology"], "toroidal");
    assert_eq!(parsed["config"]["simulate"], true);
    assert_eq!(parsed["final_triangulation"]["topology"], "toroidal");
    assert_eq!(parsed["final_triangulation"]["time_slices"], 3);
    assert_eq!(
        parsed["steps"]
            .as_array()
            .expect("steps should be an array")
            .len(),
        20
    );
    assert_eq!(
        parsed["measurements"]
            .as_array()
            .expect("measurements should be an array")
            .len(),
        5
    );
}

#[test]
fn cdt_cli_rejects_missing_post_thermalization_measurement() {
    let mut cmd = cdt_command();

    cmd.arg("--vertices").arg("12");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("19");
    cmd.arg("--thermalization-steps").arg("15");
    cmd.arg("--measurement-frequency").arg("10");
    cmd.arg("--simulate");

    cmd.assert().failure().stderr(predicate::str::contains(
        "Invalid simulation configuration: measurement schedule",
    ));
}

#[test]
fn cdt_cli_invalid_vertices_too_few() {
    // This should be caught by clap's range validation
    let mut cmd = cdt_command();

    cmd.arg("--vertices").arg("2");
    cmd.arg("--timeslices").arg("3");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("2 is not in 3.."));
}

#[test]
fn cdt_cli_invalid_timeslices_zero() {
    // This should be caught by clap's range validation
    let mut cmd = cdt_command();

    cmd.arg("--vertices").arg("10");
    cmd.arg("--timeslices").arg("0");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("0 is not in 1.."));
}

#[test]
fn cdt_cli_config_validation_comprehensive() {
    // Test a complex scenario with valid parameters to ensure our validation doesn't break normal usage
    let mut cmd = cdt_command();

    cmd.arg("--vertices").arg("12");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("50");
    cmd.arg("--measurement-frequency").arg("5");
    cmd.arg("--temperature").arg("1.5");
    cmd.arg("--thermalization-steps").arg("10");
    cmd.env("RUST_LOG", "error"); // Reduce log noise

    cmd.assert().success();
}
