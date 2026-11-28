#![forbid(unsafe_code)]

//! Stratified proposal-clock block bootstrap shared by production and calibration.

use super::acquire::{CONDITIONS, Cohort};
use super::data::Blocks;
use super::oracle::Oracle;
use super::probability::PValue;
use rand::{RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};
use rayon::prelude::*;

pub const PRODUCTION_REPLICATES: u16 = 1999;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MomentCoverage {
    PerTorus95,
    FourTorusFamily95,
}

impl From<Cohort> for MomentCoverage {
    fn from(cohort: Cohort) -> Self {
        match cohort {
            Cohort::Development | Cohort::Confirmation => Self::PerTorus95,
            Cohort::Precision => Self::FourTorusFamily95,
        }
    }
}

impl MomentCoverage {
    fn tail_denominator(self) -> usize {
        match self {
            Self::PerTorus95 => 20,
            Self::FourTorusFamily95 => 20 * CONDITIONS.len(),
        }
    }
}

fn upper_quantile(mut values: Vec<f64>, tail_denominator: usize) -> f64 {
    // Nearest rank at 1 − 1/d is n − floor(n/d), counting from one.
    let index = values.len() - values.len() / tail_denominator - 1;
    *values.select_nth_unstable_by(index, f64::total_cmp).1
}

pub fn quantile95(values: Vec<f64>) -> f64 {
    upper_quantile(values, 20)
}

fn distance(counts: &[u32], total: f64, truth: &[f64]) -> f64 {
    0.5 * counts
        .iter()
        .zip(truth)
        .map(|(&n, &p)| (f64::from(n) / total - p).abs())
        .sum::<f64>()
}

fn deviance(counts: &[u32], total: f64, truth: &[f64]) -> f64 {
    2.0 * counts
        .iter()
        .zip(truth)
        .filter(|(n, _)| **n != 0)
        .map(|(&n, &p)| {
            let value = f64::from(n);
            value * (value / (total * p)).ln()
        })
        .sum::<f64>()
}

pub struct Sample {
    pub counts: Vec<u32>,
    pub hits: Vec<u32>,
    pub tv: f64,
    pub radius: f64,
    pub statistic: f64,
    pub p: PValue,
    pub boot_moments: Vec<Vec<f64>>,
}

struct Replicate {
    distance: f64,
    exceeds: bool,
    moments: Vec<f64>,
}

pub fn sample(
    blocks: &Blocks,
    truth: &[f64],
    oracle: Option<&Oracle>,
    seed: u64,
    replicas: u16,
) -> Option<Sample> {
    let mut counts = vec![0_u32; truth.len()];
    let mut hits = Vec::with_capacity(blocks.len());
    for chain in blocks {
        let mut visits = 0;
        for &(cell, n) in chain.iter().flatten() {
            counts[cell] += n;
            visits += n;
        }
        hits.push(visits);
    }
    if hits.contains(&0) {
        return None;
    }
    let total = f64::from(hits.iter().sum::<u32>());
    let empirical: Vec<_> = counts.iter().map(|&n| f64::from(n) / total).collect();
    let statistic = deviance(&counts, total, truth);
    let tv = distance(&counts, total, truth);
    // Seed each replicate from the declared stream before parallel scheduling.
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let seeds: Vec<u64> = (0..replicas).map(|_| rng.random()).collect();
    let draw = |&subseed: &u64| {
        let mut stream = Xoshiro256PlusPlus::seed_from_u64(subseed);
        let mut histogram = vec![0_u32; truth.len()];
        let mut multiplicities = vec![0_u32; blocks[0].len()];
        for chain in blocks {
            multiplicities.fill(0);
            for _ in 0..chain.len() {
                multiplicities[stream.random_range(0..chain.len())] += 1;
            }
            for (block, &k) in chain.iter().zip(&multiplicities) {
                if k > 0 {
                    for &(i, n) in block {
                        histogram[i] += k * n;
                    }
                }
            }
        }
        let denominator = f64::from(histogram.iter().sum::<u32>());
        if denominator == 0.0 {
            return None;
        }
        let moments = match oracle {
            Some(o) => o.moments(&histogram.iter().copied().map(u64::from).collect::<Vec<_>>())?,
            None => Vec::new(),
        };
        Some(Replicate {
            distance: distance(&histogram, denominator, &empirical),
            exceeds: deviance(&histogram, denominator, &empirical) >= statistic,
            moments,
        })
    };
    let draws: Vec<_> = if oracle.is_some() {
        seeds.par_iter().map(draw).collect::<Option<Vec<_>>>()?
    } else {
        seeds.iter().map(draw).collect::<Option<Vec<_>>>()?
    };
    let radius = quantile95(draws.iter().map(|r| r.distance).collect());
    let p = PValue::empirical(draws.iter().map(|r| u16::from(r.exceeds)).sum(), replicas);
    Some(Sample {
        counts,
        hits,
        tv,
        radius,
        statistic,
        p,
        boot_moments: draws.into_iter().map(|r| r.moments).collect(),
    })
}

pub fn simultaneous(
    estimate: &[f64],
    bootstrap: &[Vec<f64>],
    coverage: MomentCoverage,
) -> Option<Vec<(f64, f64)>> {
    let n = f64::from(u16::try_from(bootstrap.len()).ok()?);
    if n < 2.0 || estimate.is_empty() || bootstrap.iter().any(|r| r.len() != estimate.len()) {
        return None;
    }
    let se: Vec<_> = (0..estimate.len())
        .map(|i| {
            let mean = bootstrap.iter().map(|r| r[i]).sum::<f64>() / n;
            (bootstrap.iter().map(|r| (r[i] - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
        })
        .collect();
    if se.iter().any(|&x| x <= 0.0 || !x.is_finite()) {
        return None;
    }
    let critical = upper_quantile(
        bootstrap
            .iter()
            .map(|r| {
                r.iter()
                    .zip(estimate)
                    .zip(&se)
                    .map(|((&v, &e), &s)| (v - e).abs() / s)
                    .fold(0.0, f64::max)
            })
            .collect(),
        coverage.tail_denominator(),
    );
    Some(
        estimate
            .iter()
            .zip(se)
            .map(|(&x, s)| ((-critical).mul_add(s, x), critical.mul_add(s, x)))
            .collect(),
    )
}
