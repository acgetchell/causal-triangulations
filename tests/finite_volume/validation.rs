#![forbid(unsafe_code)]

//! Bounded controls for the finite-volume acquisition and statistical assessment.

use super::Error;
use super::acquire::{self, CONDITIONS, Cohort, Condition, Draw};
use super::assessment::{
    BudgetSummary, assess, collector_identified, declared_design, heldout_families_rejected, holm,
    moments, transfer_summary,
};
use super::bootstrap::{MomentCoverage, sample, simultaneous};
use super::calibration::{
    BudgetTransfer, Calibration, Generator, GoodnessOfFit, HOLDOUT, PanelScores, TransferTrial,
    error_rate_acceptable,
};
use super::data::{self, Diagnostic, RowError};
use super::oracle::Oracle;
use super::probability::PValue;
use crate::{CdtSimplexCounts, SimplexCountField};
use la_stack::{BigInt, BigRational};
use rand::{SeedableRng, rngs::Xoshiro256PlusPlus};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[test]
fn production_cohorts_have_disjoint_streams_and_budget_specific_references() {
    let mut seeds = HashSet::new();
    for ((old, fresh), planned) in CONDITIONS
        .into_iter()
        .zip(Cohort::Confirmation.conditions())
        .zip(Cohort::Precision.conditions())
    {
        assert_eq!(old.reference, fresh.reference);
        if old.triangles == 64 {
            assert_eq!(planned.draws, 8_388_608);
            assert_ne!(
                planned.reference.calibration_sha256,
                old.reference.calibration_sha256
            );
        } else {
            assert_eq!(planned.draws, old.draws);
            assert_eq!(planned.reference, old.reference);
        }
        // These are the analysis streams of the recorded development run.
        assert_eq!(fresh.reference.bootstrap_seed, old.first_seed + 100);
        assert_eq!(fresh.reference.calibration_seed, old.first_seed + 100_100);
        assert_eq!(fresh.reference.reachability_seed, old.first_seed - 1);
        for seed in [
            old.reference.bootstrap_seed,
            old.reference.calibration_seed,
            old.reference.reachability_seed,
        ] {
            assert!(seeds.insert(seed));
        }
        for slot in 0..8 {
            assert!(seeds.insert(old.first_seed + slot));
            assert!(seeds.insert(fresh.first_seed + slot));
            assert!(seeds.insert(planned.first_seed + slot));
        }
    }
}

#[test]
fn acquisition_identifies_its_cohort_and_fixed_reference() {
    let mut manifest = serde_json::json!({
        "schema": "finite-volume-acquisition/v1", "conditions": CONDITIONS,
    });
    assert_eq!(
        declared_design(&manifest),
        Some((Cohort::Development, MomentCoverage::PerTorus95))
    );
    manifest["conditions"] = serde_json::json!(Cohort::Confirmation.conditions());
    assert_eq!(declared_design(&manifest), None);
    manifest["schema"] = "finite-volume-acquisition/v2".into();
    manifest["reference"] = serde_json::json!(CONDITIONS.map(|c| c.reference));
    manifest["cohort"] = "development".into();
    assert_eq!(declared_design(&manifest), None);
    manifest["cohort"] = "confirmation".into();
    assert_eq!(
        declared_design(&manifest),
        Some((Cohort::Confirmation, MomentCoverage::PerTorus95))
    );
    manifest["reference"][0]["calibration_seed"] = 0.into();
    assert_eq!(declared_design(&manifest), None);
    let precision = Cohort::Precision.conditions();
    manifest["conditions"] = serde_json::json!(precision);
    manifest["reference"] = serde_json::json!(precision.map(|c| c.reference));
    manifest["cohort"] = "precision".into();
    assert_eq!(
        declared_design(&manifest),
        Some((Cohort::Precision, MomentCoverage::PerTorus95))
    );
    manifest["moment_interval_scope"] = serde_json::json!(MomentCoverage::FourTorusFamily95);
    assert_eq!(declared_design(&manifest), None);
    manifest["schema"] = "finite-volume-acquisition/v3".into();
    assert_eq!(
        declared_design(&manifest),
        Some((Cohort::Precision, MomentCoverage::FourTorusFamily95))
    );
    manifest["moment_interval_scope"] = serde_json::json!(MomentCoverage::PerTorus95);
    assert_eq!(declared_design(&manifest), None);
    manifest["moment_interval_scope"] = serde_json::json!(MomentCoverage::FourTorusFamily95);
    manifest["reference"][3] = serde_json::json!(CONDITIONS[3].reference);
    assert_eq!(declared_design(&manifest), None);
}

