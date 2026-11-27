#![forbid(unsafe_code)]

//! Explicit total-triangle bias for the periodic 1+1 ensemble.

use crate::config::CdtTopology;
use crate::errors::CdtResult;
use la_stack::{BigInt, BigRational};
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
pub(crate) enum VolumeFixingWire {
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
        let distance = triangles.abs_diff(self.target.get() as usize) as u128;
        self.scaled_count(distance * distance)
    }

    /// Computes the quadratic difference from integer counts and rounds once.
    pub(crate) fn delta(self, before: usize, change: i32) -> f64 {
        let distance = i128::try_from(before).expect("supported usize fits i128")
            - i128::from(self.target.get());
        let factor = 2 * distance + i128::from(change);
        let magnitude = self.scaled_count((i128::from(change) * factor).unsigned_abs());
        if (change < 0) ^ (factor < 0) {
            -magnitude
        } else {
            magnitude
        }
    }

    /// Computes the largest rounded bias over the supported simplex counts.
    pub(crate) fn maximum_action(self) -> f64 {
        // The quadratic is monotone in absolute distance, including after rounding.
        let target = self.target.get() as usize;
        self.action(if target > usize::MAX - target {
            0
        } else {
            usize::MAX
        })
    }

    /// Multiplies the dyadic strength by an integer without rounding the count first.
    fn scaled_count(self, count: u128) -> f64 {
        let rounded = count.to_f64().expect("u128 fits binary64 range");
        if count <= 1_u128 << f64::MANTISSA_DIGITS
            || self.strength == 0.0
            || rounded.to_u128() == Some(count)
        {
            return self.strength * rounded;
        }
        (BigRational::from_float(self.strength).expect("finite strength") * BigInt::from(count))
            .to_f64()
            .expect("binary rational rounds to binary64")
    }

    /// Rejects unsupported topology at configuration and runner boundaries.
    pub(crate) fn ensure_ensemble(self, topology: CdtTopology, timeslices: u32) -> CdtResult<()> {
        if topology != CdtTopology::Toroidal {
            return Err(VolumeFixingError::UnsupportedTopology { topology }.into());
        }
        Ok(self.ensure_toroidal_minimum(u64::from(timeslices))?)
    }

    /// Each periodic slice has at least three vertices and six triangles.
    pub(crate) fn ensure_toroidal_minimum(self, timeslices: u64) -> Result<(), VolumeFixingError> {
        let minimum = timeslices.saturating_mul(6);
        if u64::from(self.target.get()) < minimum {
            return Err(VolumeFixingError::TargetBelowMinimum {
                target: self.target.get(),
                minimum,
            });
        }
        Ok(())
    }
}
