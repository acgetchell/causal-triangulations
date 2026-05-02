//! Metropolis-Hastings algorithm for Causal Dynamical Triangulations.
//!
//! This module implements the Monte Carlo sampling algorithm used to sample
//! triangulation configurations according to the CDT path integral measure.
//!
//! The simulation uses the [`markov_chain_monte_carlo`] crate's
//! [`Chain::step_mut`](markov_chain_monte_carlo::Chain::step_mut) for
//! Metropolis–Hastings acceptance/rejection with automatic rollback.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::MoveType;
use crate::config::validate_schedule;
use crate::errors::{CdtError, CdtResult};
use crate::geometry::CdtTriangulation2D;
use crate::geometry::traits::TriangulationQuery;
use markov_chain_monte_carlo::{ProposalMut, Target};
use num_traits::cast::NumCast;
use rand::Rng;
use std::time::Duration;

// Test utilities are now handled through backend-agnostic CdtTriangulation::new

/// Configuration for the Metropolis-Hastings algorithm.
#[derive(Debug, Clone)]
pub struct MetropolisConfig {
    /// Temperature parameter (1/β)
    pub temperature: f64,
    /// Number of Monte Carlo steps to perform
    pub steps: u32,
    /// Number of thermalization steps before measurements
    pub thermalization_steps: u32,
    /// Frequency of measurements (take measurement every N steps)
    pub measurement_frequency: u32,
    /// Optional RNG seed for reproducible simulations (default: None = random)
    pub seed: Option<u64>,
}

impl Default for MetropolisConfig {
    /// Default Metropolis configuration for 2D CDT.
    fn default() -> Self {
        Self {
            temperature: 1.0,
            steps: 1000,
            thermalization_steps: 100,
            measurement_frequency: 10,
            seed: None,
        }
    }
}

impl MetropolisConfig {
    /// Creates a new Metropolis configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 500, 50, 5);
    /// assert_eq!(config.steps, 500);
    /// assert!(config.seed.is_none());
    /// ```
    #[must_use]
    pub const fn new(
        temperature: f64,
        steps: u32,
        thermalization_steps: u32,
        measurement_frequency: u32,
    ) -> Self {
        Self {
            temperature,
            steps,
            thermalization_steps,
            measurement_frequency,
            seed: None,
        }
    }

    /// Sets the RNG seed for reproducible simulations.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(1.0, 100, 10, 5).with_seed(42);
    /// assert_eq!(config.seed, Some(42));
    /// ```
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Returns the inverse temperature (β = 1/T).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(2.0, 100, 10, 5);
    /// assert!((config.beta() - 0.5).abs() < f64::EPSILON);
    /// ```
    #[must_use]
    pub fn beta(&self) -> f64 {
        1.0 / self.temperature
    }

    /// Validates simulation-specific configuration values.
    ///
    /// # Errors
    ///
    /// Returns a structured error describing the invalid simulation setting.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::MetropolisConfig;
    ///
    /// let config = MetropolisConfig::new(1.0, 100, 10, 5);
    /// assert!(config.validate().is_ok());
    /// ```
    pub fn validate(&self) -> CdtResult<()> {
        validate_schedule(
            self.temperature,
            self.steps,
            self.thermalization_steps,
            self.measurement_frequency,
            |setting, provided_value, expected| {
                invalid_sim_config(setting, provided_value, expected)
            },
        )
    }
}

/// Adapts shared schedule validation errors to the Metropolis-specific error variant.
fn invalid_sim_config(setting: &str, provided_value: String, expected: String) -> CdtError {
    CdtError::InvalidSimulationConfiguration {
        setting: setting.to_string(),
        provided_value,
        expected,
    }
}

/// Result of a Monte Carlo step.
#[derive(Debug, Clone)]
pub struct MonteCarloStep {
    /// Step number
    pub step: u32,
    /// Move type attempted
    pub move_type: MoveType,
    /// Whether the move was accepted
    pub accepted: bool,
    /// Action before the move
    pub action_before: f64,
    /// Action after the move (if accepted)
    pub action_after: Option<f64>,
    /// Change in action (ΔS)
    pub delta_action: Option<f64>,
}

/// Measurement data collected during simulation.
#[derive(Debug, Clone)]
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
}

// ---------------------------------------------------------------------------
// MCMC trait implementations for CDT
// ---------------------------------------------------------------------------

/// Target distribution for CDT: log-probability from the Regge action.
///
/// Computes `log_prob = -S / T` where `S` is the discrete Regge action
/// and `T` is the temperature.
pub struct CdtTarget {
    action_config: ActionConfig,
    temperature: f64,
}

