#![forbid(unsafe_code)]

//! 2D Regge Action calculation for Causal Dynamical Triangulations.
//!
//! This module implements the discrete Einstein-Hilbert action used in CDT,
//! which is based on the Regge calculus formulation of general relativity.

use crate::errors::{CdtError, CdtResult, ConfigurationSetting};
use num_traits::cast::NumCast;
use serde::{Deserialize, Deserializer, Serialize};
use std::f64::consts::LN_2;

/// Critical cosmological coupling for pure 1+1 CDT in the triangle-volume convention.
///
/// The exactly solved 2D CDT transfer matrix has critical point `λ_c = ln 2`
/// when triangulations are weighted by `exp(-λ N2)`, where `N2` is the number
/// of triangles.
pub const CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT: f64 = LN_2;

/// Default edge-count cosmological coupling for toroidal 1+1 CDT runs.
///
/// This crate's historical action writes the cosmological term as `λ N1`.
/// For closed toroidal 1+1 CDT triangulations, `N1 = 3 N2 / 2`, so this value
/// maps the edge-count convention to the standard 2D CDT critical triangle
/// coupling `λ_c = ln 2`.
pub const DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT: f64 =
    2.0 * CDT_1P1_CRITICAL_TRIANGLE_COSMOLOGICAL_CONSTANT / 3.0;

/// Calculates the 2D Regge Action for a given triangulation.
///
/// The 2D Regge Action in CDT is given by:
/// S = -κ₀ N₀ - κ₂ N₂ + λ N₁
/// where:
/// - N₀ = number of vertices (0-simplices)
/// - N₁ = number of edges (1-simplices)
/// - N₂ = number of triangles (2-simplices)
/// - κ₀, κ₂ = coupling constants
/// - λ = cosmological constant
///
/// # Arguments
///
/// * `vertices` - Number of vertices in the triangulation
/// * `edges` - Number of edges in the triangulation
/// * `triangles` - Number of triangles in the triangulation
/// * `coupling_0` - Coupling constant κ₀ for vertices
/// * `coupling_2` - Coupling constant κ₂ for triangles
/// * `cosmological_constant` - Cosmological constant λ
///
/// # Returns
///
/// The calculated Regge Action value
///
/// # Panics
///
/// Panics only if the platform cannot represent a `usize` simplex count as a
/// finite `f64`. Supported targets represent all `usize` counts as finite
/// floating-point values, though very large counts may lose integer precision.
///
/// # Examples
///
/// ```
/// use approx::assert_relative_eq;
/// use causal_triangulations::prelude::action::compute_regge_action;
///
/// let action = compute_regge_action(10, 20, 15, 1.0, 1.0, 0.1);
/// assert_relative_eq!(action, -23.0, epsilon = 1e-12);
/// ```
#[must_use]
pub fn compute_regge_action(
    vertices: usize,
    edges: usize,
    triangles: usize,
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
) -> f64 {
    let n_0: f64 = NumCast::from(vertices).expect("usize vertex counts should fit finite f64");
    let n_1: f64 = NumCast::from(edges).expect("usize edge counts should fit finite f64");
    let n_2: f64 = NumCast::from(triangles).expect("usize triangle counts should fit finite f64");

    cosmological_constant.mul_add(n_1, (-coupling_0).mul_add(n_0, -(coupling_2 * n_2)))
}

/// Validated configuration for CDT action parameters.
///
/// The stored couplings are always finite. Raw action parameters enter through
/// [`Self::new`] or deserialization, both of which reject NaN and infinite values
/// before the configuration can be used by action or Metropolis code.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionConfig {
    /// Coupling constant for vertices (κ₀)
    coupling_0: f64,
    /// Coupling constant for triangles (κ₂)
    coupling_2: f64,
    /// Cosmological constant (λ)
    cosmological_constant: f64,
}

#[derive(Deserialize)]
struct ActionConfigWire {
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
}

impl<'de> Deserialize<'de> for ActionConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ActionConfigWire::deserialize(deserializer)?;
        Self::new(wire.coupling_0, wire.coupling_2, wire.cosmological_constant)
            .map_err(serde::de::Error::custom)
    }
}

