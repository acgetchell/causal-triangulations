#![forbid(unsafe_code)]

//! Check the cadence and geometry before conditioning, retaining whole-block counts.

use super::acquire::{BURN_IN, CADENCE, Condition, Draw, Profile, STRENGTH};
use super::evidence::{Error, HashingIo, hex};
use super::oracle::Oracle;
use flate2::read::MultiGzDecoder;
use markov_chain_monte_carlo::{CombinedRhat, EssEstimate, EssEstimator};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const BLOCK: usize = 256;
pub type Blocks = Vec<Vec<Vec<(usize, u32)>>>;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct Diagnostic {
    pub observable: String,
    pub combined_rhat: Option<f64>,
    pub bulk_ess: Option<f64>,
}

/// Use the same upstream full-cadence estimators at collection and for unbound archived traces.
pub fn full_cadence_diagnostics(
    slices: u32,
    mut series: impl FnMut(usize) -> Vec<Vec<f64>>,
) -> Vec<Diagnostic> {
    std::iter::once("N2".to_owned())
        .chain((0..slices).map(|i| format!("l{i}")))
        .enumerate()
        .map(|(i, observable)| {
            let values = series(i);
            let chains: Vec<_> = values.iter().map(Vec::as_slice).collect();
            Diagnostic {
                observable,
                combined_rhat: CombinedRhat::estimate(&chains)
                    .ok()
                    .and_then(CombinedRhat::value),
                bulk_ess: EssEstimate::estimate(&chains, EssEstimator::Bulk)
                    .ok()
                    .map(EssEstimate::value),
            }
        })
        .collect()
}

impl Diagnostic {
    pub fn sufficient(&self) -> bool {
        self.combined_rhat.is_some_and(|x| x > 0.0 && x <= 1.01)
            && self.bulk_ess.is_some_and(|x| x.is_finite() && x >= 400.0)
    }
}

#[derive(Serialize)]
pub struct ConditionedInformation {
    occupied_blocks_per_chain: Vec<usize>,
    profile_indicators: Vec<Diagnostic>,
}

impl ConditionedInformation {
    pub fn sufficient(&self) -> bool {
        self.occupied_blocks_per_chain.len() == 8
            && self.occupied_blocks_per_chain.iter().all(|&n| n >= 50)
            && !self.profile_indicators.is_empty()
            && self.profile_indicators.iter().all(Diagnostic::sufficient)
    }

    pub fn new(blocks: &Blocks, c: Condition, oracle: &Oracle) -> Self {
        let hits: Vec<Vec<u32>> = blocks
            .iter()
            .map(|chain| {
                chain
                    .iter()
                    .map(|block| block.iter().map(|&(_, n)| n).sum())
                    .collect()
            })
            .collect();
        let occupied_blocks_per_chain = hits
            .iter()
            .map(|chain| chain.iter().filter(|&&n| n > 0).count())
            .collect();
        let total = hits.iter().flatten().sum::<u32>();
        let profile_indicators = (0..c.slices as usize)
            .map(|slice| {
                let above_mean: Vec<_> = oracle
                    .profiles
                    .iter()
                    .map(|p| 2 * c.slices * p[slice] > c.triangles)
                    .collect();
                let above: Vec<Vec<u32>> = blocks
                    .iter()
                    .map(|chain| {
                        chain
                            .iter()
                            .map(|block| {
                                block
                                    .iter()
                                    .filter(|&&(i, _)| above_mean[i])
                                    .map(|&(_, n)| n)
                                    .sum()
                            })
                            .collect()
                    })
                    .collect();
                let favorable = above.iter().flatten().sum::<u32>();
                let mut diagnostic = Diagnostic {
                    observable: format!("conditional P(l{slice} > N2/(2T)) block score"),
                    combined_rhat: None,
                    bulk_ess: None,
                };
                if favorable > 0 && favorable < total {
                    let probability = f64::from(favorable) / f64::from(total);
                    let scores: Vec<Vec<_>> = above
                        .iter()
                        .zip(&hits)
                        .map(|(above, hits)| {
                            above
                                .iter()
                                .zip(hits)
                                .map(|(&n, &k)| {
                                    let expected_above = probability * f64::from(k);
                                    f64::from(n) - expected_above
                                })
                                .collect()
                        })
                        .collect();
                    let chains: Vec<_> = scores.iter().map(Vec::as_slice).collect();
                    diagnostic.combined_rhat = CombinedRhat::estimate(&chains)
                        .ok()
                        .and_then(CombinedRhat::value);
                    diagnostic.bulk_ess = EssEstimate::estimate(&chains, EssEstimator::Bulk)
                        .ok()
                        .map(EssEstimate::value);
                }
                diagnostic
            })
            .collect();
        Self {
            occupied_blocks_per_chain,
            profile_indicators,
        }
    }
}

