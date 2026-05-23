#![forbid(unsafe_code)]

//! Configuration management for CDT simulations.
//!
//! This module provides structured configuration for various aspects of
//! Causal Dynamical Triangulation simulations, including:
//! - Simulation parameters (temperature, steps, etc.)
//! - Action calculation parameters (coupling constants, cosmological constant)
//! - Triangulation generation parameters
//! - Runtime behavior options

use crate::cdt::action::ActionConfig;
use crate::cdt::metropolis::MetropolisConfig;
use crate::errors::{CdtError, CdtResult, ConfigurationSetting};
use clap::error::ErrorKind;
use clap::{ArgGroup, Error as ClapError, Parser, ValueEnum};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::path::{Component, Path, PathBuf};

/// Topology of the spatial slices in the CDT triangulation.
///
/// Determines the boundary conditions for the simulation:
/// - [`OpenBoundary`](Self::OpenBoundary) — open-boundary generation; topology
///   validation accepts disk-like χ = 1 and sphere-like χ = 2 configurations
/// - [`Toroidal`](Self::Toroidal) — periodic in both space and time (S¹×S¹, χ = 0)
///
/// Serde uses the same kebab-case vocabulary as the CLI (`open-boundary`,
/// `toroidal`) so saved JSON configuration can round-trip through command-line
/// overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CdtTopology {
    /// Finite strip with open boundaries (Euler characteristic χ = 1 for
    /// disk-like or χ = 2 for sphere-like configurations).
    #[default]
    OpenBoundary,
    /// Periodic in both space and time, forming a torus S¹×S¹ (χ = 0).
    ///
    /// Built via
    /// [`CdtTriangulation::from_toroidal_cdt`](crate::cdt::triangulation::CdtTriangulation::from_toroidal_cdt),
    /// which constructs an explicit `(N · T)`-vertex mesh with both spatial
    /// and temporal wrap-around.
    Toroidal,
}

impl fmt::Display for CdtTopology {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenBoundary => formatter.write_str("open boundary"),
            Self::Toroidal => formatter.write_str("toroidal"),
        }
    }
}

/// Main configuration structure for CDT simulations.
///
/// This combines the canonical runtime options for CDT construction,
/// action calculation, and Metropolis sampling. Command-line parsing may derive
/// some of these values from more ergonomic inputs; for example,
/// `--vertices-per-slice` is converted into the total [`Self::vertices`] count
/// and `--volume-profile` records explicit per-slice initial spatial volumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdtConfig {
    /// Dimensionality of the triangulation.
    pub dimension: Option<u8>,

    /// Total number of vertices in the initial triangulation.
    ///
    /// This is the canonical runtime count. For regular initial CDT data it
    /// must divide evenly by [`Self::timeslices`]; the binary's
    /// `--vertices-per-slice` option computes this total before constructing
    /// `CdtConfig`.
    pub vertices: u32,

    /// Number of time slices in the initial triangulation.
    pub timeslices: u32,

    /// Optional explicit initial spatial volume profile, in vertices per time slice.
    ///
    /// When present, this vector has exactly [`Self::timeslices`] entries and its
    /// sum equals [`Self::vertices`]. It represents general nonuniform CDT
    /// initial data. When absent, initialization uses regular equal-size slices.
    pub volume_profile: Option<Vec<u32>>,

    /// Temperature for the Metropolis acceptance rule.
    pub temperature: f64,

    /// Number of Monte Carlo proposals to evaluate when simulation is enabled.
    pub steps: u32,

    /// Number of initial simulation steps to exclude from measurements.
    pub thermalization_steps: u32,

    /// Measurement frequency, in simulation steps.
    pub measurement_frequency: u32,

    /// Coupling constant κ₀ for vertices in the action.
    pub coupling_0: f64,

    /// Coupling constant κ₂ for triangles in the action.
    pub coupling_2: f64,

    /// Cosmological constant λ in the action.
    ///
    /// Current simulations do not apply volume fixing. In the unfixed-volume
    /// ensemble this coupling controls volume growth or shrinkage through the
    /// ordinary action difference.
    pub cosmological_constant: f64,

    /// Run the full CDT Metropolis simulation after constructing the initial triangulation.
    pub simulate: bool,

    /// Optional RNG seed for reproducible simulations.
    pub seed: Option<u64>,

    /// Topology and boundary conditions for triangulation generation.
    pub topology: CdtTopology,

    /// Write per-measurement simulation data to a CSV file.
    ///
    /// Relative paths are resolved from the current working directory with
    /// [`CdtConfig::resolve_path`]. Parent directories are created when output
    /// is written. [`crate::run_simulation`] rejects configurations where CSV
    /// and JSON output paths resolve to the same file.
    pub output_csv: Option<PathBuf>,

    /// Write simulation metadata and aggregate summary data to a JSON file.
    ///
    /// Relative paths are resolved from the current working directory with
    /// [`CdtConfig::resolve_path`]. Parent directories are created when output
    /// is written. [`crate::run_simulation`] rejects configurations where CSV
    /// and JSON output paths resolve to the same file.
    pub output_json: Option<PathBuf>,
}

/// Command-line arguments accepted by the `cdt` binary.
#[derive(Parser)]
#[command(
    author,
    version,
    about = "Run 1+1-dimensional Causal Dynamical Triangulations simulations",
    long_about = "Run 1+1-dimensional Causal Dynamical Triangulations simulations.\n\nConstruct a foliated CDT triangulation, optionally run the Metropolis move loop, and write measurement summaries. Prefer --vertices-per-slice for regular initial data or --volume-profile for explicit nonuniform initial slice volumes; --vertices remains available when the total initial vertex count is already known.",
    group = ArgGroup::new("vertex_count")
        .required(true)
        .args(["vertices", "vertices_per_slice", "volume_profile"])
)]
struct CdtCliArgs {
    #[arg(
        short,
        long,
        help = "Triangulation dimension; currently only 2 is implemented",
        value_parser = clap::value_parser!(u8).range(2..3)
    )]
    dimension: Option<u8>,

    #[arg(
        short,
        long,
        help = "Total initial vertex count; must divide evenly by --timeslices",
        value_parser = clap::value_parser!(u32).range(3..)
    )]
    vertices: Option<u32>,

    #[arg(
        long,
        help = "Vertices per spatial slice; total vertices are computed as this value times --timeslices",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    vertices_per_slice: Option<u32>,

    #[arg(
        short,
        long,
        help = "Number of time slices; open-boundary requires at least 2, toroidal at least 3",
        required_unless_present = "volume_profile",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    timeslices: Option<u32>,

    #[arg(
        long,
        value_name = "N0,N1,...",
        help = "Explicit vertices per time slice for nonuniform initial spatial volume",
        long_help = "Explicit vertices per time slice for nonuniform initial spatial volume, for example 8,10,7,9. If --timeslices is also supplied it must match the number of profile entries."
    )]
    volume_profile: Option<String>,

    #[arg(long, default_value = "1.0", help = "Positive Metropolis temperature")]
    temperature: f64,

    #[arg(
        long,
        default_value = "1000",
        help = "Number of Metropolis proposals to evaluate when --simulate is set"
    )]
    steps: u32,

    #[arg(
        long,
        default_value = "100",
        help = "Number of initial simulation steps to exclude from measurements"
    )]
    thermalization_steps: u32,

    #[arg(
        long,
        default_value = "10",
        help = "Record one measurement every N simulation steps",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    measurement_frequency: u32,

    #[arg(
        long,
        default_value = "1.0",
        help = "Regge action vertex coupling kappa_0"
    )]
    coupling_0: f64,

    #[arg(
        long,
        default_value = "1.0",
        help = "Regge action triangle coupling kappa_2"
    )]
    coupling_2: f64,

    #[arg(
        long,
        default_value = "0.1",
        help = "Regge action cosmological constant lambda"
    )]
    cosmological_constant: f64,

    #[arg(
        long,
        default_value_t = false,
        help = "Run the Metropolis move loop after constructing the initial triangulation"
    )]
    simulate: bool,

    #[arg(long, help = "RNG seed for reproducible move selection and acceptance")]
    seed: Option<u64>,

    #[arg(
        long,
        value_enum,
        default_value_t = CdtTopology::default(),
        help = "Topology and boundary conditions for the initial triangulation"
    )]
    topology: CdtTopology,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write per-measurement simulation data to a CSV file",
        long_help = "Write per-measurement simulation data to a CSV file. Relative paths are resolved from the current working directory, and parent directories are created when output is written."
    )]
    output_csv: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write run metadata, aggregate summaries, and final triangulation data to JSON",
        long_help = "Write run metadata, aggregate summaries, and final triangulation data to JSON. Relative paths are resolved from the current working directory, and parent directories are created when output is written."
    )]
    output_json: Option<PathBuf>,
}

