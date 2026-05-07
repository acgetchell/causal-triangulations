#![forbid(unsafe_code)]

//! Builders for CDT triangulations backed by Delaunay geometry.

use super::CdtTriangulation;
use crate::cdt::foliation::{Foliation, FoliationError};
use crate::config::CdtTopology;
use crate::errors::{CdtError, CdtResult};
use crate::geometry::DelaunayBackend2D;
use crate::geometry::generators::{
    build_delaunay2_with_data, build_periodic_toroidal_delaunay2, generate_delaunay2,
};
use crate::geometry::traits::TriangulationQuery;

/// Rewrites toroidal builder failures with CDT-level generation context.
///
/// The lower geometry builder reports failures in terms of its input shape; this
/// helper preserves the underlying diagnostic while normalizing the public error
/// fields to the toroidal CDT constructor's vertex count and first attempt.
pub(super) fn remap_toroidal_generation_error(error: CdtError, total_vertices: u32) -> CdtError {
    match error {
        CdtError::DelaunayGenerationFailed {
            coordinate_range,
            underlying_error,
            ..
        } => CdtError::DelaunayGenerationFailed {
            vertex_count: total_vertices,
            coordinate_range,
            attempt: 1,
            underlying_error,
        },
        other => other,
    }
}

/// Rewrites Delaunay strip builder failures with CDT-level generation context.
fn remap_strip_generation_error(
    error: CdtError,
    total_vertices: u32,
    coordinate_max: f64,
) -> CdtError {
    match error {
        CdtError::DelaunayGenerationFailed {
            underlying_error, ..
        } => CdtError::DelaunayGenerationFailed {
            vertex_count: total_vertices,
            coordinate_range: (0.0, coordinate_max),
            attempt: 1,
            underlying_error,
        },
        other => other,
    }
}

/// Builds a CDT-level generation error for Delaunay strip construction failures.
const fn strip_generation_error(
    total_vertices: u32,
    coordinate_max: f64,
    underlying_error: String,
) -> CdtError {
    CdtError::DelaunayGenerationFailed {
        vertex_count: total_vertices,
        coordinate_range: (0.0, coordinate_max),
        attempt: 1,
        underlying_error,
    }
}

/// Verifies that the Delaunay strip builder returned the requested mesh size.
#[expect(
    clippy::too_many_arguments,
    reason = "count mismatch diagnostics preserve both requested CDT parameters and expected builder counts"
)]
pub(super) fn validate_strip_counts(
    backend: &DelaunayBackend2D,
    total_vertices: u32,
    total_cells: u32,
    expected_vertices: usize,
    expected_faces: usize,
    vertices_per_slice: u32,
    num_slices: u32,
    coordinate_max: f64,
) -> CdtResult<()> {
    if backend.vertex_count() != expected_vertices {
        return Err(strip_generation_error(
            total_vertices,
            coordinate_max,
            format!(
                "build_delaunay2_with_data()/from_cdt_strip() produced {} vertices, expected {} for vertices_per_slice={} and num_slices={}",
                backend.vertex_count(),
                total_vertices,
                vertices_per_slice,
                num_slices,
            ),
        ));
    }
    if backend.face_count() != expected_faces {
        return Err(strip_generation_error(
            total_vertices,
            coordinate_max,
            format!(
                "build_delaunay2_with_data()/from_cdt_strip() produced {} faces, expected {} for vertices_per_slice={} and num_slices={}",
                backend.face_count(),
                total_cells,
                vertices_per_slice,
                num_slices,
            ),
        ));
    }

    Ok(())
}

/// Builds a CDT-level generation error for periodic toroidal construction failures.
const fn toroidal_generation_error(
    total_vertices: u32,
    coordinate_range: (f64, f64),
    underlying_error: String,
) -> CdtError {
    CdtError::DelaunayGenerationFailed {
        vertex_count: total_vertices,
        coordinate_range,
        attempt: 1,
        underlying_error,
    }
}

