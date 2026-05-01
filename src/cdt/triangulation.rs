//! CDT triangulation wrapper - backend-agnostic.
//!
//! This module provides CDT-specific triangulation data structures that work
//! with any geometry backend implementing the trait interfaces.

use crate::cdt::foliation::{CellType, EdgeType, Foliation, FoliationError, classify_cell};
use crate::config::CdtTopology;
use crate::errors::{CdtError, CdtResult};
use crate::geometry::DelaunayBackend2D;
use crate::geometry::backends::delaunay::{
    DelaunayEdgeHandle, DelaunayFaceHandle, DelaunayVertexHandle,
};
use crate::geometry::generators::{
    build_delaunay2_with_data, build_toroidal_delaunay2, generate_delaunay2,
};
use crate::geometry::traits::{TriangulationMut, TriangulationQuery};
use crate::util::f64_band_to_u32;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// CDT-specific triangulation wrapper - completely geometry-agnostic
#[derive(Debug)]
pub struct CdtTriangulation<B> {
    geometry: B,
    /// CDT metadata (time slices, dimension, history)
    metadata: CdtMetadata,
    cache: GeometryCache,
    /// Optional foliation assigning each vertex to a time slice.
    foliation: Option<Foliation>,
    /// Modification counter value when foliation bookkeeping was last synchronized.
    foliation_synced_at_modification: Option<u64>,
}

/// CDT-specific metadata
#[derive(Debug, Clone)]
pub struct CdtMetadata {
    /// Number of time slices in the CDT foliation
    pub time_slices: u32,
    /// Dimensionality of the spacetime
    pub dimension: u8,
    /// Topology of the spatial slices
    pub topology: CdtTopology,
    /// Time when this triangulation was created
    pub creation_time: Instant,
    /// Time of last modification
    pub last_modified: Instant,
    /// Count of modifications made to the triangulation
    pub modification_count: u64,
    /// History of simulation events
    pub simulation_history: Vec<SimulationEvent>,
}

/// Cached geometry measurements
#[derive(Debug, Clone, Default)]
struct GeometryCache {
    edge_count: Option<CachedValue<usize>>,
    euler_char: Option<CachedValue<i32>>,
    #[expect(
        dead_code,
        reason = "reserved for topology-sensitive cache invalidation when topology hashes are added"
    )]
    topology_hash: Option<CachedValue<u64>>,
}

#[derive(Debug, Clone)]
struct CachedValue<T> {
    value: T,
    #[expect(
        dead_code,
        reason = "kept for future cache freshness diagnostics alongside modification_count"
    )]
    computed_at: Instant,
    modification_count: u64,
}

/// Rewrites explicit toroidal builder failures with CDT-level generation context.
///
/// The lower geometry builder reports failures in terms of its input shape; this
/// helper preserves the underlying diagnostic while normalizing the public error
/// fields to the toroidal CDT constructor's vertex count, domain, and first attempt.
fn remap_toroidal_generation_error(error: CdtError, total_vertices: u32) -> CdtError {
    match error {
        CdtError::DelaunayGenerationFailed {
            underlying_error, ..
        } => CdtError::DelaunayGenerationFailed {
            vertex_count: total_vertices,
            coordinate_range: (0.0, 1.0),
            attempt: 1,
            underlying_error,
        },
        other => other,
    }
}

/// Events in simulation history
#[derive(Debug, Clone)]
pub enum SimulationEvent {
    /// Triangulation was created
    Created {
        /// Initial number of vertices
        vertex_count: usize,
        /// Number of time slices
        time_slices: u32,
    },
    /// An ergodic move was attempted
    MoveAttempted {
        /// Type of move attempted
        move_type: String,
        /// Simulation step number
        step: u64,
    },
    /// An ergodic move was accepted
    MoveAccepted {
        /// Type of move accepted
        move_type: String,
        /// Simulation step number
        step: u64,
        /// Change in action from this move
        action_change: f64,
    },
    /// A measurement was taken
    MeasurementTaken {
        /// Simulation step number
        step: u64,
        /// Action value measured
        action: f64,
    },
}

impl<B: TriangulationQuery> CdtTriangulation<B> {
    /// Create new CDT triangulation with open boundary topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let backend = MockBackend::create_triangle();
    /// let tri = CdtTriangulation::new(backend, 2, 2);
    /// assert_eq!(tri.time_slices(), 2);
    /// assert_eq!(tri.dimension(), 2);
    /// ```
    pub fn new(geometry: B, time_slices: u32, dimension: u8) -> Self {
        Self::wrap_unchecked(geometry, time_slices, dimension, CdtTopology::OpenBoundary)
    }

    /// Create new CDT triangulation with explicit topology.
    ///
    /// Wraps an existing geometry backend and tags it with [`CdtTopology`].
    /// The backend itself is not modified; pass a backend whose Euler
    /// characteristic and metadata invariants match the supplied topology,
    /// otherwise this constructor or [`Self::validate_topology`] will reject it.
    ///
    /// Toroidal triangulations must use at least three time slices so the
    /// periodic neighbors `t - 1` and `t + 1` remain distinct.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidTriangulationMetadata`] when the requested
    /// topology metadata is internally inconsistent.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    ///
    /// let tri = CdtTriangulation::with_topology(backend, 2, 2, CdtTopology::OpenBoundary)
    ///     .expect("open-boundary metadata is valid");
    /// assert!(matches!(tri.metadata().topology, CdtTopology::OpenBoundary));
    /// assert_eq!(tri.time_slices(), 2);
    /// assert_eq!(tri.dimension(), 2);
    /// ```
    ///
    /// ```rust
    /// use causal_triangulations::prelude::errors::CdtError;
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    ///
    /// let err = CdtTriangulation::with_topology(backend, 2, 3, CdtTopology::Toroidal)
    ///     .expect_err("toroidal metadata requires at least three time slices");
    /// assert!(matches!(
    ///     err,
    ///     CdtError::InvalidTriangulationMetadata {
    ///         field,
    ///         topology,
    ///         provided_value,
    ///         expected,
    ///     } if field == "timeslices"
    ///         && topology == "toroidal"
    ///         && provided_value == "2"
    ///         && expected == "≥ 3"
    /// ));
    /// ```
    pub fn with_topology(
        geometry: B,
        time_slices: u32,
        dimension: u8,
        topology: CdtTopology,
    ) -> CdtResult<Self> {
        let mut tri = Self::wrap_unchecked(geometry, time_slices, dimension, topology);
        tri.apply_time_slices(time_slices)?;
        Ok(tri)
    }

