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
//! - Slab-triangle profiles and finite-graph effective dimensional observables
//!   for CDT analysis
//! - Trace CSV/JSON simulation output and resumable serde-backed CDT/MCMC checkpoints
//!
//! The crate root re-exports the most common construction, simulation,
//! observable, and error types. Focused preludes under [`prelude`] provide
//! smaller import surfaces for documentation, examples, integration tests, and
//! benchmarks.
//!
//! # Feature flags
//!
//! - `slow-tests` enables long-running validation tests used by repository
//!   development commands. It does not change the library or `cdt` binary API
//!   and is not needed for normal use.
//!
//! # Checkpointing
//!
//! Durable simulation continuation uses the versioned, CDT-owned
//! [`CdtMcmcCheckpoint`] JSON format. Version 1 stores dependency-neutral geometry,
//! chain accounting, telemetry, elapsed time, and both RNG streams; restoration
//! rebuilds transient caches and validates geometry and CDT invariants before use.
//! Direct Serde serialization of a standalone [`CdtTriangulation2D`] remains an
//! implementation-shaped same-build facility rather than this compatibility contract.
//!
//! ```
//! use causal_triangulations::prelude::simulation::*;
//!
//! fn main() -> CdtResult<()> {
//!     let checkpoint = MetropolisAlgorithm::new(
//!         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(13),
//!         ActionConfig::default(),
//!     )
//!     .run_to_checkpoint(CdtTriangulation::from_cdt_strip(4, 3)?)?;
//!     let json = checkpoint.to_json()?;
//!     let restored = CdtMcmcCheckpoint::from_json(&json)?;
//!     assert_eq!(CdtMcmcCheckpoint::FORMAT_VERSION, 1);
//!     assert_eq!(restored.current_step().get(), 1);
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

/// Crate-private numeric conversion helpers.
mod util;

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

    pub use coordinates::{
        SpacetimeCoordinate, SpacetimeCoordinateComponent, SpacetimeCoordinateError,
    };
    pub use generators::{
        DelaunayTriangulation2D, GlobalTopology, TopologyGuarantee, ToroidalConstructionMode,
        ToroidalDomain,
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
            AcceptedStepTelemetry, CdtProposalPlanningOutcome, MonteCarloStep,
            MonteCarloStepOutcome, ProposalKernelTelemetry, ProposalStatistics,
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
    CdtProposalPlan, CdtProposalPlanningOutcome, CdtTarget, MetropolisAlgorithm, MetropolisConfig,
    MonteCarloStep, MonteCarloStepOutcome, ProposalKernelTelemetry, ProposalStatistics,
    RejectedProposalStepTelemetry, StepOutcome,
};
pub use cdt::observables::{
    average_dual_ball_volume_curve, average_dual_return_probability_curve,
    estimate_all_scale_effective_hausdorff_slope, estimate_short_time_effective_spectral_dimension,
};
pub use cdt::proposal_policy::{
    CdtMoveFamilyDistribution, CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError,
    CdtProposalPolicyView, CdtProposalSiteId, CdtProposalSiteIdError, CdtProposalSiteIds,
    UniformCdtMoveFamilyPolicy,
};
pub use cdt::results::{Measurement, SimulationEvent, SimulationHistory, SimulationResultsBackend};
pub use cdt::triangulation::{
    CdtMetadata, CdtSimplexCounts, CdtTriangulation, CdtTriangulation2D, CdtValidationProfile,
};
pub use config::{
    CdtConfig, CdtConfigOverrides, CdtTopology, DimensionOverride, ValidatedCdtConfig,
    ValidatedInitialSpatialVertices,
};
pub use errors::{
    BackendMutationOperation, BackendRollbackFailure, BackendRollbackFailures, CdtError, CdtResult,
    CdtValidationCheck, CdtValidationFailure, CheckpointMoveCounter, CheckpointOperation,
    CheckpointResumeFailure, ConfigurationSetting, DelaunayGenerationFailure,
    DelaunayGenerationQuantity, DelaunayGenerationStage, DelaunayValidationLevel,
    GenerationParameterIssue, MeasurementCountField, MetropolisMoveApplicationFailure,
    ObservableQuantity, OutputFormat, OutputPreparationStage, OutputWriteStage,
    ProposalTelemetryCounter, ScalarTraceField, SimplexCountField, TriangulationMetadataField,
};
pub use geometry::traits::TriangulationQuery;
pub use geometry::{SpacetimeCoordinate, SpacetimeCoordinateComponent, SpacetimeCoordinateError};
pub use markov_chain_monte_carlo::{DiscreteProposalRatioError, McmcError};

