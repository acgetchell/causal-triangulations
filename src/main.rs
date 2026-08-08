#![forbid(unsafe_code)]

//! Causal Dynamical Triangulations binary executable.
//!
//! This is the main entry point for the CDT-RS application that creates
//! and runs causal dynamical triangulations simulations.

use causal_triangulations::{CdtConfig, CdtError, run_simulation};
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // Initialize logging
    env_logger::init();

    let config = match CdtConfig::from_args().into_validated() {
        Ok(config) => config,
        Err(error) => return exit_with_error("CDT configuration failed", &error),
    };
    match run_simulation(&config) {
        Ok(_results) => {
            log::info!("CDT simulation completed successfully");
            ExitCode::SUCCESS
        }
        Err(error) => exit_with_error("CDT simulation failed", &error),
    }
}

/// Writes an unsuppressible process-level diagnostic and returns failure.
fn exit_with_error(context: &str, error: &CdtError) -> ExitCode {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ = writeln!(handle, "{context}: {error}");
    ExitCode::FAILURE
}
