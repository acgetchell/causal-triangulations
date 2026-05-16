#![forbid(unsafe_code)]

//! Simulation result containers and post-simulation summaries.
//!
//! This module owns measurement records, complete simulation outputs, and
//! convenience analysis methods that summarize recorded measurements or inspect
//! the final triangulation state.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::MoveStatistics;
use crate::cdt::metropolis::{MetropolisConfig, MonteCarloStep};
use crate::cdt::observables::{estimate_hausdorff_dimension, estimate_spectral_dimension};
use crate::config::{CdtConfig, CdtTopology};
use crate::errors::{CdtError, CdtResult, OutputFormat};
use crate::geometry::CdtTriangulation2D;
use crate::util::usize_to_f64;
use serde::{Deserialize, Serialize};
use serde_json::to_writer_pretty;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Duration;

/// Measurement data collected during simulation.
///
/// Use [`Self::new`] and builder-style methods such as
/// [`Self::with_volume_profile`] rather than struct literals outside this
/// crate; additional measurement fields may be added over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Measurement {
    /// Monte Carlo step when measurement was taken
    pub step: u32,
    /// Current action value
    pub action: f64,
    /// Number of vertices
    pub vertices: u32,
    /// Number of edges
    pub edges: u32,
    /// Number of triangles
    pub triangles: u32,
    /// Per-slice triangle counts `N₂(t)` from
    /// [`CdtTriangulation::volume_profile`](crate::cdt::triangulation::CdtTriangulation::volume_profile).
    ///
    /// Entry `t` counts classifiable CDT triangles assigned to time slab `t`;
    /// the vector is empty when the measured triangulation has no current
    /// foliation.
    pub volume_profile: Vec<u32>,
}

impl Measurement {
    /// Creates a measurement with an empty volume profile.
    ///
    /// This constructor records scalar simulation counts. Attach per-slice
    /// volume data with [`Self::with_volume_profile`] when the measured
    /// triangulation has a foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement = Measurement::new(10, -12.5, 64, 180, 117);
    /// assert_eq!(measurement.step, 10);
    /// assert!(measurement.volume_profile.is_empty());
    /// ```
    #[must_use]
    pub const fn new(step: u32, action: f64, vertices: u32, edges: u32, triangles: u32) -> Self {
        Self {
            step,
            action,
            vertices,
            edges,
            triangles,
            volume_profile: Vec::new(),
        }
    }

    /// Returns this measurement with a per-slice volume profile attached.
    ///
    /// The profile entries are triangle counts `N₂(t)` by time slab, matching
    /// [`CdtTriangulation::volume_profile`](crate::cdt::triangulation::CdtTriangulation::volume_profile).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::Measurement;
    ///
    /// let measurement =
    ///     Measurement::new(20, -10.0, 12, 26, 12).with_volume_profile(vec![6, 6, 0]);
    /// assert_eq!(measurement.volume_profile, vec![6, 6, 0]);
    /// ```
    #[must_use]
    pub fn with_volume_profile(mut self, volume_profile: Vec<u32>) -> Self {
        self.volume_profile = volume_profile;
        self
    }
}

/// Complete output from a Metropolis-Hastings CDT simulation.
///
/// Values are produced by [`MetropolisAlgorithm::run`](crate::cdt::metropolis::MetropolisAlgorithm::run)
/// and include raw Monte Carlo steps, recorded measurements, final geometry,
/// and convenience methods for common post-simulation summaries.
///
/// Serde serialization preserves the complete result object, including the
/// final triangulation checkpoint. Use [`Self::write_summary_json`] when you
/// want an analysis-friendly JSON report with configuration, aggregate
/// statistics, step telemetry, and measurements.
#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResultsBackend {
    /// Configuration used for the simulation
    config: MetropolisConfig,
    /// Action configuration used
    action_config: ActionConfig,
    /// Metropolis-level ergodic move statistics
    move_stats: MoveStatistics,
    /// All Monte Carlo steps performed
    steps: Vec<MonteCarloStep>,
    /// Measurements taken during simulation
    measurements: Vec<Measurement>,
    /// Total simulation time
    elapsed_time: Duration,
    /// Final triangulation state
    triangulation: CdtTriangulation2D,
}

#[derive(Serialize)]
struct SimulationSummary<'a> {
    config: &'a CdtConfig,
    metropolis_config: &'a MetropolisConfig,
    action_config: &'a ActionConfig,
    move_stats: &'a MoveStatistics,
    aggregate: AggregateSummary,
    final_triangulation: TriangulationSummary,
    steps: &'a [MonteCarloStep],
    measurements: &'a [Measurement],
}

