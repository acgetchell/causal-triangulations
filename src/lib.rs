#![forbid(unsafe_code)]
#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive dependencies currently resolve several shared crate versions"
)]
#![warn(missing_docs)]

//! Causal Dynamical Triangulations library for quantum gravity simulations.
//!
//! This library implements Causal Dynamical Triangulations (CDT) in 2D, providing
//! the necessary tools for Monte Carlo simulations of discrete spacetime geometries.
//!
//! # Key Features
//!
//! - Integration with delaunay crate for proper Delaunay triangulations
//! - 2D Regge Action calculation for CDT
//! - Foliated 2D triangulation construction and validation
//! - Foliation-aware 2D ergodic moves backed by bistellar flips
//! - Metropolis-Hastings sampling over foliation-aware 2D ergodic moves
//! - Volume-profile, Hausdorff-dimension, and spectral-dimension observables
//!   for CDT analysis
//! - CSV/JSON simulation output and resumable serde-backed CDT/MCMC checkpoints
//!
//! The crate root re-exports the most common construction, simulation,
//! observable, and error types. Focused preludes under [`prelude`] provide
//! smaller import surfaces for documentation, examples, integration tests, and
//! benchmarks.
//!
//! # Checkpointing
//!
//! CDT triangulations backed by [`geometry::DelaunayBackend2D`] serialize their
//! stable geometry, metadata, foliation, and simulation history while rebuilding
//! transient caches and timestamps on load.
//!
//! ```
//! use causal_triangulations::{CheckpointOperation, CdtError};
//! use causal_triangulations::prelude::triangulation::*;
//! use serde_json::{from_str, to_string};
//!
//! fn main() -> CdtResult<()> {
//!     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
//!     let json = to_string(&tri).map_err(|err| CdtError::CheckpointSerializationFailed {
//!         operation: CheckpointOperation::Serialize,
//!         target: "triangulation".to_string(),
//!         detail: err.to_string(),
//!     })?;
//!     let restored: CdtTriangulation2D =
//!         from_str(&json).map_err(|err| CdtError::CheckpointSerializationFailed {
//!             operation: CheckpointOperation::Deserialize,
//!             target: "triangulation".to_string(),
//!             detail: err.to_string(),
//!         })?;
//!     restored.validate_topology()?;
//!     restored.validate_foliation()?;
//!     restored.validate_causality()?;
//!     restored.validate_simplex_classification()?;
//!     assert_eq!(restored.slice_sizes(), &[4, 4, 4]);
//!     Ok(())
//! }
//! ```
//!
//! # Example
//!
//! ```
//! use causal_triangulations::prelude::triangulation::CdtTriangulation;
//! use causal_triangulations::prelude::errors::CdtResult;
//!
//! fn main() -> CdtResult<()> {
//!     let tri = CdtTriangulation::from_toroidal_cdt(4, 3)?;
//!     assert_eq!(tri.vertex_count(), 12);
//!     assert!(tri.validate_topology().is_ok());
//!     assert!(tri.validate_foliation().is_ok());
//!     Ok(())
//! }
//! ```

// Module declarations (avoiding mod.rs files)
/// Configuration management for CDT simulations.
pub mod config;

/// Error types for the CDT library.
pub mod errors;

/// Utility functions for random number generation and mathematical operations.
pub mod util;

/// Geometry abstraction layer for CDT simulations.
///
/// This module provides trait-based geometry operations that isolate CDT algorithms
/// from specific geometry implementations.
pub mod geometry {
    /// High-level triangulation operations.
    pub mod operations;
    /// Core geometry traits for CDT abstraction.
    pub mod traits;

    /// Delaunay triangulation generators.
    pub mod generators;

    /// Geometry backend implementations.
    pub mod backends {
        /// Delaunay backend - wraps the delaunay crate.
        pub mod delaunay;

        /// Mock backend for testing.
        pub mod mock;
    }

    // Type aliases for common backend combinations
    /// 2D Delaunay backend (most common configuration).
    ///
    /// Uses `f64` coordinates with `u32` vertex data (time-slice labels) and `i32` simplex data.
    pub type DelaunayBackend2D = backends::delaunay::DelaunayBackend<u32, i32, 2>;

    /// Default backend type for 2D CDT simulations
    pub type DefaultBackend = DelaunayBackend2D;

    /// Convenient alias for CDT triangulations using the default backend
    pub type CdtTriangulation2D = crate::cdt::triangulation::CdtTriangulation<DefaultBackend>;

