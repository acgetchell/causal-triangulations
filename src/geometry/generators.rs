#![forbid(unsafe_code)]

//! Delaunay triangulation generators.
//!
//! This module constructs 2D Delaunay triangulations via the `delaunay` crate.
//! Together with `src/geometry/backends/delaunay.rs` it forms the only boundary
//! that directly imports from the `delaunay` crate (see
//! `docs/dev/rust.md § Geometry Backend Isolation`).

use crate::errors::{
    CdtError, CdtResult, DelaunayGenerationFailure, DelaunayGenerationStage,
    GenerationParameterIssue,
};
pub use delaunay::TopologyGuarantee;
use delaunay::geometry::{
    kernel::AdaptiveKernel,
    util::{try_generate_random_points, try_generate_random_points_seeded},
};
use delaunay::prelude::Vertex;
pub use delaunay::topology::traits::{GlobalTopology, ToroidalConstructionMode, ToroidalDomain};
use delaunay::{
    ConstructionOptions, DelaunayTriangulation, DelaunayTriangulationBuilder, Triangulation,
};
use std::fmt::Display;

/// Type alias for the 2D Delaunay triangulation returned by this crate's generators.
///
/// Uses [`AdaptiveKernel`] (the default for [`DelaunayTriangulationBuilder::build`]) and
/// `u32` vertex data storing the per-vertex time-slice label (foliation).
pub type DelaunayTriangulation2D = DelaunayTriangulation<AdaptiveKernel<f64>, u32, i32, 2>;

/// Mutable Levels 1-4 owner used internally for exact layered CDT connectivity.
pub(crate) type RealizedTriangulation2D = Triangulation<AdaptiveKernel<f64>, u32, i32, 2>;

/// Validated explicit vertices and the context retained for upstream diagnostics.
struct PreparedVertices {
    vertices: Vec<Vertex<u32, 2>>,
    vertex_count: u32,
    coordinate_range: (f64, f64),
}

/// Keeps `generate_delaunay2` vertex-build failures tied to the public constructor name.
fn generate_delaunay2_vertex_build_error(
    number_of_vertices: u32,
    underlying_error: String,
) -> CdtError {
    CdtError::VertexBuildFailed {
        context: format!("generate_delaunay2({number_of_vertices} vertices)"),
        underlying_error,
    }
}

/// Builds a consistent typed validation error for generator argument checks.
fn invalid_generation_parameters(
    issue: GenerationParameterIssue,
    provided_value: String,
    expected_range: &str,
) -> CdtError {
    CdtError::InvalidGenerationParameters {
        issue,
        provided_value,
        expected_range: expected_range.to_string(),
    }
}

/// Rejects coordinate ranges before they reach random point generation.
fn validate_coordinate_range(coordinate_range: (f64, f64)) -> CdtResult<()> {
    let (min, max) = coordinate_range;
    if min.is_finite() && max.is_finite() && min < max {
        Ok(())
    } else {
        Err(invalid_generation_parameters(
            GenerationParameterIssue::InvalidCoordinateRange,
            format!("[{min}, {max}]"),
            "finite min < max",
        ))
    }
}

/// Rejects explicit vertex coordinates that geometric predicates cannot order.
fn validate_explicit_coordinates(coords_with_data: &[([f64; 2], u32)]) -> CdtResult<()> {
    for (vertex_index, (coord, _)) in coords_with_data.iter().enumerate() {
        for (axis, value) in coord.iter().copied().enumerate() {
            if !value.is_finite() {
                return Err(invalid_generation_parameters(
                    GenerationParameterIssue::NonFiniteVertexCoordinate,
                    format!("vertex {vertex_index} axis {axis} = {value}"),
                    "finite coordinate values",
                ));
            }
        }
    }
    Ok(())
}

/// Validates and converts explicit coordinate-data pairs once for all builder paths.
fn prepare_explicit_vertices(coords_with_data: &[([f64; 2], u32)]) -> CdtResult<PreparedVertices> {
    validate_explicit_coordinates(coords_with_data)?;

    let vertices = coords_with_data
        .iter()
        .enumerate()
        .map(|(i, (coord, data))| {
            Vertex::<u32, 2>::try_new_with_data(*coord, *data).map_err(|error| {
                CdtError::VertexBuildFailed {
                    context: format!("vertex {i}"),
                    underlying_error: error.to_string(),
                }
            })
        })
        .collect::<CdtResult<Vec<_>>>()?;
    let vertex_count = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
    let coordinate_range = coords_with_data
        .iter()
        .flat_map(|(coordinates, _)| coordinates.iter().copied())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), value| {
            (lo.min(value), hi.max(value))
        });

    Ok(PreparedVertices {
        vertices,
        vertex_count,
        coordinate_range,
    })
}

/// Preserves explicit-construction context for an upstream builder failure.
fn upstream_construction_error(prepared: &PreparedVertices, error: impl Display) -> CdtError {
    CdtError::DelaunayGenerationFailed {
        vertex_count: prepared.vertex_count,
        coordinate_range: prepared.coordinate_range,
        attempt: 1,
        failure: DelaunayGenerationFailure::Upstream {
            stage: DelaunayGenerationStage::TriangulationConstruction,
            detail: error.to_string(),
        },
    }
}

/// Rejects toroidal periods that cannot define a finite positive quotient domain.
fn validate_toroidal_domain(domain: [f64; 2]) -> CdtResult<()> {
    for (axis, period) in domain.into_iter().enumerate() {
        if !period.is_finite() || period <= 0.0 {
            return Err(invalid_generation_parameters(
                GenerationParameterIssue::InvalidToroidalDomain,
                format!("axis {axis} period {period}"),
                "finite and positive periods",
            ));
        }
    }
    Ok(())
}

