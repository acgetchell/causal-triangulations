#![forbid(unsafe_code)]

//! Score all declared conditions together and preserve the information behind each verdict.

use super::acquire::{CONDITIONS, Cohort, Condition};
use super::bootstrap::{MomentCoverage, PRODUCTION_REPLICATES, Sample, sample, simultaneous};
use super::calibration::{
    BudgetTransfer, Calibration, GoodnessOfFit, HOLDOUT, Panel, PanelScores, error_rate_acceptable,
};
use super::data::{self, Diagnostic};
use super::evidence::{BUILD_RUSTC, build_source_hash};
use super::oracle::Oracle;
use super::probability::PValue;
use super::{Error, file_hash, write_json};
use la_stack::{BigInt, BigRational};
use num_traits::{Signed, ToPrimitive, Zero};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

type Family<T> = [T; CONDITIONS.len()];

pub fn holm(values: Family<PValue>) -> Family<PValue> {
    const MULTIPLIERS: Family<u16> = [4, 3, 2, 1];
    let mut indices: Family<usize> = std::array::from_fn(|i| i);
    indices.sort_by_key(|&i| values[i]);
    let mut adjusted = [PValue::ZERO; CONDITIONS.len()];
    let mut previous = PValue::ZERO;
    for (i, multiplier) in indices.into_iter().zip(MULTIPLIERS) {
        previous = previous.max(values[i].adjusted(multiplier));
        adjusted[i] = previous;
    }
    adjusted
}

fn family_rejections(family: Family<GoodnessOfFit>) -> (bool, bool) {
    let rejects = |ps| holm(ps).into_iter().any(|p| p <= PValue::ALPHA);
    (
        rejects(family.map(|p| p.raw)),
        rejects(family.map(|p| p.calibrated)),
    )
}

pub fn heldout_families_rejected(scores: &[[PanelScores; 2]]) -> [usize; 2] {
    [0, 1].map(|panel| {
        (0..HOLDOUT)
            .filter(|&i| {
                let (raw, calibrated) =
                    family_rejections(std::array::from_fn(|j| scores[j][panel].null[i]));
                raw || calibrated
            })
            .count()
    })
}

#[derive(Serialize)]
pub struct BudgetSummary {
    raw_holm_rejections: usize,
    calibrated_holm_rejections: usize,
    combined_holm_rejections: usize,
    null_radius_misses: Family<usize>,
    biased_radius_misses: Family<usize>,
}

impl BudgetSummary {
    pub fn passed(&self) -> bool {
        error_rate_acceptable(self.combined_holm_rejections)
            && self
                .null_radius_misses
                .into_iter()
                .chain(self.biased_radius_misses)
                .all(error_rate_acceptable)
    }
}

/// Rows are independent panels, columns are the training and production bootstrap budgets.
pub fn transfer_summary(tori: &Family<BudgetTransfer>) -> [[BudgetSummary; 2]; 2] {
    std::array::from_fn(|panel| {
        std::array::from_fn(|budget| {
            let range = panel * HOLDOUT..(panel + 1) * HOLDOUT;
            let mut raw_holm_rejections = 0;
            let mut calibrated_holm_rejections = 0;
            let mut combined_holm_rejections = 0;
            for i in range.clone() {
                let (raw, calibrated) =
                    family_rejections(tori.each_ref().map(|t| t.null[i][budget].p));
                raw_holm_rejections += usize::from(raw);
                calibrated_holm_rejections += usize::from(calibrated);
                combined_holm_rejections += usize::from(raw || calibrated);
            }
            BudgetSummary {
                raw_holm_rejections,
                calibrated_holm_rejections,
                combined_holm_rejections,
                null_radius_misses: tori.each_ref().map(|t| {
                    range
                        .clone()
                        .filter(|&i| t.null[i][budget].radius_missed)
                        .count()
                }),
                biased_radius_misses: tori.each_ref().map(|t| {
                    range
                        .clone()
                        .filter(|&i| t.biased[i][budget].radius_missed)
                        .count()
                }),
            }
        })
    })
}

