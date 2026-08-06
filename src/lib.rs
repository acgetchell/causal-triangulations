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
//! - Trace CSV/JSON simulation output and resumable serde-backed CDT/MCMC checkpoints
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
    /// Crate-owned coordinate parsers and invariant-bearing coordinate types.
    pub mod coordinates;
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

    pub use coordinates::{
        SpacetimeCoordinate, SpacetimeCoordinateComponent, SpacetimeCoordinateError,
    };
    pub use generators::{
        DelaunayTriangulation2D, GlobalTopology, TopologyGuarantee, ToroidalConstructionMode,
    };
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
    /// planned proposal types used through `markov-chain-monte-carlo`,
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
        pub use markov_chain_monte_carlo::{
            ChainId, StepOutcome, Trace, TraceError, TraceRecord, TraceStepOutcome,
        };
        pub use runner::{MetropolisAlgorithm, MetropolisConfig};
        pub use telemetry::{
            AcceptedStepTelemetry, MonteCarloStep, MonteCarloStepOutcome, ProposalStatistics,
            RejectedProposalStepTelemetry,
        };
    }
    /// User-facing CDT observable estimators.
    pub mod observables;
    /// Borrowed proposal-policy inspection for validated 1+1 CDT states.
    pub mod proposal_policy;
    /// Simulation result containers and measurement summaries.
    pub mod results;
    /// CDT triangulation state.
    #[path = "triangulation/state.rs"]
    pub mod triangulation;
}

// Re-exports for convenience
pub use cdt::action::{
    ActionConfig, CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT,
    DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT, compute_regge_action,
};
pub use cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
pub use cdt::foliation::{
    EdgeType, Foliation, FoliationError, SimplexType, classify_edge, classify_simplex,
};
pub use cdt::metropolis::{
    AcceptedStepTelemetry, CdtMcmcCheckpoint, CdtProposal, CdtProposalError, CdtProposalInfo,
    CdtProposalPlan, CdtTarget, MetropolisAlgorithm, MetropolisConfig, MonteCarloStep,
    MonteCarloStepOutcome, ProposalStatistics, RejectedProposalStepTelemetry, StepOutcome,
};
pub use cdt::observables::{estimate_hausdorff_dimension, estimate_spectral_dimension};
pub use cdt::proposal_policy::{
    CdtProposalPolicyView, CdtProposalSiteId, CdtProposalSiteIdError, CdtProposalSiteIds,
};
pub use cdt::results::{Measurement, SimulationResultsBackend};
pub use cdt::triangulation::{
    CdtMetadata, CdtSimplexCounts, CdtTriangulation, CdtValidationProfile, SimulationEvent,
};
pub use config::{
    CdtConfig, CdtConfigOverrides, CdtTopology, DimensionOverride, TestConfig, ValidatedCdtConfig,
    ValidatedInitialVolume,
};
pub use errors::{
    BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure,
    CheckpointMoveCounter, CheckpointOperation, CheckpointResumeFailure, ConfigurationSetting,
    DelaunayValidationLevel, GenerationParameterIssue, MeasurementCountField,
    MetropolisMoveApplicationFailure, OutputFormat, ProposalTelemetryCounter, ScalarTraceField,
    SimplexCountField, TriangulationMetadataField,
};
pub use geometry::traits::TriangulationQuery;
pub use geometry::{SpacetimeCoordinate, SpacetimeCoordinateComponent, SpacetimeCoordinateError};

