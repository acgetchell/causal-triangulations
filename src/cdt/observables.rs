#![forbid(unsafe_code)]

//! Observable estimators for CDT triangulations.
//!
//! This module contains user-facing analysis routines that measure fixed
//! triangulations or simulation outputs without owning the simulation loop.

use crate::geometry::CdtTriangulation2D;
use crate::geometry::traits::TriangulationQuery;
use num_traits::cast::NumCast;
use std::collections::{HashMap, VecDeque};
use std::mem;

const MIN_SPECTRAL_DIFFUSION_STEP: usize = 2;
const MAX_SPECTRAL_DIFFUSION_STEP: usize = 16;
const SPECTRAL_SELF_LOOP_PROBABILITY: f64 = 0.5;

/// Estimates the Hausdorff dimension from dual-graph geodesic ball growth.
///
/// The estimator computes average ball volumes `<V(r)>` on the dual graph of
/// the supplied [`CdtTriangulation2D`] and returns the slope of a simple log-log
/// least squares fit to `<V(r)> ~ r^d`.
/// Here "dual graph" means the combinatorial face-adjacency graph of the
/// triangulation, not a geometric Voronoi tessellation or circumcenter dual.
///
/// Average ball volumes use a reachable-radius convention: a root contributes
/// to radius `r` only when at least one dual-graph face is reachable at that
/// distance.  The implementation precomputes face adjacency once, then runs a
/// breadth-first search from every face, so its time complexity is
/// `O(F * (F + E_d))` for `F` faces and `E_d` dual edges.
///
/// Returns `None` when the triangulation is too small, disconnected data leaves
/// fewer than two usable radii, or live face adjacency cannot be resolved.
///
/// # References
///
/// This observable follows the CDT use of spatial volume profiles and
/// graph-geodesic dimensional estimators discussed in:
///
/// - J. Ambjørn, J. Jurkiewicz, and R. Loll, "Reconstructing the Universe",
///   *Physical Review D* 72, 064014 (2005),
///   DOI: <https://doi.org/10.1103/PhysRevD.72.064014>.
/// - J. Ambjørn, T. Budd, and Y. Watabiki, "Scale-dependent Hausdorff
///   dimensions in 2d gravity", *Physics Letters B* 736, 339-343 (2014),
///   DOI: <https://doi.org/10.1016/j.physletb.2014.07.047>.
///
/// The complete bibliography is maintained in the repository `REFERENCES.md`.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::CdtResult;
/// use causal_triangulations::prelude::observables::*;
///
/// fn main() -> CdtResult<()> {
///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
///     assert!(estimate_hausdorff_dimension(&tri).is_some_and(f64::is_finite));
///     Ok(())
/// }
/// ```
#[must_use]
pub fn estimate_hausdorff_dimension(triangulation: &CdtTriangulation2D) -> Option<f64> {
    let ball_volumes = average_dual_ball_volumes(triangulation)?;
    fit_log_log_slope(&ball_volumes)
}

/// Estimates the spectral dimension from dual-graph diffusion return probability.
///
/// The estimator runs a discrete random walk on the combinatorial dual graph of
/// the supplied [`CdtTriangulation2D`], averages the probability of returning to
/// the starting face after diffusion time `sigma`, and fits the slope of
/// `log(P(sigma))` against `log(sigma)`. The returned value is `-2` times that
/// slope, matching the CDT convention `P(sigma) ~ sigma^(-d_s/2)`.
///
/// Here "dual graph" means the combinatorial face-adjacency graph of the
/// triangulation, not a geometric Voronoi tessellation or circumcenter dual.
/// The implementation precomputes face adjacency once, then evolves one
/// probability vector per root face up to a bounded diffusion time, so its time
/// complexity is `O(S * F * (F + E_d))` for `S` diffusion steps, `F` faces, and
/// `E_d` dual edges.
///
/// Returns `None` when the triangulation is too small, fewer than two positive
/// return-probability samples are available for the fit, the fitted dimension is
/// not positive, or live face adjacency cannot be resolved.
///
/// # References
///
/// This observable follows the CDT diffusion-return estimator discussed in:
///
/// - J. Ambjørn, J. Jurkiewicz, and R. Loll, "The Spectral Dimension of the
///   Universe is Scale Dependent", *Physical Review Letters* 95, 171301 (2005),
///   DOI: <https://doi.org/10.1103/PhysRevLett.95.171301>.
///
/// The complete bibliography is maintained in the repository `REFERENCES.md`.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::errors::CdtResult;
/// use causal_triangulations::prelude::observables::*;
///
/// fn main() -> CdtResult<()> {
///     let tri = CdtTriangulation::from_toroidal_cdt(6, 6)?;
///     assert!(estimate_spectral_dimension(&tri).is_some_and(f64::is_finite));
///     Ok(())
/// }
/// ```
#[must_use]
pub fn estimate_spectral_dimension(triangulation: &CdtTriangulation2D) -> Option<f64> {
    let adjacency = dual_adjacency(triangulation)?;
    estimate_spectral_dimension_from_adjacency(&adjacency)
}

