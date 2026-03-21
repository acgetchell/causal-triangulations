//! Causal Dynamical Triangulations binary executable.
//!
//! This is the main entry point for the CDT-RS application that creates
//! and runs causal dynamical triangulations simulations.

use causal_triangulations::{CdtConfig, run_simulation};

fn main() {
    // Initialize logging
    env_logger::init();

    let config = CdtConfig::from_args();
    match run_simulation(&config) {
        Ok(_results) => {
            log::info!("CDT simulation completed successfully");
        }
        Err(e) => {
            log::error!("CDT simulation failed: {e}");
            std::process::exit(1);
        }
    }
}
