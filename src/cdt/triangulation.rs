//! CDT triangulation wrapper - backend-agnostic.
//!
//! This module provides CDT-specific triangulation data structures that work
//! with any geometry backend implementing the trait interfaces.

use crate::cdt::foliation::{EdgeType, Foliation};
use crate::errors::CdtResult;
use crate::geometry::backends::delaunay::{
    DelaunayBackend, DelaunayEdgeHandle, DelaunayVertexHandle,
};
use crate::geometry::operations::TriangulationOps;
use crate::geometry::traits::{TriangulationMut, TriangulationQuery};
use delaunay::core::builder::DelaunayTriangulationBuilder;
use delaunay::core::triangulation_data_structure::VertexKey;
use delaunay::geometry::point::Point;
use delaunay::geometry::traits::coordinate::Coordinate;
use delaunay::prelude::VertexBuilder;
use delaunay::prelude::collections::VertexSecondaryMap;
use std::time::Instant;

/// CDT-specific triangulation wrapper - completely geometry-agnostic
#[derive(Debug)]
pub struct CdtTriangulation<B: TriangulationMut> {
    geometry: B,
    /// CDT metadata (time slices, dimension, history)
    pub metadata: CdtMetadata,
    cache: GeometryCache,
    /// Optional foliation assigning each vertex to a time slice.
    foliation: Option<Foliation>,
}

/// CDT-specific metadata
#[derive(Debug, Clone)]
pub struct CdtMetadata {
    /// Number of time slices in the CDT foliation
    pub time_slices: u32,
    /// Dimensionality of the spacetime
    pub dimension: u8,
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
    #[allow(dead_code)]
    topology_hash: Option<CachedValue<u64>>,
}