    pub use generators::{GlobalTopology, TopologyGuarantee, ToroidalConstructionMode};
}

/// Causal Dynamical Triangulations implementation modules.
pub mod cdt {
    /// Action calculation for CDT simulations.
    pub mod action;
    /// Ergodic moves for triangulation modifications.
    pub mod ergodic_moves;
    /// Foliation data structures (time labels, edge classification).
    pub mod foliation;
    /// Metropolis-Hastings sampling for CDT triangulations.
    ///
    /// The module is split by API boundary:
    /// [`adapter`](crate::cdt::metropolis::adapter) exposes the CDT target and
    /// delayed proposal types used by `markov-chain-monte-carlo`,
    /// [`runner`](crate::cdt::metropolis::runner) provides the transitional
    /// [`MetropolisAlgorithm`](crate::cdt::metropolis::MetropolisAlgorithm)
    /// facade, [`checkpoint`](crate::cdt::metropolis::checkpoint) owns
    /// resumable CDT/MCMC checkpoints, and
    /// [`telemetry`](crate::cdt::metropolis::telemetry) contains step and
    /// proposal counters. Most callers should import these through
    /// [`crate::prelude::simulation`] or the re-exports on this module.
    pub mod metropolis {
        /// Adapter boundary to the upstream MCMC crate.
        pub mod adapter;
        /// Checkpoint and resume validation.
        pub mod checkpoint;
        /// Shared CDT-domain helper functions for Metropolis modules.
        pub(crate) mod helpers;
        /// Transitional Metropolis runner implementation.
        pub mod runner;
        /// Step and proposal telemetry types.
        pub mod telemetry;

        pub use adapter::{
            CdtProposal, CdtProposalError, CdtProposalInfo, CdtProposalPlan, CdtTarget,
        };
        pub use checkpoint::CdtMcmcCheckpoint;
        pub use runner::{MetropolisAlgorithm, MetropolisConfig};
        pub use telemetry::{MonteCarloStep, ProposalStatistics};
    }
    /// User-facing CDT observable estimators.
    pub mod observables;
    /// Simulation result containers and measurement summaries.
    pub mod results;
    /// CDT triangulation wrapper.
    pub mod triangulation;
}

// Re-exports for convenience
pub use cdt::action::{ActionConfig, compute_regge_action};
pub use cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
pub use cdt::foliation::{EdgeType, Foliation, FoliationError, SimplexType};
pub use cdt::metropolis::{
    CdtMcmcCheckpoint, CdtProposal, CdtProposalError, CdtProposalInfo, CdtProposalPlan, CdtTarget,
    MetropolisAlgorithm, MetropolisConfig, MonteCarloStep, ProposalStatistics,
};
pub use cdt::observables::{estimate_hausdorff_dimension, estimate_spectral_dimension};
pub use cdt::results::{Measurement, SimulationResultsBackend};
pub use config::{
    CdtConfig, CdtConfigOverrides, CdtTopology, DimensionOverride, TestConfig, ValidatedCdtConfig,
    ValidatedInitialVolume,
};
pub use errors::{
    BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure,
    CheckpointMoveCounter, CheckpointOperation, CheckpointResumeFailure, ConfigurationSetting,
    DelaunayValidationLevel, GenerationParameterIssue, MetropolisMoveApplicationFailure,
    OutputFormat, TriangulationMetadataField,
};

use crate::cdt::results::SimulationResultsParts;
use crate::util::saturating_usize_to_u32;
use std::env;
use std::time::Duration;

// Trait-based triangulation (recommended)
pub use cdt::triangulation::{CdtTriangulation, SimulationEvent};
pub use geometry::traits::TriangulationQuery;

/// Prelude module for convenient imports.
///
/// Provides the small set of types most examples need for CDT construction,
/// configuration, simulation startup, and error handling. Use scoped preludes
/// such as [`prelude::simulation`], [`prelude::observables`], and
/// [`prelude::geometry`] for specialized workflows.
///
/// # Quick start
///
/// ```
/// use causal_triangulations::prelude::*;
///
/// fn main() -> CdtResult<()> {
///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
///     assert!(tri.validate_foliation().is_ok());
///     Ok(())
/// }
/// ```
pub mod prelude {
    // Core CDT types
    pub use crate::CdtTriangulation;
    pub use crate::geometry::CdtTriangulation2D;
    pub use crate::geometry::traits::TriangulationQuery;