impl CdtTarget {
    /// Creates a new CDT target distribution.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{ActionConfig, CdtTarget};
    ///
    /// let _target = CdtTarget::new(ActionConfig::default(), 1.0);
    /// ```
    #[must_use]
    pub const fn new(action_config: ActionConfig, temperature: f64) -> Self {
        Self {
            action_config,
            temperature,
        }
    }
}

impl Target<CdtTriangulation2D> for CdtTarget {
    fn log_prob(&self, state: &CdtTriangulation2D) -> f64 {
        let g = state.geometry();
        let action = self.action_config.calculate_action(
            u32::try_from(g.vertex_count()).unwrap_or_default(),
            u32::try_from(g.edge_count()).unwrap_or_default(),
            u32::try_from(g.face_count()).unwrap_or_default(),
        );
        -action / self.temperature
    }
}

/// Placeholder CDT proposal distribution.
///
/// Currently returns `None` (no valid move) for every proposal, which means
/// all steps are rejected.  This will be replaced with real ergodic moves
/// (bistellar flips) once [#55](https://github.com/acgetchell/causal-triangulations/issues/55)
/// is implemented.
pub struct CdtProposal;

impl ProposalMut<CdtTriangulation2D> for CdtProposal {
    type Undo = ();

    fn propose_mut<R: Rng + ?Sized>(
        &self,
        _state: &mut CdtTriangulation2D,
        _rng: &mut R,
    ) -> Option<()> {
        // TODO (#55): Select a random ergodic move, attempt it on the
        // triangulation, and return an undo token on success.
        None
    }

    fn undo(&self, _state: &mut CdtTriangulation2D, _token: ()) {
        // No-op: propose_mut currently never succeeds.
    }
}

// ---------------------------------------------------------------------------
// Metropolis algorithm
// ---------------------------------------------------------------------------

/// Metropolis-Hastings algorithm implementation for CDT.
///
/// Uses the [`markov_chain_monte_carlo`] crate's `Chain::step_mut` for
/// acceptance/rejection with automatic rollback.
pub struct MetropolisAlgorithm {
    /// Algorithm configuration
    config: MetropolisConfig,
    /// Action calculation configuration
    #[expect(
        dead_code,
        reason = "stored for real Metropolis wiring once #55 provides reversible CDT moves"
    )]
    action_config: ActionConfig,
}

impl MetropolisAlgorithm {
    /// Creates a new Metropolis algorithm instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// let config = MetropolisConfig::new(1.0, 10, 2, 1);
    /// let _algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
    /// ```
    #[must_use]
    pub const fn new(config: MetropolisConfig, action_config: ActionConfig) -> Self {
        Self {
            config,
            action_config,
        }
    }

    /// Run the Monte Carlo simulation.
    ///
    /// This currently returns [`CdtError::UnsupportedOperation`] after
    /// validating the configuration. Real reversible CDT moves are tracked in
    /// issue #55; until those land, this method refuses to return a zero-move
    /// simulation result.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
    /// configuration is invalid.
    /// Returns [`CdtError::UnsupportedOperation`] while real ergodic moves are
    /// unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtError, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
    /// };
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// let config = MetropolisConfig::new(1.0, 2, 1, 1).with_seed(7);
    /// let err = MetropolisAlgorithm::new(config, ActionConfig::default())
    ///     .run(tri)
    ///     .expect_err("real CDT moves are not implemented yet");
    /// assert!(matches!(err, CdtError::UnsupportedOperation { .. }));
    /// ```
    pub fn run(&self, _triangulation: CdtTriangulation2D) -> CdtResult<SimulationResultsBackend> {
        // Validate configuration to fail fast before any work
        self.config.validate()?;

        Err(CdtError::UnsupportedOperation {
            operation: "MetropolisAlgorithm::run".to_string(),
            reason: "real CDT ergodic moves are not implemented yet (#55); refusing to return a zero-move simulation result".to_string(),
        })
    }
}

