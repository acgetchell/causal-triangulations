#![forbid(unsafe_code)]

//! Independent synthetic controls for the conditional histogram and bootstrap.

use super::Error;
use super::acquire::Condition;
use super::bootstrap::{quantile95, sample};
use super::data::{BLOCK, Blocks};
use super::oracle::Oracle;
use super::probability::PValue;
use la_stack::{BigInt, BigRational};
use num_traits::Zero;
use rand::{RngExt, SeedableRng, rngs::Xoshiro256PlusPlus};
use rayon::prelude::*;
use serde::Serialize;

// At the 5% cutoff the Monte Carlo SE with 1,999 draws is below one 1/200 bootstrap grid step.
const TRAINING: u16 = 1999;
const ORIGINAL_TRAINING: usize = 99;
const HOLDOUT_TRIALS: u16 = 100;
pub const HOLDOUT: usize = HOLDOUT_TRIALS as usize;
const REPLICAS: u16 = 199;
const HOLD: u32 = 4;

fn binomial_upper(rejections: usize) -> BigRational {
    // Under Binomial(100, 1/20), mass[k] has denominator 20^100.
    // Consecutive integer numerators follow the binomial recurrence.
    let trials = u32::from(HOLDOUT_TRIALS);
    let mut mass = BigInt::from(19).pow(trials);
    let mut tail = BigInt::zero();
    for k in 0..=trials {
        if k as usize >= rejections {
            tail += &mass;
        }
        mass = mass * (trials - k) / ((k + 1) * 19);
    }
    BigRational::new(tail, BigInt::from(20).pow(trials))
}

pub fn error_rate_acceptable(count: usize) -> bool {
    binomial_upper(count) >= BigRational::new(1.into(), 100.into())
}

struct Control {
    raw_p: PValue,
    error: f64,
    radius: f64,
    tv: f64,
}

pub struct Generator<'a> {
    oracle: &'a Oracle,
    cumulative: Vec<u64>,
    flat: usize,
    condition: Condition,
}

impl<'a> Generator<'a> {
    pub fn new(c: Condition, o: &'a Oracle) -> Result<Self, Error> {
        let flat = o
            .profiles
            .binary_search(&c.start(0))
            .map_err(|_| Error::Invalid("flat control outside support".into()))?;
        Ok(Self {
            oracle: o,
            cumulative: o
                .weights
                .iter()
                .scan(0, |sum, w| {
                    *sum += w;
                    Some(*sum)
                })
                .collect(),
            flat,
            condition: c,
        })
    }
    pub fn blocks(&self, rng: &mut Xoshiro256PlusPlus, biased: bool) -> Blocks {
        let mut blocks = Vec::with_capacity(8);
        for _ in 0..8 {
            let mut chain = Vec::new();
            for _ in 0..self.condition.draws as usize / BLOCK {
                let mut hits = Vec::new();
                for _ in 0..BLOCK / HOLD as usize {
                    if rng.random_ratio(1, 4) {
                        let index = if biased && rng.random_ratio(3, 20) {
                            self.flat
                        } else {
                            let ticket = rng.random_range(0..self.oracle.normalizer);
                            self.cumulative.partition_point(|&w| w <= ticket)
                        };
                        hits.push(index);
                    }
                }
                hits.sort_unstable();
                chain.push(
                    hits.chunk_by(|a, b| a == b)
                        .map(|cell| (cell[0], cell.iter().map(|_| HOLD).sum()))
                        .collect(),
                );
            }
            blocks.push(chain);
        }
        blocks
    }
    fn experiment(&self, seed: u64, biased: bool) -> Result<Control, Error> {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let blocks = self.blocks(&mut rng, biased);
        let s = sample(
            &blocks,
            &self.oracle.probabilities,
            None,
            rng.random(),
            REPLICAS,
        )
        .ok_or_else(|| Error::Invalid("synthetic control had insufficient information".into()))?;
        if s.radius <= 0.0 {
            return Err(Error::Invalid(
                "synthetic control has zero bootstrap radius".into(),
            ));
        }
        let hits = f64::from(s.hits.iter().sum::<u32>());
        let error = if biased {
            0.5 * s
                .counts
                .iter()
                .zip(&self.oracle.probabilities)
                .enumerate()
                .map(|(i, (&n, &p))| {
                    let probability = p.mul_add(0.85, if i == self.flat { 0.15 } else { 0.0 });
                    (f64::from(n) / hits - probability).abs()
                })
                .sum::<f64>()
        } else {
            s.tv
        };
        Ok(Control {
            raw_p: s.p,
            error,
            radius: s.radius,
            tv: s.tv,
        })
    }
}

