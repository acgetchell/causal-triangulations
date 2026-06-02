#![forbid(unsafe_code)]

//! Utility functions for random number generation and mathematical operations.

use num_traits::cast::NumCast;
use rand::random;

/// Converts a `usize` to `f64`, preserving the checked conversion boundary.
#[must_use]
pub(crate) fn usize_to_f64(n: usize) -> Option<f64> {
    NumCast::from(n)
}

/// Convert a y-coordinate to a time-slice index via `round()`, clamped to `[0, max_t]`.
///
/// Returns `None` if the rounded value is negative or exceeds `u32::MAX`.
///
/// # Examples
///
/// ```
/// use causal_triangulations::util::y_to_time_bucket;
///
/// assert_eq!(y_to_time_bucket(1.6, 4), Some(2));
/// assert_eq!(y_to_time_bucket(-0.6, 4), None);
/// ```
#[must_use]
pub fn y_to_time_bucket(y: f64, max_t: u32) -> Option<u32> {
    let rounded = y.round();
    num_traits::ToPrimitive::to_u32(&rounded).map(|t| t.min(max_t))
}

/// Convert a non-negative `f64` band index to `u32`, clamped to `[0, max_t]`.
///
/// Returns 0 if the value is negative or NaN.
///
/// # Examples
///
/// ```
/// use causal_triangulations::util::f64_band_to_u32;
///
/// assert_eq!(f64_band_to_u32(2.0, 4), 2);
/// assert_eq!(f64_band_to_u32(10.0, 4), 4);
/// ```
#[must_use]
pub fn f64_band_to_u32(band_index: f64, max_t: u32) -> u32 {
    num_traits::ToPrimitive::to_u32(&band_index)
        .unwrap_or(0)
        .min(max_t)
}

/// Generates a random floating-point number between 0.0 and 1.0.
///
/// # Returns
///
/// A random `f64` value in the range [0.0, 1.0).
///
/// # Examples
///
/// ```
/// use causal_triangulations::util::generate_random_float;
///
/// let value = generate_random_float();
/// assert!((0.0..1.0).contains(&value));
/// ```
#[must_use]
pub fn generate_random_float() -> f64 {
    random::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::abs_diff_eq;

    #[test]
    fn test_generate_random_float() {
        let result = generate_random_float();
        assert!(result >= 0.0, "Random float should be >= 0.0");
        assert!(result < 1.0, "Random float should be < 1.0");
    }

    #[test]
    fn test_generate_random_float_multiple_calls() {
        let results: Vec<f64> = (0..10).map(|_| generate_random_float()).collect();

        // All values should be in valid range
        for result in &results {
            assert!(
                (&0.0..&1.0).contains(&result),
                "Random float {result} out of range"
            );
        }

        // Values should not all be identical (extremely unlikely with proper randomness)
        let first = results[0];
        let all_same = results
            .iter()
            .all(|&x| abs_diff_eq!(x, first, epsilon = f64::EPSILON));
        assert!(!all_same, "All random values should not be identical");
    }

    #[test]
    fn test_y_to_time_bucket_exact_integers() {
        assert_eq!(y_to_time_bucket(0.0, 5), Some(0));
        assert_eq!(y_to_time_bucket(1.0, 5), Some(1));
        assert_eq!(y_to_time_bucket(3.0, 5), Some(3));
    }

    #[test]
    fn test_y_to_time_bucket_rounding() {
        assert_eq!(y_to_time_bucket(0.4, 5), Some(0));
        assert_eq!(y_to_time_bucket(0.6, 5), Some(1));
        assert_eq!(y_to_time_bucket(2.499, 5), Some(2));
        assert_eq!(y_to_time_bucket(2.501, 5), Some(3));
    }

    #[test]
    fn test_y_to_time_bucket_clamping() {
        assert_eq!(y_to_time_bucket(10.0, 3), Some(3));
        assert_eq!(y_to_time_bucket(100.0, 0), Some(0));
    }

    #[test]
    fn test_y_to_time_bucket_negative() {
        assert_eq!(y_to_time_bucket(-1.0, 5), None);
        assert_eq!(y_to_time_bucket(-0.6, 5), None);
    }

    #[test]
    fn test_y_to_time_bucket_nan_inf() {
        assert_eq!(y_to_time_bucket(f64::NAN, 5), None);
        assert_eq!(y_to_time_bucket(f64::INFINITY, 5), None);
        assert_eq!(y_to_time_bucket(f64::NEG_INFINITY, 5), None);
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
    }

    #[test]
    fn test_f64_band_to_u32_negative_and_nan() {
        assert_eq!(f64_band_to_u32(-1.0, 5), 0);
        assert_eq!(f64_band_to_u32(f64::NAN, 5), 0);
        assert_eq!(f64_band_to_u32(f64::NEG_INFINITY, 5), 0);
    }
}
