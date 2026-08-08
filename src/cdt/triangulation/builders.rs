#![forbid(unsafe_code)]

//! Builders for CDT triangulations backed by Delaunay geometry.

use super::CdtTriangulation;
use crate::cdt::foliation::{Foliation, FoliationError};
use crate::config::CdtTopology;
use crate::errors::{
    BackendMutationOperation, CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure,
    DelaunayGenerationFailure, DelaunayGenerationQuantity, DelaunayValidationLevel,
    GenerationParameterIssue,
};
use crate::geometry::DelaunayBackend2D;
use crate::geometry::backends::delaunay::DelaunayVertexHandle;
use crate::geometry::generators::{
    DelaunayTriangulation2D, build_delaunay2_with_data, build_periodic_toroidal_delaunay2,
    generate_delaunay2,
};
use crate::geometry::traits::TriangulationQuery;
use std::num::NonZeroU32;

/// Default pass budget for CDT++-style causality filtering.
const FILTERED_DELAUNAY_MAX_PASSES: u32 = 50;

/// Rewrites toroidal builder failures with CDT-level generation context.
///
/// The lower geometry builder reports failures in terms of its input shape; this
/// helper preserves the underlying diagnostic while normalizing the public error
/// fields to the toroidal CDT constructor's vertex count and retry ordinal.
pub(super) fn remap_toroidal_generation_error(
    error: CdtError,
    total_vertices: u32,
    attempt: u32,
) -> CdtError {
    match error {
        CdtError::DelaunayGenerationFailed {
            coordinate_range,
            failure,
            ..
        } => CdtError::DelaunayGenerationFailed {
            vertex_count: total_vertices,
            coordinate_range,
            attempt,
            failure,
        },
        other => other,
    }
}

/// Validates a generated Delaunay triangulation before wrapping it in CDT state.
fn validated_backend(dt: DelaunayTriangulation2D) -> CdtResult<DelaunayBackend2D> {
    DelaunayBackend2D::from_triangulation(dt).map_err(|err| CdtError::DelaunayValidationFailed {
        level: DelaunayValidationLevel::Five,
        detail: err.to_string(),
    })
}

/// Rewrites Delaunay strip builder failures with CDT-level generation context.
fn remap_strip_generation_error(
    error: CdtError,
    total_vertices: u32,
    coordinate_max: f64,
) -> CdtError {
    match error {
        CdtError::DelaunayGenerationFailed { failure, .. } => CdtError::DelaunayGenerationFailed {
            vertex_count: total_vertices,
            coordinate_range: (0.0, coordinate_max),
            attempt: 1,
            failure,
        },
        other => other,
    }
}

/// Builds a CDT-level generation error for Delaunay strip construction failures.
const fn strip_generation_error(
    total_vertices: u32,
    coordinate_max: f64,
    failure: DelaunayGenerationFailure,
) -> CdtError {
    CdtError::DelaunayGenerationFailed {
        vertex_count: total_vertices,
        coordinate_range: (0.0, coordinate_max),
        attempt: 1,
        failure,
    }
}

/// Verifies that the Delaunay strip builder returned the requested mesh size.
pub(super) fn validate_strip_counts(
    backend: &DelaunayBackend2D,
    total_vertices: u32,
    expected_vertices: usize,
    expected_faces: usize,
    coordinate_max: f64,
) -> CdtResult<()> {
    if backend.vertex_count() != expected_vertices || backend.face_count() != expected_faces {
        return Err(strip_generation_error(
            total_vertices,
            coordinate_max,
            DelaunayGenerationFailure::MeshSizeMismatch {
                actual_vertices: backend.vertex_count(),
                expected_vertices,
                actual_faces: backend.face_count(),
                expected_faces,
            },
        ));
    }

    Ok(())
}

/// Computes the open-strip triangle count used to validate profiled builder output.
///
/// The count is checked before foliation construction so a backend mesh with
/// mismatched topology cannot be paired with the caller's requested profile.
fn open_profile_face_count(profile: &[u32]) -> CdtResult<u32> {
    let (&first_slice, rest) =
        profile
            .split_first()
            .ok_or_else(|| CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::EmptyVolumeProfile,
                provided_value: "[]".to_string(),
                expected_range: "at least one time slice".to_string(),
            })?;
    let last_slice = rest.last().copied().unwrap_or(first_slice);
    let total_vertices = profile.iter().try_fold(0_u32, |total, &vertices| {
        total
            .checked_add(vertices)
            .ok_or_else(|| CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                provided_value: format!("{profile:?}"),
                expected_range: "sum ≤ u32::MAX".to_string(),
            })
    })?;
    let slice_count =
        u32::try_from(profile.len()).map_err(|err| CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::VolumeProfileLengthOverflow,
            provided_value: profile.len().to_string(),
            expected_range: format!("length must fit in u32: {err}"),
        })?;

    total_vertices
        .checked_mul(2)
        .and_then(|faces| faces.checked_sub(first_slice))
        .and_then(|faces| faces.checked_sub(last_slice))
        .and_then(|faces| faces.checked_sub(slice_count.checked_mul(2)?))
        .and_then(|faces| faces.checked_add(2))
        .ok_or_else(|| CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::SimplexCountOverflow,
            provided_value: format!("{profile:?}"),
            expected_range: "open-strip face count must fit in u32".to_string(),
        })
}

/// Verifies that a profiled open-boundary builder returned the requested mesh size.
///
/// This protects [`CdtTriangulation::from_cdt_strip_profile`] from constructing
/// foliation metadata for a backend whose vertex or face topology does not
/// match the requested spatial volume profile.
fn validate_profile_strip_counts(
    backend: &DelaunayBackend2D,
    total_vertices: u32,
    expected_vertices: usize,
    expected_faces: usize,
    coordinate_max: f64,
) -> CdtResult<()> {
    if backend.vertex_count() != expected_vertices || backend.face_count() != expected_faces {
        return Err(strip_generation_error(
            total_vertices,
            coordinate_max,
            DelaunayGenerationFailure::MeshSizeMismatch {
                actual_vertices: backend.vertex_count(),
                expected_vertices,
                actual_faces: backend.face_count(),
                expected_faces,
            },
        ));
    }

    Ok(())
}

/// Builds a CDT-level generation error for periodic toroidal construction failures.
const fn toroidal_generation_error(
    total_vertices: u32,
    coordinate_range: (f64, f64),
    attempt: u32,
    failure: DelaunayGenerationFailure,
) -> CdtError {
    CdtError::DelaunayGenerationFailed {
        vertex_count: total_vertices,
        coordinate_range,
        attempt,
        failure,
    }
}

/// Verifies that the periodic toroidal builder returned the requested mesh size.
pub(super) fn validate_toroidal_counts(
    backend: &DelaunayBackend2D,
    total_vertices: u32,
    expected_vertices: usize,
    expected_faces: usize,
    coordinate_range: (f64, f64),
    attempt: u32,
) -> CdtResult<()> {
    if backend.vertex_count() != expected_vertices || backend.face_count() != expected_faces {
        return Err(toroidal_generation_error(
            total_vertices,
            coordinate_range,
            attempt,
            DelaunayGenerationFailure::MeshSizeMismatch {
                actual_vertices: backend.vertex_count(),
                expected_vertices,
                actual_faces: backend.face_count(),
                expected_faces,
            },
        ));
    }

    Ok(())
}

/// Builds and fully validates one periodic toroidal coordinate candidate.
fn build_validated_toroidal_backend(
    vertex_specs: &[([f64; 2], u32)],
    domain: [f64; 2],
    total_vertices: u32,
    expected_vertices: usize,
    expected_faces: usize,
    coordinate_range: (f64, f64),
    attempt: u32,
) -> CdtResult<DelaunayBackend2D> {
    let dt = build_periodic_toroidal_delaunay2(vertex_specs, domain)
        .map_err(|error| remap_toroidal_generation_error(error, total_vertices, attempt))?;
    let backend = validated_backend(dt)?;
    validate_toroidal_counts(
        &backend,
        total_vertices,
        expected_vertices,
        expected_faces,
        coordinate_range,
        attempt,
    )?;
    Ok(backend)
}

/// Validates an explicit per-slice spatial volume profile.
fn validate_spatial_profile(
    profile: &[u32],
    minimum_slices: u32,
    minimum_vertices_per_slice: u32,
    topology_label: &str,
) -> CdtResult<(u32, u32)> {
    if profile.is_empty() {
        return Err(CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::EmptyVolumeProfile,
            provided_value: "[]".to_string(),
            expected_range: "at least one time slice".to_string(),
        });
    }

    let num_slices =
        u32::try_from(profile.len()).map_err(|err| CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::VolumeProfileLengthOverflow,
            provided_value: profile.len().to_string(),
            expected_range: format!("length must fit in u32: {err}"),
        })?;
    if num_slices < minimum_slices {
        return Err(CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::InsufficientNumberOfTimeSlices,
            provided_value: num_slices.to_string(),
            expected_range: format!("≥ {minimum_slices} for {topology_label}"),
        });
    }

    let mut total_vertices = 0_u32;
    for (slice, &vertices) in profile.iter().enumerate() {
        if vertices < minimum_vertices_per_slice {
            return Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientVerticesInVolumeProfileSlice,
                provided_value: format!("slice {slice} has {vertices}"),
                expected_range: format!(
                    "each slice ≥ {minimum_vertices_per_slice} for {topology_label}"
                ),
            });
        }
        total_vertices = total_vertices.checked_add(vertices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                provided_value: format!("{profile:?}"),
                expected_range: "sum ≤ u32::MAX".to_string(),
            }
        })?;
    }

    Ok((total_vertices, num_slices))
}