#[derive(Serialize)]
struct AggregateSummary {
    acceptance_rate: f64,
    average_action: f64,
    elapsed_time_ms: u128,
    measurement_count: usize,
    step_count: usize,
    average_volume_profile: Vec<f64>,
    volume_fluctuations: Vec<f64>,
}

#[derive(Serialize)]
struct TriangulationSummary {
    vertices: usize,
    edges: usize,
    triangles: usize,
    time_slices: u32,
    topology: CdtTopology,
}

impl SimulationResultsBackend {
    /// Creates a validated simulation result snapshot.
    ///
    /// Use this constructor for externally assembled results. Simulation runs
    /// produced by [`MetropolisAlgorithm`](crate::cdt::metropolis::MetropolisAlgorithm)
    /// use the same data shape but avoid revalidating immediately after the run has
    /// already checked its final CDT invariants.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if `config` is not a
    /// runnable Metropolis configuration, or [`CdtError::InvalidConfiguration`] if
    /// `action_config` contains non-finite couplings.
    ///
    /// The final triangulation is checked with the crate's evolved-CDT validation
    /// path, so this can also return backend validation, topology, foliation,
    /// causality, or cell-classification errors, including
    /// [`CdtError::DelaunayValidationFailed`], [`CdtError::TopologyMismatch`],
    /// [`CdtError::Foliation`], [`CdtError::CausalityViolation`], and
    /// [`CdtError::ValidationFailed`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     assert!(results.steps().is_empty());
    ///     Ok(())
    /// }
    /// ```
    pub fn new(
        config: MetropolisConfig,
        action_config: ActionConfig,
        move_stats: MoveStatistics,
        steps: Vec<MonteCarloStep>,
        measurements: Vec<Measurement>,
        elapsed_time: Duration,
        triangulation: CdtTriangulation2D,
    ) -> CdtResult<Self> {
        config.validate()?;
        action_config.validate()?;
        triangulation.validate_evolved_cdt()?;
        Ok(Self::from_parts(
            config,
            action_config,
            move_stats,
            steps,
            measurements,
            elapsed_time,
            triangulation,
        ))
    }

    /// Creates a result snapshot from components that were already validated by this crate.
    pub(crate) const fn from_parts(
        config: MetropolisConfig,
        action_config: ActionConfig,
        move_stats: MoveStatistics,
        steps: Vec<MonteCarloStep>,
        measurements: Vec<Measurement>,
        elapsed_time: Duration,
        triangulation: CdtTriangulation2D,
    ) -> Self {
        Self {
            config,
            action_config,
            move_stats,
            steps,
            measurements,
            elapsed_time,
            triangulation,
        }
    }

