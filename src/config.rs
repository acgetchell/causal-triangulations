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
use crate::errors::{CdtError, CdtResult};
use clap::{Parser, ValueEnum};
use dirs::home_dir;
use std::path::{Component, Path, PathBuf};

/// Topology of the spatial slices in the CDT triangulation.
///
/// Determines the boundary conditions for the simulation:
/// - [`OpenBoundary`](Self::OpenBoundary) — finite strip with boundary (χ = 1)
/// - [`Toroidal`](Self::Toroidal) — periodic in both space and time (S¹×S¹, χ = 0)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum CdtTopology {
    /// Finite strip with open boundaries (Euler characteristic χ = 1).
    ///
    /// This is the current default while toroidal construction is blocked
    /// on [delaunay#313](https://github.com/acgetchell/delaunay/issues/313).
    #[default]
    OpenBoundary,
    /// Periodic in both space and time, forming a torus S¹×S¹ (χ = 0).
    ///
    /// Blocked on [delaunay#313](https://github.com/acgetchell/delaunay/issues/313);
    /// will become the default once implemented.
    Toroidal,
}

/// Main configuration structure for CDT simulations.
///
/// This combines all configuration options for the CDT simulation,
/// including triangulation generation, action calculation, and
/// Metropolis algorithm parameters.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct CdtConfig {
    /// Dimensionality of the triangulation
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(2..4))]
    pub dimension: Option<u8>,

    /// Number of vertices in the initial triangulation
    #[arg(short, long, value_parser = clap::value_parser!(u32).range(3..))]
    pub vertices: u32,

    /// Number of timeslices in the triangulation
    #[arg(short, long, value_parser = clap::value_parser!(u32).range(1..))]
    pub timeslices: u32,

    /// Temperature for Metropolis algorithm
    #[arg(long, default_value = "1.0")]
    pub temperature: f64,

    /// Number of Monte Carlo steps to execute
    #[arg(long, default_value = "1000")]
    pub steps: u32,

    /// Number of thermalization steps (before measurements begin)
    #[arg(long, default_value = "100")]
    pub thermalization_steps: u32,

    /// Measurement frequency (take measurement every N steps)
    #[arg(long, default_value = "10", value_parser = clap::value_parser!(u32).range(1..))]
    pub measurement_frequency: u32,

    /// Coupling constant κ₀ for vertices in the action
    #[arg(long, default_value = "1.0")]
    pub coupling_0: f64,

    /// Coupling constant κ₂ for triangles in the action
    #[arg(long, default_value = "1.0")]
    pub coupling_2: f64,

    /// Cosmological constant λ in the action
    #[arg(long, default_value = "0.1")]
    pub cosmological_constant: f64,

    /// Run full CDT simulation (default: true; disable to only generate triangulation)
    #[arg(long, default_value_t = true)]
    pub simulate: bool,

    /// Optional RNG seed for reproducible simulations
    #[arg(long)]
    pub seed: Option<u64>,

    /// Topology of spatial slices
    #[arg(long, value_enum, default_value_t = CdtTopology::default())]
    pub topology: CdtTopology,
}

/// Controls how dimension overrides are applied when merging configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Default, Clone, Copy)]
pub struct CdtConfigOverrides {
    /// Optional override for the triangulation dimension.
    pub dimension: Option<DimensionOverride>,
    /// Optional override for the vertex count.
    pub vertices: Option<u32>,
    /// Optional override for the timeslice count.
    pub timeslices: Option<u32>,
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
}

impl CdtConfig {
    /// Merges this configuration with a set of override values, returning a new configuration.
    ///
    /// Override fields that are `None` are ignored, leaving the original configuration values
    /// unchanged. When an override value is provided, it replaces the corresponding field in
    /// the returned configuration.
    #[must_use]
    pub fn merge_with_override(&self, overrides: &CdtConfigOverrides) -> Self {
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

        merged
    }

    /// Resolves a candidate path against a base directory, expanding user home references
    /// and normalizing relative segments (e.g., `.` and `..`).
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

fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Don't pop if the path is empty or we're at the root
                let mut components = normalized.components();
                let at_root = components.next().is_some_and(|first| {
                    components.next().is_none()
                        && matches!(first, Component::RootDir | Component::Prefix(_))
                });

