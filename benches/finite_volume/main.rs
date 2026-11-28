#![forbid(unsafe_code)]

//! Fixed-volume scientific benchmark for the intrinsic periodic 1+1 CDT sampler.

mod acquire;
mod assessment;
mod bootstrap;
mod calibration;
mod data;
mod evidence;
mod moment_controls;
mod oracle;
mod probability;

use acquire::Cohort;
use evidence::{Error, file_hash, hex, write_json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn checkout() -> Option<serde_json::Value> {
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(["--no-pager", "--no-optional-locks"])
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    };
    let revision = String::from_utf8(git(&["rev-parse", "--verify", "HEAD"])?).ok()?;
    let status = git(&["status", "--porcelain", "--untracked-files=normal"])?;
    Some(serde_json::json!({"revision":revision.trim(),"clean":status.is_empty()}))
}

fn collect(directory: &Path, cohort: Cohort) -> Result<(), Error> {
    if let Some(parent) = directory.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(directory)?;
    write_json(
        &directory.join("acquisition.json"),
        &evidence::acquisition_manifest(cohort, checkout().as_ref())?,
    )?;
    for condition in cohort.conditions() {
        eprintln!(
            "{}: acquiring {} measurements per chain",
            condition.folder(),
            condition.draws
        );
        acquire::acquire(condition, &directory.join(condition.folder()))?;
    }
    Ok(())
}

fn run(path: &Path, cohort: Cohort) -> Result<(), Error> {
    collect(path, cohort)?;
    assessment::assess(path, &path.join("assessment"))
}

/// Fit the prospective histogram controls, including both held-out families, using synthetic data only.
fn calibrate(path: &Path) -> Result<(), Error> {
    std::fs::create_dir(path)?;
    let mut holdouts = Vec::new();
    let mut transfers = Vec::new();
    let mut fits = Vec::new();
    let mut passed = true;
    for c in Cohort::Precision.conditions() {
        eprintln!(
            "{}: fitting histogram controls at {} rows per chain",
            c.folder(),
            c.draws
        );
        let oracle = oracle::Oracle::new(c)?;
        let fit = calibration::Calibration::new(c, &oracle)?;
        let value = serde_json::to_value(&fit)?;
        let digest = hex(&Sha256::digest(serde_json::to_vec(&value)?));
        write_json(&path.join(format!("{}.json", c.folder())), &value)?;
        let matches_reference = digest == c.reference.calibration_sha256;
        fits.push(serde_json::json!({"condition":c,"sha256":digest,"controls_passed":fit.passed(),"matches_reference":matches_reference}));
        passed &= fit.passed() && matches_reference;
        transfers.push(calibration::BudgetTransfer::new(c, &oracle, &fit)?);
        holdouts.push(fit.panels.each_ref().map(calibration::Panel::scores));
    }
    let families = assessment::heldout_families_rejected(&holdouts);
    passed &= families.into_iter().all(calibration::error_rate_acceptable);
    let transfers = transfers
        .try_into()
        .map_err(|_| Error::Invalid("expected four transfer controls".into()))?;
    let panels = assessment::transfer_summary(&transfers);
    passed &= panels
        .iter()
        .flatten()
        .all(assessment::BudgetSummary::passed);
    let report = path.join("report.json");
    write_json(
        &report,
        &serde_json::json!({"fits":fits,"heldout_families_rejected":families,
            "frozen_holdout_scores":holdouts,"bootstrap_budget_trials":transfers,"bootstrap_budget_controls":panels,"passed":passed}),
    )?;
    if passed {
        Ok(())
    } else {
        Err(Error::Scientific(report))
    }
}

fn main() -> Result<(), Error> {
    const USAGE: &str = "finite_volume run NEW_DIRECTORY | confirm NEW_DIRECTORY | collect NEW_DIRECTORY | assess TRACES NEW_REPORT_DIRECTORY | moment-controls NEW_DIRECTORY | budget-controls NEW_DIRECTORY | calibrate NEW_DIRECTORY";
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let bench = args.last().is_some_and(|s| s == "--bench");
    if bench {
        args.pop();
    }
    match args.as_slice() {
        [command, directory] if command == "calibrate" => calibrate(Path::new(directory)),
        [command, directory] if command == "moment-controls" => {
            moment_controls::family_controls(Path::new(directory))
        }
        [command, directory] if command == "budget-controls" => {
            moment_controls::budget_controls(Path::new(directory))
        }
        [command, directory] if command == "collect" => {
            collect(Path::new(directory), Cohort::Precision)
        }
        [command, directory] if command == "run" => run(Path::new(directory), Cohort::Precision),
        [command, directory] if command == "confirm" => {
            run(Path::new(directory), Cohort::Confirmation)
        }
        [command, directory, output] if command == "assess" => {
            assessment::assess(Path::new(directory), Path::new(output))
        }
        [] if bench => {
            let instant = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let path = PathBuf::from(format!("target/finite-volume-{}", instant.as_millis()));
            eprintln!("Writing the scientific benchmark to {}", path.display());
            run(&path, Cohort::Precision)
        }
        [] => Ok(()),
        [command] if command == "--test" => Ok(()),
        _ => Err(Error::Invalid(USAGE.into())),
    }
}