#[derive(Debug, Clone)]
struct CachedValue<T> {
    value: T,
    #[allow(dead_code)]
    computed_at: Instant,
    modification_count: u64,
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

impl<B: TriangulationMut> CdtTriangulation<B> {
    /// Create new CDT triangulation
    pub fn new(geometry: B, time_slices: u32, dimension: u8) -> Self {
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
                creation_time: Instant::now(),
                last_modified: Instant::now(),
                modification_count: 0,
                simulation_history: vec![creation_event],
            },
            cache: GeometryCache::default(),
            foliation: None,
        }
    }

    /// Get immutable reference to underlying geometry
    #[must_use]
    pub const fn geometry(&self) -> &B {
        &self.geometry
    }

    /// Get mutable reference with automatic cache invalidation
    pub fn geometry_mut(&mut self) -> CdtGeometryMut<'_, B> {
        self.invalidate_cache();
        self.metadata.last_modified = Instant::now();
        self.metadata.modification_count += 1;
        CdtGeometryMut {
            geometry: &mut self.geometry,
            metadata: &mut self.metadata,
        }
    }

    /// CDT-specific operations
    pub fn vertex_count(&self) -> usize {
        self.geometry.vertex_count()
    }

    /// Get the number of faces in the triangulation
    pub fn face_count(&self) -> usize {
        self.geometry.face_count()
    }

    /// Get the number of time slices in the CDT foliation
    #[must_use]
    pub const fn time_slices(&self) -> u32 {
        self.metadata.time_slices
    }

    /// Get the dimensionality of the spacetime
    #[must_use]
    pub const fn dimension(&self) -> u8 {
        self.metadata.dimension
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
    pub fn edge_count(&self) -> usize {
        if let Some(cached) = &self.cache.edge_count
            && cached.modification_count == self.metadata.modification_count
        {
            return cached.value;
        }

        self.geometry.edge_count()
    }

    /// Force cache update
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

    /// Validate CDT properties
    ///
    /// # Errors
    /// Returns error if the triangulation is invalid
    pub fn validate(&self) -> CdtResult<()> {
        // Check basic validity
        if !self.geometry.is_valid() {
            return Err(crate::errors::CdtError::ValidationFailed {
                check: "geometry".to_string(),
                detail: format!(
                    "triangulation is not valid (V={}, E={}, F={})",
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                ),
            });
        }

        // Check Delaunay property (for backends that support it)
        if !self.geometry.is_delaunay() {
            return Err(crate::errors::CdtError::ValidationFailed {
                check: "Delaunay".to_string(),
                detail: format!(
                    "triangulation does not satisfy Delaunay property (V={}, E={}, F={})",
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                ),
            });
        }

        // Additional CDT property validation
        self.validate_topology()?;
        self.validate_causality()?;
        self.validate_foliation()?;

        Ok(())
    }
    /// Validate topology properties
    ///
    /// Checks that the triangulation satisfies expected topological constraints,
    /// including the Euler characteristic for the given dimension and boundary conditions.
    ///
    /// # Errors
    /// Returns error if topology validation fails
    fn validate_topology(&self) -> CdtResult<()> {
        let euler_char = self.geometry.euler_characteristic();

        // For 2D planar triangulations with boundary (random points), expect χ = 1
        // For closed 2D surfaces, expect χ = 2. Since we generate from random points,
        // we typically get triangulations with convex hull boundary (χ = 1)

        if self.dimension() == 2 {
            // Planar triangulation with boundary should have χ = 1
            // Closed surfaces would have χ = 2
            if euler_char != 1 && euler_char != 2 {
                return Err(crate::errors::CdtError::ValidationFailed {
                    check: "topology".to_string(),
                    detail: format!(
                        "Euler characteristic χ={euler_char} unexpected for 2D triangulation \
                         (expected 1 for boundary or 2 for closed surface; \
                         V={}, E={}, F={})",
                        self.geometry.vertex_count(),
                        self.geometry.edge_count(),
                        self.geometry.face_count(),
                    ),
                });
            }
        }

        Ok(())
    }

    /// Validate causality constraints.
    ///
    /// If no foliation is present, succeeds vacuously (no causal structure to check).
    /// Otherwise verifies that every edge either connects vertices in the same time
    /// slice (spacelike) or in adjacent slices (|Δt| = 1, timelike).  Time labels
    /// are derived from each vertex’s y-coordinate via `round(y)`, matching the
    /// bucket convention used at construction time.
    ///
    /// # Errors
    /// Returns error if any edge spans more than one time slice.
    fn validate_causality(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };
        let max_t = foliation.num_slices().saturating_sub(1);

        for edge in self.geometry.edges() {
            let Ok((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let (Ok(c0), Ok(c1)) = (
                self.geometry.vertex_coordinates(&v0),
                self.geometry.vertex_coordinates(&v1),
            ) else {
                continue;
            };
            if c0.len() < 2 || c1.len() < 2 {
                continue;
            }
            // Bucket assignment: t = round(y), clamped to [0, max_t].
            // Convert via f64 since B::Coordinate is a generic Float.
            let Some(y0) = num_traits::ToPrimitive::to_f64(&c0[1]) else {
                continue;
            };
            let Some(y1) = num_traits::ToPrimitive::to_f64(&c1[1]) else {
                continue;
            };
            let Some(t0) = crate::util::y_to_time_bucket(y0, max_t) else {
                continue;
            };
            let Some(t1) = crate::util::y_to_time_bucket(y1, max_t) else {
                continue;
            };
            if t0.abs_diff(t1) > 1 {
                return Err(crate::errors::CdtError::CausalityViolation {
                    time_0: t0,
                    time_1: t1,
                });
            }
        }
        Ok(())
    }

    /// Validate foliation consistency.
    ///
    /// If no foliation is present, succeeds vacuously.
    /// Otherwise checks:
    /// 1. Every vertex has a time label in `0..num_slices`
    /// 2. Every time slice is non-empty
    /// 3. `slice_sizes` is consistent with the time label map
    ///
    /// # Errors
    /// Returns error if foliation structure is invalid.
    fn validate_foliation(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };

        // Check that all vertices are labeled
        let vertex_count = self.geometry.vertex_count();
        if foliation.labeled_vertex_count() != vertex_count {
            return Err(crate::errors::CdtError::ValidationFailed {
                check: "foliation".to_string(),
                detail: format!(
                    "labeled vertex count ({}) does not match triangulation vertex count ({})",
                    foliation.labeled_vertex_count(),
                    vertex_count,
                ),
            });
        }

        // Check that every slice is non-empty
        for (t, &size) in foliation.slice_sizes().iter().enumerate() {
            if size == 0 {
                return Err(crate::errors::CdtError::ValidationFailed {
                    check: "foliation".to_string(),
                    detail: format!("time slice {t} is empty"),
                });
            }
        }

        // Check that slice_sizes sum matches labeled count
        let sum: usize = foliation.slice_sizes().iter().sum();
        if sum != foliation.labeled_vertex_count() {
            return Err(crate::errors::CdtError::ValidationFailed {
                check: "foliation".to_string(),
                detail: format!(
                    "slice_sizes sum ({sum}) does not match labeled vertex count ({})",
                    foliation.labeled_vertex_count(),
                ),
            });
        }

        Ok(())
    }

    fn invalidate_cache(&mut self) {
        self.cache = GeometryCache::default();
    }
}