    /// Returns the Metropolis configuration used for this result.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = MetropolisConfig::new(1.0, 1, 0, 1);
    ///     let results = SimulationResultsBackend::new(
    ///         config.clone(),
    ///         ActionConfig::default(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         Duration::ZERO,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert_eq!(results.config(), &config);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn config(&self) -> &MetropolisConfig {
        &self.config
    }

    /// Returns the action configuration used for this result.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let action_config = ActionConfig::new(1.0, 0.0, 0.1);
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         action_config.clone(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         Duration::ZERO,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert_eq!(results.action_config(), &action_config);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn action_config(&self) -> &ActionConfig {
        &self.action_config
    }

    /// Returns Metropolis-level ergodic move statistics.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let move_stats = MoveStatistics::new();
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         move_stats,
    ///         vec![],
    ///         vec![],
    ///         Duration::ZERO,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert_eq!(results.move_stats().total_attempted(), 0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn move_stats(&self) -> &MoveStatistics {
        &self.move_stats
    }

    /// Returns recorded Monte Carlo step telemetry.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         Duration::ZERO,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert!(results.steps().is_empty());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn steps(&self) -> &[MonteCarloStep] {
        &self.steps
    }

    /// Returns recorded measurements.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         Duration::ZERO,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert!(results.measurements().is_empty());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Returns total wall-clock time recorded for the run.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let elapsed = Duration::from_millis(25);
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         elapsed,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert_eq!(results.elapsed_time(), elapsed);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn elapsed_time(&self) -> Duration {
        self.elapsed_time
    }

    /// Returns the final triangulation state.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::MoveStatistics;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         MoveStatistics::new(),
    ///         vec![],
    ///         vec![],
    ///         Duration::ZERO,
    ///         CdtTriangulation::from_cdt_strip(4, 3)?,
    ///     )?;
    ///     assert_eq!(results.triangulation().slice_sizes(), &[4, 4, 4]);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn triangulation(&self) -> &CdtTriangulation2D {
        &self.triangulation
    }

    /// Calculates the acceptance rate for the simulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let config = MetropolisConfig::new(1.0, 1, 0, 1).with_seed(7);
    ///     let results = SimulationResultsBackend::new(
    ///         config,
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     assert_relative_eq!(results.acceptance_rate(), 0.0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn acceptance_rate(&self) -> f64 {
        if self.steps.is_empty() {
            return 0.0;
        }

        let accepted_count = self.steps.iter().filter(|step| step.accepted).count();
        let total_count = self.steps.len();

        let Some(accepted_f64) = usize_to_f64(accepted_count) else {
            return 0.0;
        };
        let Some(total_f64) = usize_to_f64(total_count) else {
            return 0.0;
        };

        accepted_f64 / total_f64
    }

    /// Calculates the average action over all measurements.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let config = MetropolisConfig::new(1.0, 1, 0, 1).with_seed(7);
    ///     let results = SimulationResultsBackend::new(
    ///         config,
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     assert_relative_eq!(results.average_action(), 0.0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn average_action(&self) -> f64 {
        if self.measurements.is_empty() {
            return 0.0;
        }

        let sum: f64 = self.measurements.iter().map(|m| m.action).sum();
        let count = self.measurements.len();

        let Some(count_f64) = usize_to_f64(count) else {
            return 0.0;
        };

        sum / count_f64
    }

    /// Averages [`Measurement::volume_profile`] values after thermalization.
    ///
    /// The result has one entry per measured time slice.  Missing entries in a
    /// measurement are treated as zero, which keeps unfoliated simulations
    /// represented by an empty profile rather than a partially inferred one.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, Measurement, MetropolisConfig,
    ///     SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let config = MetropolisConfig::new(1.0, 20, 10, 5).with_seed(7);
    ///     let results = SimulationResultsBackend::new(
    ///         config,
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![
    ///             Measurement::new(0, 1.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![6, 6, 0]),
    ///             Measurement::new(10, 2.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![4, 8, 0]),
    ///         ],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     let profile = results.average_volume_profile();
    ///     assert_relative_eq!(profile[0], 4.0);
    ///     assert_relative_eq!(profile[1], 8.0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn average_volume_profile(&self) -> Vec<f64> {
        let mut measurement_count = 0_usize;
        let mut profile_len = 0_usize;
        for measurement in self.equilibrium_measurements_iter() {
            measurement_count += 1;
            profile_len = profile_len.max(measurement.volume_profile.len());
        }
        if measurement_count == 0 || profile_len == 0 {
            return Vec::new();
        }

        let mut sums = vec![0.0; profile_len];
        for measurement in self.equilibrium_measurements_iter() {
            for (index, &volume) in measurement.volume_profile.iter().enumerate() {
                sums[index] += <f64 as From<u32>>::from(volume);
            }
        }

        let Some(count) = usize_to_f64(measurement_count) else {
            return Vec::new();
        };
        sums.into_iter().map(|sum| sum / count).collect()
    }

    /// Computes per-slice standard deviations of [`Measurement::volume_profile`].
    ///
    /// The sample standard deviation is evaluated over equilibrium
    /// measurements, using the same post-thermalization selection as
    /// [`Self::average_volume_profile`].  Returns an empty vector when fewer
    /// than two equilibrium measurements are available.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, Measurement, MetropolisConfig,
    ///     SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let config = MetropolisConfig::new(1.0, 20, 10, 5).with_seed(7);
    ///     let results = SimulationResultsBackend::new(
    ///         config,
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![
    ///             Measurement::new(10, 2.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![4, 8, 0]),
    ///             Measurement::new(15, 3.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![6, 6, 0]),
    ///         ],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     let fluctuations = results.volume_fluctuations();
    ///     assert_relative_eq!(fluctuations[0], 2.0_f64.sqrt());
    ///     assert_relative_eq!(fluctuations[1], 2.0_f64.sqrt());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn volume_fluctuations(&self) -> Vec<f64> {
        let means = self.average_volume_profile();
        let n = self.equilibrium_measurements_iter().count();
        if n < 2 || means.is_empty() {
            return Vec::new();
        }

        let mut variances = vec![0.0; means.len()];
        for measurement in self.equilibrium_measurements_iter() {
            for (index, mean) in means.iter().enumerate() {
                let volume = measurement
                    .volume_profile
                    .get(index)
                    .map_or(0.0, |&volume| <f64 as From<u32>>::from(volume));
                let delta = volume - mean;
                variances[index] += delta * delta;
            }
        }

        let Some(denominator) = usize_to_f64(n - 1) else {
            return Vec::new();
        };
        variances
            .into_iter()
            .map(|variance| (variance / denominator).sqrt())
            .collect()
    }

    /// Estimates the Hausdorff dimension of the final triangulation.
    ///
    /// This is a single-state post-simulation observable computed from
    /// `self.triangulation`, the final triangulation, using dual-graph geodesic
    /// ball growth through [`estimate_hausdorff_dimension`]. It does not
    /// average over equilibrium measurements; doing so would require storing
    /// triangulation snapshots in [`Measurement`] or rerunning the chain.
    /// For ensemble-style recorded data, see [`Self::average_volume_profile`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     assert!(results
    ///         .hausdorff_dimension_estimate()
    ///         .is_some_and(f64::is_finite));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn hausdorff_dimension_estimate(&self) -> Option<f64> {
        estimate_hausdorff_dimension(&self.triangulation)
    }

    /// Estimates the spectral dimension of the final triangulation.
    ///
    /// This is a single-state post-simulation observable computed from
    /// `self.triangulation`, the final triangulation, using dual-graph
    /// diffusion return probability through [`estimate_spectral_dimension`].
    /// It does not average over equilibrium measurements; doing so would
    /// require storing triangulation snapshots in [`Measurement`] or rerunning
    /// the chain. For ensemble-style recorded data, see
    /// [`Self::average_volume_profile`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_toroidal_cdt(6, 6)?;
    ///     let results = SimulationResultsBackend::new(
    ///         MetropolisConfig::new(1.0, 1, 0, 1),
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     assert!(results
    ///         .spectral_dimension_estimate()
    ///         .is_some_and(f64::is_finite));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn spectral_dimension_estimate(&self) -> Option<f64> {
        estimate_spectral_dimension(&self.triangulation)
    }

    /// Returns measurements after thermalization.
    ///
    /// Measurements are recorded for the initial state at step 0, then after
    /// completed-move counts divisible by
    /// [`MetropolisConfig::measurement_frequency`]. This accessor defines
    /// equilibrium as `measurement.step >= thermalization_steps`, so a
    /// measurement taken exactly on the thermalization boundary is included.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtResult, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let config = MetropolisConfig::new(1.0, 2, 1, 1).with_seed(7);
    ///     let results = SimulationResultsBackend::new(
    ///         config,
    ///         ActionConfig::default(),
    ///         Default::default(),
    ///         vec![],
    ///         vec![],
    ///         Duration::from_millis(0),
    ///         tri,
    ///     )?;
    ///     assert!(results.equilibrium_measurements().is_empty());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn equilibrium_measurements(&self) -> Vec<&Measurement> {
        self.equilibrium_measurements_iter().collect()
    }

    /// Iterates over measurements after thermalization without allocating.
    fn equilibrium_measurements_iter(&self) -> impl Iterator<Item = &Measurement> {
        self.measurements
            .iter()
            .filter(|measurement| measurement.step >= self.config.thermalization_steps)
    }

    /// Writes one CSV row per recorded measurement.
    ///
    /// The CSV includes scalar measurement values plus accepted/delta-action
    /// telemetry from the Monte Carlo step with the same step number when such a
    /// step exists. Initial measurements at step 0 leave those telemetry columns
    /// blank.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::OutputWriteFailed`] if the file or a parent directory
    /// cannot be created, or if writing the CSV fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let results = MetropolisAlgorithm::new(
    ///         MetropolisConfig::new(1.0, 2, 1, 1),
    ///         ActionConfig::default(),
    ///     )
    ///     .run(tri)?;
    ///     results.write_measurements_csv("measurements.csv")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn write_measurements_csv(&self, path: impl AsRef<Path>) -> CdtResult<()> {
        let path = path.as_ref();
        ensure_parent_directory(path, OutputFormat::Csv)?;
        let file = File::create(path).map_err(|err| output_error(path, OutputFormat::Csv, err))?;
        let mut writer = BufWriter::new(file);
        writeln!(
            writer,
            "step,action,vertices,edges,triangles,accepted,delta_action"
        )
        .map_err(|err| output_error(path, OutputFormat::Csv, err))?;

        let steps_by_number: HashMap<_, _> =
            self.steps.iter().map(|step| (step.step, step)).collect();
        for measurement in &self.measurements {
            let step = steps_by_number.get(&measurement.step).copied();
            let accepted = step.map_or(String::new(), |step| step.accepted.to_string());
            let delta_action = step
                .and_then(|step| step.delta_action)
                .map_or_else(String::new, |delta| delta.to_string());
            writeln!(
                writer,
                "{},{},{},{},{},{},{}",
                measurement.step,
                measurement.action,
                measurement.vertices,
                measurement.edges,
                measurement.triangles,
                accepted,
                delta_action,
            )
            .map_err(|err| output_error(path, OutputFormat::Csv, err))?;
        }

        writer
            .flush()
            .map_err(|err| output_error(path, OutputFormat::Csv, err))
    }

    /// Writes a JSON summary for external analysis and run bookkeeping.
    ///
    /// The summary stores the top-level CLI/configuration parameters, action and
    /// Metropolis configuration, aggregate statistics, final triangulation counts,
    /// Monte Carlo step telemetry, and all measurements. The aggregate
    /// `average_action` is computed from [`Self::equilibrium_measurements`] so
    /// it excludes the initial snapshot and thermalization window.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::OutputWriteFailed`] if the file or a parent directory
    /// cannot be created, or if JSON serialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use causal_triangulations::prelude::simulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = CdtConfig {
    ///         simulate: true,
    ///         steps: 2,
    ///         thermalization_steps: 1,
    ///         measurement_frequency: 1,
    ///         ..CdtConfig::new(12, 3)
    ///     };
    ///     let results = causal_triangulations::run_simulation(&config)?;
    ///     results.write_summary_json(&config, "summary.json")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn write_summary_json(&self, config: &CdtConfig, path: impl AsRef<Path>) -> CdtResult<()> {
        let path = path.as_ref();
        ensure_parent_directory(path, OutputFormat::Json)?;
        let file = File::create(path).map_err(|err| output_error(path, OutputFormat::Json, err))?;
        let mut writer = BufWriter::new(file);
        let summary = SimulationSummary {
            config,
            metropolis_config: &self.config,
            action_config: &self.action_config,
            move_stats: &self.move_stats,
            aggregate: AggregateSummary {
                acceptance_rate: self.acceptance_rate(),
                average_action: mean_measurement_action(self.equilibrium_measurements_iter()),
                elapsed_time_ms: self.elapsed_time.as_millis(),
                measurement_count: self.measurements.len(),
                step_count: self.steps.len(),
                average_volume_profile: self.average_volume_profile(),
                volume_fluctuations: self.volume_fluctuations(),
            },
            final_triangulation: TriangulationSummary {
                vertices: self.triangulation.vertex_count(),
                edges: self.triangulation.edge_count(),
                triangles: self.triangulation.face_count(),
                time_slices: self.triangulation.time_slices(),
                topology: self.triangulation.metadata().topology,
            },
            steps: &self.steps,
            measurements: &self.measurements,
        };

        to_writer_pretty(&mut writer, &summary)
            .map_err(|err| output_error(path, OutputFormat::Json, err))?;
        writeln!(writer).map_err(|err| output_error(path, OutputFormat::Json, err))?;
        writer
            .flush()
            .map_err(|err| output_error(path, OutputFormat::Json, err))
    }
}