                if !normalized.as_os_str().is_empty() && !at_root {
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

fn invalid_configuration(
    setting: &str,
    provided_value: &impl std::fmt::Display,
    expected: &impl std::fmt::Display,
) -> CdtError {
    invalid_configuration_from_parts(setting, provided_value.to_string(), expected.to_string())
}

fn invalid_configuration_from_parts(
    setting: &str,
    provided_value: String,
    expected: String,
) -> CdtError {
    CdtError::InvalidConfiguration {
        setting: setting.to_string(),
        provided_value,
        expected,
    }
}

pub(crate) fn validate_simulation_settings(
    temperature: f64,
    steps: u32,
    thermalization_steps: u32,
    measurement_frequency: u32,
    mut error_for: impl FnMut(&str, String, String) -> CdtError,
) -> CdtResult<()> {
    let mut invalid = |setting: &str, provided_value: String, expected: String| {
        Err(error_for(setting, provided_value, expected))
    };

    if !temperature.is_finite() || temperature <= 0.0 {
        return invalid(
            "temperature",
            temperature.to_string(),
            "finite and positive".to_string(),
        );
    }

    if steps == 0 {
        return invalid("steps", steps.to_string(), "≥ 1".to_string());
    }

    if measurement_frequency == 0 {
        return invalid(
            "measurement_frequency",
            measurement_frequency.to_string(),
            "≥ 1".to_string(),
        );
    }

    if measurement_frequency > steps {
        return invalid(
            "measurement_frequency",
            measurement_frequency.to_string(),
            format!("≤ steps ({steps})"),
        );
    }

    if thermalization_steps > steps {
        return invalid(
            "thermalization_steps",
            thermalization_steps.to_string(),
            format!("≤ steps ({steps})"),
        );
    }

    let first_post_thermalization_measurement = u64::from(thermalization_steps)
        .div_ceil(u64::from(measurement_frequency))
        * u64::from(measurement_frequency);

    if first_post_thermalization_measurement > u64::from(steps) {
        return invalid(
            "measurement schedule",
            format!(
                "steps={steps}, thermalization_steps={thermalization_steps}, measurement_frequency={measurement_frequency}"
            ),
            "at least one post-thermalization measurement".to_string(),
        );
    }

    Ok(())
}

impl CdtConfig {
    /// Builds a new instance of `CdtConfig` from command line arguments.
    #[must_use]
    pub fn from_args() -> Self {
        Self::parse()
    }

    /// Creates a new `CdtConfig` with specified basic parameters and default action parameters.
    #[must_use]
    pub const fn new(vertices: u32, timeslices: u32) -> Self {
        Self {
            dimension: Some(2),
            vertices,
            timeslices,
            temperature: 1.0,
            steps: 1000,
            thermalization_steps: 100,
            measurement_frequency: 10,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: true,
            seed: None,
            topology: CdtTopology::OpenBoundary,
        }
    }

    /// Creates a `MetropolisConfig` from this configuration.
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
    #[must_use]
    pub const fn to_action_config(&self) -> ActionConfig {
        ActionConfig::new(self.coupling_0, self.coupling_2, self.cosmological_constant)
    }

    /// Gets the effective dimension (defaults to 2 if not specified).
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
    /// Returns a structured error describing the invalid configuration entry.
    pub fn validate(&self) -> CdtResult<()> {
        if self.vertices < 3 {
            return Err(invalid_configuration("vertices", &self.vertices, &"≥ 3"));
        }

        if self.timeslices == 0 {
            return Err(invalid_configuration(
                "timeslices",
                &self.timeslices,
                &"≥ 1",
            ));
        }

        if let Some(dim) = self.dimension
            && !(2..=3).contains(&dim)
        {
            return Err(invalid_configuration("dimension", &dim, &"2 or 3"));
        }

        validate_simulation_settings(
            self.temperature,
            self.steps,
            self.thermalization_steps,
            self.measurement_frequency,
            |setting, provided_value, expected| {
                invalid_configuration_from_parts(setting, provided_value, expected)
            },
        )
    }
}

/// Configuration preset for quick testing.
#[derive(Debug, Clone)]
pub struct TestConfig;

impl TestConfig {
    /// Creates a small, fast configuration suitable for unit tests.
    #[must_use]
    pub const fn small() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 16,
            timeslices: 2,
            temperature: 1.0,
            steps: 10,
            thermalization_steps: 2,
            measurement_frequency: 2,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: true,
            seed: None,
            topology: CdtTopology::OpenBoundary,
        }
    }