#[derive(Serialize)]
pub struct Moment {
    pub name: String,
    exact: f64,
    exact_fraction: String,
    estimate: f64,
    estimate_fraction: String,
    interval95: (f64, f64),
    relative_error: Option<f64>,
    pub within_tolerance: bool,
    pub interval_covers_exact: bool,
    passes: bool,
}

pub fn moments(o: &Oracle, s: &Sample, coverage: MomentCoverage) -> Option<Vec<Moment>> {
    let exact = o.exact_moments(&o.weights)?;
    let sampled = o.exact_moments(&s.counts.iter().copied().map(u64::from).collect::<Vec<_>>())?;
    let estimates = sampled
        .iter()
        .map(ToPrimitive::to_f64)
        .collect::<Option<Vec<_>>>()?;
    let intervals = simultaneous(&estimates, &s.boot_moments, coverage)?;
    let t = o.profiles[0].len();
    let labels = (0..t)
        .map(|i| (format!("mean[{i}]"), 100))
        .chain((0..t).flat_map(|i| (i..t).map(move |j| (format!("cov[{i},{j}]"), 20))));
    labels
        .zip(exact)
        .zip(sampled)
        .zip(intervals)
        .map(
            |((((name, inverse_tolerance), truth), observed), interval95)| {
                let difference = (&observed - &truth).abs();
                let within =
                    truth.is_zero() || &difference * BigInt::from(inverse_tolerance) <= truth.abs();
                let covered = BigRational::from_float(interval95.0)? <= truth
                    && truth <= BigRational::from_float(interval95.1)?;
                Some(Moment {
                    name,
                    exact: truth.to_f64()?,
                    exact_fraction: truth.to_string(),
                    estimate: observed.to_f64()?,
                    estimate_fraction: observed.to_string(),
                    interval95,
                    within_tolerance: within,
                    interval_covers_exact: covered,
                    relative_error: if truth.is_zero() {
                        None
                    } else {
                        Some((difference / truth.abs()).to_f64()?)
                    },
                    passes: within && covered,
                })
            },
        )
        .collect()
}

#[derive(Serialize)]
struct Observation {
    hits: Vec<u32>,
    visited: usize,
    tv: f64,
    bootstrap_radius: f64,
    tv_upper95: f64,
    deviance: f64,
    raw_p: PValue,
    calibrated_p: PValue,
    moments: Option<Vec<Moment>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Serialize)]
struct Outcome {
    condition: Condition,
    normalizer: u64,
    diagnostics: Vec<Diagnostic>,
    conditioned_information: data::ConditionedInformation,
    observation: Option<Observation>,
    controls_passed: bool,
    null_rejected: [usize; 2],
    biased_rejected: [usize; 2],
    holm_p: Option<PValue>,
    calibrated_holm_p: Option<PValue>,
    status: Status,
    detail_sha256: String,
    acquisition_sha256: BTreeMap<String, String>,
}

struct Evaluated {
    outcome: Outcome,
    holdouts: [PanelScores; 2],
    transfer: BudgetTransfer,
}

