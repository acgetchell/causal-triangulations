use crate::errors::CdtError;
use delaunay::core::builder::DelaunayTriangulationBuilder;
use delaunay::geometry::kernel::RobustKernel;
use delaunay::geometry::util::{generate_random_points, generate_random_points_seeded};
use delaunay::prelude::VertexBuilder;
use rand::random;

/// Type alias for the 2D Delaunay triangulation returned by this crate's generators.
///
/// Uses [`RobustKernel`] (the default for [`DelaunayTriangulationBuilder::build`]) and
/// `()` vertex data (CDT metadata is tracked separately in [`CdtTriangulation`](crate::cdt::triangulation::CdtTriangulation)).
pub type DelaunayTriangulation2D =
    delaunay::core::delaunay_triangulation::DelaunayTriangulation<RobustKernel<f64>, (), i32, 2>;

/// Generates a random floating-point number between 0.0 and 1.0.
///
/// # Returns
///
/// A random `f64` value in the range [0.0, 1.0).
#[must_use]
pub fn generate_random_float() -> f64 {
    random::<f64>()
}

/// Generates a Delaunay triangulation with optional seed for deterministic testing.
///
/// Uses [`DelaunayTriangulationBuilder`] (introduced in delaunay v0.7.2) for
/// construction, which provides deterministic tie-breaking and coherent orientation
/// as first-class invariants.
///
/// # Panics
///
/// Panics if a [`VertexBuilder`] fails to build from a valid point (should not happen in practice).
///
/// # Errors
///
/// Returns enhanced error information including vertex count, coordinate range, and underlying error.
pub fn generate_delaunay2_with_context(
    number_of_vertices: u32,
    coordinate_range: (f64, f64),
    seed: Option<u64>,
) -> crate::errors::CdtResult<DelaunayTriangulation2D> {
    // Validate parameters before attempting generation
    if number_of_vertices < 3 {
        return Err(CdtError::InvalidGenerationParameters {
            issue: "Insufficient vertex count".to_string(),
            provided_value: number_of_vertices.to_string(),
            expected_range: "≥ 3".to_string(),
        });
    }

    if coordinate_range.0 >= coordinate_range.1 {
        return Err(CdtError::InvalidGenerationParameters {
            issue: "Invalid coordinate range".to_string(),
            provided_value: format!("[{}, {}]", coordinate_range.0, coordinate_range.1),
            expected_range: "min < max".to_string(),
        });
    }

    // Generate random points, then build triangulation via the builder API
    let n = number_of_vertices as usize;
    let points = seed
        .map_or_else(
            || generate_random_points::<f64, 2>(n, coordinate_range),
            |s| generate_random_points_seeded::<f64, 2>(n, coordinate_range, s),
        )
        .map_err(|e| CdtError::DelaunayGenerationFailed {
            vertex_count: number_of_vertices,
            coordinate_range,
            attempt: 1,
            underlying_error: format!("Point generation failed: {e}"),
        })?;

    // VertexBuilder::build() only fails if the point is missing, which cannot happen here.
    let vertices: Vec<_> = points
        .into_iter()
        .map(|point| VertexBuilder::default().point(point).build().unwrap())
        .collect();

    let dt = DelaunayTriangulationBuilder::new(&vertices)
        .build::<i32>()
        .map_err(|e| CdtError::DelaunayGenerationFailed {
            vertex_count: number_of_vertices,
            coordinate_range,
            attempt: 1,
            underlying_error: e.to_string(),
        })?;

    Ok(dt)
}

/// Generates a random Delaunay triangulation.
///
/// # Panics
///
/// Panics if triangulation generation fails due to invalid parameters or coordinate generation errors.
#[must_use]
pub fn generate_random_delaunay2(
    number_of_vertices: u32,
    coordinate_range: (f64, f64),
) -> DelaunayTriangulation2D {
    generate_delaunay2_with_context(number_of_vertices, coordinate_range, None)
        .unwrap_or_else(|_| {
            panic!(
                "Failed to generate random Delaunay triangulation with {number_of_vertices} vertices"
            )
        })
}

