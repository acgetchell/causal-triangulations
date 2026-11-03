#![forbid(unsafe_code)]

//! 2D Regge Action calculation for Causal Dynamical Triangulations.
//!
//! This module implements the discrete Einstein-Hilbert action used in CDT,
//! which is based on the Regge calculus formulation of general relativity.

use crate::cdt::volume_fixing::{VolumeFixing, VolumeFixingError, VolumeFixingWire};
use crate::errors::{CdtError, CdtResult, ConfigurationSetting, ExpectedConstraint, ObservedValue};
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
/// # Errors
///
/// Returns [`CdtError::InvalidConfiguration`] when any coupling is non-finite or
/// when their combined magnitude cannot guarantee a finite action for all
/// representable simplex counts.
///
/// # Examples
///
/// ```
/// use approx::assert_relative_eq;
/// use causal_triangulations::prelude::action::compute_regge_action;
///
/// let action = compute_regge_action(10, 20, 15, 1.0, 1.0, 0.1)?;
/// assert_relative_eq!(action, -23.0, epsilon = 1e-12);
/// # Ok::<(), causal_triangulations::CdtError>(())
/// ```
pub fn compute_regge_action(
    vertices: usize,
    edges: usize,
    triangles: usize,
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
) -> CdtResult<f64> {
    let config = ActionConfig::new(coupling_0, coupling_2, cosmological_constant)?;
    Ok(config.calculate_action(vertices, edges, triangles))
}

/// Evaluates the action after the couplings have established the finite-range invariant.
fn compute_regge_action_unchecked(
    vertices: usize,
    edges: usize,
    triangles: usize,
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
) -> f64 {
    // Preserve product roundoff until all three signed terms have cancelled.
    // Nested FMA alone can leave an O(1) spurious action when equal, large
    // couplings multiply counts satisfying E - V - F = 0.
    let mut expansion = [0.0; 12];
    let mut length = 0;
    for (count, coupling) in [
        (vertices, -coupling_0),
        (triangles, -coupling_2),
        (edges, cosmological_constant),
    ] {
        // Two exactly represented 32-bit limbs also retain integer counts
        // above 2^53. On 32-bit targets the high limb is zero.
        let low = u32::try_from(count & 0xffff_ffff).expect("masked count fits u32");
        let high = u32::try_from(count.checked_shr(32).unwrap_or(0))
            .expect("supported usize has at most 64 bits");
        for limb in [
            <f64 as From<u32>>::from(low),
            <f64 as From<u32>>::from(high) * 4_294_967_296.0,
        ] {
            if limb == 0.0 || coupling == 0.0 {
                continue;
            }
            let product = coupling * limb;
            let residual = coupling.mul_add(limb, -product);
            grow_action_expansion(&mut expansion, &mut length, residual);
            grow_action_expansion(&mut expansion, &mut length, product);
        }
    }
    expansion[..length].iter().copied().sum()
}

/// Adds one term with Two-Sum / Grow-Expansion (Shewchuk, 1997, §2.4).
/// <https://www.cs.cmu.edu/~quake/robust.html>
///
/// Each addition retains its roundoff as a separate, smaller component. The
/// validated absolute action bound prevents overflow. Integer limbs ensure
/// product residuals remain multiples of the smallest subnormal, so FMA can
/// represent them even at the underflow boundary. At most twelve terms enter;
/// the fixed expansion needs no allocation. Only its final estimate is rounded.
fn grow_action_expansion(expansion: &mut [f64; 12], length: &mut usize, mut term: f64) {
    let mut output = 0;
    for index in 0..*length {
        let other = expansion[index];
        let sum = term + other;
        let other_virtual = sum - term;
        let term_virtual = sum - other_virtual;
        let residual = (term - term_virtual) + (other - other_virtual);
        if residual != 0.0 {
            expansion[output] = residual;
            output += 1;
        }
        term = sum;
    }
    if term != 0.0 {
        expansion[output] = term;
        output += 1;
    }
    *length = output;
}

/// Validated configuration for CDT action parameters.
///
/// The stored couplings are finite and jointly bounded so evaluating the action
/// for any representable `usize` simplex counts remains finite. Raw action
/// parameters enter through [`Self::new`] or deserialization, which establish
/// that stronger arithmetic invariant before storage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionConfig {
    /// Optional explicitly modified total-volume ensemble.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    volume_fixing: Option<VolumeFixing>,
    /// Coupling constant for vertices (κ₀)
    coupling_0: f64,
    /// Coupling constant for triangles (κ₂)
    coupling_2: f64,
    /// Cosmological constant (λ)
    cosmological_constant: f64,
}

#[derive(Deserialize)]
pub(crate) struct ActionConfigWire {
    #[serde(default)]
    volume_fixing: Option<VolumeFixingWire>,
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
}