    /// Creates a medium-sized configuration for integration tests.
    #[must_use]
    pub const fn medium() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 64,
            timeslices: 4,
            temperature: 1.0,
            steps: 100,
            thermalization_steps: 20,
            measurement_frequency: 5,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: true,
            seed: None,
            topology: CdtTopology::OpenBoundary,
        }
    }

    /// Creates a large configuration for performance testing.
    #[must_use]
    pub const fn large() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 256,
            timeslices: 8,
            temperature: 1.0,
            steps: 1000,
            thermalization_steps: 100,
            measurement_frequency: 10,
            coupling_0: 1.0,
            coupling_2: 1.0,
            cosmological_constant: 0.1,
            simulate: true,
            seed: None,
            topology: CdtTopology::OpenBoundary,
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
        let config = CdtConfig::new(32, 3);
        assert_eq!(config.vertices, 32);
        assert_eq!(config.timeslices, 3);
        assert_eq!(config.dimension(), 2);
        assert!(config.simulate);
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
        let valid_config = CdtConfig::new(32, 3);
        assert!(valid_config.validate().is_ok());

        let invalid_vertices = CdtConfig {
            vertices: 2,
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            invalid_vertices.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "vertices" && provided_value == "2" && expected == "≥ 3"
        ));

        let invalid_timeslices = CdtConfig {
            timeslices: 0,
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            invalid_timeslices.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "timeslices" && provided_value == "0" && expected == "≥ 1"
        ));

        let invalid_temperature = CdtConfig {
            temperature: -1.0,
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            invalid_temperature.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "temperature"
                && provided_value == "-1"
                && expected == "finite and positive"
        ));

        let invalid_measurement_frequency = CdtConfig {
            measurement_frequency: 0,
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            invalid_measurement_frequency.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "measurement_frequency"
                && provided_value == "0"
                && expected == "≥ 1"
        ));

        let invalid_steps = CdtConfig {
            steps: 0,
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            invalid_steps.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "steps" && provided_value == "0" && expected == "≥ 1"
        ));

        let invalid_dimension = CdtConfig {
            dimension: Some(4),
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            invalid_dimension.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "dimension" && provided_value == "4" && expected == "2 or 3"
        ));

        let measurement_frequency_exceeds_steps = CdtConfig {
            measurement_frequency: 2_000,
            ..CdtConfig::new(32, 3)
        };
        assert!(matches!(
            measurement_frequency_exceeds_steps.validate(),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) if setting == "measurement_frequency"
                && provided_value == "2000"
                && expected == "≤ steps (1000)"
        ));

        let boundary_aligned_measurement = CdtConfig {
            steps: 11,
            thermalization_steps: 10,
            measurement_frequency: 10,
            ..CdtConfig::new(32, 3)
        };
        assert!(
            boundary_aligned_measurement.validate().is_ok(),
            "Configurations where thermalization ends on a measurement boundary should pass validation"
        );

        let boundary_aligned_final_measurement = CdtConfig {
            steps: 10,
            thermalization_steps: 10,
            measurement_frequency: 5,
            ..CdtConfig::new(32, 3)
        };
        assert!(
            boundary_aligned_final_measurement.validate().is_ok(),
            "Configurations with a final-step post-thermalization measurement should pass validation"
        );

        let final_step_measurement = CdtConfig {
            steps: 20,
            thermalization_steps: 15,
            measurement_frequency: 10,
            ..CdtConfig::new(32, 3)
        };
        assert!(
            final_step_measurement.validate().is_ok(),
            "A measurement taken exactly at the final completed step should satisfy the schedule"
        );

        let insufficient_measurements = CdtConfig {
            steps: 19,
            thermalization_steps: 15,
            measurement_frequency: 10,
            ..CdtConfig::new(32, 3)
        };
        match insufficient_measurements.validate() {
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) => {
                assert_eq!(setting, "measurement schedule");
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

        let overflowed_post_thermalization_boundary = CdtConfig {
            steps: u32::MAX,
            thermalization_steps: u32::MAX,
            measurement_frequency: 2,
            ..CdtConfig::new(32, 3)
        };
        match overflowed_post_thermalization_boundary.validate() {
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            }) => {
                assert_eq!(setting, "measurement schedule");
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
    fn test_dimension_defaults_to_two_when_unspecified() {
        let config = CdtConfig {
            dimension: None,
            ..CdtConfig::new(32, 3)
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
            dimension: Some(DimensionOverride::Value(3)),
            vertices: Some(42),
            temperature: Some(2.5),
            simulate: Some(false),
            ..CdtConfigOverrides::default()
        };

        let merged = base.merge_with_override(&overrides);

        assert_eq!(merged.dimension(), 3);
        assert_eq!(merged.vertices, 42);
        assert_relative_eq!(merged.temperature, 2.5);
        assert!(!merged.simulate);

        // Unspecified fields should remain unchanged.
        assert_eq!(merged.timeslices, base.timeslices);
        assert_eq!(merged.steps, base.steps);
    }

    #[test]
    fn test_merge_with_override_can_clear_dimension() {
        let base = CdtConfig::new(10, 2);
        let overrides = CdtConfigOverrides {
            dimension: Some(DimensionOverride::Clear),
            ..CdtConfigOverrides::default()
        };

        let merged = base.merge_with_override(&overrides);
        assert_eq!(merged.dimension, None);
        assert_eq!(merged.dimension(), 2); // dimension() defaults to 2 when None
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
    fn test_resolve_path_normalizes_navigation_components() {
        let base = PathBuf::from("/tmp/base");
        let candidate = PathBuf::from("configs/../settings.toml");
        let resolved = CdtConfig::resolve_path(&base, candidate);
        assert_eq!(resolved, PathBuf::from("/tmp/base/settings.toml"));
    }

    #[test]
    fn test_resolve_path_cannot_escape_root() {
        let candidate = PathBuf::from("/../etc/passwd");
        let resolved = CdtConfig::resolve_path("/tmp", candidate);
        assert_eq!(resolved, PathBuf::from("/etc/passwd"));
    }
}