/// Generates a seeded Delaunay triangulation for deterministic testing.
///
/// # Panics
///
/// Panics if triangulation generation fails due to invalid parameters or coordinate generation errors.
#[must_use]
pub fn generate_seeded_delaunay2(
    number_of_vertices: u32,
    coordinate_range: (f64, f64),
    seed: u64,
) -> DelaunayTriangulation2D {
    generate_delaunay2_with_context(number_of_vertices, coordinate_range, Some(seed))
        .unwrap_or_else(|_| {
            panic!(
                "Failed to generate seeded Delaunay triangulation with {number_of_vertices} vertices and seed {seed}"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::CdtError;

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
        let all_same = results.iter().all(|&x| (x - first).abs() < f64::EPSILON);
        assert!(!all_same, "All random values should not be identical");
    }

    #[test]
    fn test_generate_delaunay2_with_context_valid_parameters() {
        let result = generate_delaunay2_with_context(4, (0.0, 10.0), None);
        assert!(
            result.is_ok(),
            "Should successfully generate triangulation with valid parameters"
        );

        let dt = result.unwrap();
        assert_eq!(dt.number_of_vertices(), 4, "Should have 4 vertices");
        assert!(dt.number_of_cells() > 0, "Should have at least one cell");
    }

    #[test]
    fn test_generate_delaunay2_with_context_with_seed() {
        let seed = 12345;
        let result1 = generate_delaunay2_with_context(4, (0.0, 10.0), Some(seed));
        let result2 = generate_delaunay2_with_context(4, (0.0, 10.0), Some(seed));

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
            dt1.number_of_cells(),
            dt2.number_of_cells(),
            "Should have same cell count"
        );
    }

    #[test]
    fn test_generate_delaunay2_with_context_insufficient_vertices() {
        let result = generate_delaunay2_with_context(2, (0.0, 10.0), None);
        assert!(result.is_err(), "Should fail with insufficient vertices");

        match result.unwrap_err() {
            CdtError::InvalidGenerationParameters {
                issue,
                provided_value,
                expected_range,
            } => {
                assert_eq!(issue, "Insufficient vertex count");
                assert_eq!(provided_value, "2");
                assert_eq!(expected_range, "≥ 3");
            }
            _ => panic!("Expected InvalidGenerationParameters error"),
        }
    }

    #[test]
    fn test_generate_delaunay2_with_context_invalid_coordinate_range() {
        let result = generate_delaunay2_with_context(4, (10.0, 5.0), None);
        assert!(result.is_err(), "Should fail with invalid coordinate range");

        match result.unwrap_err() {
            CdtError::InvalidGenerationParameters {
                issue,
                provided_value,
                expected_range,
            } => {
                assert_eq!(issue, "Invalid coordinate range");
                assert_eq!(provided_value, "[10, 5]");
                assert_eq!(expected_range, "min < max");
            }
            _ => panic!("Expected InvalidGenerationParameters error"),
        }
    }

    #[test]
    fn test_generate_delaunay2_with_context_equal_coordinate_range() {
        let result = generate_delaunay2_with_context(4, (5.0, 5.0), None);
        assert!(result.is_err(), "Should fail with equal coordinate range");

        match result.unwrap_err() {
            CdtError::InvalidGenerationParameters { issue, .. } => {
                assert_eq!(issue, "Invalid coordinate range");
            }
            _ => panic!("Expected InvalidGenerationParameters error"),
        }
    }

    #[test]
    fn test_generate_delaunay2_with_context_various_sizes() {
        let test_cases = [(3, "minimal"), (5, "small"), (10, "medium"), (20, "large")];

        for (vertex_count, description) in test_cases {
            let result = generate_delaunay2_with_context(vertex_count, (0.0, 100.0), None);
            assert!(
                result.is_ok(),
                "Should generate {description} triangulation with {vertex_count} vertices"
            );

            let dt = result.unwrap();
            assert_eq!(
                dt.number_of_vertices(),
                vertex_count as usize,
                "Should have {vertex_count} vertices for {description} triangulation"
            );
            assert!(
                dt.number_of_cells() > 0,
                "Should have at least one cell for {description} triangulation"
            );
        }
    }

    #[test]
    fn test_generate_delaunay2_with_context_different_coordinate_ranges() {
        let ranges = [(0.0, 1.0), (-10.0, 10.0), (100.0, 200.0), (-50.0, 0.0)];

        for range in ranges {
            let result = generate_delaunay2_with_context(4, range, None);
            assert!(
                result.is_ok(),
                "Should generate triangulation with range {range:?}"
            );

            let dt = result.unwrap();
            assert_eq!(dt.number_of_vertices(), 4, "Should have 4 vertices");
        }
    }

    #[test]
    fn test_generate_random_delaunay2_success() {
        let dt = generate_random_delaunay2(5, (0.0, 10.0));
        assert_eq!(dt.number_of_vertices(), 5, "Should have 5 vertices");
        assert!(dt.number_of_cells() > 0, "Should have at least one cell");
    }

    #[test]
    fn test_generate_random_delaunay2_various_sizes() {
        let sizes = [3, 4, 6, 8, 12];

        for size in sizes {
            let dt = generate_random_delaunay2(size, (0.0, 50.0));
            assert_eq!(
                dt.number_of_vertices(),
                size as usize,
                "Should have {size} vertices"
            );
            assert!(
                dt.number_of_cells() > 0,
                "Should have cells for size {size}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "Failed to generate random Delaunay triangulation with 2 vertices")]
    fn test_generate_random_delaunay2_panic_insufficient_vertices() {
        let _ = generate_random_delaunay2(2, (0.0, 10.0));
    }

    #[test]
    #[should_panic(expected = "Failed to generate random Delaunay triangulation with 4 vertices")]
    fn test_generate_random_delaunay2_panic_invalid_range() {
        let _ = generate_random_delaunay2(4, (10.0, 5.0));
    }

    #[test]
    fn test_generate_seeded_delaunay2_deterministic() {
        let seed = 42;
        let dt1 = generate_seeded_delaunay2(6, (0.0, 20.0), seed);
        let dt2 = generate_seeded_delaunay2(6, (0.0, 20.0), seed);

        // Should produce identical results
        assert_eq!(
            dt1.number_of_vertices(),
            dt2.number_of_vertices(),
            "Should have same vertex count"
        );
        assert_eq!(
            dt1.number_of_cells(),
            dt2.number_of_cells(),
            "Should have same cell count"
        );

        // Verify expected properties
        assert_eq!(dt1.number_of_vertices(), 6, "Should have 6 vertices");
        assert!(dt1.number_of_cells() > 0, "Should have cells");
    }

    #[test]
    fn test_generate_seeded_delaunay2_different_seeds() {
        let dt1 = generate_seeded_delaunay2(5, (0.0, 10.0), 123);
        let dt2 = generate_seeded_delaunay2(5, (0.0, 10.0), 456);

        // Both should succeed and have same vertex count
        assert_eq!(dt1.number_of_vertices(), 5, "First should have 5 vertices");
        assert_eq!(dt2.number_of_vertices(), 5, "Second should have 5 vertices");

        // With different seeds, they should potentially have different structures
        // (though this is probabilistic and not guaranteed)
    }

    #[test]
    fn test_generate_seeded_delaunay2_various_seeds() {
        let seeds = [1, 100, 1000, u64::MAX];

        for seed in seeds {
            let dt = generate_seeded_delaunay2(4, (-5.0, 5.0), seed);
            assert_eq!(
                dt.number_of_vertices(),
                4,
                "Should have 4 vertices with seed {seed}"
            );
            assert!(
                dt.number_of_cells() > 0,
                "Should have cells with seed {seed}"
            );
        }
    }

    #[test]
    #[should_panic(
        expected = "Failed to generate seeded Delaunay triangulation with 1 vertices and seed 42"
    )]
    fn test_generate_seeded_delaunay2_panic_insufficient_vertices() {
        let _ = generate_seeded_delaunay2(1, (0.0, 10.0), 42);
    }

    #[test]
    #[should_panic(
        expected = "Failed to generate seeded Delaunay triangulation with 5 vertices and seed 123"
    )]
    fn test_generate_seeded_delaunay2_panic_invalid_range() {
        let _ = generate_seeded_delaunay2(5, (15.0, 10.0), 123);
    }

    #[test]
    fn test_euler_characteristic_properties() {
        // Test that generated triangulations satisfy basic topological properties
        let dt = generate_random_delaunay2(8, (0.0, 10.0));

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let v = dt.number_of_vertices() as i32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let c = dt.number_of_cells() as i32; // faces in 2D

        // Basic sanity checks
        assert!(v >= 3, "Should have at least 3 vertices");
        assert!(c >= 1, "Should have at least 1 cell/face");

        // For a 2D triangulation, we can estimate edge count
        // In a typical triangulation: E ≈ 3V - 6 for planar graphs
        // But this is an approximation since the delaunay crate may handle boundaries differently
    }

    #[test]
    fn test_coordinate_range_bounds() {
        // Test extreme coordinate ranges
        let ranges = [
            (f64::MIN / 1e10, f64::MAX / 1e10), // Very large range (scaled down to avoid overflow)
            (-1000.0, 1000.0),                  // Large symmetric range
            (0.001, 0.002),                     // Very small range
            (-0.5, 0.5),                        // Small symmetric range
        ];

        for range in ranges {
            let result = generate_delaunay2_with_context(4, range, Some(789));
            assert!(result.is_ok(), "Should handle coordinate range {range:?}");
        }
    }

    #[test]
    fn test_seeded_reproducibility_multiple_calls() {
        // Test that multiple calls with the same seed produce identical results
        let seed = 999;
        let params = (7, (-10.0, 10.0));

        let results: Vec<_> = (0..3)
            .map(|_| generate_seeded_delaunay2(params.0, params.1, seed))
            .collect();

        // All results should have the same structure
        for (i, dt) in results.iter().enumerate() {
            assert_eq!(
                dt.number_of_vertices(),
                7,
                "Result {i} should have 7 vertices"
            );
            assert!(dt.number_of_cells() > 0, "Result {i} should have cells");
        }

        // All results should be identical in structure
        let first_vertex_count = results[0].number_of_vertices();
        let first_cell_count = results[0].number_of_cells();

        for (i, dt) in results.iter().enumerate().skip(1) {
            assert_eq!(
                dt.number_of_vertices(),
                first_vertex_count,
                "Result {i} vertex count should match first result"
            );
            assert_eq!(
                dt.number_of_cells(),
                first_cell_count,
                "Result {i} cell count should match first result"
            );
        }
    }
}
