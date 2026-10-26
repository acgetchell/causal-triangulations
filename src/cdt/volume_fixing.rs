#![forbid(unsafe_code)]

//! Explicit total-triangle bias for the periodic 1+1 ensemble.

use crate::config::CdtTopology;
use crate::errors::CdtResult;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// Invalid quadratic volume-fixing parameters or an unsupported ensemble.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum VolumeFixingError {
    /// A triangle target must be positive and even.
    #[error("target triangle count must be positive and even, got {target}")]
    InvalidTarget {
        /// Rejected target.
        target: u32,
    },
    /// Strength must be finite, nonnegative, and safe over supported count ranges.
    #[error("invalid quadratic volume-fixing strength {strength}")]
    InvalidStrength {
        /// Rejected strength.
        strength: f64,
    },
    /// A periodic slice has at least three spacelike links and two triangles per link.
    #[error("target {target} is below the legal minimum {minimum} triangles")]
    TargetBelowMinimum {
        /// Requested target.
        target: u32,
        /// Minimum implied by the time extent.
        minimum: u64,
    },
    /// This initial bias contract supports periodic 1+1 triangulations only.
    #[error("triangle volume fixing requires toroidal 1+1 CDT, got {topology}")]
    UnsupportedTopology {
        /// Requested topology.
        topology: CdtTopology,
    },
    /// Physical and fixing terms must have a finite combined range.
    #[error("physical and volume-fixing actions have an overflowing combined range")]
    CombinedActionOverflow,
}

/// Validated opt-in bias `epsilon * (N2 - target)^2`, in action units.
///
/// Absence of this value disables fixing. The volume observable is explicitly
/// triangle count; no hard volume constraint or post-proposal clamp is applied.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::action::VolumeFixing;
/// use approx::assert_relative_eq;
/// let fixing = VolumeFixing::quadratic_triangles(32, 0.25)?;
/// assert_relative_eq!(fixing.action(34), 1.0);
/// # Ok::<(), causal_triangulations::CdtError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "VolumeFixingWire", into = "VolumeFixingWire")]
pub struct VolumeFixing {
    target: NonZeroU32,
    strength: f64,
}

/// Version-neutral raw configuration; the tagged mode identifies the observable.
#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
enum VolumeFixingWire {
    QuadraticTriangles { target: u32, strength: f64 },
}

impl TryFrom<VolumeFixingWire> for VolumeFixing {
    type Error = VolumeFixingError;

    fn try_from(wire: VolumeFixingWire) -> Result<Self, Self::Error> {
        let VolumeFixingWire::QuadraticTriangles { target, strength } = wire;
        Self::quadratic_triangles(target, strength)
    }
}

impl From<VolumeFixing> for VolumeFixingWire {
    fn from(fixing: VolumeFixing) -> Self {
        Self::QuadraticTriangles {
            target: fixing.target.get(),
            strength: fixing.strength,
        }
    }
}

impl VolumeFixing {
    /// Parses a positive even triangle target and a finite nonnegative strength.
    ///
    /// # Errors
    /// Returns a typed parameter error, including unsafe arithmetic ranges.
    /// See the type-level example for a valid configuration.
    pub fn quadratic_triangles(target: u32, strength: f64) -> Result<Self, VolumeFixingError> {
        let target = NonZeroU32::new(target)
            .filter(|value| value.get().is_multiple_of(2))
            .ok_or(VolumeFixingError::InvalidTarget { target })?;
        let fixing = Self { target, strength };
        if !strength.is_finite() || strength < 0.0 || !fixing.maximum_action().is_finite() {
            return Err(VolumeFixingError::InvalidStrength { strength });
        }
        Ok(fixing)
    }

    /// Returns the exact target triangle count.
    #[must_use]
    pub const fn target(self) -> NonZeroU32 {
        self.target
    }

    /// Returns the quadratic coefficient in action units per squared triangle.
    #[must_use]
    pub const fn strength(self) -> f64 {
        self.strength
    }

    /// Evaluates the bias at a total triangle count.
    #[must_use]
    pub fn action(self, triangles: usize) -> f64 {
        let difference = triangles.to_f64().unwrap_or(f64::MAX) - f64::from(self.target.get());
        (self.strength * difference) * difference
    }

    /// Computes the exact quadratic difference without subtracting extensive actions.
    pub(crate) fn delta(self, before: usize, change: i32) -> f64 {
        let difference = before.to_f64().unwrap_or(f64::MAX) - f64::from(self.target.get());
        let change = f64::from(change);
        (self.strength * change) * 2.0_f64.mul_add(difference, change)
    }

    /// Bounds the bias over representable simplex counts before target construction.
    pub(crate) fn maximum_action(self) -> f64 {
        let extent = usize::MAX.to_f64().unwrap_or(f64::MAX) + f64::from(self.target.get());
        (self.strength * extent) * extent
    }

    /// Rejects unsupported topology at configuration and runner boundaries.
    pub(crate) fn ensure_ensemble(self, topology: CdtTopology, timeslices: u32) -> CdtResult<()> {
        if topology != CdtTopology::Toroidal {
            return Err(VolumeFixingError::UnsupportedTopology { topology }.into());
        }
        let minimum = 6 * u64::from(timeslices);
        if u64::from(self.target.get()) < minimum {
            return Err(VolumeFixingError::TargetBelowMinimum {
                target: self.target.get(),
                minimum,
            }
            .into());
        }
        Ok(())
    }
}