/// Returns the mean action across a measurement stream.
fn mean_measurement_action<'a>(measurements: impl IntoIterator<Item = &'a Measurement>) -> f64 {
    let mut sum = 0.0;
    let mut count = 0_usize;
    for measurement in measurements {
        sum += measurement.action;
        count += 1;
    }

    if count == 0 {
        return 0.0;
    }

    let Some(count) = usize_to_f64(count) else {
        return 0.0;
    };
    sum / count
}

/// Creates a parent directory for configured output paths when needed.
fn ensure_parent_directory(path: &Path, format: OutputFormat) -> CdtResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent).map_err(|err| output_error(path, format, err))?;
    }
    Ok(())
}

/// Builds a typed output error without exposing I/O dependencies in public APIs.
fn output_error(path: &Path, format: OutputFormat, err: impl Display) -> CdtError {
    CdtError::OutputWriteFailed {
        path: path.display().to_string(),
        format,
        detail: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::ergodic_moves::MoveType;
    use crate::cdt::foliation::FoliationError;
    use crate::cdt::triangulation::CdtTriangulation;
    use crate::geometry::traits::TriangulationQuery;
    use approx::assert_relative_eq;
    use serde_json::{Value, from_str};
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::thread;

    /// Builds a result container around deterministic geometry for summary-method tests.
    fn results_with(
        config: MetropolisConfig,
        steps: Vec<MonteCarloStep>,
        measurements: Vec<Measurement>,
        triangulation: CdtTriangulation2D,
    ) -> SimulationResultsBackend {
        SimulationResultsBackend {
            config,
            action_config: ActionConfig::default(),
            move_stats: MoveStatistics::new(),
            steps,
            measurements,
            elapsed_time: Duration::from_millis(100),
            triangulation,
        }
    }

    /// Asserts two equal-length floating-point slices using relative tolerance.
    fn assert_slice_relative_eq(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (&actual, &expected) in actual.iter().zip(expected) {
            assert_relative_eq!(actual, expected, epsilon = 1e-12);
        }
    }

    /// Asserts two optional estimates match without exact floating-point comparison.
    fn assert_optional_relative_eq(actual: Option<f64>, expected: Option<f64>) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => {
                assert_relative_eq!(actual, expected, epsilon = 1e-12);
            }
            (None, None) => {}
            other => panic!("expected matching optional estimates, got {other:?}"),
        }
    }

    /// Returns a unique temporary path for output-writer tests.
    fn temp_output_path(name: &str) -> PathBuf {
        let thread_name = safe_thread_name();
        env::temp_dir().join(format!(
            "causal-triangulations-{name}-{}-{}",
            process::id(),
            thread_name
        ))
    }

    /// Returns the current test thread name with path separators and
    /// reserved characters removed.
    fn safe_thread_name() -> String {
        thread::current()
            .name()
            .unwrap_or("test")
            .chars()
            .map(|ch| match ch {
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
                ch if ch.is_control() => '_',
                ch => ch,
            })
            .collect()
    }

    #[test]
    fn public_constructor_rejects_invalid_metropolis_config() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            MetropolisConfig::new(0.0, 1, 0, 1),
            ActionConfig::default(),
            MoveStatistics::new(),
            vec![],
            vec![],
            Duration::ZERO,
            triangulation,
        )
        .expect_err("zero temperature should be rejected");

        assert!(matches!(
            error,
            CdtError::InvalidSimulationConfiguration {
                ref setting,
                ref provided_value,
                ref expected,
            } if setting == "temperature"
                && provided_value == "0"
                && expected == "finite and positive"
        ));
    }

    #[test]
    fn public_constructor_rejects_invalid_action_config() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");

        let error = SimulationResultsBackend::new(
            MetropolisConfig::new(1.0, 1, 0, 1),
            ActionConfig::new(f64::NAN, 0.0, 0.0),
            MoveStatistics::new(),
            vec![],
            vec![],
            Duration::ZERO,
            triangulation,
        )
        .expect_err("non-finite action coupling should be rejected");

        assert!(matches!(
            error,
            CdtError::InvalidConfiguration {
                ref setting,
                ref provided_value,
                ref expected,
            } if setting == "coupling_0" && provided_value == "NaN" && expected == "finite"
        ));
    }

    #[test]
    fn public_constructor_rejects_stale_final_foliation() {
        let mut triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let vertex = triangulation
            .geometry()
            .vertices()
            .next()
            .expect("strip should contain vertices");
        let label = triangulation
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
            .expect("strip vertices should be labeled");
        triangulation
            .set_vertex_data(&vertex, Some(label))
            .expect("rewriting an existing label should mark foliation stale");

        let error = SimulationResultsBackend::new(
            MetropolisConfig::new(1.0, 1, 0, 1),
            ActionConfig::default(),
            MoveStatistics::new(),
            vec![],
            vec![],
            Duration::ZERO,
            triangulation,
        )
        .expect_err("stale foliation should be rejected");

        assert!(matches!(
            error,
            CdtError::Foliation(FoliationError::StaleBookkeeping { .. })
        ));
    }

    #[test]
    fn measurement_builders_preserve_scalar_counts_and_profile() {
        let measurement = Measurement::new(7, -3.5, 12, 26, 12).with_volume_profile(vec![6, 6, 0]);

        assert_eq!(measurement.step, 7);
        assert_relative_eq!(measurement.action, -3.5);
        assert_eq!(measurement.vertices, 12);
        assert_eq!(measurement.edges, 26);
        assert_eq!(measurement.triangles, 12);
        assert_eq!(measurement.volume_profile, vec![6, 6, 0]);
    }

    #[test]
    fn writes_measurements_csv_with_matching_step_telemetry() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 2, 1, 1),
            vec![MonteCarloStep {
                step: 1,
                move_type: MoveType::Move22,
                accepted: true,
                action_before: 3.0,
                action_after: Some(2.5),
                delta_action: Some(-0.5),
            }],
            vec![
                Measurement::new(0, 3.0, 12, 26, 12),
                Measurement::new(1, 2.5, 12, 26, 12),
            ],
            triangulation,
        );
        let path = temp_output_path("measurements.csv");

        results
            .write_measurements_csv(&path)
            .expect("CSV output should write");
        let csv = fs::read_to_string(&path).expect("CSV output should be readable");
        fs::remove_file(&path).expect("temporary CSV should be removable");

        assert_eq!(
            csv,
            "step,action,vertices,edges,triangles,accepted,delta_action\n\
             0,3,12,26,12,,\n\
             1,2.5,12,26,12,true,-0.5\n"
        );
    }

    #[test]
    fn writes_summary_json_with_config_and_aggregates() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 1, 0, 1),
            vec![MonteCarloStep {
                step: 1,
                move_type: MoveType::Move22,
                accepted: true,
                action_before: 3.0,
                action_after: Some(2.5),
                delta_action: Some(-0.5),
            }],
            vec![Measurement::new(1, 2.5, 12, 26, 12)],
            triangulation,
        );
        let config = CdtConfig {
            steps: 1,
            thermalization_steps: 0,
            measurement_frequency: 1,
            simulate: true,
            ..CdtConfig::new(12, 3)
        };
        let path = temp_output_path("summary.json");

        results
            .write_summary_json(&config, &path)
            .expect("JSON output should write");
        let json = fs::read_to_string(&path).expect("JSON output should be readable");
        fs::remove_file(&path).expect("temporary JSON should be removable");
        let parsed: Value = from_str(&json).expect("summary should be valid JSON");

        assert_eq!(parsed["config"]["vertices"], 12);
        assert_eq!(parsed["aggregate"]["measurement_count"], 1);
        assert_eq!(parsed["aggregate"]["step_count"], 1);
        assert_eq!(parsed["final_triangulation"]["time_slices"], 3);
        assert_eq!(parsed["measurements"][0]["step"], 1);
    }

    #[test]
    fn output_writers_reject_file_parent() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );
        let parent_file = temp_output_path("not-a-directory");
        fs::write(&parent_file, b"not a directory").expect("parent fixture file should write");

        let csv_path = parent_file.join("measurements.csv");
        let csv_error = results
            .write_measurements_csv(&csv_path)
            .expect_err("CSV writer should reject a parent path that is a file");
        let CdtError::OutputWriteFailed {
            path,
            format,
            detail,
        } = csv_error
        else {
            panic!("expected CSV output write failure, got {csv_error:?}");
        };
        assert_eq!(format, OutputFormat::Csv);
        assert_eq!(path, csv_path.display().to_string());
        assert!(!detail.is_empty());

        let json_path = parent_file.join("summary.json");
        let config = CdtConfig::new(12, 3);
        let json_error = results
            .write_summary_json(&config, &json_path)
            .expect_err("JSON writer should reject a parent path that is a file");
        let CdtError::OutputWriteFailed {
            path,
            format,
            detail,
        } = json_error
        else {
            panic!("expected JSON output write failure, got {json_error:?}");
        };
        assert_eq!(format, OutputFormat::Json);
        assert_eq!(path, json_path.display().to_string());
        assert!(!detail.is_empty());

        fs::remove_file(&parent_file).expect("parent fixture file should be removable");
    }

    #[test]
    fn summary_json_average_action_uses_equilibrium_measurements() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 2, 1, 1),
            vec![],
            vec![
                Measurement::new(0, 100.0, 12, 26, 12),
                Measurement::new(1, 4.0, 12, 26, 12),
                Measurement::new(2, 6.0, 12, 26, 12),
            ],
            triangulation,
        );
        let config = CdtConfig {
            steps: 2,
            thermalization_steps: 1,
            measurement_frequency: 1,
            simulate: true,
            ..CdtConfig::new(12, 3)
        };
        let path = temp_output_path("equilibrium-summary.json");

        results
            .write_summary_json(&config, &path)
            .expect("JSON output should write");
        let json = fs::read_to_string(&path).expect("JSON output should be readable");
        fs::remove_file(&path).expect("temporary JSON should be removable");
        let parsed: Value = from_str(&json).expect("summary should be valid JSON");

        assert_relative_eq!(
            parsed["aggregate"]["average_action"]
                .as_f64()
                .expect("average action should be numeric"),
            5.0
        );
    }

    #[test]
    fn summaries_use_post_thermalization_measurements() {
        let config = MetropolisConfig::new(1.0, 20, 10, 5);
        let steps = vec![
            MonteCarloStep {
                step: 1,
                move_type: MoveType::Move22,
                accepted: true,
                action_before: 3.0,
                action_after: Some(2.5),
                delta_action: Some(-0.5),
            },
            MonteCarloStep {
                step: 2,
                move_type: MoveType::Move13Add,
                accepted: false,
                action_before: 2.5,
                action_after: None,
                delta_action: Some(0.8),
            },
            MonteCarloStep {
                step: 3,
                move_type: MoveType::Move31Remove,
                accepted: true,
                action_before: 2.5,
                action_after: Some(2.0),
                delta_action: Some(-0.5),
            },
        ];
        let measurements = vec![
            Measurement::new(0, 1.0, 3, 3, 1).with_volume_profile(vec![1, 0, 0]),
            Measurement::new(10, 2.0, 4, 5, 2).with_volume_profile(vec![1, 1, 0]),
            Measurement::new(15, 3.0, 5, 7, 3).with_volume_profile(vec![1, 2, 0]),
        ];
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(config, steps, measurements, triangulation);

        assert_relative_eq!(results.acceptance_rate(), 2.0 / 3.0);
        assert_relative_eq!(results.average_action(), 2.0);
        assert_slice_relative_eq(&results.average_volume_profile(), &[1.0, 1.5, 0.0]);
        assert_slice_relative_eq(&results.volume_fluctuations(), &[0.0, 0.5_f64.sqrt(), 0.0]);

        let equilibrium = results.equilibrium_measurements();
        assert_eq!(equilibrium.len(), 2);
        assert_eq!(equilibrium[0].step, 10);
        assert_eq!(equilibrium[1].step, 15);
    }

    #[test]
    fn volume_observables_treat_missing_profile_entries_as_zero() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 20, 10, 5),
            vec![],
            vec![
                Measurement::new(10, 2.0, 4, 5, 2).with_volume_profile(vec![4, 8, 1]),
                Measurement::new(15, 3.0, 5, 7, 3).with_volume_profile(vec![6]),
            ],
            triangulation,
        );

        assert_slice_relative_eq(&results.average_volume_profile(), &[5.0, 4.0, 0.5]);
        assert_slice_relative_eq(
            &results.volume_fluctuations(),
            &[2.0_f64.sqrt(), 32.0_f64.sqrt(), 0.5_f64.sqrt()],
        );
    }

    #[test]
    fn volume_observables_are_empty_when_profiles_are_empty() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 20, 10, 5),
            vec![],
            vec![
                Measurement::new(10, 2.0, 4, 5, 2),
                Measurement::new(15, 3.0, 5, 7, 3),
            ],
            triangulation,
        );

        assert!(results.average_volume_profile().is_empty());
        assert!(results.volume_fluctuations().is_empty());
    }

    #[test]
    fn volume_fluctuations_are_empty_for_single_equilibrium_measurement() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 20, 10, 5),
            vec![],
            vec![
                Measurement::new(0, 1.0, 3, 3, 1).with_volume_profile(vec![1]),
                Measurement::new(10, 2.0, 4, 5, 2).with_volume_profile(vec![2]),
            ],
            triangulation,
        );

        assert_slice_relative_eq(&results.average_volume_profile(), &[2.0]);
        assert!(results.volume_fluctuations().is_empty());
    }

    #[test]
    fn summaries_are_empty_for_no_steps_or_measurements() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay strip should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 20, 10, 5),
            vec![],
            vec![],
            triangulation,
        );

        assert_relative_eq!(results.acceptance_rate(), 0.0);
        assert_relative_eq!(results.average_action(), 0.0);
        assert!(results.equilibrium_measurements().is_empty());
        assert!(results.average_volume_profile().is_empty());
        assert!(results.volume_fluctuations().is_empty());
    }

    #[test]
    fn dimension_estimates_delegate_to_final_triangulation() {
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(6, 6).expect("periodic torus should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );

        assert_optional_relative_eq(
            results.hausdorff_dimension_estimate(),
            estimate_hausdorff_dimension(results.triangulation()),
        );
        assert_optional_relative_eq(
            results.spectral_dimension_estimate(),
            estimate_spectral_dimension(results.triangulation()),
        );
    }

    #[test]
    fn dimension_estimates_return_none_for_tiny_final_triangulation() {
        let triangulation = CdtTriangulation::from_seeded_points(3, 1, 2, 53)
            .expect("seeded triangle should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );

        assert!(results.hausdorff_dimension_estimate().is_none());
        assert!(results.spectral_dimension_estimate().is_none());
    }
}