#[test]
fn current_manifest_requires_build_provenance_even_with_legacy_fields_present() -> Result<(), Error>
{
    let mut manifest = super::evidence::acquisition_manifest(Cohort::Precision, None)?;
    assert_eq!(
        declared_design(&manifest),
        Some((Cohort::Precision, MomentCoverage::FourTorusFamily95))
    );
    assert!(collector_identified(&manifest));
    assert!(
        manifest["build_rustc"]
            .as_str()
            .is_some_and(|s| s.contains("commit-hash:"))
    );
    manifest["compiler"] = "runtime rustc".into();
    manifest["collection_source_sha256"] = manifest["source_tree_sha256"].clone();
    manifest["source_tree_sha256"] = "invalid".into();
    assert!(!collector_identified(&manifest));
    manifest["source_tree_sha256"] = manifest["collection_source_sha256"].clone();
    manifest["build_rustc"] = serde_json::Value::Null;
    assert!(!collector_identified(&manifest));
    manifest["schema"] = "finite-volume-acquisition/v3".into();
    assert!(collector_identified(&manifest));
    Ok(())
}

#[test]
fn exact_transfer_coefficients_and_labeled_moments() -> Result<(), Error> {
    // Fixed-volume traces from the generating function in Ambjørn and Loll, eq. (17).
    // The second trace inserts l[2] * l[3] before summing the closed paths.
    let traces: [(u64, u64); 4] = [
        (27_957_475, 439_678_750),
        (11_608_941_420, 284_328_523_740),
        (3_312_100_041_142, 116_957_980_072_846),
        (4_207_880_073_023_875, 67_158_448_320_793_750),
    ];
    for (c, (total, weighted_pair)) in CONDITIONS.into_iter().zip(traces) {
        let oracle = Oracle::new(c)?;
        assert_eq!(oracle.normalizer, total);
        assert_eq!(oracle.profiles.len(), c.profiles);
        let moments = oracle
            .exact_moments(&oracle.weights)
            .expect("exact moments");
        let t = c.slices as usize;
        let mean = BigRational::new(c.triangles.into(), (2 * c.slices).into());
        assert!(moments[..t].iter().all(|x| *x == mean));
        let covariance = BigRational::new(weighted_pair.into(), total.into()) - &mean * &mean;
        // After t means, skip covariance rows 0 and 1, then the (2,2) entry.
        assert_eq!(moments[3 * t], covariance);
        if c.triangles == 32 {
            let flat = oracle
                .profiles
                .binary_search(&vec![4; 4])
                .expect("flat is in the support");
            assert_eq!(oracle.weights[flat], 35_u64.pow(4));
        }
    }
    Ok(())
}

#[test]
fn weighted_eight_slice_covariances_keep_seam_labels() -> Result<(), Error> {
    let oracle = Oracle::new(CONDITIONS[3])?;
    let mut counts = vec![0; oracle.profiles.len()];
    for (label, weight) in [(0, 1), (7, 3)] {
        let mut peak = vec![3; 8];
        peak[label] = 11;
        counts[oracle
            .profiles
            .binary_search(&peak)
            .expect("admissible labeled peak")] = weight;
    }
    let moments = oracle.moments(&counts).expect("finite moments");
    assert_eq!(&moments[..8], [5.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 9.0]);
    let mut pair = 8;
    for i in 0..8 {
        for j in i..8 {
            let expected = match (i, j) {
                (0, 0) | (7, 7) => 12.0,
                (0, 7) => -12.0,
                _ => 0.0,
            };
            assert!((moments[pair] - expected).abs() < 1e-14);
            pair += 1;
        }
    }
    Ok(())
}

