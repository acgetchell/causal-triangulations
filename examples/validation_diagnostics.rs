#![forbid(unsafe_code)]

//! Export independent periodic chains and analyze them without a notebook.
//!
//! These short fixtures demonstrate the contract, not the v0.2.0 physics gate.

use causal_triangulations::CdtError;
use causal_triangulations::prelude::action::VolumeFixing;
use causal_triangulations::prelude::errors::{OutputFormat, OutputWriteStage};
use causal_triangulations::prelude::observables::*;
use causal_triangulations::prelude::{CdtConfig, CdtResult, CdtTopology, run_simulation};
use std::env;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

fn main() -> CdtResult<()> {
    let directory = env::args_os().nth(1).map_or_else(
        || PathBuf::from("target/validation-diagnostics"),
        PathBuf::from,
    );
    fs::create_dir_all(&directory).map_err(|error| {
        output_error(&directory, OutputWriteStage::CreateParentDirectory, error)
    })?;
    let mut chains = Vec::new();
    for (index, profile) in [vec![4, 4, 4, 4], vec![3, 5, 4, 4]].into_iter().enumerate() {
        let seed = if index == 0 { 101 } else { 202 };
        let config = CdtConfig {
            topology: CdtTopology::Toroidal,
            spatial_vertex_profile: Some(profile),
            volume_fixing: Some(VolumeFixing::quadratic_triangles(32, 0.1)?),
            simulate: true,
            seed: Some(seed),
            steps: 32,
            thermalization_steps: 4,
            measurement_frequency: 1,
            ..CdtConfig::new(16, 4)
        }
        .into_validated()?;
        let results = run_simulation(&config)?;
        let chain = ValidationChain::from_results(
            "validation-example",
            &format!("chain-{index}"),
            &config,
            &results,
        )?;
        let path = directory.join(format!("chain-{index}.json"));
        let mut writer = output_writer(&path)?;
        chain.write_json(&mut writer)?;
        writer
            .flush()
            .map_err(|error| output_error(&path, OutputWriteStage::Flush, error))?;
        let file = File::open(&path).map_err(|error| CdtError::OutputReadFailed {
            path: path.display().to_string(),
            format: OutputFormat::Json,
            detail: error.to_string(),
        })?;
        chains.push(ValidationChain::read_json(file)?);
    }
    let policy = CdtDiagnosticPolicy {
        maximum_lag: 16,
        minimum_completed_blocks: 16,
        stabilization_levels: 3,
        binning_relative_tolerance: 0.2,
        require_stable_binning: true,
        minimum_chains: 8,
        minimum_ess: 400.0,
        maximum_mean_mcse: 0.1,
        quantiles: vec![QuantileRequirement {
            probability: 0.5,
            maximum_mcse: 0.1,
        }],
    };
    let report = analyze_cdt_chains(
        &chains,
        CdtDiagnosticObservable::LoopLength { slice: 0 },
        &policy,
        None,
    )?;
    let path = directory.join("diagnostics.json");
    let mut writer = output_writer(&path)?;
    report.write_json(&mut writer)?;
    writer
        .flush()
        .map_err(|error| output_error(&path, OutputWriteStage::Flush, error))?;
    println!("Cadence-aligned chain artifacts: {}", chains.len());
    println!("Diagnostic information verdict: {:?}", report.verdict);
    println!("Raw chains and diagnostics: {}", directory.display());
    Ok(())
}

/// Opens one example output with the normal typed file-error contract.
fn output_writer(path: &Path) -> CdtResult<BufWriter<File>> {
    File::create(path)
        .map(BufWriter::new)
        .map_err(|error| output_error(path, OutputWriteStage::CreateFile, error))
}

/// Retains the path and failed operation for example I/O.
fn output_error(path: &Path, stage: OutputWriteStage, error: impl Display) -> CdtError {
    CdtError::OutputWriteFailed {
        path: path.display().to_string(),
        format: OutputFormat::Json,
        stage,
        detail: error.to_string(),
    }
}
