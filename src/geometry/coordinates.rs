#![forbid(unsafe_code)]

//! Crate-owned coordinate types used across geometry and CDT layers.
//!
//! Geometry backends expose raw numeric coordinate vectors. This module parses
//! those vectors into CDT-owned types before result serialization, notebook
//! visualization, or other downstream consumers rely on coordinate shape. In
//! 1+1 dimensions the convention is `x` for space and `y` for continuous time;
//! integer foliation labels remain separate vertex data.

use serde::{Serialize, Serializer};
use std::error::Error as StdError;
use std::fmt;

const SPACETIME_DIMENSION: usize = 2;
const SPACETIME_COORDINATE_COLUMNS: [&str; SPACETIME_DIMENSION] = ["x", "y"];

/// A validated 1+1 spacetime coordinate.
///
/// The first component is the spatial coordinate `x`; the second component is
/// the continuous time coordinate `y`. CDT time labels may still be stored
/// separately as integer vertex data, but this type keeps raw backend coordinate
/// vectors from leaking into CDT result and visualization code.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::geometry::SpacetimeCoordinate;
///
/// let coordinate = SpacetimeCoordinate::try_new(0.25, 3.0)?;
/// assert_eq!(coordinate.space(), 0.25);
/// assert_eq!(coordinate.time(), 3.0);
/// # Ok::<(), causal_triangulations::prelude::geometry::SpacetimeCoordinateError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacetimeCoordinate {
    space: f64,
    time: f64,
}

impl SpacetimeCoordinate {
    /// Creates a coordinate from raw spatial and time components.
    ///
    /// # Errors
    ///
    /// Returns [`SpacetimeCoordinateError::NonFiniteComponent`] when either
    /// component is `NaN` or infinite.
    pub fn try_new(space: f64, time: f64) -> Result<Self, SpacetimeCoordinateError> {
        validate_component(SpacetimeCoordinateComponent::Space, space)?;
        validate_component(SpacetimeCoordinateComponent::Time, time)?;
        Ok(Self { space, time })
    }

    /// Parses a backend coordinate slice into a validated spacetime coordinate.
    ///
    /// # Errors
    ///
    /// Returns [`SpacetimeCoordinateError::Dimension`] when the slice is not
    /// exactly two-dimensional. Returns
    /// [`SpacetimeCoordinateError::NonFiniteComponent`] when either component is
    /// `NaN` or infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::{
    ///     SpacetimeCoordinate, SpacetimeCoordinateError,
    /// };
    /// use std::assert_matches;
    ///
    /// let coordinate = SpacetimeCoordinate::try_from_space_time_slice(&[2.0, 4.0])?;
    /// assert_eq!(coordinate.to_array(), [2.0, 4.0]);
    ///
    /// assert_matches!(
    ///     SpacetimeCoordinate::try_from_space_time_slice(&[2.0]),
    ///     Err(SpacetimeCoordinateError::Dimension {
    ///         actual: 1,
    ///         expected: 2,
    ///     })
    /// );
    /// # Ok::<(), SpacetimeCoordinateError>(())
    /// ```
    pub fn try_from_space_time_slice(
        coordinates: &[f64],
    ) -> Result<Self, SpacetimeCoordinateError> {
        let [space, time] = coordinates else {
            return Err(SpacetimeCoordinateError::Dimension {
                actual: coordinates.len(),
                expected: SPACETIME_DIMENSION,
            });
        };
        Self::try_new(*space, *time)
    }

    /// Returns the spatial `x` coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::SpacetimeCoordinate;
    ///
    /// let coordinate = SpacetimeCoordinate::try_new(1.5, 2.0)?;
    /// assert_eq!(coordinate.space(), 1.5);
    /// # Ok::<(), causal_triangulations::prelude::geometry::SpacetimeCoordinateError>(())
    /// ```
    #[must_use]
    pub const fn space(self) -> f64 {
        self.space
    }

    /// Returns the continuous time `y` coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::SpacetimeCoordinate;
    ///
    /// let coordinate = SpacetimeCoordinate::try_new(1.5, 2.0)?;
    /// assert_eq!(coordinate.time(), 2.0);
    /// # Ok::<(), causal_triangulations::prelude::geometry::SpacetimeCoordinateError>(())
    /// ```
    #[must_use]
    pub const fn time(self) -> f64 {
        self.time
    }

    /// Returns the coordinate as a `[space, time]` array.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::SpacetimeCoordinate;
    ///
    /// let coordinate = SpacetimeCoordinate::try_new(1.5, 2.0)?;
    /// assert_eq!(coordinate.to_array(), [1.5, 2.0]);
    /// # Ok::<(), causal_triangulations::prelude::geometry::SpacetimeCoordinateError>(())
    /// ```
    #[must_use]
    pub const fn to_array(self) -> [f64; SPACETIME_DIMENSION] {
        [self.space, self.time]
    }

