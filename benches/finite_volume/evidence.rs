#![forbid(unsafe_code)]

//! Typed errors, hashes and immutable JSON output for finite-volume evidence.

use super::acquire;
use super::acquire::Cohort;
use super::bootstrap::MomentCoverage;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Acquisition(#[from] acquire::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("scientific acceptance criteria unmet; see {0}")]
    Scientific(PathBuf),
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|&b| {
            [
                char::from(DIGITS[usize::from(b >> 4)]),
                char::from(DIGITS[usize::from(b & 15)]),
            ]
        })
        .collect()
}

pub fn file_hash(path: &Path) -> Result<String, Error> {
    let mut reader = HashingReader::new(File::open(path)?);
    std::io::copy(&mut reader, &mut std::io::sink())?;
    Ok(reader.digest())
}

/// Hash the bytes supplied to the caller, including gzip headers and trailers.
pub struct HashingReader<R> {
    input: R,
    hash: Sha256,
}

impl<R> HashingReader<R> {
    pub fn new(input: R) -> Self {
        Self {
            input,
            hash: Sha256::new(),
        }
    }

    pub fn digest(self) -> String {
        hex(&self.hash.finalize())
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.input.read(buffer)?;
        self.hash.update(&buffer[..count]);
        Ok(count)
    }
}

/// Hash the sorted, repository-relative input names and the exact bytes embedded by rustc.
pub fn build_source_hash() -> String {
    const SOURCES: &[(&str, &[u8])] =
        include!(concat!(env!("OUT_DIR"), "/finite_volume_sources.rs"));
    let mut tree = Sha256::new();
    for &(path, contents) in SOURCES {
        tree.update(hex(&Sha256::digest(contents)));
        tree.update(b"  ");
        tree.update(path.as_bytes());
        tree.update(b"\n");
    }
    hex(&tree.finalize())
}

pub const BUILD_RUSTC: &str = include_str!(concat!(env!("OUT_DIR"), "/finite_volume_rustc.txt"));

pub fn acquisition_manifest(
    cohort: Cohort,
    runtime_checkout: Option<&serde_json::Value>,
) -> Result<serde_json::Value, Error> {
    let conditions = cohort.conditions();
    Ok(serde_json::json!({
        "schema":"finite-volume-acquisition/v4", "cohort":cohort, "conditions":conditions,
        "moment_interval_scope":MomentCoverage::from(cohort),
        "reference":conditions.map(|c| c.reference),
        "binary_sha256":file_hash(&std::env::current_exe()?)?,
        "cargo_lock_sha256":hex(&Sha256::digest(include_bytes!("../../Cargo.lock"))),
        "build_rustc":BUILD_RUSTC,
        "runtime_checkout":runtime_checkout,
        "source_tree_sha256":build_source_hash(),
    }))
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let mut out = BufWriter::new(File::create_new(path)?);
    serde_json::to_writer_pretty(&mut out, value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