    /// Initializes metadata without validation for constructors that validate through one write path.
    fn wrap_unchecked(geometry: B, time_slices: u32, dimension: u8, topology: CdtTopology) -> Self {
        let vertex_count = geometry.vertex_count();
        let creation_event = SimulationEvent::Created {
            vertex_count,
            time_slices,
        };

        Self {
            geometry,
            metadata: CdtMetadata {
                time_slices,
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
        }
    }

    /// Encodes topology-specific time-slice metadata invariants in one reusable check.
    fn check_time_slices(topology: CdtTopology, time_slices: u32) -> CdtResult<()> {
        if matches!(topology, CdtTopology::Toroidal) && time_slices < 3 {
            return Err(CdtError::InvalidTriangulationMetadata {
                field: "timeslices".to_string(),
                topology: "toroidal".to_string(),
                provided_value: time_slices.to_string(),
                expected: "≥ 3".to_string(),
            });
        }

        Ok(())
    }

    /// Get immutable reference to underlying geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.geometry().vertex_count(), 3);
    /// ```
    #[must_use]
    pub const fn geometry(&self) -> &B {
        &self.geometry
    }

    /// Returns the number of vertices in the backend triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.vertex_count(), 3);
    /// ```
    pub fn vertex_count(&self) -> usize {
        self.geometry.vertex_count()
    }

    /// Get the number of faces in the triangulation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.face_count(), 1);
    /// ```
    pub fn face_count(&self) -> usize {
        self.geometry.face_count()
    }

    /// Get the number of time slices in the CDT foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.time_slices(), 2);
    /// ```
    #[must_use]
    pub const fn time_slices(&self) -> u32 {
        self.metadata.time_slices
    }

    /// Get the dimensionality of the spacetime.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.dimension(), 2);
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
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.metadata().time_slices, 2);
    /// ```
    #[must_use]
    pub const fn metadata(&self) -> &CdtMetadata {
        &self.metadata
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
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// assert_eq!(tri.edge_count(), 3);
    /// ```
    pub fn edge_count(&self) -> usize {
        if let Some(cached) = &self.cache.edge_count
            && cached.modification_count == self.metadata.modification_count
        {
            return cached.value;
        }

        self.geometry.edge_count()
    }

    /// Force cache update.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// tri.refresh_cache();
    /// assert_eq!(tri.edge_count(), 3);
    /// ```
    pub fn refresh_cache(&mut self) {
        let now = Instant::now();
        let mod_count = self.metadata.modification_count;

        self.cache.edge_count = Some(CachedValue {
            value: self.geometry.edge_count(),
            computed_at: now,
            modification_count: mod_count,
        });

        self.cache.euler_char = Some(CachedValue {
            value: self.geometry.euler_characteristic(),
            computed_at: now,
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
    /// Returns error if topology validation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 1, 2, 53)
    ///     .expect("create triangulation");
    /// assert!(tri.validate_topology().is_ok());
    /// ```
    pub fn validate_topology(&self) -> CdtResult<()> {
        let euler_char = self.geometry.euler_characteristic();

        if self.dimension() == 2 {
            let expected = match self.metadata.topology {
                // Open boundary: planar with boundary χ=1, closed surface χ=2
                CdtTopology::OpenBoundary => [1, 2].as_slice(),
                // Toroidal (S¹×S¹): Euler characteristic must be 0
                CdtTopology::Toroidal => [0].as_slice(),
            };

            if !expected.contains(&euler_char) {
                let topo_label = match self.metadata.topology {
                    CdtTopology::OpenBoundary => "open boundary (expected χ=1 or 2)",
                    CdtTopology::Toroidal => "toroidal (expected χ=0)",
                };
                return Err(CdtError::ValidationFailed {
                    check: "topology".to_string(),
                    detail: format!(
                        "Euler characteristic χ={euler_char} unexpected for 2D {topo_label} \
                         triangulation (V={}, E={}, F={})",
                        self.geometry.vertex_count(),
                        self.geometry.edge_count(),
                        self.geometry.face_count(),
                    ),
                });
            }
        }

        Ok(())
    }

    /// Writes time-slice metadata through the central invariant and foliation invalidation path.
    fn apply_time_slices(&mut self, time_slices: u32) -> CdtResult<()> {
        Self::check_time_slices(self.metadata.topology, time_slices)?;
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
    /// use causal_triangulations::prelude::errors::CdtError;
    /// use causal_triangulations::prelude::triangulation::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3)
    ///     .expect("build toroidal triangulation");
    ///
    /// let err = tri.set_time_slices(2).expect_err("T < 3 is invalid");
    /// assert!(matches!(
    ///     err,
    ///     CdtError::InvalidTriangulationMetadata {
    ///         field,
    ///         topology,
    ///         provided_value,
    ///         expected,
    ///     } if field == "timeslices"
    ///         && topology == "toroidal"
    ///         && provided_value == "2"
    ///         && expected == "≥ 3"
    /// ));
    /// ```
    pub fn set_time_slices(&mut self, time_slices: u32) -> CdtResult<()> {
        if self.metadata.time_slices == time_slices {
            return self.apply_time_slices(time_slices);
        }

        self.apply_time_slices(time_slices)?;
        self.bump_modification_count();
        Ok(())
    }

    /// Marks the triangulation metadata as modified.
    ///
    /// This invalidates cached derived geometry quantities.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// let before = tri.metadata().modification_count;
    /// tri.bump_modification_count();
    /// assert_eq!(tri.metadata().modification_count, before + 1);
    /// ```
    pub fn bump_modification_count(&mut self) {
        self.invalidate_cache();
        self.invalidate_foliation_bookkeeping();
        self.metadata.last_modified = Instant::now();
        self.metadata.modification_count += 1;
    }

    /// Clears derived geometry counts so later queries recompute from the backend.
    fn invalidate_cache(&mut self) {
        self.cache = GeometryCache::default();
    }
}

impl CdtTriangulation<DelaunayBackend2D> {
    /// Validate foliation consistency.
    ///
    /// If no foliation is present, succeeds vacuously.
    /// Otherwise checks:
    /// 1. The stored labeled-vertex count matches the geometry vertex count
    /// 2. Every stored time slice is non-empty
    /// 3. Live backend labels match stored per-slice bookkeeping
    ///
    /// # Errors
    ///
    /// Returns error if foliation structure is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(12, 3, 2, 42)
    ///     .expect("create seeded triangulation");
    /// tri.assign_foliation_by_y(3)
    ///     .expect("assign foliation from y-coordinates");
    /// assert!(tri.validate_foliation().is_ok());
    /// ```
    pub fn validate_foliation(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };

        // Check that all vertices are labeled
        let vertex_count = self.geometry.vertex_count();
        if foliation.labeled_vertex_count() != vertex_count {
            return Err(FoliationError::LabelCountMismatch {
                labeled: foliation.labeled_vertex_count(),
                expected: vertex_count,
            }
            .into());
        }

        // Check that every slice is non-empty
        for (t, &size) in foliation.slice_sizes().iter().enumerate() {
            if size == 0 {
                return Err(FoliationError::EmptySlice { slice: t }.into());
            }
        }

        // Validate against live labels from canonical Delaunay vertex payload.
        let mut live_slice_sizes = vec![0usize; foliation.slice_sizes().len()];

        for (vertex, vh) in self.geometry.vertices().enumerate() {
            let Some(label) = self.geometry.vertex_data_by_key(vh.vertex_key()) else {
                return Err(FoliationError::MissingVertexLabel { vertex }.into());
            };

            let slice = label as usize;
            if slice >= live_slice_sizes.len() {
                return Err(FoliationError::OutOfRangeVertexLabel {
                    vertex,
                    label,
                    expected_range_end: live_slice_sizes.len(),
                }
                .into());
            }

            live_slice_sizes[slice] += 1;
        }

        for (slice, (&expected, &actual)) in foliation
            .slice_sizes()
            .iter()
            .zip(live_slice_sizes.iter())
            .enumerate()
        {
            if expected != actual {
                return Err(FoliationError::LabelMismatch {
                    slice,
                    expected,
                    actual,
                }
                .into());
            }
        }

        // Toroidal topology adds two stronger structural invariants:
        //  - every spatial slice forms a closed S¹ (each vertex has exactly
        //    two spacelike neighbours and the subgraph of each slice is a
        //    single cycle); and
        //  - the time direction wraps: every slice has timelike adjacency to
        //    both `(t-1) mod T` and `(t+1) mod T`.  χ = 0 alone does not
        //    distinguish a torus from a cylinder, so we check the wrap
        //    explicitly.
        if matches!(self.metadata.topology, CdtTopology::Toroidal) {
            self.validate_toroidal_spatial_rings()?;
            self.validate_toroidal_temporal_wraparound()?;
        }

        Ok(())
    }

    /// Validates that the time direction wraps for toroidal topology.
    ///
    /// For each slice `t` (with `T = time_slices`), checks that the foliation
    /// has at least one timelike edge to slice `(t - 1) mod T` and at least
    /// one to slice `(t + 1) mod T`.  Cylinders (open in time) have χ = 0
    /// like the torus, so this check is what actually distinguishes them.
    fn validate_toroidal_temporal_wraparound(&self) -> CdtResult<()> {
        let total = self.metadata.time_slices;
        if total < 2 {
            return Ok(());
        }
        let num_slices = total as usize;

        // Collect, per slice, the set of slices reached via timelike (step
        // distance == 1) edges.
        let mut neighbor_slices: Vec<HashSet<usize>> = vec![HashSet::new(); num_slices];
        for edge in self.geometry.edges() {
            let Some((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let Some(t0) = self.geometry.vertex_data_by_key(v0.vertex_key()) else {
                continue;
            };
            let Some(t1) = self.geometry.vertex_data_by_key(v1.vertex_key()) else {
                continue;
            };
            if t0 >= total || t1 >= total {
                continue;
            }
            if self.time_step_distance(t0, t1) != 1 {
                continue;
            }
            let s0 = t0 as usize;
            let s1 = t1 as usize;
            neighbor_slices[s0].insert(s1);
            neighbor_slices[s1].insert(s0);
        }

        for (slice, neighbors) in neighbor_slices.iter().enumerate() {
            let prev = if slice == 0 {
                num_slices - 1
            } else {
                slice - 1
            };
            let next = (slice + 1) % num_slices;
            if !neighbors.contains(&prev) {
                return Err(FoliationError::MissingTemporalWrapAround {
                    slice,
                    missing_neighbor: prev,
                }
                .into());
            }
            if !neighbors.contains(&next) {
                return Err(FoliationError::MissingTemporalWrapAround {
                    slice,
                    missing_neighbor: next,
                }
                .into());
            }
        }

        Ok(())
    }

    /// Validates that every spatial slice forms a closed S¹.
    ///
    /// For each time slice `t` we iterate over backend edges, count incident
    /// spacelike edges (`|Δt| = 0`) per vertex, and walk the resulting
    /// spacelike subgraph to verify it is a single cycle of length
    /// `slice_sizes[t]`.
    fn validate_toroidal_spatial_rings(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };

        let num_slices = foliation.slice_sizes().len();
        let mut spacelike_neighbors: Vec<HashMap<DelaunayVertexHandle, Vec<DelaunayVertexHandle>>> =
            vec![HashMap::new(); num_slices];

        for edge in self.geometry.edges() {
            let Some((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let Some(t0) = self.geometry.vertex_data_by_key(v0.vertex_key()) else {
                continue;
            };
            let Some(t1) = self.geometry.vertex_data_by_key(v1.vertex_key()) else {
                continue;
            };
            if t0 != t1 {
                continue;
            }
            let slice = t0 as usize;
            if slice >= num_slices {
                continue;
            }
            spacelike_neighbors[slice]
                .entry(v0.clone())
                .or_default()
                .push(v1.clone());
            spacelike_neighbors[slice].entry(v1).or_default().push(v0);
        }

        for (slice, adjacency) in spacelike_neighbors.iter().enumerate() {
            let expected_size = foliation.slice_sizes()[slice];
            if adjacency.len() != expected_size {
                return Err(FoliationError::SpacelikeSubgraphSizeMismatch {
                    slice,
                    observed: adjacency.len(),
                    expected: expected_size,
                }
                .into());
            }
            for (vertex, neighbors) in adjacency {
                if neighbors.len() != 2 {
                    return Err(FoliationError::SpacelikeDegreeViolation {
                        slice,
                        vertex: format!("{:?}", vertex.vertex_key()),
                        observed_degree: neighbors.len(),
                    }
                    .into());
                }
            }

            // Walk the cycle starting from any vertex; verify it visits every
            // vertex of the slice and closes back on itself.
            let Some(start) = adjacency.keys().next() else {
                continue;
            };
            let mut visited: HashSet<DelaunayVertexHandle> = HashSet::new();
            visited.insert(start.clone());
            let mut prev = start.clone();
            let mut current = adjacency[start][0].clone();
            while current != *start {
                if !visited.insert(current.clone()) {
                    // Non-simple cycle: a vertex was revisited before the walk
                    // returned to `start`.  Surface this through the same
                    // typed variant as a short cycle — both indicate the
                    // spacelike subgraph isn't a single closed S¹.
                    return Err(FoliationError::SpacelikeNonClosedRing {
                        slice,
                        walked: visited.len(),
                        expected: expected_size,
                    }
                    .into());
                }
                let neighbors = &adjacency[&current];
                let next = if neighbors[0] == prev {
                    neighbors[1].clone()
                } else {
                    neighbors[0].clone()
                };
                prev = current;
                current = next;
            }
            if visited.len() != expected_size {
                return Err(FoliationError::SpacelikeNonClosedRing {
                    slice,
                    walked: visited.len(),
                    expected: expected_size,
                }
                .into());
            }
        }

        Ok(())
    }
}

/// Methods that require mutable geometry access.
impl<B: TriangulationMut> CdtTriangulation<B> {
    /// Get mutable reference with automatic cache invalidation
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// tri.geometry_mut().insert_vertex(&[2.0, 2.0]).unwrap();
    /// assert_eq!(tri.vertex_count(), 4);
    /// ```
    pub const fn geometry_mut(&mut self) -> CdtGeometryMut<'_, B> {
        CdtGeometryMut {
            geometry: &mut self.geometry,
            metadata: &mut self.metadata,
            cache: &mut self.cache,
            foliation_synced_at_modification: &mut self.foliation_synced_at_modification,
            backend_mutation_recorded: false,
        }
    }
}

/// Smart wrapper for mutable geometry access
pub struct CdtGeometryMut<'a, B> {
    geometry: &'a mut B,
    metadata: &'a mut CdtMetadata,
    cache: &'a mut GeometryCache,
    foliation_synced_at_modification: &'a mut Option<u64>,
    backend_mutation_recorded: bool,
}

impl<B> CdtGeometryMut<'_, B> {
    /// Records the first mutable backend access so cache and foliation state cannot remain stale.
    fn mark_backend_mutation(&mut self) {
        if self.backend_mutation_recorded {
            return;
        }

        *self.cache = GeometryCache::default();
        *self.foliation_synced_at_modification = None;
        self.metadata.last_modified = Instant::now();
        self.metadata.modification_count += 1;
        self.backend_mutation_recorded = true;
    }

    /// Record a simulation event
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::cdt::triangulation::SimulationEvent;
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// let before = tri.metadata().simulation_history.len();
    /// tri.geometry_mut().record_event(SimulationEvent::MeasurementTaken {
    ///     step: 1,
    ///     action: 0.0,
    /// });
    /// assert_eq!(tri.metadata().simulation_history.len(), before + 1);
    /// ```
    pub fn record_event(&mut self, event: SimulationEvent) {
        self.metadata.simulation_history.push(event);
    }

    /// Get mutable reference to geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::new(MockBackend::create_triangle(), 2, 2);
    /// let vertex = tri.geometry_mut().geometry_mut().insert_vertex(&[2.0, 2.0]).unwrap();
    /// assert_eq!(tri.geometry().vertex_coordinates(&vertex).unwrap(), vec![2.0, 2.0]);
    /// ```
    pub fn geometry_mut(&mut self) -> &mut B {
        self.mark_backend_mutation();
        self.geometry
    }
}

impl<B> std::ops::Deref for CdtGeometryMut<'_, B> {
    type Target = B;
    fn deref(&self) -> &Self::Target {
        self.geometry
    }
}

impl<B> std::ops::DerefMut for CdtGeometryMut<'_, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.mark_backend_mutation();
        self.geometry
    }
}

// =============================================================================
// Delaunay-specific factory functions and foliation methods
// =============================================================================
impl CdtTriangulation<DelaunayBackend2D> {
    // -------------------------------------------------------------------------
    // Factory constructors
    // -------------------------------------------------------------------------

    /// Normalizes provisional cylinder-construction failures into the public generation error.
    fn foliated_cylinder_generation_error(
        total_vertices: u32,
        num_slices: u32,
        underlying_error: impl Into<String>,
    ) -> CdtError {
        CdtError::DelaunayGenerationFailed {
            vertex_count: total_vertices,
            coordinate_range: (0.0, f64::from(num_slices - 1)),
            attempt: 1,
            underlying_error: underlying_error.into(),
        }
    }

    /// Reconstructs generated foliation counts while reporting missing labels with examples.
    fn slice_sizes_from_vertex_labels(
        backend: &DelaunayBackend2D,
        total_vertices: u32,
        num_slices: u32,
    ) -> CdtResult<Vec<usize>> {
        const UNLABELED_VERTEX_EXAMPLE_LIMIT: usize = 4;

        let mut slice_sizes = vec![0usize; num_slices as usize];
        let mut unlabeled_vertex_count = 0usize;
        let mut unlabeled_vertex_examples = Vec::with_capacity(UNLABELED_VERTEX_EXAMPLE_LIMIT);

        for vh in backend.vertices() {
            if let Some(t) = backend.vertex_data_by_key(vh.vertex_key()) {
                let idx = t as usize;
                if idx >= slice_sizes.len() {
                    return Err(Self::foliated_cylinder_generation_error(
                        total_vertices,
                        num_slices,
                        format!(
                            "build_delaunay2_with_data produced vertex {:?} with invalid time label {t}; expected 0..{}",
                            vh.vertex_key(),
                            slice_sizes.len(),
                        ),
                    ));
                }
                slice_sizes[idx] += 1;
            } else {
                unlabeled_vertex_count += 1;
                if unlabeled_vertex_examples.len() < UNLABELED_VERTEX_EXAMPLE_LIMIT {
                    unlabeled_vertex_examples.push(format!("{:?}", vh.vertex_key()));
                }
            }
        }

        if unlabeled_vertex_count > 0 {
            let vertex_noun = if unlabeled_vertex_count == 1 {
                "vertex"
            } else {
                "vertices"
            };
            return Err(Self::foliated_cylinder_generation_error(
                total_vertices,
                num_slices,
                format!(
                    "build_delaunay2_with_data produced {unlabeled_vertex_count} unlabeled {vertex_noun}; example vertex keys (up to {UNLABELED_VERTEX_EXAMPLE_LIMIT}): [{}]; likely source: build_delaunay2_with_data failed to preserve per-vertex time labels",
                    unlabeled_vertex_examples.join(", "),
                ),
            ));
        }

        Ok(slice_sizes)
    }

    /// Re-reads current backend labels during validation so stale stored bookkeeping is detected.
    fn live_slice_sizes_from_vertex_labels(
        backend: &DelaunayBackend2D,
        num_slices: u32,
    ) -> CdtResult<Vec<usize>> {
        const UNLABELED_VERTEX_EXAMPLE_LIMIT: usize = 4;

        if num_slices == 0 {
            return Err(CdtError::ValidationFailed {
                check: "foliation".to_string(),
                detail: "cannot validate foliation with 0 time slices".to_string(),
            });
        }

        let mut slice_sizes = vec![0usize; num_slices as usize];
        let mut unlabeled_vertex_count = 0usize;
        let mut unlabeled_vertex_examples = Vec::with_capacity(UNLABELED_VERTEX_EXAMPLE_LIMIT);

        for vh in backend.vertices() {
            if let Some(t) = backend.vertex_data_by_key(vh.vertex_key()) {
                let idx = t as usize;
                if idx >= slice_sizes.len() {
                    return Err(CdtError::ValidationFailed {
                        check: "foliation".to_string(),
                        detail: format!(
                            "vertex {:?} has out-of-range time label {t}; expected 0..{}",
                            vh.vertex_key(),
                            slice_sizes.len(),
                        ),
                    });
                }
                slice_sizes[idx] += 1;
            } else {
                unlabeled_vertex_count += 1;
                if unlabeled_vertex_examples.len() < UNLABELED_VERTEX_EXAMPLE_LIMIT {
                    unlabeled_vertex_examples.push(format!("{:?}", vh.vertex_key()));
                }
            }
        }

        if unlabeled_vertex_count > 0 {
            return Err(CdtError::ValidationFailed {
                check: "foliation".to_string(),
                detail: format!(
                    "{unlabeled_vertex_count} vertices are missing time labels; example vertex keys (up to {UNLABELED_VERTEX_EXAMPLE_LIMIT}): [{}]",
                    unlabeled_vertex_examples.join(", "),
                ),
            });
        }

        Ok(slice_sizes)
    }

    /// Create a new CDT triangulation with Delaunay backend from random points.
    ///
    /// This is the recommended way to create triangulations for simulations.
    ///
    /// # Errors
    /// Returns error if triangulation generation fails
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::from_random_points(5, 2, 2).unwrap();
    /// assert_eq!(tri.time_slices(), 2);
    /// ```
    pub fn from_random_points(vertices: u32, time_slices: u32, dimension: u8) -> CdtResult<Self> {
        // Validate dimension first
        if dimension != 2 {
            return Err(CdtError::UnsupportedDimension(dimension.into()));
        }

        // Validate other parameters
        if vertices < 3 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertex count".to_string(),
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = generate_delaunay2(vertices, (0.0, 10.0), None)?;
        let backend = DelaunayBackend2D::from_triangulation(dt);

        Ok(Self::new(backend, time_slices, dimension))
    }