#[derive(Deserialize)]
struct ChainFile {
    seed: u64,
    initial: Profile,
    file: String,
    sha256: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    schema: String,
    rng: String,
    family_policy: String,
    temperature: f64,
    action: serde_json::Value,
    burn_in: u32,
    cadence: u32,
    draws_per_chain: u32,
    chains: Vec<ChainFile>,
    reachable_profiles: Vec<Profile>,
    diagnostics: Vec<Diagnostic>,
}

impl Manifest {
    fn matches_protocol(&self, c: Condition, oracle: &Oracle) -> bool {
        let fixing = serde_json::json!({"mode":"quadratic-triangles","strength":STRENGTH,"target":c.triangles});
        let expected_names =
            std::iter::once("N2".into()).chain((0..c.slices).map(|i| format!("l{i}")));
        self.rng == "Xoshiro256PlusPlus"
            && self.family_policy == "uniform"
            && self.burn_in == BURN_IN
            && self.cadence == CADENCE
            && self.draws_per_chain == c.draws
            && self.chains.len() == 8
            && self.temperature.to_bits() == 1.0_f64.to_bits()
            && self.action["coupling_0"].as_f64() == Some(0.0)
            && self.action["coupling_2"].as_f64() == Some(0.0)
            && self.action["volume_fixing"] == fixing
            && self.action["cosmological_constant"]
                .as_f64()
                .is_some_and(|l| (l - 2.0 * std::f64::consts::LN_2 / 3.0).abs() < 1e-15)
            && self.reachable_profiles == oracle.profiles
            && self
                .diagnostics
                .iter()
                .map(|d| d.observable.as_str())
                .eq(expected_names)
    }
}

