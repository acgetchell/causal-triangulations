#![forbid(unsafe_code)]
//! Integration tests for shared Proptest configuration precedence.

use std::process::Command;

#[path = "common/proptest_config.rs"]
mod proptest_config;

use proptest_config::with_default_cases;

const CHILD_EXPECTED_CASES: &str = "CDT_PROPTEST_CONFIG_EXPECTED_CASES";
const FALLBACK_CASES: u32 = 8;
const PROPTEST_DEFAULT_CASES: u32 = 256;

/// Runs the child contract in a fresh process so Proptest reads the requested environment once.
fn run_config_contract(proptest_cases: Option<&str>, expected_cases: u32) {
    let executable = std::env::current_exe().expect("current integration-test executable");
    let mut command = Command::new(executable);
    command
        .arg("--exact")
        .arg("proptest_config_contract_child")
        .env(CHILD_EXPECTED_CASES, expected_cases.to_string());

    if let Some(cases) = proptest_cases {
        command.env("PROPTEST_CASES", cases);
    } else {
        command.env_remove("PROPTEST_CASES");
    }

    let output = command
        .output()
        .expect("Proptest configuration child process should run");
    assert!(
        output.status.success(),
        "configuration child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn proptest_config_contract_child() {
    let Some(expected_cases) = std::env::var_os(CHILD_EXPECTED_CASES) else {
        return;
    };
    let expected_cases = expected_cases
        .into_string()
        .expect("expected case count should be valid Unicode")
        .parse::<u32>()
        .expect("expected case count should be a u32");

    assert_eq!(with_default_cases(FALLBACK_CASES).cases, expected_cases);
}

#[test]
fn with_default_cases_preserves_fallback_and_environment_behavior() {
    run_config_contract(None, FALLBACK_CASES);
    run_config_contract(Some("17"), 17);
    run_config_contract(Some("not-a-u32"), PROPTEST_DEFAULT_CASES);
}