fn evaluate(
    dir: &Path,
    output: &Path,
    c: Condition,
    coverage: MomentCoverage,
) -> Result<Evaluated, Error> {
    eprintln!("{}: checking cadence traces and exact weights", c.folder());
    let o = Oracle::new(c)?;
    let raw = data::load(&dir.join(c.folder()), c, &o)?;
    let conditioned_information = data::ConditionedInformation::new(&raw.blocks, c, &o);
    let s = sample(
        &raw.blocks,
        &o.probabilities,
        Some(&o),
        c.reference.bootstrap_seed,
        PRODUCTION_REPLICATES,
    );
    eprintln!(
        "{}: evaluating independent calibration controls",
        c.folder()
    );
    let control = Calibration::new(c, &o)?;
    let calibration = serde_json::to_value(&control)?;
    let digest = super::hex(&Sha256::digest(serde_json::to_vec(&calibration)?));
    if digest != c.reference.calibration_sha256 {
        return Err(Error::Invalid(format!(
            "{}: calibration differs from its frozen reference",
            c.folder()
        )));
    }
    eprintln!("{}: checking fresh paired-budget holdouts", c.folder());
    let transfer = BudgetTransfer::new(c, &o, &control)?;
    let visit_counts = s.as_ref().map(|s| &s.counts);
    let profiles = o
        .profiles
        .iter()
        .zip(&o.weights)
        .enumerate()
        .map(|(i, (p, w))| json!([p, w, visit_counts.map(|v| v[i])]))
        .collect::<Vec<_>>();
    let detail = output.join(format!("{}.json", c.folder()));
    write_json(
        &detail,
        &json!({"profiles_lengths_weight_visits":profiles,"calibration":calibration,"trace_sha256":raw.hashes}),
    )?;
    let observation = s.map(|s| Observation {
        moments: moments(&o, &s, coverage),
        visited: s.counts.iter().filter(|&&n| n != 0).count(),
        tv: s.tv,
        bootstrap_radius: s.radius,
        tv_upper95: control.multiplier.mul_add(s.radius, s.tv).min(1.0),
        deviance: s.statistic,
        raw_p: s.p,
        calibrated_p: control.p(s.p),
        hits: s.hits,
    });
    let result = Outcome {
        condition: c,
        normalizer: o.normalizer,
        observation,
        diagnostics: raw.diagnostics,
        conditioned_information,
        controls_passed: control.passed(),
        null_rejected: control.panels.each_ref().map(|p| p.null.rejected),
        biased_rejected: control.panels.each_ref().map(|p| p.biased.rejected),
        holm_p: None,
        calibrated_holm_p: None,
        status: Status::Inconclusive,
        detail_sha256: file_hash(&detail)?,
        acquisition_sha256: raw.hashes,
    };
    Ok(Evaluated {
        outcome: result,
        holdouts: control.panels.each_ref().map(Panel::scores),
        transfer,
    })
}

pub fn declared_design(metadata: &serde_json::Value) -> Option<(Cohort, MomentCoverage)> {
    let schema = metadata["schema"].as_str()?;
    let cohort = match schema {
        "finite-volume-acquisition/v1" if metadata.get("cohort").is_none() => Cohort::Development,
        "finite-volume-acquisition/v2"
        | "finite-volume-acquisition/v3"
        | "finite-volume-acquisition/v4" => {
            let cohort: Cohort = serde_json::from_value(metadata["cohort"].clone()).ok()?;
            if metadata["reference"] != json!(cohort.conditions().map(|c| c.reference)) {
                return None;
            }
            cohort
        }
        _ => return None,
    };
    let coverage = if matches!(
        schema,
        "finite-volume-acquisition/v3" | "finite-volume-acquisition/v4"
    ) {
        let declared = MomentCoverage::from(cohort);
        (metadata["moment_interval_scope"] == json!(declared)).then_some(declared)?
    } else {
        metadata
            .get("moment_interval_scope")
            .is_none()
            .then_some(MomentCoverage::PerTorus95)?
    };
    (metadata["conditions"] == json!(cohort.conditions())).then_some((cohort, coverage))
}

pub fn collector_identified(metadata: &serde_json::Value) -> bool {
    let (source, compiler) = match metadata["schema"].as_str() {
        Some("finite-volume-acquisition/v4") => ("source_tree_sha256", "build_rustc"),
        Some(
            "finite-volume-acquisition/v1"
            | "finite-volume-acquisition/v2"
            | "finite-volume-acquisition/v3",
        ) => ("collection_source_sha256", "compiler"),
        _ => return false,
    };
    metadata[compiler]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
        && ["binary_sha256", "cargo_lock_sha256", source]
            .into_iter()
            .all(|key| {
                metadata[key]
                    .as_str()
                    .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            })
}