/// Builds the combinatorial face-adjacency graph for CDT dual observables.
fn dual_adjacency(triangulation: &CdtTriangulation2D) -> Option<Vec<Vec<usize>>> {
    let faces: Vec<_> = triangulation.geometry().faces().collect();
    if faces.len() < 2 {
        return Some(Vec::new());
    }

    let face_indices: HashMap<_, _> = faces
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, face)| (face, index))
        .collect();

    faces
        .iter()
        .map(|face| {
            let neighbors = triangulation.geometry().face_neighbors(face).ok()?;
            Some(
                neighbors
                    .into_iter()
                    .filter_map(|neighbor| face_indices.get(&neighbor).copied())
                    .collect(),
            )
        })
        .collect()
}

/// Computes average dual-graph ball volumes for each reachable graph radius.
fn average_dual_ball_volumes(triangulation: &CdtTriangulation2D) -> Option<Vec<f64>> {
    dual_adjacency(triangulation)
        .as_deref()
        .map(average_dual_ball_volumes_from_adjacency)
}

/// Computes reachable-radius average ball volumes from a dual adjacency list.
fn average_dual_ball_volumes_from_adjacency(adjacency: &[Vec<usize>]) -> Vec<f64> {
    if adjacency.len() < 2 {
        return Vec::new();
    }

    let mut sums = Vec::new();
    let mut counts = Vec::new();
    let mut distances = vec![None; adjacency.len()];
    let mut queue = VecDeque::new();
    let mut shell_counts = Vec::new();

    for root in 0..adjacency.len() {
        let Some(max_radius) = dual_graph_distances(adjacency, root, &mut distances, &mut queue)
        else {
            continue;
        };

        if shell_counts.len() <= max_radius {
            shell_counts.resize(max_radius + 1, 0);
        }
        shell_counts[..=max_radius].fill(0);
        for distance in distances.iter().flatten().copied() {
            shell_counts[distance] += 1;
        }

        if sums.len() <= max_radius {
            sums.resize(max_radius + 1, 0.0);
            counts.resize(max_radius + 1, 0_usize);
        }

        let mut ball_volume = 0_usize;
        for (radius, shell_count) in shell_counts
            .iter()
            .copied()
            .enumerate()
            .take(max_radius + 1)
        {
            ball_volume += shell_count;
            sums[radius] += NumCast::from(ball_volume).unwrap_or(0.0);
            counts[radius] += 1;
        }
    }

    sums.into_iter()
        .zip(counts)
        .map(|(sum, count)| {
            let count_f64 = NumCast::from(count).unwrap_or(1.0);
            sum / count_f64
        })
        .collect()
}

/// Breadth-first face distances from one dual-graph root.
fn dual_graph_distances(
    adjacency: &[Vec<usize>],
    root: usize,
    distances: &mut [Option<usize>],
    queue: &mut VecDeque<usize>,
) -> Option<usize> {
    if root >= adjacency.len() || distances.len() != adjacency.len() {
        return None;
    }

    distances.fill(None);
    queue.clear();
    distances[root] = Some(0);
    queue.push_back(root);

    let mut max_radius = 0;
    while let Some(index) = queue.pop_front() {
        let Some(distance) = distances[index] else {
            continue;
        };
        max_radius = max_radius.max(distance);
        for &neighbor in &adjacency[index] {
            if neighbor < distances.len() && distances[neighbor].is_none() {
                distances[neighbor] = Some(distance + 1);
                queue.push_back(neighbor);
            }
        }
    }

    Some(max_radius)
}

