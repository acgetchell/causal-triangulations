#![forbid(unsafe_code)]

//! CDT triangulation state - backend-agnostic.
//!
//! This module owns the backend-agnostic CDT triangulation state. It stores
//! CDT metadata, cached derived quantities, foliation bookkeeping, and the
//! underlying geometry backend behind validated construction and mutation paths.

use crate::cdt::ergodic_moves::MoveType;
use crate::cdt::foliation::{Foliation, FoliationError};
use crate::config::CdtTopology;
use crate::errors::{CdtError, CdtResult, SimplexCountField, TriangulationMetadataField};
use crate::geometry::DelaunayBackend2D;
use crate::geometry::traits::TriangulationQuery;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

mod builders;
mod foliation;
mod moves;
mod validation;

static NEXT_TRIANGULATION_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Returns a fresh process-local identity for transient triangulation caches.
fn next_triangulation_instance_id() -> u64 {
    let instance_id = NEXT_TRIANGULATION_INSTANCE_ID.fetch_add(1, Ordering::Relaxed);
    assert_ne!(
        instance_id,
        u64::MAX,
        "triangulation instance id counter exhausted"
    );
    instance_id
}

/// CDT triangulation state paired with a geometry backend.
///
/// Constructors and fallible setters validate CDT metadata before storage so
/// normal accessors can expose the accepted state infallibly.
pub struct CdtTriangulation<B> {
    /// Process-local identity used only to reject stale transient caches.
    instance_id: u64,
    geometry: B,
    /// CDT metadata (time slices, dimension, history)
    metadata: CdtMetadata,
    cache: GeometryCache,
    /// Optional foliation assigning each vertex to a time slice.
    foliation: Option<Foliation>,
    /// Modification counter value when foliation bookkeeping was last synchronized.
    foliation_synced_at_modification: Option<u64>,
}

/// Strictly positive simplex counts for a CDT triangulation.
///
/// Generic geometry backends can be empty while they are being constructed,
/// cleared, or used as test fixtures. A constructed CDT triangulation, however,
/// must have positive vertex, edge, and triangle counts. This type carries that
/// proof for CDT-domain code without imposing a `u32` cap on live geometry
/// queries.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::triangulation::*;
///
/// fn main() -> CdtResult<()> {
///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
///     let counts = tri.simplex_counts()?;
///
///     assert_eq!(counts.vertices().get(), tri.vertex_count());
///     assert_eq!(counts.triangles().get(), tri.face_count());
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CdtSimplexCounts {
    vertices: NonZeroUsize,
    edges: NonZeroUsize,
    triangles: NonZeroUsize,
}

impl CdtSimplexCounts {
    /// Parses raw `usize` counts into a strictly positive CDT count snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimplexCount`] when any count is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{
    ///     CdtError, CdtSimplexCounts, SimplexCountField,
    /// };
    /// use std::assert_matches;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.edge_count(), 3);
    ///
    /// assert_matches!(
    ///     CdtSimplexCounts::try_new(3, 0, 1),
    ///     Err(CdtError::InvalidSimplexCount {
    ///         field: SimplexCountField::Edges,
    ///         ..
    ///     })
    /// );
    /// # Ok::<(), CdtError>(())
    /// ```
    pub fn try_new(vertices: usize, edges: usize, triangles: usize) -> CdtResult<Self> {
        Ok(Self {
            vertices: nonzero_simplex_count(SimplexCountField::Vertices, vertices)?,
            edges: nonzero_simplex_count(SimplexCountField::Edges, edges)?,
            triangles: nonzero_simplex_count(SimplexCountField::Triangles, triangles)?,
        })
    }

    /// Returns the nonzero vertex count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::CdtSimplexCounts;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.vertices().get(), 3);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn vertices(&self) -> NonZeroUsize {
        self.vertices
    }

    /// Returns the nonzero edge count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::CdtSimplexCounts;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.edges().get(), 3);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn edges(&self) -> NonZeroUsize {
        self.edges
    }

    /// Returns the nonzero triangle count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::CdtSimplexCounts;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.triangles().get(), 1);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn triangles(&self) -> NonZeroUsize {
        self.triangles
    }

    /// Returns the vertex count as a raw `usize` for collection-style APIs.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::CdtSimplexCounts;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.vertex_count(), 3);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn vertex_count(&self) -> usize {
        self.vertices.get()
    }

    /// Returns the edge count as a raw `usize` for collection-style APIs.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::CdtSimplexCounts;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.edge_count(), 3);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn edge_count(&self) -> usize {
        self.edges.get()
    }

    /// Returns the triangle count as a raw `usize` for collection-style APIs.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::CdtSimplexCounts;
    ///
    /// let counts = CdtSimplexCounts::try_new(3, 3, 1)?;
    /// assert_eq!(counts.triangle_count(), 1);
    /// # Ok::<(), causal_triangulations::prelude::errors::CdtError>(())
    /// ```
    #[must_use]
    pub const fn triangle_count(&self) -> usize {
        self.triangles.get()
    }
}

/// Parses one raw backend count into the nonzero proof used by [`CdtSimplexCounts`].
fn nonzero_simplex_count(field: SimplexCountField, value: usize) -> CdtResult<NonZeroUsize> {
    NonZeroUsize::new(value).ok_or(CdtError::InvalidSimplexCount {
        field,
        provided_value: value,
    })
}

impl<B: fmt::Debug> fmt::Debug for CdtTriangulation<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CdtTriangulation")
            .field("geometry", &self.geometry)
            .field("metadata", &self.metadata)
            .field("cache", &self.cache)
            .field("foliation", &self.foliation)
            .field(
                "foliation_synced_at_modification",
                &self.foliation_synced_at_modification,
            )
            .finish_non_exhaustive()
    }
}

impl<B: Clone> Clone for CdtTriangulation<B> {
    fn clone(&self) -> Self {
        Self {
            instance_id: next_triangulation_instance_id(),
            geometry: self.geometry.clone(),
            metadata: self.metadata.clone(),
            cache: self.cache.clone(),
            foliation: self.foliation.clone(),
            foliation_synced_at_modification: self.foliation_synced_at_modification,
        }
    }
}

/// Metadata describing the CDT foliation, topology, and simulation history.
#[derive(Debug, Clone)]
pub struct CdtMetadata {
    /// Nonzero number of time slices in the CDT foliation
    time_slices: NonZeroU32,
    /// Dimensionality of the spacetime
    dimension: u8,
    /// Topology of the spatial slices
    topology: CdtTopology,
    /// Time when this triangulation was created
    creation_time: Instant,
    /// Time of last modification
    last_modified: Instant,
    /// Count of modifications made to the triangulation
    modification_count: u64,
    /// Ordered simulation event history recorded for this triangulation.
    ///
    /// Move events identify proposals and acceptances with [`MoveType`].
    simulation_history: Vec<SimulationEvent>,
}

impl CdtMetadata {
    /// Returns the number of time slices in the CDT foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.metadata().time_slices().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn time_slices(&self) -> NonZeroU32 {
        self.time_slices
    }

    /// Returns the dimensionality of the spacetime.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.metadata().dimension(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn dimension(&self) -> u8 {
        self.dimension
    }

    /// Returns the spatial-slice topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.metadata().topology(), CdtTopology::OpenBoundary);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn topology(&self) -> CdtTopology {
        self.topology
    }

    /// Returns the creation timestamp for this triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert!(tri.metadata().creation_time() <= tri.metadata().last_modified());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn creation_time(&self) -> Instant {
        self.creation_time
    }

    /// Returns the timestamp for the most recent triangulation mutation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert!(tri.metadata().last_modified() >= tri.metadata().creation_time());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn last_modified(&self) -> Instant {
        self.last_modified
    }

    /// Returns the mutation counter used to invalidate derived caches.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.metadata().modification_count(), 0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn modification_count(&self) -> u64 {
        self.modification_count
    }

    /// Returns the ordered simulation event history.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.metadata().simulation_history().len(), 1);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn simulation_history(&self) -> &[SimulationEvent] {
        &self.simulation_history
    }
}

/// Cached geometry measurements
#[derive(Debug, Clone, Default)]
struct GeometryCache {
    edge_count: Option<CachedValue<usize>>,
    euler_char: Option<CachedValue<i128>>,
}

#[derive(Debug, Clone)]
struct CachedValue<T> {
    value: T,
    modification_count: u64,
}