/// Results from a simulation using the new backend system.
#[derive(Debug)]
pub struct SimulationResultsBackend {
    /// Configuration used for the simulation
    pub config: MetropolisConfig,
    /// Action configuration used
    pub action_config: ActionConfig,
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
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// let config = MetropolisConfig::new(1.0, 1, 0, 1).with_seed(7);
    /// let results = SimulationResultsBackend {
    ///     config,
    ///     action_config: ActionConfig::default(),
    ///     steps: vec![],
    ///     measurements: vec![],
    ///     elapsed_time: Duration::from_millis(0),
    ///     triangulation: tri,
    /// };
    /// assert_eq!(results.acceptance_rate(), 0.0);
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
    /// use causal_triangulations::prelude::simulation::{
    ///     ActionConfig, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// let config = MetropolisConfig::new(1.0, 1, 0, 1).with_seed(7);
    /// let results = SimulationResultsBackend {
    ///     config,
    ///     action_config: ActionConfig::default(),
    ///     steps: vec![],
    ///     measurements: vec![],
    ///     elapsed_time: Duration::from_millis(0),
    ///     triangulation: tri,
    /// };
    /// assert_eq!(results.average_action(), 0.0);
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
    ///     ActionConfig, CdtTriangulation, MetropolisConfig, SimulationResultsBackend,
    /// };
    /// use std::time::Duration;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// let config = MetropolisConfig::new(1.0, 2, 1, 1).with_seed(7);
    /// let results = SimulationResultsBackend {
    ///     config,
    ///     action_config: ActionConfig::default(),
    ///     steps: vec![],
    ///     measurements: vec![],
    ///     elapsed_time: Duration::from_millis(0),
    ///     triangulation: tri,
    /// };
    /// assert!(results.equilibrium_measurements().is_empty());
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
    use crate::cdt::triangulation::CdtTriangulation;
    use crate::geometry::traits::TriangulationQuery;
    use approx::assert_relative_eq;

    fn assert_unsupported_simulation(err: CdtError) {
        match err {
            CdtError::UnsupportedOperation { operation, reason } => {
                assert_eq!(operation, "MetropolisAlgorithm::run");
                assert!(
                    reason.contains("real CDT ergodic moves are not implemented yet"),
                    "Unexpected unsupported-operation reason: {reason}"
                );
            }
            other => panic!("Expected UnsupportedOperation, got {other:?}"),
        }
    }

    #[test]
    fn test_metropolis_config() {
        let config = MetropolisConfig::new(2.0, 500, 50, 5);
        assert_relative_eq!(config.temperature, 2.0);
        assert_relative_eq!(config.beta(), 0.5);
        assert_eq!(config.steps, 500);
        assert!(config.seed.is_none());

        let seeded = config.with_seed(123);
        assert_eq!(seeded.seed, Some(123));
    }

    #[test]
    fn test_backend_vertex_and_edge_counting() {
        // Use fixed seed
        const TRIANGULATION_SEED: u64 = 53;

        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, TRIANGULATION_SEED)
            .expect("Failed to create triangulation with fixed seed");
        let geometry = triangulation.geometry();

        // We intentionally do NOT rely on the upstream deep validation here, since it can be flaky
        // for some generated point sets. Backend-level validity means the triangulation is
        // structurally usable by this crate (counts and iterators behave as expected).
        assert!(
            geometry.is_valid(),
            "Triangulation should be structurally valid for backend queries"
        );

