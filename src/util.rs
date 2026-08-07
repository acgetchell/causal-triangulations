#![forbid(unsafe_code)]

//! Crate-private numeric conversion helpers.

use num_traits::cast::NumCast;

/// Converts a `usize` to `f64`, preserving the checked conversion boundary.
#[must_use]
pub fn usize_to_f64(n: usize) -> Option<f64> {
    NumCast::from(n)
}

/// Convert a non-negative `f64` band index to `u32`, clamped to `[0, max_t]`.
///
/// Returns 0 if the value is negative or NaN.
#[must_use]
pub fn f64_band_to_u32(band_index: f64, max_t: u32) -> u32 {
    num_traits::ToPrimitive::to_u32(&band_index)
        .unwrap_or(0)
        .min(max_t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f64_band_to_u32_normal() {
        assert_eq!(f64_band_to_u32(0.0, 5), 0);
        assert_eq!(f64_band_to_u32(2.0, 5), 2);
        assert_eq!(f64_band_to_u32(5.0, 5), 5);
    }

    #[test]
    fn test_f64_band_to_u32_clamping() {
        assert_eq!(f64_band_to_u32(10.0, 3), 3);
    }

    #[test]
    fn test_f64_band_to_u32_negative_and_nan() {
        assert_eq!(f64_band_to_u32(-1.0, 5), 0);
        assert_eq!(f64_band_to_u32(f64::NAN, 5), 0);
        assert_eq!(f64_band_to_u32(f64::NEG_INFINITY, 5), 0);
    }
}