use crate::cdt::results::SimulationResultsParts;
use std::env;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_TEMP_OUTPUT_ID: AtomicU64 = AtomicU64::new(0);

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
    pub use crate::geometry::CdtTriangulation2D;
    pub use crate::geometry::traits::TriangulationQuery;
    pub use crate::{CdtMetadata, CdtSimplexCounts, CdtTriangulation};

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
    /// use std::assert_matches;
    /// use causal_triangulations::prelude::errors::{
    ///     BackendMutationOperation, CdtError, CdtValidationCheck,
    ///     CdtValidationFailure, FoliationError,
    ///     MetropolisMoveApplicationFailure,
    ///     SpacetimeCoordinateComponent,
    /// };
    /// use causal_triangulations::prelude::moves::MoveType;
    ///
    /// let foliation_err = CdtError::Foliation(FoliationError::EmptyFoliation);
    /// assert_matches!(
    ///     foliation_err,
    ///     CdtError::Foliation(FoliationError::EmptyFoliation)
    /// );
    /// let coordinate_err = CdtError::ValidationFailed {
    ///     check: CdtValidationCheck::Geometry,
    ///     failure: CdtValidationFailure::VertexCoordinateNonFinite {
    ///         vertex: "VertexKey(7v1)".to_string(),
    ///         component: SpacetimeCoordinateComponent::Space,
    ///         value: "NaN".to_string(),
    ///     },
    /// };
    /// assert_matches!(
    ///     coordinate_err,
    ///     CdtError::ValidationFailed {
    ///         failure: CdtValidationFailure::VertexCoordinateNonFinite {
    ///             component: SpacetimeCoordinateComponent::Space,
    ///             ..
    ///         },
    ///         ..
    ///     }
    /// );
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
        pub use crate::cdt::foliation::FoliationError;
        pub use crate::errors::{
            BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck,
            CdtValidationFailure, CheckpointMoveCounter, CheckpointOperation,
            CheckpointResumeFailure, ConfigurationSetting, DelaunayValidationLevel,
            GenerationParameterIssue, MeasurementCountField, MetropolisMoveApplicationFailure,
            OutputFormat, ProposalTelemetryCounter, ScalarTraceField, SimplexCountField,
            TriangulationMetadataField,
        };
        pub use crate::geometry::SpacetimeCoordinateComponent;
    }

    /// Focused exports for CDT action calculations.
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::*;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2)?;
    /// let action = config.calculate_action(5, 10, 8);
    /// assert_relative_eq!(action, -20.0, epsilon = 1e-12);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub mod action {
        pub use crate::cdt::action::{
            ActionConfig, CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT,
            DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT, compute_regge_action,
        };
    }

    /// Focused exports for CDT triangulation construction, queries, classification, and history events.
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
        pub use crate::cdt::foliation::{
            EdgeType, Foliation, FoliationError, SimplexType, classify_edge, classify_simplex,
        };
        pub use crate::config::CdtTopology;
        pub use crate::errors::{CdtError, CdtResult};
        pub use crate::geometry::CdtTriangulation2D;
        pub use crate::geometry::traits::TriangulationQuery;
        pub use crate::{
            CdtMetadata, CdtSimplexCounts, CdtTriangulation, CdtValidationProfile, SimulationEvent,
        };
    }

    /// Focused exports for local CDT move kernels and move statistics.
    ///
    /// This prelude intentionally contains only the move API. Combine it with
    /// `prelude::triangulation::*` and, for explicit Delaunay fixtures,
    /// `prelude::geometry::*`. Proposal-policy inspection types belong to
    /// [`crate::prelude::simulation`].
    ///
    /// ```
    /// use causal_triangulations::prelude::moves::*;
    ///
    /// let mut stats = MoveStatistics::new();
    /// stats.record_attempt(MoveType::Move22);
    /// assert_eq!(stats.attempted(MoveType::Move22), 1);
    /// ```
    pub mod moves {
        pub use crate::cdt::ergodic_moves::{ErgodicsSystem, MoveResult, MoveStatistics, MoveType};
    }

    /// Focused exports for running CDT simulations.
    ///
    /// This prelude includes [`run_simulation`], validated simulation
    /// configuration, the Metropolis runner, proposal-plan adapter, telemetry
    /// structs, result containers, and typed proposal errors needed by MCMC
    /// workflows. It also includes the triangulation query trait so callers can
    /// inspect final or checkpointed states returned by simulation APIs.
    /// Upstream MCMC traits, step outcomes, and trace/checkpoint types are
    /// re-exported here because
    /// [`CdtProposal`](crate::cdt::metropolis::CdtProposal) and
    /// [`CdtTarget`](crate::cdt::metropolis::CdtTarget) expose their primary
    /// behavior through those trait implementations, while
    /// [`SimulationResultsBackend::scalar_trace`](crate::cdt::results::SimulationResultsBackend::scalar_trace)
    /// returns an upstream [`Trace`](markov_chain_monte_carlo::Trace).
    /// Observable estimators live in [`crate::prelude::observables`].
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtConfig, CdtResult, ValidatedCdtConfig,
    /// };
    ///
    /// fn configured_steps(config: ValidatedCdtConfig) -> u32 {
    ///     config.to_metropolis_config().steps().get()
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
            AcceptedStepTelemetry, CdtMcmcCheckpoint, CdtProposal, CdtProposalError,
            CdtProposalInfo, CdtProposalPlan, CdtTarget, MetropolisAlgorithm, MetropolisConfig,
            MonteCarloStep, MonteCarloStepOutcome, ProposalStatistics,
            RejectedProposalStepTelemetry,
        };
        pub use crate::cdt::proposal_policy::{
            CdtProposalPolicyView, CdtProposalSiteId, CdtProposalSiteIdError, CdtProposalSiteIds,
        };
        pub use crate::cdt::results::{Measurement, SimulationResultsBackend};
        pub use crate::cdt::triangulation::SimulationEvent;
        pub use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
        pub use crate::errors::{CdtError, CdtResult};
        pub use crate::geometry::CdtTriangulation2D;
        pub use crate::geometry::traits::TriangulationQuery;
        pub use crate::{CdtSimplexCounts, CdtTriangulation, run_simulation};
        pub use markov_chain_monte_carlo::{
            ChainCheckpoint, ChainId, DelayedProposal, StepOutcome, Target, Trace, TraceError,
            TraceRecord, TraceStepOutcome,
        };
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
    ///     let profile = tri.volume_profile()?;
    ///
    ///     assert_eq!(profile.len(), 3);
    ///     assert!(estimate_hausdorff_dimension(&tri)?.is_some_and(f64::is_finite));
    ///     assert!(estimate_spectral_dimension(&tri)?.is_some_and(f64::is_finite));
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
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    /// use std::assert_matches;
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
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     assert!(backend.is_valid());
    ///
    ///     let topology: GlobalTopology<2> = GlobalTopology::Toroidal {
    ///         domain: ToroidalDomain::unit(),
    ///         mode: ToroidalConstructionMode::Explicit,
    ///     };
    ///     assert_matches!(topology, GlobalTopology::Toroidal { .. });
    ///
    ///     let error = backend.insert_vertex(&[0.0]).expect_err("coordinate dimension is invalid");
    ///     assert_matches!(error, DelaunayError::CoordinateDimensionMismatch { .. });
    ///     Ok(())
    /// }
    /// ```
    pub mod geometry {
        pub use crate::errors::DelaunayValidationLevel;
        pub use crate::geometry::backends::delaunay::{
            DelaunayBackend, DelaunayError, DelaunayOperation, NonFlippableEdgeReason,
        };
        pub use crate::geometry::generators::{
            GlobalTopology, TopologyGuarantee, ToroidalConstructionMode, ToroidalDomain,
            build_delaunay2_from_simplices, build_delaunay2_with_data,
            build_delaunay2_with_topology, build_periodic_toroidal_delaunay2,
            build_toroidal_delaunay2, generate_delaunay2,
        };
        pub use crate::geometry::operations::TriangulationOps;
        pub use crate::geometry::traits::{
            EdgeAdjacentFaces, EdgeAdjacentFacesResult, FlipResult, GeometryBackend,
            SubdivisionResult, TriangulationMut, TriangulationQuery,
        };
        pub use crate::geometry::{DelaunayBackend2D, DelaunayTriangulation2D};
        pub use crate::geometry::{
            SpacetimeCoordinate, SpacetimeCoordinateComponent, SpacetimeCoordinateError,
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

/// Runs a CDT simulation with an already validated configuration.
///
/// This function uses the trait-based geometry backend system, which provides
/// better abstraction and testability compared to legacy approaches.
/// Open-boundary runs construct a foliated strip; toroidal runs construct a
/// periodic mesh. When [`ValidatedCdtConfig::volume_profile`] is present, the initial
/// geometry uses those explicit per-slice spatial volumes. Otherwise the run
/// uses regular equal-size slices derived from the total
/// [`ValidatedCdtConfig::vertices`] count and [`ValidatedCdtConfig::timeslices`].
///
/// # Arguments
///
/// * `config` - Validated configuration parameters for the triangulation/simulation
///
/// # Returns
///
/// A [`SimulationResultsBackend`] value containing the simulation telemetry,
/// measurements, and final triangulation snapshot.
///
/// # Errors
///
/// The raw configuration has already been parsed into [`ValidatedCdtConfig`], so
/// this function does not report raw configuration validation failures. It can
/// still return triangulation generation, topology, foliation, or Metropolis
/// errors from the selected construction and simulation path.
/// If [`ValidatedCdtConfig::output_csv`] or [`ValidatedCdtConfig::output_json`] is set, returns
/// [`CdtError::OutputPathResolutionFailed`] if the current working directory
/// cannot be resolved. Returns [`CdtError::OutputPathConflict`] if CSV and JSON
/// outputs resolve to the same file. Output path resolution and conflict checks
/// happen before triangulation construction or sampling begins. Returns
/// [`CdtError::OutputWriteFailed`] if the configured output file, parent
/// directory creation, or JSON serialization fails after the run completes.
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
///     }
///     .into_validated()?;
///     let results = run_simulation(&config)?;
///     assert_eq!(results.measurements().len(), 1);
///     Ok(())
/// }
/// ```
pub fn run_simulation(config: &ValidatedCdtConfig) -> CdtResult<SimulationResultsBackend> {
    let output_paths = resolve_configured_output_paths(config)?;
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
            let profile: Vec<_> = profile.iter().map(|volume| volume.get()).collect();
            CdtTriangulation::from_toroidal_cdt_profile(&profile)?
        }
        (CdtTopology::Toroidal, ValidatedInitialVolume::Regular { vertices_per_slice }) => {
            log::info!("Constructing toroidal CDT (S¹×S¹)");
            CdtTriangulation::from_toroidal_cdt(vertices_per_slice.get(), timeslices.get())?
        }
        (CdtTopology::OpenBoundary, ValidatedInitialVolume::ExplicitProfile(profile)) => {
            log::info!("Constructing open-boundary CDT strip");
            let profile: Vec<_> = profile.iter().map(|volume| volume.get()).collect();
            CdtTriangulation::from_cdt_strip_profile(&profile)?
        }
        (CdtTopology::OpenBoundary, ValidatedInitialVolume::Regular { vertices_per_slice }) => {
            log::info!("Constructing open-boundary CDT strip");
            CdtTriangulation::from_cdt_strip(vertices_per_slice.get(), timeslices.get())?
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
        let counts = triangulation.simplex_counts()?;
        let action_config = config.to_action_config();
        let initial_action = action_config.calculate_action(
            counts.vertex_count(),
            counts.edge_count(),
            counts.triangle_count(),
        );

        let measurement = Measurement::try_from_simplex_counts(0, initial_action, counts)?
            .try_with_volume_profile(triangulation.volume_profile()?)?;

        SimulationResultsBackend::from_parts(SimulationResultsParts::new(
            config.to_metropolis_config(),
            action_config,
            MoveStatistics::new(),
            ProposalStatistics::new(),
            vec![],
            vec![measurement],
            vec![],
            Duration::from_millis(0),
            triangulation,
        )?)
    };

    write_configured_outputs(config, &results, &output_paths)?;
    Ok(results)
}

struct ResolvedOutputPaths {
    csv: Option<PathBuf>,
    json: Option<PathBuf>,
}

/// Resolves configured output paths before expensive triangulation or sampling work begins.
fn resolve_configured_output_paths(
    validated_config: &ValidatedCdtConfig,
) -> CdtResult<ResolvedOutputPaths> {
    if validated_config.output_csv().is_none() && validated_config.output_json().is_none() {
        return Ok(ResolvedOutputPaths {
            csv: None,
            json: None,
        });
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

    Ok(ResolvedOutputPaths {
        csv: resolved_csv,
        json: resolved_json,
    })
}

/// Writes configured result outputs after a run completes.
fn write_configured_outputs(
    validated_config: &ValidatedCdtConfig,
    results: &SimulationResultsBackend,
    output_paths: &ResolvedOutputPaths,
) -> CdtResult<()> {
    let staged_outputs = StagedOutputs::new(output_paths);
    staged_outputs.write_trace_csv(results)?;
    staged_outputs.write_summary_json(validated_config, results)?;
    staged_outputs.commit()?;

    if let Some(resolved) = &output_paths.csv {
        log::info!("Wrote trace CSV to {}", resolved.display());
    }

    if let Some(resolved) = &output_paths.json {
        log::info!("Wrote simulation JSON summary to {}", resolved.display());
    }

    Ok(())
}

/// Owns staged simulation outputs until they are committed or cleaned up.
struct StagedOutputs<'a> {
    csv: Option<StagedOutput<'a>>,
    json: Option<StagedOutput<'a>>,
}

impl<'a> StagedOutputs<'a> {
    /// Builds staged output paths next to their configured final destinations.
    fn new(output_paths: &'a ResolvedOutputPaths) -> Self {
        Self {
            csv: output_paths
                .csv
                .as_deref()
                .map(|path| StagedOutput::new(path, OutputFormat::Csv)),
            json: output_paths
                .json
                .as_deref()
                .map(|path| StagedOutput::new(path, OutputFormat::Json)),
        }
    }

    /// Writes the CSV output to its staged file when configured.
    fn write_trace_csv(&self, results: &SimulationResultsBackend) -> CdtResult<()> {
        if let Some(output) = &self.csv {
            results
                .write_trace_csv(&output.temp_path)
                .map_err(|err| output.remap_error(err))?;
        }
        Ok(())
    }

    /// Writes the JSON output to its staged file when configured.
    fn write_summary_json(
        &self,
        validated_config: &ValidatedCdtConfig,
        results: &SimulationResultsBackend,
    ) -> CdtResult<()> {
        if let Some(output) = &self.json {
            results
                .write_summary_json(validated_config, &output.temp_path)
                .map_err(|err| output.remap_error(err))?;
        }
        Ok(())
    }

    /// Publishes every staged output, rolling back an earlier CSV publish on JSON failure.
    fn commit(mut self) -> CdtResult<()> {
        let published_csv = if let Some(output) = &self.csv {
            output.persist()?;
            let final_path = output.final_path.to_path_buf();
            self.csv = None;
            Some(final_path)
        } else {
            None
        };

        if let Some(output) = &self.json
            && let Err(err) = output.persist()
        {
            if let Some(path) = &published_csv {
                let _ignored = fs::remove_file(path);
            }
            return Err(err);
        }
        self.json = None;
        Ok(())
    }
}

impl Drop for StagedOutputs<'_> {
    fn drop(&mut self) {
        if let Some(output) = &self.csv {
            output.cleanup();
        }
        if let Some(output) = &self.json {
            output.cleanup();
        }
    }
}

