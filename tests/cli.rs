#![forbid(unsafe_code)]

//! Command-line interface integration tests for the CDT-RS application.
//!
//! This module contains tests that verify the behavior of the command-line
//! interface, including argument validation, success scenarios, and error handling.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use serde_json::{Value, from_str};
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
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));
    cmd.arg("-v");
    cmd.arg("36");
    cmd.arg("-t");
    cmd.arg("3");
    cmd.assert().success();
}

#[test]
fn cdt_cli_args() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

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
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.assert().failure().stderr(predicate::str::contains(
        "error: the following required arguments were not provided:",
    ));
}

#[test]
fn cdt_cli_invalid_args() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

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
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

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
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("12");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--measurement-frequency").arg("0");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("0 is not in 1.."));
}

#[test]
fn cdt_cli_invalid_measurement_frequency_too_large() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("12");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("100");
    cmd.arg("--measurement-frequency").arg("200");
    cmd.arg("--simulate");

    cmd.assert().failure().stderr(predicate::str::contains(
        "Invalid configuration: measurement_frequency (got: 200, expected: ≤ steps (100))",
    ));
}

#[test]
fn cdt_cli_runs_simulation_with_real_moves() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("12");
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
    let csv_path = output_dir.join("measurements.csv");
    let json_path = output_dir.join("summary.json");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("12");
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

    assert!(csv.starts_with("step,action,vertices,edges,triangles,accepted,delta_action\n"));
    assert_eq!(parsed["config"]["vertices"], 12);
    assert_eq!(parsed["final_triangulation"]["time_slices"], 3);
}

#[test]
fn cdt_cli_rejects_missing_post_thermalization_measurement() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("12");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("19");
    cmd.arg("--thermalization-steps").arg("15");
    cmd.arg("--measurement-frequency").arg("10");
    cmd.arg("--simulate");

    cmd.assert().failure().stderr(predicate::str::contains(
        "Invalid configuration: measurement schedule",
    ));
}

#[test]
fn cdt_cli_invalid_vertices_too_few() {
    // This should be caught by clap's range validation
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("2");
    cmd.arg("--timeslices").arg("3");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("2 is not in 3.."));
}

#[test]
fn cdt_cli_invalid_timeslices_zero() {
    // This should be caught by clap's range validation
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("10");
    cmd.arg("--timeslices").arg("0");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("0 is not in 1.."));
}

#[test]
fn cdt_cli_config_validation_comprehensive() {
    // Test a complex scenario with valid parameters to ensure our validation doesn't break normal usage
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cdt"));

    cmd.arg("--vertices").arg("12");
    cmd.arg("--timeslices").arg("3");
    cmd.arg("--steps").arg("50");
    cmd.arg("--measurement-frequency").arg("5");
    cmd.arg("--temperature").arg("1.5");
    cmd.arg("--thermalization-steps").arg("10");
    cmd.env("RUST_LOG", "error"); // Reduce log noise

    cmd.assert().success();
}
