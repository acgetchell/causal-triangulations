//! Metropolis-Hastings algorithm for Causal Dynamical Triangulations.
//!
//! This module implements the Monte Carlo sampling algorithm used to sample
//! triangulation configurations according to the CDT path integral measure.
//!
//! The simulation accepts or rejects a proposed move type before mutating the
//! triangulation. Accepted moves are then applied through the CDT ergodic move
//! kernels; failed accepted applications are rolled back and retried at another
//! randomly selected local site.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
use crate::cdt::triangulation::SimulationEvent;
use crate::config::validate_schedule;
use crate::errors::{CdtError, CdtResult};
use crate::geometry::CdtTriangulation2D;
use crate::geometry::traits::TriangulationQuery;
use markov_chain_monte_carlo::{ProposalMut, Target};
use num_traits::cast::NumCast;
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
use std::time::{Duration, Instant};

// Test utilities are now handled through backend-agnostic CdtTriangulation::new

const ACCEPTED_MOVE_RETRIES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SimplexCounts {
    vertices: u32,
    edges: u32,
    triangles: u32,
}

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
    /// use causal_triangulations::prelude::action::ActionConfig;
    /// use causal_triangulations::prelude::simulation::CdtTarget;
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

/// Legacy CDT proposal distribution adapter.
///
/// The production simulation loop currently uses an accept-before-mutation CDT
/// ordering that the `markov-chain-monte-carlo` crate does not model yet. The
/// type remains available for callers that depend on the older mutation-first
/// `ProposalMut` integration point; issue acgetchell/markov-chain-monte-carlo#34
/// tracks the upstream delayed-commit API needed to replace the local loop.
pub struct CdtProposal;

impl ProposalMut<CdtTriangulation2D> for CdtProposal {
    type Undo = ();

