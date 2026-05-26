//! Shared CDT-domain helpers for Metropolis sampling.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::MoveType;
use crate::cdt::results::Measurement;
use crate::config::validate_schedule;
use crate::errors::{CdtError, CdtResult, ConfigurationSetting};
use crate::geometry::CdtTriangulation2D;
use crate::util::saturating_usize_to_u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimplexCounts {
    /// Number of vertices in the live CDT triangulation.
    pub vertices: u32,
    /// Number of edges in the live CDT triangulation.
    pub edges: u32,
    /// Number of triangular 2-simplices in the live CDT triangulation.
    pub triangles: u32,
}

/// Adapts shared schedule validation errors to the Metropolis-specific error variant.
///
/// This keeps [`MetropolisConfig::validate`](super::MetropolisConfig::validate)
/// aligned with the shared simulation schedule validator while preserving the
/// public Metropolis error contract.
pub const fn invalid_sim_config(
    setting: ConfigurationSetting,
    provided_value: String,
    expected: String,
) -> CdtError {
    CdtError::InvalidSimulationConfiguration {
        setting,
        provided_value,
        expected,
    }
}

/// Validates simulation-specific configuration values.
///
/// This is the shared implementation behind
/// [`MetropolisConfig::validate`](super::MetropolisConfig::validate) and the
/// runner entry points, so all public Metropolis APIs reject the same invalid
/// temperature, step-count, thermalization, and measurement schedules.
///
/// # Errors
///
/// Returns [`CdtError::InvalidSimulationConfiguration`] when `temperature` is
/// not finite and positive, when `steps` or `measurement_frequency` is zero,
/// when thermalization exceeds the step count, or when the schedule cannot
/// produce a post-thermalization measurement.
pub fn validate_metropolis_schedule(
    temperature: f64,
    steps: u32,
    thermalization_steps: u32,
    measurement_frequency: u32,
) -> CdtResult<()> {
    validate_schedule(
        temperature,
        steps,
        thermalization_steps,
        measurement_frequency,
        invalid_sim_config,
    )
}

/// Rejects temperatures that would make target log probabilities non-finite.
///
/// # Errors
///
/// Returns [`CdtError::InvalidSimulationConfiguration`] when `temperature` is
/// non-finite, zero, or negative.
pub fn validate_temperature(temperature: f64) -> CdtResult<()> {
    if temperature.is_finite() && temperature > 0.0 {
        Ok(())
    } else {
        Err(invalid_sim_config(
            ConfigurationSetting::Temperature,
            temperature.to_string(),
            "finite and positive".to_string(),
        ))
    }
}

/// Reads simplex counts through the CDT wrapper for action and measurement code.
///
/// Centralizing these conversions keeps cached query paths authoritative and
/// makes integer saturation explicit at the simulation boundary.
pub fn simplex_counts(triangulation: &CdtTriangulation2D) -> SimplexCounts {
    SimplexCounts {
        vertices: saturating_usize_to_u32(triangulation.vertex_count()),
        edges: saturating_usize_to_u32(triangulation.edge_count()),
        triangles: saturating_usize_to_u32(triangulation.face_count()),
    }
}

/// Computes the current action from live simplex counts.
///
/// The Metropolis loop calls this only after state is known to be current, which
/// avoids trusting stale values across backend mutations or rollback.
pub fn action_for(action_config: &ActionConfig, triangulation: &CdtTriangulation2D) -> f64 {
    let counts = simplex_counts(triangulation);
    action_config.calculate_action(counts.vertices, counts.edges, counts.triangles)
}

/// Captures a measurement from the live triangulation state.
///
/// Keeping measurement construction in one helper ensures recorded actions and
/// simplex counts use the same query path at every measurement step.
pub fn measurement_for(step: u32, action: f64, triangulation: &CdtTriangulation2D) -> Measurement {
    let counts = simplex_counts(triangulation);
    Measurement {
        step,
        action,
        vertices: counts.vertices,
        edges: counts.edges,
        triangles: counts.triangles,
        volume_profile: triangulation.volume_profile(),
    }
}

/// Computes the count-level action change before mutating the triangulation.
///
/// This is the core proposal-before-mutation calculation: Metropolis acceptance
/// must be based on the selected move type's known simplex-count delta, not on a
/// speculative backend edit that may need rollback.
pub fn proposed_delta_action(
    action_config: &ActionConfig,
    before: SimplexCounts,
    move_type: MoveType,
) -> Option<f64> {
    let after = match move_type {
        MoveType::Move22 | MoveType::EdgeFlip => before,
        MoveType::Move13Add => SimplexCounts {
            vertices: before.vertices.checked_add(1)?,
            edges: before.edges.checked_add(3)?,
            triangles: before.triangles.checked_add(2)?,
        },
        MoveType::Move31Remove => SimplexCounts {
            vertices: before.vertices.checked_sub(1)?,
            edges: before.edges.checked_sub(3)?,
            triangles: before.triangles.checked_sub(2)?,
        },
    };

    let action_before =
        action_config.calculate_action(before.vertices, before.edges, before.triangles);
    let action_after = action_config.calculate_action(after.vertices, after.edges, after.triangles);
    Some(action_after - action_before)
}

/// Compares action values with a scale-aware tolerance for checkpoint validation.
pub fn actions_match(left: f64, right: f64) -> bool {
    if !(left.is_finite() && right.is_finite()) {
        return false;
    }
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= f64::EPSILON * scale * 8.0
}
