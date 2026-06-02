#![forbid(unsafe_code)]

//! Shared CDT-domain helpers for Metropolis sampling.

use crate::cdt::action::ActionConfig;
use crate::cdt::ergodic_moves::MoveType;
use crate::cdt::results::Measurement;
use crate::config::validate_schedule;
use crate::errors::{CdtError, CdtResult, ConfigurationSetting};
use crate::geometry::CdtTriangulation2D;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimplexCounts {
    /// Number of vertices in the live CDT triangulation.
    pub vertices: usize,
    /// Number of edges in the live CDT triangulation.
    pub edges: usize,
    /// Number of triangular 2-simplices in the live CDT triangulation.
    pub triangles: usize,
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
/// Centralizing these reads keeps cached query paths authoritative for action,
/// measurement, and trace telemetry.
pub fn simplex_counts(triangulation: &CdtTriangulation2D) -> SimplexCounts {
    SimplexCounts {
        vertices: triangulation.vertex_count(),
        edges: triangulation.edge_count(),
        triangles: triangulation.face_count(),
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
///
/// # Errors
///
/// Returns [`CdtError::InvalidSimplexCount`] if the live triangulation reports
/// zero vertices, edges, or triangles,
/// [`CdtError::MeasurementCountOverflow`] if any live count cannot fit compact
/// measurement storage, or [`CdtError::InvalidMeasurementAction`] if `action` is
/// not finite.
pub fn measurement_for(
    step: u32,
    action: f64,
    triangulation: &CdtTriangulation2D,
) -> CdtResult<Measurement> {
    let counts = triangulation.simplex_counts()?;
    Ok(Measurement::try_from_simplex_counts(step, action, counts)?
        .with_volume_profile(triangulation.volume_profile()))
}

/// Returns true when a completed step is on the post-thermalization measurement cadence.
///
/// Measurements are emitted only at steps greater than or equal to
/// `thermalization_steps`. Step `0` is therefore recorded only for schedules with
/// zero thermalization.
pub const fn measurement_is_due(
    step: u32,
    thermalization_steps: u32,
    measurement_frequency: NonZeroU32,
) -> bool {
    step >= thermalization_steps && step.is_multiple_of(measurement_frequency.get())
}

/// Counts scheduled post-thermalization measurements through `current_step`.
///
/// This is the shared cadence calculation used by checkpoint and result
/// validation, keeping deserialized telemetry aligned with the runner's public
/// measurement schedule. Returns `None` if the schedule cannot be represented in
/// `usize`.
pub fn expected_measurement_count(
    current_step: u32,
    thermalization_steps: u32,
    measurement_frequency: NonZeroU32,
) -> Option<usize> {
    let first = first_measurement_step(thermalization_steps, measurement_frequency)?;
    if first > current_step {
        return Some(0);
    }
    let current_step = u64::from(current_step);
    let first = u64::from(first);
    let measurement_frequency = u64::from(measurement_frequency.get());
    let count = (current_step - first) / measurement_frequency + 1;
    usize::try_from(count).ok()
}

/// Returns the scheduled post-thermalization step at a zero-based measurement index.
///
/// This mirrors [`expected_measurement_count`] so checkpoint and result
/// validation reject pre-thermalization samples instead of accepting a shifted
/// measurement stream. Returns `None` if the requested index cannot be expressed
/// as a `u32` step.
pub fn expected_measurement_step(
    index: usize,
    thermalization_steps: u32,
    measurement_frequency: NonZeroU32,
) -> Option<u32> {
    let first = first_measurement_step(thermalization_steps, measurement_frequency)?;
    let offset = u32::try_from(index)
        .ok()?
        .checked_mul(measurement_frequency.get())?;
    first.checked_add(offset)
}

/// Finds the first measurement cadence at or after thermalization.
fn first_measurement_step(
    thermalization_steps: u32,
    measurement_frequency: NonZeroU32,
) -> Option<u32> {
    let measurement_frequency = u64::from(measurement_frequency.get());
    let first =
        u64::from(thermalization_steps).div_ceil(measurement_frequency) * measurement_frequency;
    u32::try_from(first).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frequency(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test measurement frequency should be nonzero")
    }

    #[test]
    fn measurement_count_ignores_steps_before_first_post_thermalization_cadence() {
        assert_eq!(expected_measurement_count(1, 2, frequency(2)), Some(0));
        assert_eq!(expected_measurement_count(2, 2, frequency(2)), Some(1));
        assert_eq!(expected_measurement_count(4, 2, frequency(2)), Some(2));
    }

    #[test]
    fn measurement_step_rounds_thermalization_up_to_cadence() {
        assert_eq!(expected_measurement_step(0, 3, frequency(2)), Some(4));
        assert_eq!(expected_measurement_step(1, 3, frequency(2)), Some(6));
    }

    #[test]
    fn measurement_count_widens_before_including_current_step() {
        let expected = usize::try_from(u64::from(u32::MAX) + 1).ok();

        assert_eq!(
            expected_measurement_count(u32::MAX, 0, frequency(1)),
            expected
        );
    }

    #[test]
    fn actions_match_rejects_nonfinite_values() {
        assert!(!actions_match(f64::NAN, 1.0));
        assert!(!actions_match(1.0, f64::INFINITY));
    }
}