    fn propose_mut<R: Rng + ?Sized>(
        &self,
        _state: &mut CdtTriangulation2D,
        _rng: &mut R,
    ) -> Option<()> {
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
/// Accepts or rejects proposed CDT move types before applying them.
pub struct MetropolisAlgorithm {
    /// Algorithm configuration
    config: MetropolisConfig,
    /// Action calculation configuration
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
    /// Each step proposes a move type, computes the Metropolis-Hastings
    /// acceptance probability from the move's simplex-count delta, and only
    /// mutates the triangulation when that proposal is accepted. Accepted moves
    /// that fail during backend application are rolled back and retried at
    /// another randomly selected local site.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
    /// configuration is invalid, [`CdtError::MetropolisMoveApplicationFailed`]
    /// if an accepted move causes a hard backend mutation failure, or a
    /// validation error for unrecoverable triangulation failures.
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
    /// let results = MetropolisAlgorithm::new(config, ActionConfig::default())
    ///     .run(tri)
    ///     .expect("run simulation");
    /// assert_eq!(results.steps.len(), 2);
    /// ```
    pub fn run(
        &self,
        mut triangulation: CdtTriangulation2D,
    ) -> CdtResult<SimulationResultsBackend> {
        // Validate configuration to fail fast before any work
        self.config.validate()?;
        self.action_config.validate()?;

        let mut rng = simulation_rng(self.config.seed);
        let mut moves = self.config.seed.map_or_else(ErgodicsSystem::new, |seed| {
            ErgodicsSystem::with_seed(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
        });
        let start = Instant::now();
        let mut move_stats = MoveStatistics::new();
        let mut steps = Vec::with_capacity(usize::try_from(self.config.steps).unwrap_or(0));
        let mut measurements = Vec::new();

        let mut current_action = action_for(&self.action_config, &triangulation);
        measurements.push(measurement_for(0, current_action, &triangulation));
        triangulation
            .geometry_mut()
            .record_event(SimulationEvent::MeasurementTaken {
                step: 0,
                action: current_action,
            });

        for step in 1..=self.config.steps {
            let move_type = moves.select_random_move();
            move_stats.record_attempt(move_type);
            triangulation
                .geometry_mut()
                .record_event(SimulationEvent::MoveAttempted {
                    move_type: format!("{move_type:?}"),
                    step: step.into(),
                });

            let action_before = current_action;
            let delta_action = proposed_delta_action(
                &self.action_config,
                simplex_counts(&triangulation),
                move_type,
            );

            let mut accepted = false;
            let mut action_after = None;
            if let Some(delta) = delta_action
                && metropolis_accept(delta, self.config.temperature, &mut rng)
            {
                let applied_action = apply_accepted_move(
                    &mut triangulation,
                    &mut moves,
                    &self.action_config,
                    move_type,
                    step,
                    action_before,
                )?;
                accepted = true;
                action_after = Some(applied_action);
                current_action = applied_action;
                move_stats.record_success(move_type);
                triangulation
                    .geometry_mut()
                    .record_event(SimulationEvent::MoveAccepted {
                        move_type: format!("{move_type:?}"),
                        step: step.into(),
                        action_change: applied_action - action_before,
                    });
            }

            steps.push(MonteCarloStep {
                step,
                move_type,
                accepted,
                action_before,
                action_after,
                delta_action,
            });

            if step.is_multiple_of(self.config.measurement_frequency) {
                measurements.push(measurement_for(step, current_action, &triangulation));
                triangulation
                    .geometry_mut()
                    .record_event(SimulationEvent::MeasurementTaken {
                        step: step.into(),
                        action: current_action,
                    });
            }
        }

        Ok(SimulationResultsBackend {
            config: self.config.clone(),
            action_config: self.action_config.clone(),
            move_stats,
            steps,
            measurements,
            elapsed_time: start.elapsed(),
            triangulation,
        })
    }
}

/// Builds the RNG used only for Metropolis acceptance draws.
///
/// This keeps acceptance randomness separate from move-site selection, so seeded
/// simulations are reproducible while unseeded simulations still draw fresh entropy.
fn simulation_rng(seed: Option<u64>) -> StdRng {
    seed.map_or_else(rand::make_rng, StdRng::seed_from_u64)
}

/// Reads simplex counts through the CDT wrapper for action and measurement code.
///
/// Centralizing these conversions keeps cached query paths authoritative and
/// makes integer saturation explicit at the simulation boundary.
fn simplex_counts(triangulation: &CdtTriangulation2D) -> SimplexCounts {
    SimplexCounts {
        vertices: u32::try_from(triangulation.vertex_count()).unwrap_or(u32::MAX),
        edges: u32::try_from(triangulation.edge_count()).unwrap_or(u32::MAX),
        triangles: u32::try_from(triangulation.face_count()).unwrap_or(u32::MAX),
    }
}

/// Computes the current action from live simplex counts.
///
/// The Metropolis loop calls this only after state is known to be current, which
/// avoids trusting stale values across backend mutations or rollback.
fn action_for(action_config: &ActionConfig, triangulation: &CdtTriangulation2D) -> f64 {
    let counts = simplex_counts(triangulation);
    action_config.calculate_action(counts.vertices, counts.edges, counts.triangles)
}

/// Captures a measurement from the live triangulation state.
///
/// Keeping measurement construction in one helper ensures recorded actions and
/// simplex counts use the same query path at every measurement step.
fn measurement_for(step: u32, action: f64, triangulation: &CdtTriangulation2D) -> Measurement {
    let counts = simplex_counts(triangulation);
    Measurement {
        step,
        action,
        vertices: counts.vertices,
        edges: counts.edges,
        triangles: counts.triangles,
    }
}

/// Computes the count-level action change before mutating the triangulation.
///
/// This is the core proposal-before-mutation calculation: Metropolis acceptance
/// must be based on the selected move type's known simplex-count delta, not on a
/// speculative backend edit that may need rollback.
fn proposed_delta_action(
    action_config: &ActionConfig,
    before: SimplexCounts,
    move_type: MoveType,
) -> Option<f64> {
    let after = match move_type {
        MoveType::Move22 | MoveType::EdgeFlip => before,
        MoveType::Move13Add => SimplexCounts {
            vertices: before.vertices.checked_add(1)?,
            edges: before.edges.checked_add(3)?,
            triangles: before.triangles.checked_add(2)?,
        },
        MoveType::Move31Remove => SimplexCounts {
            vertices: before.vertices.checked_sub(1)?,
            edges: before.edges.checked_sub(3)?,
            triangles: before.triangles.checked_sub(2)?,
        },
    };

    let action_before =
        action_config.calculate_action(before.vertices, before.edges, before.triangles);
    let action_after = action_config.calculate_action(after.vertices, after.edges, after.triangles);
    Some(action_after - action_before)
}

/// Applies the Metropolis acceptance rule to a proposed action change.
///
/// Factoring this out keeps the probability rule isolated from move selection
/// and makes deterministic unit tests possible with a seeded RNG.
fn metropolis_accept(delta_action: f64, temperature: f64, rng: &mut StdRng) -> bool {
    delta_action <= 0.0 || rng.random::<f64>() < (-delta_action / temperature).exp()
}

/// Applies an already-accepted move, rolling back and retrying failed sites.
///
/// Once the Metropolis rule accepts a move type, failure to find an applicable
/// local site is a simulation-level failure after bounded retries. Returning a
/// structured error keeps that distinct from ordinary Metropolis rejection.
fn apply_accepted_move(
    triangulation: &mut CdtTriangulation2D,
    moves: &mut ErgodicsSystem,
    action_config: &ActionConfig,
    move_type: MoveType,
    step: u32,
    action_before: f64,
) -> CdtResult<f64> {
    let mut last_failure = "no application attempt was made".to_string();
    for attempt in 1..=ACCEPTED_MOVE_RETRIES {
        let snapshot = triangulation.clone();
        let result = attempt_move(moves, move_type, triangulation);
        match result {
            MoveResult::Success => {
                let action_after = action_for(action_config, triangulation);
                return Ok(action_after);
            }
            MoveResult::HardFailure(err) => {
                *triangulation = snapshot;
                return Err(accepted_move_error(
                    step,
                    move_type,
                    attempt,
                    format!("hard failure after accepted mutation: {err}"),
                ));
            }
            MoveResult::CausalityViolation => {
                *triangulation = snapshot;
                last_failure = "causality violation at selected application site".to_string();
            }
            MoveResult::GeometricViolation => {
                *triangulation = snapshot;
                last_failure = "geometric violation at selected application site".to_string();
            }
            MoveResult::Rejected(err) => {
                *triangulation = snapshot;
                last_failure = err.to_string();
            }
        }
    }

    debug_assert!(
        (action_for(action_config, triangulation) - action_before).abs() < f64::EPSILON,
        "failed accepted move retries must leave the triangulation rolled back"
    );
    Err(accepted_move_error(
        step,
        move_type,
        ACCEPTED_MOVE_RETRIES,
        last_failure,
    ))
}

/// Builds the simulation-level error for an accepted move that could not be applied.
///
/// The move kernels keep causal, geometric, and backend failures orthogonal; this
/// wrapper adds the Metropolis step, move type, and retry context callers need to
/// debug a failed accepted application.
fn accepted_move_error(
    step: u32,
    move_type: MoveType,
    attempts: usize,
    last_failure: String,
) -> CdtError {
    CdtError::MetropolisMoveApplicationFailed {
        step,
        move_type: format!("{move_type:?}"),
        attempts,
        last_failure,
    }
}

/// Dispatches one selected move type to the ergodic move system.
///
/// Keeping dispatch behind this helper lets the Metropolis loop work with move
/// proposals uniformly while the move module retains ownership of each kernel.
fn attempt_move(
    moves: &mut ErgodicsSystem,
    move_type: MoveType,
    triangulation: &mut CdtTriangulation2D,
) -> MoveResult {
    match move_type {
        MoveType::Move22 => moves.attempt_22_move(triangulation),
        MoveType::Move13Add => moves.attempt_13_move(triangulation),
        MoveType::Move31Remove => moves.attempt_31_move(triangulation),
        MoveType::EdgeFlip => moves.attempt_edge_flip(triangulation),
    }
}

/// Results from a simulation using the new backend system.
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
    ///     move_stats: Default::default(),
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
    ///     move_stats: Default::default(),
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
    ///     move_stats: Default::default(),
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
    fn backend_counts_vertices_edges() {
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

        let log_prob = Target::log_prob(&target, &triangulation);
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
    fn seeded_simulation_runs_moves() {
        let config = MetropolisConfig::new(1.0, 10, 2, 2).with_seed(42);
        let action_config = ActionConfig::default();
        let algorithm = MetropolisAlgorithm::new(config, action_config);

        let triangulation =
            CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed to create");
        let results = algorithm
            .run(triangulation)
            .expect("simulation should run with real move loop");

        assert_eq!(results.steps.len(), 10);
        assert_relative_eq!(
            results.move_stats.total_acceptance_rate(),
            results.acceptance_rate()
        );
        assert!(results.measurements.iter().all(|measurement| {
            measurement.action.is_finite()
                && measurement.vertices > 0
                && measurement.edges > 0
                && measurement.triangles > 0
        }));
    }

    #[test]
    fn seeded_simulation_deterministic() {
        let run = |seed: u64| {
            let config = MetropolisConfig::new(1.0, 20, 5, 5).with_seed(seed);
            let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
            let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
            algorithm.run(tri).expect("seeded simulation should run")
        };

        let first = run(123);
        let second = run(123);

        assert_eq!(first.steps.len(), second.steps.len());
        for (first, second) in first.steps.iter().zip(second.steps.iter()) {
            assert_eq!(first.move_type, second.move_type);
            assert_eq!(first.accepted, second.accepted);
            assert_relative_eq!(first.action_before, second.action_before);
            assert_eq!(first.delta_action, second.delta_action);
        }
    }

    #[test]
    fn delta_action_uses_count_deltas() {
        let action_config = ActionConfig::default();
        let before = SimplexCounts {
            vertices: 5,
            edges: 8,
            triangles: 4,
        };

        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move22)
                .expect("2,2 delta should be finite"),
            0.0
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::EdgeFlip)
                .expect("edge flip delta should be finite"),
            0.0
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move13Add)
                .expect("1,3 delta should be finite"),
            -2.7,
            epsilon = 1e-12
        );
        assert_relative_eq!(
            proposed_delta_action(&action_config, before, MoveType::Move31Remove)
                .expect("3,1 delta should be finite"),
            2.7,
            epsilon = 1e-12
        );
    }

    #[test]
    fn delta_action_rejects_bad_counts() {
        let action_config = ActionConfig::default();

        assert_eq!(
            proposed_delta_action(
                &action_config,
                SimplexCounts {
                    vertices: 0,
                    edges: 2,
                    triangles: 1,
                },
                MoveType::Move31Remove,
            ),
            None
        );
        assert_eq!(
            proposed_delta_action(
                &action_config,
                SimplexCounts {
                    vertices: u32::MAX,
                    edges: 8,
                    triangles: 4,
                },
                MoveType::Move13Add,
            ),
            None
        );
    }

    #[test]
    fn accepts_non_positive_delta() {
        let mut rng = StdRng::seed_from_u64(7);
        assert!(metropolis_accept(0.0, 1.0, &mut rng));
        assert!(metropolis_accept(-1.0, 1.0, &mut rng));
    }

    #[test]
    fn accepted_move_retry_exhaustion_reports_error_and_rolls_back() {
        let mut triangulation =
            CdtTriangulation::from_seeded_points(3, 1, 2, 53).expect("Failed to create");
        let action_config = ActionConfig::default();
        let counts_before = simplex_counts(&triangulation);
        let action_before = action_for(&action_config, &triangulation);
        let mut moves = ErgodicsSystem::with_seed(7);

        let err = apply_accepted_move(
            &mut triangulation,
            &mut moves,
            &action_config,
            MoveType::Move31Remove,
            17,
            action_before,
        )
        .expect_err("accepted retry exhaustion should be reported");

        match err {
            CdtError::MetropolisMoveApplicationFailed {
                step,
                move_type,
                attempts,
                last_failure,
            } => {
                assert_eq!(step, 17);
                assert_eq!(move_type, "Move31Remove");
                assert_eq!(attempts, ACCEPTED_MOVE_RETRIES);
                assert!(
                    last_failure.contains("geometric violation")
                        || last_failure.contains("Validation failed"),
                    "unexpected last failure: {last_failure}"
                );
            }
            other => panic!("Expected MetropolisMoveApplicationFailed, got {other:?}"),
        }
        assert_eq!(simplex_counts(&triangulation), counts_before);
        assert_relative_eq!(action_for(&action_config, &triangulation), action_before);
    }

    #[test]
    fn run_rejects_zero_frequency() {
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
    fn run_rejects_bad_temperature() {
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
    fn validate_requires_measurement() {
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
    fn validate_rejects_overflow() {
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
    fn run_accepts_boundary_schedule() {
        let config = MetropolisConfig::new(1.0, 20, 15, 10).with_seed(42);
        let algorithm = MetropolisAlgorithm::new(config, ActionConfig::default());
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let results = algorithm.run(tri).expect("valid schedule should run");
        assert_eq!(results.steps.len(), 20);
        assert!(
            results
                .measurements
                .iter()
                .any(|measurement| measurement.step >= 15)
        );
    }

    #[test]
    fn run_validates_action_config() {
        let config = MetropolisConfig::new(1.0, 20, 15, 10).with_seed(42);
        let action_config = ActionConfig::new(f64::INFINITY, 1.0, 0.1);
        let algorithm = MetropolisAlgorithm::new(config, action_config);
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");

        let err = algorithm
            .run(tri)
            .expect_err("invalid action config should be reported before simulation");
        match err {
            CdtError::InvalidConfiguration {
                setting,
                provided_value,
                expected,
            } => {
                assert_eq!(setting, "coupling_0");
                assert_eq!(provided_value, "inf");
                assert_eq!(expected, "finite");
            }
            other => panic!("Expected InvalidConfiguration, got {other:?}"),
        }
    }

    #[test]
    fn unseeded_config_uses_random_rng() {
        let config = MetropolisConfig::new(1.0, 5, 1, 1); // no seed
        assert!(config.seed.is_none());

        let mut rng = simulation_rng(config.seed);
        let draw = rng.random::<f64>();
        assert!((0.0..1.0).contains(&draw));
    }

    #[test]
    fn test_simulation_results() {
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
            Measurement {
                step: 15,
                action: 3.0,
                vertices: 5,
                edges: 7,
                triangles: 3,
            },
        ];

        let triangulation =
            CdtTriangulation::from_random_points(3, 1, 2).expect("Failed to create triangulation");

        let results = SimulationResultsBackend {
            config,
            action_config: ActionConfig::default(),
            move_stats: MoveStatistics::new(),
            steps,
            measurements,
            elapsed_time: Duration::from_millis(100),
            triangulation,
        };

        assert_relative_eq!(results.acceptance_rate(), 2.0 / 3.0);
        assert_relative_eq!(results.average_action(), 2.0);

        let equilibrium = results.equilibrium_measurements();
        assert_eq!(equilibrium.len(), 2);
        assert_eq!(equilibrium[0].step, 10);
        assert_eq!(equilibrium[1].step, 15);
    }

    #[test]
    fn cdt_proposal_returns_none() {
        let proposal = CdtProposal;
        let mut triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("Failed");
        let mut rng = StdRng::seed_from_u64(7);

        assert!(proposal.propose_mut(&mut triangulation, &mut rng).is_none());
        proposal.undo(&mut triangulation, ());
    }
}
