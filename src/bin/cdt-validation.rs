#![forbid(unsafe_code)]

//! Non-interactive runner and verifier for external 1+1 validation bundles.

use causal_triangulations::prelude::simulation::validation::{
    self, CONTRACT_VERSION, ValidationError,
};
use clap::{Parser, Subcommand, error::ErrorKind};
use serde::Serialize;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(version, about)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run one chain into a new output directory.
    Run {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify a completed output directory.
    Verify {
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Serialize)]
struct Diagnostic<'a> {
    contract_version: &'static str,
    code: &'static str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<u32>,
}

fn main() -> ExitCode {
    let arguments = match Arguments::try_parse() {
        Ok(arguments) => arguments,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => return failure(2, "invalid_arguments", &error.to_string(), None),
    };
    let result = match arguments.command {
        Command::Run { config, output } => validation::run(&config, &output),
        Command::Verify { output } => validation::verify(&output),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let step = match error {
                ValidationError::Transition { step, .. } | ValidationError::Outcome(step) => {
                    Some(step)
                }
                _ => None,
            };
            failure(error.exit_status(), error.code(), &error.to_string(), step)
        }
    }
}

/// Writes one JSON error record even when application logging is disabled.
fn failure(status: u8, code: &'static str, message: &str, step: Option<u32>) -> ExitCode {
    let diagnostic = Diagnostic {
        contract_version: CONTRACT_VERSION,
        code,
        message,
        step,
    };
    let mut stderr = io::stderr().lock();
    if serde_json::to_writer(&mut stderr, &diagnostic).is_ok() {
        let _ = stderr.write_all(b"\n");
    }
    ExitCode::from(status)
}