impl CdtCliArgs {
    /// Converts parsed CLI arguments into the canonical runtime configuration.
    fn into_config(self) -> Result<CdtConfig, ClapError> {
        let (vertices, timeslices, volume_profile) = if let Some(raw_profile) = self.volume_profile
        {
            let profile = parse_volume_profile(&raw_profile).map_err(|err| {
                ClapError::raw(
                    ErrorKind::ValueValidation,
                    format!("invalid --volume-profile: {err}"),
                )
            })?;
            let profile_slices = u32::try_from(profile.len()).map_err(|err| {
                ClapError::raw(
                    ErrorKind::ValueValidation,
                    format!("--volume-profile has too many entries for u32 timeslices: {err}"),
                )
            })?;
            let timeslices = self.timeslices.unwrap_or(profile_slices);
            if timeslices != profile_slices {
                return Err(ClapError::raw(
                    ErrorKind::ValueValidation,
                    format!(
                        "--timeslices ({timeslices}) must match --volume-profile entry count ({profile_slices})"
                    ),
                ));
            }
            let vertices = profile.iter().try_fold(0_u32, |total, &volume| {
                total.checked_add(volume).ok_or_else(|| {
                    ClapError::raw(
                        ErrorKind::ValueValidation,
                        "--volume-profile total vertices exceed u32::MAX",
                    )
                })
            })?;
            (vertices, timeslices, Some(profile))
        } else {
            let timeslices = self
                .timeslices
                .expect("required clap arguments should set timeslices without a volume profile");
            let vertices = if let Some(vertices) = self.vertices {
                vertices
            } else {
                let vertices_per_slice = self
                    .vertices_per_slice
                    .expect("required clap group should set one vertex count");
                vertices_per_slice.checked_mul(timeslices).ok_or_else(|| {
                    ClapError::raw(
                        ErrorKind::ValueValidation,
                        format!(
                            "--vertices-per-slice ({vertices_per_slice}) × --timeslices ({timeslices}) exceeds u32::MAX"
                        ),
                    )
                })?
            };
            (vertices, timeslices, None)
        };

        Ok(CdtConfig {
            dimension: self.dimension,
            vertices,
            timeslices,
            volume_profile,
            temperature: self.temperature,
            steps: self.steps,
            thermalization_steps: self.thermalization_steps,
            measurement_frequency: self.measurement_frequency,
            coupling_0: self.coupling_0,
            coupling_2: self.coupling_2,
            cosmological_constant: self.cosmological_constant,
            simulate: self.simulate,
            seed: self.seed,
            topology: self.topology,
            output_csv: self.output_csv,
            output_json: self.output_json,
        })
    }
}

/// Controls how dimension overrides are applied when merging configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DimensionOverride {
    /// Replace the dimension with the supplied value.
    Value(u8),
    /// Clear the dimension so it falls back to the default.
    Clear,
}

/// A collection of optional override values for [`CdtConfig`].
///
/// Each field is optional, allowing callers to override only the configuration entries
/// that need changing while leaving the rest untouched.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CdtConfigOverrides {
    /// Optional override for the triangulation dimension.
    pub dimension: Option<DimensionOverride>,
    /// Optional override for the vertex count.
    pub vertices: Option<u32>,
    /// Optional override for the timeslice count.
    pub timeslices: Option<u32>,
    /// Optional override for the explicit initial spatial volume profile.
    ///
    /// `None` leaves the base profile unchanged, `Some(None)` clears it so
    /// regular equal-size slices are used, and `Some(Some(profile))` replaces it
    /// with explicit per-slice volumes.
    #[expect(
        clippy::option_option,
        reason = "None=no override, Some(None)=clear profile, Some(Some(v))=set profile"
    )]
    pub volume_profile: Option<Option<Vec<u32>>>,
    /// Optional override for the temperature.
    pub temperature: Option<f64>,
    /// Optional override for the total number of steps.
    pub steps: Option<u32>,
    /// Optional override for the number of thermalization steps.
    pub thermalization_steps: Option<u32>,
    /// Optional override for the measurement frequency.
    pub measurement_frequency: Option<u32>,
    /// Optional override for κ₀.
    pub coupling_0: Option<f64>,
    /// Optional override for κ₂.
    pub coupling_2: Option<f64>,
    /// Optional override for the cosmological constant λ.
    pub cosmological_constant: Option<f64>,
    /// Optional override for the simulation flag.
    pub simulate: Option<bool>,
    /// Optional override for the RNG seed.
    #[expect(
        clippy::option_option,
        reason = "None=no override, Some(None)=clear seed, Some(Some(v))=set seed"
    )]
    pub seed: Option<Option<u64>>,
    /// Optional override for the topology.
    pub topology: Option<CdtTopology>,
    /// Optional override for CSV output path.
    #[expect(
        clippy::option_option,
        reason = "None=no override, Some(None)=clear output path, Some(Some(v))=set output path"
    )]
    pub output_csv: Option<Option<PathBuf>>,
    /// Optional override for JSON output path.
    #[expect(
        clippy::option_option,
        reason = "None=no override, Some(None)=clear output path, Some(Some(v))=set output path"
    )]
    pub output_json: Option<Option<PathBuf>>,
}