/// Generates a Delaunay triangulation with optional seed for deterministic testing.
///
/// Uses the canonical [`DelaunayTriangulationBuilder`] workflow from Delaunay
/// v0.8, which provides deterministic tie-breaking and coherent orientation as
/// first-class invariants.
///
/// # Errors
///
/// Returns [`crate::CdtError::InvalidGenerationParameters`] if
/// `number_of_vertices < 3` or `coordinate_range` is not finite with `min < max`.
/// Returns [`crate::CdtError::DelaunayGenerationFailed`] if random point
/// generation or Delaunay construction fails, and
/// [`crate::CdtError::VertexBuildFailed`] if an upstream vertex cannot be built.
///
/// # Examples
///
/// ```
/// use causal_triangulations::CdtResult;
/// use causal_triangulations::prelude::geometry::*;
///
/// fn main() -> CdtResult<()> {
///     let dt = generate_delaunay2(5, (0.0, 1.0), Some(7))?;
///     assert_eq!(dt.number_of_vertices(), 5);
///     Ok(())
/// }
/// ```
pub fn generate_delaunay2(
    number_of_vertices: u32,
    coordinate_range: (f64, f64),
    seed: Option<u64>,
) -> CdtResult<DelaunayTriangulation2D> {
    // Validate parameters before attempting generation
    if number_of_vertices < 3 {
        return Err(invalid_generation_parameters(
            GenerationParameterIssue::InsufficientVertexCount,
            number_of_vertices.to_string(),
            "≥ 3",
        ));
    }

    validate_coordinate_range(coordinate_range)?;

    // Generate random points, then build triangulation via the builder API
    let n = number_of_vertices as usize;
    let points = seed
        .map_or_else(
            || try_generate_random_points::<2>(n, coordinate_range),
            |s| try_generate_random_points_seeded::<2>(n, coordinate_range, s),
        )
        .map_err(|e| CdtError::DelaunayGenerationFailed {
            vertex_count: number_of_vertices,
            coordinate_range,
            attempt: 1,
            failure: DelaunayGenerationFailure::Upstream {
                stage: DelaunayGenerationStage::PointSampling,
                detail: e.to_string(),
            },
        })?;

    // Explicitly type the vertices as Vertex<u32, 2> so the triangulation
    // has u32 vertex data available for time-slice labels (even if unset here).
    let vertices: Vec<_> = points
        .into_iter()
        .map(|point| Vertex::<u32, 2>::try_new(*point.coords()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| generate_delaunay2_vertex_build_error(number_of_vertices, e.to_string()))?;

    let dt = DelaunayTriangulationBuilder::new(&vertices)
        .simplex_data_type::<i32>()
        .build()
        .map_err(|e| CdtError::DelaunayGenerationFailed {
            vertex_count: number_of_vertices,
            coordinate_range,
            attempt: 1,
            failure: DelaunayGenerationFailure::Upstream {
                stage: DelaunayGenerationStage::TriangulationConstruction,
                detail: e.to_string(),
            },
        })?;

    Ok(dt)
}

/// Builds a 2D Delaunay triangulation from coordinate-data pairs.
///
/// Each element provides `[x, y]` coordinates and `u32` vertex data
/// (e.g., a time-slice label).  The vertex data is embedded directly on
/// each vertex of the underlying Delaunay triangulation.
///
/// # Errors
///
/// Returns [`crate::CdtError::InvalidGenerationParameters`] if any coordinate is
/// NaN or infinite. Returns [`crate::CdtError::VertexBuildFailed`] if a vertex
/// cannot be constructed, or [`crate::CdtError::DelaunayGenerationFailed`] if
/// the Delaunay builder rejects the finite vertex set.
///
/// # Examples
///
/// ```
/// use causal_triangulations::CdtResult;
/// use causal_triangulations::prelude::geometry::*;
///
/// fn main() -> CdtResult<()> {
///     let dt = build_delaunay2_with_data(&[
///         ([0.0, 0.0], 0),
///         ([1.0, 0.0], 0),
///         ([0.5, 1.0], 1),
///     ])?;
///     assert_eq!(dt.number_of_vertices(), 3);
///     Ok(())
/// }
/// ```
pub fn build_delaunay2_with_data(
    coords_with_data: &[([f64; 2], u32)],
) -> CdtResult<DelaunayTriangulation2D> {
    let prepared = prepare_explicit_vertices(coords_with_data)?;

    DelaunayTriangulationBuilder::new(&prepared.vertices)
        .simplex_data_type::<i32>()
        .build()
        .map_err(|error| upstream_construction_error(&prepared, error))
}

/// Builds an exact layered 2D triangulation from explicit CDT connectivity.
///
/// The upstream non-enforcing policy preserves exact collinear slice coordinates
/// and validates the imported mesh through Level 4 without requiring the Level 5
/// empty-circumsphere predicate.
pub(crate) fn build_layered_delaunay2_from_simplices(
    coords_with_data: &[([f64; 2], u32)],
    simplices: &[Vec<usize>],
) -> CdtResult<RealizedTriangulation2D> {
    let prepared = prepare_explicit_vertices(coords_with_data)?;

    DelaunayTriangulationBuilder::try_from_vertices_and_simplices(&prepared.vertices, simplices)
        .map_err(|error| upstream_construction_error(&prepared, error))?
        .simplex_data_type::<i32>()
        .topology_guarantee(TopologyGuarantee::DEFAULT)
        .global_topology(GlobalTopology::Euclidean)
        .construction_options(ConstructionOptions::default().without_final_delaunay_enforcement())
        .build_triangulation()
        .map_err(|error| upstream_construction_error(&prepared, error))
}

/// Builds a 2D triangulation from explicit vertex coordinates, data, and simplex connectivity.
///
/// Each vertex is specified as `([x, y], data)`. Each simplex is a `Vec<usize>` of
/// vertex indices (must contain exactly 3 indices for 2D).  The triangulation is
/// assembled combinatorially — **no Delaunay point-insertion** is performed.
///
/// Topology defaults to [`TopologyGuarantee::DEFAULT`] (PL-manifold) and
/// [`GlobalTopology::Euclidean`].  For explicit meshes that need non-default
/// topology metadata, use [`build_delaunay2_with_topology`].  For toroidal CDT
/// meshes, prefer [`build_periodic_toroidal_delaunay2`] or
/// [`CdtTriangulation::from_toroidal_cdt`](crate::CdtTriangulation::from_toroidal_cdt):
/// `delaunay` v0.8 rejects explicit non-Euclidean connectivity for toroidal
/// construction.
///
/// This is one of the only call sites for
/// [`DelaunayTriangulationBuilder::try_from_vertices_and_simplices`], maintaining
/// geometry backend isolation.
///
/// # Errors
///
/// Returns [`crate::CdtError::InvalidGenerationParameters`] if any coordinate is
/// NaN or infinite. Returns [`crate::CdtError::VertexBuildFailed`] if a vertex
/// cannot be constructed, or [`crate::CdtError::DelaunayGenerationFailed`] if
/// the explicit simplex builder rejects the input (for example invalid simplex arity,
/// out-of-bounds indices, or topological validation failure).
///
/// # Examples
///
/// ```
/// use causal_triangulations::CdtResult;
/// use causal_triangulations::prelude::geometry::*;
///
/// fn main() -> CdtResult<()> {
///     // Single labeled triangle (PL-manifold-with-boundary, Euclidean):
///     let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
///     let simplices = vec![vec![0, 1, 2]];
///
///     let dt = build_delaunay2_from_simplices(&vertices, &simplices)?;
///     assert_eq!(dt.number_of_vertices(), 3);
///     assert_eq!(dt.number_of_simplices(), 1);
///     Ok(())
/// }
/// ```
pub fn build_delaunay2_from_simplices(
    coords_with_data: &[([f64; 2], u32)],
    simplices: &[Vec<usize>],
) -> CdtResult<DelaunayTriangulation2D> {
    build_delaunay2_with_topology(
        coords_with_data,
        simplices,
        TopologyGuarantee::DEFAULT,
        GlobalTopology::Euclidean,
    )
}

/// Like [`build_delaunay2_from_simplices`] but with explicit [`TopologyGuarantee`] and
/// [`GlobalTopology`] metadata.
///
/// Use [`TopologyGuarantee::Pseudomanifold`] for supported explicit meshes whose
/// Euler characteristic differs from the default closed-sphere expectation, and
/// pair it with the matching [`GlobalTopology`] so the builder validates against
/// the correct expected χ.  For toroidal CDT meshes, use
/// [`build_periodic_toroidal_delaunay2`]; `delaunay` v0.8 rejects
/// [`GlobalTopology::Toroidal`] explicit simplex connectivity pending upstream
/// quotient-validation support.
///
/// # Errors
///
/// Same as [`build_delaunay2_from_simplices`]: coordinates must be finite, vertices
/// must build successfully, and the explicit simplices must satisfy the selected
/// topology guarantee and global topology.
///
/// # Examples
///
/// Import topology metadata from this crate's geometry prelude:
///
/// ```
/// use causal_triangulations::CdtResult;
/// use causal_triangulations::prelude::geometry::*;
///
/// fn main() -> CdtResult<()> {
///     // Single labeled triangle, default PL-manifold guarantee, Euclidean global topology.
///     let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
///     let simplices = vec![vec![0, 1, 2]];
///
///     let dt = build_delaunay2_with_topology(
///         &vertices,
///         &simplices,
///         TopologyGuarantee::DEFAULT,
///         GlobalTopology::Euclidean,
///     )?;
///     assert_eq!(dt.number_of_vertices(), 3);
///     assert_eq!(dt.number_of_simplices(), 1);
///     Ok(())
/// }
/// ```
pub fn build_delaunay2_with_topology(
    coords_with_data: &[([f64; 2], u32)],
    simplices: &[Vec<usize>],
    topology_guarantee: TopologyGuarantee,
    global_topology: GlobalTopology<2>,
) -> CdtResult<DelaunayTriangulation2D> {
    build_delaunay2_with_topology_options(
        coords_with_data,
        simplices,
        topology_guarantee,
        global_topology,
        ConstructionOptions::default(),
    )
}

/// Builds explicit 2D connectivity with the requested topology and construction policies.
fn build_delaunay2_with_topology_options(
    coords_with_data: &[([f64; 2], u32)],
    simplices: &[Vec<usize>],
    topology_guarantee: TopologyGuarantee,
    global_topology: GlobalTopology<2>,
    construction_options: ConstructionOptions,
) -> CdtResult<DelaunayTriangulation2D> {
    let prepared = prepare_explicit_vertices(coords_with_data)?;

    DelaunayTriangulationBuilder::try_from_vertices_and_simplices(&prepared.vertices, simplices)
        .map_err(|error| upstream_construction_error(&prepared, error))?
        .simplex_data_type::<i32>()
        .topology_guarantee(topology_guarantee)
        .global_topology(global_topology)
        .construction_options(construction_options)
        .build()
        .map_err(|error| upstream_construction_error(&prepared, error))
}

/// Attempts to build a 2D toroidal explicit triangulation.
///
/// Sets [`TopologyGuarantee::Pseudomanifold`] and
/// [`GlobalTopology::Toroidal`] with [`ToroidalConstructionMode::Explicit`]
/// so the builder validates the mesh against χ = 0 instead of the default
/// closed-sphere expectation.
///
/// `delaunay` v0.8 rejects explicit non-Euclidean connectivity for toroidal
/// topology before quotient validation can run.  This helper remains as the
/// stable explicit-topology entry point, but callers that need an actual
/// toroidal CDT mesh should use [`build_periodic_toroidal_delaunay2`] or the
/// higher-level
/// [`CdtTriangulation::from_toroidal_cdt`](crate::CdtTriangulation::from_toroidal_cdt)
/// constructor.
///
/// # Errors
///
/// Returns [`crate::CdtError::InvalidGenerationParameters`] if either toroidal
/// period in `domain` is NaN, infinite, or non-positive. Otherwise the error
/// behavior is the same as [`build_delaunay2_with_topology`], including the
/// upstream explicit-toroidal rejection described above.
///
/// # Examples
///
/// The helper validates the toroidal domain before forwarding explicit simplex
/// connectivity to `delaunay`:
///
/// ```
/// use causal_triangulations::{CdtError, CdtResult, DelaunayGenerationFailure};
/// use causal_triangulations::prelude::geometry::*;
/// use std::assert_matches;
///
/// fn main() -> CdtResult<()> {
///     const N: usize = 3;
///     const LABELS: [u32; 3] = [0, 1, 2];
///     const T: usize = LABELS.len();
///
///     // Vertex (i, t) lives at index i + t*N, with x = i/N, y = t/T, label = t.
///     let mut vertices: Vec<([f64; 2], u32)> = Vec::with_capacity(N * T);
///     for (t, label) in LABELS.into_iter().enumerate() {
///         for i in 0..N {
///             #[allow(clippy::cast_precision_loss)]
///             let coord = [i as f64 / N as f64, t as f64 / T as f64];
///             vertices.push((coord, label));
///         }
///     }
///
///     // Each (i, t) quad contributes one Up and one Down triangle.
///     let mut simplices: Vec<Vec<usize>> = Vec::with_capacity(2 * N * T);
///     for t in 0..T {
///         let t_next = (t + 1) % T;
///         for i in 0..N {
///             let i_next = (i + 1) % N;
///             simplices.push(vec![i + t * N, i_next + t * N, i + t_next * N]);
///             simplices.push(vec![i_next + t * N, i_next + t_next * N, i + t_next * N]);
///         }
///     }
///
///     let result = build_toroidal_delaunay2(&vertices, &simplices, [1.0, 1.0]);
///     assert_matches!(
///         result,
///         Err(CdtError::DelaunayGenerationFailed {
///             failure: DelaunayGenerationFailure::Upstream { ref detail, .. },
///             ..
///         }) if detail.contains("Explicit non-Euclidean connectivity")
///     );
///     Ok(())
/// }
/// ```
pub fn build_toroidal_delaunay2(
    coords_with_data: &[([f64; 2], u32)],
    simplices: &[Vec<usize>],
    domain: [f64; 2],
) -> CdtResult<DelaunayTriangulation2D> {
    validate_toroidal_domain(domain)?;

    build_delaunay2_with_topology(
        coords_with_data,
        simplices,
        TopologyGuarantee::Pseudomanifold,
        GlobalTopology::try_toroidal(domain, ToroidalConstructionMode::Explicit).map_err(|e| {
            invalid_generation_parameters(
                GenerationParameterIssue::InvalidToroidalDomain,
                format!("{domain:?}: {e}"),
                "finite and positive periods",
            )
        })?,
    )
}

/// Builds a periodic 2D toroidal Delaunay triangulation from coordinate-data pairs.
///
/// This uses the upstream periodic image-point constructor rather than explicit
/// simplex assembly. The builder requests [`TopologyGuarantee::PLManifold`], so
/// the resulting toroidal mesh is suitable for the full Delaunay Level 1-5
/// validation path exposed by
/// [`DelaunayBackend::validate_delaunay`](crate::geometry::backends::delaunay::DelaunayBackend::validate_delaunay).
///
/// # Errors
///
/// Returns [`crate::CdtError::InvalidGenerationParameters`] if any coordinate is
/// non-finite or either toroidal period is non-finite/non-positive. Returns
/// [`crate::CdtError::VertexBuildFailed`] if a vertex cannot be constructed, or
/// [`crate::CdtError::DelaunayGenerationFailed`] if upstream periodic Delaunay
/// construction rejects the point set.
///
/// # Examples
///
/// Build the minimal 3 × 3 periodic toroidal lattice used by
/// [`CdtTriangulation::from_toroidal_cdt`](crate::CdtTriangulation::from_toroidal_cdt)
/// and validate it with the upstream Level 1-5 checks:
///
/// ```
/// use causal_triangulations::{CdtError, CdtResult, DelaunayValidationLevel};
/// use causal_triangulations::prelude::geometry::*;
///
/// fn main() -> CdtResult<()> {
///     const N: usize = 3;
///     const LABELS: [u32; 3] = [0, 1, 2];
///     const T: usize = LABELS.len();
///     let mut vertices: Vec<([f64; 2], u32)> = Vec::with_capacity(N * T);
///
///     for (t, label) in LABELS.into_iter().enumerate() {
///         let phase = std::f64::consts::TAU * t as f64 / T as f64;
///         let slice_offset = phase.sin() / 32.0;
///         for i in 0..N {
///             #[allow(clippy::cast_precision_loss)]
///             let coord = [
///                 (i as f64 + slice_offset).rem_euclid(N as f64),
///                 t as f64,
///             ];
///             vertices.push((coord, label));
///         }
///     }
///
///     let dt = build_periodic_toroidal_delaunay2(&vertices, [3.0, 3.0])?;
///     assert_eq!(dt.number_of_vertices(), N * T);
///     assert_eq!(dt.number_of_simplices(), 2 * N * T);
///
///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
///         CdtError::DelaunayValidationFailed {
///             level: DelaunayValidationLevel::Five,
///             detail: err.to_string(),
///         }
///     })?;
///     backend.validate_delaunay().map_err(|err| CdtError::DelaunayValidationFailed {
///         level: DelaunayValidationLevel::Five,
///         detail: err.to_string(),
///     })?;
///     Ok(())
/// }
/// ```
pub fn build_periodic_toroidal_delaunay2(
    coords_with_data: &[([f64; 2], u32)],
    domain: [f64; 2],
) -> CdtResult<DelaunayTriangulation2D> {
    validate_toroidal_domain(domain)?;
    validate_explicit_coordinates(coords_with_data)?;

    let vertices: Vec<_> = coords_with_data
        .iter()
        .enumerate()
        .map(|(i, (coord, data))| {
            Vertex::<u32, 2>::try_new_with_data(*coord, *data).map_err(|e| {
                CdtError::VertexBuildFailed {
                    context: format!("periodic toroidal vertex {i}"),
                    underlying_error: e.to_string(),
                }
            })
        })
        .collect::<CdtResult<Vec<_>>>()?;

    let vertex_count = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
    let coordinate_range = coords_with_data
        .iter()
        .flat_map(|(c, _)| c.iter().copied())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });

    DelaunayTriangulationBuilder::new(&vertices)
        .simplex_data_type::<i32>()
        .try_toroidal(domain)
        .map_err(|e| CdtError::DelaunayGenerationFailed {
            vertex_count,
            coordinate_range,
            attempt: 1,
            failure: DelaunayGenerationFailure::Upstream {
                stage: DelaunayGenerationStage::TriangulationConstruction,
                detail: e.to_string(),
            },
        })?
        .topology_guarantee(TopologyGuarantee::PLManifold)
        .build()
        .map_err(|e| CdtError::DelaunayGenerationFailed {
            vertex_count,
            coordinate_range,
            attempt: 1,
            failure: DelaunayGenerationFailure::Upstream {
                stage: DelaunayGenerationStage::TriangulationConstruction,
                detail: e.to_string(),
            },
        })
}