pub struct Data {
    pub blocks: Blocks,
    pub diagnostics: Vec<Diagnostic>,
    pub hashes: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RowError {
    #[error("{field}: expected {expected}, read {actual}")]
    Field {
        field: &'static str,
        expected: u64,
        actual: u64,
    },
    #[error("slice {slice}: expected at least {minimum} links, read {actual}")]
    ShortLoop {
        slice: usize,
        minimum: u32,
        actual: u32,
    },
    #[error("target profile {0:?} outside exact support")]
    OutsideSupport(Profile),
}

impl RowError {
    fn check(field: &'static str, expected: u64, actual: u64) -> Result<(), Self> {
        if actual == expected {
            Ok(())
        } else {
            Err(Self::Field {
                field,
                expected,
                actual,
            })
        }
    }
}

pub fn count_row(
    row: &Draw,
    c: Condition,
    index: usize,
    profiles: &HashMap<&[u32], usize>,
) -> Result<Option<usize>, RowError> {
    RowError::check(
        "profile length",
        u64::from(c.slices),
        row.profile.len() as u64,
    )?;
    if let Some((slice, &actual)) = row.profile.iter().enumerate().find(|&(_, &n)| n < 3) {
        return Err(RowError::ShortLoop {
            slice,
            minimum: 3,
            actual,
        });
    }
    let step = u64::from(BURN_IN) + (index as u64 + 1) * u64::from(CADENCE);
    RowError::check("step", step, u64::from(row.step))?;
    let vertices: u64 = row.profile.iter().copied().map(u64::from).sum();
    RowError::check("vertices", vertices, u64::from(row.vertices))?;
    RowError::check("triangles", 2 * vertices, u64::from(row.triangles))?;
    RowError::check("edges", 3 * vertices, u64::from(row.edges))?;
    if row.triangles != c.triangles {
        return Ok(None);
    }
    profiles
        .get(row.profile.as_slice())
        .copied()
        .map(Some)
        .ok_or_else(|| RowError::OutsideSupport(row.profile.clone()))
}

pub fn load(dir: &Path, c: Condition, oracle: &Oracle) -> Result<Data, Error> {
    let manifest = dir.join("manifest.json");
    let bytes = std::fs::read(&manifest)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    let archived = m.schema == format!("intrinsic-{}/native-v1", c.folder());
    let bound = m.schema == format!("intrinsic-{}/native-v2", c.folder());
    if !(archived || (bound && m.chains.iter().all(|chain| chain.sha256.is_some())))
        || !m.matches_protocol(c, oracle)
    {
        return Err(Error::Invalid(format!(
            "wrong acquisition protocol in {}",
            manifest.display()
        )));
    }
    let indexes: HashMap<_, _> = oracle
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_slice(), i))
        .collect();
    let mut blocks = Vec::with_capacity(8);
    // Legacy manifests lack trace hashes. Retain just the integer scalar series in chain order.
    let mut columns = archived.then(|| vec![Vec::<u32>::new(); 1 + c.slices as usize]);
    let mut hashes = BTreeMap::from([("manifest.json".into(), hex(&Sha256::digest(&bytes)))]);
    for (slot, chain) in m.chains.into_iter().enumerate() {
        let seed = c.first_seed + slot as u64;
        if chain.seed != seed
            || chain.initial != c.start(slot)
            || chain.file != format!("chain-{seed}.jsonl.gz")
        {
            return Err(Error::Invalid(format!(
                "unexpected chain or start for seed {seed}"
            )));
        }
        let path = dir.join(&chain.file);
        let mut compressed = HashingIo::new(File::open(&path)?);
        let mut reader = BufReader::new(MultiGzDecoder::new(&mut compressed));
        let mut line = Vec::new();
        let mut chain_blocks = Vec::with_capacity(c.draws as usize / BLOCK);
        let mut index = 0;
        while reader.read_until(b'\n', &mut line)? != 0 {
            let rows: Vec<Draw> = serde_json::from_slice(&line)?;
            if rows.len() != BLOCK || index >= c.draws as usize {
                return Err(Error::Invalid(format!("invalid block for seed {seed}")));
            }
            let mut cells = BTreeMap::new();
            for row in rows {
                if let Some(i) =
                    count_row(&row, c, index, &indexes).map_err(|source| Error::TraceRow {
                        seed,
                        path: path.clone(),
                        measurement: index + 1,
                        source,
                    })?
                {
                    *cells.entry(i).or_default() += 1;
                }
                if let Some(columns) = &mut columns {
                    for (column, &value) in columns
                        .iter_mut()
                        .zip(std::iter::once(&row.triangles).chain(&row.profile))
                    {
                        column.push(value);
                    }
                }
                index += 1;
            }
            chain_blocks.push(cells.into_iter().collect());
            line.clear();
        }
        if index != c.draws as usize {
            return Err(Error::Invalid(format!("incomplete trace for seed {seed}")));
        }
        let actual = compressed.digest();
        if let Some(expected) = chain.sha256
            && actual != expected
        {
            return Err(Error::TraceDigest {
                seed,
                path,
                expected,
                actual,
            });
        }
        hashes.insert(chain.file, actual);
        blocks.push(chain_blocks);
    }
    let diagnostics = match columns {
        Some(columns) => full_cadence_diagnostics(c.slices, |i| {
            columns[i]
                .chunks_exact(c.draws as usize)
                .map(|chain| chain.iter().copied().map(f64::from).collect())
                .collect()
        }),
        None => m.diagnostics,
    };
    Ok(Data {
        blocks,
        diagnostics,
        hashes,
    })
}