use crate::cdt::results::{SimulationResultsParts, ensure_parent_directory};
use std::env;
use std::fmt::Display;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind};
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
    pub use crate::geometry::traits::TriangulationQuery;
    pub use crate::{CdtTriangulation, CdtTriangulation2D};

    // Action and simulation setup
    pub use crate::cdt::action::ActionConfig;
    pub use crate::cdt::metropolis::{MetropolisAlgorithm, MetropolisConfig};
    pub use crate::run_simulation;

    // Configuration and errors
    pub use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
    pub use crate::errors::{CdtError, CdtResult};

    /// Focused exports for configuration parsing and overrides.
    pub mod config {
        pub use crate::config::{
            CdtConfig, CdtConfigOverrides, CdtTopology, DimensionOverride, ValidatedCdtConfig,
            ValidatedInitialSpatialVertices,
        };
    }

    /// Focused exports for crate error handling.
    ///
    /// ```
    /// use std::assert_matches;
    /// use causal_triangulations::prelude::errors::{
    ///     BackendMutationOperation, CdtError, CdtValidationCheck,
    ///     CdtMoveFamilyPolicyError, CdtValidationFailure,
    ///     DiscreteProposalRatioError, FoliationError, McmcError,
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
    /// let policy_err = CdtError::ProposalPolicyFailed {
    ///     source: CdtMoveFamilyPolicyError::EmptySupport,
    /// };
    /// assert_matches!(
    ///     policy_err,
    ///     CdtError::ProposalPolicyFailed {
    ///         source: CdtMoveFamilyPolicyError::EmptySupport,
    ///     }
    /// );
    /// let ratio_err = CdtError::ProposalRatioFailed {
    ///     move_type: MoveType::Move22,
    ///     source: DiscreteProposalRatioError::ZeroForwardSiteCount,
    /// };
    /// assert_matches!(
    ///     ratio_err,
    ///     CdtError::ProposalRatioFailed {
    ///         source: DiscreteProposalRatioError::ZeroForwardSiteCount,
    ///         ..
    ///     }
    /// );
    /// let mcmc_err = CdtError::Mcmc(McmcError::NanProposedLogProb);
    /// assert_matches!(mcmc_err, CdtError::Mcmc(McmcError::NanProposedLogProb));
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
        pub use crate::cdt::proposal_policy::CdtMoveFamilyPolicyError;
        pub use crate::errors::{
            BackendMutationOperation, BackendRollbackFailure, BackendRollbackFailures, CdtError,
            CdtResult, CdtValidationCheck, CdtValidationFailure, CheckpointMoveCounter,
            CheckpointOperation, CheckpointResumeFailure, ConfigurationSetting,
            DelaunayGenerationFailure, DelaunayGenerationQuantity, DelaunayGenerationStage,
            DelaunayValidationLevel, GenerationParameterIssue, MeasurementCountField,
            MetropolisMoveApplicationFailure, ObservableQuantity, OutputFormat,
            OutputPreparationStage, OutputWriteStage, ProposalTelemetryCounter, ScalarTraceField,
            SimplexCountField, TriangulationMetadataField,
        };
        pub use crate::geometry::SpacetimeCoordinateComponent;
        pub use markov_chain_monte_carlo::{DiscreteProposalRatioError, McmcError};
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

    /// Focused exports for CDT triangulation construction, queries, and classification.
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
        pub use crate::geometry::traits::TriangulationQuery;
        pub use crate::{
            CdtMetadata, CdtSimplexCounts, CdtTriangulation, CdtTriangulation2D,
            CdtValidationProfile,
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
            CdtProposalInfo, CdtProposalPlan, CdtProposalPlanningOutcome, CdtTarget,
            MetropolisAlgorithm, MetropolisConfig, MonteCarloStep, MonteCarloStepOutcome,
            ProposalKernelTelemetry, ProposalStatistics, RejectedProposalStepTelemetry,
        };
        pub use crate::cdt::proposal_policy::{
            CdtMoveFamilyDistribution, CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError,
            CdtProposalPolicyView, CdtProposalSiteId, CdtProposalSiteIdError, CdtProposalSiteIds,
            UniformCdtMoveFamilyPolicy,
        };
        pub use crate::cdt::results::{
            Measurement, SimulationEvent, SimulationHistory, SimulationResultsBackend,
        };
        pub use crate::config::{CdtConfig, CdtTopology, ValidatedCdtConfig};
        pub use crate::errors::{CdtError, CdtResult};
        pub use crate::geometry::traits::TriangulationQuery;
        pub use crate::{CdtSimplexCounts, CdtTriangulation, CdtTriangulation2D, run_simulation};
        pub use markov_chain_monte_carlo::{
            ChainCheckpoint, ChainId, DelayedProposal, DiscreteProposalRatioError, McmcError,
            StepOutcome, Target, Trace, TraceError, TraceRecord, TraceStepOutcome,
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
    ///     let profile = tri.slab_triangle_profile()?;
    ///
    ///     assert_eq!(profile.len(), 3);
    ///     assert!(estimate_all_scale_effective_hausdorff_slope(&tri)?.is_some_and(f64::is_finite));
    ///     assert!(estimate_short_time_effective_spectral_dimension(&tri)?.is_some_and(f64::is_finite));
    ///     Ok(())
    /// }
    /// ```
    pub mod observables {
        pub use crate::cdt::observables::{
            average_dual_ball_volume_curve, average_dual_return_probability_curve,
            estimate_all_scale_effective_hausdorff_slope,
            estimate_short_time_effective_spectral_dimension,
        };
        pub use crate::{CdtTriangulation, CdtTriangulation2D};
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
            DelaunayBackend, DelaunayError, DelaunayFlipOutputFailure, DelaunayOperation,
            NonFlippableEdgeReason,
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
    /// This prelude exposes canned test configurations, the mock geometry
    /// backend, and the traits commonly exercised by downstream tests without
    /// mixing fixture-only types into production preludes.
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let config = TestConfig::small();
    /// assert_eq!(config.steps, 10);
    /// let backend = MockBackend::create_triangle();
    /// assert_eq!(backend.vertex_count(), 3);
    /// ```
    pub mod testing {
        pub use crate::config::TestConfig;
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
/// periodic mesh. When [`ValidatedCdtConfig::spatial_vertex_profile`] is present, the initial
/// geometry uses those explicit per-slice spatial-vertex counts. Otherwise the run
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
/// [`CdtError::OutputPathBusy`] if another simulation owns either configured
/// output destination. Output locks are acquired before triangulation
/// construction and held through publication of all configured outputs. Lock or
/// parent-directory I/O failures return [`CdtError::OutputWriteFailed`] before
/// construction begins. Returns
/// [`CdtError::OutputPreparationFailed`] if trace or mesh preparation fails
/// after the run completes. Returns [`CdtError::OutputWriteFailed`] if configured
/// output-file creation, serialization, or staged publication fails.
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
    let output_locks = OutputPathLocks::acquire(&output_paths)?;
    let vertices = config.vertices();
    let timeslices = config.timeslices();

    log::info!("Dimensionality: {}", config.dimension());
    log::info!("Number of vertices: {vertices}");
    log::info!("Number of timeslices: {timeslices}");
    if let Some(profile) = config.spatial_vertex_profile() {
        log::info!("Initial spatial vertex profile: {profile:?}");
    }
    log::info!("Topology: {:?}", config.topology());
    log::info!("Using trait-based backend system");

    // Create initial triangulation from the validated topology/profile matrix.
    let triangulation = match (config.topology(), config.initial_spatial_vertices()) {
        (CdtTopology::Toroidal, ValidatedInitialSpatialVertices::ExplicitProfile(profile)) => {
            log::info!("Constructing toroidal CDT (S¹×S¹)");
            let profile: Vec<_> = profile.iter().map(|volume| volume.get()).collect();
            CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(&profile)?
        }
        (
            CdtTopology::Toroidal,
            ValidatedInitialSpatialVertices::Regular { vertices_per_slice },
        ) => {
            log::info!("Constructing toroidal CDT (S¹×S¹)");
            CdtTriangulation::from_toroidal_cdt(vertices_per_slice.get(), timeslices.get())?
        }
        (CdtTopology::OpenBoundary, ValidatedInitialSpatialVertices::ExplicitProfile(profile)) => {
            log::info!("Constructing open-boundary CDT strip");
            let profile: Vec<_> = profile.iter().map(|volume| volume.get()).collect();
            CdtTriangulation::from_cdt_strip_spatial_vertex_profile(&profile)?
        }
        (
            CdtTopology::OpenBoundary,
            ValidatedInitialSpatialVertices::Regular { vertices_per_slice },
        ) => {
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
            .try_with_slab_triangle_profile(triangulation.slab_triangle_profile()?)?;

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
    drop(output_locks);
    Ok(results)
}

struct ResolvedOutputPaths {
    csv: Option<PathBuf>,
    json: Option<PathBuf>,
}

/// Holds exclusive operating-system locks for every configured output destination.
struct OutputPathLocks {
    _files: Vec<File>,
}

impl OutputPathLocks {
    /// Acquires every configured destination lock in stable path order.
    fn acquire(output_paths: &ResolvedOutputPaths) -> CdtResult<Self> {
        let mut destinations = Vec::with_capacity(2);
        if let Some(path) = output_paths.csv.as_deref() {
            destinations.push((path, OutputFormat::Csv));
        }
        if let Some(path) = output_paths.json.as_deref() {
            destinations.push((path, OutputFormat::Json));
        }
        destinations.sort_unstable_by_key(|(path, _)| *path);

        let mut files = Vec::with_capacity(destinations.len());
        for (path, format) in destinations {
            ensure_parent_directory(path, format)?;
            let lock_path = sibling_output_lock_path(path);
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .map_err(|err| {
                    output_write_failed(path, format, OutputWriteStage::AcquireLock, err)
                })?;
            match file.try_lock() {
                Ok(()) => files.push(file),
                Err(TryLockError::WouldBlock) => {
                    return Err(CdtError::OutputPathBusy {
                        path: path.display().to_string(),
                        format,
                    });
                }
                Err(TryLockError::Error(err)) => {
                    return Err(output_write_failed(
                        path,
                        format,
                        OutputWriteStage::AcquireLock,
                        err,
                    ));
                }
            }
        }

        Ok(Self { _files: files })
    }
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

    /// Publishes every staged output while preserving any previous destination set.
    fn commit(mut self) -> CdtResult<()> {
        for output in self.csv.iter().chain(&self.json) {
            output.validate_destination()?;
        }
        for output in self.csv.iter_mut().chain(&mut self.json) {
            if let Err(error) = output.back_up_existing() {
                let rollback_failures = self.rollback();
                return Err(attach_output_rollback_failures(error, &rollback_failures));
            }
        }
        for output in self.csv.iter_mut().chain(&mut self.json) {
            if let Err(error) = output.persist() {
                let rollback_failures = self.rollback();
                return Err(attach_output_rollback_failures(error, &rollback_failures));
            }
        }
        for output in self.csv.iter_mut().chain(&mut self.json) {
            output.finalize();
        }
        Ok(())
    }

    /// Restores every destination touched by a partial commit.
    fn rollback(&mut self) -> Vec<String> {
        self.csv
            .iter_mut()
            .chain(&mut self.json)
            .flat_map(StagedOutput::rollback)
            .collect()
    }
}

impl Drop for StagedOutputs<'_> {
    fn drop(&mut self) {
        for output in self.csv.iter_mut().chain(&mut self.json) {
            for failure in output.rollback() {
                log::error!("failed to restore staged output during cleanup: {failure}");
            }
            output.cleanup();
        }
    }
}

/// One configured output file staged through a sibling temporary path.
struct StagedOutput<'a> {
    final_path: &'a Path,
    temp_path: PathBuf,
    backup_path: PathBuf,
    format: OutputFormat,
    backup_created: bool,
    published: bool,
}

impl<'a> StagedOutput<'a> {
    /// Builds one same-directory temporary output path.
    fn new(final_path: &'a Path, format: OutputFormat) -> Self {
        Self {
            final_path,
            temp_path: sibling_temp_output_path(final_path, format),
            backup_path: sibling_backup_output_path(final_path, format),
            format,
            backup_created: false,
            published: false,
        }
    }

    /// Rejects destination types that cannot participate in file replacement.
    fn validate_destination(&self) -> CdtResult<()> {
        match fs::symlink_metadata(self.final_path) {
            Ok(metadata) if metadata.file_type().is_file() => Ok(()),
            Ok(_) => Err(output_write_failed(
                self.final_path,
                self.format,
                OutputWriteStage::ValidateDestination,
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "existing output destination is not a regular file",
                ),
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(output_write_failed(
                self.final_path,
                self.format,
                OutputWriteStage::ValidateDestination,
                error,
            )),
        }
    }

    /// Moves a previous regular output aside for failure-atomic replacement.
    fn back_up_existing(&mut self) -> CdtResult<()> {
        if !self.final_path.exists() {
            return Ok(());
        }
        if self.backup_path.exists() {
            return Err(output_write_failed(
                self.final_path,
                self.format,
                OutputWriteStage::BackupExisting,
                io::Error::new(
                    ErrorKind::AlreadyExists,
                    format!("backup path {} already exists", self.backup_path.display()),
                ),
            ));
        }
        fs::rename(self.final_path, &self.backup_path).map_err(|error| {
            output_write_failed(
                self.final_path,
                self.format,
                OutputWriteStage::BackupExisting,
                error,
            )
        })?;
        self.backup_created = true;
        Ok(())
    }

    /// Renames the staged output into its final path.
    fn persist(&mut self) -> CdtResult<()> {
        fs::rename(&self.temp_path, self.final_path).map_err(|err| {
            output_write_failed(self.final_path, self.format, OutputWriteStage::Persist, err)
        })?;
        self.published = true;
        Ok(())
    }

    /// Restores the previous destination or removes a newly published output.
    fn rollback(&mut self) -> Vec<String> {
        let remove_failure = if self.published {
            match fs::remove_file(self.final_path) {
                Ok(()) => None,
                Err(error) if error.kind() == ErrorKind::NotFound => None,
                Err(error) => Some(format!(
                    "remove replacement {}: {error}",
                    self.final_path.display()
                )),
            }
        } else {
            None
        };
        self.published = false;

        if self.backup_created {
            match fs::rename(&self.backup_path, self.final_path) {
                Ok(()) => {
                    self.backup_created = false;
                    Vec::new()
                }
                Err(error) => {
                    let mut failures = Vec::with_capacity(2);
                    if let Some(remove_failure) = remove_failure {
                        failures.push(remove_failure);
                    }
                    failures.push(format!(
                        "restore backup {} to {}: {error}",
                        self.backup_path.display(),
                        self.final_path.display()
                    ));
                    failures
                }
            }
        } else {
            remove_failure.into_iter().collect()
        }
    }

    /// Finishes a successful commit and discards its no-longer-needed backup.
    fn finalize(&mut self) {
        self.published = false;
        if self.backup_created {
            if let Err(error) = fs::remove_file(&self.backup_path) {
                log::warn!(
                    "could not remove committed output backup {}: {error}",
                    self.backup_path.display()
                );
            }
            self.backup_created = false;
        }
    }

    /// Removes the staged temporary file if it still exists.
    fn cleanup(&self) {
        let _ignored = fs::remove_file(&self.temp_path);
    }

    /// Reports a staged write failure against the configured final path.
    fn remap_error(&self, error: CdtError) -> CdtError {
        match error {
            CdtError::OutputPreparationFailed { stage, detail, .. } => {
                CdtError::OutputPreparationFailed {
                    path: self.final_path.display().to_string(),
                    format: self.format,
                    stage,
                    detail,
                }
            }
            CdtError::OutputWriteFailed { stage, detail, .. } => CdtError::OutputWriteFailed {
                path: self.final_path.display().to_string(),
                format: self.format,
                stage,
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

/// Builds a unique sibling backup path used only during multi-output commit.
fn sibling_backup_output_path(path: &Path, format: OutputFormat) -> PathBuf {
    let suffix = match format {
        OutputFormat::Csv => "csv",
        OutputFormat::Json => "json",
    };
    let token = NEXT_TEMP_OUTPUT_ID.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map_or_else(|| "output".into(), |name| name.to_string_lossy());
    path.with_file_name(format!(
        ".{file_name}.{}.{}.{}.backup",
        process::id(),
        token,
        suffix
    ))
}

/// Retains the primary commit error while reporting any failed restoration work.
fn attach_output_rollback_failures(error: CdtError, failures: &[String]) -> CdtError {
    if failures.is_empty() {
        return error;
    }
    match error {
        CdtError::OutputWriteFailed {
            path,
            format,
            stage,
            detail,
        } => CdtError::OutputWriteFailed {
            path,
            format,
            stage,
            detail: format!("{detail}; output rollback failed: {}", failures.join("; ")),
        },
        error => error,
    }
}

/// Builds the stable sibling path used to coordinate writers of one output destination.
///
/// The empty coordination file intentionally persists after the operating-system
/// lock is released. Its presence never denotes ownership, and retaining the
/// path prevents concurrent writers from locking different file identities.
fn sibling_output_lock_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map_or_else(|| "output".into(), |name| name.to_string_lossy());
    path.with_file_name(format!(".{file_name}.causal-triangulations.lock"))
}

/// Builds a typed output write error for crate-root persistence helpers.
fn output_write_failed(
    path: &Path,
    format: OutputFormat,
    stage: OutputWriteStage,
    err: impl Display,
) -> CdtError {
    CdtError::OutputWriteFailed {
        path: path.display().to_string(),
        format,
        stage,
        detail: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::action::DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT;
    use serde_json::{Value, from_str, to_string};
    use std::assert_matches;
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn create_test_config() -> CdtConfig {
        CdtConfig {
            dimension: Some(2),
            vertices: 36,
            timeslices: 3,
            spatial_vertex_profile: None,
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

    /// Removes a persistent output coordination file created by a test.
    fn remove_output_lock(path: &Path) {
        let lock_path = sibling_output_lock_path(path);
        match fs::remove_file(&lock_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!(
                "output lock {} should be removable: {err}",
                lock_path.display()
            ),
        }
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
                .slab_triangle_profile()
                .expect("run triangulation should have a valid slab-triangle profile")
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

        let json = to_string(&results).expect("construction snapshot should serialize");
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
        remove_output_lock(&csv_path);
        remove_output_lock(&json_path);
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
    fn run_simulation_rejects_invalid_json_parent_before_staging() {
        let csv_path = temp_output_path("atomic-trace.csv");
        let blocked_parent = temp_output_path("blocked-output-parent");
        fs::write(&blocked_parent, "not a directory").expect("blocked parent fixture should write");
        let json_path = blocked_parent.join("summary.json");
        let mut config = create_test_config();
        config.output_csv = Some(csv_path.clone());
        config.output_json = Some(json_path.clone());
        let config = validated(config);

        let error = run_simulation(&config)
            .expect_err("JSON output with an invalid parent should fail before staging");

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
        remove_output_lock(&csv_path);
        fs::remove_file(&blocked_parent).expect("blocked parent fixture should be removable");
    }

    #[test]
    fn staged_outputs_clean_csv_after_json_write_failure() {
        let config = validated(create_test_config());
        let results = run_simulation(&config).expect("output fixture simulation should succeed");
        let directory = temp_output_path("staged-json-write-failure");
        fs::create_dir(&directory).expect("output fixture directory should be created");
        let csv_path = directory.join("trace.csv");
        let json_path = directory.join("summary.json");
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

        staged_outputs
            .write_trace_csv(&results)
            .expect("CSV staging should succeed before the JSON failure");
        fs::create_dir(&json_temp).expect("JSON staged path should block file creation");
        let error = staged_outputs
            .write_summary_json(&config, &results)
            .expect_err("JSON staging should fail when its staged path is a directory");

        assert_matches!(
            error,
            CdtError::OutputWriteFailed {
                format: OutputFormat::Json,
                stage: OutputWriteStage::CreateFile,
                ..
            }
        );
        assert!(
            csv_temp.exists(),
            "CSV should have been staged before failure"
        );
        drop(staged_outputs);
        assert!(
            !csv_temp.exists(),
            "staged CSV should be cleaned after failure"
        );
        assert!(
            !csv_path.exists(),
            "CSV final output should not be published"
        );
        assert!(
            !json_path.exists(),
            "JSON final output should not be published"
        );
        fs::remove_dir(&json_temp).expect("blocking JSON staged directory should be removable");
        fs::remove_dir(&directory).expect("output fixture directory should be removable");
    }

    #[test]
    fn sibling_temp_output_path_is_unique_across_threads() {
        const WORKERS: usize = 16;

        let output_path = temp_output_path("unique-trace.csv");
        let barrier = Arc::new(Barrier::new(WORKERS));
        #[expect(
            clippy::needless_collect,
            reason = "all barrier participants must be spawned before any worker is joined"
        )]
        let handles = (0..WORKERS)
            .map(|_| {
                let output_path = output_path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    sibling_temp_output_path(&output_path, OutputFormat::Csv)
                })
            })
            .collect::<Vec<_>>();

        let paths = handles
            .into_iter()
            .map(|handle| handle.join().expect("temporary-path worker should finish"))
            .collect::<HashSet<_>>();

        assert_eq!(paths.len(), WORKERS);
        assert!(
            paths
                .iter()
                .all(|path| path.parent() == output_path.parent()),
            "temporary paths should remain beside the final output"
        );
    }

    #[test]
    fn output_path_locks_exclude_other_threads_and_release_on_drop() {
        let directory = temp_output_path("exclusive-output-lock");
        let csv_path = directory.join("trace.csv");
        let output_paths = ResolvedOutputPaths {
            csv: Some(csv_path.clone()),
            json: None,
        };
        let owner = OutputPathLocks::acquire(&output_paths)
            .expect("first writer should acquire the output lock");
        let barrier = Arc::new(Barrier::new(2));
        let contender_barrier = Arc::clone(&barrier);
        let contender_path = csv_path.clone();
        let contender = thread::spawn(move || {
            let output_paths = ResolvedOutputPaths {
                csv: Some(contender_path),
                json: None,
            };
            contender_barrier.wait();
            OutputPathLocks::acquire(&output_paths)
        });

        barrier.wait();
        let Err(error) = contender.join().expect("lock contender should finish") else {
            panic!("another thread should not acquire a held output lock");
        };
        assert_matches!(
            error,
            CdtError::OutputPathBusy {
                path,
                format: OutputFormat::Csv,
            } if path == csv_path.display().to_string()
        );

        drop(owner);
        let reacquired = OutputPathLocks::acquire(&output_paths)
            .expect("output lock should be released when its owner is dropped");
        drop(reacquired);
        remove_output_lock(&csv_path);
        fs::remove_dir(&directory).expect("output lock fixture directory should be removable");
    }

    #[test]
    fn failed_multi_output_lock_releases_earlier_locks() {
        let directory = temp_output_path("partial-output-lock");
        let csv_path = directory.join("a-trace.csv");
        let json_path = directory.join("z-summary.json");
        let json_only = ResolvedOutputPaths {
            csv: None,
            json: Some(json_path.clone()),
        };
        let json_owner = OutputPathLocks::acquire(&json_only)
            .expect("first writer should acquire the JSON output lock");
        let both = ResolvedOutputPaths {
            csv: Some(csv_path.clone()),
            json: Some(json_path.clone()),
        };

        let Err(error) = OutputPathLocks::acquire(&both) else {
            panic!("multi-output acquisition should fail on the held JSON lock");
        };
        assert_matches!(
            error,
            CdtError::OutputPathBusy {
                path,
                format: OutputFormat::Json,
            } if path == json_path.display().to_string()
        );

        let csv_only = ResolvedOutputPaths {
            csv: Some(csv_path.clone()),
            json: None,
        };
        let csv_owner = OutputPathLocks::acquire(&csv_only)
            .expect("failed multi-output acquisition should release its earlier CSV lock");
        drop(csv_owner);
        drop(json_owner);
        let both_owner = OutputPathLocks::acquire(&both)
            .expect("both output locks should be available after their owners are dropped");
        drop(both_owner);

        remove_output_lock(&csv_path);
        remove_output_lock(&json_path);
        fs::remove_dir(&directory).expect("output lock fixture directory should be removable");
    }

    #[test]
    fn run_simulation_rejects_busy_output_destinations_before_writing() {
        let directory = temp_output_path("busy-simulation-output");
        let csv_path = directory.join("a-trace.csv");
        let json_path = directory.join("z-summary.json");
        let output_paths = ResolvedOutputPaths {
            csv: Some(csv_path.clone()),
            json: Some(json_path.clone()),
        };
        let owner = OutputPathLocks::acquire(&output_paths)
            .expect("fixture should own both output destinations");
        let mut config = create_test_config();
        config.output_csv = Some(csv_path.clone());
        config.output_json = Some(json_path.clone());
        let config = validated(config);

        let error = run_simulation(&config)
            .expect_err("a run should reject destinations owned by another run");

        assert_matches!(
            error,
            CdtError::OutputPathBusy {
                path,
                format: OutputFormat::Csv,
            } if path == csv_path.display().to_string()
        );
        assert!(!csv_path.exists(), "busy run should not publish CSV output");
        assert!(
            !json_path.exists(),
            "busy run should not publish JSON output"
        );

        drop(owner);
        remove_output_lock(&csv_path);
        remove_output_lock(&json_path);
        fs::remove_dir(&directory).expect("busy output fixture directory should be removable");
    }

    #[test]
    fn staged_outputs_reject_unsupported_destination_before_publish() {
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
                stage: OutputWriteStage::ValidateDestination,
                ..
            }
        );
        assert!(
            !csv_path.exists(),
            "CSV final output should not be published before destination validation"
        );
        assert!(
            !csv_temp.exists(),
            "staged CSV should be cleaned without being renamed"
        );
        assert!(
            !json_temp.exists(),
            "staged JSON should be cleaned when commit fails"
        );
        fs::remove_dir(&json_path).expect("blocking JSON directory should be removable");
    }

    #[test]
    fn staged_outputs_restore_existing_files_when_later_publish_fails() {
        let csv_path = temp_output_path("restore-trace.csv");
        let json_path = temp_output_path("restore-summary.json");
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
        let csv_backup = staged_outputs
            .csv
            .as_ref()
            .expect("CSV output should have a backup path")
            .backup_path
            .clone();
        let json_backup = staged_outputs
            .json
            .as_ref()
            .expect("JSON output should have a backup path")
            .backup_path
            .clone();
        fs::write(&csv_path, "previous trace").expect("previous CSV should write");
        fs::write(&json_path, "previous summary").expect("previous JSON should write");
        fs::write(&csv_temp, "replacement trace").expect("staged CSV should write");

        let error = staged_outputs
            .commit()
            .expect_err("missing staged JSON should fail after CSV publication");

        assert_matches!(
            error,
            CdtError::OutputWriteFailed {
                format: OutputFormat::Json,
                stage: OutputWriteStage::Persist,
                ..
            }
        );
        assert_eq!(
            fs::read_to_string(&csv_path).expect("previous CSV should be restored"),
            "previous trace"
        );
        assert_eq!(
            fs::read_to_string(&json_path).expect("previous JSON should be restored"),
            "previous summary"
        );
        assert!(
            !csv_temp.exists(),
            "published CSV staging path should be gone"
        );
        assert!(
            !json_temp.exists(),
            "missing JSON staging path should stay absent"
        );
        assert!(
            !csv_backup.exists(),
            "restored CSV backup should be consumed"
        );
        assert!(
            !json_backup.exists(),
            "restored JSON backup should be consumed"
        );
        fs::remove_file(&csv_path).expect("restored CSV fixture should be removable");
        fs::remove_file(&json_path).expect("restored JSON fixture should be removable");
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

        assert_matches!(
            config.into_validated(),
            Err(CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::MeasurementFrequency,
                ref provided_value,
                ref expected,
            }) if provided_value == "0" && expected == "≥ 1"
        );
    }

    #[test]
    fn test_config_validation_measurement_frequency_too_large() {
        let mut config = create_test_config();
        config.steps = 100;
        config.measurement_frequency = 200; // Greater than steps

        assert_matches!(
            config.into_validated(),
            Err(CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::MeasurementFrequency,
                ref provided_value,
                ref expected,
            }) if provided_value == "200" && expected == "≤ steps (100)"
        );
    }

    #[test]
    fn test_config_validation_invalid_vertices() {
        let mut config = create_test_config();
        config.vertices = 2; // Less than minimum of 3

        assert_matches!(
            config.into_validated(),
            Err(CdtError::InvalidConfiguration {
                setting: ConfigurationSetting::Vertices,
                ref provided_value,
                ref expected,
            }) if provided_value == "2" && expected == "≥ 3"
        );
    }

    #[test]
    fn test_config_validation_negative_temperature() {
        let mut config = create_test_config();
        config.temperature = -1.0;

        assert_matches!(
            config.into_validated(),
            Err(CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::Temperature,
                ref provided_value,
                ref expected,
            }) if provided_value == "-1"
                && expected == "finite and positive with a finite reciprocal"
        );
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
    fn test_run_simulation_toroidal_uses_total_vertex_count() {
        // For toroidal topology `config.vertices` is the *total* vertex
        // count.  With vertices=12, timeslices=3 we expect a triangulation
        // with exactly 12 vertices (4 per slice on a 3-slice torus), not
        // 36 (which would result from treating the field as per-slice).
        let config = CdtConfig {
            dimension: Some(2),
            vertices: 12,
            timeslices: 3,
            spatial_vertex_profile: None,
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
    fn test_run_simulation_uses_nonuniform_spatial_vertex_profile() {
        let config = CdtConfig {
            vertices: 15,
            timeslices: 3,
            spatial_vertex_profile: Some(vec![4, 6, 5]),
            steps: 4,
            thermalization_steps: 0,
            measurement_frequency: 1,
            ..create_test_config()
        };
        let config = validated(config);

        let results = run_simulation(&config).expect("profile-based simulation should run");

        assert_eq!(results.triangulation().vertex_count(), 15);
        assert_eq!(results.triangulation().slice_sizes(), &[4, 6, 5]);
        assert_eq!(results.measurements()[0].slab_triangle_profile().len(), 3);
        results
            .triangulation()
            .validate()
            .expect("profile-based initial CDT should satisfy evolved invariants");
    }

    #[test]
    fn test_run_simulation_uses_nonuniform_toroidal_spatial_vertex_profile() {
        let config = CdtConfig {
            vertices: 16,
            timeslices: 4,
            topology: CdtTopology::Toroidal,
            spatial_vertex_profile: Some(vec![3, 4, 5, 4]),
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