/// Fits the slope of `log(volume)` against `log(radius)`.
fn fit_log_log_slope(ball_volumes: &[f64]) -> Option<f64> {
    fit_linear_slope(
        ball_volumes
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(radius, &volume)| {
                // Exclude root-only samples: ln(1.0) = 0 biases the slope toward zero.
                if volume > 1.0 && volume.is_finite() {
                    let radius_f64: f64 = NumCast::from(radius)?;
                    Some((radius_f64.ln(), volume.ln()))
                } else {
                    None
                }
            }),
    )
}

/// Estimates spectral dimension from a precomputed dual adjacency list.
fn estimate_spectral_dimension_from_adjacency(adjacency: &[Vec<usize>]) -> Option<f64> {
    if adjacency.len() < 3 {
        return None;
    }

    let max_step = MAX_SPECTRAL_DIFFUSION_STEP.min(adjacency.len().saturating_sub(1));
    if max_step < MIN_SPECTRAL_DIFFUSION_STEP {
        return None;
    }

    let return_probabilities = average_return_probabilities(adjacency, max_step);
    fit_spectral_dimension(&return_probabilities)
}

/// Computes average random-walk return probabilities for diffusion steps.
fn average_return_probabilities(adjacency: &[Vec<usize>], max_step: usize) -> Vec<f64> {
    let node_count = adjacency.len();
    if node_count == 0 {
        return Vec::new();
    }

    let mut sums = vec![0.0; max_step + 1];
    let mut current = vec![0.0; node_count];
    let mut next = vec![0.0; node_count];

    for root in 0..node_count {
        current.fill(0.0);
        current[root] = 1.0;
        sums[0] += 1.0;

        for sum in sums.iter_mut().take(max_step + 1).skip(1) {
            next.fill(0.0);
            for (index, &probability) in current.iter().enumerate() {
                if probability <= 0.0 {
                    continue;
                }

                let stay = probability * SPECTRAL_SELF_LOOP_PROBABILITY;
                let move_probability =
                    probability.mul_add(-SPECTRAL_SELF_LOOP_PROBABILITY, probability);
                next[index] += stay;

                let live_neighbor_count = adjacency[index]
                    .iter()
                    .filter(|&&neighbor| neighbor < node_count)
                    .count();
                if live_neighbor_count == 0 {
                    next[index] += move_probability;
                    continue;
                }

                let neighbor_count = NumCast::from(live_neighbor_count).unwrap_or(1.0);
                let share = move_probability / neighbor_count;
                for neighbor in adjacency[index]
                    .iter()
                    .copied()
                    .filter(|&neighbor| neighbor < node_count)
                {
                    next[neighbor] += share;
                }
            }

            mem::swap(&mut current, &mut next);
            *sum += current[root];
        }
    }

    let node_count_f64 = NumCast::from(node_count).unwrap_or(1.0);
    sums.into_iter().map(|sum| sum / node_count_f64).collect()
}

/// Fits `d_s = -2 d log(P(sigma)) / d log(sigma)` from return probabilities.
fn fit_spectral_dimension(return_probabilities: &[f64]) -> Option<f64> {
    let slope = fit_linear_slope(
        return_probabilities
            .iter()
            .enumerate()
            .skip(MIN_SPECTRAL_DIFFUSION_STEP)
            .filter_map(|(step, &probability)| {
                if probability > 0.0 && probability < 1.0 && probability.is_finite() {
                    let step_f64: f64 = NumCast::from(step)?;
                    Some((step_f64.ln(), probability.ln()))
                } else {
                    None
                }
            }),
    )?;

    let dimension = -2.0 * slope;
    (dimension.is_finite() && dimension > 0.0).then_some(dimension)
}