impl CdtConfig {
    /// Merges this configuration with a set of override values, returning a new configuration.
    ///
    /// Override fields that are `None` are ignored, leaving the original configuration values
    /// unchanged. When an override value is provided, it replaces the corresponding field in
    /// the returned configuration.
    ///
    /// A provided volume profile is an atomic override for its derived counts:
    /// the returned configuration recomputes `vertices` from the profile sum and
    /// `timeslices` from the profile length, even if those scalar fields were
    /// also present in the override set.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::errors::CdtResult;
    /// use causal_triangulations::prelude::config::{
    ///     CdtConfig, CdtConfigOverrides, DimensionOverride,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    /// let base = CdtConfig::new(10, 2);
    /// let overrides = CdtConfigOverrides {
    ///     dimension: Some(DimensionOverride::Clear),
    ///     vertices: Some(24),
    ///     ..CdtConfigOverrides::default()
    /// };
    ///
    /// let merged = base.merge_with_override(&overrides)?;
    /// assert_eq!(merged.dimension, None);
    /// assert_eq!(merged.vertices, 24);
    /// assert_eq!(merged.timeslices, 2);
    /// # Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidConfiguration`] if an override volume profile cannot
    /// be represented by the `u32` vertex or time-slice counts stored in [`CdtConfig`].
    pub fn merge_with_override(&self, overrides: &CdtConfigOverrides) -> CdtResult<Self> {
        let mut merged = self.clone();

        if let Some(dimension_override) = overrides.dimension {
            match dimension_override {
                DimensionOverride::Value(value) => {
                    merged.dimension = Some(value);
                }
                DimensionOverride::Clear => {
                    merged.dimension = None;
                }
            }
        }

        if let Some(vertices) = overrides.vertices {
            merged.vertices = vertices;
        }

        if let Some(timeslices) = overrides.timeslices {
            merged.timeslices = timeslices;
        }

        // Profiles are atomic overrides for their derived counts; otherwise a
        // merged config can pair stale vertex/time-slice totals with new slices.
        match &overrides.volume_profile {
            Some(Some(profile)) => {
                let profile_vertices = profile.iter().try_fold(0_u32, |total, &vertices| {
                    total.checked_add(vertices).ok_or_else(|| {
                        invalid_config(
                            ConfigurationSetting::Vertices,
                            &format!("{profile:?}"),
                            &"volume profile sum <= u32::MAX",
                        )
                    })
                })?;
                let profile_timeslices = u32::try_from(profile.len()).map_err(|err| {
                    invalid_config(
                        ConfigurationSetting::Timeslices,
                        &profile.len(),
                        &format!("volume profile length must fit in u32: {err}"),
                    )
                })?;

                merged.vertices = profile_vertices;
                merged.timeslices = profile_timeslices;
                merged.volume_profile = Some(profile.clone());
            }
            Some(None) => merged.volume_profile = None,
            None => {}
        }

        if let Some(temperature) = overrides.temperature {
            merged.temperature = temperature;
        }

        if let Some(steps) = overrides.steps {
            merged.steps = steps;
        }

        if let Some(thermalization_steps) = overrides.thermalization_steps {
            merged.thermalization_steps = thermalization_steps;
        }

        if let Some(measurement_frequency) = overrides.measurement_frequency {
            merged.measurement_frequency = measurement_frequency;
        }

        if let Some(coupling_0) = overrides.coupling_0 {
            merged.coupling_0 = coupling_0;
        }

        if let Some(coupling_2) = overrides.coupling_2 {
            merged.coupling_2 = coupling_2;
        }

        if let Some(cosmological_constant) = overrides.cosmological_constant {
            merged.cosmological_constant = cosmological_constant;
        }

        if let Some(simulate) = overrides.simulate {
            merged.simulate = simulate;
        }

        if let Some(seed) = overrides.seed {
            merged.seed = seed;
        }

        if let Some(topology) = overrides.topology {
            merged.topology = topology;
        }

        if let Some(output_csv) = &overrides.output_csv {
            merged.output_csv.clone_from(output_csv);
        }

        if let Some(output_json) = &overrides.output_json {
            merged.output_json.clone_from(output_json);
        }

        Ok(merged)
    }

    /// Resolves a candidate path against a base directory, expanding user home references
    /// and normalizing relative segments (e.g., `.` and `..`).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use causal_triangulations::CdtConfig;
    ///
    /// let resolved = CdtConfig::resolve_path("/tmp/project", "data/../config.toml");
    /// assert_eq!(resolved, PathBuf::from("/tmp/project/config.toml"));
    /// ```
    #[must_use]
    pub fn resolve_path(base_dir: impl AsRef<Path>, candidate: impl AsRef<Path>) -> PathBuf {
        let candidate = candidate.as_ref();

        if candidate.is_absolute() {
            return normalize_components(candidate);
        }

        if let Some(candidate_str) = candidate.to_str()
            && let Some(rest) = candidate_str.strip_prefix('~')
        {
            if rest.is_empty() {
                if let Some(home) = home_dir() {
                    return normalize_components(&home);
                }
            } else if matches!(rest.chars().next(), Some('/' | '\\'))
                && let Some(home) = home_dir()
            {
                let stripped = rest.trim_start_matches(['/', '\\']);
                let path = if stripped.is_empty() {
                    home
                } else {
                    home.join(stripped)
                };
                return normalize_components(&path);
            }
        }

        let joined = base_dir.as_ref().join(candidate);
        normalize_components(&joined)
    }
}

/// Normalizes paths without touching the filesystem so config parsing stays side-effect free.
fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Don't pop if the path is empty or we're at the root
                let mut components = normalized.components();
                let at_root = match components.next() {
                    Some(Component::RootDir) => components.next().is_none(),
                    Some(Component::Prefix(_)) => {
                        matches!(components.next(), Some(Component::RootDir))
                            && components.next().is_none()
                    }
                    _ => false,
                };
                let ends_with_parent = normalized
                    .components()
                    .next_back()
                    .is_some_and(|last| matches!(last, Component::ParentDir));

                if at_root {
                    continue;
                }

                if normalized.as_os_str().is_empty() || ends_with_parent {
                    normalized.push(Component::ParentDir.as_os_str());
                } else {
                    normalized.pop();
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
            Component::Normal(segment) => {
                normalized.push(segment);
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        PathBuf::from(Component::CurDir.as_os_str())
    } else {
        normalized
    }
}

/// Builds a typed configuration error from displayable values without duplicating field names.
fn invalid_config(
    setting: ConfigurationSetting,
    provided_value: &impl Display,
    expected: &impl Display,
) -> CdtError {
    invalid_config_parts(setting, provided_value.to_string(), expected.to_string())
}

/// Preserves already-formatted validation values when shared validators produce owned strings.
const fn invalid_config_parts(
    setting: ConfigurationSetting,
    provided_value: String,
    expected: String,
) -> CdtError {
    CdtError::InvalidConfiguration {
        setting,
        provided_value,
        expected,
    }
}

/// Rejects non-finite action couplings before they can poison action/log-probability math.
fn validate_coupling(setting: ConfigurationSetting, value: f64) -> CdtResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(invalid_config(setting, &value, &"finite"))
    }
}

/// Parses a comma-separated spatial volume profile from the CLI.
fn parse_volume_profile(raw: &str) -> Result<Vec<u32>, String> {
    let mut profile = Vec::new();

    for (index, entry) in raw.split(',').enumerate() {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(format!("volume profile entry {} is empty", index + 1));
        }
        let volume = trimmed.parse::<u32>().map_err(|err| {
            format!(
                "volume profile entry {} ({trimmed}) is not a positive integer: {err}",
                index + 1
            )
        })?;
        if volume == 0 {
            return Err(format!(
                "volume profile entry {} must be at least 1",
                index + 1
            ));
        }
        profile.push(volume);
    }

    if profile.is_empty() {
        Err("volume profile must contain at least one entry".to_string())
    } else {
        Ok(profile)
    }
}