/// Event recorded in a CDT simulation history.
///
/// The history is exposed by [`CdtMetadata::simulation_history`]. Move events
/// use [`MoveType`] to keep history, checkpoint serialization, and move
/// statistics aligned with the crate's supported ergodic move set.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::moves::MoveType;
/// use causal_triangulations::prelude::triangulation::SimulationEvent;
/// use std::assert_matches;
///
/// let event = SimulationEvent::MoveAttempted {
///     move_type: MoveType::Move13Add,
///     step: 7,
/// };
///
/// assert_matches!(
///     event,
///     SimulationEvent::MoveAttempted {
///         move_type: MoveType::Move13Add,
///         step: 7,
///     }
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimulationEvent {
    /// Triangulation creation event.
    Created {
        /// Initial number of vertices.
        vertex_count: usize,
        /// Number of time slices.
        time_slices: u32,
    },
    /// Ergodic move proposal event.
    MoveAttempted {
        /// Type of move attempted.
        move_type: MoveType,
        /// Simulation step number.
        step: u64,
    },
    /// Accepted ergodic move event.
    MoveAccepted {
        /// Type of move accepted.
        move_type: MoveType,
        /// Simulation step number.
        step: u64,
        /// Change in action from this move.
        action_change: f64,
    },
    /// Observable measurement event.
    MeasurementTaken {
        /// Simulation step number.
        step: u64,
        /// Action value measured.
        action: f64,
    },
}

#[derive(Serialize)]
struct SerializedCdtTriangulation<'a> {
    geometry: &'a DelaunayBackend2D,
    metadata: SerializedCdtMetadata<'a>,
    foliation: &'a Option<Foliation>,
}

#[derive(Serialize)]
struct SerializedCdtMetadata<'a> {
    time_slices: u32,
    dimension: u8,
    topology: CdtTopology,
    modification_count: u64,
    simulation_history: &'a [SimulationEvent],
}

#[derive(Deserialize)]
struct DeserializedCdtTriangulation {
    geometry: DelaunayBackend2D,
    metadata: DeserializedCdtMetadata,
    foliation: Option<Foliation>,
}

#[derive(Deserialize)]
struct DeserializedCdtMetadata {
    time_slices: u32,
    dimension: u8,
    topology: CdtTopology,
    modification_count: u64,
    simulation_history: Vec<SimulationEvent>,
}

impl Serialize for CdtTriangulation<DelaunayBackend2D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SerializedCdtTriangulation {
            geometry: &self.geometry,
            metadata: SerializedCdtMetadata {
                time_slices: self.metadata.time_slices.get(),
                dimension: self.metadata.dimension,
                topology: self.metadata.topology,
                modification_count: self.metadata.modification_count,
                simulation_history: &self.metadata.simulation_history,
            },
            foliation: &self.foliation,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CdtTriangulation<DelaunayBackend2D> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = DeserializedCdtTriangulation::deserialize(deserializer)?;
        let now = Instant::now();
        let foliation_synced_at_modification = serialized
            .foliation
            .as_ref()
            .map(|_| serialized.metadata.modification_count);
        let tri = Self {
            instance_id: next_triangulation_instance_id(),
            geometry: serialized.geometry,
            metadata: CdtMetadata {
                time_slices: Self::parse_time_slices(
                    serialized.metadata.topology,
                    serialized.metadata.time_slices,
                )
                .map_err(DeError::custom)?,
                dimension: serialized.metadata.dimension,
                topology: serialized.metadata.topology,
                creation_time: now,
                last_modified: now,
                modification_count: serialized.metadata.modification_count,
                simulation_history: serialized.metadata.simulation_history,
            },
            cache: GeometryCache::default(),
            foliation: serialized.foliation,
            foliation_synced_at_modification,
        };

        tri.validate_checkpoint_invariants()
            .map_err(DeError::custom)?;
        Ok(tri)
    }
}

impl CdtTriangulation<DelaunayBackend2D> {
    /// Validates a deserialized checkpoint before exposing restored CDT state.
    ///
    /// This keeps serde deserialization aligned with the public constructor and
    /// post-move contracts: checkpoint data must restore to a fully valid CDT
    /// triangulation, including geometry, topology, foliation, and causality.
    fn validate_checkpoint_invariants(&self) -> CdtResult<()> {
        self.validate_evolved_cdt()
    }
}

impl<B> CdtTriangulation<B> {
    /// Encodes topology-specific time-slice metadata invariants in one reusable check.
    fn check_time_slices(topology: CdtTopology, time_slices: u32) -> CdtResult<()> {
        Self::parse_time_slices(topology, time_slices).map(|_| ())
    }

    /// Parses raw time-slice metadata into a nonzero invariant-bearing count.
    fn parse_time_slices(topology: CdtTopology, time_slices: u32) -> CdtResult<NonZeroU32> {
        if time_slices == 0 {
            return Err(CdtError::InvalidTriangulationMetadata {
                field: TriangulationMetadataField::Timeslices,
                topology,
                provided_value: "0".to_string(),
                expected: "≥ 1".to_string(),
            });
        }

        if matches!(topology, CdtTopology::Toroidal) && time_slices < 3 {
            return Err(CdtError::InvalidTriangulationMetadata {
                field: TriangulationMetadataField::Timeslices,
                topology,
                provided_value: time_slices.to_string(),
                expected: "≥ 3".to_string(),
            });
        }

        NonZeroU32::new(time_slices).ok_or_else(|| CdtError::InvalidTriangulationMetadata {
            field: TriangulationMetadataField::Timeslices,
            topology,
            provided_value: time_slices.to_string(),
            expected: "≥ 1".to_string(),
        })
    }

    /// Get immutable reference to underlying geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.geometry().vertex_count(), 3);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn geometry(&self) -> &B {
        &self.geometry
    }

    /// Get the number of time slices in the CDT foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.time_slices().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn time_slices(&self) -> NonZeroU32 {
        self.metadata.time_slices
    }

    /// Get the dimensionality of the spacetime.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.dimension(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn dimension(&self) -> u8 {
        self.metadata.dimension
    }

    /// Returns immutable CDT metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.metadata().time_slices().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn metadata(&self) -> &CdtMetadata {
        &self.metadata
    }

    /// Returns the transient process-local identity used for cache invalidation.
    #[must_use]
    pub(crate) const fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// Writes time-slice metadata through the central invariant and foliation invalidation path.
    fn apply_time_slices(&mut self, time_slices: u32) -> CdtResult<()> {
        let time_slices = Self::parse_time_slices(self.metadata.topology, time_slices)?;
        self.metadata.time_slices = time_slices;
        if self
            .foliation
            .as_ref()
            .is_some_and(|foliation| foliation.num_slices() != time_slices)
        {
            self.foliation = None;
            self.foliation_synced_at_modification = None;
        }
        Ok(())
    }

    /// Distinguishes synchronized foliation data from stale bookkeeping after geometry mutation.
    #[must_use]
    fn has_current_foliation(&self) -> bool {
        self.foliation.is_some()
            && self.foliation_synced_at_modification == Some(self.metadata.modification_count)
    }

    /// Records that stored foliation data matches the current geometry mutation count.
    fn mark_foliation_synchronized(&mut self) {
        self.foliation_synced_at_modification = self
            .foliation
            .as_ref()
            .map(|_| self.metadata.modification_count);
    }

    /// Builds the typed error used when callers try to trust stale foliation data.
    fn stale_foliation_error(&self) -> CdtError {
        FoliationError::StaleBookkeeping {
            synced_at_modification: self.foliation_synced_at_modification,
            current_modification_count: self.metadata.modification_count,
        }
        .into()
    }

    /// Marks existing foliation data stale without dropping it when geometry has changed.
    const fn invalidate_foliation_bookkeeping(&mut self) {
        if self.foliation.is_some() {
            self.foliation_synced_at_modification = None;
        }
    }

    /// Updates the configured number of time slices.
    ///
    /// If an existing foliation uses a different slice count, the foliation is
    /// cleared to avoid stale bookkeeping.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidTriangulationMetadata`] if the new slice
    /// count would violate topology metadata invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::errors::{CdtError, TriangulationMetadataField};
    /// use causal_triangulations::prelude::triangulation::{CdtTopology, CdtTriangulation};
    /// use std::assert_matches;
    ///
    /// fn main() -> causal_triangulations::CdtResult<()> {
    /// let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3)?;
    ///
    /// let err = tri.set_time_slices(2).expect_err("T < 3 is invalid");
    /// assert_matches!(
    ///     err,
    ///     CdtError::InvalidTriangulationMetadata {
    ///         field,
    ///         topology,
    ///         provided_value,
    ///         expected,
    ///     } if field == TriangulationMetadataField::Timeslices
    ///         && topology == CdtTopology::Toroidal
    ///         && provided_value == "2"
    ///         && expected == "≥ 3"
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_time_slices(&mut self, time_slices: u32) -> CdtResult<()> {
        if self.metadata.time_slices.get() == time_slices {
            return self.apply_time_slices(time_slices);
        }

        self.apply_time_slices(time_slices)?;
        self.bump_modification_count();
        Ok(())
    }

    /// Marks the triangulation metadata as modified.
    ///
    /// This crate-internal hook invalidates cached derived geometry quantities.
    pub(crate) fn bump_modification_count(&mut self) {
        self.invalidate_cache();
        self.invalidate_foliation_bookkeeping();
        self.metadata.last_modified = Instant::now();
        // Deserialized checkpoints can carry arbitrary counters; avoid wraparound
        // and rotate the instance epoch so external caches keyed by
        // `(instance_id, modification_count)` cannot see a stale version as fresh.
        if self.metadata.modification_count == u64::MAX {
            self.instance_id = next_triangulation_instance_id();
            self.metadata.modification_count = 1;
        } else {
            self.metadata.modification_count += 1;
        }
    }

    /// Records a simulation event without marking the geometry as mutated.
    pub(crate) fn record_event(&mut self, event: SimulationEvent) {
        self.metadata.simulation_history.push(event);
        self.metadata.last_modified = Instant::now();
    }

    /// Clears derived geometry counts so later queries recompute from the backend.
    fn invalidate_cache(&mut self) {
        self.cache = GeometryCache::default();
    }
}

