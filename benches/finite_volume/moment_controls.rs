#![forbid(unsafe_code)]

//! Measure the production moment rule's rejection frequency under the exact conditional law.

use super::acquire::{CONDITIONS, Cohort, Condition};
use super::assessment::{Moment, moments};
use super::bootstrap::{MomentCoverage, PRODUCTION_REPLICATES, Sample, sample};
use super::calibration::{Generator, HOLDOUT, error_rate_acceptable};
use super::oracle::Oracle;
use super::{Error, file_hash, write_json};
use rand::{SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Serialize)]
struct Trial {
    seed: u64,
    visits: u32,
    point_misses: Vec<String>,
    interval_misses: Vec<String>,
}

impl Trial {
    fn new(seed: u64, visits: u32, scores: &[Moment]) -> Self {
        let point_misses = scores.iter().filter(|m| !m.within_tolerance);
        let interval_misses = scores.iter().filter(|m| !m.interval_covers_exact);
        Self {
            seed,
            visits,
            point_misses: point_misses.map(|m| m.name.clone()).collect(),
            interval_misses: interval_misses.map(|m| m.name.clone()).collect(),
        }
    }
}

#[derive(Serialize)]
struct Misses {
    any_covariance: usize,
    covariance_2_3: usize,
    any_mean: usize,
    any_interval: usize,
    full_rule: usize,
}

impl Misses {
    fn count(trials: &[Trial]) -> Self {
        Self {
            any_covariance: trials
                .iter()
                .filter(|t| t.point_misses.iter().any(|n| n.starts_with("cov[")))
                .count(),
            covariance_2_3: trials
                .iter()
                .filter(|t| t.point_misses.iter().any(|n| n == "cov[2,3]"))
                .count(),
            any_mean: trials
                .iter()
                .filter(|t| t.point_misses.iter().any(|n| n.starts_with("mean[")))
                .count(),
            any_interval: trials
                .iter()
                .filter(|t| !t.interval_misses.is_empty())
                .count(),
            full_rule: trials
                .iter()
                .filter(|t| !t.interval_misses.is_empty() || !t.point_misses.is_empty())
                .count(),
        }
    }
}

fn exact_law_sample(
    generator: &Generator<'_>,
    oracle: &Oracle,
    seed: u64,
    c: Condition,
) -> Result<Sample, Error> {
    let blocks = generator.blocks(&mut Xoshiro256PlusPlus::seed_from_u64(seed), false);
    sample(
        &blocks,
        &oracle.probabilities,
        Some(oracle),
        c.reference.bootstrap_seed,
        PRODUCTION_REPLICATES,
    )
    .ok_or_else(|| Error::Invalid(format!("empty exact-law sample for seed {seed}")))
}

fn panel(c: Condition, factor: u32, path: &Path) -> Result<Misses, Error> {
    let condition = Condition {
        draws: c.draws * factor,
        ..c
    };
    let oracle = Oracle::new(condition)?;
    let generator = Generator::new(condition, &oracle)?;
    let first_seed = u64::from(factor) * 3_000_000_000 + u64::from(c.triangles) * 10_000;
    let mut trials = Vec::with_capacity(HOLDOUT);
    for trial in 0..HOLDOUT {
        let seed = first_seed + trial as u64;
        let result = exact_law_sample(&generator, &oracle, seed, c)?;
        let scores = moments(&oracle, &result, MomentCoverage::PerTorus95)
            .ok_or_else(|| Error::Invalid(format!("unresolved moment for seed {seed}")))?;
        trials.push(Trial::new(seed, result.hits.iter().sum(), &scores));
        if (trial + 1).is_multiple_of(10) {
            eprintln!(
                "{} at {factor}x, {} of {HOLDOUT} exact-law cohorts",
                c.folder(),
                trial + 1
            );
        }
    }
    let misses = Misses::count(&trials);
    write_json(
        &path.join(format!("{}-{factor}x.json", c.folder())),
        &serde_json::json!({
            "condition":condition, "factor":factor, "bootstrap_seed":c.reference.bootstrap_seed,
            "misses":misses, "trials":trials,
        }),
    )?;
    Ok(misses)
}