/// One configured output file staged through a sibling temporary path.
struct StagedOutput<'a> {
    final_path: &'a Path,
    temp_path: PathBuf,
    format: OutputFormat,
}

impl<'a> StagedOutput<'a> {
    /// Builds one same-directory temporary output path.
    fn new(final_path: &'a Path, format: OutputFormat) -> Self {
        Self {
            final_path,
            temp_path: sibling_temp_output_path(final_path, format),
            format,
        }
    }

    /// Renames the staged output into its final path.
    fn persist(&self) -> CdtResult<()> {
        fs::rename(&self.temp_path, self.final_path)
            .map_err(|err| output_write_failed(self.final_path, self.format, err))
    }

    /// Removes the staged temporary file if it still exists.
    fn cleanup(&self) {
        let _ignored = fs::remove_file(&self.temp_path);
    }

    /// Reports a staged write failure against the configured final path.
    fn remap_error(&self, error: CdtError) -> CdtError {
        match error {
            CdtError::OutputWriteFailed { detail, .. } => CdtError::OutputWriteFailed {
                path: self.final_path.display().to_string(),
                format: self.format,
                detail,
            },
            error => error,
        }
    }
}

/// Builds a same-directory temporary output path for later atomic rename.
fn sibling_temp_output_path(path: &Path, format: OutputFormat) -> PathBuf {
    let suffix = match format {
        OutputFormat::Csv => "csv",
        OutputFormat::Json => "json",
    };
    let token = NEXT_TEMP_OUTPUT_ID.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map_or_else(|| "output".into(), |name| name.to_string_lossy());
    path.with_file_name(format!(
        ".{file_name}.{}.{}.{}.tmp",
        process::id(),
        token,
        suffix
    ))
}

