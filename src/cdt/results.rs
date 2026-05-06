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
use crate::geometry::CdtTriangulation2D;
use num_traits::cast::NumCast;
use std::time::Duration;

/// Measurement data collected during simulation.
///
/// Use [`Self::new`] and builder-style methods such as
/// [`Self::with_volume_profile`] rather than struct literals outside this
/// crate; additional measurement fields may be added over time.
#[derive(Debug, Clone)]
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
#[derive(Debug)]
pub struct SimulationResultsBackend {
    /// Configuration used for the simulation
    pub config: MetropolisConfig,
    /// Action configuration used
    pub action_config: ActionConfig,
    /// Metropolis-level ergodic move statistics
    pub move_stats: MoveStatistics,
    /// All Monte Carlo steps performed
    pub steps: Vec<MonteCarloStep>,
    /// Measurements taken during simulation
    pub measurements: Vec<Measurement>,
    /// Total simulation time
    pub elapsed_time: Duration,
    /// Final triangulation state
    pub triangulation: CdtTriangulation2D,
}

impl SimulationResultsBackend {
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
    ///     let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53)?;
    ///     let config = MetropolisConfig::new(1.0, 1, 0, 1).with_seed(7);
    ///     let results = SimulationResultsBackend {
    ///         config,
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
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

        let accepted_f64 = NumCast::from(accepted_count).unwrap_or(0.0);
        let total_f64 = NumCast::from(total_count).unwrap_or(1.0);

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
    ///     let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53)?;
    ///     let config = MetropolisConfig::new(1.0, 1, 0, 1).with_seed(7);
    ///     let results = SimulationResultsBackend {
    ///         config,
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
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

        let count_f64 = NumCast::from(count).unwrap_or(1.0);

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
    ///     let results = SimulationResultsBackend {
    ///         config,
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![
    ///             Measurement::new(0, 1.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![6, 6, 0]),
    ///             Measurement::new(10, 2.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![4, 8, 0]),
    ///         ],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
    ///     let profile = results.average_volume_profile();
    ///     assert_relative_eq!(profile[0], 4.0);
    ///     assert_relative_eq!(profile[1], 8.0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn average_volume_profile(&self) -> Vec<f64> {
        let measurements = self.equilibrium_measurements();
        let profile_len = measurements
            .iter()
            .map(|measurement| measurement.volume_profile.len())
            .max()
            .unwrap_or(0);
        if measurements.is_empty() || profile_len == 0 {
            return Vec::new();
        }

        let mut sums = vec![0.0; profile_len];
        for measurement in &measurements {
            for (index, &volume) in measurement.volume_profile.iter().enumerate() {
                let volume: f64 = volume.into();
                sums[index] += volume;
            }
        }

        let count = NumCast::from(measurements.len()).unwrap_or(1.0);
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
    ///     let results = SimulationResultsBackend {
    ///         config,
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![
    ///             Measurement::new(10, 2.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![4, 8, 0]),
    ///             Measurement::new(15, 3.0, 12, 26, 12)
    ///                 .with_volume_profile(vec![6, 6, 0]),
    ///         ],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
    ///     let fluctuations = results.volume_fluctuations();
    ///     assert_relative_eq!(fluctuations[0], 2.0_f64.sqrt());
    ///     assert_relative_eq!(fluctuations[1], 2.0_f64.sqrt());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn volume_fluctuations(&self) -> Vec<f64> {
        let measurements = self.equilibrium_measurements();
        let means = self.average_volume_profile();
        let n = measurements.len();
        if n < 2 || means.is_empty() {
            return Vec::new();
        }

        let mut variances = vec![0.0; means.len()];
        for measurement in &measurements {
            for (index, mean) in means.iter().enumerate() {
                let volume = measurement
                    .volume_profile
                    .get(index)
                    .map_or(0.0, |&volume| {
                        let volume: f64 = volume.into();
                        volume
                    });
                let delta = volume - mean;
                variances[index] += delta * delta;
            }
        }

        let denominator = NumCast::from(n - 1).unwrap_or(1.0);
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
    ///     let results = SimulationResultsBackend {
    ///         config: MetropolisConfig::new(1.0, 1, 0, 1),
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
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
    ///     let results = SimulationResultsBackend {
    ///         config: MetropolisConfig::new(1.0, 1, 0, 1),
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
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
    ///     let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53)?;
    ///     let config = MetropolisConfig::new(1.0, 2, 1, 1).with_seed(7);
    ///     let results = SimulationResultsBackend {
    ///         config,
    ///         action_config: ActionConfig::default(),
    ///         move_stats: Default::default(),
    ///         steps: vec![],
    ///         measurements: vec![],
    ///         elapsed_time: Duration::from_millis(0),
    ///         triangulation: tri,
    ///     };
    ///     assert!(results.equilibrium_measurements().is_empty());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn equilibrium_measurements(&self) -> Vec<&Measurement> {
        self.measurements
            .iter()
            .filter(|m| m.step >= self.config.thermalization_steps)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::ergodic_moves::MoveType;
    use crate::cdt::triangulation::CdtTriangulation;
    use approx::assert_relative_eq;

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
            CdtTriangulation::from_cdt_strip(4, 3).expect("explicit strip should build");
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
            CdtTriangulation::from_cdt_strip(4, 3).expect("explicit strip should build");
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
            CdtTriangulation::from_cdt_strip(4, 3).expect("explicit strip should build");
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
            CdtTriangulation::from_cdt_strip(4, 3).expect("explicit strip should build");
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
            CdtTriangulation::from_cdt_strip(4, 3).expect("explicit strip should build");
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
            CdtTriangulation::from_toroidal_cdt(6, 6).expect("explicit torus should build");
        let results = results_with(
            MetropolisConfig::new(1.0, 1, 0, 1),
            vec![],
            vec![],
            triangulation,
        );

        assert_optional_relative_eq(
            results.hausdorff_dimension_estimate(),
            estimate_hausdorff_dimension(&results.triangulation),
        );
        assert_optional_relative_eq(
            results.spectral_dimension_estimate(),
            estimate_spectral_dimension(&results.triangulation),
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
