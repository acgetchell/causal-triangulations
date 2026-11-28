#![forbid(unsafe_code)]

//! Check the cadence and geometry before conditioning, retaining whole-block counts.

use super::acquire::{BURN_IN, CADENCE, Condition, Draw, Profile, STRENGTH};
use super::evidence::{Error, HashingReader, hex};
use super::oracle::Oracle;
use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const BLOCK: usize = 256;
pub type Blocks = Vec<Vec<Vec<(usize, u32)>>>;

#[derive(Deserialize, Serialize)]
pub struct Diagnostic {
    pub observable: String,
    pub combined_rhat: Option<f64>,
    pub bulk_ess: Option<f64>,
}

impl Diagnostic {
    pub fn sufficient(&self) -> bool {
        self.combined_rhat.is_some_and(|x| x > 0.0 && x <= 1.01)
            && self.bulk_ess.is_some_and(|x| x.is_finite() && x >= 400.0)
    }
}

#[derive(Deserialize)]
struct ChainFile {
    seed: u64,
    initial: Profile,
    file: String,
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

pub struct Data {
    pub blocks: Blocks,
    pub diagnostics: Vec<Diagnostic>,
    pub hashes: BTreeMap<String, String>,
}

pub fn count_row(
    row: &Draw,
    c: Condition,
    index: usize,
    profiles: &HashMap<&[u32], usize>,
) -> Result<Option<usize>, Error> {
    let vertices: u64 = row.profile.iter().copied().map(u64::from).sum();
    if row.profile.len() != c.slices as usize
        || row.profile.iter().any(|&l| l < 3)
        || row.step as usize != BURN_IN as usize + (index + 1) * CADENCE as usize
        || vertices != u64::from(row.vertices)
        || 2 * vertices != u64::from(row.triangles)
        || 3 * vertices != u64::from(row.edges)
    {
        return Err(Error::Invalid(format!(
            "invalid geometry or cadence at measurement {}",
            index + 1
        )));
    }
    if row.triangles != c.triangles {
        return Ok(None);
    }
    profiles
        .get(row.profile.as_slice())
        .copied()
        .map(Some)
        .ok_or_else(|| Error::Invalid("target profile outside exact support".into()))
}

pub fn load(dir: &Path, c: Condition, oracle: &Oracle) -> Result<Data, Error> {
    let manifest = dir.join("manifest.json");
    let bytes = std::fs::read(&manifest)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    let fixing =
        serde_json::json!({"mode":"quadratic-triangles","strength":STRENGTH,"target":c.triangles});
    let lambda = m.action["cosmological_constant"].as_f64();
    let expected_names = std::iter::once("N2".into()).chain((0..c.slices).map(|i| format!("l{i}")));
    if m.schema != format!("intrinsic-t{}-n{}/native-v1", c.slices, c.triangles)
        || m.rng != "Xoshiro256PlusPlus"
        || m.family_policy != "uniform"
        || m.burn_in != BURN_IN
        || m.cadence != CADENCE
        || m.draws_per_chain != c.draws
        || m.chains.len() != 8
        || m.temperature.to_bits() != 1.0_f64.to_bits()
        || m.action["coupling_0"].as_f64() != Some(0.0)
        || m.action["coupling_2"].as_f64() != Some(0.0)
        || m.action["volume_fixing"] != fixing
        || !lambda.is_some_and(|l| (l - 2.0 * std::f64::consts::LN_2 / 3.0).abs() < 1e-15)
        || m.reachable_profiles != oracle.profiles
        || !m
            .diagnostics
            .iter()
            .map(|d| d.observable.as_str())
            .eq(expected_names)
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
    let mut hashes = BTreeMap::from([("manifest.json".into(), hex(&Sha256::digest(&bytes)))]);
    for (slot, chain) in m.chains.iter().enumerate() {
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
        let mut compressed = HashingReader::new(File::open(&path)?);
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
                if let Some(i) = count_row(&row, c, index, &indexes)? {
                    *cells.entry(i).or_default() += 1;
                }
                index += 1;
            }
            chain_blocks.push(cells.into_iter().collect());
            line.clear();
        }
        if index != c.draws as usize {
            return Err(Error::Invalid(format!("incomplete trace for seed {seed}")));
        }
        hashes.insert(chain.file.clone(), compressed.digest());
        blocks.push(chain_blocks);
    }
    Ok(Data {
        blocks,
        diagnostics: m.diagnostics,
        hashes,
    })
}