    // Action and simulation setup
    pub use crate::cdt::action::ActionConfig;
    pub use crate::cdt::metropolis::{MetropolisAlgorithm, MetropolisConfig};
    pub use crate::run_simulation;

    // Configuration and errors
    pub use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
    pub use crate::errors::{CdtError, CdtResult};

    /// Focused exports for configuration parsing and presets.
    pub mod config {
        pub use crate::config::{
            CdtConfig, CdtConfigOverrides, CdtTopology, DimensionOverride, TestConfig,
            ValidatedCdtConfig, ValidatedInitialVolume,
        };
    }

    /// Focused exports for crate error handling.
    ///
    /// ```
    /// use causal_triangulations::prelude::errors::{
    ///     BackendMutationOperation, CdtError, MetropolisMoveApplicationFailure,
    /// };
    /// use causal_triangulations::prelude::moves::MoveType;
    ///
    /// let err = CdtError::MetropolisMoveApplicationFailed {
    ///     step: 3,
    ///     move_type: MoveType::Move31Remove,
    ///     attempts: 8,
    ///     source: MetropolisMoveApplicationFailure::BackendMutation {
    ///         operation: BackendMutationOperation::RemoveVertex,
    ///         target: "vertex VertexKey(7v1)".to_string(),
    ///         detail: "backend reported invalid vertex key".to_string(),
    ///     },
    /// };
    /// assert!(format!("{err}").contains("Metropolis accepted Move31Remove"));
    /// ```
    pub mod errors {
        pub use crate::errors::{
            BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck,
            CdtValidationFailure, CheckpointMoveCounter, CheckpointOperation,
            CheckpointResumeFailure, ConfigurationSetting, DelaunayValidationLevel,
            GenerationParameterIssue, MetropolisMoveApplicationFailure, OutputFormat,
            TriangulationMetadataField,
        };
    }