impl Default for ActionConfig {
    /// Default CDT action parameters for 1+1 CDT simulations.
    ///
    /// In pure 1+1 gravity the curvature/Newton term is topological at fixed
    /// topology. The defaults therefore leave the vertex and triangle couplings
    /// at zero and use the edge-count cosmological coupling that maps to the
    /// standard 2D CDT critical value `λ_c = ln 2` for toroidal triangulations.
    fn default() -> Self {
        Self::from_validated_parts(0.0, 0.0, DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT)
    }
}

impl ActionConfig {
    /// Creates a new validated action configuration.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::InvalidConfiguration`] when any coupling is NaN or
    /// infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::ActionConfig;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2)?;
    /// assert_relative_eq!(config.coupling_0(), 2.0);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn new(coupling_0: f64, coupling_2: f64, cosmological_constant: f64) -> CdtResult<Self> {
        validate_coupling(ConfigurationSetting::Coupling0, coupling_0)?;
        validate_coupling(ConfigurationSetting::Coupling2, coupling_2)?;
        validate_coupling(
            ConfigurationSetting::CosmologicalConstant,
            cosmological_constant,
        )?;
        Ok(Self::from_validated_parts(
            coupling_0,
            coupling_2,
            cosmological_constant,
        ))
    }

    /// Builds an action configuration after the caller has already checked coupling finiteness.
    ///
    /// This keeps validated higher-level configuration conversion infallible
    /// without opening a second public path that could store invalid couplings.
    pub(crate) const fn from_validated_parts(
        coupling_0: f64,
        coupling_2: f64,
        cosmological_constant: f64,
    ) -> Self {
        Self {
            coupling_0,
            coupling_2,
            cosmological_constant,
        }
    }

    /// Returns the vertex coupling (κ₀).
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::ActionConfig;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2)?;
    /// assert_relative_eq!(config.coupling_0(), 2.0);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn coupling_0(&self) -> f64 {
        self.coupling_0
    }

    /// Returns the triangle coupling (κ₂).
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::ActionConfig;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2)?;
    /// assert_relative_eq!(config.coupling_2(), 1.5);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn coupling_2(&self) -> f64 {
        self.coupling_2
    }

    /// Returns the cosmological coupling (λ).
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::ActionConfig;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2)?;
    /// assert_relative_eq!(config.cosmological_constant(), 0.2);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub const fn cosmological_constant(&self) -> f64 {
        self.cosmological_constant
    }

    /// Confirms that stored action couplings are finite.
    ///
    /// This method is kept for code that wants a common validation hook across
    /// configuration-like types. Because [`ActionConfig`] validates before
    /// storage, it is an infallible debug assertion of the stored invariant.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::action::ActionConfig;
    ///
    /// ActionConfig::default().validate();
    /// assert!(ActionConfig::new(f64::NAN, 1.0, 0.1).is_err());
    /// ```
    pub fn validate(&self) {
        debug_assert!(self.coupling_0.is_finite());
        debug_assert!(self.coupling_2.is_finite());
        debug_assert!(self.cosmological_constant.is_finite());
    }

    /// Calculates the action for given simplex counts.
    ///
    /// # Panics
    ///
    /// Panics only if the platform cannot represent a `usize` simplex count as a
    /// finite `f64`. Supported targets represent all `usize` counts as finite
    /// floating-point values, though very large counts may lose integer precision.
    ///
    /// # Examples
    ///
    /// ```
    /// use approx::assert_relative_eq;
    /// use causal_triangulations::prelude::action::ActionConfig;
    ///
    /// let config = ActionConfig::new(2.0, 1.5, 0.2)?;
    /// let action = config.calculate_action(5, 10, 8);
    /// assert_relative_eq!(action, -20.0, epsilon = 1e-12);
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    #[must_use]
    pub fn calculate_action(&self, vertices: usize, edges: usize, triangles: usize) -> f64 {
        compute_regge_action(
            vertices,
            edges,
            triangles,
            self.coupling_0,
            self.coupling_2,
            self.cosmological_constant,
        )
    }
}

