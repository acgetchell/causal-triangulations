#![forbid(unsafe_code)]

//! Independent, integer fixed-volume transfer matrix with the marked entrance convention.

use super::Error;
use super::acquire::{Condition, Profile};
use la_stack::{BigInt, BigRational};
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

pub struct Oracle {
    pub profiles: Vec<Profile>,
    pub weights: Vec<u64>,
    pub normalizer: u64,
    pub probabilities: Vec<f64>,
}

fn enumerate(surplus: u32, remaining: usize, profile: &mut Profile, out: &mut Vec<Profile>) {
    if remaining == 1 {
        profile.push(3 + surplus);
        out.push(profile.clone());
        profile.pop();
        return;
    }
    for extra in 0..=surplus {
        profile.push(3 + extra);
        enumerate(surplus - extra, remaining - 1, profile, out);
        profile.pop();
    }
}

fn binomial(n: u32, k: u32) -> u64 {
    (1..=k.min(n - k)).fold(1, |w, j| w * u64::from(n + 1 - j) / u64::from(j))
}

impl Oracle {
    pub fn new(condition: Condition) -> Result<Self, Error> {
        let max = condition.triangles / 2 - 3 * (condition.slices - 1);
        let size = max as usize + 1;
        // Count words after the first (marked) Up triangle by appending Up or Down.
        let mut strip = vec![vec![1_u64; size]; size];
        for a in 2..size {
            for b in 1..size {
                strip[a][b] = strip[a - 1][b] + strip[a][b - 1];
            }
        }
        let mut profiles = Vec::new();
        enumerate(
            condition.triangles / 2 - 3 * condition.slices,
            condition.slices as usize,
            &mut Vec::new(),
            &mut profiles,
        );
        let mut weights = Vec::with_capacity(profiles.len());
        for p in &profiles {
            let mut weight = 1;
            let mut projected = 1;
            for (&a, &b) in p.iter().zip(p.iter().cycle().skip(1)) {
                weight *= binomial(a + b - 1, a - 1);
                projected *= strip[a as usize][b as usize];
            }
            if weight != projected {
                return Err(Error::Invalid("projected strip weights disagree".into()));
            }
            weights.push(weight);
        }
        // Coefficient of g^N in Tr(M(g)^T), with no profile enumeration.
        let mut trace = 0_u128;
        for entrance in 3..=max {
            let mut states = BTreeMap::from([((entrance, 0), 1_u128)]);
            for _ in 0..condition.slices {
                let mut next = BTreeMap::new();
                for ((a, degree), weight) in states {
                    for b in 3..=max {
                        if degree + a + b <= condition.triangles {
                            *next.entry((b, degree + a + b)).or_default() +=
                                weight * u128::from(strip[a as usize][b as usize]);
                        }
                    }
                }
                states = next;
            }
            trace += states
                .get(&(entrance, condition.triangles))
                .copied()
                .unwrap_or_default();
        }
        let normalizer = weights.iter().sum::<u64>();
        if profiles.len() != condition.profiles || u128::from(normalizer) != trace {
            return Err(Error::Invalid(
                "profile support or transfer trace disagrees with enumeration".into(),
            ));
        }
        let probabilities = weights
            .iter()
            .map(|&w| BigRational::new(w.into(), normalizer.into()).to_f64())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error::Invalid("unrepresentable profile probability".into()))?;
        Ok(Self {
            profiles,
            weights,
            normalizer,
            probabilities,
        })
    }

    pub fn exact_moments(&self, counts: &[u64]) -> Option<Vec<BigRational>> {
        let total: i128 = counts.iter().copied().map(i128::from).sum();
        if counts.len() != self.profiles.len() || total == 0 {
            return None;
        }
        let n = BigInt::from(total);
        let t = self.profiles[0].len();
        let mut first = vec![0_i128; t];
        let mut second = vec![0_i128; t * (t + 1) / 2];
        for (profile, &count) in self.profiles.iter().zip(counts) {
            let mut pos = 0;
            for (i, &a) in profile.iter().enumerate() {
                let weighted = i128::from(count) * i128::from(a);
                first[i] += weighted;
                for &b in &profile[i..] {
                    second[pos] += weighted * i128::from(b);
                    pos += 1;
                }
            }
        }
        let mut out: Vec<_> = first
            .iter()
            .map(|&x| BigRational::new(x.into(), n.clone()))
            .collect();
        let denominator = &n * &n;
        let mut pos = 0;
        for i in 0..t {
            for j in i..t {
                let centered = BigInt::from(second[pos]) * &n - BigInt::from(first[i]) * first[j];
                out.push(BigRational::new(centered, denominator.clone()));
                pos += 1;
            }
        }
        Some(out)
    }

    pub fn moments(&self, counts: &[u64]) -> Option<Vec<f64>> {
        self.exact_moments(counts)?
            .iter()
            .map(ToPrimitive::to_f64)
            .collect()
    }
}
