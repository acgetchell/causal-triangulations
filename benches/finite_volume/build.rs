#![forbid(unsafe_code)]

//! Capture build inputs for the opt-in finite-volume benchmark.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn rust_sources(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_sources(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let directory = Path::new("benches/finite_volume");
    println!("cargo::rerun-if-changed={}", directory.display());
    println!("cargo::rerun-if-changed=src");
    let out =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing OUT_DIR"))?);
    let rustc = env::var_os("RUSTC").ok_or_else(|| io::Error::other("missing RUSTC"))?;
    let version = Command::new(rustc).arg("-vV").output()?;
    if !version.status.success() || version.stdout.is_empty() {
        return Err(io::Error::other(
            "could not identify Cargo's build compiler",
        ));
    }
    fs::write(out.join("finite_volume_rustc.txt"), version.stdout)?;
    for variable in ["TARGET", "PROFILE", "OPT_LEVEL", "DEBUG"] {
        let value = env::var(variable).map_err(|error| {
            io::Error::other(format!("Cargo did not supply {variable}: {error}"))
        })?;
        println!("cargo::rustc-env=CDT_BUILD_{variable}={value}");
    }

    // Only the benchmark and its bounded tests compile the generated includes.
    // rustc tracks their contents. The directory watches above cover new and removed files.
    let mut files = ["Cargo.toml", "Cargo.lock"].map(PathBuf::from).to_vec();
    rust_sources(Path::new("src"), &mut files)?;
    rust_sources(directory, &mut files)?;
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        let path = file
            .to_str()
            .ok_or_else(|| io::Error::other("non-UTF-8 Rust source path"))?;
        paths.push(path.replace(std::path::MAIN_SEPARATOR, "/"));
    }
    paths.sort_unstable();
    let mut list = fs::File::create(out.join("finite_volume_sources.rs"))?;
    writeln!(list, "&[")?;
    for path in paths {
        writeln!(
            list,
            "({path:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/\", {path:?})).as_slice()),"
        )?;
    }
    writeln!(list, "]")
}