    /// Focused exports for CDT action calculations.
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::*;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2);
    /// let action = config.calculate_action(5, 10, 8);
    /// assert_relative_eq!(action, -20.0, epsilon = 1e-12);
    /// ```
    pub mod action {
        pub use crate::cdt::action::{ActionConfig, compute_regge_action};
    }

    /// Focused exports for CDT triangulation construction, queries, and history events.
    ///
    /// Lighter than `prelude::*` — just the types needed for building and
    /// inspecting triangulations (the most common doctest pattern).
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     assert_eq!(tri.slice_sizes(), &[4, 4, 4]);
    ///     Ok(())
    /// }
    /// ```
    pub mod triangulation {
        pub use crate::cdt::foliation::{EdgeType, Foliation, FoliationError, SimplexType};
        pub use crate::config::CdtTopology;
        pub use crate::errors::{CdtError, CdtResult};
        pub use crate::geometry::CdtTriangulation2D;
        pub use crate::geometry::traits::TriangulationQuery;
        pub use crate::{CdtTriangulation, SimulationEvent};
    }

    /// Focused exports for local CDT move kernels and move statistics.
    ///
    /// This prelude intentionally contains only the move API. Combine it with
    /// `prelude::triangulation::*` and, for explicit Delaunay fixtures,
    /// `prelude::geometry::*`.
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::*;
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// assert_eq!(stats.moves_22_attempted, 1);
    /// ```
    pub mod moves {
        pub use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
    }

    /// Focused exports for running CDT simulations.
    ///
    /// This prelude includes [`run_simulation`], validated simulation
    /// configuration, the Metropolis runner, delayed proposal adapter, telemetry
    /// structs, result containers, and typed proposal errors needed by MCMC
    /// workflows. It also includes the triangulation query trait so callers can
    /// inspect final or checkpointed states returned by simulation APIs. The
    /// upstream MCMC traits are re-exported here because
    /// [`CdtProposal`](crate::cdt::metropolis::CdtProposal) and
    /// [`CdtTarget`](crate::cdt::metropolis::CdtTarget) expose their primary
    /// behavior through those trait implementations.
    /// Observable estimators live in [`crate::prelude::observables`].
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtConfig, CdtResult, ValidatedCdtConfig,
    /// };
    ///
    /// fn configured_steps(config: ValidatedCdtConfig) -> u32 {
    ///     config.to_metropolis_config().steps
    /// }
    ///
    /// fn main() -> CdtResult<()> {
    ///     let config = CdtConfig {
    ///         steps: 5,
    ///         thermalization_steps: 0,
    ///         measurement_frequency: 1,
    ///         ..CdtConfig::new(16, 4)
    ///     }
    ///     .into_validated()?;
    ///
    ///     assert_eq!(configured_steps(config), 5);
    ///     Ok(())
    /// }
    /// ```
    pub mod simulation {
        pub use crate::cdt::action::{ActionConfig, compute_regge_action};
        pub use crate::cdt::ergodic_moves::MoveType;
        pub use crate::cdt::metropolis::{
            CdtMcmcCheckpoint, CdtProposal, CdtProposalError, CdtProposalInfo, CdtProposalPlan,
            CdtTarget, MetropolisAlgorithm, MetropolisConfig, MonteCarloStep, ProposalStatistics,
        };
        pub use crate::cdt::results::{Measurement, SimulationResultsBackend};
        pub use crate::cdt::triangulation::SimulationEvent;
        pub use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
        pub use crate::errors::{CdtError, CdtResult};
        pub use crate::geometry::CdtTriangulation2D;
        pub use crate::geometry::traits::TriangulationQuery;
        pub use crate::{CdtTriangulation, run_simulation};
        pub use markov_chain_monte_carlo::{ChainCheckpoint, DelayedProposal, Target};
    }

    /// Focused exports for CDT observables and post-simulation analysis.
    ///
    /// This prelude is intended for measuring triangulations without importing
    /// simulation runner, telemetry, proposal, or move APIs.
    /// It intentionally re-exports [`CdtTriangulation`] and
    /// [`CdtTriangulation2D`] so observable doctests can build inputs with
    /// constructors such as [`CdtTriangulation::from_cdt_strip`] without
    /// importing the triangulation or geometry preludes separately.
    ///
    /// ```
    /// use causal_triangulations::prelude::errors::CdtResult;
    /// use causal_triangulations::prelude::observables::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let profile = tri.volume_profile();
    ///
    ///     assert_eq!(profile.len(), 3);
    ///     assert!(estimate_hausdorff_dimension(&tri).is_some_and(f64::is_finite));
    ///     assert!(estimate_spectral_dimension(&tri).is_some_and(f64::is_finite));
    ///     Ok(())
    /// }
    /// ```
    pub mod observables {
        pub use crate::CdtTriangulation;
        pub use crate::cdt::observables::{
            estimate_hausdorff_dimension, estimate_spectral_dimension,
        };
        pub use crate::geometry::CdtTriangulation2D;
    }

    /// Focused exports for geometry backend construction and querying.
    ///
    /// This prelude is intended for backend-level workflows (e.g. building
    /// triangulations with explicit vertex data and running trait-based geometry
    /// queries), without pulling in simulation-specific symbols.
    ///
    /// ```
    /// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///
    ///     let mut backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Four,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     assert!(backend.is_valid());
    ///
    ///     let topology: GlobalTopology<2> = GlobalTopology::Toroidal {
    ///         domain: [1.0, 1.0],
    ///         mode: ToroidalConstructionMode::Explicit,
    ///     };
    ///     assert!(matches!(topology, GlobalTopology::Toroidal { .. }));
    ///
    ///     let error = backend.insert_vertex(&[0.0]).expect_err("coordinate dimension is invalid");
    ///     assert!(matches!(error, DelaunayError::CoordinateDimensionMismatch { .. }));
    ///     Ok(())
    /// }
    /// ```
    pub mod geometry {
        pub use crate::geometry::DelaunayBackend2D;
        pub use crate::geometry::backends::delaunay::{
            DelaunayBackend, DelaunayError, DelaunayOperation, NonFlippableEdgeReason,
        };
        pub use crate::geometry::generators::{
            GlobalTopology, TopologyGuarantee, ToroidalConstructionMode,
            build_delaunay2_from_simplices, build_delaunay2_with_data,
            build_delaunay2_with_topology, build_periodic_toroidal_delaunay2,
            build_toroidal_delaunay2, generate_delaunay2,
        };
        pub use crate::geometry::operations::TriangulationOps;
        pub use crate::geometry::traits::{
            EdgeAdjacentFaces, EdgeAdjacentFacesResult, FlipResult, GeometryBackend,
            SubdivisionResult, TriangulationMut, TriangulationQuery,
        };
    }

    /// Focused exports for tests and documentation fixtures.
    ///
    /// This prelude exposes the mock geometry backend and the traits commonly
    /// exercised by downstream tests without mixing fixture-only types into the
    /// production geometry prelude.
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// assert_eq!(backend.vertex_count(), 3);
    /// ```
    pub mod testing {
        pub use crate::geometry::backends::mock::{
            MockBackend, MockError, MockNonFlippableReason, MockOperation, MockStorageTarget,
        };
        pub use crate::geometry::operations::TriangulationOps;
        pub use crate::geometry::traits::{TriangulationMut, TriangulationQuery};
    }
}