/// Verifies that the periodic toroidal builder returned the requested mesh size.
pub(super) fn validate_toroidal_counts(
    backend: &DelaunayBackend2D,
    total_vertices: u32,
    expected_vertices: usize,
    expected_faces: usize,
    coordinate_range: (f64, f64),
) -> CdtResult<()> {
    if backend.vertex_count() != expected_vertices || backend.face_count() != expected_faces {
        return Err(toroidal_generation_error(
            total_vertices,
            coordinate_range,
            format!(
                "periodic toroidal builder produced {} vertices and {} faces, expected {} vertices and {} faces",
                backend.vertex_count(),
                backend.face_count(),
                total_vertices,
                expected_faces,
            ),
        ));
    }

    Ok(())
}

impl CdtTriangulation<DelaunayBackend2D> {
    /// Re-reads current backend labels during validation so stale stored bookkeeping is detected.
    pub(super) fn live_slice_sizes_from_vertex_labels(
        backend: &DelaunayBackend2D,
        num_slices: u32,
    ) -> CdtResult<Vec<usize>> {
        if num_slices == 0 {
            return Err(FoliationError::SliceSizeMismatch {
                slice_sizes_len: 0,
                num_slices,
            }
            .into());
        }

        let mut slice_sizes = vec![0usize; num_slices as usize];

        for (vertex, vh) in backend.vertices().enumerate() {
            if let Some(t) = backend.vertex_data_by_key(vh.vertex_key()) {
                let idx = t as usize;
                if idx >= slice_sizes.len() {
                    return Err(FoliationError::OutOfRangeVertexLabel {
                        vertex,
                        label: t,
                        expected_range_end: slice_sizes.len(),
                    }
                    .into());
                }
                slice_sizes[idx] += 1;
            } else {
                return Err(FoliationError::MissingVertexLabel { vertex }.into());
            }
        }

        Ok(slice_sizes)
    }

    /// Creates an unfoliated triangulation with a Delaunay backend from random points.
    ///
    /// This is useful for raw geometry tests and experiments. It does not
    /// assign time labels or CDT cell classifications, so production CDT
    /// simulations should prefer [`CdtTriangulation::from_cdt_strip`] or
    /// [`CdtTriangulation::from_toroidal_cdt`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::UnsupportedDimension`] if `dimension != 2`.
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices < 3`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if random point generation
    /// or Delaunay construction fails. Propagates metadata validation errors
    /// from [`CdtTriangulation::try_new`], including `time_slices == 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_random_points(5, 2, 2)?;
    ///     assert_eq!(tri.time_slices(), 2);
    ///     assert!(!tri.has_foliation());
    ///     Ok(())
    /// }
    /// ```
    pub fn from_random_points(vertices: u32, time_slices: u32, dimension: u8) -> CdtResult<Self> {
        if dimension != 2 {
            return Err(CdtError::UnsupportedDimension(dimension.into()));
        }

        if vertices < 3 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertex count".to_string(),
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = generate_delaunay2(vertices, (0.0, 10.0), None)?;
        let backend = DelaunayBackend2D::from_triangulation(dt);

        Self::try_new(backend, time_slices, dimension)
    }