#[derive(Serialize)]
pub struct Holdout {
    pub p_values: Vec<PValue>,
    pub rejected: usize,
    radius_misses: usize,
    tv_bound_misses: usize,
    true_tv: f64,
    passed: bool,
}

#[derive(Serialize)]
pub struct Calibration {
    seed: u64,
    pub multiplier: f64,
    training_p: Vec<PValue>,
    pub panels: [Panel; 2],
}

#[derive(Serialize)]
pub struct Panel {
    pub null: Holdout,
    pub biased: Holdout,
}

fn rank(raw: PValue, training: &[PValue]) -> PValue {
    let rounded = raw.round_up_to_grid(REPLICAS + 1);
    let at_or_below = training.iter().map(|&p| u16::from(p <= rounded)).sum();
    PValue::empirical(at_or_below, TRAINING)
}

impl Calibration {
    pub fn p(&self, raw: PValue) -> PValue {
        rank(raw, &self.training_p)
    }
    pub fn passed(&self) -> bool {
        self.panels.iter().all(|p| p.null.passed && p.biased.passed)
    }
    pub fn new(c: Condition, o: &Oracle) -> Result<Self, Error> {
        let seed = c.reference.calibration_seed;
        let mut stream = Xoshiro256PlusPlus::seed_from_u64(seed);
        let mut seeds = std::iter::from_fn(|| Some(stream.random::<u64>()));
        // Reserve the original holdouts before extending training. The confirmation panel is unseen.
        let mut training_seeds: Vec<_> = seeds.by_ref().take(ORIGINAL_TRAINING).collect();
        let original: Vec<_> = seeds.by_ref().take(2 * HOLDOUT).collect();
        training_seeds.extend(
            seeds
                .by_ref()
                .take(usize::from(TRAINING) - ORIGINAL_TRAINING),
        );
        let confirmation: Vec<_> = seeds.take(2 * HOLDOUT).collect();
        let generator = Generator::new(c, o)?;
        let experiments = |seeds: &[u64], biased| {
            seeds
                .par_iter()
                .map(|&s| generator.experiment(s, biased))
                .collect::<Result<Vec<_>, _>>()
        };
        let training = experiments(&training_seeds, false)?;
        let training_p = training.iter().map(|v| v.raw_p).collect::<Vec<_>>();
        let multiplier = quantile95(training.iter().map(|v| v.error / v.radius).collect()).max(1.0);
        let holdout = |controls: &[Control], true_tv: f64| {
            let p_values: Vec<_> = controls
                .iter()
                .map(|v| rank(v.raw_p, &training_p))
                .collect();
            let rejected = p_values.iter().filter(|&&p| p <= PValue::ALPHA).count();
            let radius_misses = controls
                .iter()
                .filter(|v| v.error > multiplier * v.radius)
                .count();
            let tv_bound_misses = controls
                .iter()
                .filter(|v| true_tv > multiplier.mul_add(v.radius, v.tv))
                .count();
            let passed = error_rate_acceptable(radius_misses)
                && if true_tv == 0.0 {
                    error_rate_acceptable(rejected)
                } else {
                    rejected >= 90
                };
            Holdout {
                p_values,
                rejected,
                radius_misses,
                tv_bound_misses,
                true_tv,
                passed,
            }
        };
        let panel = |seeds: &[u64]| -> Result<Panel, Error> {
            let (null, biased) = seeds.split_at(HOLDOUT);
            Ok(Panel {
                null: holdout(&experiments(null, false)?, 0.0),
                biased: holdout(
                    &experiments(biased, true)?,
                    0.15 * (1.0 - o.probabilities[generator.flat]),
                ),
            })
        };
        let panels = [panel(&original)?, panel(&confirmation)?];
        Ok(Self {
            seed,
            multiplier,
            training_p,
            panels,
        })
    }
}

#[test]
fn synthetic_blocks_preserve_the_production_clock() -> Result<(), Error> {
    let c = Condition {
        draws: 2048,
        ..super::acquire::CONDITIONS[0]
    };
    let oracle = Oracle::new(c)?;
    let generator = Generator::new(c, &oracle)?;
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(41);
    for biased in [false, true] {
        let blocks = generator.blocks(&mut rng, biased);
        assert_eq!(blocks.len(), 8);
        for chain in blocks {
            assert_eq!(chain.len(), 8);
            for block in chain {
                let n: u32 = block.iter().map(|&(_, count)| count).sum();
                assert!(n <= 256 && block.iter().all(|&(_, count)| count.is_multiple_of(HOLD)));
            }
        }
    }
    Ok(())
}