#[test]
fn exact_centering_preserves_rare_visits_above_binary64_integer_precision() -> Result<(), Error> {
    let c = CONDITIONS[0];
    let oracle = Oracle::new(c)?;
    let mut counts = vec![0; oracle.profiles.len()];
    assert!(oracle.exact_moments(&counts).is_none());
    let flat = oracle
        .profiles
        .binary_search(&c.start(0))
        .expect("admissible flat");
    let peak = oracle
        .profiles
        .binary_search(&c.start(4))
        .expect("admissible peak");
    counts[flat] = u64::MAX;
    counts[peak] = 1;
    let n = BigInt::from(u64::MAX) + 1u8;
    // One observation has length seven, every other length is four.
    let exact = oracle.exact_moments(&counts).expect("positive total");
    assert_eq!(exact[0], BigRational::new(4 * &n + 3, n.clone()));
    assert_eq!(
        exact[4],
        BigRational::new(9 * BigInt::from(u64::MAX), &n * &n)
    );
    Ok(())
}

#[test]
fn collector_identifies_the_failed_geometry_invariant() {
    let measure = |slices, profile: [usize; 4], vertices, edges, triangles| {
        let counts =
            CdtSimplexCounts::try_new(vertices, edges, triangles).expect("positive counts");
        Draw::from_counts(16_400, slices, profile.into_iter(), counts)
    };
    let profile = [3, 4, 4, 4];
    let draw = measure(4, profile, 15, 45, 30).expect("consistent off-target draw");
    assert_eq!(
        (draw.step, draw.vertices, draw.edges, draw.triangles),
        (16_400, 15, 45, 30)
    );
    assert_eq!(draw.profile, [3, 4, 4, 4]);
    assert!(matches!(
        measure(8, profile, 15, 45, 30),
        Err(acquire::Error::TimeExtent {
            step: 16_400,
            expected: 8,
            actual: 4
        })
    ));
    assert!(matches!(
        measure(4, [3, 2, 4, 4], 13, 39, 26),
        Err(acquire::Error::ShortLoop {
            step: 16_400,
            slice: 1,
            actual: 2
        })
    ));
    for (vertices, edges, triangles, field, expected, actual) in [
        (14, 42, 28, SimplexCountField::Vertices, 15, 14),
        (15, 45, 29, SimplexCountField::Triangles, 30, 29),
        (15, 44, 30, SimplexCountField::Edges, 45, 44),
    ] {
        assert!(matches!(measure(4, profile, vertices, edges, triangles),
            Err(acquire::Error::CountMismatch { step: 16_400, field: failed, expected: wanted, actual: found })
            if (failed, wanted, found) == (field, expected, actual)));
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn collector_distinguishes_unrepresentable_measurements_from_bad_geometry() {
    let oversized = u32::MAX as usize + 1;
    let counts = CdtSimplexCounts::try_new(15, 45, 30).expect("positive counts");
    assert!(
        matches!(Draw::from_counts(32, 4, [3, 4, oversized, 4].into_iter(), counts),
        Err(acquire::Error::SliceLengthOutOfRange { step: 32, slice: 2, actual, .. }) if actual == oversized)
    );
    let counts = CdtSimplexCounts::try_new(15, oversized, 30).expect("positive counts");
    assert!(
        matches!(Draw::from_counts(32, 4, [3, 4, 4, 4].into_iter(), counts),
        Err(acquire::Error::CountOutOfRange { step: 32, field: SimplexCountField::Edges, actual, .. }) if actual == oversized)
    );
}

#[test]
fn malformed_off_sector_geometry_is_rejected_before_conditioning() {
    let c = CONDITIONS[0];
    let mut row = Draw {
        edges: 45,
        vertices: 15,
        triangles: 30,
        step: 16_400,
        profile: vec![3, 4, 4, 4],
    };
    let support = HashMap::new();
    assert!(matches!(data::count_row(&row, c, 0, &support), Ok(None)));
    row.edges = 44;
    assert!(matches!(
        data::count_row(&row, c, 0, &support),
        Err(RowError::Field {
            field: "edges",
            expected: 45,
            actual: 44
        })
    ));
    row.edges = 45;
    assert!(matches!(
        data::count_row(&row, c, 1, &support),
        Err(RowError::Field {
            field: "step",
            expected: 16_416,
            actual: 16_400
        })
    ));
}

#[test]
fn assessment_rejects_an_incomplete_acquisition_before_writing_output() -> Result<(), Error> {
    let dir =
        std::env::temp_dir().join(format!("cdt-incomplete-acquisition-{}", std::process::id()));
    std::fs::create_dir(&dir)?;
    std::fs::write(dir.join("acquisition.json"), b"{}")?;
    let out = dir.join("assessment");
    let result = assess(&dir, &out);
    let output_created = out.exists();
    std::fs::remove_dir_all(dir)?;
    assert!(matches!(result, Err(Error::Invalid(message)) if message.contains("declared matrix")));
    assert!(!output_created);
    Ok(())
}

#[test]
fn acquisition_round_trip_binds_diagnostics_and_rejects_corrupt_traces() -> Result<(), Error> {
    let c = Condition {
        draws: 256,
        ..CONDITIONS[0]
    };
    let path = std::env::temp_dir().join(format!("cdt-finite-volume-{}", std::process::id()));
    acquire::acquire(c, &path)?;
    let oracle = Oracle::new(c)?;
    let data = data::load(&path, c, &oracle)?;
    assert_eq!(data.blocks.len(), 8);
    assert!(data.blocks.iter().all(|chain| chain.len() == 1));
    assert_eq!(data.hashes.len(), 9);
    for (name, digest) in &data.hashes {
        let bytes = std::fs::read(path.join(name))?;
        assert_eq!(*digest, super::hex(&Sha256::digest(bytes)), "{name}");
    }
    assert_eq!(data.diagnostics.len(), 5);
    let first = format!("chain-{}.jsonl.gz", c.first_seed);
    let second = format!("chain-{}.jsonl.gz", c.first_seed + 1);
    let file = path.join(&first);
    let original = std::fs::read(&file)?;
    std::fs::copy(path.join(&second), &file)?;
    assert!(
        matches!(data::load(&path, c, &oracle), Err(Error::TraceDigest {
        seed, path: bad_file, expected, actual
    }) if seed == c.first_seed && bad_file == file && expected == data.hashes[&first] && actual == data.hashes[&second])
    );

    let manifest_path = path.join("manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    for chain in manifest["chains"].as_array_mut().expect("chain list") {
        chain.as_object_mut().expect("chain entry").remove("sha256");
    }
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
    assert!(matches!(
        data::load(&path, c, &oracle),
        Err(Error::Invalid(_))
    ));

    // An archive without hashes must use the rows even when its old scalar results look valid.
    manifest["schema"] = format!("intrinsic-{}/native-v1", c.folder()).into();
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
    assert_ne!(data::load(&path, c, &oracle)?.diagnostics, data.diagnostics);
    std::fs::write(&file, &original)?;
    assert_eq!(data::load(&path, c, &oracle)?.diagnostics, data.diagnostics);

    let mut rows: Vec<Draw> =
        serde_json::from_reader(flate2::read::GzDecoder::new(original.as_slice()))?;
    let expected_edges = u64::from(rows[1].edges);
    rows[1].edges -= 1;
    let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    serde_json::to_writer(&mut writer, &rows)?;
    std::fs::write(&file, writer.finish()?)?;
    assert!(
        matches!(data::load(&path, c, &oracle), Err(Error::TraceRow {
        seed, path: bad_file, measurement: 2, source: RowError::Field { field: "edges", expected, actual }
    }) if seed == c.first_seed && bad_file == file && expected == expected_edges && actual == expected_edges - 1)
    );

    std::fs::write(&file, original)?;
    let trace = std::fs::OpenOptions::new().write(true).open(&file)?;
    trace.set_len(trace.metadata()?.len() / 2)?;
    drop(trace);
    assert!(data::load(&path, c, &oracle).is_err());
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[test]
fn bootstrap_uses_each_chains_blocks_and_target_denominator() {
    let blocks = vec![vec![vec![(0, 3)], vec![]], vec![vec![(1, 1)], vec![(1, 1)]]];
    let s = sample(&blocks, &[0.5, 0.5], None, 917, 199)
        .expect("second chain always visits the target");
    assert_eq!(s.counts, [3, 2]);
    assert_eq!(s.hits, [3, 2]);
    assert!((s.tv - 0.1).abs() < 1e-14);
    // A replicate that draws two empty first-chain blocks has probabilities (0,1).
    assert!((s.radius - 0.6).abs() < 1e-14);
    let absent = vec![vec![vec![]], vec![vec![(1, 1)]]];
    assert!(sample(&absent, &[0.5, 0.5], None, 917, 199).is_none());
    assert!(
        simultaneous(
            &[1.0],
            &[vec![1.0], vec![1.0]],
            MomentCoverage::FourTorusFamily95
        )
        .is_none()
    );
}

#[test]
fn family_band_uses_the_bonferroni_rank_of_within_torus_maxima() {
    assert_eq!(
        MomentCoverage::from(Cohort::Precision),
        MomentCoverage::FourTorusFamily95
    );
    for cohort in [Cohort::Development, Cohort::Confirmation] {
        assert_eq!(MomentCoverage::from(cohort), MomentCoverage::PerTorus95);
    }
    // Each column has the same scale. One adverse displacement occurs at either end.
    let draws: Vec<_> = (1..=1999)
        .map(|i| vec![f64::from(i), f64::from(2000 - i)])
        .collect();
    for (scope, width) in [
        (MomentCoverage::PerTorus95, 1950.0),
        (MomentCoverage::FourTorusFamily95, 1987.0),
    ] {
        // The sorted maxima are 1000, 1001, 1001, ..., 1999, 1999.
        // Ranks 1900 and 1975 give the stated widths after multiplication by the scale.
        let bounds = simultaneous(&[0.0, 0.0], &draws, scope).expect("finite intervals");
        for (lo, hi) in bounds {
            assert!((lo + width).abs() < 1e-10 && (hi - width).abs() < 1e-10);
        }
    }
}

#[test]
fn moment_reports_keep_point_tolerance_and_interval_coverage_separate() -> Result<(), Error> {
    let oracle = Oracle::new(CONDITIONS[0])?;
    // Every visit has one profile. Its empirical covariance is zero while the exact (2,3) covariance is nonzero.
    let blocks = vec![vec![vec![(0, 16)], vec![(0, 16)]]];
    let mut draw = sample(&blocks, &oracle.probabilities, None, 17, 3).expect("positive visits");
    let estimate = oracle
        .moments(
            &draw
                .counts
                .iter()
                .copied()
                .map(u64::from)
                .collect::<Vec<_>>(),
        )
        .expect("finite");
    for width in [1.0, 0.01] {
        draw.boot_moments = [-width, 0.0, width]
            .map(|delta| estimate.iter().map(|x| x + delta).collect())
            .into();
        let scores = moments(&oracle, &draw, MomentCoverage::FourTorusFamily95)
            .expect("positive bootstrap uncertainty");
        let pair = scores
            .iter()
            .find(|m| m.name == "cov[2,3]")
            .expect("labeled covariance");
        assert!(!pair.within_tolerance);
        assert_eq!(pair.interval_covers_exact, width > 0.3);
        let report = serde_json::to_value(pair)?;
        assert_eq!(report["passes"], false);
    }
    Ok(())
}

#[test]
fn calibration_resolves_the_bootstrap_grid_and_checks_two_holdout_panels() -> Result<(), Error> {
    let condition = Condition {
        draws: 512,
        ..CONDITIONS[0]
    };
    let calibration = Calibration::new(condition, &Oracle::new(condition)?)?;
    let report = serde_json::to_value(&calibration)?;
    let training: Vec<f64> = serde_json::from_value(report["training_p"].clone())?;
    // At p = 1/20, SE < 1/200 requires more than 1,900 independent training draws.
    assert!(training.len() > 1900);
    let at_cutoff = training.iter().filter(|&&p| p <= 0.05).count();
    let expected = PValue::empirical(
        u16::try_from(at_cutoff).expect("bounded calibration count"),
        u16::try_from(training.len()).expect("bounded training size"),
    );
    assert_eq!(calibration.p(PValue::ALPHA), expected);
    for panel in &calibration.panels {
        assert_eq!(panel.null.p_values.len(), 100);
        assert_eq!(panel.biased.p_values.len(), 100);
    }
    let [original, confirmation] = &calibration.panels;
    assert_ne!(original.null.p_values, confirmation.null.p_values);
    Ok(())
}

#[test]
fn small_p_values_and_inadequate_information_affect_the_decision() {
    let hundredths = |numerator: u16| PValue::empirical(numerator - 1, 99);
    assert_eq!(
        holm([4, 1, 3, 60].map(hundredths)),
        [9, 4, 9, 60].map(hundredths)
    );
    assert_eq!(
        PValue::empirical(127, 1999).round_up_to_grid(200),
        PValue::empirical(12, 199)
    );
    assert_eq!(
        PValue::empirical(129, 1999).round_up_to_grid(200),
        PValue::empirical(12, 199)
    );
    assert_eq!(PValue::empirical(24, 1999).adjusted(4), PValue::ALPHA);
    assert!(error_rate_acceptable(11) && !error_rate_acceptable(12));
    let mut diagnostic = Diagnostic {
        observable: "N2".into(),
        combined_rhat: Some(1.01),
        bulk_ess: Some(400.0),
    };
    assert!(diagnostic.sufficient());
    diagnostic.bulk_ess = None;
    assert!(!diagnostic.sufficient());
}

#[test]
fn holdouts_count_the_union_of_the_two_holm_decisions_once_per_family() {
    let large = PValue::empirical(199, 199);
    let small = PValue::empirical(0, 199);
    let accepted = GoodnessOfFit {
        raw: large,
        calibrated: large,
    };
    let trial = TransferTrial {
        p: accepted,
        radius_missed: false,
    };
    let mut tori = CONDITIONS.map(|_| BudgetTransfer {
        first_null_seed: 0,
        first_biased_seed: 200,
        replicates: [199, 1999],
        null: vec![[trial; 2]; 2 * HOLDOUT],
        biased: vec![[trial; 2]; 2 * HOLDOUT],
    });
    // Raw only, calibrated only, and both on different tori in the same family.
    tori[0].null[0][1].p.raw = small;
    tori[1].null[1][1].p.calibrated = small;
    tori[2].null[2][1].p.raw = small;
    tori[3].null[2][1].p.calibrated = small;
    let holdouts = tori.each_ref().map(|torus| {
        std::array::from_fn(|panel| {
            let range = panel * HOLDOUT..(panel + 1) * HOLDOUT;
            PanelScores {
                null: torus.null[range.clone()]
                    .iter()
                    .map(|row| row[1].p)
                    .collect(),
                biased: torus.biased[range].iter().map(|row| row[1].p).collect(),
            }
        })
    });
    assert_eq!(heldout_families_rejected(&holdouts), [3, 0]);
    for row in &mut tori[2].biased[HOLDOUT..HOLDOUT + 11] {
        row[1].radius_missed = true;
    }
    let panels = transfer_summary(&tori);
    assert!(panels.iter().flatten().all(BudgetSummary::passed));
    let counts = serde_json::json!(panels);
    assert_eq!(counts[0][0]["combined_holm_rejections"], 0);
    assert_eq!(counts[0][1]["raw_holm_rejections"], 2);
    assert_eq!(counts[0][1]["calibrated_holm_rejections"], 2);
    assert_eq!(counts[0][1]["combined_holm_rejections"], 3);
    tori[2].biased[HOLDOUT + 11][1].radius_missed = true;
    let panels = transfer_summary(&tori);
    assert!(panels[1][0].passed());
    assert!(!panels[1][1].passed());
    assert_eq!(
        serde_json::json!(panels)[1][1]["biased_radius_misses"],
        serde_json::json!([0, 0, 12, 0])
    );
}

#[test]
fn conditioned_information_requires_occupied_blocks_and_varying_slice_indicators()
-> Result<(), Error> {
    let c = Condition {
        draws: 256 * 512,
        ..CONDITIONS[0]
    };
    let oracle = Oracle::new(c)?;
    let generator = Generator::new(c, &oracle)?;
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(487);
    let mut blocks = generator.blocks(&mut rng, false);
    assert!(data::ConditionedInformation::new(&blocks, c, &oracle).sufficient());
    for block in &mut blocks[0][49..] {
        block.clear();
    }
    let sparse = data::ConditionedInformation::new(&blocks, c, &oracle);
    assert_eq!(
        serde_json::json!(sparse)["occupied_blocks_per_chain"][0],
        49
    );
    assert!(!sparse.sufficient());

    let flat = oracle
        .profiles
        .binary_search(&c.start(0))
        .expect("admissible flat");
    let blocks = vec![vec![vec![(flat, 256)]; 512]; 8];
    let constant = data::ConditionedInformation::new(&blocks, c, &oracle);
    assert!(!constant.sufficient());
    let json = serde_json::json!(constant);
    assert!(json["profile_indicators"][0]["bulk_ess"].is_null());
    Ok(())
}