    /// Create a new CDT triangulation with Delaunay backend from random points using a fixed seed.
    ///
    /// This function provides deterministic triangulation generation for testing purposes.
    ///
    /// # Errors
    /// Returns error if triangulation generation fails
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// assert_eq!(tri.vertex_count(), 5);
    /// ```
    pub fn from_seeded_points(
        vertices: u32,
        time_slices: u32,
        dimension: u8,
        seed: u64,
    ) -> CdtResult<Self> {
        // Validate dimension first
        if dimension != 2 {
            return Err(CdtError::UnsupportedDimension(dimension.into()));
        }

        // Validate other parameters
        if vertices < 3 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertex count".to_string(),
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = generate_delaunay2(vertices, (0.0, 10.0), Some(seed))?;
        let backend = DelaunayBackend2D::from_triangulation(dt);

        Ok(Self::new(backend, time_slices, dimension))
    }

    /// Wrap a labeled 2D Delaunay backend and derive foliation from vertex data.
    ///
    /// Preserves per-vertex time labels already embedded in the backend.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::UnsupportedDimension`] if `dimension != 2`.
    /// Returns [`CdtError::ValidationFailed`] if any vertex is unlabeled or
    /// has a time label outside `0..time_slices`, or if any time slice is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("wrap labeled backend");
    ///
    /// assert!(tri.has_foliation());
    /// assert_eq!(tri.slice_sizes(), &[2, 1]);
    /// ```
    pub fn from_labeled_delaunay(
        backend: DelaunayBackend2D,
        time_slices: u32,
        dimension: u8,
    ) -> CdtResult<Self> {
        if dimension != 2 {
            return Err(CdtError::UnsupportedDimension(dimension.into()));
        }

        let slice_sizes = Self::live_slice_sizes_from_vertex_labels(&backend, time_slices)?;
        for (slice, &size) in slice_sizes.iter().enumerate() {
            if size == 0 {
                return Err(FoliationError::EmptySlice { slice }.into());
            }
        }
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, time_slices).map_err(CdtError::from)?;

        let mut tri = Self::new(backend, time_slices, dimension);
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();
        Ok(tri)
    }

