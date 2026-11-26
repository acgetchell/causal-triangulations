#![forbid(unsafe_code)]

//! Numeric conversions and certified comparisons for CDT decisions.

use la_stack::{BigRational, Interval};
use num_traits::ToPrimitive;

/// Converts a `usize` to `f64`, preserving the checked conversion boundary.
#[must_use]
pub fn usize_to_f64(n: usize) -> Option<f64> {
    n.to_f64()
}

/// Convert a non-negative `f64` band index to `u32`, clamped to `[0, max_t]`.
///
/// Returns 0 if the value is negative or NaN.
#[must_use]
pub fn f64_band_to_u32(band_index: f64, max_t: u32) -> u32 {
    band_index
        .clamp(0.0, f64::from(max_t))
        .to_u32()
        .unwrap_or(0)
}

/// Tests `|left - right| <= tolerance * scale` for the original finite binary64 inputs.
pub fn within_scaled_tolerance(left: f64, right: f64, tolerance: f64, scale: f64) -> bool {
    if !left.is_finite()
        || !right.is_finite()
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !scale.is_finite()
        || scale < 0.0
    {
        return false;
    }
    if left.to_bits() == right.to_bits() {
        return true;
    }
    let high = left.max(right);
    let low = left.min(right);
    let distance = Interval::try_from_subtraction(high, low);
    let limit = Interval::point(tolerance).and_then(|t| t.try_mul(&Interval::point(scale)?));
    if let (Ok(distance), Ok(limit)) = (distance, limit) {
        if distance.upper() <= limit.lower() {
            return true;
        }
        if distance.lower() > limit.upper() {
            return false;
        }
    }
    let exact = |x| BigRational::from_float(x).expect("finite comparison operand");
    exact(high) - exact(low) <= exact(tolerance) * exact(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_tolerance_compares_distances_beyond_the_finite_range() {
        // Both the distance and limit overflow in binary64. Equality is included,
        // and lowering the multiplier by one step makes the same pair fail.
        assert!(within_scaled_tolerance(f64::MAX, -f64::MAX, 2.0, f64::MAX));
        assert!(!within_scaled_tolerance(
            -f64::MAX,
            f64::MAX,
            2.0_f64.next_down(),
            f64::MAX
        ));
    }

    #[test]
    fn test_f64_band_to_u32_normal() {
        assert_eq!(f64_band_to_u32(0.0, 5), 0);
        assert_eq!(f64_band_to_u32(2.0, 5), 2);
        assert_eq!(f64_band_to_u32(5.0, 5), 5);
    }

    #[test]
    fn test_f64_band_to_u32_clamping() {
        assert_eq!(f64_band_to_u32(10.0, 3), 3);
        assert_eq!(f64_band_to_u32(f64::MAX, 3), 3);
        assert_eq!(f64_band_to_u32(f64::INFINITY, 3), 3);
    }

    #[test]
    fn test_f64_band_to_u32_negative_and_nan() {
        assert_eq!(f64_band_to_u32(-1.0, 5), 0);
        assert_eq!(f64_band_to_u32(f64::NAN, 5), 0);
        assert_eq!(f64_band_to_u32(f64::NEG_INFINITY, 5), 0);
    }
}
