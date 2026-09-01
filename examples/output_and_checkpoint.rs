#![forbid(unsafe_code)]

//! Example: writing simulation output files and using CDT checkpoints.
//!
//! This example runs a short CDT simulation, writes the configured trace CSV and
//! JSON outputs, persists a versioned CDT-owned MCMC checkpoint, and resumes it.

use causal_triangulations::prelude::errors::{OutputFormat, OutputWriteStage};
use causal_triangulations::prelude::simulation::*;
use serde_json::{Value, from_str};
use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() -> CdtResult<()> {
    let output_dir =
        env::temp_dir().join(format!("causal-triangulations-output-{}", process::id()));
    let csv_path = output_dir.join("trace.csv");
    let json_path = output_dir.join("summary.json");
    let checkpoint_path = output_dir.join("checkpoint-v1.json");

    let config = CdtConfig {
        simulate: true,
        steps: 4,
        thermalization_steps: 0,
        measurement_frequency: 1,
        seed: Some(13),
        output_csv: Some(csv_path.clone()),
        output_json: Some(json_path.clone()),
        ..CdtConfig::new(12, 3)
    }
    .into_validated()?;

    let results = run_simulation(&config)?;

    let csv = read_output(&csv_path, OutputFormat::Csv)?;
    let summary_json = read_output(&json_path, OutputFormat::Json)?;
    let summary: Value = from_str(&summary_json).map_err(|err| CdtError::OutputReadFailed {
        path: json_path.display().to_string(),
        format: OutputFormat::Json,
        detail: err.to_string(),
    })?;
    assert!(csv.starts_with(
        "chain_id,step,accepted,proposed,log_prob,action,vertices,edges,triangles,move_family"
    ));
    assert_eq!(summary["config"]["vertices"], config.vertices().get());
    assert_eq!(
        summary["final_triangulation"]["time_slices"],
        config.timeslices().get()
    );
    assert_eq!(
        summary["measurements"].as_array().map_or(0, Vec::len),
        results.measurements().len()
    );

    let mcmc_checkpoint = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(13),
        ActionConfig::default(),
    )
    .run_to_checkpoint(CdtTriangulation2D::from_cdt_strip(4, 3)?)?;
    let checkpoint_json = mcmc_checkpoint.to_json()?;
    fs::write(&checkpoint_path, checkpoint_json).map_err(|err| CdtError::OutputWriteFailed {
        path: checkpoint_path.display().to_string(),
        format: OutputFormat::Json,
        stage: OutputWriteStage::Serialize,
        detail: err.to_string(),
    })?;
    let checkpoint_json = read_output(&checkpoint_path, OutputFormat::Json)?;
    let restored = CdtMcmcCheckpoint::from_json(&checkpoint_json)?;
    restored.triangulation().validate_topology()?;
    restored.triangulation().validate_foliation()?;
    restored.triangulation().validate_causality()?;
    restored.triangulation().validate_simplex_classification()?;
    let resumed = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(999),
        ActionConfig::default(),
    )
    .resume_from_checkpoint(restored)?;

    println!("Trace CSV rows: {}", csv.lines().count().saturating_sub(1));
    println!(
        "JSON summary measurements: {}",
        summary["measurements"].as_array().map_or(0, Vec::len)
    );
    println!(
        "Resumed MCMC checkpoint steps (v{} JSON): {}",
        CdtMcmcCheckpoint::FORMAT_VERSION,
        resumed.steps().len()
    );
    println!("Output and checkpoint example completed successfully!");

    let _ = fs::remove_dir_all(output_dir);
    Ok(())
}

/// Read an example output file and preserve the path and format in typed errors.
fn read_output(path: &Path, format: OutputFormat) -> CdtResult<String> {
    fs::read_to_string(path).map_err(|err| CdtError::OutputReadFailed {
        path: path.display().to_string(),
        format,
        detail: err.to_string(),
    })
}