pub fn budget_controls(path: &Path) -> Result<(), Error> {
    std::fs::create_dir(path)?;
    let executable = file_hash(&std::env::current_exe()?)?;
    let mut summaries = Vec::new();
    let mut covariance_budget_passed = true;
    let mut full_rule_passed = true;
    for (c, planned) in CONDITIONS.into_iter().zip(Cohort::Precision.conditions()) {
        let current = panel(c, 1, path)?;
        let increased = if error_rate_acceptable(current.any_covariance) {
            None
        } else {
            Some(panel(c, 4, path)?)
        };
        let factor = if increased.is_some() { 4 } else { 1 };
        let selected = increased.as_ref().unwrap_or(&current);
        covariance_budget_passed &=
            planned.draws == c.draws * factor && error_rate_acceptable(selected.any_covariance);
        full_rule_passed &= error_rate_acceptable(selected.full_rule);
        let mut artifacts = std::collections::BTreeMap::new();
        for scale in std::iter::once(1).chain(increased.as_ref().map(|_| 4)) {
            let name = format!("{}-{scale}x.json", c.folder());
            let digest = file_hash(&path.join(&name))?;
            artifacts.insert(name, digest);
        }
        summaries.push(serde_json::json!({"torus":c.folder(),"original_budget":current,"fourfold":increased,"details_sha256":artifacts}));
    }
    let report = path.join("report.json");
    write_json(
        &report,
        &serde_json::json!({
            "schema":"finite-volume-moment-controls/v1","trials_per_panel":HOLDOUT,
            "block_rows":super::data::BLOCK,"bootstrap_replicates":PRODUCTION_REPLICATES,
            "design":"Eight chains. Each independent four-row group hits with probability 1/4; its profile is drawn by exact integer weight.",
            "budget_rule":"Test a fresh fourfold panel when >=12 of 100 cohorts miss any covariance point tolerance.",
            "binary_sha256":executable,"conditions":summaries,"planned_covariance_budget_passed":covariance_budget_passed,
            "planned_full_rule_passed":full_rule_passed,
            "seed_rule":"3,000,000,000 * budget factor + 10,000 * triangles + trial index 0..99",
        }),
    )?;
    if covariance_budget_passed && full_rule_passed {
        Ok(())
    } else {
        Err(Error::Scientific(report))
    }
}

#[derive(Serialize)]
struct PairedTrial {
    #[serde(flatten)]
    family_rule: Trial,
    per_torus_interval_misses: Vec<String>,
}

#[derive(Serialize)]
struct FamilyMisses {
    per_torus_interval: usize,
    family_interval: usize,
    point: usize,
    per_torus_full_rule: usize,
    family_full_rule: usize,
}

fn family_counts(rows: &[Vec<PairedTrial>], indices: std::ops::Range<usize>) -> FamilyMisses {
    let mut misses = FamilyMisses {
        per_torus_interval: 0,
        family_interval: 0,
        point: 0,
        per_torus_full_rule: 0,
        family_full_rule: 0,
    };
    for i in indices {
        let point = rows
            .iter()
            .any(|r| !r[i].family_rule.point_misses.is_empty());
        let per_torus = rows
            .iter()
            .any(|r| !r[i].per_torus_interval_misses.is_empty());
        let family = rows
            .iter()
            .any(|r| !r[i].family_rule.interval_misses.is_empty());
        misses.point += usize::from(point);
        misses.per_torus_interval += usize::from(per_torus);
        misses.family_interval += usize::from(family);
        misses.per_torus_full_rule += usize::from(point || per_torus);
        misses.family_full_rule += usize::from(point || family);
    }
    misses
}

const FAMILIES: usize = 2 * HOLDOUT;

fn first_family_seed(c: Condition) -> u64 {
    20_000_000_000 + 10_000 * u64::from(c.triangles)
}