impl<'de> Deserialize<'de> for ActionConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ActionConfigWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl TryFrom<ActionConfigWire> for ActionConfig {
    type Error = CdtError;

    fn try_from(wire: ActionConfigWire) -> CdtResult<Self> {
        Self::new(wire.coupling_0, wire.coupling_2, wire.cosmological_constant)?
            .with_volume_fixing(wire.volume_fixing.map(VolumeFixing::try_from).transpose()?)
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
    /// infinite, or when their combined magnitude could overflow action
    /// evaluation for representable simplex counts.
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
        validate_action_range(coupling_0, coupling_2, cosmological_constant)?;
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
            volume_fixing: None,
            coupling_0,
            coupling_2,
            cosmological_constant,
        }
    }

    /// Retains the bias already range-checked by `ValidatedCdtConfig`.
    pub(crate) const fn with_validated_volume_fixing(
        mut self,
        fixing: Option<VolumeFixing>,
    ) -> Self {
        self.volume_fixing = fixing;
        self
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
        debug_assert!(
            maximum_action_magnitude(self.coupling_0, self.coupling_2, self.cosmological_constant)
                .is_some()
        );
    }

    /// Calculates the action for given simplex counts.
    ///
    /// Integer counts are retained exactly through product accumulation on
    /// supported 32-bit and 64-bit targets; the final action is rounded to `f64`.
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
        let physical = self.physical_action(vertices, edges, triangles);
        self.volume_fixing
            .map_or(physical, |fixing| physical + fixing.action(triangles))
    }

    /// Evaluates only the physical Regge action, excluding any explicit bias.
    #[must_use]
    pub fn physical_action(&self, vertices: usize, edges: usize, triangles: usize) -> f64 {
        let action = compute_regge_action_unchecked(
            vertices,
            edges,
            triangles,
            self.coupling_0,
            self.coupling_2,
            self.cosmological_constant,
        );
        debug_assert!(action.is_finite());
        action
    }

    /// Returns a conservative upper bound for the magnitude of any action
    /// representable by this configuration's simplex-count types.
    pub(crate) fn maximum_action_magnitude(&self) -> f64 {
        maximum_action_magnitude(self.coupling_0, self.coupling_2, self.cosmological_constant)
            .expect("validated action couplings must retain a finite magnitude bound")
            + self.volume_fixing.map_or(0.0, VolumeFixing::maximum_action)
    }

    /// Returns the optional explicit total-triangle bias.
    #[must_use]
    pub const fn volume_fixing(&self) -> Option<VolumeFixing> {
        self.volume_fixing
    }

    /// Evaluates only the fixing action, or zero in the default bare ensemble.
    #[must_use]
    pub fn fixing_action(&self, triangles: usize) -> f64 {
        self.volume_fixing
            .map_or(0.0, |fixing| fixing.action(triangles))
    }

    /// Returns a configuration with an explicit bias after checking combined range.
    ///
    /// # Errors
    /// Returns a typed overflow error if the combined action range is not finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::action::{ActionConfig, VolumeFixing};
    /// let action = ActionConfig::default()
    ///     .with_volume_fixing(Some(VolumeFixing::quadratic_triangles(32, 0.1)?))?;
    /// assert_eq!(action.volume_fixing().map(|fixing| fixing.target().get()), Some(32));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn with_volume_fixing(mut self, fixing: Option<VolumeFixing>) -> CdtResult<Self> {
        self.volume_fixing = fixing;
        if !self.maximum_action_magnitude().is_finite() {
            return Err(VolumeFixingError::CombinedActionOverflow.into());
        }
        Ok(self)
    }
}

/// Rejects non-finite action couplings before they can poison action/log-probability math.
fn validate_coupling(setting: ConfigurationSetting, value: f64) -> CdtResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(CdtError::InvalidConfiguration {
            setting,
            provided_value: value.into(),
            expected: ExpectedConstraint::Finite,
        })
    }
}

/// Computes a conservative finite action bound for all `usize` count triples.
fn maximum_action_magnitude(
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
) -> Option<f64> {
    let maximum_count: f64 = NumCast::from(usize::MAX)?;
    let coupling_sum = coupling_0.abs() + coupling_2.abs() + cosmological_constant.abs();
    if !coupling_sum.is_finite() {
        return None;
    }
    let bound = maximum_count * coupling_sum;
    bound.is_finite().then_some(bound)
}