/// Converts a spatial volume profile to `usize` slice sizes.
fn profile_slice_sizes(
    profile: &[u32],
    mut generation_failed: impl FnMut(DelaunayGenerationFailure) -> CdtError,
) -> CdtResult<Vec<usize>> {
    profile
        .iter()
        .map(|&volume| {
            generation_count_to_usize(volume, DelaunayGenerationQuantity::SliceVolume)
                .map_err(&mut generation_failed)
        })
        .collect()
}

/// Converts one validated generation count to the platform index type.
fn generation_count_to_usize(
    value: u32,
    quantity: DelaunayGenerationQuantity,
) -> Result<usize, DelaunayGenerationFailure> {
    usize::try_from(value).map_err(|err| DelaunayGenerationFailure::NumericConversion {
        quantity,
        detail: err.to_string(),
    })
}

/// Carries constructor-validated slice counts into foliation construction.
///
/// CDT builders validate topology-specific slice bounds before constructing
/// foliation metadata, so reaching this helper with zero would indicate an
/// internal constructor invariant regression.
const fn checked_nonzero_slice_count(num_slices: u32) -> NonZeroU32 {
    NonZeroU32::new(num_slices).expect("validated CDT slice count should be nonzero")
}

/// Maximum number of deterministic generic-position embeddings tried per torus.
const TOROIDAL_EMBEDDING_ATTEMPTS: u8 = 14;

