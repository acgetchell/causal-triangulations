#![forbid(unsafe_code)]

//! Example: writing simulation output files and using CDT checkpoints.
//!
//! This example runs a short CDT simulation, writes the configured trace CSV and
//! JSON outputs, round-trips a Delaunay-valid triangulation checkpoint, and
//! resumes an in-memory MCMC checkpoint.

use causal_triangulations::prelude::errors::{CheckpointOperation, OutputFormat};
use causal_triangulations::prelude::simulation::*;
use serde_json::{Value, from_str, to_string};
use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() -> CdtResult<()> {
    let output_dir =
        env::temp_dir().join(format!("causal-triangulations-output-{}", process::id()));
    let csv_path = output_dir.join("trace.csv");
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

    let checkpoint_source = CdtTriangulation2D::from_cdt_strip(4, 3)?;
    let checkpoint =
        to_string(&checkpoint_source).map_err(|err| CdtError::CheckpointSerializationFailed {
            operation: CheckpointOperation::Serialize,
            target: "Delaunay-valid triangulation".to_string(),
            detail: err.to_string(),
        })?;
    let restored: CdtTriangulation2D =
        from_str(&checkpoint).map_err(|err| CdtError::CheckpointSerializationFailed {
            operation: CheckpointOperation::Deserialize,
            target: "Delaunay-valid triangulation".to_string(),
            detail: err.to_string(),
        })?;
    restored.validate_topology()?;
    restored.validate_foliation()?;
    restored.validate_causality()?;
    restored.validate_simplex_classification()?;

    let mcmc_checkpoint = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(13),
        ActionConfig::default(),
    )
    .run_to_checkpoint(CdtTriangulation2D::from_cdt_strip(4, 3)?)?;
    let resumed = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 2, 0, 1)?.with_seed(999),
        ActionConfig::default(),
    )
    .resume_from_checkpoint(mcmc_checkpoint)?;

    println!("Trace CSV rows: {}", csv.lines().count().saturating_sub(1));
    println!(
        "JSON summary measurements: {}",
        summary["measurements"].as_array().map_or(0, Vec::len)
    );
    println!(
        "Delaunay-valid checkpoint roundtrip vertices: {}",
        restored.vertex_count()
    );
    println!(
        "Resumed MCMC checkpoint steps (in-memory): {}",
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
