#![forbid(unsafe_code)]

//! Causal Dynamical Triangulations binary executable.
//!
//! This is the main entry point for the CDT-RS application that creates
//! and runs causal dynamical triangulations simulations.

use causal_triangulations::{CdtConfig, run_simulation};
use std::process::exit;

fn main() {
    // Initialize logging
    env_logger::init();

    let config = match CdtConfig::from_args().into_validated() {
        Ok(config) => config,
        Err(e) => {
            log::error!("CDT configuration failed: {e}");
            exit(1);
        }
    };
    match run_simulation(&config) {
        Ok(_results) => {
            log::info!("CDT simulation completed successfully");
        }
        Err(e) => {
            log::error!("CDT simulation failed: {e}");
            exit(1);
        }
    }
}