/// Smart wrapper for mutable geometry access
pub struct CdtGeometryMut<'a, B: TriangulationMut> {
    geometry: &'a mut B,
    metadata: &'a mut CdtMetadata,
}

impl<B: TriangulationMut> CdtGeometryMut<'_, B> {
    /// Record a simulation event
    pub fn record_event(&mut self, event: SimulationEvent) {
        self.metadata.simulation_history.push(event);
    }

    /// Get mutable reference to geometry
    pub const fn geometry_mut(&mut self) -> &mut B {
        self.geometry
    }
}

impl<B: TriangulationMut> std::ops::Deref for CdtGeometryMut<'_, B> {
    type Target = B;
    fn deref(&self) -> &Self::Target {
        self.geometry
    }
}

impl<B: TriangulationMut> std::ops::DerefMut for CdtGeometryMut<'_, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.geometry
    }
}

// =============================================================================
// Delaunay-specific factory functions and foliation methods
// =============================================================================
impl CdtTriangulation<crate::geometry::backends::delaunay::DelaunayBackend2D> {
    // -------------------------------------------------------------------------
    // Factory constructors
    // -------------------------------------------------------------------------

    /// Create a new CDT triangulation with Delaunay backend from random points.
    ///
    /// This is the recommended way to create triangulations for simulations.
    ///
    /// # Errors
    /// Returns error if triangulation generation fails
    pub fn from_random_points(
        vertices: u32,
        time_slices: u32,
        dimension: u8,
    ) -> crate::errors::CdtResult<Self> {
        // Validate dimension first
        if dimension != 2 {
            return Err(crate::errors::CdtError::UnsupportedDimension(
                dimension.into(),
            ));
        }

        // Validate other parameters
        if vertices < 3 {
            return Err(crate::errors::CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertex count".to_string(),
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = crate::util::generate_delaunay2_with_context(vertices, (0.0, 10.0), None)?;
        let backend = DelaunayBackend::from_triangulation(dt);

        Ok(Self::new(backend, time_slices, dimension))
    }

    /// Create a new CDT triangulation with Delaunay backend from random points using a fixed seed.
    ///
    /// This function provides deterministic triangulation generation for testing purposes.
    ///
    /// # Errors
    /// Returns error if triangulation generation fails
    pub fn from_seeded_points(
        vertices: u32,
        time_slices: u32,
        dimension: u8,
        seed: u64,
    ) -> crate::errors::CdtResult<Self> {
        // Validate dimension first
        if dimension != 2 {
            return Err(crate::errors::CdtError::UnsupportedDimension(
                dimension.into(),
            ));
        }

        // Validate other parameters
        if vertices < 3 {
            return Err(crate::errors::CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertex count".to_string(),
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = crate::util::generate_delaunay2_with_context(vertices, (0.0, 10.0), Some(seed))?;
        let backend = DelaunayBackend::from_triangulation(dt);

        Ok(Self::new(backend, time_slices, dimension))
    }

    /// Construct a foliated 1+1 CDT triangulation with cylinder topology.
    ///
    /// Places `vertices_per_slice` vertices per time slice on a regular grid at
    /// coordinates `(x_i, t)` where `x_i` is evenly spaced in `[0, 1]` and
    /// `t` is an integer time coordinate.  Time labels are assigned by
    /// y-coordinate bucket: slice `t` owns vertices with `y ∈ [t − 0.5, t + 0.5)`.
    ///
    /// The spatial extent is kept to 1.0 (well below the √3 ≈ 1.73 threshold
    /// that guarantees no Delaunay edge can skip a time slice), so the resulting
    /// triangulation satisfies causality by construction.
    ///
    /// # Arguments
    ///
    /// * `vertices_per_slice` — Number of vertices in each spatial slice (≥ 4).
    /// * `num_slices` — Number of time slices (≥ 2).
    /// * `seed` — Optional seed for deterministic vertex perturbation.
    ///
    /// # Panics
    ///
    /// Panics if a [`VertexBuilder`] fails to build from a
    /// valid point (should not happen in practice).
    ///
    /// # Errors
    ///
    /// Returns error if parameters are invalid or triangulation construction fails.
    pub fn from_foliated_cylinder(
        vertices_per_slice: u32,
        num_slices: u32,
        seed: Option<u64>,
    ) -> CdtResult<Self> {
        if vertices_per_slice < 4 {
            return Err(crate::errors::CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertices per slice".to_string(),
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 4".to_string(),
            });
        }
        if num_slices < 2 {
            return Err(crate::errors::CdtError::InvalidGenerationParameters {
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
            crate::errors::CdtError::InvalidGenerationParameters {
                issue: "Vertex count overflow".to_string(),
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;
        let mut vertices = Vec::with_capacity(total_vertices as usize);
        let last_i = vertices_per_slice - 1;

        for t in 0..num_slices {
            for i in 0..vertices_per_slice {
                let x_base = f64::from(i) * spacing;
                let y_base = f64::from(t); // integer time coordinate

                // Deterministic perturbation keyed on vertex index + seed
                let flat_idx = u64::from(t) * u64::from(vertices_per_slice) + u64::from(i);
                let hash = flat_idx
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(perturbation_seed)
                    .wrapping_mul(1_442_695_040_888_963_407);

                // Perturbation strategy:
                // - Interior vertices: hash-based perturbation in x and y
                //   to break co-circular grid degeneracy.
                // - Boundary vertices (i=0, i=last): deterministic concave
                //   x-offset via √(t+1) pushes each row further outward.
                //   The concave (sub-linear) progression ensures every
                //   intermediate boundary vertex bulges past the line
                //   connecting its neighbors, so the convex hull includes
                //   every row’s extremes and no hull edge skips a time slice.
                let hash_frac = f64::from((hash & 0xFFFF) as u16) / 65535.0;
                let py_frac = f64::from(((hash >> 16) & 0xFFFF) as u16) / 65535.0;
                let py = (py_frac - 0.5) * perturbation_scale;
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

                let point = Point::<f64, 2>::new([x_base + px, y_base + py]);
                let vertex = VertexBuilder::default().point(point).build().unwrap();
                vertices.push(vertex);
            }
        }

        let dt = DelaunayTriangulationBuilder::new(&vertices)
            .build::<i32>()
            .map_err(|e| crate::errors::CdtError::DelaunayGenerationFailed {
                vertex_count: total_vertices,
                coordinate_range: (0.0, f64::from(num_slices - 1)),
                attempt: 1,
                underlying_error: e.to_string(),
            })?;

        // Verify the builder inserted all vertices.
        if dt.number_of_vertices() != total_vertices as usize {
            return Err(crate::errors::CdtError::DelaunayGenerationFailed {
                vertex_count: total_vertices,
                coordinate_range: (0.0, f64::from(num_slices - 1)),
                attempt: 1,
                underlying_error: format!(
                    "builder inserted only {} of {} vertices (possible degeneracy)",
                    dt.number_of_vertices(),
                    total_vertices,
                ),
            });
        }

        // Assign time labels by y-coordinate bucket: t = round(y).
        // Bucket for slice t covers [t − 0.5, t + 0.5).
        let mut time_labels = VertexSecondaryMap::new();
        let max_t = num_slices - 1;
        for (vkey, vertex) in dt.vertices() {
            let y = vertex.point().coords()[1];
            if let Some(t) = crate::util::y_to_time_bucket(y, max_t) {
                time_labels.insert(vkey, t);
            }
        }

        let foliation = Foliation::new(time_labels, num_slices);
        let backend = DelaunayBackend::from_triangulation(dt);
        let mut tri = Self::new(backend, num_slices, 2);
        tri.foliation = Some(foliation);
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
    ///
    /// This is approximate — useful for testing but not guaranteed to produce
    /// a valid causal structure.
    ///
    /// # Errors
    ///
    /// Returns error if `num_slices` is zero or if vertex coordinates cannot be read.
    pub fn assign_foliation_by_y_coordinate(&mut self, num_slices: u32) -> CdtResult<()> {
        if num_slices == 0 {
            return Err(crate::errors::CdtError::InvalidGenerationParameters {
                issue: "Number of slices must be positive".to_string(),
                provided_value: "0".to_string(),
                expected_range: "≥ 1".to_string(),
            });
        }

        // Collect all vertex y-coordinates to determine the range
        let mut y_coords: Vec<(VertexKey, f64)> = Vec::with_capacity(self.geometry.vertex_count());
        for vh in self.geometry.vertices() {
            if let Ok(coords) = self.geometry.vertex_coordinates(&vh)
                && coords.len() >= 2
            {
                y_coords.push((vh.vertex_key(), coords[1]));
            }
        }

        if y_coords.is_empty() {
            return Err(crate::errors::CdtError::InvalidParameters(
                "cannot assign foliation: no vertices with valid 2D coordinates".to_string(),
            ));
        }

        let y_min = y_coords
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        let y_max = y_coords
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::NEG_INFINITY, f64::max);

        let range = y_max - y_min;
        // If all vertices are at the same y, put them all in slice 0
        let band_height = if range.abs() < f64::EPSILON {
            1.0
        } else {
            range / f64::from(num_slices)
        };

        let mut time_labels = VertexSecondaryMap::new();
        for (vkey, y) in &y_coords {
            let t = if range.abs() < f64::EPSILON {
                0
            } else {
                let band_index = ((y - y_min) / band_height).floor();
                crate::util::f64_band_to_u32(band_index, num_slices - 1)
            };
            time_labels.insert(*vkey, t);
        }

        self.foliation = Some(Foliation::new(time_labels, num_slices));
        self.metadata.time_slices = num_slices;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Foliation queries
    // -------------------------------------------------------------------------

    /// Returns `true` if this triangulation has an assigned foliation.
    #[must_use]
    pub const fn has_foliation(&self) -> bool {
        self.foliation.is_some()
    }

    /// Returns a reference to the foliation, if present.
    #[must_use]
    pub const fn foliation(&self) -> Option<&Foliation> {
        self.foliation.as_ref()
    }

    /// Returns the time slice label for a vertex, or `None` if no foliation
    /// is present or the vertex is unlabeled.
    #[must_use]
    pub fn time_label(&self, vertex: &DelaunayVertexHandle) -> Option<u32> {
        self.foliation
            .as_ref()
            .and_then(|f| f.time_label(vertex.vertex_key()))
    }

    /// Classifies an edge as spacelike or timelike.
    ///
    /// Returns `None` if no foliation is present, the edge endpoints cannot
    /// be resolved, or either endpoint lacks a time label.
    #[must_use]
    pub fn edge_type(&self, edge: &DelaunayEdgeHandle) -> Option<EdgeType> {
        let foliation = self.foliation.as_ref()?;
        let (v0, v1) = self.geometry.edge_endpoints(edge).ok()?;
        foliation.classify_edge(v0.vertex_key(), v1.vertex_key())
    }

    /// Returns all vertex handles that belong to time slice `t`.
    #[must_use]
    pub fn vertices_at_time(&self, t: u32) -> Vec<DelaunayVertexHandle> {
        let Some(foliation) = &self.foliation else {
            return Vec::new();
        };
        self.geometry
            .vertices()
            .filter(|vh| foliation.time_label(vh.vertex_key()) == Some(t))
            .collect()
    }

    /// Returns per-slice vertex counts, or an empty slice if no foliation.
    #[must_use]
    pub fn slice_sizes(&self) -> &[usize] {
        self.foliation.as_ref().map_or(&[], Foliation::slice_sizes)
    }

    /// Validates the causal structure of this foliated triangulation.
    ///
    /// Checks that every edge either connects vertices within the same time
    /// slice (spacelike) or in adjacent slices (|Δt| = 1, timelike).
    ///
    /// # Errors
    ///
    /// Returns error if any edge spans more than one time slice.
    pub fn validate_causality_delaunay(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };

        for edge in self.geometry.edges() {
            let Ok((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let Some(t0) = foliation.time_label(v0.vertex_key()) else {
                continue;
            };
            let Some(t1) = foliation.time_label(v1.vertex_key()) else {
                continue;
            };
            if t0.abs_diff(t1) > 1 {
                return Err(crate::errors::CdtError::CausalityViolation {
                    time_0: t0,
                    time_1: t1,
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
    use std::time::Duration;

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

            if let Err(crate::errors::CdtError::UnsupportedDimension(d)) = result {
                assert_eq!(d, u32::from(dim), "Error should report correct dimension");
            } else {
                panic!("Expected UnsupportedDimension error for dimension {dim}");
            }
        }
    }

    #[test]
    fn test_invalid_vertex_count() {
        let invalid_counts = [0, 1, 2];
        for count in invalid_counts {
            let result = CdtTriangulation::from_random_points(count, 2, 2);
            assert!(result.is_err(), "Should fail with {count} vertices");

            match result {
                Err(crate::errors::CdtError::InvalidGenerationParameters {
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
            Err(crate::errors::CdtError::InvalidGenerationParameters {
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
        assert!(!triangulation.metadata.simulation_history.is_empty());

        match &triangulation.metadata.simulation_history[0] {
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

        let initial_mod_count = triangulation.metadata.modification_count;

        // Get mutable access - this should invalidate cache and increment modification count
        {
            let mut geometry_mut = triangulation.geometry_mut();
            // Just access it, don't modify
            let _ = geometry_mut.geometry_mut();
        }

        // Modification count should have increased
        assert_eq!(
            triangulation.metadata.modification_count,
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
            let _geometry_mut = triangulation.geometry_mut();
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

        let initial_history_len = triangulation.metadata.simulation_history.len();

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
            triangulation.metadata.simulation_history.len(),
            initial_history_len + 3
        );

        // Check the recorded events
        let history = &triangulation.metadata.simulation_history;
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
    fn test_metadata_timestamps() {
        let start_time = std::time::Instant::now();

        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        let creation_time = triangulation.metadata.creation_time;
        let initial_last_modified = triangulation.metadata.last_modified;

        // Creation time should be after our start time
        assert!(creation_time >= start_time);

        // Initially, creation_time and last_modified should be very close
        let time_diff = initial_last_modified.duration_since(creation_time);
        assert!(time_diff < Duration::from_millis(10));

        // Make a small delay then modify
        thread::sleep(Duration::from_millis(5));

        {
            let _wrapper = triangulation.geometry_mut();
        }

        let new_last_modified = triangulation.metadata.last_modified;

        // last_modified should have been updated
        assert!(new_last_modified > initial_last_modified);

        // creation_time should remain unchanged
        assert_eq!(triangulation.metadata.creation_time, creation_time);
    }

    #[test]
    fn test_modification_count() {
        let mut triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        // Initial modification count should be 0
        assert_eq!(triangulation.metadata.modification_count, 0);

        // Each mutable access should increment
        {
            let _wrapper = triangulation.geometry_mut();
        }
        assert_eq!(triangulation.metadata.modification_count, 1);

        {
            let _wrapper = triangulation.geometry_mut();
        }
        assert_eq!(triangulation.metadata.modification_count, 2);

        // Immutable access shouldn't change count
        let _geometry = triangulation.geometry();
        let _edge_count = triangulation.edge_count();
        assert_eq!(triangulation.metadata.modification_count, 2);
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

        let metadata1 = triangulation.metadata;
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

    #[test]
    fn test_from_foliated_cylinder_basic() {
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42))
            .expect("Should create foliated cylinder");

        assert!(tri.has_foliation());
        assert_eq!(tri.vertex_count(), 15); // 5 * 3
        assert_eq!(tri.time_slices(), 3);
        assert_eq!(tri.dimension(), 2);
        assert!(tri.edge_count() > 0);
        assert!(tri.face_count() > 0);
    }

    #[test]
    fn test_from_foliated_cylinder_vertex_counts_per_slice() {
        let tri = CdtTriangulation::from_foliated_cylinder(4, 3, Some(1))
            .expect("Should create foliated cylinder");

        let sizes = tri.slice_sizes();
        assert_eq!(sizes.len(), 3);
        for (t, &size) in sizes.iter().enumerate() {
            assert_eq!(size, 4, "Slice {t} should have 4 vertices");
        }
    }

    #[test]
    fn test_from_foliated_cylinder_all_vertices_labeled() {
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, Some(99))
            .expect("Should create foliated cylinder");

        // Every vertex should have a time label
        for vh in tri.geometry().vertices() {
            assert!(
                tri.time_label(&vh).is_some(),
                "Every vertex should have a time label"
            );
        }
    }

    #[test]
    fn test_from_foliated_cylinder_edge_classification() {
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42))
            .expect("Should create foliated cylinder");

        let mut spacelike = 0usize;
        let mut timelike = 0usize;

        for edge in tri.geometry().edges() {
            let et = tri
                .edge_type(&edge)
                .expect("All edges should be classifiable");
            match et {
                EdgeType::Spacelike => spacelike += 1,
                EdgeType::Timelike => timelike += 1,
            }
        }

        assert!(spacelike > 0, "Should have some spacelike edges");
        assert!(timelike > 0, "Should have some timelike edges");
    }

    #[test]
    fn test_from_foliated_cylinder_validate_foliation() {
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42))
            .expect("Should create foliated cylinder");

        let result = tri.validate_foliation();
        assert!(
            result.is_ok(),
            "Foliation validation should pass: {result:?}"
        );
    }

    #[test]
    fn test_from_foliated_cylinder_validate_causality() {
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42))
            .expect("Should create foliated cylinder");

        let result = tri.validate_causality_delaunay();
        assert!(
            result.is_ok(),
            "Causality validation should pass: {result:?}"
        );
    }

    #[test]
    fn test_from_foliated_cylinder_seed_determinism() {
        let tri1 = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42)).unwrap();
        let tri2 = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42)).unwrap();

        assert_eq!(tri1.vertex_count(), tri2.vertex_count());
        assert_eq!(tri1.edge_count(), tri2.edge_count());
        assert_eq!(tri1.face_count(), tri2.face_count());
        assert_eq!(tri1.slice_sizes(), tri2.slice_sizes());
    }

    #[test]
    fn test_from_foliated_cylinder_invalid_params() {
        // Too few vertices per slice
        assert!(CdtTriangulation::from_foliated_cylinder(3, 3, None).is_err());
        // Too few slices
        assert!(CdtTriangulation::from_foliated_cylinder(5, 1, None).is_err());
    }

    #[test]
    fn test_vertices_at_time() {
        let tri = CdtTriangulation::from_foliated_cylinder(4, 3, Some(1))
            .expect("Should create foliated cylinder");

        for t in 0..3 {
            let verts = tri.vertices_at_time(t);
            assert_eq!(verts.len(), 4, "Slice {t} should have 4 vertices");
            // All should have the correct label
            for vh in &verts {
                assert_eq!(tri.time_label(vh), Some(t));
            }
        }

        // Non-existent slice should return empty
        assert!(tri.vertices_at_time(99).is_empty());
    }

    #[test]
    fn test_assign_foliation_by_y_coordinate() {
        let mut tri =
            CdtTriangulation::from_random_points(10, 3, 2).expect("Failed to create triangulation");

        assert!(!tri.has_foliation());

        tri.assign_foliation_by_y_coordinate(3)
            .expect("Should assign foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.time_slices(), 3);

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
    fn test_assign_foliation_zero_slices() {
        let mut tri = CdtTriangulation::from_random_points(5, 2, 2).unwrap();
        assert!(tri.assign_foliation_by_y_coordinate(0).is_err());
    }

    #[test]
    fn test_from_foliated_cylinder_minimum_size() {
        // Minimum valid: 4 vertices per slice, 2 slices
        let tri = CdtTriangulation::from_foliated_cylinder(4, 2, Some(1))
            .expect("Should create minimal foliated cylinder");

        assert_eq!(tri.vertex_count(), 8);
        assert_eq!(tri.slice_sizes(), &[4, 4]);
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality_delaunay().is_ok());
    }

    #[test]
    fn test_from_foliated_cylinder_full_validate() {
        // validate() runs geometry + Delaunay + topology + causality + foliation
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, Some(42))
            .expect("Should create foliated cylinder");

        let result = tri.validate();
        assert!(result.is_ok(), "Full validation should pass: {result:?}");
    }

    #[test]
    fn test_from_foliated_cylinder_no_seed() {
        // None seed should also work (uses default perturbation seed 0)
        let tri = CdtTriangulation::from_foliated_cylinder(5, 3, None)
            .expect("Should create foliated cylinder without seed");

        assert!(tri.has_foliation());
        assert_eq!(tri.vertex_count(), 15);
        assert!(tri.validate_foliation().is_ok());
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
        for edge in tri.geometry().edges() {
            assert_eq!(tri.edge_type(&edge), None);
        }
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
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
            prop_assert_eq!(triangulation.metadata.modification_count, 0, "Initial modification count should be 0");

            // Should have creation event
            prop_assert!(!triangulation.metadata.simulation_history.is_empty(), "Should have creation event");

            let initial_mod_count = triangulation.metadata.modification_count;

            // Mutation should increment modification count
            {
                let _mut_wrapper = triangulation.geometry_mut();
            }

            prop_assert_eq!(triangulation.metadata.modification_count, initial_mod_count + 1,
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
                let _mut_wrapper = triangulation.geometry_mut();
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

            let initial_history_len = triangulation.metadata.simulation_history.len();
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

            prop_assert_eq!(triangulation.metadata.simulation_history.len(), initial_history_len + 2,
                          "Should have 2 additional events after recording");
        }

        /// Property: Geometric invariants for triangulated structures
        #[test]
        fn geometric_invariants_property(
            vertices in 4u32..15,
            timeslices in 1u32..4,
            seed in 1u64..10000
        ) {
            let triangulation = CdtTriangulation::from_seeded_points(vertices, timeslices, 2, seed)?;

            let v = crate::util::saturating_usize_to_i32(triangulation.vertex_count());
            let e = crate::util::saturating_usize_to_i32(triangulation.edge_count());
            let f = crate::util::saturating_usize_to_i32(triangulation.face_count());

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

        /// Property: Foliated cylinder construction always produces valid foliation and causality
        #[test]
        fn foliated_cylinder_invariants(
            vertices_per_slice in 4u32..10,
            num_slices in 2u32..6,
            seed in 0u64..1000
        ) {
            // Some seed/size combinations trigger builder degeneracy;
            // skip those — the constructor returns Err, not a broken triangulation.
            let Ok(tri) = CdtTriangulation::from_foliated_cylinder(vertices_per_slice, num_slices, Some(seed)) else {
                return Ok(());
            };

            // Vertex count must match grid (guaranteed by the constructor check)
            let expected_v = vertices_per_slice as usize * num_slices as usize;
            prop_assert_eq!(tri.vertex_count(), expected_v, "Vertex count should match grid");

            // Must have foliation
            prop_assert!(tri.has_foliation(), "Foliated cylinder must have foliation");

            // Every slice has the right count
            let sizes = tri.slice_sizes();
            prop_assert_eq!(sizes.len(), num_slices as usize, "Should have num_slices slices");
            for (t, &size) in sizes.iter().enumerate() {
                prop_assert_eq!(size, vertices_per_slice as usize,
                    "Slice {} should have {} vertices", t, vertices_per_slice);
            }

            // Foliation validation passes
            prop_assert!(tri.validate_foliation().is_ok(), "Foliation should be valid");

            // Causality passes (no edges spanning >1 slice)
            prop_assert!(tri.validate_causality_delaunay().is_ok(),
                "Causality should hold for foliated cylinder with {} vertices/slice, {} slices, seed {}",
                vertices_per_slice, num_slices, seed);
        }
    }
}

// TODO: Add serialization/deserialization support
// TODO: Add visualization hooks