/// Fits the slope of `y = a + bx` from streaming `(x, y)` samples.
fn fit_linear_slope(samples: impl IntoIterator<Item = (f64, f64)>) -> Option<f64> {
    let (count, x_total, y_total, square_total, product_total) = samples.into_iter().fold(
        (0_usize, 0.0, 0.0, 0.0, 0.0),
        |(count, x_total, y_total, square_total, product_total), (x, y)| {
            (
                count + 1,
                x_total + x,
                y_total + y,
                x.mul_add(x, square_total),
                x.mul_add(y, product_total),
            )
        },
    );

    if count < 2 {
        return None;
    }

    let count_f64: f64 = NumCast::from(count).unwrap_or(1.0);
    let denominator = count_f64.mul_add(square_total, -x_total * x_total);
    if denominator <= f64::EPSILON {
        return None;
    }

    Some(count_f64.mul_add(product_total, -x_total * y_total) / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::triangulation::CdtTriangulation;
    use approx::assert_relative_eq;

    #[test]
    fn hausdorff_estimate_uses_dual_graph_ball_growth() {
        let triangulation = CdtTriangulation::from_cdt_strip(4, 3).expect("create explicit strip");
        let estimate = estimate_hausdorff_dimension(&triangulation)
            .expect("strip dual graph should have enough radii for a fit");

        assert!(estimate.is_finite());
        assert!(estimate > 0.0);
    }

    #[test]
    fn hausdorff_estimate_returns_none_for_too_little_dual_growth() {
        let triangulation =
            CdtTriangulation::from_random_points(3, 1, 2).expect("create minimal triangulation");

        assert_eq!(estimate_hausdorff_dimension(&triangulation), None);
    }

    #[test]
    fn spectral_estimate_uses_dual_graph_return_probability() {
        let triangulation =
            CdtTriangulation::from_toroidal_cdt(6, 6).expect("create explicit torus");
        let estimate = estimate_spectral_dimension(&triangulation)
            .expect("torus dual graph should have enough diffusion data");

        assert!(estimate.is_finite());
        assert!(estimate > 0.0);
    }

    #[test]
    fn spectral_estimate_returns_none_for_too_little_diffusion_data() {
        let adjacency = vec![vec![1], vec![0]];

        assert_eq!(estimate_spectral_dimension_from_adjacency(&adjacency), None);
    }

    #[test]
    fn spectral_fit_recovers_power_law_return_probability() {
        let probabilities = vec![1.0, 0.75, 0.5, 1.0 / 3.0, 0.25, 0.2];

        let estimate = fit_spectral_dimension(&probabilities)
            .expect("decreasing power-law return probabilities should fit");

        assert_relative_eq!(estimate, 2.0, epsilon = 1e-12);
    }

    #[test]
    fn spectral_estimate_returns_none_for_stationary_isolated_graph() {
        let adjacency = vec![vec![], vec![], vec![]];

        let probabilities = average_return_probabilities(&adjacency, 4);

        for probability in probabilities {
            assert_relative_eq!(probability, 1.0);
        }
        assert_eq!(estimate_spectral_dimension_from_adjacency(&adjacency), None);
    }

    #[test]
    fn dual_ball_averages_use_reachable_radius_convention() {
        let path_graph = vec![vec![1], vec![0, 2], vec![1]];

        let ball_volumes = average_dual_ball_volumes_from_adjacency(&path_graph);

        assert_eq!(ball_volumes.len(), 3);
        assert_relative_eq!(ball_volumes[0], 1.0);
        assert_relative_eq!(ball_volumes[1], 7.0 / 3.0);
        assert_relative_eq!(ball_volumes[2], 3.0);
    }

    #[test]
    fn dual_graph_distances_ignore_out_of_bounds_neighbors() {
        let adjacency = vec![vec![1, 99], vec![0]];
        let mut distances = vec![None; adjacency.len()];
        let mut queue = VecDeque::new();

        let max_radius = dual_graph_distances(&adjacency, 0, &mut distances, &mut queue);

        assert_eq!(max_radius, Some(1));
        assert_eq!(distances, vec![Some(0), Some(1)]);
    }

    #[test]
    fn dual_graph_distances_reject_mismatched_buffers() {
        let adjacency = vec![vec![1], vec![0]];
        let mut distances = vec![None; adjacency.len() - 1];
        let mut queue = VecDeque::new();

        assert_eq!(
            dual_graph_distances(&adjacency, 0, &mut distances, &mut queue),
            None
        );
    }

    #[test]
    fn return_probabilities_follow_two_node_walk() {
        let adjacency = vec![vec![1], vec![0]];

        let probabilities = average_return_probabilities(&adjacency, 4);

        assert_relative_eq!(probabilities[0], 1.0);
        assert_relative_eq!(probabilities[1], 0.5);
        assert_relative_eq!(probabilities[2], 0.5);
        assert_relative_eq!(probabilities[3], 0.5);
        assert_relative_eq!(probabilities[4], 0.5);
    }
}