/// Runs a CDT simulation with the specified configuration.
///
/// This function uses the trait-based geometry backend system, which provides
/// better abstraction and testability compared to legacy approaches.
/// Open-boundary runs construct a foliated strip; toroidal runs construct a
/// periodic mesh. When [`CdtConfig::volume_profile`] is present, the initial
/// geometry uses those explicit per-slice spatial volumes. Otherwise the run
/// uses regular equal-size slices derived from the total [`CdtConfig::vertices`]
/// count and [`CdtConfig::timeslices`].
///
/// # Arguments
///
/// * `config` - Configuration parameters for the triangulation/simulation
///
/// # Returns
///
/// A [`SimulationResultsBackend`] value containing the simulation telemetry,
/// measurements, and final triangulation snapshot.
///
/// # Errors
///
/// Returns [`CdtError::InvalidConfiguration`] if geometry, topology, action, or
/// initial-volume configuration fails validation, including unsupported
/// dimensions, unsupported open-boundary or toroidal slice counts, invalid
/// regular slice counts, or inconsistent explicit spatial volume profiles.
/// Returns [`CdtError::InvalidSimulationConfiguration`] if the Metropolis
/// temperature or measurement schedule is invalid.
/// Returns triangulation generation, topology, foliation, or Metropolis errors
/// from the selected construction and simulation path.
/// If [`CdtConfig::output_csv`] or [`CdtConfig::output_json`] is set, returns
/// [`CdtError::OutputPathResolutionFailed`] if the current working directory
/// cannot be resolved. Returns [`CdtError::OutputPathConflict`] if CSV and JSON
/// outputs resolve to the same file. Returns [`CdtError::OutputWriteFailed`] if
/// the configured output file, parent directory creation, or JSON serialization
/// fails.
///
/// # Examples
///
/// ```
/// use causal_triangulations::{CdtConfig, CdtResult, run_simulation};
///
/// fn main() -> CdtResult<()> {
///     let config = CdtConfig {
///         steps: 1,
///         thermalization_steps: 0,
///         measurement_frequency: 1,
///         seed: Some(7),
///         simulate: false,
///         ..CdtConfig::new(8, 2)
///     };
///     let results = run_simulation(&config)?;
///     assert_eq!(results.measurements().len(), 1);
///     Ok(())
/// }
/// ```
pub fn run_simulation(config: &CdtConfig) -> CdtResult<SimulationResultsBackend> {
    let config = config.clone().into_validated()?;

    let vertices = config.vertices();
    let timeslices = config.timeslices();

    log::info!("Dimensionality: {}", config.dimension());
    log::info!("Number of vertices: {vertices}");
    log::info!("Number of timeslices: {timeslices}");
    if let Some(profile) = config.volume_profile() {
        log::info!("Initial spatial volume profile: {profile:?}");
    }
    log::info!("Topology: {:?}", config.topology());
    log::info!("Using trait-based backend system");

    // Create initial triangulation from the validated topology/profile matrix.
    let triangulation = match (config.topology(), config.initial_volume()) {
        (CdtTopology::Toroidal, ValidatedInitialVolume::ExplicitProfile(profile)) => {
            log::info!("Constructing toroidal CDT (S¹×S¹)");
            CdtTriangulation::from_toroidal_cdt_profile(profile)?
        }
        (CdtTopology::Toroidal, ValidatedInitialVolume::Regular { vertices_per_slice }) => {
            log::info!("Constructing toroidal CDT (S¹×S¹)");
            CdtTriangulation::from_toroidal_cdt(vertices_per_slice, timeslices)?
        }
        (CdtTopology::OpenBoundary, ValidatedInitialVolume::ExplicitProfile(profile)) => {
            log::info!("Constructing open-boundary CDT strip");
            CdtTriangulation::from_cdt_strip_profile(profile)?
        }
        (CdtTopology::OpenBoundary, ValidatedInitialVolume::Regular { vertices_per_slice }) => {
            log::info!("Constructing open-boundary CDT strip");
            CdtTriangulation::from_cdt_strip(vertices_per_slice, timeslices)?
        }
    };

    log::info!(
        "Triangulation created with {} vertices, {} edges, {} faces",
        triangulation.vertex_count(),
        triangulation.edge_count(),
        triangulation.face_count()
    );

    let results = if config.simulate() {
        // Run full CDT simulation with MCMC backend
        let metropolis_config = config.to_metropolis_config();
        let action_config = config.to_action_config();

        let algorithm = MetropolisAlgorithm::new(metropolis_config, action_config);
        let results = algorithm.run(triangulation)?;

        log::info!("Simulation Results:");
        log::info!(
            "  Acceptance rate: {:.2}%",
            results.acceptance_rate() * 100.0
        );
        log::info!("  Average action: {:.3}", results.average_action());

        results
    } else {
        // Just return basic simulation results with the triangulation
        let vertices = saturating_usize_to_u32(triangulation.vertex_count());
        let edges = saturating_usize_to_u32(triangulation.edge_count());
        let triangles = saturating_usize_to_u32(triangulation.face_count());
        let action_config = config.to_action_config();
        let initial_action = action_config.calculate_action(vertices, edges, triangles);

        SimulationResultsBackend::from_parts(SimulationResultsParts {
            config: config.to_metropolis_config(),
            action_config,
            move_stats: MoveStatistics::new(),
            proposal_stats: ProposalStatistics::new(),
            steps: vec![],
            measurements: vec![Measurement {
                step: 0,
                action: initial_action,
                vertices,
                edges,
                triangles,
                volume_profile: triangulation.volume_profile(),
            }],
            elapsed_time: Duration::from_millis(0),
            triangulation,
        })
    };

    write_configured_outputs(&config, &results)?;
    Ok(results)
}