/// Rejects jointly finite couplings whose arithmetic range is still unsafe.
fn validate_action_range(
    coupling_0: f64,
    coupling_2: f64,
    cosmological_constant: f64,
) -> CdtResult<()> {
    if maximum_action_magnitude(coupling_0, coupling_2, cosmological_constant).is_some() {
        return Ok(());
    }

    Err(CdtError::InvalidConfiguration {
        setting: ConfigurationSetting::ActionCouplings,
        provided_value: ObservedValue::ActionCouplings {
            coupling_0,
            coupling_2,
            cosmological_constant,
        },
        expected: ExpectedConstraint::FiniteAction {
            maximum_simplex_count: usize::MAX,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use serde_json::from_str;

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
        )
        .expect("finite couplings should produce a finite action");

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
    fn equal_large_couplings_preserve_torus_action_and_boltzmann_ratio() {
        let coupling = 9_007_199_254_740_994.0; // exactly 2^53 + 2
        let config = ActionConfig::new(coupling, coupling, coupling).unwrap();
        let before = config.calculate_action(16, 48, 32);
        let after = config.calculate_action(15, 45, 30);
        // Independently: a(E - V - F) = -a χ = 0 on both toroidal states.
        assert_eq!(before, 0.0);
        assert_eq!(after, 0.0);
        assert_eq!((before - after).exp(), 1.0);
    }

    #[test]
    fn cancelling_actions_match_exact_integer_oracle() {
        let a = 9_007_199_254_740_994_i128;
        for (vertices, edges, triangles) in [(16, 48, 32), (15, 45, 30), (4, 6, 4)] {
            for (k0, k2, lambda) in [(a, a, a), (a + 2, a - 2, a), (-a, -a, -a)] {
                let exact = -k0 * vertices - k2 * triangles + lambda * edges;
                let config = ActionConfig::new(
                    NumCast::from(k0).unwrap(),
                    NumCast::from(k2).unwrap(),
                    NumCast::from(lambda).unwrap(),
                )
                .unwrap();
                let actual = config.calculate_action(
                    usize::try_from(vertices).unwrap(),
                    usize::try_from(edges).unwrap(),
                    usize::try_from(triangles).unwrap(),
                );
                assert_eq!(actual, <f64 as NumCast>::from(exact).unwrap());
            }
        }
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn action_retains_count_units_above_f64_integer_precision() {
        let count = (1_usize << 53) + 1;
        let config = ActionConfig::new(1.0, 1.0, 1.0).unwrap();
        assert_eq!(config.calculate_action(count, count + 2, 1), 1.0);
        let subnormal = f64::from_bits(1);
        let config = ActionConfig::new(subnormal, subnormal, subnormal).unwrap();
        assert_eq!(config.calculate_action(count, count + 2, 1), subnormal);
    }

    #[test]
    fn action_config_deserialization_rejects_non_finite_couplings() {
        let payload = r#"{"coupling_0":null,"coupling_2":0.0,"cosmological_constant":0.0}"#;
        let error = from_str::<ActionConfig>(payload)
            .expect_err("non-finite action coupling should be rejected");

        assert!(
            error.to_string().contains("invalid type"),
            "serde error should reject non-number coupling before storage, got {error}"
        );

        let payload = r#"{"coupling_0":1e999,"coupling_2":0.0,"cosmological_constant":0.0}"#;
        let error = from_str::<ActionConfig>(payload)
            .expect_err("infinite action coupling should be rejected");

        assert!(
            error.to_string().contains("number out of range"),
            "serde error should reject out-of-range JSON numbers before storage, got {error}"
        );
        assert!(ActionConfig::new(f64::INFINITY, 0.0, 0.0).is_err());
    }

    #[test]
    fn action_config_rejects_finite_couplings_that_can_overflow_actions() {
        let error = ActionConfig::new(f64::MAX, 0.0, 0.0)
            .expect_err("finite but unbounded coupling must not enter action evaluation");

        assert!(matches!(
            error,
            CdtError::InvalidConfiguration {
                setting: ConfigurationSetting::ActionCouplings,
                ..
            }
        ));
    }

    #[test]
    fn direct_action_calculation_rejects_invalid_couplings() {
        for couplings in [
            (f64::NAN, 0.0, 0.0),
            (0.0, f64::INFINITY, 0.0),
            (0.0, 0.0, f64::NEG_INFINITY),
            (f64::MAX, 0.0, 0.0),
        ] {
            assert!(
                compute_regge_action(1, 1, 1, couplings.0, couplings.1, couplings.2).is_err(),
                "invalid direct-action couplings should be rejected: {couplings:?}"
            );
        }
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
            ).expect("bounded proptest couplings should be valid");

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
            ).expect("bounded proptest couplings should be valid");

            prop_assert!(
                relative_eq!(action_config, action_direct, epsilon = f64::EPSILON),
                "Config-based and direct calculations should match: {} vs {}",
                action_config, action_direct
            );
        }
    }
}