    /// Creates an unfoliated triangulation with a Delaunay backend from a fixed random seed.
    ///
    /// Use this builder for raw geometry examples, tests, and benchmarks that
    /// need deterministic input geometry. It does not assign time labels or
    /// CDT cell classifications, so production CDT simulations should prefer
    /// [`CdtTriangulation::from_cdt_strip`] or
    /// [`CdtTriangulation::from_toroidal_cdt`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::UnsupportedDimension`] if `dimension != 2`.
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices < 3`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if seeded point
    /// generation or Delaunay construction fails. Propagates metadata
    /// validation errors from [`CdtTriangulation::try_new`], including
    /// `time_slices == 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53)?;
    ///     assert_eq!(tri.vertex_count(), 5);
    ///     assert!(!tri.has_foliation());
    ///     Ok(())
    /// }
    /// ```
    pub fn from_seeded_points(
        vertices: u32,
        time_slices: u32,
        dimension: u8,
        seed: u64,
    ) -> CdtResult<Self> {
        if dimension != 2 {
            return Err(CdtError::UnsupportedDimension(dimension.into()));
        }

        if vertices < 3 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: "Insufficient vertex count".to_string(),
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = generate_delaunay2(vertices, (0.0, 10.0), Some(seed))?;
        let backend = DelaunayBackend2D::from_triangulation(dt);

        Self::try_new(backend, time_slices, dimension)
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
    /// Returns [`CdtError::DelaunayValidationFailed`] if the backend fails the
    /// upstream Level 1-4 Delaunay validator. Returns topology, foliation,
    /// causality, or classification errors if the labels do not form a strict
    /// CDT mesh.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt);
    ///     let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///
    ///     assert!(tri.has_foliation());
    ///     assert_eq!(tri.slice_sizes(), &[2, 1]);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_labeled_delaunay(
        backend: DelaunayBackend2D,
        time_slices: u32,
        dimension: u8,
    ) -> CdtResult<Self> {
        if dimension != 2 {
            return Err(CdtError::UnsupportedDimension(dimension.into()));
        }

        Self::check_time_slices(CdtTopology::OpenBoundary, time_slices)?;
        let slice_sizes = Self::live_slice_sizes_from_vertex_labels(&backend, time_slices)?;
        for (slice, &size) in slice_sizes.iter().enumerate() {
            if size == 0 {
                return Err(FoliationError::EmptySlice { slice }.into());
            }
        }
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, time_slices).map_err(CdtError::from)?;

        let mut tri = Self::try_new(backend, time_slices, dimension)?;
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();
        tri.validate_initial_delaunay_cdt()?;
        Ok(tri)
    }

    /// Construct a Delaunay-backed true 1+1 CDT strip from layered points.
    ///
    /// Places `vertices_per_slice` vertices on each open spatial slice and
    /// builds a Delaunay triangulation from the labeled coordinates. The
    /// resulting finite faces must all classify as Up `(2,1)` or Down `(1,2)`
    /// triangles before the constructor succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices_per_slice < 4`,
    /// `num_slices < 2`, or the derived vertex or cell count overflows `u32`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if constructor storage cannot
    /// be reserved, if the underlying Delaunay builder rejects the points, if
    /// `build_delaunay2_with_data()` returns a vertex or face count that does not
    /// match the requested strip. Returns [`CdtError::DelaunayValidationFailed`]
    /// if the constructed backend does not satisfy the Level 1-4 Delaunay
    /// validator. Returns [`CdtError::Foliation`],
    /// [`CdtError::CausalityViolation`], or [`CdtError::ValidationFailed`] if the
    /// constructed strip fails CDT validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert_eq!(tri.vertex_count(), 8);
    ///     assert_eq!(tri.face_count(), 6);
    ///     assert!(tri.validate_cell_classification().is_ok());
    ///     Ok(())
    /// }
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "Delaunay strip construction includes fallible allocation handling and post-build validation"
    )]
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

        let total_vertices = vertices_per_slice.checked_mul(num_slices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: "Vertex count overflow".to_string(),
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;

        let spatial_quads = vertices_per_slice - 1;
        let temporal_quads = num_slices - 1;
        let total_quads = spatial_quads.checked_mul(temporal_quads).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: "Cell count overflow".to_string(),
                provided_value: format!("{spatial_quads} × {temporal_quads}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;
        let total_cells =
            total_quads
                .checked_mul(2)
                .ok_or_else(|| CdtError::InvalidGenerationParameters {
                    issue: "Cell count overflow".to_string(),
                    provided_value: format!("2 × {total_quads}"),
                    expected_range: "product ≤ u32::MAX".to_string(),
                })?;

        let coordinate_max = f64::from(num_slices).max(2.0);
        let generation_failed = |underlying_error: String| {
            strip_generation_error(total_vertices, coordinate_max, underlying_error)
        };

        let expected_vertices =
            usize::try_from(total_vertices).map_err(|err| generation_failed(err.to_string()))?;
        let expected_faces =
            usize::try_from(total_cells).map_err(|err| generation_failed(err.to_string()))?;

        let n = usize::try_from(vertices_per_slice)
            .map_err(|err| generation_failed(err.to_string()))?;
        let t_count =
            usize::try_from(num_slices).map_err(|err| generation_failed(err.to_string()))?;

        let spacing = 1.0_f64 / f64::from(vertices_per_slice - 1);
        let side_jitter = spacing / 4.0;
        let interior_jitter = spacing / (16.0 * f64::from(num_slices));
        let vertical_jitter = 1.0_f64 / (64.0 * f64::from(num_slices));
        let mut vertex_specs: Vec<([f64; 2], u32)> = Vec::new();
        vertex_specs
            .try_reserve_exact(expected_vertices)
            .map_err(|err| {
                generation_failed(format!(
                    "from_cdt_strip() failed to reserve {expected_vertices} vertex specs for vertices_per_slice={vertices_per_slice}, num_slices={num_slices}: {err}"
                ))
            })?;
        for t in 0..num_slices {
            for i in 0..vertices_per_slice {
                let temporal_index = f64::from(t);
                let temporal_span = f64::from(num_slices - 1);
                let side_arc = side_jitter * temporal_index * f64::from(num_slices - 1 - t)
                    / temporal_span.powi(2);
                let x = if i == 0 || i == vertices_per_slice - 1 {
                    let boundary = f64::from(i).mul_add(spacing, side_jitter);
                    if i == 0 {
                        boundary - side_arc
                    } else {
                        boundary + side_arc
                    }
                } else {
                    let sign = if (i + t).is_multiple_of(2) { 1.0 } else { -1.0 };
                    f64::from(i).mul_add(spacing, side_jitter) + sign * interior_jitter
                };
                let spatial_index = f64::from(i);
                let arc = vertical_jitter * spatial_index * f64::from(vertices_per_slice - 1 - i)
                    / f64::from((vertices_per_slice - 1).pow(2));
                let base_y = f64::from(t) + vertical_jitter;
                let y = if t == 0 {
                    base_y - arc
                } else if t == num_slices - 1 {
                    base_y + arc
                } else {
                    let sign = if (i + t).is_multiple_of(2) { 1.0 } else { -1.0 };
                    (sign * arc).mul_add(0.5, base_y)
                };
                vertex_specs.push(([x, y], t));
            }
        }

        let dt = build_delaunay2_with_data(&vertex_specs)
            .map_err(|err| remap_strip_generation_error(err, total_vertices, coordinate_max))?;

        let backend = DelaunayBackend2D::from_triangulation(dt);
        validate_strip_counts(
            &backend,
            total_vertices,
            total_cells,
            expected_vertices,
            expected_faces,
            vertices_per_slice,
            num_slices,
            coordinate_max,
        )?;
        let slice_sizes = vec![n; t_count];
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, num_slices).map_err(CdtError::from)?;

        let mut tri = Self::try_new(backend, num_slices, 2)?;
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();
        tri.validate_initial_delaunay_cdt()?;

        Ok(tri)
    }

    /// Construct a foliated 1+1 CDT on a torus (S¹×S¹).
    ///
    /// Places `vertices_per_slice` vertices per time slice on a unit lattice
    /// in an `N × T` toroidal domain.  Time slices wrap: slice
    /// `num_slices - 1` connects back to slice `0`.
    ///
    /// The triangulation is built through
    /// [`crate::geometry::generators::build_periodic_toroidal_delaunay2`],
    /// which uses the upstream periodic image-point constructor and then
    /// requires full Delaunay Level 1-4 validation before the CDT wrapper is
    /// returned.
    ///
    /// # Mesh structure
    ///
    /// With `N = vertices_per_slice` and `T = num_slices` the resulting mesh
    /// has `N · T` vertices, `3 · N · T` edges, and `2 · N · T` triangles
    /// (`V − E + F = 0`, the Euler characteristic of the torus).  Each pair of
    /// adjacent slices `(t, t+1) mod T` and each spatial pair `(i, i+1) mod N`
    /// contribute two Delaunay triangles, and every triangle must classify as
    /// Up `(2,1)` or Down `(1,2)`, with exactly one spacelike edge and two
    /// timelike edges.
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
    /// or `num_slices < 3`, or if the derived vertex or face count overflows `u32`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if upstream periodic
    /// Delaunay construction rejects the mesh, if constructor storage cannot be
    /// reserved, or if the builder returns a vertex or face count that does not
    /// match the requested toroidal CDT. Returns
    /// [`CdtError::DelaunayValidationFailed`] if full Delaunay validation fails.
    /// Returns [`CdtError::Foliation`],
    /// [`CdtError::CausalityViolation`], or [`CdtError::ValidationFailed`] if the
    /// constructed triangulation fails CDT validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_toroidal_cdt(4, 3)?;
    ///     assert_eq!(tri.vertex_count(), 12);
    ///     assert_eq!(tri.face_count(), 24);
    ///     assert!(tri.has_foliation());
    ///     Ok(())
    /// }
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
        let total_cells =
            total_vertices
                .checked_mul(2)
                .ok_or_else(|| CdtError::InvalidGenerationParameters {
                    issue: "Cell count overflow".to_string(),
                    provided_value: format!("2 × {total_vertices}"),
                    expected_range: "product ≤ u32::MAX".to_string(),
                })?;

        let generation_failed = |underlying_error: String| {
            let coordinate_max = f64::from(vertices_per_slice.max(num_slices) - 1);
            toroidal_generation_error(total_vertices, (0.0, coordinate_max), underlying_error)
        };

        let expected_vertices =
            usize::try_from(total_vertices).map_err(|err| generation_failed(err.to_string()))?;
        let expected_faces =
            usize::try_from(total_cells).map_err(|err| generation_failed(err.to_string()))?;

        let n = usize::try_from(vertices_per_slice)
            .map_err(|err| generation_failed(err.to_string()))?;
        let t_count =
            usize::try_from(num_slices).map_err(|err| generation_failed(err.to_string()))?;

        // --- Vertex coordinates (S¹ × S¹) ---
        //
        // Use a unit square lattice in a toroidal domain of size N × T. This
        // keeps neighboring spatial and temporal lattice spacings comparable for
        // the periodic Delaunay constructor, independent of the requested aspect
        // ratio.
        let n_f = f64::from(vertices_per_slice);
        let t_f = f64::from(num_slices);
        let mut vertex_specs: Vec<([f64; 2], u32)> = Vec::new();
        vertex_specs
            .try_reserve_exact(expected_vertices)
            .map_err(|err| {
                generation_failed(format!(
                    "from_toroidal_cdt() failed to reserve {expected_vertices} vertex specs for vertices_per_slice={vertices_per_slice}, num_slices={num_slices}: {err}"
                ))
            })?;
        for t in 0..num_slices {
            for i in 0..vertices_per_slice {
                let x = f64::from(i);
                let y = f64::from(t);
                vertex_specs.push(([x, y], t));
            }
        }

        let domain = [n_f, t_f];
        let dt = build_periodic_toroidal_delaunay2(&vertex_specs, domain)
            .map_err(|e| remap_toroidal_generation_error(e, total_vertices))?;

        let backend = DelaunayBackend2D::from_triangulation(dt);
        validate_toroidal_counts(
            &backend,
            total_vertices,
            expected_vertices,
            expected_faces,
            (0.0, n_f.max(t_f) - 1.0),
        )?;

        let slice_sizes = vec![n; t_count];
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, num_slices).map_err(CdtError::from)?;

        let mut tri = Self::with_topology(backend, num_slices, 2, CdtTopology::Toroidal)?;
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();
        tri.validate_initial_delaunay_cdt()?;

        Ok(tri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::foliation::{CellType, EdgeType, FoliationError};
    use crate::geometry::generators::{build_delaunay2_from_cells, build_delaunay2_with_data};

    /// Builds a minimal labeled Delaunay backend for constructor tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt)
    }

    /// Builds a Delaunay strip and verifies it is a strict CDT mesh.
    fn strict_strip(
        vertices_per_slice: u32,
        num_slices: u32,
    ) -> CdtTriangulation<DelaunayBackend2D> {
        let tri = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("Delaunay strip construction should succeed");
        assert_eq!(
            tri.vertex_count(),
            vertices_per_slice as usize * num_slices as usize
        );
        assert_eq!(
            tri.face_count(),
            2 * (vertices_per_slice as usize - 1) * (num_slices as usize - 1)
        );
        assert_eq!(
            tri.slice_sizes(),
            vec![vertices_per_slice as usize; num_slices as usize].as_slice()
        );
        tri.validate_foliation()
            .expect("Delaunay strip foliation should validate");
        tri.validate_causality_delaunay()
            .expect("Delaunay strip causality should validate");
        tri.validate_topology()
            .expect("Delaunay strip topology should validate");
        tri.geometry()
            .validate_delaunay()
            .expect("Delaunay strip should pass upstream Level 1-4 validation");
        tri.validate_cell_classification()
            .expect("all Delaunay strip cells should classify");
        for face in tri.geometry().faces() {
            assert!(tri.cell_type(&face).is_some());
            assert!(tri.cell_type_from_data(&face).is_some());
        }
        tri
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
                coordinate_range: (-1.0, 1.0),
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

        assert_eq!(tri1.dimension(), tri2.dimension());
        assert_eq!(tri1.time_slices(), tri2.time_slices());
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
    fn test_from_seeded_points_rejects_zero_time_slices() {
        let result = CdtTriangulation::from_seeded_points(5, 0, 2, 53);

        assert!(matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref provided_value,
                ref expected,
                ..
            }) if field == "timeslices" && provided_value == "0" && expected == "≥ 1"
        ));
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
    fn test_from_labeled_delaunay_preserves_foliation() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), &[2, 1]);
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_cell_classification().is_ok());

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
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                ref provided_value,
                ref expected,
                ..
            }) if field == "timeslices" && provided_value == "0" && expected == "≥ 1"
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
            Err(CdtError::Foliation(FoliationError::OutOfRangeVertexLabel {
                label: 5,
                expected_range_end: 2,
                ..
            }))
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
            Err(CdtError::Foliation(FoliationError::EmptySlice { slice: 1 }))
        ));
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_non_cdt_cells() {
        let dt = build_delaunay2_from_cells(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 0),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit cells should build before constructor validation");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 2);

        assert!(matches!(
            result,
            Err(CdtError::ValidationFailed {
                ref check,
                ref detail,
            }) if *check == crate::CdtValidationCheck::Causality
                && detail.contains("invalid CDT triangle")
        ));
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_explicit_non_delaunay_cells() {
        let dt = build_delaunay2_from_cells(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([0.2, 0.2], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit cells should build before constructor validation");
        let backend = DelaunayBackend2D::from_triangulation(dt);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 2);

        assert!(matches!(
            result,
            Err(CdtError::DelaunayValidationFailed {
                level,
                ..
            }) if level == crate::DelaunayValidationLevel::Four
        ));
    }

    #[test]
    fn test_from_cdt_strip_all_vertices_labeled() {
        let tri = strict_strip(5, 3);
        for vertex in tri.geometry().vertices() {
            assert!(tri.time_label(&vertex).is_some());
        }
    }

    #[test]
    fn test_from_cdt_strip_edge_classification() {
        let tri = strict_strip(5, 3);
        for edge in tri.geometry().edges() {
            assert!(matches!(
                tri.edge_type(&edge),
                Some(EdgeType::Spacelike | EdgeType::Timelike)
            ));
        }
    }

    #[test]
    fn test_from_cdt_strip_rejects_invalid_params() {
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
    fn test_from_cdt_strip_rejects_cell_count_overflow() {
        let result = CdtTriangulation::from_cdt_strip(65_535, 65_537);

        assert!(matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if issue == "Cell count overflow"
                && provided_value == "2 × 4294836224"
                && expected_range == "product ≤ u32::MAX"
        ));
    }

    #[test]
    fn test_from_cdt_strip_builds_valid_mesh() {
        let tri = CdtTriangulation::from_cdt_strip(4, 2).expect("Delaunay strip should build");
        assert_eq!(tri.vertex_count(), 8);
        assert_eq!(tri.face_count(), 6);
        assert!(tri.geometry().validate_delaunay().is_ok());
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality_delaunay().is_ok());
        assert!(tri.validate_cell_classification().is_ok());
    }

    #[test]
    fn test_explicit_strip_count_validation_rejects_face_mismatch() {
        let tri = CdtTriangulation::from_cdt_strip(4, 2).expect("Delaunay strip should build");
        let result = validate_strip_counts(tri.geometry(), 8, 7, 8, 7, 4, 2, 1.0);

        assert!(matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count: 8,
                coordinate_range: (0.0, 1.0),
                attempt: 1,
                ref underlying_error,
            }) if underlying_error.contains("build_delaunay2_with_data()/from_cdt_strip()")
                && underlying_error.contains("produced 6 faces, expected 7")
                && underlying_error.contains("vertices_per_slice=4")
                && underlying_error.contains("num_slices=2")
        ));
    }

    #[test]
    fn test_cell_type_returns_up_or_down() {
        let tri = strict_strip(5, 3);
        for face in tri.geometry().faces() {
            assert!(matches!(
                tri.cell_type(&face),
                Some(CellType::Up | CellType::Down)
            ));
        }
    }

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
    fn test_from_toroidal_cdt_initializes_delaunay_pl_manifold() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        assert_eq!(tri.vertex_count(), 12);
        assert_eq!(tri.face_count(), 24);
        assert_eq!(tri.geometry().periodic_domain(), Some([4.0, 3.0]));
        tri.geometry()
            .validate_delaunay()
            .expect("initial toroidal CDT must pass upstream Level 1-4 validation");
        tri.validate_topology()
            .expect("initial toroidal CDT must satisfy torus topology");
        tri.validate_foliation()
            .expect("initial toroidal CDT must have valid time-slice foliation");
        tri.validate_causality()
            .expect("initial toroidal CDT must only contain adjacent-slice edges");
        tri.validate_cell_classification()
            .expect("initial toroidal CDT must classify every face as an Up or Down CDT cell");
    }

    #[test]
    fn test_from_toroidal_cdt_each_slice_is_closed_s1() {
        let tri = CdtTriangulation::from_toroidal_cdt(6, 4).expect("build toroidal CDT");
        tri.validate_foliation()
            .expect("periodic toroidal CDT must satisfy closed-S¹ per-slice invariant");
    }

    #[test]
    fn test_from_toroidal_cdt_invalid_params() {
        let few_vertices = CdtTriangulation::from_toroidal_cdt(2, 3);
        assert!(matches!(
            few_vertices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if issue == "Insufficient vertices per slice"
                && provided_value == "2"
                && expected_range == "≥ 3"
        ));

        for slices in [1, 2] {
            let few_slices = CdtTriangulation::from_toroidal_cdt(4, slices);
            assert!(matches!(
                few_slices,
                Err(CdtError::InvalidGenerationParameters {
                    ref issue,
                    ref provided_value,
                    ref expected_range,
                }) if issue == "Insufficient number of time slices"
                    && provided_value == &slices.to_string()
                    && expected_range == "≥ 3"
            ));
        }
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
    fn test_periodic_toroidal_count_validation_rejects_face_mismatch() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        let result = validate_toroidal_counts(tri.geometry(), 12, 12, 23, (0.0, 3.0));

        assert!(matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count: 12,
                coordinate_range: (0.0, 3.0),
                attempt: 1,
                ref underlying_error,
            }) if underlying_error.contains("periodic toroidal builder")
                && underlying_error.contains("produced 12 vertices and 24 faces")
                && underlying_error.contains("expected 12 vertices and 23 faces")
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
}