fn paired_trials(c: Condition) -> Result<Vec<PairedTrial>, Error> {
    let oracle = Oracle::new(c)?;
    let generator = Generator::new(c, &oracle)?;
    let mut trials = Vec::with_capacity(FAMILIES);
    for i in 0..FAMILIES {
        let seed = first_family_seed(c) + i as u64;
        let result = exact_law_sample(&generator, &oracle, seed, c)?;
        let score = |scope| {
            moments(&oracle, &result, scope)
                .ok_or_else(|| Error::Invalid(format!("unresolved moment for seed {seed}")))
        };
        let local = score(MomentCoverage::PerTorus95)?;
        let joint = score(MomentCoverage::from(Cohort::Precision))?;
        let hits = result.hits.iter().sum();
        let local = Trial::new(seed, hits, &local);
        let family_rule = Trial::new(seed, hits, &joint);
        if local.point_misses != family_rule.point_misses
            || !family_rule
                .interval_misses
                .iter()
                .all(|m| local.interval_misses.contains(m))
        {
            return Err(Error::Invalid(format!(
                "inconsistent paired decisions for seed {seed}"
            )));
        }
        trials.push(PairedTrial {
            family_rule,
            per_torus_interval_misses: local.interval_misses,
        });
        if (i + 1).is_multiple_of(25) {
            eprintln!("{}: {} of {FAMILIES} paired controls", c.folder(), i + 1);
        }
    }
    Ok(trials)
}

pub fn family_controls(path: &Path) -> Result<(), Error> {
    std::fs::create_dir(path)?;
    let conditions = Cohort::Precision.conditions();
    let plan = path.join("plan.json");
    let hash = |bytes: &[u8]| super::hex(&Sha256::digest(bytes));
    let protocol = serde_json::json!({
        "schema":"finite-volume-family-controls/v1", "conditions":conditions,
        "first_seed":conditions.map(first_family_seed), "bootstrap_seeds":conditions.map(|c| c.reference.bootstrap_seed),
        "bootstrap_replicates":PRODUCTION_REPLICATES,"block_rows":super::data::BLOCK,
        "panels":2,"families_per_panel":HOLDOUT,"chains_per_torus":8,
        "null_model":"Independent groups of four cadence rows hit with probability 1/4 and repeat one exact-law profile.",
        "procedure":"Bonferroni: 5%/4 per torus, applied to the within-torus bootstrap maximum. Pair both methods on identical samples and replicates.",
        "endpoint":"Count a family once when any labeled interval misses. Count point misses and the full moment rule separately.",
        "decision":"Flag 12 or more interval misses or 12 or more full-rule misses in either fixed panel of 100. Report both panels.",
        "binary_sha256":file_hash(&std::env::current_exe()?)?, "checkout":super::checkout(),
        "cargo_lock_sha256":hash(include_bytes!("../../Cargo.lock")),
        "source_sha256":{
            "acquire.rs":hash(include_bytes!("acquire.rs")), "assessment.rs":hash(include_bytes!("assessment.rs")),
            "bootstrap.rs":hash(include_bytes!("bootstrap.rs")), "calibration.rs":hash(include_bytes!("calibration.rs")),
            "moment_controls.rs":hash(include_bytes!("moment_controls.rs")), "oracle.rs":hash(include_bytes!("oracle.rs")),
            "data.rs":hash(include_bytes!("data.rs")), "probability.rs":hash(include_bytes!("probability.rs")),
        },
    });
    write_json(&plan, &protocol)?;
    let mut rows = Vec::new();
    let mut artifacts = std::collections::BTreeMap::new();
    for c in conditions {
        let trials = paired_trials(c)?;
        let name = format!("{}.json", c.folder());
        let detail = path.join(&name);
        write_json(&detail, &trials)?;
        artifacts.insert(name, file_hash(&detail)?);
        rows.push(trials);
    }
    let panels = [
        family_counts(&rows, 0..HOLDOUT),
        family_counts(&rows, HOLDOUT..FAMILIES),
    ];
    let passed = panels.iter().all(|p| {
        error_rate_acceptable(p.family_interval) && error_rate_acceptable(p.family_full_rule)
    });
    let report = path.join("report.json");
    write_json(
        &report,
        &serde_json::json!({
            "schema":"finite-volume-family-controls/v1","families":FAMILIES,"families_per_panel":HOLDOUT,
            "plan":protocol,"plan_sha256":file_hash(&plan)?,"details_sha256":artifacts,"panels":panels,
            "total":family_counts(&rows, 0..FAMILIES),"passed":passed,
        }),
    )?;
    if passed {
        Ok(())
    } else {
        Err(Error::Scientific(report))
    }
}