/// Rejects non-finite action couplings before they can poison action/log-probability math.
fn validate_coupling(setting: ConfigurationSetting, value: f64) -> CdtResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(CdtError::InvalidConfiguration {
            setting,
            provided_value: value.to_string(),
            expected: "finite".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_regge_action_calculation() {
        let vertices = 10;
        let edges = 20;
        let triangles = 15;
        let coupling_0 = 1.0;
        let coupling_2 = 1.0;
        let cosmological_constant = 0.1;

        let action = compute_regge_action(
            vertices,
            edges,
            triangles,
            coupling_0,
            coupling_2,
            cosmological_constant,
        );

        // Expected: -1.0 * 10 - 1.0 * 15 + 0.1 * 20 = -10 - 15 + 2 = -23
        let expected = -23.0;
        assert_relative_eq!(action, expected);
    }

    #[test]
    fn test_action_config_default() {
        let config = ActionConfig::default();
        assert_relative_eq!(config.coupling_0(), 0.0);
        assert_relative_eq!(config.coupling_2(), 0.0);
        assert_relative_eq!(
            config.cosmological_constant(),
            DEFAULT_CDT_1P1_EDGE_COSMOLOGICAL_CONSTANT
        );
    }

    #[test]
    fn test_action_config_calculate() {
        let config = ActionConfig::new(2.0, 1.5, 0.2).expect("finite couplings are valid");
        let action = config.calculate_action(5, 10, 8);

        // Expected: -2.0 * 5 - 1.5 * 8 + 0.2 * 10 = -10 - 12 + 2 = -20
        let expected = -20.0;
        assert_relative_eq!(action, expected);
    }

    #[test]
    fn action_config_deserialization_rejects_non_finite_couplings() {
        let payload = r#"{"coupling_0":null,"coupling_2":0.0,"cosmological_constant":0.0}"#;
        let error = serde_json::from_str::<ActionConfig>(payload)
            .expect_err("non-finite action coupling should be rejected");

        assert!(
            error.to_string().contains("invalid type"),
            "serde error should reject non-number coupling before storage, got {error}"
        );

        let payload = r#"{"coupling_0":1e999,"coupling_2":0.0,"cosmological_constant":0.0}"#;
        let error = serde_json::from_str::<ActionConfig>(payload)
            .expect_err("infinite action coupling should be rejected");

        assert!(
            error.to_string().contains("number out of range"),
            "serde error should reject out-of-range JSON numbers before storage, got {error}"
        );
        assert!(ActionConfig::new(f64::INFINITY, 0.0, 0.0).is_err());
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use approx::relative_eq;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn action_always_finite(
            vertices in 0usize..100,
            edges in 0usize..500,
            triangles in 0usize..300,
            coupling_0 in -10.0f64..10.0,
            coupling_2 in -10.0f64..10.0,
            cosmological_constant in -5.0f64..5.0
        ) {
            let action = compute_regge_action(
                vertices, edges, triangles,
                coupling_0, coupling_2, cosmological_constant
            );

            prop_assert!(action.is_finite(), "Action must always be finite, got: {}", action);
            prop_assert!(!action.is_nan(), "Action must not be NaN");
        }

        #[test]
        fn action_config_consistency(
            vertices in 0usize..50,
            edges in 0usize..150,
            triangles in 0usize..100,
            coupling_0 in -5.0f64..5.0,
            coupling_2 in -5.0f64..5.0,
            cosmological_constant in -2.0f64..2.0
        ) {
            let config = ActionConfig::new(coupling_0, coupling_2, cosmological_constant)
                .expect("proptest finite ranges are valid action couplings");

            // Config should preserve values
            prop_assert!(relative_eq!(
                config.coupling_0(),
                coupling_0,
                epsilon = f64::EPSILON
            ));
            prop_assert!(relative_eq!(
                config.coupling_2(),
                coupling_2,
                epsilon = f64::EPSILON
            ));
            prop_assert!(relative_eq!(
                config.cosmological_constant(),
                cosmological_constant,
                epsilon = f64::EPSILON
            ));

            // Config-based calculation should match direct function call
            let action_config = config.calculate_action(vertices, edges, triangles);
            let action_direct = compute_regge_action(
                vertices, edges, triangles,
                coupling_0, coupling_2, cosmological_constant
            );

            prop_assert!(
                relative_eq!(action_config, action_direct, epsilon = f64::EPSILON),
                "Config-based and direct calculations should match: {} vs {}",
                action_config, action_direct
            );
        }
    }
}
