#![forbid(unsafe_code)]

//! Downstream contracts for typed validation diagnostics.

use causal_triangulations::prelude::errors::{
    CdtError, ConfigurationSetting, ExpectedConstraint, GenerationParameterIssue, ObservedValue,
};
use causal_triangulations::prelude::geometry::{build_delaunay2_with_data, generate_delaunay2};
use causal_triangulations::prelude::simulation::{ActionConfig, CdtTarget, MetropolisConfig};
use causal_triangulations::{CdtConfig, CdtConfigOverrides, CdtTopology, CdtTriangulation};
use std::assert_matches;

#[test]
fn temperature_diagnostics_preserve_original_bits_across_entry_points() {
    for temperature in [
        f64::from_bits(0x7ff8_0000_0000_0042),
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        f64::from_bits(1),
    ] {
        let config = CdtConfig {
            temperature,
            ..CdtConfig::new(36, 3)
        };
        let overrides = CdtConfigOverrides {
            temperature: Some(temperature),
            ..CdtConfigOverrides::default()
        };
        let errors = [
            config.into_validated().expect_err("invalid temperature"),
            CdtConfig::new(36, 3)
                .merge_with_override(&overrides)
                .expect_err("invalid override"),
            MetropolisConfig::new(temperature, 10, 2, 2).expect_err("invalid schedule temperature"),
            CdtTarget::new(ActionConfig::default(), temperature)
                .err()
                .expect("invalid target temperature"),
        ];
        for error in errors {
            assert_matches!(error, CdtError::InvalidSimulationConfiguration {
                setting: ConfigurationSetting::Temperature,
                provided_value: ObservedValue::Float(value),
                expected: ExpectedConstraint::PositiveFiniteReciprocal,
            } if value.to_bits() == temperature.to_bits());
        }
    }
}

#[test]
fn non_finite_coupling_keeps_its_category_and_nan_payload() {
    let value = f64::from_bits(0xfff8_0000_0000_1234);
    for (couplings, expected_setting) in [
        ([value, 0.0, 0.0], ConfigurationSetting::Coupling0),
        ([0.0, value, 0.0], ConfigurationSetting::Coupling2),
        (
            [0.0, 0.0, value],
            ConfigurationSetting::CosmologicalConstant,
        ),
    ] {
        assert_matches!(ActionConfig::new(couplings[0], couplings[1], couplings[2]),
            Err(CdtError::InvalidConfiguration {
                setting,
                provided_value: ObservedValue::Float(actual),
                expected: ExpectedConstraint::Finite,
            }) if setting == expected_setting && actual.to_bits() == value.to_bits());
    }
}

#[test]
fn action_and_target_range_failures_expose_distinct_constraints() {
    assert_matches!(ActionConfig::new(f64::MAX, -1.0, 0.0),
        Err(CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::ActionCouplings,
            provided_value: ObservedValue::ActionCouplings {
                coupling_0, coupling_2, cosmological_constant,
            },
            expected: ExpectedConstraint::FiniteAction { maximum_simplex_count: usize::MAX },
        }) if coupling_0.to_bits() == f64::MAX.to_bits()
            && coupling_2.to_bits() == (-1.0_f64).to_bits()
            && cosmological_constant.to_bits() == 0.0_f64.to_bits());

    assert_matches!(CdtTarget::new(ActionConfig::default(), f64::MIN_POSITIVE).err(),
        Some(CdtError::InvalidSimulationConfiguration {
            setting: ConfigurationSetting::Temperature,
            provided_value: ObservedValue::Float(value),
            expected: ExpectedConstraint::FiniteLogProbability { maximum_action_magnitude },
        }) if value.to_bits() == f64::MIN_POSITIVE.to_bits()
            && maximum_action_magnitude.is_finite()
            && (maximum_action_magnitude / value).is_infinite());
}

#[test]
fn geometry_diagnostics_preserve_bounds_and_coordinate_locations() {
    let nan = f64::from_bits(0x7ff8_0000_0000_5678);
    for range in [(nan, 1.0), (-0.0, f64::INFINITY), (2.0, 1.0)] {
        assert_matches!(generate_delaunay2(4, range, Some(42)),
            Err(CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::InvalidCoordinateRange,
                provided_value: ObservedValue::CoordinateRange { min, max },
                expected_range: ExpectedConstraint::FiniteIncreasingBounds,
            }) if min.to_bits() == range.0.to_bits() && max.to_bits() == range.1.to_bits());
    }

    let vertices = [([0.0, 0.0], 0), ([1.0, nan], 0), ([0.0, 1.0], 1)];
    assert_matches!(build_delaunay2_with_data(&vertices),
        Err(CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::NonFiniteVertexCoordinate,
            provided_value: ObservedValue::VertexCoordinate { vertex_index: 1, axis: 1, value },
            expected_range: ExpectedConstraint::Finite,
        }) if value.to_bits() == nan.to_bits());
}

#[test]
fn profile_and_count_overflows_retain_inputs_without_overflowing_diagnostics() {
    let config = CdtConfig {
        topology: CdtTopology::Toroidal,
        spatial_vertex_profile: Some(vec![u32::MAX, 3, 3]),
        ..CdtConfig::new(12, 3)
    };
    assert_matches!(config.into_validated(), Err(CdtError::InvalidConfiguration {
        setting: ConfigurationSetting::SpatialVertexProfile,
        provided_value: ObservedValue::SpatialVertexProfile(profile),
        expected: ExpectedConstraint::ProfileSumFitsU32,
    }) if profile == [u32::MAX, 3, 3]);

    let error = CdtTriangulation::from_toroidal_cdt(u32::MAX, 3)
        .expect_err("vertex product cannot fit u32");
    assert_matches!(
        &error,
        CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::VertexCountOverflow,
            provided_value: ObservedValue::CountProduct {
                left: u32::MAX,
                right: 3
            },
            expected_range: ExpectedConstraint::FitsU32,
        }
    );
    assert!(error.to_string().contains("4294967295 × 3"));
    assert!(error.to_string().contains("must fit in u32"));
}

#[test]
fn count_conversions_preserve_the_full_platform_range() {
    assert_matches!(ObservedValue::from(usize::MAX), ObservedValue::Count(value)
        if value == usize::MAX as u128);
    assert_matches!(
        ObservedValue::from(u32::MAX),
        ObservedValue::Count(4_294_967_295)
    );
    assert_matches!(ObservedValue::from(u8::MAX), ObservedValue::Count(255));
}