    /// Returns the JSON column labels matching [`Self::to_array`].
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::SpacetimeCoordinate;
    ///
    /// assert_eq!(SpacetimeCoordinate::coordinate_columns(), ["x", "y"]);
    /// ```
    #[must_use]
    pub const fn coordinate_columns() -> [&'static str; SPACETIME_DIMENSION] {
        SPACETIME_COORDINATE_COLUMNS
    }
}

impl TryFrom<[f64; SPACETIME_DIMENSION]> for SpacetimeCoordinate {
    type Error = SpacetimeCoordinateError;

    fn try_from(coordinates: [f64; SPACETIME_DIMENSION]) -> Result<Self, Self::Error> {
        let [space, time] = coordinates;
        Self::try_new(space, time)
    }
}

impl TryFrom<&[f64]> for SpacetimeCoordinate {
    type Error = SpacetimeCoordinateError;

    fn try_from(coordinates: &[f64]) -> Result<Self, Self::Error> {
        Self::try_from_space_time_slice(coordinates)
    }
}

impl Serialize for SpacetimeCoordinate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_array().serialize(serializer)
    }
}

/// Component names for spacetime coordinate validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpacetimeCoordinateComponent {
    /// Spatial `x` component.
    Space,
    /// Continuous time `y` component.
    Time,
}

impl fmt::Display for SpacetimeCoordinateComponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Space => formatter.write_str("space"),
            Self::Time => formatter.write_str("time"),
        }
    }
}

/// Failure to parse raw coordinates into a [`SpacetimeCoordinate`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SpacetimeCoordinateError {
    /// The raw coordinate vector did not have the required 1+1 arity.
    Dimension {
        /// Observed coordinate count.
        actual: usize,
        /// Expected coordinate count.
        expected: usize,
    },
    /// One coordinate component was `NaN` or infinite.
    NonFiniteComponent {
        /// Component that failed validation.
        component: SpacetimeCoordinateComponent,
        /// Observed non-finite value.
        value: f64,
    },
}

impl fmt::Display for SpacetimeCoordinateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimension { actual, expected } => write!(
                formatter,
                "spacetime coordinate expected {expected} components, got {actual}"
            ),
            Self::NonFiniteComponent { component, value } => {
                write!(
                    formatter,
                    "spacetime coordinate {component} component is non-finite: {value}"
                )
            }
        }
    }
}

impl StdError for SpacetimeCoordinateError {}

const fn validate_component(
    component: SpacetimeCoordinateComponent,
    value: f64,
) -> Result<(), SpacetimeCoordinateError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(SpacetimeCoordinateError::NonFiniteComponent { component, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use serde_json::to_value;
    use std::assert_matches;

    #[test]
    fn parses_finite_space_time_components() {
        let coordinate =
            SpacetimeCoordinate::try_new(1.25, 4.5).expect("finite components should parse");

        assert_relative_eq!(coordinate.space(), 1.25);
        assert_relative_eq!(coordinate.time(), 4.5);
        let [space, time] = coordinate.to_array();
        assert_relative_eq!(space, 1.25);
        assert_relative_eq!(time, 4.5);
        assert_eq!(SpacetimeCoordinate::coordinate_columns(), ["x", "y"]);
    }

    #[test]
    fn rejects_wrong_coordinate_dimension_before_storage() {
        let too_few = SpacetimeCoordinate::try_from_space_time_slice(&[1.0])
            .expect_err("too few spacetime components should be rejected");

        assert_matches!(
            too_few,
            SpacetimeCoordinateError::Dimension {
                actual: 1,
                expected: 2,
            }
        );

        let too_many = SpacetimeCoordinate::try_from_space_time_slice(&[1.0, 2.0, 3.0])
            .expect_err("too many spacetime components should be rejected");

        assert_matches!(
            too_many,
            SpacetimeCoordinateError::Dimension {
                actual: 3,
                expected: 2,
            }
        );
    }

    #[test]
    fn rejects_non_finite_components_before_storage() {
        for (space, time, expected_component) in [
            (f64::NAN, 0.0, SpacetimeCoordinateComponent::Space),
            (0.0, f64::INFINITY, SpacetimeCoordinateComponent::Time),
        ] {
            let error = SpacetimeCoordinate::try_new(space, time)
                .expect_err("non-finite spacetime components should be rejected");

            assert_matches!(
                error,
                SpacetimeCoordinateError::NonFiniteComponent {
                    component,
                    value,
                } if component == expected_component && !value.is_finite()
            );
        }
    }

    #[test]
    fn serializes_as_space_time_array_for_notebook_consumers() {
        let coordinate =
            SpacetimeCoordinate::try_new(2.0, 3.0).expect("finite components should parse");

        assert_eq!(
            to_value(coordinate).expect("coordinate should serialize"),
            serde_json::json!([2.0, 3.0])
        );
    }
}