/// Writes configured result outputs after a run completes.
fn write_configured_outputs(
    validated_config: &ValidatedCdtConfig,
    results: &SimulationResultsBackend,
) -> CdtResult<()> {
    if validated_config.output_csv().is_none() && validated_config.output_json().is_none() {
        return Ok(());
    }

    let base_dir = env::current_dir().map_err(|err| CdtError::OutputPathResolutionFailed {
        base_path: ".".to_string(),
        detail: err.to_string(),
    })?;

    let resolved_csv = validated_config
        .output_csv()
        .map(|path| CdtConfig::resolve_path(&base_dir, path));
    let resolved_json = validated_config
        .output_json()
        .map(|path| CdtConfig::resolve_path(&base_dir, path));

    if let (Some(csv_path), Some(json_path)) = (&resolved_csv, &resolved_json)
        && csv_path == json_path
    {
        return Err(CdtError::OutputPathConflict {
            csv_path: csv_path.display().to_string(),
            json_path: json_path.display().to_string(),
        });
    }

    if let Some(resolved) = resolved_csv {
        results.write_measurements_csv(&resolved)?;
        log::info!("Wrote measurement CSV to {}", resolved.display());
    }

    if let Some(resolved) = resolved_json {
        results.write_summary_json(validated_config.config(), &resolved)?;
        log::info!("Wrote simulation JSON summary to {}", resolved.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use serde_json::{Value, from_str};
    use std::assert_matches;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::thread;

    fn create_test_config() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 36,
            timeslices: 3,
            volume_profile: None,
            temperature: 1.0,
            steps: 10,
            thermalization_steps: 5,
            measurement_frequency: 2,
            coupling_0: 0.0,
            coupling_2: 0.0,
            cosmological_constant: crate::cdt::action::DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT,
            simulate: false,
            seed: Some(42),
            topology: CdtTopology::OpenBoundary,
            output_csv: None,
            output_json: None,
        }
    }

    fn temp_output_path(name: &str) -> PathBuf {
        let thread_name = safe_thread_name();
        env::temp_dir().join(format!(
            "causal-triangulations-run-{name}-{}-{}",
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
    fn test_run_simulation() {
        let config = create_test_config();
        assert!(config.dimension.is_some());
        let results = run_simulation(&config).expect("Failed to run triangulation");
        assert!(results.triangulation().face_count() > 0);
        assert!(results.triangulation().has_foliation());
        assert_eq!(results.triangulation().slice_sizes(), &[12, 12, 12]);
        assert!(!results.triangulation().volume_profile().is_empty());
        results
            .triangulation()
            .validate_foliation()
            .expect("open-boundary run should build a valid foliation");
        results
            .triangulation()
            .validate_causality()
            .expect("open-boundary run should preserve adjacent-slice causality");
        results
            .triangulation()
            .validate_simplex_classification()
            .expect("open-boundary run should classify CDT simplices");
        assert!(!results.measurements().is_empty());
    }

    #[test]
    fn triangulation_contains_triangles() {
        let config = create_test_config();
        let results = run_simulation(&config).expect("Failed to run triangulation");
        // Check that we have some triangles
        assert!(results.triangulation().face_count() > 0);
    }

    #[test]
    fn run_simulation_writes_configured_outputs() {
        let csv_path = temp_output_path("measurements.csv");
        let json_path = temp_output_path("summary.json");
        let mut config = create_test_config();
        config.output_csv = Some(csv_path.clone());
        config.output_json = Some(json_path.clone());

        run_simulation(&config).expect("configured outputs should write");

        let csv = fs::read_to_string(&csv_path).expect("CSV output should be readable");
        let json = fs::read_to_string(&json_path).expect("JSON output should be readable");
        fs::remove_file(&csv_path).expect("temporary CSV output should be removable");
        fs::remove_file(&json_path).expect("temporary JSON output should be removable");
        let parsed: Value = from_str(&json).expect("JSON output should parse");

        assert!(csv.starts_with("step,action,vertices,edges,triangles,accepted,delta_action\n"));
        assert_eq!(parsed["config"]["vertices"], config.vertices);
        assert_eq!(
            parsed["final_triangulation"]["time_slices"],
            config.timeslices
        );
    }

    #[test]
    fn run_simulation_rejects_overlapping_output_paths() {
        let path = temp_output_path("shared-output");
        let mut config = create_test_config();
        config.output_csv = Some(path.clone());
        config.output_json = Some(path.clone());

        let error = run_simulation(&config).expect_err("overlapping outputs should fail");

        let CdtError::OutputPathConflict {
            csv_path,
            json_path,
        } = error
        else {
            panic!("expected output path conflict error");
        };
        assert_eq!(csv_path, path.display().to_string());
        assert_eq!(json_path, path.display().to_string());
        assert!(!path.exists());
    }

    #[test]
    fn test_config_validation_invalid_measurement_frequency() {
        let mut config = create_test_config();
        config.measurement_frequency = 0;

        let result = run_simulation(&config);
        assert!(result.is_err(), "Should reject zero measurement frequency");

        if let Err(CdtError::InvalidSimulationConfiguration {
            setting,
            provided_value,
            expected,
        }) = result
        {
            assert_eq!(setting, ConfigurationSetting::MeasurementFrequency);
            assert_eq!(provided_value, "0");
            assert_eq!(expected, "≥ 1");
        } else {
            panic!("Expected InvalidSimulationConfiguration error");
        }
    }

    #[test]
    fn test_config_validation_measurement_frequency_too_large() {
        let mut config = create_test_config();
        config.steps = 100;
        config.measurement_frequency = 200; // Greater than steps

        let result = run_simulation(&config);
        assert!(
            result.is_err(),
            "Should reject measurement frequency greater than steps"
        );

        if let Err(CdtError::InvalidSimulationConfiguration {
            setting,
            provided_value,
            expected,
        }) = result
        {
            assert_eq!(setting, ConfigurationSetting::MeasurementFrequency);
            assert_eq!(provided_value, "200");
            assert_eq!(expected, "≤ steps (100)");
        } else {
            panic!("Expected InvalidSimulationConfiguration error");
        }
    }

    #[test]
    fn test_config_validation_invalid_vertices() {
        let mut config = create_test_config();
        config.vertices = 2; // Less than minimum of 3

        let result = run_simulation(&config);
        assert!(result.is_err(), "Should reject too few vertices");

        if let Err(CdtError::InvalidConfiguration {
            setting,
            provided_value,
            expected,
        }) = result
        {
            assert_eq!(setting, ConfigurationSetting::Vertices);
            assert_eq!(provided_value, "2");
            assert_eq!(expected, "≥ 3");
        } else {
            panic!("Expected InvalidConfiguration error");
        }
    }

    #[test]
    fn test_config_validation_negative_temperature() {
        let mut config = create_test_config();
        config.temperature = -1.0;

        let result = run_simulation(&config);
        assert!(result.is_err(), "Should reject negative temperature");

        if let Err(CdtError::InvalidSimulationConfiguration {
            setting,
            provided_value,
            expected,
        }) = result
        {
            assert_eq!(setting, ConfigurationSetting::Temperature);
            assert_eq!(provided_value, "-1");
            assert_eq!(expected, "finite and positive");
        } else {
            panic!("Expected InvalidSimulationConfiguration error");
        }
    }

    #[test]
    fn test_run_simulation_with_real_moves() {
        let mut config = create_test_config();
        config.simulate = true;

        let results = run_simulation(&config).expect("simulation should run with real moves");
        assert_eq!(
            results.steps().len(),
            usize::try_from(config.steps).unwrap()
        );
        assert!(results.triangulation().has_foliation());
        results
            .triangulation()
            .validate_foliation()
            .expect("simulated open-boundary run should keep valid foliation");
        results
            .triangulation()
            .validate_causality()
            .expect("simulated open-boundary run should keep adjacent-slice causality");
        results
            .triangulation()
            .validate_simplex_classification()
            .expect("simulated open-boundary run should keep CDT simplex classification");
        assert!(!results.measurements().is_empty());
    }

    #[test]
    fn test_config_conversions() {
        let config = create_test_config()
            .into_validated()
            .expect("test config should validate");

        let metropolis_config = config.to_metropolis_config();
        assert_relative_eq!(metropolis_config.temperature, 1.0);
        assert_eq!(metropolis_config.steps, 10);

        let action_config = config.to_action_config();
        assert_relative_eq!(action_config.coupling_0, 0.0);
        assert_relative_eq!(action_config.coupling_2, 0.0);
        assert_relative_eq!(
            action_config.cosmological_constant,
            crate::cdt::action::DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT
        );
    }

    #[test]
    fn test_run_simulation_toroidal_uses_total_vertex_count() {
        // For toroidal topology `config.vertices` is the *total* vertex
        // count.  With vertices=12, timeslices=3 we expect a triangulation
        // with exactly 12 vertices (4 per slice on a 3-slice torus), not
        // 36 (which would result from treating the field as per-slice).
        let config = CdtConfig {
            dimension: Some(2),
            vertices: 12,
            timeslices: 3,
            volume_profile: None,
            temperature: 1.0,
            steps: 10,
            thermalization_steps: 5,
            measurement_frequency: 2,
            coupling_0: 0.0,
            coupling_2: 0.0,
            cosmological_constant: crate::cdt::action::DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT,
            simulate: false,
            seed: None,
            topology: CdtTopology::Toroidal,
            output_csv: None,
            output_json: None,
        };

        let results = run_simulation(&config).expect("toroidal simulation should run");
        assert_eq!(
            results.triangulation().vertex_count(),
            12,
            "Toroidal run_simulation must treat config.vertices as the TOTAL vertex count"
        );
        assert_eq!(
            results.triangulation().time_slices(),
            3,
            "Toroidal run_simulation must preserve the configured timeslice count"
        );
        assert_matches!(
            results.triangulation().metadata().topology,
            CdtTopology::Toroidal
        );
    }

    #[test]
    fn test_run_simulation_uses_nonuniform_volume_profile() {
        let config = CdtConfig {
            vertices: 15,
            timeslices: 3,
            volume_profile: Some(vec![4, 6, 5]),
            steps: 4,
            thermalization_steps: 0,
            measurement_frequency: 1,
            ..create_test_config()
        };

        let results = run_simulation(&config).expect("profile-based simulation should run");

        assert_eq!(results.triangulation().vertex_count(), 15);
        assert_eq!(results.triangulation().slice_sizes(), &[4, 6, 5]);
        assert_eq!(results.measurements()[0].volume_profile.len(), 3);
        results
            .triangulation()
            .validate()
            .expect("profile-based initial CDT should satisfy evolved invariants");
    }

    #[test]
    fn test_run_simulation_uses_nonuniform_toroidal_volume_profile() {
        let config = CdtConfig {
            vertices: 16,
            timeslices: 4,
            topology: CdtTopology::Toroidal,
            volume_profile: Some(vec![3, 4, 5, 4]),
            steps: 4,
            thermalization_steps: 0,
            measurement_frequency: 1,
            ..create_test_config()
        };

        let results =
            run_simulation(&config).expect("toroidal profile-based simulation should run");

        assert_eq!(results.triangulation().vertex_count(), 16);
        assert_eq!(results.triangulation().slice_sizes(), &[3, 4, 5, 4]);
        assert_matches!(
            results.triangulation().metadata().topology,
            CdtTopology::Toroidal
        );
        results
            .triangulation()
            .validate()
            .expect("toroidal profile-based initial CDT should satisfy evolved invariants");
    }
}