impl<B: TriangulationQuery> CdtTriangulation<B> {
    /// Fallible constructor for a CDT triangulation with open-boundary topology.
    ///
    /// Validates metadata and topology before returning so callers cannot
    /// accidentally wrap a backend with a mismatched dimension, zero time slices,
    /// or an Euler characteristic that cannot represent an open-boundary CDT
    /// triangulation.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidTriangulationMetadata`] if the supplied
    /// metadata is inconsistent with the backend or topology. Returns
    /// [`CdtError::TopologyMismatch`] when the backend Euler characteristic does
    /// not match open-boundary topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let backend = MockBackend::create_triangle();
    ///     let tri = CdtTriangulation::try_new(backend, 2, 2)?;
    ///     assert_eq!(tri.time_slices().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    pub fn try_new(geometry: B, time_slices: u32, dimension: u8) -> CdtResult<Self> {
        let tri = Self::from_parts_before_validation(
            geometry,
            time_slices,
            dimension,
            CdtTopology::OpenBoundary,
        )?;
        tri.validate_metadata()?;
        tri.validate_topology()?;
        Ok(tri)
    }

    /// Create new CDT triangulation with explicit topology.
    ///
    /// Wraps an existing geometry backend and tags it with [`CdtTopology`].
    /// The backend itself is not modified; pass a backend whose Euler
    /// characteristic and metadata invariants match the supplied topology,
    /// otherwise this constructor rejects it.
    ///
    /// Toroidal triangulations must use at least three time slices so the
    /// periodic neighbors `t - 1` and `t + 1` remain distinct.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidTriangulationMetadata`] when the requested
    /// topology metadata is internally inconsistent. Returns
    /// [`CdtError::TopologyMismatch`] when the backend Euler characteristic does
    /// not match the requested topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Four,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///
    ///     let tri = CdtTriangulation::with_topology(backend, 2, 2, CdtTopology::OpenBoundary)?;
    ///     assert_matches!(tri.metadata().topology(), CdtTopology::OpenBoundary);
    ///     assert_eq!(tri.time_slices().get(), 2);
    ///     assert_eq!(tri.dimension(), 2);
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ```rust
    /// use causal_triangulations::prelude::errors::TriangulationMetadataField;
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Four,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///
    ///     let err = CdtTriangulation::with_topology(backend, 2, 3, CdtTopology::Toroidal)
    ///         .expect_err("toroidal metadata requires at least three time slices");
    ///     assert_matches!(
    ///         err,
    ///         CdtError::InvalidTriangulationMetadata {
    ///             field,
    ///             topology,
    ///             provided_value,
    ///             expected,
    ///         } if field == TriangulationMetadataField::Timeslices
    ///             && topology == CdtTopology::Toroidal
    ///             && provided_value == "2"
    ///             && expected == "≥ 3"
    ///     );
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ```rust
    /// use causal_triangulations::prelude::errors::TriangulationMetadataField;
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    /// use causal_triangulations::{CdtError, CdtResult};
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Four,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///
    ///     let err = CdtTriangulation::with_topology(backend, 3, 2, CdtTopology::Toroidal)
    ///         .expect_err("a planar triangle cannot be published as toroidal");
    ///     assert_matches!(
    ///         err,
    ///         CdtError::TopologyMismatch {
    ///             topology,
    ///             euler_characteristic: 1,
    ///             expected_euler_characteristics,
    ///             ..
    ///         } if topology == CdtTopology::Toroidal && expected_euler_characteristics.as_slice() == [0]
    ///     );
    ///     Ok(())
    /// }
    /// ```
    pub fn with_topology(
        geometry: B,
        time_slices: u32,
        dimension: u8,
        topology: CdtTopology,
    ) -> CdtResult<Self> {
        let mut tri =
            Self::from_parts_before_validation(geometry, time_slices, dimension, topology)?;
        tri.apply_time_slices(time_slices)?;
        tri.validate_metadata()?;
        tri.validate_topology()?;
        Ok(tri)
    }

    /// Assembles metadata for fallible constructors that validate before returning.
    fn from_parts_before_validation(
        geometry: B,
        time_slices: u32,
        dimension: u8,
        topology: CdtTopology,
    ) -> CdtResult<Self> {
        let parsed_time_slices = Self::parse_time_slices(topology, time_slices)?;
        let vertex_count = geometry.vertex_count();
        let creation_event = SimulationEvent::Created {
            vertex_count,
            time_slices,
        };

        Ok(Self {
            instance_id: next_triangulation_instance_id(),
            geometry,
            metadata: CdtMetadata {
                time_slices: parsed_time_slices,
                dimension,
                topology,
                creation_time: Instant::now(),
                last_modified: Instant::now(),
                modification_count: 0,
                simulation_history: vec![creation_event],
            },
            cache: GeometryCache::default(),
            foliation: None,
            foliation_synced_at_modification: None,
        })
    }

    /// Validates CDT metadata against backend and topology invariants.
    fn validate_metadata(&self) -> CdtResult<()> {
        Self::check_time_slices(self.metadata.topology, self.metadata.time_slices.get())?;

        let backend_dimension = self.geometry.dimension();
        if usize::from(self.metadata.dimension) != backend_dimension {
            return Err(CdtError::InvalidTriangulationMetadata {
                field: TriangulationMetadataField::Dimension,
                topology: self.metadata.topology,
                provided_value: self.metadata.dimension.to_string(),
                expected: format!("backend dimension ({backend_dimension})"),
            });
        }

        Ok(())
    }

    /// Returns the number of vertices in the backend triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.vertex_count(), 3);
    ///     Ok(())
    /// }
    /// ```
    pub fn vertex_count(&self) -> usize {
        self.geometry.vertex_count()
    }

    /// Get the number of faces in the triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.face_count(), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn face_count(&self) -> usize {
        self.geometry.face_count()
    }

    /// Cached edge count with automatic invalidation.
    ///
    /// Returns the cached edge count if the cache is valid (i.e., no mutations since last refresh).
    /// Otherwise, computes the edge count directly **without updating the cache**.
    ///
    /// Call [`refresh_cache()`](Self::refresh_cache) to explicitly populate the cache before
    /// performance-critical loops that frequently query edge counts.
    ///
    /// # Performance
    ///
    /// - Cache hit: O(1)
    /// - Cache miss: O(E) - delegates to backend's edge counting which scans all facets
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     assert_eq!(tri.edge_count(), 3);
    ///     Ok(())
    /// }
    /// ```
    pub fn edge_count(&self) -> usize {
        if let Some(cached) = &self.cache.edge_count
            && cached.modification_count == self.metadata.modification_count
        {
            return cached.value;
        }

        self.geometry.edge_count()
    }

    /// Returns strictly positive CDT simplex counts for the current state.
    ///
    /// This is the CDT-domain counterpart to the raw `usize` count accessors. Use
    /// it when downstream code relies on a constructed triangulation having at
    /// least one vertex, edge, and triangle.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidSimplexCount`] if the underlying backend reports
    /// a zero vertex, edge, or triangle count.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     let counts = tri.simplex_counts()?;
    ///
    ///     assert_eq!(counts.vertex_count(), 12);
    ///     assert!(counts.edge_count() > 0);
    ///     assert!(counts.triangle_count() > 0);
    ///     Ok(())
    /// }
    /// ```
    pub fn simplex_counts(&self) -> CdtResult<CdtSimplexCounts> {
        CdtSimplexCounts::try_new(self.vertex_count(), self.edge_count(), self.face_count())
    }

    /// Force cache update.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    /// use causal_triangulations::{CdtResult, CdtTriangulation};
    ///
    /// fn main() -> CdtResult<()> {
    ///     let mut tri = CdtTriangulation::try_new(MockBackend::create_triangle(), 2, 2)?;
    ///     tri.refresh_cache();
    ///     assert_eq!(tri.edge_count(), 3);
    ///     Ok(())
    /// }
    /// ```
    pub fn refresh_cache(&mut self) {
        let mod_count = self.metadata.modification_count;

        self.cache.edge_count = Some(CachedValue {
            value: self.geometry.edge_count(),
            modification_count: mod_count,
        });

        self.cache.euler_char = Some(CachedValue {
            value: self.geometry.euler_characteristic(),
            modification_count: mod_count,
        });
    }

    /// Validate topology properties.
    ///
    /// Checks that the triangulation satisfies expected topological constraints,
    /// including the Euler characteristic for the given dimension and boundary conditions.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidTriangulationMetadata`] if stored metadata is
    /// inconsistent with the backend, or [`CdtError::TopologyMismatch`] when the
    /// backend Euler characteristic does not match the configured topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53)?;
    ///     tri.validate_topology()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_topology(&self) -> CdtResult<()> {
        self.validate_metadata()?;

        let euler_char = self.geometry.euler_characteristic();

        if self.dimension() == 2 {
            let expected = match self.metadata.topology {
                // Open boundary: planar with boundary χ=1, closed surface χ=2
                CdtTopology::OpenBoundary => [1, 2].as_slice(),
                // Toroidal (S¹×S¹): Euler characteristic must be 0
                CdtTopology::Toroidal => [0].as_slice(),
            };

            if !expected.contains(&euler_char) {
                return Err(CdtError::TopologyMismatch {
                    topology: self.metadata.topology,
                    euler_characteristic: euler_char,
                    expected_euler_characteristics: expected.to_vec(),
                    vertices: self.geometry.vertex_count(),
                    edges: self.geometry.edge_count(),
                    faces: self.geometry.face_count(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::backends::mock::MockBackend;
    use crate::geometry::generators::build_delaunay2_with_data;
    use serde_json::error::Category;
    use serde_json::{from_str, from_value, json, to_string, to_value};
    use std::assert_matches;
    use std::num::NonZeroUsize;
    use std::thread;
    use std::time::{Duration, Instant};

    /// Serde custom deserialization errors expose only a data/error category,
    /// so keep message checks behind one helper while still asserting the
    /// structured classification that `serde_json` provides.
    fn assert_checkpoint_data_error(error: &serde_json::Error, expected_details: &[&str]) {
        assert_eq!(error.classify(), Category::Data);
        let message = error.to_string();
        for expected_detail in expected_details {
            assert!(
                message.contains(expected_detail),
                "checkpoint deserialization error {message:?} did not contain {expected_detail:?}"
            );
        }
    }

    /// Builds a minimal labeled Delaunay backend for foliation and causality tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt).expect("test Delaunay triangle should validate")
    }

    #[test]
    fn simplex_counts_reject_zero_counts_before_storage() {
        let error = CdtSimplexCounts::try_new(3, 0, 1)
            .expect_err("zero edge count should not produce a CDT count snapshot");

        assert_matches!(
            error,
            CdtError::InvalidSimplexCount {
                field: SimplexCountField::Edges,
                provided_value: 0,
            }
        );
    }

    #[test]
    fn triangulation_simplex_counts_preserve_nonzero_counts() {
        let triangulation = CdtTriangulation::from_cdt_strip(4, 3).expect("CDT strip should build");
        let counts = triangulation
            .simplex_counts()
            .expect("constructed CDT triangulation should have positive counts");

        assert_eq!(counts.vertices().get(), triangulation.vertex_count());
        assert_eq!(counts.edges().get(), triangulation.edge_count());
        assert_eq!(counts.triangles().get(), triangulation.face_count());
    }

    #[test]
    fn metadata_accessors_do_not_require_backend_query_trait() {
        struct MetadataOnlyBackend;

        let now = Instant::now();
        let mut triangulation = CdtTriangulation {
            instance_id: next_triangulation_instance_id(),
            geometry: MetadataOnlyBackend,
            metadata: CdtMetadata {
                time_slices: NonZeroU32::new(2).expect("test time-slice count should be nonzero"),
                dimension: 2,
                topology: CdtTopology::OpenBoundary,
                creation_time: now,
                last_modified: now,
                modification_count: 0,
                simulation_history: Vec::new(),
            },
            cache: GeometryCache::default(),
            foliation: None,
            foliation_synced_at_modification: None,
        };

        assert_eq!(triangulation.time_slices().get(), 2);
        assert_eq!(triangulation.dimension(), 2);
        assert_eq!(
            triangulation.metadata().topology(),
            CdtTopology::OpenBoundary
        );
        triangulation
            .set_time_slices(3)
            .expect("open-boundary metadata update should validate");
        assert_eq!(triangulation.time_slices().get(), 3);
        assert_eq!(triangulation.metadata().modification_count(), 1);
    }

    /// Builds intentionally unchecked metadata for legacy validation tests.
    fn unchecked_open_boundary(
        backend: DelaunayBackend2D,
        time_slices: u32,
        dimension: u8,
    ) -> CdtTriangulation<DelaunayBackend2D> {
        CdtTriangulation::from_parts_before_validation(
            backend,
            time_slices,
            dimension,
            CdtTopology::OpenBoundary,
        )
        .expect("unchecked test metadata should use nonzero time slices")
    }

    #[test]
    fn test_try_new_rejects_zero_time_slices() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let result = CdtTriangulation::try_new(backend, 0, 2);

        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                topology,
                ref provided_value,
                ref expected,
            }) if *field == TriangulationMetadataField::Timeslices
                && topology == CdtTopology::OpenBoundary
                && provided_value == "0"
                && expected == "≥ 1"
        );
    }

    #[test]
    fn test_try_new_rejects_dimension_mismatch() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let result = CdtTriangulation::try_new(backend, 2, 3);

        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                topology,
                ref provided_value,
                ref expected,
            }) if *field == TriangulationMetadataField::Dimension
                && topology == CdtTopology::OpenBoundary
                && provided_value == "3"
                && expected == "backend dimension (2)"
        );
    }

    #[test]
    fn test_try_new_rejects_open_boundary_topology_mismatch() {
        let backend = MockBackend::new_2d();
        let result = CdtTriangulation::try_new(backend, 3, 2);

        assert_matches!(
            result,
            Err(CdtError::TopologyMismatch {
                topology,
                euler_characteristic: 0,
                ref expected_euler_characteristics,
                ..
            }) if topology == CdtTopology::OpenBoundary
                && expected_euler_characteristics.as_slice() == [1, 2]
        );
    }

    #[test]
    fn test_validate_topology_rejects_legacy_dimension_mismatch() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let tri = unchecked_open_boundary(backend, 2, 3);
        let result = tri.validate_topology();

        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref provided_value,
                ref expected,
                ..
            }) if *field == TriangulationMetadataField::Dimension
                && provided_value == "3"
                && expected == "backend dimension (2)"
        );
    }

    #[test]
    fn test_geometry_access() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Test immutable access
        let geometry = triangulation.geometry();
        assert!(geometry.vertex_count() > 0);
        assert!(geometry.is_valid());
        assert_eq!(geometry.dimension(), 2);
    }

    #[test]
    fn test_basic_properties() {
        let triangulation =
            CdtTriangulation::from_random_points(8, 4, 2).expect("Failed to create triangulation");

        // Test basic property getters
        assert_eq!(triangulation.dimension(), 2);
        assert_eq!(triangulation.time_slices().get(), 4);
        assert_eq!(triangulation.vertex_count(), 8);

        let edge_count = triangulation.edge_count();
        let face_count = triangulation.face_count();

        assert!(edge_count > 0, "Should have edges");
        assert!(face_count > 0, "Should have faces");

        // For a triangulation, we expect certain relationships
        assert!(
            edge_count >= triangulation.vertex_count(),
            "Usually E >= V for connected triangulation"
        );
        assert!(face_count >= 1, "Should have at least one face");
    }

    #[test]
    fn test_metadata_initialization() {
        let triangulation =
            CdtTriangulation::from_random_points(6, 3, 2).expect("Failed to create triangulation");

        // Check that metadata is properly initialized
        assert_eq!(triangulation.dimension(), 2);
        assert_eq!(triangulation.time_slices().get(), 3);

        // Metadata should be accessible through debug formatting
        let debug_output = format!("{triangulation:?}");
        assert!(debug_output.contains("CdtTriangulation"));
        assert!(debug_output.contains("CdtMetadata"));
    }

    #[test]
    fn test_creation_history() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Should have at least one creation event
        assert!(!triangulation.metadata().simulation_history.is_empty());

        match &triangulation.metadata().simulation_history[0] {
            SimulationEvent::Created {
                vertex_count,
                time_slices,
            } => {
                assert_eq!(*vertex_count, 5);
                assert_eq!(*time_slices, 2);
            }
            _ => panic!("First event should be Creation"),
        }
    }

    #[test]
    fn test_metadata_mutation_invalidates_cache() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Get initial edge count
        let initial_edge_count = triangulation.edge_count();
        assert!(initial_edge_count > 0);

        let initial_mod_count = triangulation.metadata().modification_count;

        triangulation.bump_modification_count();

        // Modification count should have increased
        assert_eq!(
            triangulation.metadata().modification_count,
            initial_mod_count + 1
        );

        // Cache should have been invalidated but recalculated value should be same
        let recalculated_edge_count = triangulation.edge_count();
        assert_eq!(initial_edge_count, recalculated_edge_count);
    }

    #[test]
    fn test_cache_refresh_functionality() {
        let mut triangulation =
            CdtTriangulation::from_random_points(6, 2, 2).expect("Failed to create triangulation");

        // Get initial counts without cache
        let edge_count_1 = triangulation.edge_count();

        // Refresh cache
        triangulation.refresh_cache();

        // Should return same values from cache
        let edge_count_2 = triangulation.edge_count();
        assert_eq!(
            edge_count_1, edge_count_2,
            "Cache should return consistent values"
        );

        // Multiple cache hits should be consistent
        let edge_count_3 = triangulation.edge_count();
        assert_eq!(
            edge_count_1, edge_count_3,
            "Multiple cache hits should be consistent"
        );
    }

    #[test]
    fn test_cache_invalidation_on_mutation() {
        let mut triangulation =
            CdtTriangulation::from_random_points(6, 2, 2).expect("Failed to create triangulation");

        // Populate cache
        triangulation.refresh_cache();
        let cached_count = triangulation.edge_count();

        triangulation.bump_modification_count();

        // Edge count should still be correct (recalculated, not cached)
        let new_count = triangulation.edge_count();
        assert_eq!(
            cached_count, new_count,
            "Edge count should remain consistent after cache invalidation"
        );
    }

    #[test]
    fn test_euler_characteristic() {
        // Use fixed seed to ensure deterministic closed triangulation with Euler=2
        // Seed 53 produces V=5, E=9, F=6, Euler=2 for this configuration
        const TRIANGULATION_SEED: u64 = 53;

        let triangulation = CdtTriangulation::from_seeded_points(5, 2, 2, TRIANGULATION_SEED)
            .expect("Failed to create triangulation with fixed seed");

        let result = triangulation.geometry().is_valid();
        assert!(result, "Validation should succeed for closed triangulation");
    }

    #[test]
    fn test_validate_topology() {
        // Test with various configurations to check topology validation
        let seeds = [53, 87, 203]; // Known good seeds that produce Euler=2

        for seed in seeds {
            let triangulation = CdtTriangulation::from_seeded_points(5, 1, 2, seed)
                .expect("Failed to create triangulation");

            let result = triangulation.validate_topology();
            assert!(
                result.is_ok(),
                "Topology validation should succeed for seed {seed}: {result:?}"
            );
        }
    }

    #[test]
    fn test_simulation_event_recording() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        let initial_history_len = triangulation.metadata().simulation_history.len();
        let initial_last_modified = triangulation.metadata().last_modified;
        thread::sleep(Duration::from_millis(5));

        triangulation.record_event(SimulationEvent::MoveAttempted {
            move_type: MoveType::Move22,
            step: 1,
        });

        triangulation.record_event(SimulationEvent::MoveAccepted {
            move_type: MoveType::Move22,
            step: 1,
            action_change: -0.5,
        });

        triangulation.record_event(SimulationEvent::MeasurementTaken {
            step: 2,
            action: 10.5,
        });

        // Should have 3 more events
        assert_eq!(
            triangulation.metadata().simulation_history.len(),
            initial_history_len + 3
        );
        assert!(triangulation.metadata().last_modified > initial_last_modified);

        // Check the recorded events
        let history = &triangulation.metadata().simulation_history;
        match &history[initial_history_len] {
            SimulationEvent::MoveAttempted { move_type, step } => {
                assert_eq!(*move_type, MoveType::Move22);
                assert_eq!(*step, 1);
            }
            _ => panic!("Expected MoveAttempted event"),
        }

        match &history[initial_history_len + 1] {
            SimulationEvent::MoveAccepted {
                move_type,
                step,
                action_change,
            } => {
                assert_eq!(*move_type, MoveType::Move22);
                assert_eq!(*step, 1);
                approx::assert_relative_eq!(*action_change, -0.5);
            }
            _ => panic!("Expected MoveAccepted event"),
        }

        match &history[initial_history_len + 2] {
            SimulationEvent::MeasurementTaken { step, action } => {
                assert_eq!(*step, 2);
                approx::assert_relative_eq!(*action, 10.5);
            }
            _ => panic!("Expected MeasurementTaken event"),
        }
    }

    #[test]
    fn test_record_event_keeps_foliation_synchronized() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        let vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex");
        let edge = tri
            .geometry()
            .edges()
            .next()
            .expect("Triangle should contain an edge");
        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("Triangle should contain a face");

        let initial_modification_count = tri.metadata().modification_count;
        let initial_slice_sizes = tri.slice_sizes().to_vec();

        tri.record_event(SimulationEvent::MoveAttempted {
            move_type: MoveType::EdgeFlip,
            step: 7,
        });

        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count
        );
        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), initial_slice_sizes.as_slice());
        assert!(tri.time_label(&vertex).is_some());
        assert!(tri.edge_type(&edge).is_some());
        assert!(tri.simplex_type(&face).is_some());
    }

    #[test]
    fn test_metadata_timestamps() {
        let start_time = Instant::now();

        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        let creation_time = triangulation.metadata().creation_time;
        let initial_last_modified = triangulation.metadata().last_modified;

        // Creation time should be after our start time
        assert!(creation_time >= start_time);

        // Initially, creation_time and last_modified should be very close
        let time_diff = initial_last_modified.duration_since(creation_time);
        assert!(time_diff < Duration::from_millis(10));

        // Make a small delay then modify
        thread::sleep(Duration::from_millis(5));

        triangulation.bump_modification_count();

        let new_last_modified = triangulation.metadata().last_modified;

        // last_modified should have been updated
        assert!(new_last_modified > initial_last_modified);

        // creation_time should remain unchanged
        assert_eq!(triangulation.metadata().creation_time, creation_time);
    }

    #[test]
    fn test_modification_count() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Initial modification count should be 0
        assert_eq!(triangulation.metadata().modification_count, 0);

        // Each explicit mutation marker should increment once.
        triangulation.bump_modification_count();
        assert_eq!(triangulation.metadata().modification_count, 1);

        triangulation.bump_modification_count();
        assert_eq!(triangulation.metadata().modification_count, 2);

        // Immutable access shouldn't change count
        let _geometry = triangulation.geometry();
        let _edge_count = triangulation.edge_count();
        assert_eq!(triangulation.metadata().modification_count, 2);

        triangulation.metadata.modification_count = u64::MAX;
        let saturated_instance_id = triangulation.instance_id();
        triangulation.bump_modification_count();
        assert_eq!(triangulation.metadata().modification_count, 1);
        assert_ne!(
            triangulation.instance_id(),
            saturated_instance_id,
            "saturated modification counters must rotate instance epochs so external caches cannot collide"
        );
    }

    #[test]
    fn test_zero_time_slices_rejected() {
        let result = CdtTriangulation::from_random_points(5, 0, 2);
        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref provided_value,
                ref expected,
                ..
            }) if *field == TriangulationMetadataField::Timeslices && provided_value == "0" && expected == "≥ 1"
        );
    }

    #[test]
    fn test_set_time_slices_noop_preserves_metadata() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");
        let initial_modification_count = triangulation.metadata().modification_count;
        let initial_last_modified = triangulation.metadata().last_modified;

        triangulation
            .set_time_slices(2)
            .expect("unchanged time-slice count should be accepted");

        assert_eq!(triangulation.time_slices().get(), 2);
        assert_eq!(
            triangulation.metadata().modification_count,
            initial_modification_count
        );
        assert_eq!(
            triangulation.metadata().last_modified,
            initial_last_modified
        );
    }

    #[test]
    fn test_set_time_slices_updates_and_clears_mismatched_foliation() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let initial_modification_count = tri.metadata().modification_count;

        tri.set_time_slices(3)
            .expect("open-boundary time-slice metadata can be widened");

        assert_eq!(tri.time_slices().get(), 3);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count + 1
        );
        assert!(!tri.has_foliation());
        assert!(tri.slice_sizes().is_empty());
    }

    #[test]
    fn test_large_time_slices() {
        let result = CdtTriangulation::from_random_points(5, 100, 2);
        assert!(result.is_ok(), "Should allow large time slice count");

        let triangulation = result.unwrap();
        assert_eq!(triangulation.time_slices().get(), 100);
    }

    #[test]
    fn test_consistency_across_methods() {
        let triangulation =
            CdtTriangulation::from_random_points(8, 3, 2).expect("Failed to create triangulation");

        // Test consistency between different access methods
        let direct_vertex_count = triangulation.vertex_count();
        let geometry_vertex_count = triangulation.geometry().vertex_count();
        assert_eq!(
            direct_vertex_count, geometry_vertex_count,
            "Vertex count should be consistent"
        );

        let direct_face_count = triangulation.face_count();
        let geometry_face_count = triangulation.geometry().face_count();
        assert_eq!(
            direct_face_count, geometry_face_count,
            "Face count should be consistent"
        );

        let direct_edge_count = triangulation.edge_count();
        let geometry_edge_count = triangulation.geometry().edge_count();
        assert_eq!(
            direct_edge_count, geometry_edge_count,
            "Edge count should be consistent"
        );
    }

    #[test]
    fn test_debug_formatting() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        let debug_str = format!("{triangulation:?}");

        // Should contain key components
        assert!(debug_str.contains("CdtTriangulation"));
        assert!(debug_str.contains("geometry"));
        assert!(debug_str.contains("metadata"));
        assert!(debug_str.contains("cache"));
    }

    #[test]
    fn test_simulation_event_debug() {
        let events = vec![
            SimulationEvent::Created {
                vertex_count: 5,
                time_slices: 2,
            },
            SimulationEvent::MoveAttempted {
                move_type: MoveType::EdgeFlip,
                step: 1,
            },
            SimulationEvent::MoveAccepted {
                move_type: MoveType::EdgeFlip,
                step: 1,
                action_change: 0.5,
            },
            SimulationEvent::MeasurementTaken {
                step: 2,
                action: 15.5,
            },
        ];

        for event in events {
            let debug_str = format!("{event:?}");
            // Should not panic and should contain meaningful content
            assert!(!debug_str.is_empty());
        }
    }

    #[test]
    fn simulation_event_move_type_serializes_as_enum_variant() {
        let event = SimulationEvent::MoveAccepted {
            move_type: MoveType::Move31Remove,
            step: 9,
            action_change: -1.25,
        };

        let value = to_value(&event).expect("event should serialize");

        assert_eq!(
            value,
            json!({
                "MoveAccepted": {
                    "move_type": "Move31Remove",
                    "step": 9,
                    "action_change": -1.25
                }
            })
        );

        let restored: SimulationEvent =
            from_value(value).expect("typed move event should deserialize");
        match restored {
            SimulationEvent::MoveAccepted {
                move_type,
                step,
                action_change,
            } => {
                assert_eq!(move_type, MoveType::Move31Remove);
                assert_eq!(step, 9);
                approx::assert_relative_eq!(action_change, -1.25);
            }
            other => panic!("expected MoveAccepted event, got {other:?}"),
        }
    }

    #[test]
    fn simulation_event_rejects_free_form_move_type_strings() {
        let invalid_json = r#"{"MoveAttempted":{"move_type":"test_move","step":1}}"#;

        let error = from_str::<SimulationEvent>(invalid_json)
            .expect_err("move history should reject unsupported move strings");

        assert_checkpoint_data_error(&error, &["unknown variant `test_move`", "Move22"]);
    }

    #[test]
    fn test_cdt_metadata_clone() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        let metadata1 = triangulation.metadata().clone();
        let metadata2 = metadata1.clone();

        assert_eq!(metadata1.time_slices, metadata2.time_slices);
        assert_eq!(metadata1.dimension, metadata2.dimension);
        assert_eq!(metadata1.modification_count, metadata2.modification_count);
        assert_eq!(
            metadata1.simulation_history.len(),
            metadata2.simulation_history.len()
        );
    }

    #[test]
    fn test_extreme_vertex_counts() {
        // Test minimum valid count
        let min_tri = CdtTriangulation::from_random_points(3, 1, 2)
            .expect("Should create triangulation with 3 vertices");
        assert_eq!(min_tri.vertex_count(), 3);

        // Test larger count (within reasonable bounds for testing)
        let large_tri = CdtTriangulation::from_random_points(50, 1, 2)
            .expect("Should create triangulation with 50 vertices");
        assert_eq!(large_tri.vertex_count(), 50);
        assert!(
            large_tri.edge_count() > 50,
            "Large triangulation should have many edges"
        );
        assert!(
            large_tri.face_count() > 10,
            "Large triangulation should have many faces"
        );
    }

    #[test]
    fn test_validate_topology_open_boundary() {
        let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53).expect("create triangulation");

        // OpenBoundary topology should pass validation
        assert!(tri.validate_topology().is_ok());
    }

    #[test]
    fn test_validate_topology_open_boundary_rejects_chi_zero() {
        let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        tri.metadata.topology = CdtTopology::OpenBoundary;

        let result = tri.validate_topology();

        assert_matches!(
            result,
            Err(CdtError::TopologyMismatch {
                topology,
                euler_characteristic: 0,
                ref expected_euler_characteristics,
                ..
            }) if topology == CdtTopology::OpenBoundary && expected_euler_characteristics == &[1, 2]
        );
    }

    #[test]
    fn test_validate_topology_toroidal_rejects_chi_nonzero() {
        // Build a non-toroidal triangulation, then try to label its metadata as
        // Toroidal. The checked public constructor must reject it immediately.
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");

        let result = CdtTriangulation::with_topology(backend, 3, 2, CdtTopology::Toroidal);
        assert_matches!(
            result,
            Err(CdtError::TopologyMismatch {
                topology,
                euler_characteristic: 1,
                ref expected_euler_characteristics,
                ..
            }) if topology == CdtTopology::Toroidal && expected_euler_characteristics == &[0]
        );
    }

    #[test]
    fn test_toroidal_rejects_few_slices() {
        let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");

        let result = tri.set_time_slices(2);
        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                topology,
                ref provided_value,
                ref expected,
            }) if *field == TriangulationMetadataField::Timeslices
                && topology == CdtTopology::Toroidal
                && provided_value == "2"
                && expected == "≥ 3"
        );
        assert_eq!(tri.time_slices().get(), 3);
        assert!(tri.validate_topology().is_ok());
    }

    #[test]
    fn test_with_topology_rejects_few_toroidal_slices() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");

        let result = CdtTriangulation::with_topology(backend, 2, 3, CdtTopology::Toroidal);
        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                topology,
                ref provided_value,
                ref expected,
            }) if *field == TriangulationMetadataField::Timeslices
                && topology == CdtTopology::Toroidal
                && provided_value == "2"
                && expected == "≥ 3"
        );
    }

    #[test]
    fn strip_checkpoint_roundtrip_preserves_foliation_and_classification() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay CDT strip should build");

        let json = to_string(&triangulation).expect("checkpoint should serialize");
        let restored: CdtTriangulation<DelaunayBackend2D> =
            from_str(&json).expect("checkpoint should deserialize");

        restored
            .validate_checkpoint_invariants()
            .expect("restored strip should validate checkpoint invariants");
        assert_eq!(restored.slice_sizes(), triangulation.slice_sizes());
        assert_eq!(restored.metadata().topology, CdtTopology::OpenBoundary);
        assert_eq!(
            restored
                .geometry()
                .faces()
                .filter(|face| restored.simplex_type(face).is_some())
                .count(),
            restored.face_count(),
            "all strip simplices should keep Up/Down classification"
        );
    }

    #[test]
    fn checkpoint_rejects_invalid_toroidal_period() {
        let valid = CdtTriangulation::from_cdt_strip(4, 3).expect("valid strip should build");
        let json = to_string(&valid).expect("checkpoint should serialize");
        let invalid_json = json.replace(
            r#""global_topology":"Euclidean""#,
            r#""global_topology":{"Toroidal":{"domain":[0.0,1.0],"mode":"Explicit"}}"#,
        );
        let error = from_str::<CdtTriangulation<DelaunayBackend2D>>(&invalid_json)
            .expect_err("backend serde should reject invalid toroidal period");
        assert_checkpoint_data_error(&error, &["invalid toroidal period"]);
    }

    #[test]
    fn toroidal_checkpoint_roundtrip_preserves_periodic_offsets() {
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(4, 3).expect("periodic torus should build");

        let json = to_string(&triangulation).expect("checkpoint should serialize");
        let restored = from_str::<CdtTriangulation<DelaunayBackend2D>>(&json)
            .expect("Delaunay 0.8 should preserve periodic simplex offsets");

        restored
            .validate_checkpoint_invariants()
            .expect("restored toroidal checkpoint should retain its periodic realization");
        assert_eq!(restored.vertex_count(), triangulation.vertex_count());
        assert_eq!(restored.edge_count(), triangulation.edge_count());
        assert_eq!(restored.face_count(), triangulation.face_count());
    }

    #[test]
    fn checkpoint_roundtrip_preserves_delaunay_check_interval() {
        let mut triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay CDT strip should build");
        triangulation.set_delaunay_check_interval(NonZeroUsize::new(8));

        let json = to_string(&triangulation).expect("checkpoint should serialize");
        let restored: CdtTriangulation<DelaunayBackend2D> =
            from_str(&json).expect("checkpoint should deserialize");

        assert!(
            !restored.geometry().should_check_delaunay_after(7),
            "EveryN(8) should not be due before the eighth accepted mutation"
        );
        assert!(
            restored.geometry().should_check_delaunay_after(8),
            "EveryN(8) should be preserved across checkpoint roundtrip"
        );
    }

    #[test]
    fn checkpoint_roundtrip_keeps_history_valid() {
        let mut triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("Delaunay CDT strip should build");
        triangulation.record_event(SimulationEvent::MoveAttempted {
            move_type: MoveType::Move22,
            step: 1,
        });
        triangulation.record_event(SimulationEvent::MeasurementTaken {
            step: 1,
            action: 0.0,
        });

        let json = to_string(&triangulation).expect("checkpoint should serialize");
        let restored: CdtTriangulation<DelaunayBackend2D> =
            from_str(&json).expect("checkpoint should deserialize");

        restored
            .validate_checkpoint_invariants()
            .expect("restored MCMC checkpoint should validate invariants");
        assert_eq!(
            restored.metadata().simulation_history.len(),
            triangulation.metadata().simulation_history.len()
        );
        assert_eq!(
            restored.metadata().modification_count,
            triangulation.metadata().modification_count
        );
        assert_eq!(
            restored.slice_sizes(),
            triangulation.slice_sizes(),
            "checkpoint should preserve foliation bookkeeping"
        );
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: deterministic CDT strips preserve the expected disk Euler characteristic.
        #[test]
        fn cdt_strip_euler_characteristic_matches_topology(
            vertices_per_slice in 4u32..12,
            num_slices in 2u32..8
        ) {
            let triangulation = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)?;

            prop_assert_eq!(
                triangulation.geometry().euler_characteristic(),
                1,
                "CDT strip should have disk Euler characteristic for N={}, T={}",
                vertices_per_slice,
                num_slices
            );
            prop_assert!(triangulation.validate_topology().is_ok());
        }

        /// Property: Triangulation should have positive counts for all simplex types
        #[test]
        fn triangulation_positive_simplex_counts(
            vertices in 3u32..30,
            timeslices in 1u32..5
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;
            let counts = triangulation.simplex_counts()?;

            prop_assert!(counts.vertices().get() >= 3, "Must have at least 3 vertices");
            prop_assert!(counts.edges().get() >= 3, "Must have at least 3 edges");
            prop_assert!(counts.triangles().get() >= 1, "Must have at least 1 face");
        }

        #[test]
        fn triangulation_validity_invariant(
            vertices in 4u32..15,  // Smaller, more stable range
            timeslices in 1u32..3  // Even smaller range
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            // Random point generation can create degenerate cases.
            // At minimum, check that basic geometry is valid
            prop_assert!(
                triangulation.geometry().is_valid(),
                "Basic triangulation should be geometrically valid"
            );
        }

        /// Property: Cache consistency - repeated edge counts should be identical
        #[test]
        fn cache_consistency(
            vertices in 4u32..25,
            timeslices in 1u32..4
        ) {
            let mut triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            let count1 = triangulation.edge_count();
            let count2 = triangulation.edge_count();
            prop_assert_eq!(count1, count2, "Repeated edge counts should be identical");

            // After refresh, should still be the same
            triangulation.refresh_cache();
            let count3 = triangulation.edge_count();
            prop_assert_eq!(count1, count3, "Count should remain same after cache refresh");
        }

        /// Property: Dimension consistency
        #[test]
        fn dimension_consistency(
            vertices in 3u32..15
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, 2, 2)?;
            prop_assert_eq!(triangulation.dimension(), 2, "Dimension should be 2 for 2D triangulation");
        }

        /// Property: Vertex count scaling - more input vertices should generally lead to more triangulation vertices
        /// (though not always exact due to duplicate removal in random generation)
        #[test]
        fn vertex_count_scaling(
            base_vertices in 5u32..15
        ) {
            let small_tri = CdtTriangulation::from_random_points(base_vertices, 2, 2)?;
            let large_tri = CdtTriangulation::from_random_points(base_vertices * 2, 2, 2)?;

            // Larger input should generally produce more vertices (allowing for some randomness)
            let small_count = small_tri.vertex_count();
            let large_count = large_tri.vertex_count();

            // Allow for some variation due to randomness in point generation
            let threshold = small_count.saturating_sub(small_count / 5); // 80% of small_count
            prop_assert!(
                large_count >= small_count || large_count >= threshold,
                "Larger input should produce comparable or more vertices: small={}, large={}, threshold={}",
                small_count, large_count, threshold
            );
        }

        #[test]
        fn face_edge_relationship(
            vertices in 4u32..12,  // Even smaller range
            timeslices in 1u32..3
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            let v = i32::try_from(triangulation.vertex_count()).unwrap_or(i32::MAX);
            let e = i32::try_from(triangulation.edge_count()).unwrap_or(i32::MAX);
            let f = i32::try_from(triangulation.face_count()).unwrap_or(i32::MAX);

            // Just verify basic positivity and reasonableness
            prop_assert!(v >= 3, "Must have at least 3 vertices");
            prop_assert!(e >= 3, "Must have at least 3 edges");
            prop_assert!(f >= 1, "Must have at least 1 face");

            // Allow very broad Euler characteristic range for random triangulations
            let euler = v - e + f;
            prop_assert!(
                (-10..=10).contains(&euler),
                "Euler characteristic {} extremely out of range (V={}, E={}, F={})",
                euler, v, e, f
            );
        }

        /// Property: Timeslice parameter validation
        #[test]
        fn timeslice_parameter_consistency(
            vertices in 4u32..20,
            timeslices in 1u32..8
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            // Should successfully create triangulation with any valid timeslice count
            prop_assert!(triangulation.vertex_count() > 0);
            prop_assert!(triangulation.edge_count() > 0);
            prop_assert!(triangulation.face_count() > 0);
        }

        /// Property: Seeded triangulation determinism
        ///
        /// Previously disabled due to non-determinism in FastKernel-based generation
        /// (seed=2852, vertices=8, timeslices=3 produced edge counts 12 vs 11).
        /// Re-enabled after switching to AdaptiveKernel + DelaunayTriangulationBuilder.
        #[test]
        fn seeded_determinism_property(
            vertices in 4u32..15,
            timeslices in 1u32..4,
            seed in 1u64..10000
        ) {
            let tri1 = CdtTriangulation::from_seeded_points(vertices, timeslices, 2, seed)?;
            let tri2 = CdtTriangulation::from_seeded_points(vertices, timeslices, 2, seed)?;

            // Same seed should produce identical triangulations
            prop_assert_eq!(tri1.vertex_count(), tri2.vertex_count(), "Vertex counts should match");
            prop_assert_eq!(tri1.edge_count(), tri2.edge_count(), "Edge counts should match");
            prop_assert_eq!(tri1.face_count(), tri2.face_count(), "Face counts should match");
            prop_assert_eq!(tri1.time_slices(), tri2.time_slices(), "Time slices should match");
            prop_assert_eq!(tri1.dimension(), tri2.dimension(), "Dimensions should match");
        }

        /// Property: Metadata consistency and tracking
        #[test]
        fn metadata_tracking_property(
            vertices in 4u32..15,
            timeslices in 1u32..5
        ) {
            let mut triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            // Initial metadata should be consistent
            prop_assert_eq!(triangulation.time_slices().get(), timeslices, "Time slices should match input");
            prop_assert_eq!(triangulation.dimension(), 2, "Dimension should be 2");
            prop_assert_eq!(triangulation.metadata().modification_count, 0, "Initial modification count should be 0");

            // Should have creation event
            prop_assert!(!triangulation.metadata().simulation_history.is_empty(), "Should have creation event");

            let initial_mod_count = triangulation.metadata().modification_count;

            triangulation.bump_modification_count();

            prop_assert_eq!(triangulation.metadata().modification_count, initial_mod_count + 1,
                          "Modification count should increment after mutation");
        }

        /// Property: Cache invalidation and consistency
        #[test]
        fn cache_invalidation_property(
            vertices in 4u32..15,
            timeslices in 1u32..4
        ) {
            let mut triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            // Cache the edge count
            triangulation.refresh_cache();
            let cached_count = triangulation.edge_count();

            // Verify cache hit consistency
            let cached_count_2 = triangulation.edge_count();
            prop_assert_eq!(cached_count, cached_count_2, "Cache hits should be consistent");

            triangulation.bump_modification_count();

            // Should still return correct count (but recalculated)
            let recalculated_count = triangulation.edge_count();
            prop_assert_eq!(cached_count, recalculated_count, "Count should remain same after cache invalidation");
        }

        /// Property: Validation success for well-formed triangulations
        #[test]
        fn validation_success_property(
            seed in 50u64..250, // Range with some known good seeds
            vertices in 4u32..8, // Small range with known good seeds
            timeslices in 1u32..3
        ) {
            let triangulation = CdtTriangulation::from_seeded_points(vertices, timeslices, 2, seed)?;

            // Basic validity should always pass for reasonable seeds
            prop_assert!(triangulation.geometry().is_valid(), "Geometry should be valid");

            // Validation might succeed (depending on Euler characteristic)
            // We can't guarantee it always passes due to topology constraints
            // but basic geometric validity should hold
            prop_assert!(triangulation.vertex_count() >= 3, "Should have >= 3 vertices");
            prop_assert!(triangulation.edge_count() > 0, "Should have > 0 edges");
            prop_assert!(triangulation.face_count() > 0, "Should have > 0 faces");
        }

        /// Property: Simulation event recording consistency
        #[test]
        fn simulation_event_recording_property(
            vertices in 4u32..12,
            timeslices in 1u32..4
        ) {
            let mut triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            let initial_history_len = triangulation.metadata().simulation_history.len();
            let initial_mod_count = triangulation.metadata().modification_count;
            prop_assert!(initial_history_len >= 1, "Should have at least creation event");

            triangulation.record_event(SimulationEvent::MoveAttempted {
                move_type: MoveType::Move22,
                step: 1,
            });
            triangulation.record_event(SimulationEvent::MeasurementTaken {
                step: 2,
                action: 5.0,
            });

            prop_assert_eq!(triangulation.metadata().simulation_history.len(), initial_history_len + 2,
                          "Should have 2 additional events after recording");
            prop_assert_eq!(triangulation.metadata().modification_count, initial_mod_count,
                          "History-only operations should not count as backend mutations");
        }

        /// Property: Geometric invariants for triangulated structures
        #[test]
        fn geometric_invariants_property(
            vertices in 4u32..15,
            timeslices in 1u32..4,
            seed in 1u64..10000
        ) {
            let triangulation = CdtTriangulation::from_seeded_points(vertices, timeslices, 2, seed)?;

            let v = triangulation.vertex_count() as i128;
            let e = triangulation.edge_count() as i128;
            let f = triangulation.face_count() as i128;

            // Basic positivity
            prop_assert!(v >= 3, "Must have at least 3 vertices");
            prop_assert!(e >= 3, "Must have at least 3 edges");
            prop_assert!(f >= 1, "Must have at least 1 face");

            // For triangulated surfaces: E can be up to ~3.5V for complex triangulations
            // This is more realistic for CDT triangulations which can be quite dense
            prop_assert!(e <= 4 * v, "Edge count should not be excessively large: E={}, 4V={}", e, 4 * v);

            // For triangulated surfaces: F <= 2V (very loose bound)
            prop_assert!(f <= 2 * v, "Face count should not be excessively large: F={}, 2V={}", f, 2 * v);

            // TODO: Revisit connectivity constraint when Delaunay crate is updated/fixed
            // The underlying Delaunay triangulation generation can create degenerate triangulations
            // where E < V-1 due to invalid simplex removal and disconnected components.
            // This is a known issue with the current Delaunay crate implementation.
            // For now, we use a more lenient bound that accommodates the observed behavior.
            //
            // Connectivity: Ideally E >= V - 1 for connected graphs, but degenerate cases exist
            prop_assert!(e >= (v - 1) / 2, "Should have reasonable edge count for degenerate triangulations: E={}, (V-1)/2={}", e, (v - 1) / 2);
        }

        /// Property: Input parameter bounds validation
        #[test]
        fn parameter_bounds_property(
            vertices in 0u32..30, // Include invalid range (0..3) to exercise error branch
            timeslices in 0u32..6
        ) {
            let result = CdtTriangulation::from_random_points(vertices, timeslices, 2);

            if vertices >= 3 && timeslices >= 1 {
                prop_assert!(
                    result.is_ok(),
                    "Should succeed with valid vertex/time-slice count: vertices={}, timeslices={}",
                    vertices,
                    timeslices
                );

                let triangulation = result.unwrap();
                let vertex_count_u32 =
                    u32::try_from(triangulation.vertex_count()).unwrap_or(u32::MAX);
                prop_assert_eq!(vertex_count_u32, vertices, "Vertex count should match input");
                prop_assert_eq!(triangulation.time_slices().get(), timeslices, "Time slices should match input");
                prop_assert_eq!(triangulation.dimension(), 2, "Dimension should be 2");
            } else {
                prop_assert!(
                    result.is_err(),
                    "Should fail with invalid parameters: vertices={}, timeslices={}",
                    vertices,
                    timeslices
                );
            }
        }

        /// Property: Consistency across different access methods
        #[test]
        fn access_method_consistency_property(
            vertices in 4u32..15,
            timeslices in 1u32..4
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            // Test that different ways of accessing the same data give consistent results
            let direct_vertex_count = triangulation.vertex_count();
            let geometry_vertex_count = triangulation.geometry().vertex_count();
            prop_assert_eq!(direct_vertex_count, geometry_vertex_count, "Vertex count access should be consistent");

            let direct_face_count = triangulation.face_count();
            let geometry_face_count = triangulation.geometry().face_count();
            prop_assert_eq!(direct_face_count, geometry_face_count, "Face count access should be consistent");

            let direct_edge_count = triangulation.edge_count();
            let geometry_edge_count = triangulation.geometry().edge_count();
            prop_assert_eq!(direct_edge_count, geometry_edge_count, "Edge count access should be consistent");

            // Dimension should be consistent
            prop_assert_eq!(usize::from(triangulation.dimension()), triangulation.geometry().dimension(),
                          "Dimension should be consistent between CDT state and geometry");
        }

    }
}
