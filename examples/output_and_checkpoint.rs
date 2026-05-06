#![forbid(unsafe_code)]

//! Example: writing simulation output files and round-tripping a CDT checkpoint.
//!
//! This example runs a short CDT simulation, writes the configured CSV and JSON
//! outputs, and serializes the final triangulation as a serde checkpoint.

use causal_triangulations::prelude::simulation::*;
use serde_json::{Value, from_str, to_string};
use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() -> CdtResult<()> {
    let output_dir =
        env::temp_dir().join(format!("causal-triangulations-output-{}", process::id()));
    let csv_path = output_dir.join("measurements.csv");
    let json_path = output_dir.join("summary.json");

    let config = CdtConfig {
        simulate: true,
        steps: 4,
        thermalization_steps: 0,
        measurement_frequency: 1,
        seed: Some(13),
        output_csv: Some(csv_path.clone()),
        output_json: Some(json_path.clone()),
        ..CdtConfig::new(12, 3)
    };

    let results = run_simulation(&config)?;

    let csv = read_output(&csv_path, "CSV")?;
    let summary_json = read_output(&json_path, "JSON")?;
    let summary: Value = from_str(&summary_json).map_err(|err| CdtError::OutputReadFailed {
        path: json_path.display().to_string(),
        format: "JSON".to_string(),
        detail: err.to_string(),
    })?;
    assert!(csv.starts_with("step,action,vertices,edges,triangles,accepted,delta_action\n"));
    assert_eq!(summary["config"]["vertices"], config.vertices);
    assert_eq!(
        summary["final_triangulation"]["time_slices"],
        config.timeslices
    );

    let checkpoint = to_string(&results.triangulation).map_err(|err| {
        CdtError::CheckpointSerializationFailed {
            operation: "serialize".to_string(),
            target: "final triangulation".to_string(),
            detail: err.to_string(),
        }
    })?;
    let restored: CdtTriangulation2D =
        from_str(&checkpoint).map_err(|err| CdtError::CheckpointSerializationFailed {
            operation: "deserialize".to_string(),
            target: "final triangulation".to_string(),
            detail: err.to_string(),
        })?;
    restored.validate_topology()?;
    restored.validate_foliation()?;
    restored.validate_causality()?;
    restored.validate_cell_classification()?;

    let mcmc_checkpoint = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 2, 0, 1).with_seed(13),
        ActionConfig::default(),
    )
    .run_to_checkpoint(CdtTriangulation2D::from_cdt_strip(4, 3)?)?;
    let checkpoint_json =
        to_string(&mcmc_checkpoint).map_err(|err| CdtError::CheckpointSerializationFailed {
            operation: "serialize".to_string(),
            target: "MCMC state".to_string(),
            detail: err.to_string(),
        })?;
    let restored_checkpoint: CdtMcmcCheckpoint =
        from_str(&checkpoint_json).map_err(|err| CdtError::CheckpointSerializationFailed {
            operation: "deserialize".to_string(),
            target: "MCMC state".to_string(),
            detail: err.to_string(),
        })?;
    let resumed = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 2, 0, 1).with_seed(999),
        ActionConfig::default(),
    )
    .resume_from_checkpoint(restored_checkpoint)?;

    println!("CSV output rows: {}", csv.lines().count().saturating_sub(1));
    println!(
        "JSON summary measurements: {}",
        summary["measurements"].as_array().map_or(0, Vec::len)
    );
    println!("Checkpoint roundtrip vertices: {}", restored.vertex_count());
    println!("Resumed MCMC checkpoint steps: {}", resumed.steps.len());
    println!("Output and checkpoint example completed successfully!");

    let _ = fs::remove_dir_all(output_dir);
    Ok(())
}

/// Read an example output file and preserve the path and format in typed errors.
fn read_output(path: &Path, format: &'static str) -> CdtResult<String> {
    fs::read_to_string(path).map_err(|err| CdtError::OutputReadFailed {
        path: path.display().to_string(),
        format: format.to_string(),
        detail: err.to_string(),
    })
}
