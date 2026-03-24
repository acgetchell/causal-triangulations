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
use crate::geometry::traits::TriangulationQuery;
use markov_chain_monte_carlo::{Chain, ProposalMut, Target};
use num_traits::cast::NumCast;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::Instant;

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
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Returns the inverse temperature (β = 1/T).
    #[must_use]
    pub fn beta(&self) -> f64 {
        1.0 / self.temperature
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
    #[must_use]
    pub const fn new(action_config: ActionConfig, temperature: f64) -> Self {
        Self {
            action_config,
            temperature,
        }
    }
}

impl Target<crate::geometry::CdtTriangulation2D> for CdtTarget {
    fn log_prob(&self, state: &crate::geometry::CdtTriangulation2D) -> f64 {
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

impl ProposalMut<crate::geometry::CdtTriangulation2D> for CdtProposal {
    type Undo = ();

    fn propose_mut<R: Rng + ?Sized>(
        &self,
        _state: &mut crate::geometry::CdtTriangulation2D,
        _rng: &mut R,
    ) -> Option<()> {
        // TODO (#55): Select a random ergodic move, attempt it on the
        // triangulation, and return an undo token on success.
        None
    }

    fn undo(&self, _state: &mut crate::geometry::CdtTriangulation2D, _token: ()) {
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
    action_config: ActionConfig,
}

impl MetropolisAlgorithm {
    /// Creates a new Metropolis algorithm instance.
    #[must_use]
    pub const fn new(config: MetropolisConfig, action_config: ActionConfig) -> Self {
        Self {
            config,
            action_config,
        }
    }

    /// Run the Monte Carlo simulation.
    ///
    /// This runs the Metropolis-Hastings algorithm on the given triangulation
    /// using the `markov-chain-monte-carlo` crate for acceptance/rejection.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::Mcmc`](crate::errors::CdtError::Mcmc) if the MCMC
    /// framework encounters a NaN log-probability or proposal ratio.
    pub fn run(
        &self,
        triangulation: crate::geometry::CdtTriangulation2D,
    ) -> crate::errors::CdtResult<SimulationResultsBackend> {
        // Validate configuration to fail fast before any work
        if self.config.measurement_frequency == 0 {
            return Err(crate::errors::CdtError::InvalidParameters(
                "measurement_frequency must be > 0".to_string(),
            ));
        }
        if !self.config.temperature.is_finite() || self.config.temperature <= 0.0 {
            return Err(crate::errors::CdtError::InvalidParameters(format!(
                "temperature must be finite and positive, got {}",
                self.config.temperature,
            )));
        }

        let start_time = Instant::now();
        let mut mc_steps = Vec::new();
        let mut measurements = Vec::new();

        // Ensure we have a concrete seed for provenance (generate one if not provided)
        let seed = self.config.seed.unwrap_or_else(rand::random::<u64>);

        log::info!("Starting Metropolis-Hastings simulation...");
        log::info!("Temperature: {}", self.config.temperature);
        log::info!("Total steps: {}", self.config.steps);
        log::info!("Thermalization steps: {}", self.config.thermalization_steps);
        log::info!("RNG seed: {seed}");

        // Set up MCMC chain
        let target = CdtTarget::new(self.action_config.clone(), self.config.temperature);
        let proposal = CdtProposal;
        let mut chain = Chain::new(triangulation, &target)?;

        // Create seeded RNG (always deterministic from the resolved seed)
        let mut rng = StdRng::seed_from_u64(seed);

        for step_num in 0..self.config.steps {
            let action_before = -chain.log_prob * self.config.temperature;
            let accepted = chain.step_mut(&target, &proposal, &mut rng)?;
            let action_after = -chain.log_prob * self.config.temperature;

            // TODO: Record actual move type once #55 provides real moves
            let mc_step = MonteCarloStep {
                step: step_num,
                move_type: MoveType::Move22, // placeholder
                accepted,
                action_before,
                action_after: if accepted { Some(action_after) } else { None },
                delta_action: if accepted {
                    Some(action_after - action_before)
                } else {
                    None
                },
            };
            mc_steps.push(mc_step);

            // Take measurement if needed
            if step_num % self.config.measurement_frequency == 0 {
                let g = chain.state.geometry();
                measurements.push(Measurement {
                    step: step_num,
                    action: action_after,
                    vertices: u32::try_from(g.vertex_count()).unwrap_or_default(),
                    edges: u32::try_from(g.edge_count()).unwrap_or_default(),
                    triangles: u32::try_from(g.face_count()).unwrap_or_default(),
                });
            }

            // Progress reporting
            if step_num % 100 == 0 {
                log::debug!(
                    "Step {}/{}, Action: {:.3}",
                    step_num,
                    self.config.steps,
                    action_after,
                );
            }
        }

        let elapsed_time = start_time.elapsed();
        log::info!("Simulation completed in {elapsed_time:.2?}");
        log::info!("Acceptance rate: {:.2}%", chain.acceptance_rate() * 100.0);

        // Store the resolved seed in the results for provenance
        let mut result_config = self.config.clone();
        result_config.seed = Some(seed);

        Ok(SimulationResultsBackend {
            config: result_config,
            action_config: self.action_config.clone(),
            steps: mc_steps,
            measurements,
            elapsed_time,
            triangulation: chain.state,
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
    pub elapsed_time: std::time::Duration,
    /// Final triangulation state
    pub triangulation: crate::geometry::CdtTriangulation2D,
}

impl SimulationResultsBackend {
    /// Calculates the acceptance rate for the simulation.
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
    fn test_simulation_runs_with_seed() {
        let config = MetropolisConfig::new(1.0, 10, 2, 2).with_seed(42);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        let triangulation =
            CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed to create");
        let results = algorithm
            .run(triangulation)
            .expect("Simulation should succeed");

        // With placeholder proposal, all moves are rejected
        assert_relative_eq!(results.acceptance_rate(), 0.0);
        assert!(!results.measurements.is_empty());
    }

    #[test]
    fn test_seeded_simulation_determinism() {
        let run = |seed: u64| {
            let config = MetropolisConfig::new(1.0, 20, 5, 5).with_seed(seed);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
            algorithm.run(tri).expect("Failed")
        };

        let r1 = run(123);
        let r2 = run(123);

        assert_eq!(r1.steps.len(), r2.steps.len());
        assert_eq!(r1.measurements.len(), r2.measurements.len());
        for (m1, m2) in r1.measurements.iter().zip(r2.measurements.iter()) {
            assert_relative_eq!(m1.action, m2.action);
        }
    }

    #[test]
    fn test_run_rejects_zero_measurement_frequency() {
        let config = MetropolisConfig::new(1.0, 10, 2, 0);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm.run(tri).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("measurement_frequency"), "Error: {msg}");
    }

    #[test]
    fn test_run_rejects_invalid_temperature() {
        for bad_temp in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let config = MetropolisConfig::new(bad_temp, 10, 2, 2);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

            let err = algorithm.run(tri).unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("temperature"), "T={bad_temp}: {msg}");
        }
    }

    #[test]
    fn test_run_records_seed_for_provenance() {
        // When no seed is provided, run() should generate and record one
        let config = MetropolisConfig::new(1.0, 5, 1, 1); // no seed
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let results = algorithm.run(tri).expect("Should succeed");
        assert!(
            results.config.seed.is_some(),
            "Results should always contain a resolved seed"
        );
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
            elapsed_time: std::time::Duration::from_millis(100),
            triangulation,
        };

        assert_relative_eq!(results.average_action(), 1.5);
    }
}