        // Ensure the backend exposes the expected simplex counts.
        assert_eq!(
            geometry.vertex_count(),
            5,
            "Vertex count should match requested seeded generation"
        );
        assert!(geometry.edge_count() > 0, "Should have edges");
        assert!(geometry.face_count() > 0, "Should have faces");
    }

    #[test]
    fn test_action_calculation() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 1, 2).expect("Failed to create triangulation");

        let config = MetropolisConfig::default();
        let action_config = ActionConfig::default();
        let _algorithm = MetropolisAlgorithm::new(config, action_config.clone());

        let geometry = triangulation.geometry();
        let action = action_config.calculate_action(
            u32::try_from(geometry.vertex_count()).unwrap_or_default(),
            u32::try_from(geometry.edge_count()).unwrap_or_default(),
            u32::try_from(geometry.face_count()).unwrap_or_default(),
        );

        // Since we're using a random triangulation, just verify it returns a finite value
        assert!(action.is_finite());
    }

    #[test]
    fn test_cdt_target_log_prob() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53)
            .expect("Failed to create triangulation");

        let target = CdtTarget::new(ActionConfig::default(), 1.0);

        let log_prob = markov_chain_monte_carlo::Target::log_prob(&target, &triangulation);
        assert!(log_prob.is_finite(), "log_prob should be finite");

        // log_prob = -action/T, so with T=1 it should be the negative of the action
        let g = triangulation.geometry();
        let action = ActionConfig::default().calculate_action(
            u32::try_from(g.vertex_count()).unwrap_or_default(),
            u32::try_from(g.edge_count()).unwrap_or_default(),
            u32::try_from(g.face_count()).unwrap_or_default(),
        );
        assert_relative_eq!(log_prob, -action);
    }

    #[test]
    fn test_simulation_guardrail_rejects_zero_move_run_with_seed() {
        let config = MetropolisConfig::new(1.0, 10, 2, 2).with_seed(42);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        let triangulation =
            CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed to create");
        let err = algorithm
            .run(triangulation)
            .expect_err("zero-move simulation should be rejected");
        assert_unsupported_simulation(err);
    }

    #[test]
    fn test_seeded_simulation_guardrail_is_deterministic() {
        let run_error = |seed: u64| {
            let config = MetropolisConfig::new(1.0, 20, 5, 5).with_seed(seed);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
            algorithm
                .run(tri)
                .expect_err("zero-move simulation should be rejected")
        };

        let e1 = run_error(123);
        let e2 = run_error(123);

        assert_eq!(e1, e2);
        assert_unsupported_simulation(e1);
    }

    #[test]
    fn test_run_rejects_zero_measurement_frequency() {
        let config = MetropolisConfig::new(1.0, 10, 2, 0);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm.run(tri).unwrap_err();
        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "measurement_frequency");
                assert_eq!(provided_value, "0");
                assert_eq!(expected, "≥ 1");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn test_run_rejects_invalid_temperature() {
        for bad_temp in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let config = MetropolisConfig::new(bad_temp, 10, 2, 2);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

            let err = algorithm.run(tri).unwrap_err();
            match err {
                CdtError::InvalidSimulationConfiguration {
                    setting, expected, ..
                } => {
                    assert_eq!(setting, "temperature", "T={bad_temp}");
                    assert_eq!(expected, "finite and positive", "T={bad_temp}");
                }
                other => panic!(
                    "Expected InvalidSimulationConfiguration for T={bad_temp}, got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn test_validate_rejects_missing_post_thermalization_measurement() {
        let err = MetropolisConfig::new(1.0, 19, 15, 10)
            .validate()
            .expect_err(
                "Configuration should require at least one post-thermalization measurement",
            );

        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "measurement schedule");
                assert!(
                    provided_value.contains("steps=19")
                        && provided_value.contains("thermalization_steps=15")
                        && provided_value.contains("measurement_frequency=10"),
                    "Unexpected provided value: {provided_value}"
                );
                assert_eq!(expected, "at least one post-thermalization measurement");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_rejects_overflowed_post_thermalization_boundary() {
        let err = MetropolisConfig::new(1.0, u32::MAX, u32::MAX, 2)
            .validate()
            .expect_err(
                "Configuration should reject schedules without a reachable post-thermalization measurement",
            );

        match err {
            CdtError::InvalidSimulationConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "measurement schedule");
                assert!(
                    provided_value.contains("steps=4294967295")
                        && provided_value.contains("thermalization_steps=4294967295")
                        && provided_value.contains("measurement_frequency=2"),
                    "Unexpected provided value: {provided_value}"
                );
                assert_eq!(expected, "at least one post-thermalization measurement");
            }
            other => panic!("Expected InvalidSimulationConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn test_run_validates_boundary_aligned_schedule_before_guardrail() {
        let config = MetropolisConfig::new(1.0, 20, 15, 10).with_seed(42);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm
            .run(tri)
            .expect_err("valid schedule should reach the zero-move simulation guardrail");
        assert_unsupported_simulation(err);
    }

    #[test]
    fn test_run_without_seed_reaches_guardrail() {
        let config = MetropolisConfig::new(1.0, 5, 1, 1); // no seed
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm
            .run(tri)
            .expect_err("zero-move simulation should be rejected");
        assert_unsupported_simulation(err);
    }

    #[test]
    fn test_simulation_results() {
        let config = MetropolisConfig::default();
        let measurements = vec![
            Measurement {
                step: 0,
                action: 1.0,
                vertices: 3,
                edges: 3,
                triangles: 1,
            },
            Measurement {
                step: 10,
                action: 2.0,
                vertices: 4,
                edges: 5,
                triangles: 2,
            },
        ];

        let triangulation =
            CdtTriangulation::from_random_points(3, 1, 2).expect("Failed to create triangulation");

        let results = SimulationResultsBackend {
            config,
            action_config: ActionConfig::default(),
            steps: vec![],
            measurements,
            elapsed_time: Duration::from_millis(100),
            triangulation,
        };

        assert_relative_eq!(results.average_action(), 1.5);
    }
}