/// Shares schedule validation between top-level config and Metropolis config.
pub(crate) fn validate_schedule(
    temperature: f64,
    steps: u32,
    thermalization_steps: u32,
    measurement_frequency: u32,
    mut error_for: impl FnMut(ConfigurationSetting, String, String) -> CdtError,
) -> CdtResult<()> {
    let mut invalid = |setting: ConfigurationSetting, provided_value: String, expected: String| {
        Err(error_for(setting, provided_value, expected))
    };

    if !temperature.is_finite() || temperature <= 0.0 {
        return invalid(
            ConfigurationSetting::Temperature,
            temperature.to_string(),
            "finite and positive".to_string(),
        );
    }

    if steps == 0 {
        return invalid(
            ConfigurationSetting::Steps,
            steps.to_string(),
            "≥ 1".to_string(),
        );
    }

    if measurement_frequency == 0 {
        return invalid(
            ConfigurationSetting::MeasurementFrequency,
            measurement_frequency.to_string(),
            "≥ 1".to_string(),
        );
    }

    if measurement_frequency > steps {
        return invalid(
            ConfigurationSetting::MeasurementFrequency,
            measurement_frequency.to_string(),
            format!("≤ steps ({steps})"),
        );
    }

    if thermalization_steps > steps {
        return invalid(
            ConfigurationSetting::ThermalizationSteps,
            thermalization_steps.to_string(),
            format!("≤ steps ({steps})"),
        );
    }

    let first_post_thermalization_measurement = u64::from(thermalization_steps)
        .div_ceil(u64::from(measurement_frequency))
        * u64::from(measurement_frequency);

    if first_post_thermalization_measurement > u64::from(steps) {
        return invalid(
            ConfigurationSetting::MeasurementSchedule,
            format!(
                "steps={steps}, thermalization_steps={thermalization_steps}, measurement_frequency={measurement_frequency}"
            ),
            "at least one post-thermalization measurement".to_string(),
        );
    }

    Ok(())
}

impl CdtConfig {
    /// Builds a new instance of `CdtConfig` from command-line arguments.
    ///
    /// This uses the binary-facing CLI parser and exits the process with
    /// Clap's diagnostic when parsing fails. Prefer [`Self::try_from_args`] in
    /// tests or embedding contexts that need structured errors.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use causal_triangulations::CdtConfig;
    ///
    /// let _config = CdtConfig::from_args();
    /// ```
    #[must_use]
    pub fn from_args() -> Self {
        CdtCliArgs::parse()
            .into_config()
            .unwrap_or_else(|err| err.exit())
    }

    /// Parses a `CdtConfig` from an explicit argument iterator without exiting
    /// the process on invalid input.
    ///
    /// Use this in tests, libraries, and other embedding contexts that need to
    /// inspect CLI parse errors. The first item should be the binary name, just
    /// like [`clap::Parser::try_parse_from`].
    ///
    /// # Errors
    ///
    /// Returns [`clap::Error`] when required arguments are missing, when any
    /// value fails Clap validation, when `--vertices-per-slice × --timeslices`
    /// overflows `u32`, or when `--volume-profile` cannot be parsed or does not
    /// match an explicitly supplied `--timeslices` count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtConfig;
    ///
    /// let config = CdtConfig::try_from_args([
    ///     "cdt",
    ///     "--vertices-per-slice",
    ///     "4",
    ///     "--timeslices",
    ///     "4",
    /// ])
    /// .expect("valid CLI arguments");
    ///
    /// assert_eq!(config.vertices, 16);
    /// assert_eq!(config.timeslices, 4);
    /// ```
    pub fn try_from_args<I, T>(args: I) -> Result<Self, ClapError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        CdtCliArgs::try_parse_from(args)?.into_config()
    }

    /// Creates a new configuration from total vertices and time slices.
    ///
    /// The `vertices` argument is the total initial vertex count, not a
    /// per-slice count. Call [`Self::validate`] before running a simulation to
    /// check topology-specific divisibility and per-slice minimums.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtConfig, CdtTopology};
    ///
    /// let config = CdtConfig::new(16, 4);
    /// assert_eq!(config.vertices, 16);
    /// assert_eq!(config.timeslices, 4);
    /// assert_eq!(config.topology, CdtTopology::OpenBoundary);
    /// ```
    #[must_use]
    pub const fn new(vertices: u32, timeslices: u32) -> Self {
        Self {
            dimension: Some(2),
            vertices,
            timeslices,
            volume_profile: None,
            temperature: 1.0,
            steps: 1000,
            thermalization_steps: 100,
            measurement_frequency: 10,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: false,
            seed: None,
            topology: CdtTopology::OpenBoundary,
            output_csv: None,
            output_json: None,
        }
    }

    /// Creates a `MetropolisConfig` from this configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtConfig;
    ///
    /// let config = CdtConfig {
    ///     seed: Some(42),
    ///     ..CdtConfig::new(16, 4)
    /// };
    /// let metropolis = config.to_metropolis_config();
    /// assert_eq!(metropolis.seed, Some(42));
    /// ```
    #[must_use]
    pub const fn to_metropolis_config(&self) -> MetropolisConfig {
        let config = MetropolisConfig::new(
            self.temperature,
            self.steps,
            self.thermalization_steps,
            self.measurement_frequency,
        );
        // Wire seed through if present
        MetropolisConfig {
            seed: self.seed,
            ..config
        }
    }

    /// Creates an `ActionConfig` from this configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::CdtConfig;
    ///
    /// let action = CdtConfig::new(16, 4).to_action_config();
    /// assert_relative_eq!(action.coupling_0, 1.0);
    /// ```
    #[must_use]
    pub const fn to_action_config(&self) -> ActionConfig {
        ActionConfig::new(self.coupling_0, self.coupling_2, self.cosmological_constant)
    }

    /// Gets the effective dimension (defaults to 2 if not specified).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtConfig;
    ///
    /// let config = CdtConfig {
    ///     dimension: None,
    ///     ..CdtConfig::new(16, 4)
    /// };
    /// assert_eq!(config.dimension(), 2);
    /// ```
    #[must_use]
    pub const fn dimension(&self) -> u8 {
        match self.dimension {
            Some(d) => d,
            None => 2,
        }
    }

    /// Validates the configuration parameters.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidConfiguration`] if the dimension is not 2, if
    /// action couplings or temperature are non-finite, if the measurement
    /// schedule cannot produce a post-thermalization measurement, or if topology
    /// constraints on time slices, total vertices, or per-slice vertices are not
    /// satisfied. Regular equal-slice initialization requires `vertices` to be
    /// divisible by `timeslices`; explicit [`Self::volume_profile`] initialization
    /// instead requires the profile length and sum to match the configured counts.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtConfig;
    ///
    /// let config = CdtConfig::new(16, 4);
    /// assert!(config.validate().is_ok());
    /// ```
    pub fn validate(&self) -> CdtResult<()> {
        if self.vertices < 3 {
            return Err(invalid_config(
                ConfigurationSetting::Vertices,
                &self.vertices,
                &"≥ 3",
            ));
        }

        if self.timeslices == 0 {
            return Err(invalid_config(
                ConfigurationSetting::Timeslices,
                &self.timeslices,
                &"≥ 1",
            ));
        }

        if let Some(dim) = self.dimension
            && dim != 2
        {
            return Err(invalid_config(ConfigurationSetting::Dimension, &dim, &"2"));
        }

        validate_coupling(ConfigurationSetting::Coupling0, self.coupling_0)?;
        validate_coupling(ConfigurationSetting::Coupling2, self.coupling_2)?;
        validate_coupling(
            ConfigurationSetting::CosmologicalConstant,
            self.cosmological_constant,
        )?;

        self.validate_volume_constraints()?;

        validate_schedule(
            self.temperature,
            self.steps,
            self.thermalization_steps,
            self.measurement_frequency,
            |setting, provided_value, expected| {
                invalid_config_parts(setting, provided_value, expected)
            },
        )
    }

    /// Validates topology-specific initial volume constraints.
    ///
    /// Regular configurations require equal slice divisibility and minimum
    /// per-slice volume; explicit profiles are delegated to the profile-aware
    /// validator so count and shape errors remain distinguishable.
    fn validate_volume_constraints(&self) -> CdtResult<()> {
        let (minimum_slices, minimum_vertices_per_slice, topology_label) = match self.topology {
            CdtTopology::OpenBoundary => (2, 4, "open-boundary topology"),
            CdtTopology::Toroidal => (3, 3, "toroidal topology"),
        };

        if self.timeslices < minimum_slices {
            return Err(invalid_config_parts(
                ConfigurationSetting::Timeslices,
                self.timeslices.to_string(),
                format!("≥ {minimum_slices} for {topology_label}"),
            ));
        }

        if let Some(profile) = &self.volume_profile {
            return self.validate_explicit_volume_profile(
                profile,
                minimum_vertices_per_slice,
                topology_label,
            );
        }

        if !self.vertices.is_multiple_of(self.timeslices) {
            return Err(invalid_config_parts(
                ConfigurationSetting::Vertices,
                self.vertices.to_string(),
                format!(
                    "divisible by timeslices ({}) for {topology_label}",
                    self.timeslices
                ),
            ));
        }

        let min_total = self
            .timeslices
            .checked_mul(minimum_vertices_per_slice)
            .ok_or_else(|| {
                invalid_config_parts(
                    ConfigurationSetting::Timeslices,
                    self.timeslices.to_string(),
                    format!(
                        "{minimum_vertices_per_slice} · timeslices must fit in u32 for {topology_label}"
                    ),
                )
            })?;
        if self.vertices < min_total {
            return Err(invalid_config_parts(
                ConfigurationSetting::Vertices,
                self.vertices.to_string(),
                format!(
                    "≥ {minimum_vertices_per_slice} · timeslices ({min_total}) for {topology_label}"
                ),
            ));
        }

        Ok(())
    }

    /// Validates that an explicit volume profile matches configured counts.
    ///
    /// This checks profile length, per-slice minima, sum overflow, and agreement
    /// with the top-level total vertex count before construction begins.
    fn validate_explicit_volume_profile(
        &self,
        profile: &[u32],
        minimum_vertices_per_slice: u32,
        topology_label: &str,
    ) -> CdtResult<()> {
        let expected_len = usize::try_from(self.timeslices).map_err(|err| {
            invalid_config_parts(
                ConfigurationSetting::Timeslices,
                self.timeslices.to_string(),
                format!("must fit usize for volume profile validation: {err}"),
            )
        })?;
        if profile.len() != expected_len {
            return Err(invalid_config_parts(
                ConfigurationSetting::VolumeProfile,
                format!("{} entries", profile.len()),
                format!("{} entries for configured timeslices", self.timeslices),
            ));
        }

        let mut total = 0_u32;
        for (slice, &volume) in profile.iter().enumerate() {
            if volume < minimum_vertices_per_slice {
                return Err(invalid_config_parts(
                    ConfigurationSetting::VolumeProfile,
                    format!("slice {slice} has {volume}"),
                    format!("each slice ≥ {minimum_vertices_per_slice} for {topology_label}"),
                ));
            }
            total = total.checked_add(volume).ok_or_else(|| {
                invalid_config_parts(
                    ConfigurationSetting::VolumeProfile,
                    format!("{profile:?}"),
                    "sum must fit in u32".to_string(),
                )
            })?;
        }

        if total != self.vertices {
            return Err(invalid_config_parts(
                ConfigurationSetting::Vertices,
                self.vertices.to_string(),
                format!("sum of volume_profile ({total})"),
            ));
        }

        Ok(())
    }
}