// =========================================================================
// Test helpers (panicking convenience wrappers, compiled only during tests)
// =========================================================================

/// Generates a random Delaunay triangulation. Panics on failure.
#[cfg(test)]
#[must_use]
pub(crate) fn random_delaunay2(
    number_of_vertices: u32,
    coordinate_range: (f64, f64),
) -> DelaunayTriangulation2D {
    generate_delaunay2(number_of_vertices, coordinate_range, None).unwrap_or_else(|_| {
        panic!(
            "Failed to generate random Delaunay triangulation with {number_of_vertices} vertices"
        )
    })
}

/// Generates a seeded Delaunay triangulation. Panics on failure.
#[cfg(test)]
#[must_use]
pub(crate) fn seeded_delaunay2(
    number_of_vertices: u32,
    coordinate_range: (f64, f64),
    seed: u64,
) -> DelaunayTriangulation2D {
    generate_delaunay2(number_of_vertices, coordinate_range, Some(seed)).unwrap_or_else(
        |_| {
            panic!(
                "Failed to generate seeded Delaunay triangulation with {number_of_vertices} vertices and seed {seed}"
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::DelaunayBackend2D;
    use crate::geometry::traits::TriangulationQuery;
    use std::assert_matches;
    use std::collections::HashMap;

    /// Produces an order-independent snapshot of vertices and simplex connectivity for seeded tests.
    fn triangulation_signature(dt: &DelaunayTriangulation2D) -> (Vec<String>, Vec<Vec<String>>) {
        let mut vertex_coords: Vec<_> = dt
            .vertices()
            .map(|(_, vertex)| format!("{:?}", vertex.point().coords()))
            .collect();
        vertex_coords.sort();

        let coord_by_key: HashMap<_, _> = dt
            .vertices()
            .map(|(key, vertex)| (key, format!("{:?}", vertex.point().coords())))
            .collect();

        let mut simplices: Vec<_> = dt
            .simplices()
            .map(|(_, simplex)| {
                let mut vertices: Vec<_> = simplex
                    .vertices()
                    .iter()
                    .map(|key| {
                        coord_by_key
                            .get(key)
                            .cloned()
                            .expect("simplex vertices should refer to live vertices")
                    })
                    .collect();
                vertices.sort();
                vertices
            })
            .collect();
        simplices.sort();

        (vertex_coords, simplices)
    }

    fn assert_coordinates_in_range(dt: &DelaunayTriangulation2D, coordinate_range: (f64, f64)) {
        for (_, vertex) in dt.vertices() {
            for coordinate in vertex.point().coords() {
                assert!(
                    (coordinate_range.0..=coordinate_range.1).contains(coordinate),
                    "coordinate {coordinate} should lie in {coordinate_range:?}"
                );
            }
        }
    }

    #[test]
    fn test_generate_delaunay2_vertex_build_error_context() {
        let error = generate_delaunay2_vertex_build_error(5, "missing point".to_string());

        assert_matches!(
            error,
            CdtError::VertexBuildFailed {
                ref context,
                ref underlying_error,
            } if context == "generate_delaunay2(5 vertices)"
                && underlying_error == "missing point"
        );
    }

    #[test]
    fn test_build_delaunay2_from_simplices_single_triangle() {
        // Default topology (PL-manifold + Euclidean) should accept a single
        // triangle with the standard 0-1 strip labeling.
        let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let simplices = vec![vec![0, 1, 2]];

        let dt = build_delaunay2_from_simplices(&vertices, &simplices)
            .expect("single-triangle explicit mesh should build with defaults");
        assert_eq!(dt.number_of_vertices(), 3);
        assert_eq!(dt.number_of_simplices(), 1);
    }

    #[test]
    fn explicit_non_overlapping_faces_pass_embedding_validation() {
        let vertices = [
            ([0.0, 0.0], 0_u32),
            ([1.0, 0.0], 0),
            ([0.0, 1.0], 1),
            ([1.0, 1.0], 1),
        ];
        let simplices = vec![vec![0, 1, 2], vec![1, 3, 2]];

        let dt = build_delaunay2_from_simplices(&vertices, &simplices)
            .expect("non-overlapping explicit faces should build");

        dt.as_triangulation()
            .validate_realization()
            .expect("non-overlapping explicit faces should pass Levels 1-4");
    }

    #[test]
    fn explicit_crossing_edges_fail_embedding_validation() {
        let vertices = [
            ([1.850_341_970_997_476_4, 3.808_736_162_215_642_4], 0_u32),
            ([-1.705_108_018_057_679, 3.541_228_835_829_82], 0),
            ([-1.151_312_061_387_885_3, 0.227_299_663_756_810_77], 0),
            ([0.478_746_443_632_698_25, 2.055_189_799_064_582], 0),
            ([-1.383_321_070_900_029, -1.797_028_018_114_396_3], 0),
            ([3.030_089_610_961_752_6, 2.181_406_554_808_236_6], 0),
        ];
        let simplices = vec![
            vec![0, 2, 3],
            vec![5, 3, 2],
            vec![4, 3, 1],
            vec![3, 4, 0],
            vec![3, 5, 1],
        ];

        let error = build_delaunay2_from_simplices(&vertices, &simplices)
            .expect_err("crossing non-adjacent edges must fail embedding validation");
        let CdtError::DelaunayGenerationFailed {
            failure: DelaunayGenerationFailure::Upstream { detail, .. },
            ..
        } = error
        else {
            panic!("expected a Delaunay generation failure");
        };
        assert!(
            detail.contains("intersect outside their shared face"),
            "unexpected crossing-edge diagnostic: {detail}"
        );
    }

    #[test]
    fn explicit_degenerate_triangle_fails_embedding_validation() {
        let vertices = [([0.0, 0.0], 0_u32), ([1.0, 0.0], 0), ([2.0, 0.0], 1)];
        let simplices = vec![vec![0, 1, 2]];

        let error = build_delaunay2_from_simplices(&vertices, &simplices)
            .expect_err("collinear triangle must fail embedding validation");
        let CdtError::DelaunayGenerationFailed {
            failure: DelaunayGenerationFailure::Upstream { detail, .. },
            ..
        } = error
        else {
            panic!("expected a Delaunay generation failure");
        };
        assert!(
            detail.to_ascii_lowercase().contains("degenerate"),
            "unexpected degenerate-triangle diagnostic: {detail}"
        );
    }

    #[test]
    fn layered_builder_preserves_embedding_failure_context() {
        let vertices = [([0.0, 0.0], 0_u32), ([1.0, 0.0], 0), ([2.0, 0.0], 1)];
        let simplices = vec![vec![0, 1, 2]];

        let error = build_layered_delaunay2_from_simplices(&vertices, &simplices)
            .expect_err("collinear layered connectivity must fail realization validation");

        assert_matches!(
            error,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 3,
                coordinate_range: (0.0, 2.0),
                attempt: 1,
                failure: DelaunayGenerationFailure::Upstream {
                    stage: DelaunayGenerationStage::TriangulationConstruction,
                    ref detail,
                },
            } if detail.to_ascii_lowercase().contains("degenerate")
        );
    }

    #[test]
    fn test_build_delaunay2_from_simplices_rejects_bad_index() {
        // Simplex references vertex 3 which doesn't exist (only indices 0..3 are valid).
        let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let simplices = vec![vec![0, 1, 3]];

        let result = build_delaunay2_from_simplices(&vertices, &simplices);
        assert_matches!(
            result,
            Err(CdtError::DelaunayGenerationFailed { .. }),
            "explicit builder must reject out-of-bounds vertex indices"
        );
    }

    #[test]
    fn layered_builder_preserves_invalid_index_context() {
        let vertices = [([0.0, 0.0], 0_u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let simplices = vec![vec![0, 1, 3]];

        let error = build_layered_delaunay2_from_simplices(&vertices, &simplices)
            .expect_err("layered connectivity must reject an out-of-bounds vertex index");

        assert_matches!(
            error,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 3,
                coordinate_range: (0.0, 1.0),
                attempt: 1,
                failure: DelaunayGenerationFailure::Upstream {
                    stage: DelaunayGenerationStage::TriangulationConstruction,
                    ref detail,
                },
            } if !detail.is_empty()
        );
    }

    #[test]
    fn test_build_delaunay2_with_topology_euclidean() {
        // Same single-triangle mesh, but with explicit topology metadata.
        let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let simplices = vec![vec![0, 1, 2]];

        let dt = build_delaunay2_with_topology(
            &vertices,
            &simplices,
            TopologyGuarantee::DEFAULT,
            GlobalTopology::Euclidean,
        )
        .expect("single-triangle explicit mesh with explicit topology should build");
        assert_eq!(dt.number_of_vertices(), 3);
        assert_eq!(dt.number_of_simplices(), 1);
    }

    #[test]
    fn test_explicit_toroidal_simplices_are_rejected() {
        // A real 3×3 toroidal mesh: V=9, F=18, E=27, χ=0.
        const N: usize = 3;
        const T: usize = 3;
        let mut vertices: Vec<([f64; 2], u32)> = Vec::with_capacity(N * T);
        for t in 0..T {
            for i in 0..N {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "small deterministic test indices are converted to normalized f64 coordinates"
                )]
                let coord = [i as f64 / N as f64, t as f64 / T as f64];
                let label = u32::try_from(t).expect("slice index fits in u32");
                vertices.push((coord, label));
            }
        }
        let mut simplices: Vec<Vec<usize>> = Vec::with_capacity(2 * N * T);
        for t in 0..T {
            let t_next = (t + 1) % T;
            for i in 0..N {
                let i_next = (i + 1) % N;
                simplices.push(vec![i + t * N, i_next + t * N, i + t_next * N]);
                simplices.push(vec![i_next + t * N, i_next + t_next * N, i + t_next * N]);
            }
        }

        let error = build_toroidal_delaunay2(&vertices, &simplices, [1.0, 1.0])
            .expect_err("explicit toroidal topology should report upstream limitation");
        assert_matches!(
            error,
            CdtError::DelaunayGenerationFailed {
                vertex_count: 9,
                failure: DelaunayGenerationFailure::Upstream { ref detail, .. },
                ..
            } if detail.contains(
                "Explicit non-Euclidean connectivity is not supported for Toroidal"
            ),
            "explicit toroidal mesh should fail with the upstream topology limitation"
        );
    }

    #[test]
    fn test_build_periodic_toroidal_delaunay2_3x3_validates_level_1_to_5() {
        const N: usize = 3;
        const T: usize = 3;
        const DOMAIN: [f64; 2] = [3.0, 3.0];
        let mut vertices: Vec<([f64; 2], u32)> = Vec::with_capacity(N * T);
        for t in 0..T {
            #[expect(
                clippy::cast_precision_loss,
                reason = "small deterministic test indices are converted to f64 phases"
            )]
            let phase = std::f64::consts::TAU * t as f64 / T as f64;
            let slice_offset = phase.sin() / 32.0;
            for i in 0..N {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "small deterministic test indices are converted to f64 lattice coordinates"
                )]
                let coord = [(i as f64 + slice_offset).rem_euclid(N as f64), t as f64];
                let label = u32::try_from(t).expect("slice index fits in u32");
                vertices.push((coord, label));
            }
        }

        let dt = build_periodic_toroidal_delaunay2(&vertices, DOMAIN)
            .expect("periodic 3×3 toroidal mesh should build");
        assert_eq!(dt.number_of_vertices(), N * T);
        assert_eq!(dt.number_of_simplices(), 2 * N * T);

        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("periodic toroidal mesh should validate");
        backend
            .validate_delaunay()
            .expect("periodic toroidal mesh must pass upstream Level 1-5 validation");
    }

    #[test]
    fn test_build_periodic_toroidal_delaunay2_rejects_invalid_domain() {
        let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.0, 1.0], 1)];

        for (domain, expected_value) in [
            ([0.0, 3.0], "axis 0 period 0"),
            ([-1.0, 3.0], "axis 0 period -1"),
            ([3.0, f64::NAN], "axis 1 period NaN"),
            ([f64::INFINITY, 3.0], "axis 0 period inf"),
        ] {
            let result = build_periodic_toroidal_delaunay2(&vertices, domain);
            assert_matches!(
                result,
                Err(CdtError::InvalidGenerationParameters {
                    ref issue,
                    ref provided_value,
                    ref expected_range,
                }) if *issue == GenerationParameterIssue::InvalidToroidalDomain
                    && provided_value == expected_value
                    && expected_range == "finite and positive periods",
                "invalid periodic toroidal domain {domain:?} should be rejected"
            );
        }
    }

    #[test]
    fn test_build_periodic_toroidal_delaunay2_rejects_non_finite_coordinate() {
        let vertices = [
            ([0.0, 0.0], 0u32),
            ([1.0, f64::NEG_INFINITY], 0),
            ([0.0, 1.0], 1),
        ];

        let result = build_periodic_toroidal_delaunay2(&vertices, [3.0, 3.0]);
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::NonFiniteVertexCoordinate
                && provided_value == "vertex 1 axis 1 = -inf"
                && expected_range == "finite coordinate values",
            "periodic toroidal non-finite coordinate should be rejected"
        );
    }

    #[test]
    fn test_generate_delaunay2_valid_parameters() {
        let dt = generate_delaunay2(4, (0.0, 10.0), None)
            .expect("Should successfully generate triangulation with valid parameters");
        assert_eq!(dt.number_of_vertices(), 4, "Should have 4 vertices");
        assert!(
            dt.number_of_simplices() > 0,
            "Should have at least one simplex"
        );
    }

    #[test]
    fn test_generate_delaunay2_with_seed() {
        let seed = 12345;
        let result1 = generate_delaunay2(4, (0.0, 10.0), Some(seed));
        let result2 = generate_delaunay2(4, (0.0, 10.0), Some(seed));

        assert!(result1.is_ok(), "First generation should succeed");
        assert!(result2.is_ok(), "Second generation should succeed");

        let dt1 = result1.unwrap();
        let dt2 = result2.unwrap();

        // With the same seed, should produce identical triangulations
        assert_eq!(
            dt1.number_of_vertices(),
            dt2.number_of_vertices(),
            "Should have same vertex count"
        );
        assert_eq!(
            dt1.number_of_simplices(),
            dt2.number_of_simplices(),
            "Should have same simplex count"
        );
        assert_eq!(
            triangulation_signature(&dt1),
            triangulation_signature(&dt2),
            "Seeded generation should produce identical vertex coordinates and simplex connectivity"
        );
    }

    #[test]
    fn test_generate_delaunay2_insufficient_vertices() {
        let result = generate_delaunay2(2, (0.0, 10.0), None);
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
    fn test_generate_delaunay2_invalid_range() {
        let result = generate_delaunay2(4, (10.0, 5.0), None);
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InvalidCoordinateRange,
                ref provided_value,
                ref expected_range,
            }) if provided_value == "[10, 5]" && expected_range == "finite min < max"
        );
    }

    #[test]
    fn test_generate_delaunay2_equal_range() {
        let result = generate_delaunay2(4, (5.0, 5.0), None);
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InvalidCoordinateRange,
                ref provided_value,
                ref expected_range,
            }) if provided_value == "[5, 5]" && expected_range == "finite min < max"
        );
    }

    #[test]
    fn test_generate_delaunay2_rejects_non_finite_range() {
        for range in [(f64::NAN, 1.0), (0.0, f64::INFINITY)] {
            let result = generate_delaunay2(4, range, None);
            assert_matches!(
                result,
                Err(CdtError::InvalidGenerationParameters {
                    ref issue,
                    ref expected_range,
                    ..
                }) if *issue == GenerationParameterIssue::InvalidCoordinateRange
                    && expected_range == "finite min < max",
                "non-finite range {range:?} should be rejected"
            );
        }
    }

    #[test]
    fn test_build_delaunay2_with_data_rejects_non_finite_coordinate() {
        let vertices = [([0.0, 0.0], 0u32), ([1.0, f64::NAN], 0), ([0.5, 1.0], 1)];

        let result = build_delaunay2_with_data(&vertices);
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::NonFiniteVertexCoordinate
                && provided_value == "vertex 1 axis 1 = NaN"
                && expected_range == "finite coordinate values",
            "explicit non-finite coordinate should be rejected"
        );
    }

    #[test]
    fn test_build_delaunay2_from_simplices_rejects_non_finite_coordinate() {
        let vertices = [
            ([0.0, 0.0], 0u32),
            ([1.0, 0.0], 0),
            ([0.5, f64::NEG_INFINITY], 1),
        ];
        let simplices = vec![vec![0, 1, 2]];

        let result = build_delaunay2_from_simplices(&vertices, &simplices);
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::NonFiniteVertexCoordinate
                && provided_value == "vertex 2 axis 1 = -inf"
                && expected_range == "finite coordinate values",
            "delegating explicit-simplex builder should reject non-finite coordinates"
        );
    }

    #[test]
    fn test_build_delaunay2_with_topology_rejects_non_finite_coordinate() {
        let vertices = [
            ([0.0, 0.0], 0u32),
            ([f64::INFINITY, 0.0], 0),
            ([0.5, 1.0], 1),
        ];
        let simplices = vec![vec![0, 1, 2]];

        let result = build_delaunay2_with_topology(
            &vertices,
            &simplices,
            TopologyGuarantee::DEFAULT,
            GlobalTopology::Euclidean,
        );
        assert_matches!(
            result,
            Err(CdtError::InvalidGenerationParameters {
                ref issue,
                ref provided_value,
                ref expected_range,
            }) if *issue == GenerationParameterIssue::NonFiniteVertexCoordinate
                && provided_value == "vertex 1 axis 0 = inf"
                && expected_range == "finite coordinate values",
            "explicit non-finite topology coordinate should be rejected"
        );
    }

    #[test]
    fn test_build_toroidal_delaunay2_rejects_invalid_domain() {
        let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let simplices = vec![vec![0, 1, 2]];

        for (domain, expected_value) in [
            ([0.0, 1.0], "axis 0 period 0"),
            ([-1.0, 1.0], "axis 0 period -1"),
            ([1.0, f64::NAN], "axis 1 period NaN"),
            ([f64::INFINITY, 1.0], "axis 0 period inf"),
        ] {
            let result = build_toroidal_delaunay2(&vertices, &simplices, domain);
            assert_matches!(
                result,
                Err(CdtError::InvalidGenerationParameters {
                    ref issue,
                    ref provided_value,
                    ref expected_range,
                }) if *issue == GenerationParameterIssue::InvalidToroidalDomain
                    && provided_value == expected_value
                    && expected_range == "finite and positive periods",
                "invalid domain {domain:?} should be rejected"
            );
        }
    }

    #[test]
    fn test_invalid_toroidal_domain_display_is_actionable() {
        let vertices = [([0.0, 0.0], 0u32), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let simplices = vec![vec![0, 1, 2]];

        let error = build_toroidal_delaunay2(&vertices, &simplices, [-1.0, 1.0])
            .expect_err("negative toroidal period should be rejected");
        assert_eq!(
            error.to_string(),
            "Invalid triangulation parameters: Invalid toroidal domain (got: axis 0 period -1, expected: finite and positive periods)"
        );
    }

    #[test]
    fn test_generate_delaunay2_various_sizes() {
        let test_cases = [(3, "minimal"), (5, "small"), (10, "medium"), (20, "large")];

        for (vertex_count, description) in test_cases {
            let dt = generate_delaunay2(
                vertex_count,
                (0.0, 100.0),
                Some(u64::from(vertex_count)),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "Should generate {description} triangulation with {vertex_count} vertices: {error}"
                )
            });
            assert_eq!(
                dt.number_of_vertices(),
                vertex_count as usize,
                "Should have {vertex_count} vertices for {description} triangulation"
            );
            assert!(
                dt.number_of_simplices() > 0,
                "Should have at least one simplex for {description} triangulation"
            );
        }
    }

    #[test]
    fn test_generate_delaunay2_different_ranges() {
        let ranges = [(0.0, 1.0), (-10.0, 10.0), (100.0, 200.0), (-50.0, 0.0)];

        for (index, range) in ranges.into_iter().enumerate() {
            let seed = u64::try_from(index).expect("range index should fit u64") + 100;
            let dt = generate_delaunay2(4, range, Some(seed)).unwrap_or_else(|error| {
                panic!("Should generate triangulation with range {range:?}: {error}")
            });
            assert_eq!(dt.number_of_vertices(), 4, "Should have 4 vertices");
            assert_coordinates_in_range(&dt, range);
        }
    }

    #[test]
    fn test_random_delaunay2_success() {
        let dt = random_delaunay2(5, (0.0, 10.0));
        assert_eq!(dt.number_of_vertices(), 5, "Should have 5 vertices");
        assert!(
            dt.number_of_simplices() > 0,
            "Should have at least one simplex"
        );
    }

    #[test]
    fn test_seeded_delaunay2_various_sizes() {
        let sizes = [3, 4, 6, 8, 12];

        for size in sizes {
            let dt = seeded_delaunay2(size, (0.0, 50.0), 200 + u64::from(size));
            assert_eq!(
                dt.number_of_vertices(),
                size as usize,
                "Should have {size} vertices"
            );
            assert!(
                dt.number_of_simplices() > 0,
                "Should have simplices for size {size}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "Failed to generate random Delaunay triangulation with 2 vertices")]
    fn test_random_delaunay2_panic_insufficient_vertices() {
        let _ = random_delaunay2(2, (0.0, 10.0));
    }

    #[test]
    #[should_panic(expected = "Failed to generate random Delaunay triangulation with 4 vertices")]
    fn test_random_delaunay2_panic_invalid_range() {
        let _ = random_delaunay2(4, (10.0, 5.0));
    }

    #[test]
    fn test_seeded_delaunay2_deterministic() {
        let seed = 42;
        let dt1 = seeded_delaunay2(6, (0.0, 20.0), seed);
        let dt2 = seeded_delaunay2(6, (0.0, 20.0), seed);

        // Should produce identical results
        assert_eq!(
            dt1.number_of_vertices(),
            dt2.number_of_vertices(),
            "Should have same vertex count"
        );
        assert_eq!(
            dt1.number_of_simplices(),
            dt2.number_of_simplices(),
            "Should have same simplex count"
        );
        assert_eq!(triangulation_signature(&dt1), triangulation_signature(&dt2));

        // Verify expected properties
        assert_eq!(dt1.number_of_vertices(), 6, "Should have 6 vertices");
        assert!(dt1.number_of_simplices() > 0, "Should have simplices");
    }

    #[test]
    fn test_seeded_delaunay2_different_seeds() {
        let dt1 = seeded_delaunay2(5, (0.0, 10.0), 123);
        let dt2 = seeded_delaunay2(5, (0.0, 10.0), 456);

        // Both should succeed and have same vertex count
        assert_eq!(dt1.number_of_vertices(), 5, "First should have 5 vertices");
        assert_eq!(dt2.number_of_vertices(), 5, "Second should have 5 vertices");
        assert_ne!(
            triangulation_signature(&dt1),
            triangulation_signature(&dt2),
            "different seeds should change generated coordinates or connectivity"
        );
    }

    #[test]
    fn test_seeded_delaunay2_various_seeds() {
        let seeds = [1, 100, 1000, u64::MAX];

        for seed in seeds {
            let dt = seeded_delaunay2(4, (-5.0, 5.0), seed);
            assert_eq!(
                dt.number_of_vertices(),
                4,
                "Should have 4 vertices with seed {seed}"
            );
            assert!(
                dt.number_of_simplices() > 0,
                "Should have simplices with seed {seed}"
            );
        }
    }

    #[test]
    #[should_panic(
        expected = "Failed to generate seeded Delaunay triangulation with 1 vertices and seed 42"
    )]
    fn test_seeded_delaunay2_panic_insufficient_vertices() {
        let _ = seeded_delaunay2(1, (0.0, 10.0), 42);
    }

    #[test]
    #[should_panic(
        expected = "Failed to generate seeded Delaunay triangulation with 5 vertices and seed 123"
    )]
    fn test_seeded_delaunay2_panic_invalid_range() {
        let _ = seeded_delaunay2(5, (15.0, 10.0), 123);
    }

    #[test]
    fn test_euler_characteristic_properties() {
        let dt = seeded_delaunay2(5, (0.0, 10.0), 53);
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("seed 53 should produce a valid backend");

        assert_eq!(backend.vertex_count(), 5);
        assert_eq!(backend.edge_count(), 8);
        assert_eq!(backend.face_count(), 4);
        assert_eq!(backend.euler_characteristic(), 1);
    }

    #[test]
    fn test_coordinate_range_bounds() {
        // Test representative finite coordinate ranges.  Astronomical f64
        // spans can violate robust predicate preconditions before they are
        // meaningful geometry fixtures.
        let ranges = [
            (-1.0e6, 1.0e6), // Broad symmetric range
            (-1000.0, 1000.0),
            (0.001, 0.002),
            (-0.5, 0.5),
        ];

        for range in ranges {
            let dt = generate_delaunay2(4, range, Some(789)).unwrap_or_else(|error| {
                panic!("Should handle coordinate range {range:?}: {error}")
            });
            assert_eq!(dt.number_of_vertices(), 4);
            assert_coordinates_in_range(&dt, range);
        }
    }

    #[test]
    fn test_build_delaunay2_with_data_empty_input() {
        // Empty input should produce a valid (but empty) triangulation
        // or fail gracefully — either way, no panic.
        let result = build_delaunay2_with_data(&[]);
        // The delaunay builder may accept or reject zero vertices;
        // we just verify it doesn't panic.
        match result {
            Ok(dt) => assert_eq!(dt.number_of_vertices(), 0),
            Err(error) => assert_matches!(
                error,
                CdtError::DelaunayGenerationFailed {
                    vertex_count: 0,
                    ..
                }
            ),
        }
    }

    #[test]
    fn test_build_delaunay2_with_data_single_point() {
        // A single point is insufficient for a triangulation.
        let result = build_delaunay2_with_data(&[([0.0, 0.0], 0)]);
        // May succeed with degenerate DT or fail — no panic.
        match result {
            Ok(dt) => assert_eq!(dt.number_of_vertices(), 1),
            Err(error) => assert_matches!(
                error,
                CdtError::DelaunayGenerationFailed {
                    vertex_count: 1,
                    ..
                }
            ),
        }
    }

    #[test]
    fn test_build_delaunay2_with_data_valid_triangle() {
        let coords = [([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)];
        let dt = build_delaunay2_with_data(&coords)
            .expect("Should build triangulation from 3 non-degenerate points");
        assert_eq!(dt.number_of_vertices(), 3);
        assert_eq!(dt.number_of_simplices(), 1);
    }

    #[test]
    fn test_seeded_reproducibility_multiple_calls() {
        // Test that multiple calls with the same seed produce identical results
        let seed = 999;
        let params = (7, (-10.0, 10.0));

        let results: Vec<_> = (0..3)
            .map(|_| seeded_delaunay2(params.0, params.1, seed))
            .collect();

        // All results should have the same structure
        for (i, dt) in results.iter().enumerate() {
            assert_eq!(
                dt.number_of_vertices(),
                7,
                "Result {i} should have 7 vertices"
            );
            assert!(
                dt.number_of_simplices() > 0,
                "Result {i} should have simplices"
            );
        }

        let first_signature = triangulation_signature(&results[0]);

        for (i, dt) in results.iter().enumerate().skip(1) {
            assert_eq!(
                triangulation_signature(dt),
                first_signature,
                "Result {i} coordinates and connectivity should match the first result"
            );
        }
    }
}