pub fn assess(dir: &Path, output: &Path) -> Result<(), Error> {
    let metadata: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(dir.join("acquisition.json"))?)?;
    let (cohort, coverage) = declared_design(&metadata)
        .filter(|_| collector_identified(&metadata))
        .ok_or_else(|| Error::Invalid("acquisition does not cover the declared matrix".into()))?;
    std::fs::create_dir(output)?;
    let mut outcomes = Vec::new();
    let mut holdouts = Vec::new();
    let mut transfers = Vec::new();
    for c in cohort.conditions() {
        let evaluated = evaluate(dir, output, c, coverage)?;
        outcomes.push(evaluated.outcome);
        holdouts.push(evaluated.holdouts);
        transfers.push(evaluated.transfer);
    }
    let transfers: Family<_> = transfers
        .try_into()
        .map_err(|_| Error::Invalid("expected four transfer controls".into()))?;
    let transfer = transfer_summary(&transfers);
    let transfer_passed = transfer.iter().flatten().all(BudgetSummary::passed);
    write_json(
        &output.join("bootstrap-budget-controls.json"),
        &json!({"conditions":cohort.conditions(), "frozen_holdout_scores":holdouts,
            "trials":transfers, "panels":transfer, "passed":transfer_passed}),
    )?;
    let rejected = heldout_families_rejected(&holdouts);
    let family_control_passed = rejected.into_iter().all(error_rate_acceptable);
    let probabilities: Option<Vec<_>> = outcomes
        .iter()
        .map(|r| r.observation.as_ref().map(|s| (s.raw_p, s.calibrated_p)))
        .collect();
    if let Some(probabilities) = probabilities {
        let raw = holm(std::array::from_fn(|i| probabilities[i].0));
        let calibrated = holm(std::array::from_fn(|i| probabilities[i].1));
        for ((r, p), q) in outcomes.iter_mut().zip(raw).zip(calibrated) {
            r.holm_p = Some(p);
            r.calibrated_holm_p = Some(q);
            if let Some(s) = &r.observation
                && let Some(m) = &s.moments
                && r.diagnostics.iter().all(Diagnostic::sufficient)
                && r.conditioned_information.sufficient()
                && transfer_passed
                && family_control_passed
            {
                r.status = if r.controls_passed
                    && p > PValue::ALPHA
                    && q > PValue::ALPHA
                    && s.tv_upper95 <= 0.05
                    && m.iter().all(|m| m.passes)
                {
                    Status::Pass
                } else {
                    Status::Fail
                };
            }
        }
    }
    let passed = transfer_passed
        && family_control_passed
        && outcomes.iter().all(|r| r.status == Status::Pass);
    let source_hash = |bytes: &[u8]| super::hex(&Sha256::digest(bytes));
    write_json(
        &output.join("report.json"),
        &json!({
            "schema":"finite-volume-benchmark/v5",
            "moment_interval_scope": coverage, "acquisition": metadata,
            "analysis_binary_sha256": file_hash(&std::env::current_exe()?)?,
            "analysis_build_rustc": BUILD_RUSTC,
            "analysis_source_tree_sha256": build_source_hash(),
            "analysis_cargo_lock_sha256": source_hash(include_bytes!("../../Cargo.lock")),
            "analysis_source_sha256": {
                "assessment.rs": source_hash(include_bytes!("assessment.rs")),
                "bootstrap.rs": source_hash(include_bytes!("bootstrap.rs")),
                "calibration.rs": source_hash(include_bytes!("calibration.rs")),
                "data.rs": source_hash(include_bytes!("data.rs")),
                "oracle.rs": source_hash(include_bytes!("oracle.rs")),
                "probability.rs": source_hash(include_bytes!("probability.rs")),
            },
            "block_rows": data::BLOCK,
            "bootstrap_budget_controls":transfer,"bootstrap_budget_controls_passed":transfer_passed,
            "bootstrap_budget_controls_sha256":file_hash(&output.join("bootstrap-budget-controls.json"))?,
            "conditions": outcomes, "panels": ["original", "confirmation"],
            "heldout_families_rejected": rejected, "heldout_families_per_panel": HOLDOUT,
            "family_control_passed": family_control_passed, "passed": passed
        }),
    )?;
    for r in outcomes {
        eprintln!("{}: {:?}", r.condition.folder(), r.status);
    }
    if passed {
        Ok(())
    } else {
        Err(Error::Scientific(output.join("report.json")))
    }
}