/// Returns the first valid deterministic toroidal candidate.
///
/// Candidate identifiers select coordinate perturbations, while the public attempt number is
/// the one-based ordinal of the candidate actually tried. On exhaustion, the final failure is
/// returned with that ordinal intact.
fn first_valid_toroidal_candidate<T>(
    candidates: impl IntoIterator<Item = u8>,
    mut build: impl FnMut(u8, u32) -> CdtResult<T>,
    no_candidates: impl FnOnce() -> CdtError,
) -> CdtResult<T> {
    let mut attempt = 0_u32;
    let mut last_error = None;
    for candidate in candidates {
        attempt = attempt.saturating_add(1);
        match build(candidate, attempt) {
            Ok(value) => return Ok(value),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(no_candidates))
}

/// Returns a small periodic offset that puts a toroidal lattice in generic position.
///
/// Exact rectangular lattices have cocircular point sets. Delaunay 0.8 rejects
/// some quotient selections from that degenerate input during exhaustive
/// realization validation. Uniform constructors first try a deterministic
/// hash perturbation that retains the inverse-move sites used by the regular
/// CDT seed, then fall back to a smooth slice shear and a bounded sequence of
/// combined perturbations. Profile constructors use the latter embeddings because
/// their unequal slice spacing does not have the regular seed's inverse-site
/// contract. Every candidate preserves slice labels and remains far below the
/// unit lattice spacing.
fn toroidal_vertex_offset(slice: u32, index: u32, num_slices: u32, candidate: u8) -> [f64; 2] {
    let phase = std::f64::consts::TAU * f64::from(slice) / f64::from(num_slices);
    if candidate == 0 {
        let x_hash = (17 * u64::from(index) + 34 * u64::from(slice) + 35) % 97;
        let y_hash = (48 * u64::from(index) + 11 * u64::from(slice) + 15) % 89;
        let centered_x = f64::from(u32::try_from(x_hash).unwrap_or(0)) / 97.0 - 0.5;
        let centered_y = f64::from(u32::try_from(y_hash).unwrap_or(0)) / 89.0 - 0.5;
        return [centered_x / 16.0, centered_y / 16.0];
    }
    if candidate == 1 {
        return [phase.sin() / 32.0, 0.0];
    }

    let seed = u64::from(candidate - 1);
    let x_hash = (17 * u64::from(index) + (29 + seed) * u64::from(slice) + 7 * seed) % 97;
    let y_hash = ((43 + seed) * u64::from(index) + 11 * u64::from(slice) + 3 * seed) % 89;
    let centered_x = f64::from(u32::try_from(x_hash).unwrap_or(0)) / 97.0 - 0.5;
    let centered_y = f64::from(u32::try_from(y_hash).unwrap_or(0)) / 89.0 - 0.5;
    let amplitude = f64::from(u32::from(candidate % 3) + 1) / 32.0;
    [
        phase.sin() / 16.0 + centered_x * amplitude,
        centered_y * amplitude,
    ]
}

/// Builds labeled periodic coordinates for a toroidal CDT profile.
fn toroidal_profile_vertices(
    profile: &[u32],
    total_vertices: u32,
    num_slices: u32,
    candidate: u8,
    attempt: u32,
) -> CdtResult<Vec<([f64; 2], u32)>> {
    let expected_vertices = usize::try_from(total_vertices).map_err(|err| {
        toroidal_generation_error(
            total_vertices,
            (0.0, 0.0),
            attempt,
            DelaunayGenerationFailure::NumericConversion {
                quantity: DelaunayGenerationQuantity::TotalVertices,
                detail: err.to_string(),
            },
        )
    })?;
    let max_slice_volume = profile.iter().copied().max().unwrap_or(1);
    let domain_x = f64::from(max_slice_volume);
    let mut vertex_specs = Vec::new();
    vertex_specs
        .try_reserve_exact(expected_vertices)
        .map_err(|err| {
            toroidal_generation_error(
                total_vertices,
                (0.0, domain_x),
                attempt,
                DelaunayGenerationFailure::StorageReservation {
                    requested_capacity: expected_vertices,
                    detail: err.to_string(),
                },
            )
        })?;

    for (slice, &vertices) in profile.iter().enumerate() {
        let label = u32::try_from(slice).map_err(|err| {
            toroidal_generation_error(
                total_vertices,
                (0.0, domain_x),
                attempt,
                DelaunayGenerationFailure::NumericConversion {
                    quantity: DelaunayGenerationQuantity::SliceIndex,
                    detail: err.to_string(),
                },
            )
        })?;
        let spacing = domain_x / f64::from(vertices);
        for index in 0..vertices {
            let [x_offset, y_offset] = toroidal_vertex_offset(label, index, num_slices, candidate);
            let x = f64::from(index)
                .mul_add(spacing, x_offset)
                .rem_euclid(domain_x);
            let y = (f64::from(label) + y_offset).rem_euclid(f64::from(num_slices));
            vertex_specs.push(([x, y], label));
        }
    }

    Ok(vertex_specs)
}

/// Computes one labeled open-boundary strip coordinate.
///
/// Both regular and profiled open-strip constructors use this helper so the
/// same boundary side-arc, interior perturbation, and vertical jitter rules feed
/// Delaunay generation. Keeping those coordinates centralized preserves the
/// public constructor contract that initial open-boundary slices validate as
/// ordered intervals before any Metropolis moves run.
fn open_strip_vertex_spec(
    slice: u32,
    index: u32,
    vertices: u32,
    profile_len: u32,
    side_jitter: f64,
    interior_jitter: f64,
    vertical_jitter: f64,
) -> ([f64; 2], u32) {
    let spacing = 1.0_f64 / f64::from(vertices - 1);
    let temporal_index = f64::from(slice);
    let temporal_span = f64::from(profile_len - 1);
    let side_arc = if temporal_span.abs() < f64::EPSILON {
        0.0
    } else {
        side_jitter * temporal_index * (temporal_span - temporal_index) / temporal_span.powi(2)
    };
    let x = if index == 0 || index == vertices - 1 {
        let boundary = f64::from(index).mul_add(spacing, side_jitter);
        if index == 0 {
            boundary - side_arc
        } else {
            boundary + side_arc
        }
    } else {
        let sign = if (index + slice).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        f64::from(index).mul_add(spacing, side_jitter) + sign * interior_jitter
    };
    let spatial_index = f64::from(index);
    let arc = vertical_jitter * spatial_index * f64::from(vertices - 1 - index)
        / f64::from((vertices - 1).pow(2));
    let base_y = f64::from(slice);
    let y = if slice == 0 {
        base_y - arc
    } else if slice + 1 == profile_len {
        base_y + arc
    } else {
        let sign = if (index + slice).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        (sign * arc).mul_add(0.5, base_y)
    };
    ([x, y], slice)
}

/// Builds labeled open-boundary coordinates for a CDT strip profile.
fn open_profile_vertices(profile: &[u32], total_vertices: u32) -> CdtResult<Vec<([f64; 2], u32)>> {
    let expected_vertices = usize::try_from(total_vertices).map_err(|err| {
        strip_generation_error(
            total_vertices,
            f64::from(total_vertices),
            DelaunayGenerationFailure::NumericConversion {
                quantity: DelaunayGenerationQuantity::TotalVertices,
                detail: err.to_string(),
            },
        )
    })?;
    let profile_len = u32::try_from(profile.len()).map_err(|err| {
        strip_generation_error(
            total_vertices,
            f64::from(total_vertices),
            DelaunayGenerationFailure::NumericConversion {
                quantity: DelaunayGenerationQuantity::TimeSlices,
                detail: err.to_string(),
            },
        )
    })?;
    let max_slice_volume = profile.iter().copied().max().unwrap_or(2);
    let min_spacing = 1.0_f64 / f64::from(max_slice_volume - 1);
    let side_jitter = min_spacing / 4.0;
    let interior_jitter = min_spacing / (16.0 * f64::from(profile_len));
    // TODO(acgetchell/delaunay#447): remove once exact collinear CDT boundaries build.
    let vertical_jitter = 1.0e-9;
    let coordinate_max = f64::from(profile_len).max(2.0);
    let mut vertex_specs = Vec::new();
    vertex_specs
        .try_reserve_exact(expected_vertices)
        .map_err(|err| {
            strip_generation_error(
                total_vertices,
                coordinate_max,
                DelaunayGenerationFailure::StorageReservation {
                    requested_capacity: expected_vertices,
                    detail: err.to_string(),
                },
            )
        })?;

    for (slice, &vertices) in profile.iter().enumerate() {
        let label = u32::try_from(slice).map_err(|err| {
            strip_generation_error(
                total_vertices,
                coordinate_max,
                DelaunayGenerationFailure::NumericConversion {
                    quantity: DelaunayGenerationQuantity::SliceIndex,
                    detail: err.to_string(),
                },
            )
        })?;
        for index in 0..vertices {
            vertex_specs.push(open_strip_vertex_spec(
                label,
                index,
                vertices,
                profile_len,
                side_jitter,
                interior_jitter,
                vertical_jitter,
            ));
        }
    }

    Ok(vertex_specs)
}

impl CdtTriangulation<DelaunayBackend2D> {
    /// Rebuilds open-boundary foliation bookkeeping from live backend labels.
    ///
    /// The filtered constructor mutates the backend between passes, so it cannot
    /// reuse stored slice sizes. Rebuilding from vertex payloads preserves the
    /// public guarantee that a zero strict-causal violation count was computed
    /// from the current geometry rather than stale bookkeeping.
    ///
    /// # Errors
    ///
    /// Returns [`FoliationError`] when live labels are missing, out of range, or
    /// cannot form valid open-boundary foliation bookkeeping.
    fn rebuild_open_foliation_from_live_labels(&mut self) -> CdtResult<()> {
        let slice_sizes = Self::live_slice_sizes_from_vertex_labels(
            &self.geometry,
            self.metadata.time_slices.get(),
        )?;
        let foliation = Foliation::from_slice_sizes(slice_sizes, self.metadata.time_slices)
            .map_err(CdtError::from)?;

        self.foliation = Some(foliation);
        self.mark_foliation_synchronized();
        Ok(())
    }

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
    /// assign time labels or CDT simplex classifications, so production CDT
    /// simulations should prefer [`CdtTriangulation::from_cdt_strip`] or
    /// [`CdtTriangulation::from_toroidal_cdt`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::UnsupportedDimension`] if `dimension != 2`.
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices < 3`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if random point generation
    /// or Delaunay construction fails, [`CdtError::VertexBuildFailed`] if an
    /// upstream vertex cannot be built, or [`CdtError::DelaunayValidationFailed`]
    /// if the generated backend fails Level 1-5 validation. Propagates metadata
    /// validation errors from [`CdtTriangulation::try_new`], including
    /// `time_slices == 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_random_points(5, 2, 2)?;
    ///     assert_eq!(tri.time_slices().get(), 2);
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
                issue: GenerationParameterIssue::InsufficientVertexCount,
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = generate_delaunay2(vertices, (0.0, 10.0), None)?;
        let backend = validated_backend(dt)?;

        Self::try_new(backend, time_slices, dimension)
    }

    /// Creates an unfoliated triangulation with a Delaunay backend from a fixed random seed.
    ///
    /// Use this builder for raw geometry examples, tests, and benchmarks that
    /// need deterministic input geometry. It does not assign time labels or
    /// CDT simplex classifications, so production CDT simulations should prefer
    /// [`CdtTriangulation::from_cdt_strip`] or
    /// [`CdtTriangulation::from_toroidal_cdt`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::UnsupportedDimension`] if `dimension != 2`.
    /// Returns [`CdtError::InvalidGenerationParameters`] if `vertices < 3`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if seeded point
    /// generation or Delaunay construction fails, [`CdtError::VertexBuildFailed`]
    /// if an upstream vertex cannot be built, or
    /// [`CdtError::DelaunayValidationFailed`] if the generated backend fails
    /// Level 1-5 validation. Propagates metadata validation errors from
    /// [`CdtTriangulation::try_new`], including `time_slices == 0`.
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
                issue: GenerationParameterIssue::InsufficientVertexCount,
                provided_value: vertices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let dt = generate_delaunay2(vertices, (0.0, 10.0), Some(seed))?;
        let backend = validated_backend(dt)?;

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
    /// upstream Level 1-5 Delaunay validator. Returns topology, foliation,
    /// causality, or classification errors if the labels do not form a strict
    /// CDT mesh.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
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
        let mut tri = Self::try_new(backend, time_slices, dimension)?;
        tri.foliation = Some(
            Foliation::from_slice_sizes(slice_sizes, checked_nonzero_slice_count(time_slices))
                .map_err(CdtError::from)?,
        );
        tri.mark_foliation_synchronized();
        tri.validate_initial_delaunay_cdt()?;
        Ok(tri)
    }

    /// Selects a removable vertex from the first non-strict CDT simplex.
    ///
    /// The selector prefers extreme time labels on acausal faces and never
    /// removes the last vertex above the caller's per-slice minimum. It is the
    /// local policy behind [`Self::from_filtered_delaunay_strip`]'s convergence
    /// loop, not a general move proposal for simulation.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] if the offending face cannot be
    /// inspected or has missing vertex labels. Returns
    /// [`CdtError::DelaunayGenerationFailed`] if every candidate slice is already
    /// at the caller's per-slice minimum.
    fn invalid_simplex_removal_candidate(
        &self,
        minimum_slice_size: usize,
        total_vertices: u32,
        coordinate_max: f64,
    ) -> CdtResult<Option<DelaunayVertexHandle>> {
        let slice_sizes = self.slice_sizes();

        for face in self.geometry.faces() {
            if self.simplex_type(&face)?.is_some() {
                continue;
            }

            let vertices =
                self.geometry
                    .face_vertices(&face)
                    .map_err(|err| CdtError::ValidationFailed {
                        check: CdtValidationCheck::SimplexClassification,
                        failure: CdtValidationFailure::FaceVerticesUnavailable {
                            face: format!("{:?}", face.simplex_key()),
                            detail: err.to_string(),
                        },
                    })?;
            let mut labeled_vertices = Vec::with_capacity(vertices.len());
            for vertex in vertices {
                let label = self
                    .geometry
                    .vertex_data_by_key(vertex.vertex_key())
                    .ok_or_else(|| CdtError::ValidationFailed {
                        check: CdtValidationCheck::SimplexClassification,
                        failure: CdtValidationFailure::MissingVertexTimeLabel {
                            vertex: format!("{:?}", vertex.vertex_key()),
                        },
                    })?;
                labeled_vertices.push((vertex, label));
            }

            let Some(min_label) = labeled_vertices.iter().map(|(_, label)| *label).min() else {
                return Ok(None);
            };
            let Some(max_label) = labeled_vertices.iter().map(|(_, label)| *label).max() else {
                return Ok(None);
            };
            let mut candidates: Vec<_> = if max_label.saturating_sub(min_label) > 1 {
                labeled_vertices
                    .iter()
                    .filter(|(_, label)| *label == min_label || *label == max_label)
                    .collect()
            } else {
                labeled_vertices.iter().collect()
            };
            candidates.sort_by_key(|(_, label)| {
                let count = labeled_vertices
                    .iter()
                    .filter(|(_, other)| *other == *label)
                    .count();
                (count, *label)
            });

            for (vertex, label) in candidates {
                let Some(&slice_size) = slice_sizes.get(*label as usize) else {
                    continue;
                };
                if slice_size > minimum_slice_size {
                    return Ok(Some((*vertex).clone()));
                }
            }

            return Err(CdtError::DelaunayGenerationFailed {
                vertex_count: total_vertices,
                coordinate_range: (0.0, coordinate_max),
                attempt: 1,
                failure: DelaunayGenerationFailure::MinimumSliceSizeReached {
                    face: format!("{:?}", face.simplex_key()),
                    minimum_slice_size,
                },
            });
        }

        Ok(None)
    }

    /// Removes vertices incident to non-strict CDT simplices until the violation count is zero.
    ///
    /// This is the bounded CDT-plusplus-style filtering loop used by
    /// [`Self::from_filtered_delaunay_strip`]. Each pass rebuilds foliation
    /// bookkeeping from live labels, counts non-strict simplices through
    /// [`Self::strict_causal_simplex_violation_count`], removes one offending
    /// vertex, and lets the Delaunay backend retriangulate the cavity.
    ///
    /// # Errors
    ///
    /// Returns foliation or validation errors from rebuilding and recounting.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if the pass budget is
    /// exhausted or no removable offending vertex exists. Returns
    /// [`CdtError::BackendMutationFailed`] if backend vertex removal fails.
    fn filter_invalid_open_delaunay_simplices(
        &mut self,
        minimum_vertices_per_slice: usize,
        max_passes: u32,
        total_vertices: u32,
        coordinate_max: f64,
    ) -> CdtResult<()> {
        for pass in 0..=max_passes {
            self.rebuild_open_foliation_from_live_labels()?;
            let violations = self.strict_causal_simplex_violation_count()?;
            if violations == 0 {
                return Ok(());
            }
            if pass == max_passes {
                return Err(CdtError::DelaunayGenerationFailed {
                    vertex_count: total_vertices,
                    coordinate_range: (0.0, coordinate_max),
                    attempt: pass + 1,
                    failure: DelaunayGenerationFailure::FilterPassBudgetExhausted {
                        max_passes,
                        remaining_violations: violations,
                    },
                });
            }

            let Some(vertex) = self.invalid_simplex_removal_candidate(
                minimum_vertices_per_slice,
                total_vertices,
                coordinate_max,
            )?
            else {
                return Err(CdtError::DelaunayGenerationFailed {
                    vertex_count: total_vertices,
                    coordinate_range: (0.0, coordinate_max),
                    attempt: pass + 1,
                    failure: DelaunayGenerationFailure::NoRemovableFilterVertex {
                        remaining_violations: violations,
                    },
                });
            };

            let target = format!("vertex {:?}", vertex.vertex_key());
            self.remove_vertex(vertex)
                .map_err(|err| CdtError::BackendMutationFailed {
                    operation: BackendMutationOperation::RemoveVertex,
                    target,
                    detail: err.to_string(),
                })?;
        }

        Ok(())
    }

    /// Construct a Delaunay-backed 1+1 CDT strip by filtering surplus labeled points.
    ///
    /// Starts from a valid Delaunay triangulation containing a regular
    /// open-boundary CDT strip plus deterministic surplus vertices whose
    /// coordinates are intentionally placed near the opposite temporal boundary.
    /// The constructor repeatedly counts non-strict causal simplices through
    /// [`Self::strict_causal_simplex_violation_count`], removes a vertex incident
    /// to one such simplex through
    /// [`TriangulationMut::remove_vertex`](crate::geometry::traits::TriangulationMut::remove_vertex),
    /// lets the Delaunay backend retriangulate each cavity, and returns only
    /// after the violation count reaches zero and the full initial CDT validation
    /// contract passes.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidGenerationParameters`] under the same
    /// parameter bounds as [`Self::from_cdt_strip`]. Returns
    /// [`CdtError::DelaunayGenerationFailed`] if the overcomplete Delaunay
    /// construction fails, filtering cannot make progress without violating the
    /// requested per-slice minimum, or the 50-pass filtering budget is exhausted.
    /// Returns [`CdtError::VertexBuildFailed`] if an upstream vertex cannot be
    /// built, or [`CdtError::DelaunayValidationFailed`] if the constructed
    /// backend fails Level 1-5 validation.
    /// Returns [`CdtError::BackendMutationFailed`] if backend vertex removal
    /// fails. Returns validation errors if the filtered triangulation does not
    /// satisfy CDT topology, foliation, causality, and simplex-classification
    /// invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_filtered_delaunay_strip(4, 3)?;
    ///     assert_eq!(tri.slice_sizes(), &[4, 4, 4]);
    ///     assert_eq!(tri.strict_causal_simplex_violation_count()?, 0);
    ///     tri.validate_simplex_classification()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn from_filtered_delaunay_strip(
        vertices_per_slice: u32,
        num_slices: u32,
    ) -> CdtResult<Self> {
        if vertices_per_slice < 4 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientVerticesPerSlice,
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 4".to_string(),
            });
        }
        if num_slices < 2 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientNumberOfTimeSlices,
                provided_value: num_slices.to_string(),
                expected_range: "≥ 2".to_string(),
            });
        }

        let core_vertices = vertices_per_slice.checked_mul(num_slices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;
        let surplus_vertices = if num_slices > 2 { 2 } else { 0 };
        let total_vertices = core_vertices.checked_add(surplus_vertices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                provided_value: format!("{core_vertices} + {surplus_vertices}"),
                expected_range: "sum ≤ u32::MAX".to_string(),
            }
        })?;

        let coordinate_max = f64::from(num_slices).max(2.0);
        let mut vertex_specs = open_profile_vertices(
            &vec![vertices_per_slice; num_slices as usize],
            core_vertices,
        )?;
        vertex_specs
            .try_reserve_exact(surplus_vertices as usize)
            .map_err(|err| {
                strip_generation_error(
                    total_vertices,
                    coordinate_max,
                    DelaunayGenerationFailure::StorageReservation {
                        requested_capacity: surplus_vertices as usize,
                        detail: err.to_string(),
                    },
                )
            })?;

        if num_slices > 2 {
            let midpoint = 0.5;
            vertex_specs.push(([midpoint, f64::from(num_slices - 1) - 0.1], 0));
            vertex_specs.push(([midpoint, 0.1], num_slices - 1));
        }

        let dt = build_delaunay2_with_data(&vertex_specs)
            .map_err(|err| remap_strip_generation_error(err, total_vertices, coordinate_max))?;
        let backend = validated_backend(dt)?;
        let mut tri = Self::try_new(backend, num_slices, 2)?;
        let minimum_vertices_per_slice = usize::try_from(vertices_per_slice).map_err(|err| {
            strip_generation_error(
                total_vertices,
                coordinate_max,
                DelaunayGenerationFailure::NumericConversion {
                    quantity: DelaunayGenerationQuantity::VerticesPerSlice,
                    detail: err.to_string(),
                },
            )
        })?;
        tri.filter_invalid_open_delaunay_simplices(
            minimum_vertices_per_slice,
            FILTERED_DELAUNAY_MAX_PASSES,
            total_vertices,
            coordinate_max,
        )?;
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
    /// `num_slices < 2`, or the derived vertex or simplex count overflows `u32`.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if constructor storage cannot
    /// be reserved, if the underlying Delaunay builder rejects the points, if
    /// [`build_delaunay2_with_data`] returns a vertex or face count that does not
    /// match the requested strip. Returns [`CdtError::VertexBuildFailed`] if an
    /// upstream vertex cannot be built, or [`CdtError::DelaunayValidationFailed`]
    /// if the constructed backend does not satisfy the Level 1-5 Delaunay
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
    ///     assert!(tri.validate_simplex_classification().is_ok());
    ///     Ok(())
    /// }
    /// ```
    pub fn from_cdt_strip(vertices_per_slice: u32, num_slices: u32) -> CdtResult<Self> {
        if vertices_per_slice < 4 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientVerticesPerSlice,
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 4".to_string(),
            });
        }
        if num_slices < 2 {
            return Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientNumberOfTimeSlices,
                provided_value: num_slices.to_string(),
                expected_range: "≥ 2".to_string(),
            });
        }

        let total_vertices = vertices_per_slice.checked_mul(num_slices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;

        let spatial_quads = vertices_per_slice - 1;
        let temporal_quads = num_slices - 1;
        let total_quads = spatial_quads.checked_mul(temporal_quads).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::SimplexCountOverflow,
                provided_value: format!("{spatial_quads} × {temporal_quads}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;
        let total_simplices =
            total_quads
                .checked_mul(2)
                .ok_or_else(|| CdtError::InvalidGenerationParameters {
                    issue: GenerationParameterIssue::SimplexCountOverflow,
                    provided_value: format!("2 × {total_quads}"),
                    expected_range: "product ≤ u32::MAX".to_string(),
                })?;

        let coordinate_max = f64::from(num_slices).max(2.0);
        let generation_failed = |failure: DelaunayGenerationFailure| {
            strip_generation_error(total_vertices, coordinate_max, failure)
        };

        let expected_vertices =
            generation_count_to_usize(total_vertices, DelaunayGenerationQuantity::TotalVertices)
                .map_err(generation_failed)?;
        let expected_faces =
            generation_count_to_usize(total_simplices, DelaunayGenerationQuantity::TotalFaces)
                .map_err(generation_failed)?;
        let n = generation_count_to_usize(
            vertices_per_slice,
            DelaunayGenerationQuantity::VerticesPerSlice,
        )
        .map_err(generation_failed)?;
        let t_count = generation_count_to_usize(num_slices, DelaunayGenerationQuantity::TimeSlices)
            .map_err(generation_failed)?;

        let min_spacing = 1.0_f64 / f64::from(vertices_per_slice - 1);
        let side_jitter = min_spacing / 4.0;
        let interior_jitter = min_spacing / (16.0 * f64::from(num_slices));
        // TODO(acgetchell/delaunay#447): remove once exact collinear CDT boundaries build.
        let vertical_jitter = 1.0e-9;
        let mut vertex_specs: Vec<([f64; 2], u32)> = Vec::new();
        vertex_specs
            .try_reserve_exact(expected_vertices)
            .map_err(|err| {
                generation_failed(DelaunayGenerationFailure::StorageReservation {
                    requested_capacity: expected_vertices,
                    detail: err.to_string(),
                })
            })?;
        for t in 0..num_slices {
            for i in 0..vertices_per_slice {
                vertex_specs.push(open_strip_vertex_spec(
                    t,
                    i,
                    vertices_per_slice,
                    num_slices,
                    side_jitter,
                    interior_jitter,
                    vertical_jitter,
                ));
            }
        }

        let dt = build_delaunay2_with_data(&vertex_specs)
            .map_err(|err| remap_strip_generation_error(err, total_vertices, coordinate_max))?;

        let backend = validated_backend(dt)?;
        validate_strip_counts(
            &backend,
            total_vertices,
            expected_vertices,
            expected_faces,
            coordinate_max,
        )?;
        let slice_sizes = vec![n; t_count];
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, checked_nonzero_slice_count(num_slices))
                .map_err(CdtError::from)?;

        let mut tri = Self::try_new(backend, num_slices, 2)?;
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();
        tri.validate_initial_delaunay_cdt()?;

        Ok(tri)
    }

    /// Construct a Delaunay-backed open-boundary 1+1 CDT strip from a spatial volume profile.
    ///
    /// Each entry in `volume_profile` is the number of vertices on that time
    /// slice. Unlike [`Self::from_cdt_strip`], adjacent slices may have different
    /// spatial volumes; this represents a general nonuniform CDT initial
    /// geometry rather than a regular fixture.
    /// The triangulation itself is delegated to
    /// [`crate::geometry::generators::build_delaunay2_with_data`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidGenerationParameters`] if the profile has fewer
    /// than two slices, any slice has fewer than four vertices, or derived counts
    /// overflow.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if coordinate storage cannot
    /// be reserved, if the Delaunay constructor rejects the profiled point data,
    /// or if the generated backend vertex or face count does not match the
    /// requested profile. Returns [`CdtError::VertexBuildFailed`] if an upstream
    /// vertex cannot be built.
    /// Returns [`CdtError::DelaunayValidationFailed`] if the constructed backend
    /// fails the Level 1-5 Delaunay validator. Returns [`CdtError::TopologyMismatch`],
    /// [`CdtError::Foliation`], [`CdtError::CausalityViolation`], or
    /// [`CdtError::ValidationFailed`] if the constructed mesh violates CDT
    /// invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip_profile(&[4, 6, 5])?;
    ///     assert_eq!(tri.slice_sizes(), &[4, 6, 5]);
    ///     assert!(tri.validate_simplex_classification().is_ok());
    ///     Ok(())
    /// }
    /// ```
    pub fn from_cdt_strip_profile(volume_profile: &[u32]) -> CdtResult<Self> {
        let (total_vertices, num_slices) =
            validate_spatial_profile(volume_profile, 2, 4, "open-boundary topology")?;
        let coordinate_max = f64::from(num_slices);
        let expected_vertices = usize::try_from(total_vertices).map_err(|err| {
            strip_generation_error(
                total_vertices,
                coordinate_max,
                DelaunayGenerationFailure::NumericConversion {
                    quantity: DelaunayGenerationQuantity::TotalVertices,
                    detail: err.to_string(),
                },
            )
        })?;
        let expected_faces =
            usize::try_from(open_profile_face_count(volume_profile)?).map_err(|err| {
                strip_generation_error(
                    total_vertices,
                    coordinate_max,
                    DelaunayGenerationFailure::NumericConversion {
                        quantity: DelaunayGenerationQuantity::TotalFaces,
                        detail: err.to_string(),
                    },
                )
            })?;
        let vertex_specs = open_profile_vertices(volume_profile, total_vertices)?;
        let dt = build_delaunay2_with_data(&vertex_specs)
            .map_err(|error| remap_strip_generation_error(error, total_vertices, coordinate_max))?;
        let backend = validated_backend(dt)?;
        validate_profile_strip_counts(
            &backend,
            total_vertices,
            expected_vertices,
            expected_faces,
            coordinate_max,
        )?;
        let slice_sizes = profile_slice_sizes(volume_profile, |err| {
            strip_generation_error(total_vertices, coordinate_max, err)
        })?;

        let foliation =
            Foliation::from_slice_sizes(slice_sizes, checked_nonzero_slice_count(num_slices))
                .map_err(CdtError::from)?;
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
    /// requires full Delaunay Level 1-5 validation before the CDT wrapper is
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
    /// match the requested toroidal CDT. Returns [`CdtError::VertexBuildFailed`]
    /// if an upstream vertex cannot be built. Returns
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
                issue: GenerationParameterIssue::InsufficientVerticesPerSlice,
                provided_value: vertices_per_slice.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }
        if num_slices < 3 {
            // With T=2 the wrap-around makes every pair of adjacent slices
            // identify (t-1, t) with (t, t+1), so each spatial edge would be
            // shared by 4 triangles instead of 2 — a non-manifold mesh.
            return Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientNumberOfTimeSlices,
                provided_value: num_slices.to_string(),
                expected_range: "≥ 3".to_string(),
            });
        }

        let total_vertices = vertices_per_slice.checked_mul(num_slices).ok_or_else(|| {
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                provided_value: format!("{vertices_per_slice} × {num_slices}"),
                expected_range: "product ≤ u32::MAX".to_string(),
            }
        })?;
        let total_simplices =
            total_vertices
                .checked_mul(2)
                .ok_or_else(|| CdtError::InvalidGenerationParameters {
                    issue: GenerationParameterIssue::SimplexCountOverflow,
                    provided_value: format!("2 × {total_vertices}"),
                    expected_range: "product ≤ u32::MAX".to_string(),
                })?;

        let generation_failed = |attempt: u32, failure: DelaunayGenerationFailure| {
            let coordinate_max = f64::from(vertices_per_slice.max(num_slices) - 1);
            toroidal_generation_error(total_vertices, (0.0, coordinate_max), attempt, failure)
        };

        let expected_vertices =
            generation_count_to_usize(total_vertices, DelaunayGenerationQuantity::TotalVertices)
                .map_err(|failure| generation_failed(1, failure))?;
        let expected_faces =
            generation_count_to_usize(total_simplices, DelaunayGenerationQuantity::TotalFaces)
                .map_err(|failure| generation_failed(1, failure))?;
        let n = generation_count_to_usize(
            vertices_per_slice,
            DelaunayGenerationQuantity::VerticesPerSlice,
        )
        .map_err(|failure| generation_failed(1, failure))?;
        let t_count = generation_count_to_usize(num_slices, DelaunayGenerationQuantity::TimeSlices)
            .map_err(|failure| generation_failed(1, failure))?;

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
                generation_failed(
                    1,
                    DelaunayGenerationFailure::StorageReservation {
                        requested_capacity: expected_vertices,
                        detail: err.to_string(),
                    },
                )
            })?;
        let domain = [n_f, t_f];
        let coordinate_range = (0.0, n_f.max(t_f) - 1.0);
        let backend = first_valid_toroidal_candidate(
            0..TOROIDAL_EMBEDDING_ATTEMPTS,
            |candidate, attempt| {
                vertex_specs.clear();
                for t in 0..num_slices {
                    for i in 0..vertices_per_slice {
                        let [x_offset, y_offset] =
                            toroidal_vertex_offset(t, i, num_slices, candidate);
                        let x = (f64::from(i) + x_offset).rem_euclid(n_f);
                        let y = (f64::from(t) + y_offset).rem_euclid(t_f);
                        vertex_specs.push(([x, y], t));
                    }
                }

                build_validated_toroidal_backend(
                    &vertex_specs,
                    domain,
                    total_vertices,
                    expected_vertices,
                    expected_faces,
                    coordinate_range,
                    attempt,
                )
            },
            || generation_failed(0, DelaunayGenerationFailure::NoEmbeddingCandidates),
        )?;

        let slice_sizes = vec![n; t_count];
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, checked_nonzero_slice_count(num_slices))
                .map_err(CdtError::from)?;

        let mut tri = Self::with_topology(backend, num_slices, 2, CdtTopology::Toroidal)?;
        tri.foliation = Some(foliation);
        tri.mark_foliation_synchronized();
        tri.validate_initial_delaunay_cdt()?;

        Ok(tri)
    }

    /// Construct a periodic 1+1 CDT torus from a spatial volume profile.
    ///
    /// Each profile entry gives the number of vertices on one closed S¹ spatial
    /// slice. Time wraps periodically, so the final slice is adjacent to slice
    /// zero. Unlike [`Self::from_toroidal_cdt`], adjacent slices may have
    /// different spatial volumes.
    /// The triangulation itself is delegated to
    /// [`crate::geometry::generators::build_periodic_toroidal_delaunay2`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidGenerationParameters`] if the profile has fewer
    /// than three slices, any slice has fewer than three vertices, or derived
    /// counts overflow.
    /// Returns [`CdtError::DelaunayGenerationFailed`] if coordinate storage cannot
    /// be reserved, if the periodic Delaunay constructor rejects the profiled point
    /// data, or if the resulting vertex or face counts do not match the requested
    /// profile. Returns [`CdtError::VertexBuildFailed`] if an upstream vertex
    /// cannot be built. Returns [`CdtError::DelaunayValidationFailed`] if the
    /// constructed backend fails the Level 1-5 Delaunay validator. Returns
    /// [`CdtError::TopologyMismatch`], [`CdtError::Foliation`],
    /// [`CdtError::CausalityViolation`], or [`CdtError::ValidationFailed`] if the
    /// constructed mesh violates CDT invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_toroidal_cdt_profile(&[3, 4, 5, 4])?;
    ///     assert_eq!(tri.slice_sizes(), &[3, 4, 5, 4]);
    ///     assert_eq!(tri.time_slices().get(), 4);
    ///     Ok(())
    /// }
    /// ```
    pub fn from_toroidal_cdt_profile(volume_profile: &[u32]) -> CdtResult<Self> {
        let (total_vertices, num_slices) =
            validate_spatial_profile(volume_profile, 3, 3, "toroidal topology")?;
        let total_simplices =
            total_vertices
                .checked_mul(2)
                .ok_or_else(|| CdtError::InvalidGenerationParameters {
                    issue: GenerationParameterIssue::SimplexCountOverflow,
                    provided_value: format!("2 × {total_vertices}"),
                    expected_range: "product ≤ u32::MAX".to_string(),
                })?;
        let expected_vertices = usize::try_from(total_vertices).map_err(|err| {
            toroidal_generation_error(
                total_vertices,
                (0.0, 0.0),
                1,
                DelaunayGenerationFailure::NumericConversion {
                    quantity: DelaunayGenerationQuantity::TotalVertices,
                    detail: err.to_string(),
                },
            )
        })?;
        let expected_faces = usize::try_from(total_simplices).map_err(|err| {
            toroidal_generation_error(
                total_vertices,
                (0.0, 0.0),
                1,
                DelaunayGenerationFailure::NumericConversion {
                    quantity: DelaunayGenerationQuantity::TotalFaces,
                    detail: err.to_string(),
                },
            )
        })?;
        let max_slice_volume = volume_profile.iter().copied().max().unwrap_or(1);
        let domain = [f64::from(max_slice_volume), f64::from(num_slices)];
        let coordinate_range = (0.0, domain[0].max(domain[1]) - 1.0);
        let generation_failed = |attempt: u32, failure: DelaunayGenerationFailure| {
            toroidal_generation_error(total_vertices, coordinate_range, attempt, failure)
        };

        let backend = first_valid_toroidal_candidate(
            1..TOROIDAL_EMBEDDING_ATTEMPTS,
            |candidate, attempt| {
                let vertex_specs = toroidal_profile_vertices(
                    volume_profile,
                    total_vertices,
                    num_slices,
                    candidate,
                    attempt,
                )?;
                build_validated_toroidal_backend(
                    &vertex_specs,
                    domain,
                    total_vertices,
                    expected_vertices,
                    expected_faces,
                    coordinate_range,
                    attempt,
                )
            },
            || generation_failed(0, DelaunayGenerationFailure::NoEmbeddingCandidates),
        )?;

        let slice_sizes = profile_slice_sizes(volume_profile, |error| generation_failed(1, error))?;
        let foliation =
            Foliation::from_slice_sizes(slice_sizes, checked_nonzero_slice_count(num_slices))
                .map_err(CdtError::from)?;

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
    use crate::cdt::foliation::{EdgeType, SimplexType};
    use crate::errors::TriangulationMetadataField;
    use crate::geometry::generators::build_delaunay2_from_simplices;
    use approx::assert_relative_eq;
    use std::assert_matches;

    fn coordinate_signature(triangulation: &CdtTriangulation<DelaunayBackend2D>) -> Vec<Vec<u64>> {
        let mut coordinates = triangulation
            .geometry()
            .vertices()
            .map(|vertex| {
                triangulation
                    .geometry()
                    .vertex_coordinates(&vertex)
                    .expect("generated vertex coordinates should be readable")
                    .iter()
                    .copied()
                    .map(f64::to_bits)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        coordinates.sort_unstable();
        coordinates
    }

    /// Builds a minimal labeled Delaunay backend for constructor tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt).expect("test Delaunay triangle should validate")
    }

    fn same_slice_non_strict_triangle() -> CdtTriangulation<DelaunayBackend2D> {
        let mut tri = CdtTriangulation::try_new(labeled_triangle_backend([0, 0, 0]), 1, 2)
            .expect("single Delaunay triangle should satisfy bare topology");
        tri.foliation = Some(
            Foliation::from_slice_sizes(vec![3], checked_nonzero_slice_count(1))
                .expect("single nonempty slice should form foliation bookkeeping"),
        );
        tri.mark_foliation_synchronized();
        tri
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
            .expect("Delaunay strip should pass upstream Level 1-5 validation");
        tri.validate_simplex_classification()
            .expect("all Delaunay strip simplices should classify");
        for face in tri.geometry().faces() {
            assert!(tri.simplex_type(&face).is_ok_and(|kind| kind.is_some()));
            assert!(
                tri.simplex_type_from_data(&face)
                    .is_ok_and(|kind| kind.is_some())
            );
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
                failure: DelaunayGenerationFailure::Upstream {
                    stage: crate::errors::DelaunayGenerationStage::TriangulationConstruction,
                    detail: "builder failed".to_string(),
                },
            },
            12,
            4,
        );

        assert_matches!(
            remapped,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 12,
                coordinate_range: (-1.0, 1.0),
                attempt: 4,
                failure: DelaunayGenerationFailure::Upstream { ref detail, .. },
            } if detail == "builder failed"
        );
    }

    #[test]
    fn test_remap_toroidal_generation_error_preserves_other_errors() {
        let original = CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::InvalidCoordinateRange,
            provided_value: "x".to_string(),
            expected_range: "y".to_string(),
        };
        assert_eq!(
            remap_toroidal_generation_error(original.clone(), 12, 4),
            original
        );
    }

    #[test]
    fn toroidal_candidate_retry_returns_first_success_with_actual_attempt() {
        let mut attempted = Vec::new();
        let result = first_valid_toroidal_candidate(
            4..8,
            |candidate, attempt| {
                attempted.push((candidate, attempt));
                if attempt < 3 {
                    Err(toroidal_generation_error(
                        12,
                        (0.0, 3.0),
                        attempt,
                        DelaunayGenerationFailure::Upstream {
                            stage:
                                crate::errors::DelaunayGenerationStage::TriangulationConstruction,
                            detail: format!("candidate {candidate} failed"),
                        },
                    ))
                } else {
                    Ok((candidate, attempt))
                }
            },
            || unreachable!("nonempty candidate range should be attempted"),
        )
        .expect("third candidate should succeed");

        assert_eq!(result, (6, 3));
        assert_eq!(attempted, vec![(4, 1), (5, 2), (6, 3)]);
    }

    #[test]
    fn toroidal_candidate_exhaustion_preserves_final_attempt_and_cause() {
        let error = first_valid_toroidal_candidate(
            4..7,
            |candidate, attempt| {
                Err::<(), _>(toroidal_generation_error(
                    12,
                    (0.0, 3.0),
                    attempt,
                    DelaunayGenerationFailure::Upstream {
                        stage: crate::errors::DelaunayGenerationStage::TriangulationConstruction,
                        detail: format!("candidate {candidate} failed"),
                    },
                ))
            },
            || unreachable!("nonempty candidate range should be attempted"),
        )
        .expect_err("all candidates should fail");

        assert_matches!(
            error,
            CdtError::DelaunayGenerationFailed {
                attempt: 3,
                failure: DelaunayGenerationFailure::Upstream { ref detail, .. },
                ..
            } if detail == "candidate 6 failed"
        );
    }

    #[test]
    fn test_from_random_points() {
        let triangulation =
            CdtTriangulation::from_random_points(10, 3, 2).expect("Failed to create triangulation");

        assert_eq!(triangulation.dimension(), 2);
        assert_eq!(triangulation.time_slices().get(), 3);
        assert_eq!(triangulation.vertex_count(), 10);
        assert!(triangulation.edge_count() > 0);
        assert!(triangulation.face_count() > 0);
    }

    #[test]
    fn test_from_seeded_points_various_sizes() {
        let test_cases = [
            (3, 1, "minimal"),
            (5, 2, "small"),
            (10, 3, "medium"),
            (20, 5, "large"),
        ];

        for (vertices, time_slices, description) in test_cases {
            let triangulation =
                CdtTriangulation::from_seeded_points(vertices, time_slices, 2, u64::from(vertices))
                    .unwrap_or_else(|e| {
                        panic!("Failed to create {description} triangulation: {e}")
                    });

            assert_eq!(
                triangulation.dimension(),
                2,
                "Dimension should be 2 for {description}"
            );
            assert_eq!(
                triangulation.time_slices().get(),
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
        assert_eq!(triangulation.time_slices().get(), 2);
        assert_eq!(triangulation.vertex_count(), 8);
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
        assert_eq!(
            coordinate_signature(&triangulation1),
            coordinate_signature(&triangulation2),
            "the same seed should reproduce every generated coordinate"
        );
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
        assert_ne!(
            coordinate_signature(&tri1),
            coordinate_signature(&tri2),
            "different seeds should change generated coordinates"
        );
    }

    #[test]
    fn test_invalid_dimension() {
        let invalid_dimensions = [0, 1, 3, 4, 5];
        for dim in invalid_dimensions {
            let result = CdtTriangulation::from_random_points(10, 3, dim);
            assert_matches!(
                result,
                Err(CdtError::UnsupportedDimension(d)) if d == u32::from(dim),
                "error should report unsupported dimension {dim}"
            );
        }
    }

    #[test]
    fn test_from_seeded_points_rejects_invalid_dimension() {
        let result = CdtTriangulation::from_seeded_points(10, 3, 3, 42);

        assert_matches!(result, Err(CdtError::UnsupportedDimension(3)));
    }

    #[test]
    fn test_from_seeded_points_rejects_zero_time_slices() {
        let result = CdtTriangulation::from_seeded_points(5, 0, 2, 53);

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
    fn test_invalid_vertex_count() {
        let invalid_counts = [0, 1, 2];
        for count in invalid_counts {
            let result = CdtTriangulation::from_random_points(count, 2, 2);
            assert_matches!(
                result,
                Err(CdtError::InvalidGenerationParameters {
                    issue: GenerationParameterIssue::InsufficientVertexCount,
                    ref provided_value,
                    ref expected_range,
                }) if provided_value == &count.to_string() && expected_range == "≥ 3",
                "error should report insufficient vertex count {count}"
            );
        }
    }

    #[test]
    fn test_invalid_vertex_count_seeded() {
        let result = CdtTriangulation::from_seeded_points(2, 2, 2, 123);
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientVertexCount,
                ref provided_value,
                ref expected_range,
            }) if provided_value == "2" && expected_range == "≥ 3"
        );
    }

    #[test]
    fn test_from_labeled_delaunay_preserves_foliation() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), &[2, 1]);
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_simplex_classification().is_ok());

        for vh in tri.geometry().vertices() {
            assert!(tri.time_label(&vh).is_ok_and(|label| label.is_some()));
        }
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_invalid_dimension() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 3);

        assert_matches!(result, Err(CdtError::UnsupportedDimension(3)));
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_zero_slices() {
        let backend = labeled_triangle_backend([0, 0, 1]);

        let result = CdtTriangulation::from_labeled_delaunay(backend, 0, 2);

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
    fn test_from_labeled_delaunay_rejects_out_of_range_labels() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 5)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 2);
        assert_matches!(
            result,
            Err(CdtError::Foliation(FoliationError::OutOfRangeVertexLabel {
                label: 5,
                expected_range_end: 2,
                ..
            }))
        );
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_empty_intermediate_slice() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 2), ([0.5, 1.0], 2)])
            .expect("Should build labeled triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");

        let result = CdtTriangulation::from_labeled_delaunay(backend, 3, 2);
        assert_matches!(
            result,
            Err(CdtError::Foliation(FoliationError::EmptySlice { slice: 1 }))
        );
    }

    #[test]
    fn test_from_labeled_delaunay_rejects_non_interval_spatial_slice() {
        let dt = build_delaunay2_from_simplices(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 0),
                ([1.0, 1.0], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        )
        .expect("explicit simplices should build before constructor validation");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay square should validate");

        let result = CdtTriangulation::from_labeled_delaunay(backend, 2, 2);

        assert_matches!(
            result,
            Err(CdtError::Foliation(
                FoliationError::SpacelikeOpenSliceEndpointCount {
                    slice: 0,
                    observed: 0,
                    expected: 2,
                }
            ))
        );
    }

    #[test]
    fn test_builder_rejects_non_delaunay_simplices() {
        let result = build_delaunay2_from_simplices(
            &[
                ([0.0, 0.0], 0),
                ([1.0, 0.0], 0),
                ([0.0, 1.0], 1),
                ([0.2, 0.2], 1),
            ],
            &[vec![0, 1, 2], vec![1, 3, 2]],
        );

        assert_matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count: 4,
                attempt: 1,
                ..
            })
        );
    }

    #[test]
    fn test_from_cdt_strip_all_vertices_labeled() {
        let tri = strict_strip(5, 3);
        for vertex in tri.geometry().vertices() {
            assert!(tri.time_label(&vertex).is_ok_and(|label| label.is_some()));
        }
    }

    #[test]
    fn test_from_cdt_strip_edge_classification() {
        let tri = strict_strip(5, 3);
        for edge in tri.geometry().edges() {
            assert_matches!(
                tri.edge_type(&edge),
                Ok(Some(EdgeType::Spacelike | EdgeType::Timelike))
            );
        }
    }

    #[test]
    fn test_from_cdt_strip_rejects_invalid_params() {
        let few_vertices = CdtTriangulation::from_cdt_strip(3, 3);
        assert_matches!(
            few_vertices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientVerticesPerSlice
                && provided_value == "3"
                && expected_range == "≥ 4"
        );

        let few_slices = CdtTriangulation::from_cdt_strip(4, 1);
        assert_matches!(
            few_slices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientNumberOfTimeSlices
                && provided_value == "1"
                && expected_range == "≥ 2"
        );
    }

    #[test]
    fn test_from_cdt_strip_rejects_simplex_count_overflow() {
        let result = CdtTriangulation::from_cdt_strip(65_535, 65_537);

        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::SimplexCountOverflow
                && provided_value == "2 × 4294836224"
                && expected_range == "product ≤ u32::MAX"
        );
    }

    #[test]
    fn open_strip_vertex_spec_applies_arc_and_jitter() {
        let side_jitter = 1.0 / 12.0;
        let interior_jitter = 1.0 / 48.0;
        let vertical_jitter = 1.0e-9;

        let ([left_boundary_x, left_boundary_y], left_boundary_label) =
            open_strip_vertex_spec(1, 0, 4, 3, side_jitter, interior_jitter, vertical_jitter);
        assert_eq!(left_boundary_label, 1);
        assert_relative_eq!(left_boundary_x, 1.0 / 16.0, epsilon = 1e-15);
        assert_relative_eq!(left_boundary_y, 1.0, epsilon = 1e-15);

        let ([interior_x, interior_y], interior_label) =
            open_strip_vertex_spec(1, 1, 4, 3, side_jitter, interior_jitter, vertical_jitter);
        assert_eq!(interior_label, 1);
        assert_relative_eq!(interior_x, 7.0 / 16.0, epsilon = 1e-15);
        assert_relative_eq!(interior_y, 1.0 + 1.0e-9 / 9.0, epsilon = 1e-18);

        let ([top_boundary_x, top_boundary_y], top_boundary_label) =
            open_strip_vertex_spec(2, 3, 4, 3, side_jitter, interior_jitter, vertical_jitter);
        assert_eq!(top_boundary_label, 2);
        assert_relative_eq!(top_boundary_x, 13.0 / 12.0, epsilon = 1e-15);
        assert_relative_eq!(top_boundary_y, 2.0, epsilon = 1e-15);
    }

    #[test]
    fn test_from_cdt_strip_builds_valid_mesh() {
        let tri = CdtTriangulation::from_cdt_strip(4, 2).expect("Delaunay strip should build");
        assert_eq!(tri.vertex_count(), 8);
        assert_eq!(tri.face_count(), 6);
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality_delaunay().is_ok());
        assert!(tri.validate_simplex_classification().is_ok());
        assert_eq!(
            tri.strict_causal_simplex_violation_count()
                .expect("Delaunay strip should expose a current strict-causality count"),
            0
        );
    }

    #[test]
    fn test_from_filtered_delaunay_strip_removes_surplus_invalid_vertices() {
        let tri = CdtTriangulation::from_filtered_delaunay_strip(4, 3)
            .expect("filtered Delaunay strip should build");

        assert_eq!(tri.vertex_count(), 12);
        assert_eq!(tri.slice_sizes(), &[4, 4, 4]);
        assert!(tri.has_foliation());
        assert!(tri.geometry().validate_delaunay().is_ok());
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality_delaunay().is_ok());
        assert!(tri.validate_simplex_classification().is_ok());
        assert_eq!(
            tri.strict_causal_simplex_violation_count()
                .expect("filtered strip should expose a current strict-causality count"),
            0
        );
    }

    #[test]
    fn test_from_filtered_delaunay_strip_accepts_two_slice_boundary() {
        let tri = CdtTriangulation::from_filtered_delaunay_strip(4, 2)
            .expect("filtered Delaunay strip should build without surplus vertices");

        assert_eq!(tri.vertex_count(), 8);
        assert_eq!(tri.face_count(), 6);
        assert_eq!(tri.slice_sizes(), &[4, 4]);
        assert!(tri.has_foliation());
        assert!(tri.geometry().validate_delaunay().is_ok());
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality_delaunay().is_ok());
        assert!(tri.validate_simplex_classification().is_ok());
        assert_eq!(
            tri.strict_causal_simplex_violation_count()
                .expect("two-slice filtered strip should expose a strict-causality count"),
            0
        );
    }

    #[test]
    fn test_invalid_simplex_removal_candidate_allows_surplus_same_slice_vertex() {
        let tri = same_slice_non_strict_triangle();

        let candidate = tri
            .invalid_simplex_removal_candidate(2, 3, 2.0)
            .expect("surplus same-slice face should be inspectable")
            .expect("same-slice face should yield a removable vertex");

        assert_eq!(
            tri.geometry().vertex_data_by_key(candidate.vertex_key()),
            Some(0)
        );
    }

    #[test]
    fn test_invalid_simplex_removal_candidate_respects_minimum_slice_size() {
        let tri = same_slice_non_strict_triangle();

        let err = tri
            .invalid_simplex_removal_candidate(3, 3, 2.0)
            .expect_err("minimum slice size should prevent removal");

        assert_matches!(
            err,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 3,
                coordinate_range: (0.0, 2.0),
                attempt: 1,
                failure: DelaunayGenerationFailure::MinimumSliceSizeReached {
                    minimum_slice_size: 3,
                    ..
                },
            }
        );
    }

    #[test]
    fn test_invalid_simplex_removal_candidate_returns_none_for_strict_strip() {
        let tri = strict_strip(4, 2);

        assert!(
            tri.invalid_simplex_removal_candidate(4, 8, 2.0)
                .expect("strict strip should be inspectable")
                .is_none()
        );
    }

    #[test]
    fn test_filter_invalid_open_delaunay_simplices_rejects_missing_live_label() {
        let mut tri = same_slice_non_strict_triangle();
        let vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("test triangle should contain a vertex");
        tri.set_vertex_data(&vertex, None)
            .expect("test vertex should accept clearing its label");

        assert_matches!(
            tri.filter_invalid_open_delaunay_simplices(2, 1, 3, 2.0),
            Err(CdtError::Foliation(FoliationError::MissingVertexLabel {
                vertex: 0
            }))
        );
    }

    #[test]
    fn test_filter_invalid_open_delaunay_simplices_reports_pass_budget_exhaustion() {
        let mut tri = same_slice_non_strict_triangle();

        let err = tri
            .filter_invalid_open_delaunay_simplices(2, 0, 3, 2.0)
            .expect_err("zero-pass budget should report remaining violations");

        assert_matches!(
            err,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 3,
                coordinate_range: (0.0, 2.0),
                attempt: 1,
                failure: DelaunayGenerationFailure::FilterPassBudgetExhausted {
                    max_passes: 0,
                    remaining_violations: 1,
                },
            }
        );
    }

    #[test]
    fn test_from_filtered_delaunay_strip_rejects_invalid_parameters() {
        let few_vertices = CdtTriangulation::from_filtered_delaunay_strip(3, 3);
        assert_matches!(
            few_vertices,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientVerticesPerSlice,
                ..
            })
        );

        let few_slices = CdtTriangulation::from_filtered_delaunay_strip(4, 1);
        assert_matches!(
            few_slices,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InsufficientNumberOfTimeSlices,
                ..
            })
        );

        let product_overflow = CdtTriangulation::from_filtered_delaunay_strip(u32::MAX, 2);
        assert_matches!(
            product_overflow,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                ref provided_value,
                ref expected_range,
            }) if provided_value == "4294967295 × 2"
                && expected_range == "product ≤ u32::MAX"
        );

        let surplus_overflow = CdtTriangulation::from_filtered_delaunay_strip(1_431_655_765, 3);
        assert_matches!(
            surplus_overflow,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::VertexCountOverflow,
                ref provided_value,
                ref expected_range,
            }) if provided_value == "4294967295 + 2" && expected_range == "sum ≤ u32::MAX"
        );
    }

    #[test]
    fn test_from_cdt_strip_profile_builds_nonuniform_valid_mesh() {
        let tri = CdtTriangulation::from_cdt_strip_profile(&[4, 6, 5])
            .expect("nonuniform Delaunay strip should build");

        assert_eq!(tri.vertex_count(), 15);
        assert_eq!(tri.face_count(), 17);
        assert_eq!(tri.slice_sizes(), &[4, 6, 5]);
        assert_eq!(
            tri.volume_profile()
                .expect("nonuniform strip profile should be valid")
                .len(),
            3
        );
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality_delaunay().is_ok());
        assert!(tri.validate_simplex_classification().is_ok());
    }

    #[test]
    fn test_open_profile_face_count_matches_open_strip_topology() {
        assert_eq!(
            open_profile_face_count(&[4, 4]).expect("regular two-slice strip should count"),
            6
        );
        assert_eq!(
            open_profile_face_count(&[4, 4, 4]).expect("regular three-slice strip should count"),
            12
        );
        assert_eq!(
            open_profile_face_count(&[4, 6, 5]).expect("nonuniform strip should count"),
            17
        );
    }

    #[test]
    fn test_open_profile_face_count_rejects_empty_profile() {
        let result = open_profile_face_count(&[]);

        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::EmptyVolumeProfile
                && provided_value == "[]"
                && expected_range == "at least one time slice"
        );
    }

    #[test]
    fn test_profile_strip_count_validation_rejects_backend_count_mismatch() {
        let tri = CdtTriangulation::from_cdt_strip_profile(&[4, 6, 5])
            .expect("nonuniform Delaunay strip should build");
        let result = validate_profile_strip_counts(tri.geometry(), 15, 16, 18, 3.0);

        assert_matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count: 15,
                coordinate_range: (0.0, 3.0),
                attempt: 1,
                failure: DelaunayGenerationFailure::MeshSizeMismatch {
                    actual_vertices: 15,
                    expected_vertices: 16,
                    actual_faces: 17,
                    expected_faces: 18,
                },
            })
        );
    }

    #[test]
    fn test_from_cdt_strip_profile_rejects_invalid_profile() {
        let result = CdtTriangulation::from_cdt_strip_profile(&[4, 3, 5]);

        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientVerticesInVolumeProfileSlice
                && provided_value == "slice 1 has 3"
                && expected_range == "each slice ≥ 4 for open-boundary topology"
        );
    }

    #[test]
    fn test_from_cdt_strip_profile_rejects_too_few_slices() {
        let result = CdtTriangulation::from_cdt_strip_profile(&[4]);

        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientNumberOfTimeSlices
                && provided_value == "1"
                && expected_range == "≥ 2 for open-boundary topology"
        );
    }

    #[test]
    fn test_explicit_strip_count_validation_rejects_face_mismatch() {
        let tri = CdtTriangulation::from_cdt_strip(4, 2).expect("Delaunay strip should build");
        let result = validate_strip_counts(tri.geometry(), 8, 8, 7, 1.0);

        assert_matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count: 8,
                coordinate_range: (0.0, 1.0),
                attempt: 1,
                failure: DelaunayGenerationFailure::MeshSizeMismatch {
                    actual_vertices: 8,
                    expected_vertices: 8,
                    actual_faces: 6,
                    expected_faces: 7,
                },
            })
        );
    }

    #[test]
    fn test_simplex_type_returns_up_or_down() {
        let tri = strict_strip(5, 3);
        for face in tri.geometry().faces() {
            assert_matches!(
                tri.simplex_type(&face),
                Ok(Some(SimplexType::Up | SimplexType::Down))
            );
        }
    }

    #[test]
    fn test_from_toroidal_cdt_basic() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3)
            .expect("toroidal CDT should build with delaunay v0.8");

        // V = N*T = 12, F = 2*N*T = 24, E = 3*N*T = 36, χ = 0.
        assert_eq!(tri.vertex_count(), 12);
        assert_eq!(tri.face_count(), 24);
        assert_eq!(tri.edge_count(), 36);
        assert_eq!(tri.geometry().euler_characteristic(), 0);
        assert_eq!(tri.dimension(), 2);
        assert_eq!(tri.time_slices().get(), 3);
        assert_matches!(tri.metadata().topology, CdtTopology::Toroidal);
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
                tri.vertices_at_time(t).count(),
                5,
                "slice {t} should contain N=5 vertices"
            );
        }
    }

    #[test]
    fn test_from_toroidal_cdt_profile_builds_nonuniform_valid_mesh() {
        let tri = CdtTriangulation::from_toroidal_cdt_profile(&[3, 4, 5, 4])
            .expect("nonuniform toroidal CDT should build");

        assert_eq!(tri.vertex_count(), 16);
        assert_eq!(tri.face_count(), 32);
        assert_eq!(tri.edge_count(), 48);
        assert_eq!(tri.slice_sizes(), &[3, 4, 5, 4]);
        assert_eq!(tri.geometry().euler_characteristic(), 0);
        assert_matches!(tri.metadata().topology, CdtTopology::Toroidal);
        assert!(tri.geometry().validate_delaunay().is_ok());
        assert!(tri.validate_topology().is_ok());
        assert!(tri.validate_foliation().is_ok());
        assert!(tri.validate_causality().is_ok());
        assert!(tri.validate_simplex_classification().is_ok());
    }

    #[test]
    fn test_from_toroidal_cdt_profile_rejects_invalid_profile() {
        let few_slices = CdtTriangulation::from_toroidal_cdt_profile(&[3, 4]);
        assert_matches!(
            few_slices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientNumberOfTimeSlices
                && provided_value == "2"
                && expected_range == "≥ 3 for toroidal topology"
        );

        let small_slice = CdtTriangulation::from_toroidal_cdt_profile(&[3, 2, 3]);
        assert_matches!(
            small_slice,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientVerticesInVolumeProfileSlice
                && provided_value == "slice 1 has 2"
                && expected_range == "each slice ≥ 3 for toroidal topology"
        );
    }

    #[test]
    fn test_from_toroidal_cdt_initializes_delaunay_pl_manifold() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        assert_eq!(tri.vertex_count(), 12);
        assert_eq!(tri.face_count(), 24);
        assert_eq!(tri.geometry().periodic_domain(), Some([4.0, 3.0]));
        tri.geometry()
            .validate_delaunay()
            .expect("initial toroidal CDT must pass upstream Level 1-5 validation");
        tri.validate_topology()
            .expect("initial toroidal CDT must satisfy torus topology");
        tri.validate_foliation()
            .expect("initial toroidal CDT must have valid time-slice foliation");
        tri.validate_causality()
            .expect("initial toroidal CDT must only contain adjacent-slice edges");
        tri.validate_simplex_classification()
            .expect("initial toroidal CDT must classify every face as an Up or Down CDT simplex");
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
        assert_matches!(
            few_vertices,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::InsufficientVerticesPerSlice
                && provided_value == "2"
                && expected_range == "≥ 3"
        );

        for slices in [1, 2] {
            let few_slices = CdtTriangulation::from_toroidal_cdt(4, slices);
            assert_matches!(
                few_slices,
                Err(CdtError::InvalidGenerationParameters {
                    ref issue,
                    ref provided_value,
                    ref expected_range,
                }) if *issue == GenerationParameterIssue::InsufficientNumberOfTimeSlices
                    && provided_value == &slices.to_string()
                    && expected_range == "≥ 3"
            );
        }
    }

    #[test]
    fn test_from_toroidal_cdt_rejects_vertex_count_overflow() {
        let result = CdtTriangulation::from_toroidal_cdt(u32::MAX, 3);

        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::VertexCountOverflow
                && provided_value == "4294967295 × 3"
                && expected_range == "product ≤ u32::MAX"
        );
    }

    #[test]
    fn test_periodic_toroidal_count_validation_rejects_face_mismatch() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        let result = validate_toroidal_counts(tri.geometry(), 12, 12, 23, (0.0, 3.0), 5);

        assert_matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed {
                vertex_count: 12,
                coordinate_range: (0.0, 3.0),
                attempt: 5,
                failure: DelaunayGenerationFailure::MeshSizeMismatch {
                    actual_vertices: 12,
                    expected_vertices: 12,
                    actual_faces: 24,
                    expected_faces: 23,
                },
            })
        );
    }

    #[test]
    fn test_toroidal_simplex_classification_uses_temporal_wrap() {
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
                .map(|vh| {
                    tri.geometry()
                        .vertex_data_by_key(vh.vertex_key())
                        .expect("toroidal vertices are labeled")
                })
                .collect();

            if labels.contains(&0) && labels.contains(&2) {
                let simplex_type = tri
                    .simplex_type(&face)
                    .expect("wrap-around toroidal face query should succeed")
                    .expect("wrap-around toroidal face should classify");
                let edge_types = tri
                    .face_edge_types(&face)
                    .expect("wrap-around toroidal face query should succeed")
                    .expect("wrap-around toroidal face should expose edge types");
                saw_wrap_timelike_edge |= edge_types
                    .iter()
                    .any(|edge_type| matches!(edge_type, EdgeType::Timelike));

                let zero_count = labels.iter().filter(|&&label| label == 0).count();
                let two_count = labels.iter().filter(|&&label| label == 2).count();
                let is_wrap_up = zero_count == 1 && two_count == 2;
                let is_wrap_down = zero_count == 2 && two_count == 1;

                if is_wrap_up {
                    assert_eq!(simplex_type, SimplexType::Up);
                }
                if is_wrap_down {
                    assert_eq!(simplex_type, SimplexType::Down);
                }

                saw_wrap_up |= is_wrap_up;
                saw_wrap_down |= is_wrap_down;
            }
        }

        assert!(
            saw_wrap_up,
            "expected an Up simplex across the temporal wrap"
        );
        assert!(
            saw_wrap_down,
            "expected a Down simplex across the temporal wrap"
        );
        assert!(
            saw_wrap_timelike_edge,
            "expected a timelike edge across the temporal wrap"
        );
    }
}
