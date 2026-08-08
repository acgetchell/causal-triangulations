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