/// Configuration preset for quick testing.
#[derive(Debug, Clone)]
pub struct TestConfig;

impl TestConfig {
    /// Creates a small, fast configuration suitable for unit tests.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::TestConfig;
    ///
    /// let config = TestConfig::small();
    /// assert_eq!(config.steps, 10);
    /// ```
    #[must_use]
    pub const fn small() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 16,
            timeslices: 2,
            volume_profile: None,
            temperature: 1.0,
            steps: 10,
            thermalization_steps: 2,
            measurement_frequency: 2,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: false,
            seed: None,
            topology: CdtTopology::OpenBoundary,
            output_csv: None,
            output_json: None,
        }
    }

    /// Creates a medium-sized configuration for integration tests.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::TestConfig;
    ///
    /// let config = TestConfig::medium();
    /// assert_eq!(config.vertices, 64);
    /// ```
    #[must_use]
    pub const fn medium() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 64,
            timeslices: 4,
            volume_profile: None,
            temperature: 1.0,
            steps: 100,
            thermalization_steps: 20,
            measurement_frequency: 5,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: false,
            seed: None,
            topology: CdtTopology::OpenBoundary,
            output_csv: None,
            output_json: None,
        }
    }

    /// Creates a large configuration for performance testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::TestConfig;
    ///
    /// let config = TestConfig::large();
    /// assert_eq!(config.vertices, 256);
    /// ```
    #[must_use]
    pub const fn large() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 256,
            timeslices: 8,
            volume_profile: None,
            temperature: 1.0,
            steps: 1000,
            thermalization_steps: 100,
            measurement_frequency: 10,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: false,
            seed: None,
            topology: CdtTopology::OpenBoundary,
            output_csv: None,
            output_json: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use dirs::home_dir;
    use std::path::PathBuf;

    #[test]
    fn test_config_new() {
        let config = CdtConfig::new(36, 3);
        assert_eq!(config.vertices, 36);
        assert_eq!(config.timeslices, 3);
        assert_eq!(config.dimension(), 2);
        assert!(!config.simulate);
        assert_eq!(config.output_csv, None);
        assert_eq!(config.output_json, None);
    }

    #[test]
    fn try_from_args_parses_valid_cli_values() {
        let config = CdtConfig::try_from_args([
            "cdt",
            "--vertices",
            "36",
            "--timeslices",
            "3",
            "--simulate",
            "--seed",
            "42",
        ])
        .expect("valid CLI arguments should parse");

        assert_eq!(config.vertices, 36);
        assert_eq!(config.timeslices, 3);
        assert!(config.simulate);
        assert_eq!(config.seed, Some(42));
    }

    #[test]
    fn try_from_args_computes_total_vertices_from_vertices_per_slice() {
        let config = CdtConfig::try_from_args([
            "cdt",
            "--vertices-per-slice",
            "8",
            "--timeslices",
            "6",
            "--topology",
            "toroidal",
        ])
        .expect("per-slice CLI arguments should parse");

        assert_eq!(config.vertices, 48);
        assert_eq!(config.timeslices, 6);
        assert_eq!(config.topology, CdtTopology::Toroidal);
    }

    #[test]
    fn try_from_args_rejects_vertices_per_slice_overflow() {
        let vertices_per_slice = u32::MAX.to_string();
        let error = CdtConfig::try_from_args([
            "cdt",
            "--vertices-per-slice",
            &vertices_per_slice,
            "--timeslices",
            "2",
        ])
        .expect_err("overflowed per-slice total should be a parse error");

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(
            error
                .to_string()
                .contains("--vertices-per-slice (4294967295) × --timeslices (2) exceeds u32::MAX"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn try_from_args_returns_structured_error_without_exiting() {
        let error = CdtConfig::try_from_args([
            "cdt",
            "--vertices",
            "36",
            "--timeslices",
            "3",
            "--dimension",
            "3",
        ])
        .expect_err("unsupported dimension should be reported as a parse error");

        assert!(matches!(
            error.kind(),
            clap::error::ErrorKind::ValueValidation | clap::error::ErrorKind::InvalidValue
        ));
    }

    #[test]
    fn topology_serializes_with_cli_vocabulary() {
        let json =
            serde_json::to_string(&CdtTopology::OpenBoundary).expect("topology should serialize");
        assert_eq!(json, "\"open-boundary\"");

        let topology: CdtTopology =
            serde_json::from_str("\"toroidal\"").expect("topology should deserialize");
        assert_eq!(topology, CdtTopology::Toroidal);
    }

    #[test]
    fn test_config_conversions() {
        let config = CdtConfig::new(64, 4);

        let metropolis_config = config.to_metropolis_config();
        assert_relative_eq!(metropolis_config.temperature, 1.0);
        assert_eq!(metropolis_config.steps, 1000);

        let action_config = config.to_action_config();
        assert_relative_eq!(action_config.coupling_0, 1.0);
        assert_relative_eq!(action_config.coupling_2, 1.0);
        assert_relative_eq!(action_config.cosmological_constant, 0.1);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "validation test exercises the full structured configuration error matrix"
    )]
    fn test_config_validation() {
        let valid_config = CdtConfig::new(36, 3);
        assert!(valid_config.validate().is_ok());

        let invalid_vertices = CdtConfig {
            vertices: 2,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            invalid_vertices.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Vertices && provided_value == "2" && expected == "≥ 3"
        ));

        let invalid_timeslices = CdtConfig {
            timeslices: 0,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            invalid_timeslices.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Timeslices && provided_value == "0" && expected == "≥ 1"
        ));

        let invalid_temperature = CdtConfig {
            temperature: -1.0,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            invalid_temperature.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Temperature
                && provided_value == "-1"
                && expected == "finite and positive"
        ));

        let invalid_measurement_frequency = CdtConfig {
            measurement_frequency: 0,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            invalid_measurement_frequency.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::MeasurementFrequency
                && provided_value == "0"
                && expected == "≥ 1"
        ));

        let invalid_steps = CdtConfig {
            steps: 0,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            invalid_steps.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Steps && provided_value == "0" && expected == "≥ 1"
        ));

        let invalid_dimension = CdtConfig {
            dimension: Some(4),
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            invalid_dimension.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Dimension && provided_value == "4" && expected == "2"
        ));

        for (setting, value) in [
            (ConfigurationSetting::Coupling0, f64::NAN),
            (ConfigurationSetting::Coupling2, f64::INFINITY),
            (
                ConfigurationSetting::CosmologicalConstant,
                f64::NEG_INFINITY,
            ),
        ] {
            let mut invalid_action_coupling = CdtConfig::new(36, 3);
            match setting {
                ConfigurationSetting::Coupling0 => invalid_action_coupling.coupling_0 = value,
                ConfigurationSetting::Coupling2 => invalid_action_coupling.coupling_2 = value,
                ConfigurationSetting::CosmologicalConstant => {
                    invalid_action_coupling.cosmological_constant = value;
                }
                _ => unreachable!("test case should name a known action coupling"),
            }
            assert!(matches!(
                invalid_action_coupling.validate(),
                Err(CdtError::InvalidConfiguration {
                    setting: invalid_setting,
                    expected,
                    ..
                }) if invalid_setting == setting && expected == "finite"
            ));
        }

        let finite_action_couplings = CdtConfig {
            coupling_0: -1.5,
            coupling_2: 0.0,
            cosmological_constant: 2.25,
            ..CdtConfig::new(36, 3)
        };
        assert!(
            finite_action_couplings.validate().is_ok(),
            "finite signed action couplings should be accepted"
        );

        let measurement_frequency_exceeds_steps = CdtConfig {
            measurement_frequency: 2_000,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            measurement_frequency_exceeds_steps.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::MeasurementFrequency
                && provided_value == "2000"
                && expected == "≤ steps (1000)"
        ));

        let boundary_aligned_measurement = CdtConfig {
            steps: 11,
            thermalization_steps: 10,
            measurement_frequency: 10,
            ..CdtConfig::new(36, 3)
        };
        assert!(
            boundary_aligned_measurement.validate().is_ok(),
            "Configurations where thermalization ends on a measurement boundary should pass validation"
        );

        let boundary_aligned_final_measurement = CdtConfig {
            steps: 10,
            thermalization_steps: 10,
            measurement_frequency: 5,
            ..CdtConfig::new(36, 3)
        };
        assert!(
            boundary_aligned_final_measurement.validate().is_ok(),
            "Configurations with a final-step post-thermalization measurement should pass validation"
        );

        let final_step_measurement = CdtConfig {
            steps: 20,
            thermalization_steps: 15,
            measurement_frequency: 10,
            ..CdtConfig::new(36, 3)
        };
        assert!(
            final_step_measurement.validate().is_ok(),
            "A measurement taken exactly at the final completed step should satisfy the schedule"
        );

        let insufficient_measurements = CdtConfig {
            steps: 19,
            thermalization_steps: 15,
            measurement_frequency: 10,
            ..CdtConfig::new(36, 3)
        };
        match insufficient_measurements.validate() {
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) => {
                assert_eq!(setting, ConfigurationSetting::MeasurementSchedule);
                assert!(
                    provided_value.contains("steps=19")
                        && provided_value.contains("thermalization_steps=15")
                        && provided_value.contains("measurement_frequency=10"),
                    "Unexpected provided value: {provided_value}"
                );
                assert_eq!(expected, "at least one post-thermalization measurement");
            }
            other => panic!("Unexpected validation result: {other:?}"),
        }

        let thermalization_exceeds_steps = CdtConfig {
            steps: 10,
            thermalization_steps: 11,
            measurement_frequency: 1,
            ..CdtConfig::new(36, 3)
        };
        assert!(matches!(
            thermalization_exceeds_steps.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::ThermalizationSteps
                && provided_value == "11"
                && expected == "≤ steps (10)"
        ));

        let overflowed_post_thermalization_boundary = CdtConfig {
            steps: u32::MAX,
            thermalization_steps: u32::MAX,
            measurement_frequency: 2,
            ..CdtConfig::new(36, 3)
        };
        match overflowed_post_thermalization_boundary.validate() {
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) => {
                assert_eq!(setting, ConfigurationSetting::MeasurementSchedule);
                assert!(
                    provided_value.contains("steps=4294967295")
                        && provided_value.contains("thermalization_steps=4294967295")
                        && provided_value.contains("measurement_frequency=2"),
                    "Unexpected provided value: {provided_value}"
                );
                assert_eq!(expected, "at least one post-thermalization measurement");
            }
            other => panic!("Unexpected validation result: {other:?}"),
        }
    }

    #[test]
    fn test_config_validation_toroidal_topology() {
        // Valid toroidal config: T=3, N=4 vertices/slice, total=12
        let valid_toroidal = CdtConfig {
            topology: CdtTopology::Toroidal,
            vertices: 12,
            timeslices: 3,
            ..CdtConfig::new(12, 3)
        };
        assert!(
            valid_toroidal.validate().is_ok(),
            "Valid toroidal config (T=3, V=12) should validate"
        );

        // T < 3 must be rejected for toroidal topology.
        let toroidal_t_too_small = CdtConfig {
            topology: CdtTopology::Toroidal,
            vertices: 6,
            timeslices: 2,
            ..CdtConfig::new(6, 2)
        };
        assert!(matches!(
            toroidal_t_too_small.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Timeslices
                && provided_value == "2"
                && expected == "≥ 3 for toroidal topology"
        ));

        // Vertices not divisible by timeslices must be rejected.
        let toroidal_indivisible = CdtConfig {
            topology: CdtTopology::Toroidal,
            vertices: 11,
            timeslices: 3,
            ..CdtConfig::new(11, 3)
        };
        assert!(matches!(
            toroidal_indivisible.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Vertices
                && provided_value == "11"
                && expected == "divisible by timeslices (3) for toroidal topology"
        ));

        // Fewer than 3 vertices per slice must be rejected (e.g. T=3, N=2).
        let toroidal_too_few_per_slice = CdtConfig {
            topology: CdtTopology::Toroidal,
            vertices: 6,
            timeslices: 3,
            ..CdtConfig::new(6, 3)
        };
        assert!(matches!(
            toroidal_too_few_per_slice.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Vertices
                && provided_value == "6"
                && expected == "≥ 3 · timeslices (9) for toroidal topology"
        ));

        let toroidal_min_total_overflow = CdtConfig {
            topology: CdtTopology::Toroidal,
            vertices: u32::MAX,
            timeslices: u32::MAX,
            ..CdtConfig::new(u32::MAX, u32::MAX)
        };
        assert!(matches!(
            toroidal_min_total_overflow.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Timeslices
                && provided_value == u32::MAX.to_string()
                && expected == "3 · timeslices must fit in u32 for toroidal topology"
        ));
    }

    #[test]
    fn test_config_validation_open_boundary_regular_slices() {
        let valid_open_boundary = CdtConfig {
            topology: CdtTopology::OpenBoundary,
            vertices: 12,
            timeslices: 3,
            ..CdtConfig::new(12, 3)
        };
        assert!(
            valid_open_boundary.validate().is_ok(),
            "valid open-boundary config should validate"
        );

        let open_boundary_too_few_slices = CdtConfig {
            topology: CdtTopology::OpenBoundary,
            vertices: 4,
            timeslices: 1,
            ..CdtConfig::new(4, 1)
        };
        assert!(matches!(
            open_boundary_too_few_slices.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Timeslices
                && provided_value == "1"
                && expected == "≥ 2 for open-boundary topology"
        ));

        let open_boundary_indivisible = CdtConfig {
            topology: CdtTopology::OpenBoundary,
            vertices: 11,
            timeslices: 3,
            ..CdtConfig::new(11, 3)
        };
        assert!(matches!(
            open_boundary_indivisible.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Vertices
                && provided_value == "11"
                && expected == "divisible by timeslices (3) for open-boundary topology"
        ));

        let open_boundary_too_few_per_slice = CdtConfig {
            topology: CdtTopology::OpenBoundary,
            vertices: 9,
            timeslices: 3,
            ..CdtConfig::new(9, 3)
        };
        assert!(matches!(
            open_boundary_too_few_per_slice.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Vertices
                && provided_value == "9"
                && expected == "≥ 4 · timeslices (12) for open-boundary topology"
        ));
    }

    #[test]
    fn test_config_validation_open_boundary_volume_profile() {
        let valid_profile = CdtConfig {
            vertices: 15,
            timeslices: 3,
            volume_profile: Some(vec![4, 6, 5]),
            ..CdtConfig::new(12, 3)
        };
        assert!(
            valid_profile.validate().is_ok(),
            "nonuniform open-boundary profiles should not require divisible vertex counts"
        );

        let mismatched_sum = CdtConfig {
            vertices: 14,
            timeslices: 3,
            volume_profile: Some(vec![4, 6, 5]),
            ..CdtConfig::new(12, 3)
        };
        assert!(matches!(
            mismatched_sum.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::Vertices
                && provided_value == "14"
                && expected == "sum of volume_profile (15)"
        ));

        let mismatched_len = CdtConfig {
            vertices: 15,
            timeslices: 4,
            volume_profile: Some(vec![4, 6, 5]),
            ..CdtConfig::new(16, 4)
        };
        assert!(matches!(
            mismatched_len.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::VolumeProfile
                && provided_value == "3 entries"
                && expected == "4 entries for configured timeslices"
        ));
    }

    #[test]
    fn test_config_validation_toroidal_volume_profile_minimum_slice_size() {
        let valid_profile = CdtConfig {
            vertices: 16,
            timeslices: 4,
            topology: CdtTopology::Toroidal,
            volume_profile: Some(vec![3, 4, 5, 4]),
            ..CdtConfig::new(16, 4)
        };
        assert!(valid_profile.validate().is_ok());

        let too_small = CdtConfig {
            vertices: 11,
            timeslices: 4,
            topology: CdtTopology::Toroidal,
            volume_profile: Some(vec![3, 2, 3, 3]),
            ..CdtConfig::new(12, 4)
        };
        assert!(matches!(
            too_small.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == ConfigurationSetting::VolumeProfile
                && provided_value == "slice 1 has 2"
                && expected == "each slice ≥ 3 for toroidal topology"
        ));
    }

    #[test]
    fn test_try_from_args_derives_counts_from_volume_profile() {
        let config = CdtConfig::try_from_args(["cdt", "--volume-profile", "4, 6,5"])
            .expect("volume profile CLI should derive counts");

        assert_eq!(config.vertices, 15);
        assert_eq!(config.timeslices, 3);
        assert_eq!(config.volume_profile, Some(vec![4, 6, 5]));
    }

    #[test]
    fn test_try_from_args_rejects_malformed_volume_profile() {
        let error = CdtConfig::try_from_args(["cdt", "--volume-profile", "4,,5"])
            .expect_err("empty profile entries should be rejected");

        assert!(
            error
                .to_string()
                .contains("invalid --volume-profile: volume profile entry 2 is empty"),
            "{error}"
        );
    }

    #[test]
    fn test_dimension_defaults_to_two_when_unspecified() {
        let config = CdtConfig {
            dimension: None,
            ..CdtConfig::new(36, 3)
        };
        assert_eq!(config.dimension(), 2);
    }

    #[test]
    fn test_preset_configs() {
        let small = TestConfig::small();
        assert!(small.validate().is_ok());
        assert_eq!(small.vertices, 16);
        assert_eq!(small.steps, 10);

        let medium = TestConfig::medium();
        assert!(medium.validate().is_ok());
        assert_eq!(medium.vertices, 64);
        assert_eq!(medium.steps, 100);

        let large = TestConfig::large();
        assert!(large.validate().is_ok());
        assert_eq!(large.vertices, 256);
        assert_eq!(large.steps, 1000);
    }

    #[test]
    fn test_merge_with_override_updates_specified_fields() {
        let base = CdtConfig::new(10, 2);
        let overrides = CdtConfigOverrides {
            dimension: Some(DimensionOverride::Clear),
            vertices: Some(42),
            temperature: Some(2.5),
            simulate: Some(false),
            ..CdtConfigOverrides::default()
        };

        let merged = base
            .merge_with_override(&overrides)
            .expect("override merge should succeed");

        assert_eq!(merged.dimension, None);
        assert_eq!(merged.dimension(), 2);
        assert_eq!(merged.vertices, 42);
        assert_relative_eq!(merged.temperature, 2.5);
        assert!(!merged.simulate);

        // Unspecified fields should remain unchanged.
        assert_eq!(merged.timeslices, base.timeslices);
        assert_eq!(merged.steps, base.steps);
    }

    #[test]
    fn test_merge_with_override_updates_remaining_fields() {
        let base = CdtConfig::new(10, 2);
        let overrides = CdtConfigOverrides {
            timeslices: Some(5),
            volume_profile: Some(Some(vec![4, 6, 5])),
            steps: Some(250),
            thermalization_steps: Some(25),
            measurement_frequency: Some(5),
            coupling_0: Some(1.5),
            coupling_2: Some(2.5),
            cosmological_constant: Some(0.25),
            seed: Some(Some(99)),
            topology: Some(CdtTopology::Toroidal),
            output_csv: Some(Some(PathBuf::from("measurements.csv"))),
            output_json: Some(Some(PathBuf::from("summary.json"))),
            ..CdtConfigOverrides::default()
        };

        let merged = base
            .merge_with_override(&overrides)
            .expect("override merge should succeed");

        assert_eq!(merged.vertices, 15);
        assert_eq!(merged.timeslices, 3);
        assert_eq!(merged.volume_profile, Some(vec![4, 6, 5]));
        assert_eq!(merged.steps, 250);
        assert_eq!(merged.thermalization_steps, 25);
        assert_eq!(merged.measurement_frequency, 5);
        assert_relative_eq!(merged.coupling_0, 1.5);
        assert_relative_eq!(merged.coupling_2, 2.5);
        assert_relative_eq!(merged.cosmological_constant, 0.25);
        assert_eq!(merged.seed, Some(99));
        assert_eq!(merged.topology, CdtTopology::Toroidal);
        assert_eq!(merged.output_csv, Some(PathBuf::from("measurements.csv")));
        assert_eq!(merged.output_json, Some(PathBuf::from("summary.json")));
    }

    #[test]
    fn test_merge_with_override_can_clear_seed() {
        let base = CdtConfig {
            seed: Some(77),
            ..CdtConfig::new(10, 2)
        };
        let overrides = CdtConfigOverrides {
            seed: Some(None),
            ..CdtConfigOverrides::default()
        };

        let merged = base
            .merge_with_override(&overrides)
            .expect("override merge should succeed");

        assert_eq!(merged.seed, None);
    }

    #[test]
    fn test_merge_with_override_can_clear_volume_profile() {
        let base = CdtConfig {
            vertices: 15,
            timeslices: 3,
            volume_profile: Some(vec![4, 6, 5]),
            ..CdtConfig::new(12, 3)
        };
        let overrides = CdtConfigOverrides {
            volume_profile: Some(None),
            ..CdtConfigOverrides::default()
        };

        let merged = base
            .merge_with_override(&overrides)
            .expect("override merge should succeed");

        assert_eq!(merged.volume_profile, None);
        assert_eq!(merged.vertices, 15);
        assert_eq!(merged.timeslices, 3);
    }

    #[test]
    fn test_merge_with_override_can_clear_dimension() {
        let base = CdtConfig::new(10, 2);
        let overrides = CdtConfigOverrides {
            dimension: Some(DimensionOverride::Clear),
            ..CdtConfigOverrides::default()
        };

        let merged = base
            .merge_with_override(&overrides)
            .expect("override merge should succeed");
        assert_eq!(merged.dimension, None);
        assert_eq!(merged.dimension(), 2); // dimension() defaults to 2 when None
    }

    #[test]
    fn test_merge_with_override_rejects_volume_profile_vertex_overflow() {
        let base = CdtConfig::new(10, 2);
        let overrides = CdtConfigOverrides {
            volume_profile: Some(Some(vec![u32::MAX, 1])),
            ..CdtConfigOverrides::default()
        };

        let result = base.merge_with_override(&overrides);

        assert!(
            matches!(
                result,
                Err(CdtError::InvalidConfiguration {
                    setting: ConfigurationSetting::Vertices,
                    ref provided_value,
                    ref expected,
                }) if provided_value == "[4294967295, 1]"
                    && expected == "volume profile sum <= u32::MAX"
            ),
            "{result:?}"
        );
    }

    #[test]
    fn test_resolve_path_with_absolute_path() {
        let abs = PathBuf::from("/tmp/example");
        let resolved = CdtConfig::resolve_path("/does/not/matter", &abs);
        assert_eq!(resolved, PathBuf::from("/tmp/example"));
    }

    #[test]
    fn test_resolve_path_with_relative_path() {
        let base = PathBuf::from("/tmp/base");
        let candidate = PathBuf::from("config/settings.toml");
        let resolved = CdtConfig::resolve_path(&base, &candidate);
        assert_eq!(resolved, PathBuf::from("/tmp/base/config/settings.toml"));
    }

    #[test]
    fn test_resolve_path_with_home_expansion() {
        let home = home_dir().expect("Home directory must be resolvable for this test");
        let resolved = CdtConfig::resolve_path("/tmp", PathBuf::from("~/config.toml"));
        assert_eq!(resolved, home.join("config.toml"));
    }

    #[test]
    fn test_resolve_path_with_home_directory_only() {
        let home = home_dir().expect("Home directory must be resolvable for this test");
        let resolved = CdtConfig::resolve_path("/tmp", PathBuf::from("~"));
        assert_eq!(resolved, home);
    }

    #[test]
    fn test_resolve_path_with_trailing_home_separator() {
        let home = home_dir().expect("Home directory must be resolvable for this test");
        let resolved = CdtConfig::resolve_path("/tmp", PathBuf::from("~/"));
        assert_eq!(resolved, home);
    }

    #[test]
    fn test_resolve_path_with_windows_style_home_separator() {
        let home = home_dir().expect("Home directory must be resolvable for this test");
        let resolved = CdtConfig::resolve_path("/tmp", PathBuf::from("~\\config.toml"));
        assert_eq!(resolved, home.join("config.toml"));
    }

    #[test]
    fn test_resolve_path_normalizes_navigation_components() {
        let base = PathBuf::from("/tmp/base");
        let candidate = PathBuf::from("configs/../settings.toml");
        let resolved = CdtConfig::resolve_path(&base, candidate);
        assert_eq!(resolved, PathBuf::from("/tmp/base/settings.toml"));
    }

    #[test]
    fn test_resolve_path_preserves_relative_parent_prefix() {
        let resolved = CdtConfig::resolve_path(".", PathBuf::from("../../settings.toml"));
        assert_eq!(resolved, PathBuf::from("../../settings.toml"));
    }

    #[test]
    fn test_resolve_path_cannot_escape_root() {
        let candidate = PathBuf::from("/../etc/passwd");
        let resolved = CdtConfig::resolve_path("/tmp", candidate);
        assert_eq!(resolved, PathBuf::from("/etc/passwd"));
    }
}