/// Builds a typed output write error for crate-root persistence helpers.
fn output_write_failed(path: &Path, format: OutputFormat, err: impl Display) -> CdtError {
    CdtError::OutputWriteFailed {
        path: path.display().to_string(),
        format,
        detail: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::action::DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT;
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
            cosmological_constant: DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT,
            simulate: false,
            seed: Some(42),
            topology: CdtTopology::OpenBoundary,
            output_csv: None,
            output_json: None,
        }
    }

    fn validated(config: CdtConfig) -> ValidatedCdtConfig {
        config
            .into_validated()
            .expect("test config should validate")
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
        let config = validated(create_test_config());
        assert_eq!(config.dimension(), 2);
        let results = run_simulation(&config).expect("Failed to run triangulation");
        assert!(results.triangulation().face_count() > 0);
        assert!(results.triangulation().has_foliation());
        assert_eq!(results.triangulation().slice_sizes(), &[12, 12, 12]);
        assert!(
            !results
                .triangulation()
                .volume_profile()
                .expect("run triangulation should have a valid volume profile")
                .is_empty()
        );
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
    fn construction_only_results_roundtrip_through_serde() {
        let config = validated(create_test_config());
        let results = run_simulation(&config).expect("construction-only run should succeed");
        assert!(results.steps().is_empty());
        assert_eq!(
            results.measurements().first().map(Measurement::step),
            Some(0)
        );

        let json = serde_json::to_string(&results).expect("construction snapshot should serialize");
        let roundtrip: SimulationResultsBackend =
            from_str(&json).expect("construction snapshot should deserialize");

        assert!(roundtrip.steps().is_empty());
        assert_eq!(
            roundtrip.measurements().first().map(Measurement::step),
            Some(0)
        );
        assert_eq!(roundtrip.triangulation().slice_sizes(), &[12, 12, 12]);
    }

    #[test]
    fn triangulation_contains_triangles() {
        let config = validated(create_test_config());
        let results = run_simulation(&config).expect("Failed to run triangulation");
        // Check that we have some triangles
        assert!(results.triangulation().face_count() > 0);
    }

    #[test]
    fn run_simulation_writes_configured_outputs() {
        let csv_path = temp_output_path("trace.csv");
        let json_path = temp_output_path("summary.json");
        let mut config = create_test_config();
        config.output_csv = Some(csv_path.clone());
        config.output_json = Some(json_path.clone());
        let config = validated(config);

        run_simulation(&config).expect("configured outputs should write");

        let csv = fs::read_to_string(&csv_path).expect("CSV output should be readable");
        let json = fs::read_to_string(&json_path).expect("JSON output should be readable");
        fs::remove_file(&csv_path).expect("temporary CSV output should be removable");
        fs::remove_file(&json_path).expect("temporary JSON output should be removable");
        let parsed: Value = from_str(&json).expect("JSON output should parse");

        assert!(csv.starts_with(
            "chain_id,step,accepted,proposed,log_prob,action,vertices,edges,triangles,move_family"
        ));
        assert_eq!(parsed["config"]["vertices"], config.vertices().get());
        assert_eq!(
            parsed["final_triangulation"]["time_slices"],
            config.timeslices().get()
        );
    }

    #[test]
    fn run_simulation_cleans_staged_csv_when_json_write_fails() {
        let csv_path = temp_output_path("atomic-trace.csv");
        let blocked_parent = temp_output_path("blocked-output-parent");
        fs::write(&blocked_parent, "not a directory").expect("blocked parent fixture should write");
        let json_path = blocked_parent.join("summary.json");
        let csv_temp = sibling_temp_output_path(&csv_path, OutputFormat::Csv);
        let json_temp = sibling_temp_output_path(&json_path, OutputFormat::Json);
        let mut config = create_test_config();
        config.output_csv = Some(csv_path.clone());
        config.output_json = Some(json_path.clone());
        let config = validated(config);

        let error =
            run_simulation(&config).expect_err("JSON output with a missing parent should fail");

        assert_matches!(
            error,
            CdtError::OutputWriteFailed {
                format: OutputFormat::Json,
                ..
            }
        );
        assert!(
            !csv_path.exists(),
            "CSV final output should not be published"
        );
        assert!(
            !json_path.exists(),
            "JSON final output should not be published"
        );
        assert!(!csv_temp.exists(), "staged CSV output should be cleaned");
        assert!(!json_temp.exists(), "staged JSON output should be absent");
        fs::remove_file(&blocked_parent).expect("blocked parent fixture should be removable");
    }

    #[test]
    fn sibling_temp_output_path_is_unique_per_call() {
        let output_path = temp_output_path("unique-trace.csv");

        let first = sibling_temp_output_path(&output_path, OutputFormat::Csv);
        let second = sibling_temp_output_path(&output_path, OutputFormat::Csv);

        assert_ne!(first, second);
    }

    #[test]
    fn staged_outputs_roll_back_published_csv_when_json_persist_fails() {
        let csv_path = temp_output_path("rollback-trace.csv");
        let json_path = temp_output_path("rollback-summary.json");
        let output_paths = ResolvedOutputPaths {
            csv: Some(csv_path.clone()),
            json: Some(json_path.clone()),
        };
        let staged_outputs = StagedOutputs::new(&output_paths);
        let csv_temp = staged_outputs
            .csv
            .as_ref()
            .expect("CSV output should be staged")
            .temp_path
            .clone();
        let json_temp = staged_outputs
            .json
            .as_ref()
            .expect("JSON output should be staged")
            .temp_path
            .clone();
        fs::write(&csv_temp, "trace").expect("staged CSV fixture should write");
        fs::write(&json_temp, "{}").expect("staged JSON fixture should write");
        fs::create_dir(&json_path).expect("JSON final path directory should block rename");

        let error = staged_outputs
            .commit()
            .expect_err("JSON persist failure should roll back already-published CSV");

        assert_matches!(
            error,
            CdtError::OutputWriteFailed {
                format: OutputFormat::Json,
                ..
            }
        );
        assert!(
            !csv_path.exists(),
            "CSV final output should be removed after JSON publish failure"
        );
        assert!(!csv_temp.exists(), "staged CSV should have been renamed");
        assert!(
            !json_temp.exists(),
            "staged JSON should be cleaned when commit fails"
        );
        fs::remove_dir(&json_path).expect("blocking JSON directory should be removable");
    }

    #[test]
    fn run_simulation_rejects_overlapping_output_paths() {
        let path = temp_output_path("shared-output");
        let mut config = create_test_config();
        config.output_csv = Some(path.clone());
        config.output_json = Some(path.clone());
        let config = validated(config);

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

        let result = config.into_validated();
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

        let result = config.into_validated();
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

        let result = config.into_validated();
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

        let result = config.into_validated();
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
        let config = validated(config);

        let results = run_simulation(&config).expect("simulation should run with real moves");
        assert_eq!(
            results.steps().len(),
            usize::try_from(config.to_metropolis_config().steps().get()).unwrap()
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
        assert_relative_eq!(metropolis_config.temperature(), 1.0);
        assert_eq!(metropolis_config.steps().get(), 10);

        let action_config = config.to_action_config();
        assert_relative_eq!(action_config.coupling_0(), 0.0);
        assert_relative_eq!(action_config.coupling_2(), 0.0);
        assert_relative_eq!(
            action_config.cosmological_constant(),
            DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT
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
            cosmological_constant: DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT,
            simulate: false,
            seed: None,
            topology: CdtTopology::Toroidal,
            output_csv: None,
            output_json: None,
        };
        let config = validated(config);

        let results = run_simulation(&config).expect("toroidal simulation should run");
        assert_eq!(
            results.triangulation().vertex_count(),
            12,
            "Toroidal run_simulation must treat config.vertices as the TOTAL vertex count"
        );
        assert_eq!(
            results.triangulation().time_slices().get(),
            3,
            "Toroidal run_simulation must preserve the configured timeslice count"
        );
        assert_matches!(
            results.triangulation().metadata().topology(),
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
        let config = validated(config);

        let results = run_simulation(&config).expect("profile-based simulation should run");

        assert_eq!(results.triangulation().vertex_count(), 15);
        assert_eq!(results.triangulation().slice_sizes(), &[4, 6, 5]);
        assert_eq!(results.measurements()[0].volume_profile().len(), 3);
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
        let config = validated(config);

        let results =
            run_simulation(&config).expect("toroidal profile-based simulation should run");

        assert_eq!(results.triangulation().vertex_count(), 16);
        assert_eq!(results.triangulation().slice_sizes(), &[3, 4, 5, 4]);
        assert_matches!(
            results.triangulation().metadata().topology(),
            CdtTopology::Toroidal
        );
        results
            .triangulation()
            .validate()
            .expect("toroidal profile-based initial CDT should satisfy evolved invariants");
    }
}
