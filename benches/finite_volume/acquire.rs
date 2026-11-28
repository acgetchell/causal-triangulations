#![forbid(unsafe_code)]

//! Acquire intrinsic chains at the four declared fixed-volume comparison conditions.

use super::data::{Diagnostic, full_cadence_diagnostics};
use super::evidence::HashingIo;
use causal_triangulations::prelude::action::{VolumeFixing, VolumeFixingError};
use causal_triangulations::prelude::errors::{IntrinsicCdtError, McmcError, SimplexCountField};
use causal_triangulations::prelude::simulation::{
    ActionConfig, CdtError, CdtSimplexCounts, CdtTarget, Chain, DelayedProposal,
    IntrinsicCdtProposal, IntrinsicCdtTriangulation,
};
use flate2::{Compression, write::GzEncoder};
use markov_chain_monte_carlo::DelayedStepError;
use rand::{SeedableRng, rngs::Xoshiro256PlusPlus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::TryFromIntError;
use std::path::Path;
use thiserror::Error;

pub type Profile = Vec<u32>;
pub const BURN_IN: u32 = 16_384;
pub const CADENCE: u32 = 16;
pub const STRENGTH: f64 = 0.05;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Condition {
    pub slices: u32,
    pub triangles: u32,
    pub profiles: usize,
    pub first_seed: u64,
    pub draws: u32,
    // The original acquisition schema records the production seeds. New manifests also record the fixed analysis design.
    #[serde(skip)]
    pub reference: Reference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Reference {
    pub reachability_seed: u64,
    pub bootstrap_seed: u64,
    pub calibration_seed: u64,
    pub calibration_sha256: &'static str,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Cohort {
    Development,
    Confirmation,
    Precision,
}

impl Cohort {
    pub fn conditions(self) -> [Condition; 4] {
        let offset = match self {
            Self::Development => 0,
            Self::Confirmation => 1_000_000_000,
            Self::Precision => 2_000_000_000,
        };
        CONDITIONS.map(|mut c| {
            c.first_seed += offset;
            if self == Self::Precision && c.triangles == 64 {
                c.draws *= 4;
                c.reference.calibration_sha256 =
                    "881eeb6b353dd12beea96f0866b1d1e1602e8651f81e0892abdab5d10018b06e";
            }
            c
        })
    }
}

const N32: Condition = Condition {
    slices: 4,
    triangles: 32,
    profiles: 35,
    first_seed: 320_101,
    draws: 262_144,
    reference: Reference {
        reachability_seed: 320_100,
        bootstrap_seed: 320_201,
        calibration_seed: 420_201,
        calibration_sha256: "e42bbbb8cbdbb45cd8e7781b0b6b1308efc641db6f4328df7831a55b4147ee7b",
    },
};
const N40: Condition = Condition {
    slices: 4,
    triangles: 40,
    profiles: 165,
    first_seed: 400_101,
    draws: 131_072,
    reference: Reference {
        reachability_seed: 400_100,
        bootstrap_seed: 400_201,
        calibration_seed: 500_201,
        calibration_sha256: "455f0448f4be6bfa401fb2165c077df7b7e718357b5ce516ed5432d3b37d1b60",
    },
};
const N48: Condition = Condition {
    slices: 4,
    triangles: 48,
    profiles: 455,
    first_seed: 480_101,
    draws: 262_144,
    reference: Reference {
        reachability_seed: 480_100,
        bootstrap_seed: 480_201,
        calibration_seed: 580_201,
        calibration_sha256: "38be7af16f2c57871b5476421e31f559d33aca324570c2ea719eb3fe184e5b43",
    },
};
const N64: Condition = Condition {
    slices: 8,
    triangles: 64,
    profiles: 6_435,
    first_seed: 640_101,
    draws: 2_097_152,
    reference: Reference {
        reachability_seed: 640_100,
        bootstrap_seed: 640_201,
        calibration_seed: 740_201,
        calibration_sha256: "3a9d877b0285f480252e284a417e82247bfb25310caa2c7cdb542594b1252e62",
    },
};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Geometry(#[from] IntrinsicCdtError),
    #[error(transparent)]
    Configuration(#[from] CdtError),
    #[error(transparent)]
    Fixing(#[from] VolumeFixingError),
    #[error(transparent)]
    Initialization(#[from] McmcError),
    #[error(transparent)]
    Step(#[from] DelayedStepError<IntrinsicCdtError>),
    #[error("step {step}: expected {expected} slices, observed {actual}")]
    TimeExtent {
        step: u32,
        expected: usize,
        actual: usize,
    },
    #[error("step {step}, slice {slice}: expected at least 3 links, observed {actual}")]
    ShortLoop {
        step: u32,
        slice: usize,
        actual: usize,
    },
    #[error("step {step}, slice {slice}: length {actual} exceeds the u32 measurement range")]
    SliceLengthOutOfRange {
        step: u32,
        slice: usize,
        actual: usize,
        #[source]
        source: TryFromIntError,
    },
    #[error("step {step}: {field} count {actual} exceeds the u32 measurement range")]
    CountOutOfRange {
        step: u32,
        field: SimplexCountField,
        actual: usize,
        #[source]
        source: TryFromIntError,
    },
    #[error("step {step}: expected {expected} {field}, observed {actual}")]
    CountMismatch {
        step: u32,
        field: SimplexCountField,
        expected: u64,
        actual: u32,
    },
    #[error("failed to witness positive forward and reverse support for every target profile")]
    Reachability,
    #[error("an acquisition worker panicked")]
    Worker,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Deserialize, Serialize)]
pub struct Draw {
    pub edges: u32,
    pub profile: Profile,
    pub step: u32,
    pub triangles: u32,
    pub vertices: u32,
}

#[derive(Serialize)]
struct Run {
    draws: Vec<Draw>,
    initial: Profile,
    seed: u64,
}

/// Serialize the cadence traces directly, retaining the existing sorted JSON field order.
#[derive(Serialize)]
struct Acquisition<'a> {
    action: serde_json::Value,
    burn_in: u32,
    cadence: u32,
    chains: &'a [Run],
    crate_version: &'static str,
    diagnostics: Vec<Diagnostic>,
    draws_per_chain: u32,
    family_policy: &'static str,
    reachable_profiles: BTreeSet<Profile>,
    rng: &'static str,
    schema: String,
    temperature: f64,
}

/// Reads the backend's face count independently of the labeled loop lengths.
fn measure(state: &IntrinsicCdtTriangulation, step: u32, slices: usize) -> Result<Draw, Error> {
    Draw::from_counts(
        step,
        slices,
        state.spatial_loop_lengths(),
        state.simplex_counts()?,
    )
}

impl Draw {
    pub(super) fn from_counts(
        step: u32,
        slices: usize,
        lengths: impl ExactSizeIterator<Item = usize>,
        counts: CdtSimplexCounts,
    ) -> Result<Self, Error> {
        if lengths.len() != slices {
            return Err(Error::TimeExtent {
                step,
                expected: slices,
                actual: lengths.len(),
            });
        }
        let profile = lengths
            .enumerate()
            .map(|(slice, actual)| {
                if actual < 3 {
                    return Err(Error::ShortLoop {
                        step,
                        slice,
                        actual,
                    });
                }
                u32::try_from(actual).map_err(|source| Error::SliceLengthOutOfRange {
                    step,
                    slice,
                    actual,
                    source,
                })
            })
            .collect::<Result<Profile, _>>()?;
        let count = |field, actual: usize| {
            u32::try_from(actual).map_err(|source| Error::CountOutOfRange {
                step,
                field,
                actual,
                source,
            })
        };
        let vertices = count(SimplexCountField::Vertices, counts.vertex_count())?;
        let edges = count(SimplexCountField::Edges, counts.edge_count())?;
        let triangles = count(SimplexCountField::Triangles, counts.triangle_count())?;
        let total_length = profile.iter().copied().map(u64::from).sum();
        for (field, expected, actual) in [
            (SimplexCountField::Vertices, total_length, vertices),
            (
                SimplexCountField::Triangles,
                2 * u64::from(vertices),
                triangles,
            ),
            (SimplexCountField::Edges, 3 * u64::from(vertices), edges),
        ] {
            if expected != u64::from(actual) {
                return Err(Error::CountMismatch {
                    step,
                    field,
                    expected,
                    actual,
                });
            }
        }
        Ok(Self {
            edges,
            profile,
            step,
            triangles,
            vertices,
        })
    }
}

/// Builds a tree of actual reversible insertions from the minimum torus to each profile.
/// The search RNG is independent of every measurement chain.
fn reachability(condition: Condition) -> Result<BTreeSet<Profile>, Error> {
    let slices = condition.slices as usize;
    let minimum = vec![3; slices];
    let start = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&vec![3; slices])?;
    let mut pending = VecDeque::from([(minimum.clone(), start)]);
    let mut visited = BTreeSet::from([minimum]);
    let mut targets = BTreeSet::new();
    let mut proposal = IntrinsicCdtProposal::default();
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(condition.reference.reachability_seed);
    while let Some((profile, state)) = pending.pop_front() {
        if state.triangle_count() == condition.triangles as usize {
            targets.insert(profile);
            continue;
        }
        let mut children = BTreeSet::new();
        for slice in 0..slices {
            let mut child = profile.clone();
            child[slice] += 1;
            if !visited.contains(&child) {
                children.insert(child);
            }
        }
        for _ in 0..8192 {
            if children.is_empty() {
                break;
            }
            if let Some(plan) = proposal.propose_plan(&state, &mut rng)? {
                let next = plan.proposed_state();
                let child = measure(next, 0, slices)?.profile;
                let (forward, reverse) = plan.transition_probabilities();
                if forward > 0.0 && reverse > 0.0 && children.remove(&child) {
                    visited.insert(child.clone());
                    pending.push_back((child, next.clone()));
                }
            }
        }
        if !children.is_empty() {
            return Err(Error::Reachability);
        }
    }
    if targets.len() != condition.profiles {
        return Err(Error::Reachability);
    }
    Ok(targets)
}

/// Retains repeated states and off-target volumes on the same proposal-step clock.
fn sample(
    seed: u64,
    initial: Profile,
    burn_in: u32,
    draws: u32,
    target: &CdtTarget,
) -> Result<Run, Error> {
    let lengths: Vec<_> = initial.iter().map(|&n| n as usize).collect();
    let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&lengths)?;
    let mut chain = Chain::new(state, target)?;
    let mut proposal = IntrinsicCdtProposal::default();
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut rows = Vec::with_capacity(draws as usize);
    for step in 1..=burn_in + draws * CADENCE {
        let _step = chain.step_delayed(target, &mut proposal, &mut rng)?;
        if step > burn_in && (step - burn_in).is_multiple_of(CADENCE) {
            rows.push(measure(chain.state(), step, initial.len())?);
        }
    }
    Ok(Run {
        draws: rows,
        initial,
        seed,
    })
}

/// Diagnoses the full-cadence volume and each labeled slice with upstream estimators.
fn diagnostics(runs: &[Run], slices: u32) -> Vec<Diagnostic> {
    full_cadence_diagnostics(slices, |i| {
        runs.iter()
            .map(|run| {
                run.draws
                    .iter()
                    .map(|row| {
                        f64::from(if i == 0 {
                            row.triangles
                        } else {
                            row.profile[i - 1]
                        })
                    })
                    .collect()
            })
            .collect()
    })
}

/// Store blocks of 256 cadence rows per JSON line so replay only needs one block at a time.
fn write_chunked(path: &Path, record: &Acquisition<'_>) -> Result<(), Error> {
    std::fs::create_dir(path)?;
    let mut chains = Vec::new();
    for run in record.chains {
        let file = format!("chain-{}.jsonl.gz", run.seed);
        let mut writer = GzEncoder::new(
            HashingIo::new(BufWriter::new(File::create_new(path.join(&file))?)),
            Compression::fast(),
        );
        for block in run.draws.chunks(256) {
            serde_json::to_writer(&mut writer, block)?;
            writer.write_all(b"\n")?;
        }
        let mut compressed = writer.finish()?;
        compressed.flush()?;
        chains.push(json!({"seed":run.seed, "initial":run.initial, "file":file, "sha256":compressed.digest()}));
    }
    let mut header = BufWriter::new(File::create_new(path.join("manifest.json"))?);
    serde_json::to_writer(
        &mut header,
        &json!({
            "action":record.action, "burn_in":record.burn_in, "cadence":record.cadence, "chains":chains,
            "crate_version":record.crate_version, "diagnostics":record.diagnostics, "draws_per_chain":record.draws_per_chain,
            "family_policy":record.family_policy, "reachable_profiles":record.reachable_profiles, "rng":record.rng,
            "schema":record.schema, "temperature":record.temperature
        }),
    )?;
    header.flush()?;
    Ok(())
}

pub const CONDITIONS: [Condition; 4] = [N32, N40, N48, N64];

impl Condition {
    pub fn start(self, slot: usize) -> Profile {
        let slices = self.slices as usize;
        if slot < 4 {
            return vec![self.triangles / (2 * self.slices); slices];
        }
        let mut profile = vec![3; slices];
        profile[(slot - 4) * slices / 4] = self.triangles / 2 - 3 * (self.slices - 1);
        profile
    }

    pub fn folder(self) -> String {
        format!("t{}-n{}", self.slices, self.triangles)
    }
}

pub fn acquire(condition: Condition, path: &Path) -> Result<(), Error> {
    let reachable = reachability(condition)?;
    let fixing = VolumeFixing::quadratic_triangles(condition.triangles, STRENGTH)?;
    let action = ActionConfig::default().with_volume_fixing(Some(fixing))?;
    let target = CdtTarget::new(action.clone(), 1.0)?;
    let runs = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for (slot, seed) in (condition.first_seed..condition.first_seed + 8).enumerate() {
            let target_ref = &target;
            workers.push(scope.spawn(move || {
                sample(
                    seed,
                    condition.start(slot),
                    BURN_IN,
                    condition.draws,
                    target_ref,
                )
            }));
        }
        workers
            .into_iter()
            .map(|worker| worker.join().map_err(|_| Error::Worker)?)
            .collect::<Result<Vec<_>, Error>>()
    })?;
    eprintln!(
        "{}: collected all eight chains; computing full-cadence diagnostics",
        condition.folder()
    );
    let record = Acquisition {
        action: serde_json::to_value(action)?,
        burn_in: BURN_IN,
        cadence: CADENCE,
        chains: &runs,
        crate_version: env!("CARGO_PKG_VERSION"),
        diagnostics: diagnostics(&runs, condition.slices),
        draws_per_chain: condition.draws,
        family_policy: "uniform",
        reachable_profiles: reachable,
        rng: "Xoshiro256PlusPlus",
        schema: format!(
            "intrinsic-t{}-n{}/native-v2",
            condition.slices, condition.triangles
        ),
        temperature: 1.0,
    };
    write_chunked(path, &record)
}