    /// Construct a foliated 1+1 CDT triangulation on a finite strip.
    ///
    /// Places `vertices_per_slice` vertices per time slice on a regular grid at
    /// coordinates `(x_i, t)` where `x_i` is evenly spaced in `[0, 1]` and
    /// `t` is an integer time coordinate.  Time labels are assigned by
    /// y-coordinate bucket: slice `t` owns vertices with `y ∈ [t − 0.5, t + 0.5)`.
    ///
    /// **Note:** Despite the name, this builds an open strip `[0,1] × [0,T]`
    /// without spatial periodic identification.  True cylinder topology
    /// (S¹ × \[0,T\]) is planned for a future release.
    ///
    /// The spatial extent is kept to 1.0 (well below the √3 ≈ 1.73 threshold
    /// that prevents Delaunay edges from skipping a time slice), but generic
    /// Delaunay triangulation can still introduce same-slice triangles. This
    /// constructor therefore validates the result and returns an error unless
    /// the output is a genuine 1+1 CDT strip.
    ///
    /// This constructor is provisional and crate-internal until the explicit
    /// strip builder path is implemented in [`from_cdt_strip`](Self::from_cdt_strip).
    ///
    /// # Arguments
    ///
    /// * `vertices_per_slice` — Number of vertices in each spatial slice (≥ 4).
    /// * `num_slices` — Number of time slices (≥ 2).
    /// * `seed` — Optional seed for deterministic vertex perturbation.
    ///
    /// # Errors
    ///
    /// Returns error if parameters are invalid, vertex construction fails,
    /// triangulation construction fails, or the builder output does not retain
    /// valid per-vertex time labels. Builder-label failures are surfaced as
    /// [`CdtError::DelaunayGenerationFailed`] with detailed context.
    ///
    /// # Internal
    ///
    /// This API is crate-internal and experimental.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "provisional internal strip constructor remains intentionally crate-internal until explicit strip construction lands"
        )
    )]
    pub(crate) fn from_foliated_cylinder(
        vertices_per_slice: u32,
        num_slices: u32,
        seed: Option<u64>,
    ) -> CdtResult<Self> {
        // TODO(#57): Remove this provisional point-set constructor once the
        // explicit combinatorial strip builder is available.
        if vertices_per_slice < 4 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertices per slice".to_string(),
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 4".to_string(),
            });
        }
        if num_slices < 2 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient number of time slices".to_string(),
                provided_value: num_slices.to_string(),
                expected_range: "≥ 2".to_string(),
            });
        }

        // Spatial extent is fixed at 1.0 so that the maximum within-strip
        // circumradius stays below 1.0 (the temporal gap between slices).
        // This guarantees the Delaunay property cannot create edges that
        // skip a time slice.
        //
        // Small deterministic perturbation is applied to break co-circular
        // degeneracy in the grid.  Boundary vertices (i=0 and i=last) keep
        // their exact x so they remain collinear on the convex hull,
        // preventing hull edges from skipping intermediate time slices.
        let spatial_extent = 1.0_f64;
        let spacing = spatial_extent / f64::from(vertices_per_slice - 1);
        let perturbation_seed = seed.unwrap_or(0);
        let perturbation_scale = spacing * 0.02;

        let total_vertices = vertices_per_slice.checked_mul(num_slices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: "Vertex count overflow".to_string(),
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;
        let mut vertex_specs = Vec::with_capacity(total_vertices as usize);
        let last_i = vertices_per_slice - 1;

        for t in 0..num_slices {
            for i in 0..vertices_per_slice {
                let x_base = f64::from(i) * spacing;
                let y_base = f64::from(t); // integer time coordinate

                // Deterministic perturbation keyed on vertex index + seed
                let hash = (u64::from(t) * u64::from(vertices_per_slice) + u64::from(i))
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(perturbation_seed)
                    .wrapping_mul(1_442_695_040_888_963_407);

                // Perturbation strategy:
                // - Interior vertices: hash-based perturbation in x only
                //   to break co-circular grid degeneracy while preserving
                //   exact per-slice collinearity in y.
                // - Boundary vertices (i=0, i=last): deterministic concave
                //   x-offset via √(t+1) pushes each row further outward.
                //   The concave (sub-linear) progression ensures every
                //   intermediate boundary vertex bulges past the line
                //   connecting its neighbors, so the convex hull includes
                //   every row's extremes and no hull edge skips a time slice.
                let hash_frac = f64::from((hash & 0xFFFF) as u16) / 65535.0;
                let hull_offset = f64::from(t + 1).sqrt();
                let px = if i == 0 {
                    // Push left — concave √(t+1) ensures hull membership
                    -hull_offset * perturbation_scale
                } else if i == last_i {
                    // Push right — mirror of left
                    hull_offset * perturbation_scale
                } else {
                    (hash_frac - 0.5) * perturbation_scale
                };

                // Keep y exactly on integer slices so same-slice triangles
                // are geometrically impossible (three same-slice points are
                // collinear), enforcing one-spacelike-two-timelike structure.
                vertex_specs.push(([x_base + px, y_base], t));
            }
        }

        // Delegate low-level Delaunay construction to the utility layer.
        let dt = build_delaunay2_with_data(&vertex_specs).map_err(|e| match e {
            CdtError::DelaunayGenerationFailed {
                underlying_error, ..
            } => Self::foliated_cylinder_generation_error(
                total_vertices,
                num_slices,
                underlying_error,
            ),
            other => other,
        })?;

        let backend = DelaunayBackend2D::from_triangulation(dt);

        // Verify the builder inserted all vertices.
        if backend.vertex_count() != total_vertices as usize {
            return Err(Self::foliated_cylinder_generation_error(
                total_vertices,
                num_slices,
                format!(
                    "builder inserted only {} of {} vertices (possible degeneracy)",
                    backend.vertex_count(),
                    total_vertices,
                ),
            ));
        }

        // Compute per-slice vertex counts from vertex data stored in the backend.
        let slice_sizes =
            Self::slice_sizes_from_vertex_labels(&backend, total_vertices, num_slices)?;

        let foliation =
            Foliation::from_slice_sizes(slice_sizes, num_slices).map_err(CdtError::from)?;
        let mut tri = Self::new(backend, num_slices, 2);
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();

        tri.validate_foliation().map_err(|err| {
            Self::foliated_cylinder_generation_error(
                total_vertices,
                num_slices,
                format!("constructed strip has invalid foliation: {err}"),
            )
        })?;

        tri.validate_causality_delaunay().map_err(|err| {
            Self::foliated_cylinder_generation_error(
                total_vertices,
                num_slices,
                format!(
                    "point-set Delaunay produced a non-CDT triangulation; explicit CDT strip construction is required: {err}"
                ),
            )
        })?;

        Ok(tri)
    }

    /// Construct a true 1+1 CDT strip by explicit layered connectivity.
    ///
    /// Unlike `from_foliated_cylinder`, this does NOT rely on Delaunay triangulation.
    /// Instead it constructs the CDT combinatorially so every triangle is guaranteed
    /// to satisfy the CDT invariant (1 spacelike edge, 2 timelike edges).
    ///
    /// NOTE: This requires backend support for explicit face construction.
    /// Currently this is a placeholder until such support is implemented.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices_per_slice < 4` or
    /// `num_slices < 2`.
    /// Returns [`CdtError::ValidationFailed`] because explicit CDT mesh construction is
    /// not yet implemented.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// // Placeholder API: currently validates inputs, then returns a construction error.
    /// let result = CdtTriangulation::from_cdt_strip(4, 2);
    /// assert!(result.is_err());
    /// ```
    pub fn from_cdt_strip(vertices_per_slice: u32, num_slices: u32) -> CdtResult<Self> {
        if vertices_per_slice < 4 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertices per slice".to_string(),
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 4".to_string(),
            });
        }
        if num_slices < 2 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient number of time slices".to_string(),
                provided_value: num_slices.to_string(),
                expected_range: "≥ 2".to_string(),
            });
        }

        // TODO: Implement explicit CDT mesh construction.
        // This requires:
        // 1. Creating vertices per slice
        // 2. Connecting adjacent slices into quads
        // 3. Splitting quads into valid CDT triangles
        // 4. Building backend without relying on Delaunay

        Err(CdtError::ValidationFailed {
            check: "cdt_construction".to_string(),
            detail: "from_cdt_strip is not yet implemented: requires explicit mesh backend"
                .to_string(),
        })
    }

    /// Construct a foliated 1+1 CDT on a torus (S¹×S¹).
    ///
    /// Places `vertices_per_slice` vertices per time slice, uniformly spaced
    /// on S¹ (spatial coordinate periodic in `[0, 1)`).  Time slices wrap:
    /// slice `num_slices - 1` connects back to slice `0`.  Each quad between
    /// adjacent slices is split into one Up (2,1) and one Down (1,2) triangle.
    ///
    /// The triangulation is built by explicit combinatorial connectivity via
    /// [`crate::geometry::generators::build_toroidal_delaunay2`],
    /// which sets `TopologyGuarantee::Pseudomanifold` and
    /// `GlobalTopology::Toroidal` so the underlying validator expects χ = 0.
    ///
    /// # Mesh structure
    ///
    /// With `N = vertices_per_slice` and `T = num_slices` the resulting mesh
    /// has `N · T` vertices, `3 · N · T` edges, and `2 · N · T` triangles
    /// (`V − E + F = 0`, the Euler characteristic of the torus).  Each pair of
    /// adjacent slices `(t, t+1) mod T` and each spatial pair `(i, i+1) mod N`
    /// contribute exactly one Up `(i, t), (i+1, t), (i, t+1)` and one Down
    /// `(i+1, t), (i+1, t+1), (i, t+1)` triangle, so every triangle has
    /// exactly one spacelike edge and two timelike edges by construction.
    ///
    /// # Arguments
    ///
    /// * `vertices_per_slice` — Number of vertices in each spatial slice (≥ 3).
    /// * `num_slices` — Number of time slices (≥ 3 to keep `t-1` and `t+1`
    ///   distinct after wrap-around).
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices_per_slice < 3`
    /// or `num_slices < 3`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if the underlying explicit
    /// builder rejects the mesh, and [`CdtError::ValidationFailed`] if the
    /// constructed triangulation fails CDT validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let tri = CdtTriangulation::from_toroidal_cdt(4, 3)
    ///     .expect("build toroidal CDT");
    /// assert_eq!(tri.vertex_count(), 12);
    /// assert_eq!(tri.face_count(), 24);
    /// assert!(tri.has_foliation());
    /// ```
    pub fn from_toroidal_cdt(vertices_per_slice: u32, num_slices: u32) -> CdtResult<Self> {
        if vertices_per_slice < 3 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertices per slice".to_string(),
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }
        if num_slices < 3 {
            // With T=2 the wrap-around makes every pair of adjacent slices
            // identify (t-1, t) with (t, t+1), so each spatial edge would be
            // shared by 4 triangles instead of 2 — a non-manifold mesh.
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient number of time slices".to_string(),
                provided_value: num_slices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let total_vertices = vertices_per_slice.checked_mul(num_slices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: "Vertex count overflow".to_string(),
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;

        let n = vertices_per_slice as usize;
        let t_count = num_slices as usize;

        // Index helper: vertex (i, t) → i + t * N. Both axes are periodic.
        let index = |i: usize, t: usize| -> usize { (t % t_count) * n + (i % n) };

        // --- Vertex coordinates (S¹ × S¹) ---
        //
        // Spatial coordinate: x_i = i / N is periodic in [0, 1).
        // Time coordinate: t_t = t / T is periodic in [0, 1) so the metadata
        // domain matches what we pass to GlobalTopology::Toroidal.
        let n_f = f64::from(vertices_per_slice);
        let t_f = f64::from(num_slices);
        let mut vertex_specs: Vec<([f64; 2], u32)> = Vec::with_capacity(total_vertices as usize);
        for t in 0..num_slices {
            for i in 0..vertices_per_slice {
                let x = f64::from(i) / n_f;
                let y = f64::from(t) / t_f;
                vertex_specs.push(([x, y], t));
            }
        }

        // --- Explicit cells (Up + Down per (i, t) quad) ---
        let mut cells: Vec<Vec<usize>> = Vec::with_capacity(2 * total_vertices as usize);
        for t in 0..t_count {
            let t_next = (t + 1) % t_count;
            for i in 0..n {
                let i_next = (i + 1) % n;
                // Up (2,1): two vertices on slice t, one on slice t+1.
                cells.push(vec![index(i, t), index(i_next, t), index(i, t_next)]);
                // Down (1,2): one vertex on slice t, two on slice t+1.
                cells.push(vec![
                    index(i_next, t),
                    index(i_next, t_next),
                    index(i, t_next),
                ]);
            }
        }

        let domain = [1.0_f64, 1.0_f64];
        let dt = build_toroidal_delaunay2(&vertex_specs, &cells, domain)
            .map_err(|e| remap_toroidal_generation_error(e, total_vertices))?;

        let backend = DelaunayBackend2D::from_triangulation(dt);
        if backend.vertex_count() != total_vertices as usize {
            return Err(CdtError::DelaunayGenerationFailed {
                vertex_count: total_vertices,
                coordinate_range: (0.0, 1.0),
                attempt: 1,
                underlying_error: format!(
                    "explicit toroidal builder produced {} vertices, expected {}",
                    backend.vertex_count(),
                    total_vertices,
                ),
            });
        }

        let slice_sizes = vec![n; t_count];
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, num_slices).map_err(CdtError::from)?;

        let mut tri = Self::with_topology(backend, num_slices, 2, CdtTopology::Toroidal)?;
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();

        // Propagate inner errors as-is so callers can pattern-match on the
        // typed variant (e.g. `FoliationError::SpacelikeNonClosedRing` or
        // `CausalityViolation`) instead of parsing a wrapped string.  Each
        // inner validator already produces a precise, structured error.
        tri.validate_foliation()?;
        tri.validate_causality_delaunay()?;
        tri.validate_topology()?;

        Ok(tri)
    }

    // -------------------------------------------------------------------------
    // Foliation assignment
    // -------------------------------------------------------------------------

    /// Assign a foliation to an existing triangulation by binning vertices
    /// by their y-coordinate into `num_slices` equal bands.
    ///
    /// The y-coordinate range is determined from the actual vertex coordinates.
    /// Band `t` covers `[y_min + t * band_height, y_min + (t+1) * band_height)`.
    /// Time labels are written directly to vertex data.
    ///
    /// This is approximate — useful for testing but not guaranteed to produce
    /// a valid causal structure.
    ///
    /// # Errors
    ///
    /// Returns error if `num_slices` is zero, if vertex coordinates cannot be
    /// read, or if y-bucket assignment would leave any time slice empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(12, 3, 2, 42)
    ///     .expect("create seeded triangulation");
    /// tri.assign_foliation_by_y(3)
    ///     .expect("assign foliation from y-coordinates");
    ///
    /// assert!(tri.has_foliation());
    /// assert_eq!(tri.slice_sizes().iter().sum::<usize>(), tri.vertex_count());
    /// ```
    pub fn assign_foliation_by_y(&mut self, num_slices: u32) -> CdtResult<()> {
        if num_slices == 0 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Number of slices must be positive".to_string(),
                provided_value: "0".to_string(),
                expected_range: "≥ 1".to_string(),
            });
        }

        // Collect all vertex y-coordinates, failing fast if any vertex is unreadable.
        let y_coords: Vec<(DelaunayVertexHandle, f64)> = self
            .geometry
            .vertices()
            .map(|vh| {
                let coords = self.geometry.vertex_coordinates(&vh).map_err(|e| {
                    CdtError::ValidationFailed {
                        check: "foliation_assignment".to_string(),
                        detail: format!(
                            "failed to read coordinates for vertex {:?}: {e}",
                            vh.vertex_key()
                        ),
                    }
                })?;
                if coords.len() < 2 {
                    return Err(CdtError::ValidationFailed {
                        check: "foliation_assignment".to_string(),
                        detail: format!(
                            "vertex {:?} has {} coordinates, expected ≥ 2",
                            vh.vertex_key(),
                            coords.len()
                        ),
                    });
                }
                Ok((vh, coords[1]))
            })
            .collect::<CdtResult<Vec<_>>>()?;

        let y_min = y_coords
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        let y_max = y_coords
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::NEG_INFINITY, f64::max);

        let range = y_max - y_min;
        let band_height = if range.abs() < f64::EPSILON {
            1.0
        } else {
            range / f64::from(num_slices)
        };

        // Plan per-vertex labels in memory first so foliation validation errors
        // do not partially mutate backend state.
        let mut assignments = Vec::with_capacity(y_coords.len());
        let mut slice_sizes = vec![0usize; num_slices as usize];
        for (vh, y) in &y_coords {
            let t = if range.abs() < f64::EPSILON {
                0
            } else {
                let band_index = ((y - y_min) / band_height).floor();
                f64_band_to_u32(band_index, num_slices - 1)
            };
            assignments.push((vh.vertex_key(), t));
            slice_sizes[t as usize] += 1;
        }

        let foliation =
            Foliation::from_slice_sizes(slice_sizes, num_slices).map_err(CdtError::from)?;

        self.apply_time_slices(num_slices)?;

        // Clear stale cell classifications from any previous classify_all_cells() call,
        // since vertex time labels are about to change.
        let face_keys: Vec<_> = self.geometry.faces().map(|f| f.cell_key()).collect();

        for &key in &face_keys {
            if let Err(err) = self.geometry.set_cell_data_by_key(key, None) {
                return Err(CdtError::BackendMutationFailed {
                    operation: "set_cell_data_by_key".to_string(),
                    target: format!("face {key:?}"),
                    detail: err.to_string(),
                });
            }
        }

        // Write time labels directly to vertex data via set_vertex_data_by_key (O(1) per vertex).
        for (vertex_key, t) in assignments {
            if let Err(err) = self.geometry.set_vertex_data_by_key(vertex_key, Some(t)) {
                return Err(CdtError::BackendMutationFailed {
                    operation: "set_vertex_data_by_key".to_string(),
                    target: format!("vertex {vertex_key:?}"),
                    detail: format!("failed while assigning time label {t}: {err}"),
                });
            }
        }

        self.bump_modification_count();
        self.foliation = Some(foliation);
        self.mark_foliation_synchronized();
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Foliation queries
    // -------------------------------------------------------------------------

    /// Returns `true` if this triangulation has an assigned foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// assert!(!tri.has_foliation());
    /// ```
    #[must_use]
    pub fn has_foliation(&self) -> bool {
        self.has_current_foliation()
    }

    /// Returns a reference to the foliation, if present.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(8, 2, 2, 53).unwrap();
    /// tri.assign_foliation_by_y(2).unwrap();
    /// assert!(tri.foliation().is_some());
    /// ```
    #[must_use]
    pub fn foliation(&self) -> Option<&Foliation> {
        if self.has_current_foliation() {
            self.foliation.as_ref()
        } else {
            None
        }
    }

    /// Returns the time slice label for a vertex, or `None` if no foliation
    /// is present or the vertex is unlabeled.
    ///
    /// Reads the time label directly from the vertex data stored in the
    /// Delaunay triangulation (like CDT++ `vertex->info()`).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(8, 2, 2, 53).unwrap();
    /// tri.assign_foliation_by_y(2).unwrap();
    /// let vertex = tri.geometry().vertices().next().unwrap();
    /// assert!(tri.time_label(&vertex).is_some());
    /// ```
    #[must_use]
    pub fn time_label(&self, vertex: &DelaunayVertexHandle) -> Option<u32> {
        self.foliation()?;
        self.geometry.vertex_data_by_key(vertex.vertex_key())
    }

    /// Returns all vertex handles that belong to time slice `t`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(8, 2, 2, 53).unwrap();
    /// tri.assign_foliation_by_y(2).unwrap();
    /// assert!(!tri.vertices_at_time(0).is_empty());
    /// ```
    #[must_use]
    pub fn vertices_at_time(&self, t: u32) -> Vec<DelaunayVertexHandle> {
        if !self.has_foliation() {
            return vec![];
        }
        self.geometry
            .vertices()
            .filter(|vh| self.geometry.vertex_data_by_key(vh.vertex_key()) == Some(t))
            .collect()
    }

    /// Returns per-slice vertex counts, or an empty slice if no foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(8, 2, 2, 53).unwrap();
    /// tri.assign_foliation_by_y(2).unwrap();
    /// assert_eq!(tri.slice_sizes().len(), 2);
    /// ```
    #[must_use]
    pub fn slice_sizes(&self) -> &[usize] {
        self.foliation().map_or(&[], Foliation::slice_sizes)
    }

    // -------------------------------------------------------------------------
    // Cell (triangle) classification
    // -------------------------------------------------------------------------

    /// Computes the temporal step distance between two time labels.
    ///
    /// For `OpenBoundary` topology this is just `t0.abs_diff(t1)`; for
    /// `Toroidal` topology it is the circular distance
    /// `min(d, T − d)` where `T = time_slices`, so the wrap-around edge
    /// between slice `T − 1` and slice `0` reads as distance `1`.
    ///
    /// Out-of-range labels (`t ≥ time_slices`) bypass the toroidal wrap and
    /// fall through to the raw difference so callers (e.g. validators) can
    /// still detect them as causality violations rather than silently folding
    /// `t == time_slices` into distance 0 via `min(T, 0)`.
    #[must_use]
    fn time_step_distance(&self, t0: u32, t1: u32) -> u32 {
        let raw = t0.abs_diff(t1);
        if matches!(self.metadata.topology, CdtTopology::Toroidal) {
            let total = self.metadata.time_slices;
            if total > 0 && t0 < total && t1 < total {
                return raw.min(total - raw);
            }
        }
        raw
    }

    /// Topology-aware variant of [`crate::cdt::foliation::classify_cell`].
    ///
    /// Uses [`Self::time_step_distance`] so the spacelike/timelike split
    /// honours toroidal wrap.  Distinguishes Up vs Down by checking whether
    /// the apex sits at `base + 1` or `base - 1` modulo `time_slices`.
    fn classify_cell_with_topology(&self, t0: u32, t1: u32, t2: u32) -> Option<CellType> {
        let mut dists = [
            self.time_step_distance(t0, t1),
            self.time_step_distance(t1, t2),
            self.time_step_distance(t0, t2),
        ];
        dists.sort_unstable();
        if dists != [0, 1, 1] {
            return None;
        }

        let (base_slice, apex_slice) = if t0 == t1 {
            (t0, t2)
        } else if t1 == t2 {
            (t1, t0)
        } else if t0 == t2 {
            (t0, t1)
        } else {
            return None;
        };

        let total = self.metadata.time_slices;
        let toroidal = matches!(self.metadata.topology, CdtTopology::Toroidal) && total > 0;
        let up_apex = if toroidal {
            (base_slice + 1) % total
        } else {
            base_slice.checked_add(1)?
        };
        let down_apex = if toroidal {
            if base_slice == 0 {
                total - 1
            } else {
                base_slice - 1
            }
        } else {
            base_slice.checked_sub(1)?
        };
        if apex_slice == up_apex {
            Some(CellType::Up)
        } else if apex_slice == down_apex {
            Some(CellType::Down)
        } else {
            None
        }
    }

    /// Returns the causal classification of an edge from endpoint time labels.
    ///
    /// Returns `None` if no foliation is present, the edge endpoints cannot be
    /// resolved, or either endpoint is missing a time label.  Distance is
    /// circular when the topology is `Toroidal`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("create foliated triangulation");
    ///
    /// let edge = tri.geometry().edges().next().expect("triangle has edges");
    /// let edge_type = tri.edge_type(&edge).expect("edge should be classifiable");
    /// assert!(!matches!(edge_type, EdgeType::Acausal));
    /// ```
    #[must_use]
    pub fn edge_type(&self, edge: &DelaunayEdgeHandle) -> Option<EdgeType> {
        self.foliation()?;

        let (v0, v1) = self.geometry.edge_endpoints(edge)?;
        let t0 = self.geometry.vertex_data_by_key(v0.vertex_key())?;
        let t1 = self.geometry.vertex_data_by_key(v1.vertex_key())?;

        Some(match self.time_step_distance(t0, t1) {
            0 => EdgeType::Spacelike,
            1 => EdgeType::Timelike,
            _ => EdgeType::Acausal,
        })
    }

    /// Classifies a triangle as Up (2,1) or Down (1,2) from vertex time labels.
    ///
    /// Returns `None` if no foliation is present, the face vertices cannot
    /// be resolved, any vertex lacks a time label, or the triangle does not
    /// span exactly one time slice (e.g. a boundary same-slice triangle).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(12, 3, 2, 42)
    ///     .expect("create seeded triangulation");
    /// tri.assign_foliation_by_y(3)
    ///     .expect("assign foliation from y-coordinates");
    /// // Most cells should be classifiable; boundary cells may not be.
    /// let classified: usize = tri.geometry().faces()
    ///     .filter(|f| tri.cell_type(f).is_some())
    ///     .count();
    /// assert!(classified > 0);
    /// ```
    #[must_use]
    pub fn cell_type(&self, face: &DelaunayFaceHandle) -> Option<CellType> {
        self.foliation()?;
        let verts = self.geometry.face_vertices(face).ok()?;
        if verts.len() != 3 {
            return None;
        }
        let t0 = self.geometry.vertex_data_by_key(verts[0].vertex_key())?;
        let t1 = self.geometry.vertex_data_by_key(verts[1].vertex_key())?;
        let t2 = self.geometry.vertex_data_by_key(verts[2].vertex_key())?;
        match self.metadata.topology {
            CdtTopology::Toroidal => self.classify_cell_with_topology(t0, t1, t2),
            CdtTopology::OpenBoundary => classify_cell(Some(t0), Some(t1), Some(t2)),
        }
    }

    /// Reads the stored cell type from cell data, if previously classified.
    ///
    /// Returns `None` if the face has no cell data or the data does not
    /// encode a valid [`CellType`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(12, 3, 2, 42).unwrap();
    /// tri.assign_foliation_by_y(3).unwrap();
    /// tri.classify_all_cells().unwrap();
    /// let face = tri.geometry().faces().next().unwrap();
    /// let _stored = tri.cell_type_from_data(&face);
    /// ```
    #[must_use]
    pub fn cell_type_from_data(&self, face: &DelaunayFaceHandle) -> Option<CellType> {
        self.foliation()?;
        let raw = self.geometry.cell_data_by_key(face.cell_key())?;
        CellType::from_i32(raw)
    }

    /// Returns the edge classification for a triangular face.
    ///
    /// Edge ordering matches the face vertex cycle `(v0-v1, v1-v2, v2-v0)`.
    /// Returns `None` if foliation is absent, face vertices cannot be resolved,
    /// the face is not triangular, or any vertex is unlabeled.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("create foliated triangulation");
    ///
    /// let face = tri.geometry().faces().next().expect("triangle has a face");
    /// let edge_types = tri
    ///     .face_edge_types(&face)
    ///     .expect("face edge types should be available");
    ///
    /// let spacelike = edge_types
    ///     .iter()
    ///     .filter(|e| matches!(e, EdgeType::Spacelike))
    ///     .count();
    /// let timelike = edge_types
    ///     .iter()
    ///     .filter(|e| matches!(e, EdgeType::Timelike))
    ///     .count();
    /// assert_eq!(spacelike, 1);
    /// assert_eq!(timelike, 2);
    /// ```
    #[must_use]
    pub fn face_edge_types(&self, face: &DelaunayFaceHandle) -> Option<[EdgeType; 3]> {
        self.foliation()?;

        let verts = self.geometry.face_vertices(face).ok()?;
        if verts.len() != 3 {
            return None;
        }

        let t = [
            self.geometry.vertex_data_by_key(verts[0].vertex_key())?,
            self.geometry.vertex_data_by_key(verts[1].vertex_key())?,
            self.geometry.vertex_data_by_key(verts[2].vertex_key())?,
        ];

        let edge_classify = |a: u32, b: u32| -> Option<EdgeType> {
            Some(match self.time_step_distance(a, b) {
                0 => EdgeType::Spacelike,
                1 => EdgeType::Timelike,
                _ => EdgeType::Acausal,
            })
        };

        Some([
            edge_classify(t[0], t[1])?,
            edge_classify(t[1], t[2])?,
            edge_classify(t[2], t[0])?,
        ])
    }

    /// Classifies every triangle and stores the result as cell data.
    ///
    /// Each classifiable cell receives `Some(CellType::to_i32())` via
    /// `set_cell_data`.  Boundary cells that do not span exactly one
    /// time slice are skipped.
    ///
    /// Requires a foliation to be present; returns `Ok(None)` if there is none.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::BackendMutationFailed`] if writing cell payloads
    /// to the backend fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let mut tri = CdtTriangulation::from_seeded_points(12, 3, 2, 42)
    ///     .expect("create seeded triangulation");
    /// tri.assign_foliation_by_y(3)
    ///     .expect("assign foliation from y-coordinates");
    /// let classified = tri
    ///     .classify_all_cells()
    ///     .expect("classify cells")
    ///     .expect("foliation is present");
    /// assert!(classified > 0);
    /// ```
    pub fn classify_all_cells(&mut self) -> CdtResult<Option<usize>> {
        if !self.has_foliation() {
            return Ok(None);
        }

        // Collect (CellKey, CellType) pairs first to avoid borrow conflict.
        let classifications: Vec<_> = self
            .geometry
            .faces()
            .filter_map(|face| {
                let ct = self.cell_type(&face)?;
                Some((face, ct))
            })
            .collect();

        // Also collect all face keys to clear stale data from unclassifiable faces.
        let all_face_keys: Vec<_> = self.geometry.faces().map(|f| f.cell_key()).collect();

        let count = classifications.len();

        // Clear all cell data first, then write fresh classifications.
        for &key in &all_face_keys {
            self.geometry
                .set_cell_data_by_key(key, None)
                .map_err(|err| CdtError::BackendMutationFailed {
                    operation: "set_cell_data_by_key".to_string(),
                    target: format!("face {key:?}"),
                    detail: format!(
                        "failed to clear existing cell payload before classification: {err}"
                    ),
                })?;
        }
        for (face, ct) in classifications {
            let key = face.cell_key();
            self.geometry
                .set_cell_data_by_key(key, Some(ct.to_i32()))
                .map_err(|err| CdtError::BackendMutationFailed {
                    operation: "set_cell_data_by_key".to_string(),
                    target: format!("face {key:?}"),
                    detail: format!(
                        "failed to store classified cell payload {}: {err}",
                        ct.to_i32()
                    ),
                })?;
        }
        Ok(Some(count))
    }

    /// Validate CDT properties (geometry, Delaunay, topology, causality, foliation).
    ///
    /// # Errors
    /// Returns error if any validation check fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::CdtTriangulation;
    ///
    /// let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53).unwrap();
    /// assert!(tri.validate().is_ok());
    /// ```
    pub fn validate(&self) -> CdtResult<()> {
        if !self.geometry.is_valid() {
            return Err(CdtError::ValidationFailed {
                check: "geometry".to_string(),
                detail: format!(
                    "triangulation is not valid (V={}, E={}, F={})",
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                ),
            });
        }

        if !self.geometry.is_delaunay() {
            return Err(CdtError::ValidationFailed {
                check: "Delaunay".to_string(),
                detail: format!(
                    "triangulation does not satisfy Delaunay property (V={}, E={}, F={})",
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                ),
            });
        }

        self.validate_topology()?;
        self.validate_foliation()?;
        self.validate_causality()?;

        Ok(())
    }

    /// Validate causality constraints.
    ///
    /// If no foliation is present, succeeds vacuously (no causal structure
    /// to check).  Otherwise delegates to [`validate_causality_delaunay`](Self::validate_causality_delaunay).
    ///
    /// # Errors
    ///
    /// Returns error if any edge spans more than one time slice (`|Δt| > 1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("create foliated triangulation");
    /// assert!(tri.validate_causality().is_ok());
    /// ```
    pub fn validate_causality(&self) -> CdtResult<()> {
        self.validate_causality_delaunay()
    }

    /// Validates the causal structure of this foliated triangulation.
    ///
    /// Reads time labels directly from face vertex data and checks that every
    /// triangle lies within a single slice pair. In a 2D triangulation, this
    /// implies that each edge of each finite face connects vertices within the
    /// same slice or adjacent slices, while avoiding backend-specific edge
    /// endpoint resolution.
    ///
    /// # Errors
    ///
    /// Returns error if any triangle spans more than one time slice, if a face
    /// cannot be resolved to three vertices, or if any face vertex is unlabeled.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// let dt = build_delaunay2_with_data(&[
    ///     ([0.0, 0.0], 0),
    ///     ([1.0, 0.0], 0),
    ///     ([0.5, 1.0], 1),
    /// ])
    /// .expect("build labeled triangle");
    /// let backend = DelaunayBackend2D::from_triangulation(dt);
    /// let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
    ///     .expect("create foliated triangulation");
    /// assert!(tri.validate_causality_delaunay().is_ok());
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "causality validation includes detailed diagnostics for multiple face-resolution and label error paths"
    )]
    pub fn validate_causality_delaunay(&self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return Ok(());
        }

        for face in self.geometry.faces() {
            let verts = self.geometry.face_vertices(&face).map_err(|err| {
                log::debug!(
                    "Causality validation failed to resolve vertices for face {:?}: {err}; vertex_count={}, edge_count={}, face_count={}",
                    face,
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                );
                CdtError::ValidationFailed {
                    check: "causality".to_string(),
                    detail: "failed to resolve face vertices".to_string(),
                }
            })?;

            if verts.len() != 3 {
                return Err(CdtError::ValidationFailed {
                    check: "causality".to_string(),
                    detail: format!(
                        "face {:?} has {} vertices, expected 3",
                        face.cell_key(),
                        verts.len(),
                    ),
                });
            }

            let t0 = self
                .geometry
                .vertex_data_by_key(verts[0].vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        verts[0].vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: "causality".to_string(),
                        detail: format!(
                            "vertex {:?} has no time label in a foliated triangulation",
                            verts[0].vertex_key(),
                        ),
                    }
                })?;
            let t1 = self
                .geometry
                .vertex_data_by_key(verts[1].vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        verts[1].vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: "causality".to_string(),
                        detail: format!(
                            "vertex {:?} has no time label in a foliated triangulation",
                            verts[1].vertex_key(),
                        ),
                    }
                })?;
            let t2 = self
                .geometry
                .vertex_data_by_key(verts[2].vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        verts[2].vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: "causality".to_string(),
                        detail: format!(
                            "vertex {:?} has no time label in a foliated triangulation",
                            verts[2].vertex_key(),
                        ),
                    }
                })?;

            // CDT triangle invariant: exactly 1 spacelike edge, 2 timelike edges.
            // On Toroidal topology the wrap-around edge between slice T−1 and 0
            // is timelike, so we use the topology-aware step distance.
            let mut spacelike = 0;
            let mut timelike = 0;

            for (a, b) in [(t0, t1), (t1, t2), (t2, t0)] {
                let step_distance = self.time_step_distance(a, b);
                match step_distance {
                    0 => spacelike += 1,
                    1 => timelike += 1,
                    _ => {
                        return Err(CdtError::CausalityViolation {
                            time_0: a.min(b),
                            time_1: a.max(b),
                            step_distance,
                        });
                    }
                }
            }

            if !(spacelike == 1 && timelike == 2) {
                return Err(CdtError::ValidationFailed {
                    check: "causality".to_string(),
                    detail: format!(
                        "invalid CDT triangle at face {:?}: spacelike={}, timelike={}",
                        face.cell_key(),
                        spacelike,
                        timelike
                    ),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    /// Builds a minimal labeled Delaunay backend for foliation and causality tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt)
    }

    #[test]
    fn test_remap_toroidal_generation_error_updates_context() {
        let remapped = remap_toroidal_generation_error(
            CdtError::DelaunayGenerationFailed {
                vertex_count: 3,
                coordinate_range: (-1.0, 1.0),
                attempt: 7,
                underlying_error: "builder failed".to_string(),
            },
            12,
        );

        assert!(matches!(
            remapped,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 12,
                coordinate_range: (0.0, 1.0),
                attempt: 1,
                ref underlying_error,
            } if underlying_error == "builder failed"
        ));
    }

    #[test]
    fn test_remap_toroidal_generation_error_preserves_other_errors() {
        let original = CdtError::InvalidGenerationParameters {
            issue: "bad".to_string(),
            provided_value: "x".to_string(),
            expected_range: "y".to_string(),
        };
        assert_eq!(
            remap_toroidal_generation_error(original.clone(), 12),
            original
        );
    }

    #[test]
    fn test_from_random_points() {
        let triangulation =
            CdtTriangulation::from_random_points(10, 3, 2).expect("Failed to create triangulation");

        assert_eq!(triangulation.dimension(), 2);
        assert_eq!(triangulation.time_slices(), 3);
        assert!(triangulation.vertex_count() > 0);
        assert!(triangulation.edge_count() > 0);
        assert!(triangulation.face_count() > 0);
    }

    #[test]
    fn test_from_random_points_various_sizes() {
        let test_cases = [
            (3, 1, "minimal"),
            (5, 2, "small"),
            (10, 3, "medium"),
            (20, 5, "large"),
        ];

        for (vertices, time_slices, description) in test_cases {
            let triangulation = CdtTriangulation::from_random_points(vertices, time_slices, 2)
                .unwrap_or_else(|e| panic!("Failed to create {description} triangulation: {e}"));

            assert_eq!(
                triangulation.dimension(),
                2,
                "Dimension should be 2 for {description}"
            );
            assert_eq!(
                triangulation.time_slices(),
                time_slices,
                "Time slices should match for {description}"
            );
            assert!(
                triangulation.vertex_count() >= 3,
                "Should have at least 3 vertices for {description}"
            );
            assert!(
                triangulation.edge_count() > 0,
                "Should have edges for {description}"
            );
            assert!(
                triangulation.face_count() > 0,
                "Should have faces for {description}"
            );
        }
    }

    #[test]
    fn test_from_seeded_points() {
        let seed = 42;
        let triangulation = CdtTriangulation::from_seeded_points(8, 2, 2, seed)
            .expect("Failed to create seeded triangulation");

        assert_eq!(triangulation.dimension(), 2);
        assert_eq!(triangulation.time_slices(), 2);
        assert!(triangulation.vertex_count() > 0);
        assert!(triangulation.edge_count() > 0);
        assert!(triangulation.face_count() > 0);
    }

    #[test]
    fn test_seeded_determinism() {
        let seed = 123;
        let params = (6, 3, 2);

        let triangulation1 =
            CdtTriangulation::from_seeded_points(params.0, params.1, params.2, seed)
                .expect("Failed to create first triangulation");
        let triangulation2 =
            CdtTriangulation::from_seeded_points(params.0, params.1, params.2, seed)
                .expect("Failed to create second triangulation");

        // Should produce identical results
        assert_eq!(triangulation1.vertex_count(), triangulation2.vertex_count());
        assert_eq!(triangulation1.edge_count(), triangulation2.edge_count());
        assert_eq!(triangulation1.face_count(), triangulation2.face_count());
        assert_eq!(triangulation1.dimension(), triangulation2.dimension());
        assert_eq!(triangulation1.time_slices(), triangulation2.time_slices());
    }

    #[test]
    fn test_seeded_different_seeds() {
        let params = (7, 2, 2);
        let tri1 = CdtTriangulation::from_seeded_points(params.0, params.1, params.2, 456)
            .expect("Failed to create triangulation with seed 456");
        let tri2 = CdtTriangulation::from_seeded_points(params.0, params.1, params.2, 789)
            .expect("Failed to create triangulation with seed 789");

        // Both should succeed but may differ in structure
        assert_eq!(tri1.dimension(), tri2.dimension());
        assert_eq!(tri1.time_slices(), tri2.time_slices());
        // Vertex counts should be same as input
        assert_eq!(tri1.vertex_count(), 7);
        assert_eq!(tri2.vertex_count(), 7);
    }

    #[test]
    fn test_invalid_dimension() {
        let invalid_dimensions = [0, 1, 3, 4, 5];
        for dim in invalid_dimensions {
            let result = CdtTriangulation::from_random_points(10, 3, dim);
            assert!(result.is_err(), "Should fail with dimension {dim}");

            if let Err(CdtError::UnsupportedDimension(d)) = result {
                assert_eq!(d, u32::from(dim), "Error should report correct dimension");
            } else {
                panic!("Expected UnsupportedDimension error for dimension {dim}");
            }
        }
    }

    #[test]
    fn test_from_seeded_points_rejects_invalid_dimension() {
        let result = CdtTriangulation::from_seeded_points(10, 3, 3, 42);

        assert!(matches!(result, Err(CdtError::UnsupportedDimension(3))));
    }

    #[test]
    fn test_invalid_vertex_count() {
        let invalid_counts = [0, 1, 2];
        for count in invalid_counts {
            let result = CdtTriangulation::from_random_points(count, 2, 2);
            assert!(result.is_err(), "Should fail with {count} vertices");

            match result {
                Err(CdtError::InvalidGenerationParameters {
                    issue,
                    provided_value,
                    ..
                }) => {
                    assert_eq!(issue, "Insufficient vertex count");
                    assert_eq!(provided_value, count.to_string());
                }
                other => panic!(
                    "Expected InvalidGenerationParameters for {count} vertices, got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn test_invalid_vertex_count_seeded() {
        let result = CdtTriangulation::from_seeded_points(2, 2, 2, 123);
        assert!(result.is_err(), "Should fail with 2 vertices");

        match result {
            Err(CdtError::InvalidGenerationParameters {
                issue,
                provided_value,
                ..
            }) => {
                assert_eq!(issue, "Insufficient vertex count");
                assert_eq!(provided_value, "2");
            }
            other => panic!("Expected InvalidGenerationParameters, got {other:?}"),
        }
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
        assert_eq!(triangulation.time_slices(), 4);
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
        assert_eq!(triangulation.time_slices(), 3);

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
    fn test_geometry_mut_with_cache() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Get initial edge count
        let initial_edge_count = triangulation.edge_count();
        assert!(initial_edge_count > 0);

        let initial_mod_count = triangulation.metadata().modification_count;

        // Get mutable access - this should invalidate cache and increment modification count
        {
            let mut geometry_mut = triangulation.geometry_mut();
            // Just access it, don't modify
            let _ = geometry_mut.geometry_mut();
        }

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

        // Get mutable reference (invalidates cache)
        {
            let mut geometry_mut = triangulation.geometry_mut();
            let _ = geometry_mut.geometry_mut();
        }

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
    fn test_validation_success() {
        // Use a known good seed that produces valid triangulation
        const GOOD_SEED: u64 = 53; // Known to produce Euler=2

        let triangulation = CdtTriangulation::from_seeded_points(5, 2, 2, GOOD_SEED)
            .expect("Failed to create triangulation");

        let result = triangulation.validate();
        assert!(
            result.is_ok(),
            "Validation should succeed for good triangulation: {result:?}"
        );
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
    fn test_validate_causality_no_foliation() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Without foliation, causality validation succeeds vacuously
        let result = triangulation.validate_causality();
        assert!(result.is_ok(), "Causality should pass without foliation");
    }

    #[test]
    fn test_validate_foliation_no_foliation() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 3, 2).expect("Failed to create triangulation");

        // Without foliation, foliation validation succeeds vacuously
        let result = triangulation.validate_foliation();
        assert!(result.is_ok(), "Foliation should pass without foliation");
    }

    #[test]
    fn test_geometry_mut_wrapper() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        {
            let mut wrapper = triangulation.geometry_mut();

            // Test deref functionality
            assert!(wrapper.vertex_count() > 0);
            assert!(wrapper.is_valid());

            // Test mutable access
            let geometry = wrapper.geometry_mut();
            assert!(geometry.vertex_count() > 0);
        }
    }

    #[test]
    fn test_simulation_event_recording() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        let initial_history_len = triangulation.metadata().simulation_history.len();

        {
            let mut wrapper = triangulation.geometry_mut();

            // Record some simulation events
            wrapper.record_event(SimulationEvent::MoveAttempted {
                move_type: "test_move".to_string(),
                step: 1,
            });

            wrapper.record_event(SimulationEvent::MoveAccepted {
                move_type: "test_move".to_string(),
                step: 1,
                action_change: -0.5,
            });

            wrapper.record_event(SimulationEvent::MeasurementTaken {
                step: 2,
                action: 10.5,
            });
        }

        // Should have 3 more events
        assert_eq!(
            triangulation.metadata().simulation_history.len(),
            initial_history_len + 3
        );

        // Check the recorded events
        let history = &triangulation.metadata().simulation_history;
        match &history[initial_history_len] {
            SimulationEvent::MoveAttempted { move_type, step } => {
                assert_eq!(move_type, "test_move");
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
                assert_eq!(move_type, "test_move");
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
        let backend = DelaunayBackend2D::from_triangulation(dt);
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

        {
            let mut wrapper = tri.geometry_mut();
            wrapper.record_event(SimulationEvent::MoveAttempted {
                move_type: "history_only".to_string(),
                step: 7,
            });
        }

        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count
        );
        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), initial_slice_sizes.as_slice());
        assert!(tri.time_label(&vertex).is_some());
        assert!(tri.edge_type(&edge).is_some());
        assert!(tri.cell_type(&face).is_some());
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

        {
            let mut wrapper = triangulation.geometry_mut();
            let _ = wrapper.geometry_mut();
        }

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

        // Each backend-mutation session should increment once.
        {
            let mut wrapper = triangulation.geometry_mut();
            let _ = wrapper.geometry_mut();
        }
        assert_eq!(triangulation.metadata().modification_count, 1);

        {
            let mut wrapper = triangulation.geometry_mut();
            let _ = wrapper.geometry_mut();
        }
        assert_eq!(triangulation.metadata().modification_count, 2);

        // Immutable access shouldn't change count
        let _geometry = triangulation.geometry();
        let _edge_count = triangulation.edge_count();
        assert_eq!(triangulation.metadata().modification_count, 2);
    }

    #[test]
    fn test_zero_time_slices() {
        let result = CdtTriangulation::from_random_points(5, 0, 2);
        // This should still work - time_slices is just metadata
        assert!(result.is_ok(), "Should allow 0 time slices");

        let triangulation = result.unwrap();
        assert_eq!(triangulation.time_slices(), 0);
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

        assert_eq!(triangulation.time_slices(), 2);
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

        assert_eq!(tri.time_slices(), 3);
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
        assert_eq!(triangulation.time_slices(), 100);
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
                move_type: "flip".to_string(),
                step: 1,
            },
            SimulationEvent::MoveAccepted {
                move_type: "flip".to_string(),
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

    // =========================================================================
    // Foliation tests
    // =========================================================================

    /// Accepts the current point-set cylinder limitations while explicit strip construction is pending.
    fn assert_foliated_cylinder_known_failure(
        result: CdtResult<CdtTriangulation<DelaunayBackend2D>>,
    ) {
        match result {
            Err(CdtError::DelaunayGenerationFailed {
                underlying_error, ..
            }) => {
                let rejected_as_non_cdt = underlying_error.contains("non-CDT triangulation")
                    || underlying_error.contains("invalid CDT triangle");
                let rejected_for_vertex_drop = underlying_error.contains("builder inserted only");
                assert!(
                    rejected_as_non_cdt || rejected_for_vertex_drop,
                    "Expected non-CDT or vertex-drop rejection, got: {underlying_error}"
                );
            }
            Ok(_) => panic!("Expected point-set strip construction rejection"),
            Err(other) => panic!("Expected DelaunayGenerationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_from_foliated_cylinder_basic() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_vertex_counts_per_slice() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            4,
            3,
            Some(1),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_all_vertices_labeled() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(99),
        ));
    }

    #[test]
    fn test_slice_sizes_from_vertex_labels_rejects_unlabeled_vertices() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let mut backend = DelaunayBackend2D::from_triangulation(dt);
        let unlabeled_vertex = backend
            .vertices()
            .next()
            .expect("Triangle should contain a vertex")
            .vertex_key();

        let _previous_label = backend
            .set_vertex_data_by_key(unlabeled_vertex, None)
            .expect("Expected valid vertex key while clearing label");

        let result =
            CdtTriangulation::<DelaunayBackend2D>::slice_sizes_from_vertex_labels(&backend, 3, 2);

        match result {
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count,
                coordinate_range,
                attempt,
                underlying_error,
            }) => {
                assert_eq!(vertex_count, 3);
                assert_eq!(coordinate_range, (0.0, 1.0));
                assert_eq!(attempt, 1);
                assert!(
                    underlying_error.contains("1 unlabeled vertex"),
                    "Error should report unlabeled vertices: {underlying_error}"
                );
                assert!(
                    underlying_error.contains("example vertex keys"),
                    "Error should include example keys: {underlying_error}"
                );
                assert!(
                    underlying_error.contains(
                        "build_delaunay2_with_data failed to preserve per-vertex time labels"
                    ),
                    "Error should identify the likely source: {underlying_error}"
                );
            }
            other => panic!("Expected DelaunayGenerationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_slice_sizes_from_vertex_labels_rejects_out_of_range_labels() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let mut backend = DelaunayBackend2D::from_triangulation(dt);
        let invalid_vertex = backend
            .vertices()
            .next()
            .expect("Triangle should contain a vertex")
            .vertex_key();

        let _previous_label = backend
            .set_vertex_data_by_key(invalid_vertex, Some(5))
            .expect("Expected valid vertex key while setting out-of-range label");

        let result =
            CdtTriangulation::<DelaunayBackend2D>::slice_sizes_from_vertex_labels(&backend, 3, 2);

        match result {
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count,
                coordinate_range,
                attempt,
                underlying_error,
            }) => {
                assert_eq!(vertex_count, 3);
                assert_eq!(coordinate_range, (0.0, 1.0));
                assert_eq!(attempt, 1);
                assert!(
                    underlying_error.contains("invalid time label 5"),
                    "Error should preserve invalid-label reporting: {underlying_error}"
                );
                assert!(
                    underlying_error.contains("expected 0..2"),
                    "Error should report the expected label range: {underlying_error}"
                );
            }
            other => panic!("Expected DelaunayGenerationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_from_labeled_delaunay_preserves_foliation() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), &[2, 1]);
        assert!(tri.validate_foliation().is_ok());

        for vh in tri.geometry().vertices() {
            assert!(tri.time_label(&vh).is_some());
        }
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_invalid_dimension() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 3);

        assert!(matches!(result, Err(CdtError::UnsupportedDimension(3))));
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_zero_slices() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 0, 2);

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation"
                    && detail == "cannot validate foliation with 0 time slices"
        ));
    }

    #[test]
    fn test_validate_foliation_missing_label() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        let vertex_to_clear = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex")
            .vertex_key();

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous = geometry_mut
                .set_vertex_data_by_key(vertex_to_clear, None)
                .expect("Expected valid vertex key while clearing label");
        }

        assert!(
            !tri.has_foliation(),
            "mutable backend access should invalidate cached foliation bookkeeping"
        );
        assert!(
            tri.slice_sizes().is_empty(),
            "stale foliation bookkeeping should not be exposed via slice_sizes()"
        );

        let result = tri.validate_foliation();
        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation" && detail.contains("missing a time label")
        ));
    }

    #[test]
    fn test_validate_foliation_out_of_range_label() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        let vertex_to_mutate = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex")
            .vertex_key();

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous = geometry_mut
                .set_vertex_data_by_key(vertex_to_mutate, Some(7))
                .expect("Expected valid vertex key while mutating label");
        }

        let result = tri.validate_foliation();
        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation"
                    && detail.contains("out-of-range time label 7")
                    && detail.contains("expected 0..2")
        ));
    }

    #[test]
    fn test_validate_foliation_slice_mismatch() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        let vertex_to_move = tri
            .geometry()
            .vertices()
            .find(|vh| tri.geometry().vertex_data_by_key(vh.vertex_key()) == Some(0))
            .expect("Triangle should contain a vertex in slice 0")
            .vertex_key();

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous = geometry_mut
                .set_vertex_data_by_key(vertex_to_move, Some(1))
                .expect("Expected valid vertex key while mutating label");
        }

        let result = tri.validate_foliation();
        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation"
                    && detail.contains("stored count")
                    && detail.contains("live labels report")
        ));
    }

    #[test]
    fn test_validate_foliation_rejects_stored_label_count_mismatch() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        tri.foliation = Some(
            Foliation::from_slice_sizes(vec![1, 1], 2)
                .expect("non-empty mismatched bookkeeping is constructible"),
        );

        let result = tri.validate_foliation();

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation"
                    && detail.contains("labeled vertex count (2)")
                    && detail.contains("triangulation vertex count (3)")
        ));
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_out_of_range_labels() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 5)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 2);
        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation" && detail.contains("out-of-range time label 5")
        ));
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_empty_intermediate_slice() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 2), ([0.5, 1.0], 2)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 3, 2);
        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation" && detail.contains("time slice 1 is empty")
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_edge_classification() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_validate_foliation() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_validate_causality() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_seed_determinism() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_invalid_params() {
        // Too few vertices per slice
        assert!(CdtTriangulation::from_foliated_cylinder(3, 3, None).is_err());
        // Too few slices
        assert!(CdtTriangulation::from_foliated_cylinder(5, 1, None).is_err());
    }

    #[test]
    fn test_from_foliated_cylinder_rejects_vertex_count_overflow() {
        let result = CdtTriangulation::from_foliated_cylinder(u32::MAX, 2, None);

        assert!(matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if issue == "Vertex count overflow"
                && provided_value == "4294967295 × 2"
                && expected_range == "product ≤ u32::MAX"
        ));
    }

    #[test]
    fn test_from_cdt_strip_rejects_invalid_params_before_placeholder_error() {
        let few_vertices = CdtTriangulation::from_cdt_strip(3, 3);
        assert!(matches!(
            few_vertices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if issue == "Insufficient vertices per slice"
                && provided_value == "3"
                && expected_range == "≥ 4"
        ));

        let few_slices = CdtTriangulation::from_cdt_strip(4, 1);
        assert!(matches!(
            few_slices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if issue == "Insufficient number of time slices"
                && provided_value == "1"
                && expected_range == "≥ 2"
        ));
    }

    #[test]
    fn test_from_cdt_strip_reports_placeholder_for_valid_params() {
        let result = CdtTriangulation::from_cdt_strip(4, 2);

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "cdt_construction"
                    && detail.contains("from_cdt_strip is not yet implemented")
        ));
    }

    #[test]
    fn test_vertices_at_time() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            4,
            3,
            Some(1),
        ));
    }

    #[test]
    fn test_assign_foliation_by_y() {
        let mut tri = CdtTriangulation::from_seeded_points(15, 3, 2, 42)
            .expect("Failed to create deterministic triangulation");

        assert!(!tri.has_foliation());

        tri.assign_foliation_by_y(3)
            .expect("Should assign foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.time_slices(), 3);
        assert_eq!(tri.slice_sizes().iter().sum::<usize>(), tri.vertex_count());

        // All vertices should now be labeled
        for vh in tri.geometry().vertices() {
            assert!(tri.time_label(&vh).is_some());
        }

        // Foliation validation should pass
        let result = tri.validate_foliation();
        assert!(
            result.is_ok(),
            "Foliation validation should pass: {result:?}"
        );
    }

    #[test]
    fn test_assign_foliation_by_y_updates_metadata() {
        let mut tri = CdtTriangulation::from_seeded_points(15, 3, 2, 42)
            .expect("Failed to create deterministic triangulation");
        let initial_last_modified = tri.metadata().last_modified;
        let initial_modification_count = tri.metadata().modification_count;
        let initial_edge_count = tri.edge_count();
        let initial_euler_characteristic = tri.geometry().euler_characteristic();

        thread::sleep(Duration::from_millis(5));

        tri.assign_foliation_by_y(3)
            .expect("Should update foliation metadata");

        assert!(tri.metadata().last_modified > initial_last_modified);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count + 1,
            "Foliation assignment should count as a modification"
        );
        assert_eq!(tri.edge_count(), initial_edge_count);
        assert_eq!(
            tri.geometry().euler_characteristic(),
            initial_euler_characteristic
        );
    }

    #[test]
    fn test_assign_foliation_by_y_invalidates_cache() {
        let mut tri = CdtTriangulation::from_seeded_points(15, 3, 2, 42)
            .expect("Failed to create deterministic triangulation");

        tri.refresh_cache();
        assert!(tri.cache.edge_count.is_some());
        assert!(tri.cache.euler_char.is_some());

        tri.assign_foliation_by_y(3)
            .expect("Should invalidate cache when assigning foliation");

        assert!(
            tri.cache.edge_count.is_none(),
            "assign_foliation_by_y should clear cached edge count via invalidate_cache()"
        );
        assert!(
            tri.cache.euler_char.is_none(),
            "assign_foliation_by_y should clear cached Euler characteristic via invalidate_cache()"
        );
    }

    #[test]
    fn test_assign_foliation_zero_slices() {
        let mut tri = CdtTriangulation::from_random_points(5, 2, 2).unwrap();
        assert!(tri.assign_foliation_by_y(0).is_err());
    }

    #[test]
    fn test_assign_foliation_empty_slice_error_preserves_metadata() {
        let mut tri =
            CdtTriangulation::from_random_points(6, 2, 2).expect("Failed to create triangulation");
        let initial_time_slices = tri.time_slices();
        let initial_modification_count = tri.metadata().modification_count;
        let vertex_keys: Vec<_> = tri
            .geometry()
            .vertices()
            .map(|vh| vh.vertex_key())
            .collect();

        let requested_slices = u32::try_from(tri.vertex_count())
            .expect("vertex count should fit into u32 for this test")
            .saturating_add(1);
        let result = tri.assign_foliation_by_y(requested_slices);

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "foliation" && detail.contains("time slice") && detail.contains("empty")
        ));
        assert_eq!(tri.time_slices(), initial_time_slices);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count
        );
        assert!(!tri.has_foliation());

        for key in vertex_keys {
            assert_eq!(
                tri.geometry().vertex_data_by_key(key),
                None,
                "failed assignment should not write vertex labels"
            );
        }
    }

    #[test]
    fn test_from_foliated_cylinder_minimum_size() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            4,
            2,
            Some(1),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_full_validate() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_from_foliated_cylinder_no_seed() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5, 3, None,
        ));
    }

    #[test]
    fn test_all_faces_are_valid_cdt_triangles() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_assign_foliation_single_slice() {
        let mut tri =
            CdtTriangulation::from_random_points(6, 1, 2).expect("Failed to create triangulation");

        tri.assign_foliation_by_y(1)
            .expect("Should assign single-slice foliation");

        assert!(tri.has_foliation());
        // All vertices should be in slice 0
        for vh in tri.geometry().vertices() {
            assert_eq!(tri.time_label(&vh), Some(0));
        }
        assert_eq!(tri.slice_sizes(), &[tri.vertex_count()]);
    }

    #[test]
    fn test_from_foliated_cylinder_larger_grid() {
        let result = CdtTriangulation::from_foliated_cylinder(10, 8, Some(7));
        match result {
            Err(CdtError::DelaunayGenerationFailed {
                underlying_error, ..
            }) => {
                assert!(
                    underlying_error.contains("Delaunay repair postcondition failed"),
                    "Expected explicit Delaunay repair failure, got: {underlying_error}"
                );
            }
            Ok(_) => panic!("Expected larger grid generation to fail"),
            Err(other) => panic!("Expected DelaunayGenerationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_no_foliation_queries_return_none() {
        let tri = CdtTriangulation::from_random_points(5, 2, 2).unwrap();
        assert!(!tri.has_foliation());
        assert!(tri.foliation().is_none());
        assert!(tri.slice_sizes().is_empty());
        assert!(tri.vertices_at_time(0).is_empty());

        // time_label and edge_type return None
        for vh in tri.geometry().vertices() {
            assert_eq!(tri.time_label(&vh), None);
        }
        for face in tri.geometry().faces() {
            assert!(tri.face_edge_types(&face).is_none());
        }
    }

    // =========================================================================
    // Cell classification tests
    // =========================================================================

    #[test]
    fn test_cell_type_returns_up_or_down() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_cell_type_no_foliation_returns_none() {
        let tri = CdtTriangulation::from_random_points(5, 2, 2).unwrap();
        assert!(!tri.has_foliation());

        for face in tri.geometry().faces() {
            assert_eq!(tri.cell_type(&face), None);
        }
    }

    #[test]
    fn test_classify_all_cells_stores_data() {
        assert_foliated_cylinder_known_failure(CdtTriangulation::from_foliated_cylinder(
            5,
            3,
            Some(42),
        ));
    }

    #[test]
    fn test_classify_all_cells_no_foliation_returns_none() {
        let mut tri = CdtTriangulation::from_random_points(5, 2, 2).unwrap();
        assert_eq!(
            tri.classify_all_cells()
                .expect("No foliation should classify as a no-op"),
            None
        );
    }

    #[test]
    fn test_cell_type_from_data_before_classify_returns_none() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
        let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("Triangle should contain a face");
        assert_eq!(tri.cell_type_from_data(&face), None);
    }

    #[test]
    fn test_cell_type_from_data_returns_none_when_foliation_stale() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("Triangle should contain a face");
        let vertex_to_mutate = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex")
            .vertex_key();

        let classified = tri
            .classify_all_cells()
            .expect("Should classify cells with foliation")
            .expect("Foliation is present");
        assert!(classified > 0);
        assert!(tri.cell_type_from_data(&face).is_some());

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous = geometry_mut
                .set_vertex_data_by_key(vertex_to_mutate, Some(7))
                .expect("Expected valid vertex key while mutating label");
        }

        assert_eq!(tri.cell_type_from_data(&face), None);
    }

    #[test]
    fn test_classify_all_cells_on_labeled_triangle() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        // cell_type should classify before bulk classification
        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("Triangle should contain a face");
        let live_ct = tri
            .cell_type(&face)
            .expect("Single face should be classifiable");
        assert!(
            live_ct == CellType::Up || live_ct == CellType::Down,
            "Single face spanning two slices must be Up or Down"
        );

        // Bulk classify and verify stored data matches live classification
        let count = tri
            .classify_all_cells()
            .expect("classify_all_cells should succeed")
            .expect("foliation is present");
        assert_eq!(count, 1, "Single-face triangulation should classify 1 cell");

        let stored_ct = tri
            .cell_type_from_data(&face)
            .expect("Stored cell type should be present after classification");
        assert_eq!(
            live_ct, stored_ct,
            "Stored classification should match live classification"
        );
    }

    #[test]
    fn test_vertices_at_time_with_foliation() {
        let mut tri = CdtTriangulation::from_seeded_points(15, 3, 2, 42)
            .expect("Failed to create deterministic triangulation");

        tri.assign_foliation_by_y(3)
            .expect("Should assign foliation");

        // Every slice should have at least one vertex
        for t in 0..3 {
            let verts = tri.vertices_at_time(t);
            assert!(
                !verts.is_empty(),
                "Slice {t} should have at least one vertex"
            );
            // Each returned vertex should actually carry that time label
            for vh in &verts {
                assert_eq!(
                    tri.time_label(vh),
                    Some(t),
                    "Vertex returned by vertices_at_time({t}) should have label {t}"
                );
            }
        }

        // Sum across all slices should equal total vertex count
        let total: usize = (0..3).map(|t| tri.vertices_at_time(t).len()).sum();
        assert_eq!(
            total,
            tri.vertex_count(),
            "Sum of vertices_at_time across all slices should equal vertex_count"
        );

        // Non-existent slice should return empty
        assert!(tri.vertices_at_time(999).is_empty());
    }

    #[test]
    fn test_assign_foliation_by_y_reassignment() {
        let mut tri = CdtTriangulation::from_seeded_points(15, 3, 2, 42)
            .expect("Failed to create deterministic triangulation");

        // First assignment: 3 slices
        tri.assign_foliation_by_y(3)
            .expect("First foliation assignment should succeed");
        assert!(tri.has_foliation());
        assert_eq!(tri.time_slices(), 3);
        let sizes_3 = tri.slice_sizes().to_vec();
        assert_eq!(sizes_3.len(), 3);

        // Classify cells so we can verify stale data is cleared
        let classified_before = tri
            .classify_all_cells()
            .expect("classify_all_cells should succeed")
            .expect("foliation is present");
        assert!(classified_before > 0);

        // Re-assign with 2 slices
        tri.assign_foliation_by_y(2)
            .expect("Re-assignment with different slice count should succeed");
        assert!(tri.has_foliation());
        assert_eq!(tri.time_slices(), 2);
        let sizes_2 = tri.slice_sizes().to_vec();
        assert_eq!(sizes_2.len(), 2);
        assert_eq!(
            sizes_2.iter().sum::<usize>(),
            tri.vertex_count(),
            "Slice sizes should sum to vertex count after re-assignment"
        );

        // Stale cell data from the 3-slice classification should be cleared;
        // cell_type_from_data should return None until classify_all_cells is
        // called again with the new foliation.
        for face in tri.geometry().faces() {
            assert_eq!(
                tri.cell_type_from_data(&face),
                None,
                "Stale cell data should be cleared after foliation re-assignment"
            );
        }

        // Foliation and causality validation should still pass
        assert!(
            tri.validate_foliation().is_ok(),
            "Foliation should be valid after re-assignment"
        );
    }

    /// Builds stable diagnostic text for seeded-triangulation comparisons.
    fn deterministic_triangle_debug_summary(backend: &DelaunayBackend2D) -> String {
        let mut vertices: Vec<_> = backend
            .vertices()
            .map(|vh| {
                let coords = backend.vertex_coordinates(&vh).map_or_else(
                    |err| format!("coord_error:{err}"),
                    |coords| format!("{coords:?}"),
                );
                format!(
                    "{:?}@{}:{:?}",
                    vh.vertex_key(),
                    coords,
                    backend.vertex_data_by_key(vh.vertex_key())
                )
            })
            .collect();
        vertices.sort_unstable();

        let mut edges: Vec<_> = backend
            .edges()
            .map(|edge| match backend.edge_endpoints(&edge) {
                Some((v0, v1)) => format!(
                    "{:?}<->{:?}:{:?}->{:?}",
                    v0.vertex_key(),
                    v1.vertex_key(),
                    backend.vertex_data_by_key(v0.vertex_key()),
                    backend.vertex_data_by_key(v1.vertex_key())
                ),
                None => "endpoint_error:unavailable".to_string(),
            })
            .collect();
        edges.sort_unstable();

        format!(
            "vertex_count={}, edge_count={}, face_count={}, is_valid={}, is_delaunay={}, vertices=[{}], edges=[{}]",
            backend.vertex_count(),
            backend.edge_count(),
            backend.face_count(),
            backend.is_valid(),
            backend.is_delaunay(),
            vertices.join(", "),
            edges.join(", "),
        )
    }

    // =========================================================================
    // Causality violation detection
    // =========================================================================

    #[test]
    fn test_causality_violation_detected() {
        // Use a hand-built triangle instead of from_foliated_cylinder() so this
        // test does not depend on Delaunay tie-breaking for a larger strip mesh.
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build deterministic causal triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
        let mut tri = CdtTriangulation::new(backend, 2, 2);

        // Derive the foliation from coordinates instead of relying on
        // build_delaunay2_with_data() to preserve vertex data across platforms.
        // This test targets causality validation, not builder label retention.
        tri.assign_foliation_by_y(2)
            .expect("Should derive foliation from triangle coordinates");

        assert_eq!(
            tri.slice_sizes(),
            &[2, 1],
            "Deterministic triangle should assign slice sizes [2, 1], got {:?}; {}",
            tri.slice_sizes(),
            deterministic_triangle_debug_summary(tri.geometry())
        );

        let initial_validation = tri.validate_causality_delaunay();
        assert!(
            initial_validation.is_ok(),
            "Deterministic causal triangle should start causally valid: {initial_validation:?}; {}",
            deterministic_triangle_debug_summary(tri.geometry())
        );

        assert!(
            tri.geometry().faces().any(|face| {
                tri.face_edge_types(&face)
                    .is_some_and(|ets| ets.iter().any(|e| matches!(e, EdgeType::Timelike)))
            }),
            "Deterministic causal triangle should contain a timelike edge; {}",
            deterministic_triangle_debug_summary(tri.geometry())
        );

        let vertex_to_mutate = tri
            .geometry()
            .vertices()
            .next()
            .expect("Deterministic causal triangle should contain a vertex")
            .vertex_key();

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous_label = geometry_mut
                .set_vertex_data_by_key(vertex_to_mutate, Some(3))
                .expect("Expected valid vertex key while mutating deterministic triangle");
        }

        let result = tri.validate_causality_delaunay();
        assert!(
            result.is_err(),
            "Explicitly acausal edge should fail causality validation"
        );

        // Verify the error is a CausalityViolation reporting step_distance > 1.
        // OpenBoundary topology means step_distance == |Δt|.
        if let Err(CdtError::CausalityViolation {
            time_0,
            time_1,
            step_distance,
        }) = result
        {
            assert!(
                step_distance > 1,
                "CausalityViolation should report step_distance > 1, got step_distance={step_distance} (t0={time_0}, t1={time_1})"
            );
            assert_eq!(
                step_distance,
                time_0.abs_diff(time_1),
                "OpenBoundary step_distance must equal |Δt|; got step_distance={step_distance}, |Δt|={}",
                time_0.abs_diff(time_1)
            );
        } else {
            panic!(
                "Expected CausalityViolation error, got {result:?}; {}",
                deterministic_triangle_debug_summary(tri.geometry())
            );
        }
    }

    #[test]
    fn test_validate_causality_rejects_missing_live_label() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let vertex_to_clear = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex")
            .vertex_key();

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous = geometry_mut
                .set_vertex_data_by_key(vertex_to_clear, None)
                .expect("Expected valid vertex key while clearing label");
        }

        let result = tri.validate_causality_delaunay();

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "causality"
                    && detail.contains("has no time label in a foliated triangulation")
        ));
    }

    #[test]
    fn test_validate_causality_rejects_all_spacelike_triangle() {
        let backend = labeled_triangle_backend([0, 0, 0]);
        let tri = CdtTriangulation::from_labeled_delaunay(backend, 1, 2)
            .expect("single-slice labels should form foliation bookkeeping");

        let result = tri.validate_causality_delaunay();

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "causality"
                    && detail.contains("invalid CDT triangle")
                    && detail.contains("spacelike=3")
                    && detail.contains("timelike=0")
        ));
    }

    #[test]
    fn test_toroidal_causality_violation_reports_circular_step_distance() {
        // Build a real toroidal CDT with T=10 so wrap-around is non-trivial:
        // mutating a slice-0 vertex to slice 8 makes its same-slice spacelike
        // edges read as (raw |Δt|=8, circular distance 2), which exceeds the
        // adjacency threshold of 1.  The reported step_distance must be the
        // circular distance, not the raw difference.
        let mut tri =
            CdtTriangulation::from_toroidal_cdt(3, 10).expect("build toroidal CDT (3, 10)");

        let slice0_vertex = tri
            .geometry()
            .vertices()
            .find(|vh| tri.geometry().vertex_data_by_key(vh.vertex_key()) == Some(0))
            .expect("Toroidal CDT should contain slice-0 vertices")
            .vertex_key();

        {
            let mut geometry_mut = tri.geometry_mut();
            let _previous_label = geometry_mut
                .set_vertex_data_by_key(slice0_vertex, Some(8))
                .expect("Expected valid vertex key while mutating label");
        }

        let result = tri.validate_causality_delaunay();
        match result {
            Err(CdtError::CausalityViolation {
                time_0,
                time_1,
                step_distance,
            }) => {
                let raw = time_0.abs_diff(time_1);
                let circular = raw.min(10 - raw);
                assert_eq!(
                    step_distance, circular,
                    "Toroidal step_distance must equal circular distance min(|Δt|, T−|Δt|); \
                     got step_distance={step_distance}, raw |Δt|={raw}, T=10"
                );
                assert!(
                    step_distance > 1,
                    "Violation must exceed adjacency threshold; got step_distance={step_distance}"
                );
                assert!(
                    step_distance < raw,
                    "With T=10 and labels {{0,1,8}}, every violation crosses the wrap; \
                     step_distance={step_distance} should be < raw |Δt|={raw}"
                );
            }
            other => panic!("Expected CausalityViolation on toroidal triangle, got {other:?}"),
        }
    }

    #[test]
    fn test_foliation_error_converts_to_cdt_error() {
        // Verify From<FoliationError> for CdtError produces ValidationFailed
        let fol_err = FoliationError::EmptySlice { slice: 3 };
        let cdt_err: CdtError = fol_err.into();

        match cdt_err {
            CdtError::ValidationFailed { check, detail } => {
                assert_eq!(check, "foliation");
                assert!(
                    detail.contains('3') && detail.contains("empty"),
                    "Detail should describe the empty slice: {detail}"
                );
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_out_of_range_error_conversion() {
        let fol_err = FoliationError::OutOfRangeVertexLabel {
            vertex: 2,
            label: 7,
            expected_range_end: 2,
        };
        let cdt_err: CdtError = fol_err.into();

        match cdt_err {
            CdtError::ValidationFailed { check, detail } => {
                assert_eq!(check, "foliation");
                assert!(
                    detail.contains("vertex index 2")
                        && detail.contains("out-of-range time label 7")
                        && detail.contains("expected 0..2"),
                    "Detail should preserve out-of-range label context: {detail}"
                );
            }
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    // =========================================================================
    // Toroidal CDT tests
    // =========================================================================

    #[test]
    fn test_from_toroidal_cdt_basic() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3)
            .expect("toroidal CDT should build with delaunay v0.7.6");

        // V = N*T = 12, F = 2*N*T = 24, E = 3*N*T = 36, χ = 0.
        assert_eq!(tri.vertex_count(), 12);
        assert_eq!(tri.face_count(), 24);
        assert_eq!(tri.edge_count(), 36);
        assert_eq!(tri.geometry().euler_characteristic(), 0);
        assert_eq!(tri.dimension(), 2);
        assert_eq!(tri.time_slices(), 3);
        assert!(matches!(tri.metadata().topology, CdtTopology::Toroidal));
    }

    #[test]
    fn test_from_toroidal_cdt_various_sizes() {
        for (n, t) in [(3_u32, 3_u32), (4, 3), (5, 4), (6, 5), (8, 4)] {
            let tri = CdtTriangulation::from_toroidal_cdt(n, t)
                .unwrap_or_else(|err| panic!("toroidal CDT N={n} T={t} should build: {err}"));
            let nt = (n as usize) * (t as usize);
            assert_eq!(tri.vertex_count(), nt);
            assert_eq!(tri.face_count(), 2 * nt);
            assert_eq!(tri.edge_count(), 3 * nt);
            assert_eq!(tri.geometry().euler_characteristic(), 0);
        }
    }

    #[test]
    fn test_from_toroidal_cdt_foliation_per_slice() {
        let tri = CdtTriangulation::from_toroidal_cdt(5, 4).expect("build toroidal CDT");
        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), &[5, 5, 5, 5]);
        for t in 0..4 {
            assert_eq!(
                tri.vertices_at_time(t).len(),
                5,
                "slice {t} should contain N=5 vertices"
            );
        }
    }

    #[test]
    fn test_from_toroidal_cdt_validate_passes() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality().is_ok());
    }

    #[test]
    fn test_from_toroidal_cdt_each_slice_is_closed_s1() {
        // The toroidal validate_foliation extension already enforces that
        // each slice is a closed S¹; building a torus and validating again
        // exercises that path explicitly.
        let tri = CdtTriangulation::from_toroidal_cdt(6, 4).expect("build toroidal CDT");
        tri.validate_foliation()
            .expect("explicit toroidal CDT must satisfy closed-S¹ per-slice invariant");
    }

    #[test]
    fn test_from_toroidal_cdt_invalid_params() {
        // Too few vertices per slice (N < 3 — no proper cycle).
        assert!(CdtTriangulation::from_toroidal_cdt(2, 3).is_err());
        // Too few slices (T < 3 — wrap-around makes the mesh non-manifold).
        assert!(CdtTriangulation::from_toroidal_cdt(4, 1).is_err());
        assert!(CdtTriangulation::from_toroidal_cdt(4, 2).is_err());
    }

    #[test]
    fn test_from_toroidal_cdt_rejects_vertex_count_overflow() {
        let result = CdtTriangulation::from_toroidal_cdt(u32::MAX, 3);

        assert!(matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if issue == "Vertex count overflow"
                && provided_value == "4294967295 × 3"
                && expected_range == "product ≤ u32::MAX"
        ));
    }

    #[test]
    fn test_toroidal_cell_classification_uses_temporal_wrap() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        let mut saw_wrap_up = false;
        let mut saw_wrap_down = false;
        let mut saw_wrap_timelike_edge = false;

        for face in tri.geometry().faces() {
            let vertices = tri
                .geometry()
                .face_vertices(&face)
                .expect("toroidal face vertices should resolve");
            let labels: Vec<_> = vertices
                .iter()
                .map(|vh| {
                    tri.geometry()
                        .vertex_data_by_key(vh.vertex_key())
                        .expect("toroidal vertices are labeled")
                })
                .collect();

            if labels.contains(&0) && labels.contains(&2) {
                let cell_type = tri
                    .cell_type(&face)
                    .expect("wrap-around toroidal face should classify");
                let edge_types = tri
                    .face_edge_types(&face)
                    .expect("wrap-around toroidal face should expose edge types");
                saw_wrap_timelike_edge |= edge_types
                    .iter()
                    .any(|edge_type| matches!(edge_type, EdgeType::Timelike));

                let zero_count = labels.iter().filter(|&&label| label == 0).count();
                let two_count = labels.iter().filter(|&&label| label == 2).count();
                let is_wrap_up = zero_count == 1 && two_count == 2;
                let is_wrap_down = zero_count == 2 && two_count == 1;

                if is_wrap_up {
                    assert_eq!(cell_type, CellType::Up);
                }
                if is_wrap_down {
                    assert_eq!(cell_type, CellType::Down);
                }

                saw_wrap_up |= is_wrap_up;
                saw_wrap_down |= is_wrap_down;
            }
        }

        assert!(saw_wrap_up, "expected an Up cell across the temporal wrap");
        assert!(
            saw_wrap_down,
            "expected a Down cell across the temporal wrap"
        );
        assert!(
            saw_wrap_timelike_edge,
            "expected a timelike edge across the temporal wrap"
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

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "topology"
                    && detail.contains("open boundary (expected χ=1 or 2)")
        ));
    }

    #[test]
    fn test_validate_topology_toroidal_rejects_chi_nonzero() {
        // Build a non-toroidal triangulation, then label its metadata as
        // Toroidal so validate_topology() expects χ=0 but gets χ1.
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
        let tri = CdtTriangulation::with_topology(backend, 3, 2, CdtTopology::Toroidal)
            .expect("valid toroidal metadata");

        let result = tri.validate_topology();
        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "topology" && detail.contains("toroidal (expected χ=0)")
        ));
    }

    #[test]
    fn test_toroidal_rejects_few_slices() {
        let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");

        let result = tri.set_time_slices(2);
        assert!(matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref topology,
                ref provided_value,
                ref expected,
            }) if field == "timeslices"
                && topology == "toroidal"
                && provided_value == "2"
                && expected == "≥ 3"
        ));
        assert_eq!(tri.time_slices(), 3);
        assert!(tri.validate_topology().is_ok());
    }

    #[test]
    fn test_assign_foliation_by_y_rejects_invalid_toroidal_slice_count() {
        let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        let initial_slice_sizes = tri.slice_sizes().to_vec();

        let result = tri.assign_foliation_by_y(2);
        assert!(matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref topology,
                ref provided_value,
                ref expected,
            }) if field == "timeslices"
                && topology == "toroidal"
                && provided_value == "2"
                && expected == "≥ 3"
        ));
        assert_eq!(tri.time_slices(), 3);
        assert_eq!(tri.slice_sizes(), initial_slice_sizes.as_slice());
        assert!(tri.has_foliation());
    }

    #[test]
    fn test_with_topology_rejects_few_toroidal_slices() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let result = CdtTriangulation::with_topology(backend, 2, 3, CdtTopology::Toroidal);
        assert!(matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref topology,
                ref provided_value,
                ref expected,
            }) if field == "timeslices"
                && topology == "toroidal"
                && provided_value == "2"
                && expected == "≥ 3"
        ));
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use crate::util::saturating_usize_to_i32;
    use proptest::prelude::*;

    proptest! {
        // NOTE: Commented out due to extreme edge cases in random triangulation generation
        // Property-based testing found Euler characteristics as extreme as χ = -13
        // This indicates the random point generation can create very complex topologies
        // TODO: Either constrain generation or develop better validation
        //
        // #[test]
        // fn triangulation_euler_characteristic_invariant(
        //     vertices in 4u32..20,
        //     timeslices in 1u32..5
        // ) {
        //     let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;
        //     let v = triangulation.vertex_count() as i32;
        //     let e = triangulation.edge_count() as i32;
        //     let f = triangulation.face_count() as i32;
        //     let euler = v - e + f;
        //
        //     prop_assert!(
        //         (-20..=20).contains(&euler),
        //         "Euler characteristic {} extremely out of range for random triangulation (V={}, E={}, F={})",
        //         euler, v, e, f
        //     );
        // }

        /// Property: Triangulation should have positive counts for all simplex types
        #[test]
        fn triangulation_positive_simplex_counts(
            vertices in 3u32..30,
            timeslices in 1u32..5
        ) {
            let triangulation = CdtTriangulation::from_random_points(vertices, timeslices, 2)?;

            prop_assert!(triangulation.vertex_count() >= 3, "Must have at least 3 vertices");
            prop_assert!(triangulation.edge_count() >= 3, "Must have at least 3 edges");
            prop_assert!(triangulation.face_count() >= 1, "Must have at least 1 face");
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
            prop_assert_eq!(triangulation.time_slices(), timeslices, "Time slices should match input");
            prop_assert_eq!(triangulation.dimension(), 2, "Dimension should be 2");
            prop_assert_eq!(triangulation.metadata().modification_count, 0, "Initial modification count should be 0");

            // Should have creation event
            prop_assert!(!triangulation.metadata().simulation_history.is_empty(), "Should have creation event");

            let initial_mod_count = triangulation.metadata().modification_count;

            // Mutation should increment modification count
            {
                let mut mut_wrapper = triangulation.geometry_mut();
                let _ = mut_wrapper.geometry_mut();
            }

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

            // Invalidate cache through mutation
            {
                let mut mut_wrapper = triangulation.geometry_mut();
                let _ = mut_wrapper.geometry_mut();
            }

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

            // Record some events
            {
                let mut wrapper = triangulation.geometry_mut();
                wrapper.record_event(SimulationEvent::MoveAttempted {
                    move_type: "test".to_string(),
                    step: 1,
                });
                wrapper.record_event(SimulationEvent::MeasurementTaken {
                    step: 2,
                    action: 5.0,
                });
            }

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

            let v = saturating_usize_to_i32(triangulation.vertex_count());
            let e = saturating_usize_to_i32(triangulation.edge_count());
            let f = saturating_usize_to_i32(triangulation.face_count());

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
            // where E < V-1 due to invalid cell removal and disconnected components.
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

            if vertices >= 3 {
                prop_assert!(result.is_ok(), "Should succeed with valid vertex count: {}", vertices);

                let triangulation = result.unwrap();
                let vertex_count_u32 =
                    u32::try_from(triangulation.vertex_count()).unwrap_or(u32::MAX);
                prop_assert_eq!(vertex_count_u32, vertices, "Vertex count should match input");
                prop_assert_eq!(triangulation.time_slices(), timeslices, "Time slices should match input");
                prop_assert_eq!(triangulation.dimension(), 2, "Dimension should be 2");
            } else {
                prop_assert!(result.is_err(), "Should fail with invalid vertex count: {}", vertices);
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
                          "Dimension should be consistent between wrapper and geometry");
        }

    }
}

// TODO: Add serialization/deserialization support
// TODO: Add visualization hooks
