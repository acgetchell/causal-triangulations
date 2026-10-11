#![forbid(unsafe_code)]

//! Downstream contracts for typed validation diagnostics.

use causal_triangulations::prelude::errors::{
    CdtError, ConfigurationSetting, ExpectedConstraint, GenerationParameterIssue, ObservedValue,
};
use causal_triangulations::prelude::geometry::{
    build_delaunay2_with_data, build_toroidal_delaunay2, generate_delaunay2,
};
use causal_triangulations::prelude::simulation::{ActionConfig, CdtTarget, MetropolisConfig};
use causal_triangulations::{CdtConfig, CdtConfigOverrides, CdtTopology, CdtTriangulation};
use std::assert_matches;

/// Check actionable context without treating complete diagnostic prose as a schema.
fn assert_rendered_context(error: &CdtError, fragments: &[&str]) {
    let rendered = error.to_string();
    for fragment in fragments {
        assert!(
            rendered.contains(fragment),
            "missing {fragment:?} in {rendered:?}"
        );
    }
}

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
            assert_rendered_context(&error, &["temperature", "finite reciprocal"]);
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
    let error = ActionConfig::new(f64::MAX, -1.0, 0.0).expect_err("action bound overflows");
    assert_rendered_context(
        &error,
        &["-1, 0]", "simplex counts", &usize::MAX.to_string()],
    );
    assert_matches!(error,
        CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::ActionCouplings,
            provided_value: ObservedValue::ActionCouplings {
                coupling_0, coupling_2, cosmological_constant,
            },
            expected: ExpectedConstraint::FiniteAction { maximum_simplex_count: usize::MAX },
        } if coupling_0.to_bits() == f64::MAX.to_bits()
            && coupling_2.to_bits() == (-1.0_f64).to_bits()
            && cosmological_constant.to_bits() == 0.0_f64.to_bits());

    let error = CdtTarget::new(ActionConfig::default(), f64::MIN_POSITIVE)
        .err()
        .expect("log-probability bound overflows");
    assert_rendered_context(
        &error,
        &["temperature", "-action / temperature", "action magnitude"],
    );
    assert_matches!(error,
        CdtError::InvalidSimulationConfiguration {
            setting: ConfigurationSetting::Temperature,
            provided_value: ObservedValue::Float(value),
            expected: ExpectedConstraint::FiniteLogProbability { maximum_action_magnitude },
        } if value.to_bits() == f64::MIN_POSITIVE.to_bits()
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
    let error = build_delaunay2_with_data(&vertices).expect_err("non-finite coordinate");
    assert_rendered_context(&error, &["vertex 1", "axis 1", "NaN", "finite"]);
    assert_matches!(error,
        CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::NonFiniteVertexCoordinate,
            provided_value: ObservedValue::VertexCoordinate { vertex_index: 1, axis: 1, value },
            expected_range: ExpectedConstraint::Finite,
        } if value.to_bits() == nan.to_bits());
}

#[test]
fn configuration_diagnostics_render_relational_constraints() {
    // Each fixture fails a different relation; existing typed-payload tests
    // protect the machine-readable contract independently of these log checks.
    for (config, fragments) in [
        (CdtConfig::new(12, 1), vec!["1", "≥ 2", "open"]),
        (
            CdtConfig::new(13, 3),
            vec!["13", "divisible", "(3)", "open"],
        ),
        (CdtConfig::new(9, 3), vec!["9", "≥ 4", "(12)", "open"]),
        (
            CdtConfig::new(u32::MAX, u32::MAX),
            vec!["4294967295", "4 · timeslices", "fit in u32", "open"],
        ),
        (
            CdtConfig {
                dimension: Some(4),
                ..CdtConfig::new(12, 3)
            },
            vec!["dimension", "4", "2"],
        ),
        (
            CdtConfig {
                spatial_vertex_profile: Some(vec![4, 4]),
                ..CdtConfig::new(12, 3)
            },
            vec!["2", "3 entries"],
        ),
        (
            CdtConfig {
                spatial_vertex_profile: Some(vec![4, 3, 5]),
                ..CdtConfig::new(12, 3)
            },
            vec!["slice 1", "3", "≥ 4", "open"],
        ),
        (
            CdtConfig {
                spatial_vertex_profile: Some(vec![4, 5, 6]),
                ..CdtConfig::new(12, 3)
            },
            vec!["12", "spatial_vertex_profile (15)"],
        ),
        (
            CdtConfig {
                spatial_vertex_profile: Some(vec![u32::MAX, 4, 4]),
                ..CdtConfig::new(12, 3)
            },
            vec!["[4294967295, 4, 4]", "sum must fit in u32"],
        ),
    ] {
        let error = config
            .into_validated()
            .expect_err("invalid configuration relation");
        assert_matches!(&error, CdtError::InvalidConfiguration { .. });
        assert_rendered_context(&error, &fragments);
    }
}

#[test]
fn schedule_diagnostics_render_counts_and_joint_settings() {
    let error =
        MetropolisConfig::new(1.0, 19, 15, 10).expect_err("no measurement after thermalization");
    assert_matches!(
        &error,
        CdtError::InvalidSimulationConfiguration {
            setting: ConfigurationSetting::MeasurementSchedule,
            provided_value: ObservedValue::MeasurementSchedule {
                steps: 19,
                thermalization_steps: 15,
                measurement_frequency: 10,
            },
            expected: ExpectedConstraint::PostThermalizationMeasurement,
        }
    );
    assert_rendered_context(
        &error,
        &[
            "steps=19",
            "thermalization_steps=15",
            "measurement_frequency=10",
            "post-thermalization measurement",
        ],
    );

    let error = MetropolisConfig::new(1.0, 10, 11, 2).expect_err("thermalization exceeds steps");
    assert_matches!(
        &error,
        CdtError::InvalidSimulationConfiguration {
            setting: ConfigurationSetting::ThermalizationSteps,
            provided_value: ObservedValue::Count(11),
            expected: ExpectedConstraint::AtMostSteps { steps: 10 },
        }
    );
    assert_rendered_context(&error, &["11", "≤ steps (10)"]);
}

#[test]
fn geometry_diagnostics_render_original_range_and_period_axis() {
    let error = generate_delaunay2(4, (3.5, -2.5), Some(42)).expect_err("reversed bounds");
    assert_rendered_context(&error, &["[3.5, -2.5]", "finite min < max"]);

    for (domain, expected_axis, expected_value) in [
        ([-0.0, 1.0], 0, -0.0_f64),
        ([1.0, f64::INFINITY], 1, f64::INFINITY),
    ] {
        let error = build_toroidal_delaunay2(&[], &[], domain).expect_err("invalid period");
        assert_matches!(&error, CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::InvalidToroidalDomain,
            provided_value: ObservedValue::ToroidalPeriod { axis, value },
            expected_range: ExpectedConstraint::FinitePositivePeriods,
        } if *axis == expected_axis && value.to_bits() == expected_value.to_bits());
        assert_rendered_context(
            &error,
            &[
                &format!("axis {expected_axis}"),
                "period",
                "finite and positive",
            ],
        );
    }
}

#[test]
fn profile_constructors_reject_count_overflow_before_allocating_meshes() {
    for error in [
        CdtTriangulation::from_cdt_strip_spatial_vertex_profile(&[u32::MAX, 4, 4])
            .expect_err("open profile sum overflows"),
        CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(&[u32::MAX, 4, 4])
            .expect_err("toroidal profile sum overflows"),
    ] {
        assert_matches!(&error, CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::VertexCountOverflow,
            provided_value: ObservedValue::SpatialVertexProfile(profile),
            expected_range: ExpectedConstraint::ProfileSumFitsU32,
        } if profile == &[u32::MAX, 4, 4]);
        assert_rendered_context(&error, &["[4294967295, 4, 4]", "sum must fit in u32"]);
    }

    // The total fits u32, but doubling it for the checked open-strip face
    // formula does not. No large allocation is needed to reject these inputs.
    let profile = [u32::MAX / 2, 4];
    let error = CdtTriangulation::from_cdt_strip_spatial_vertex_profile(&profile)
        .expect_err("intermediate face count overflows");
    assert_matches!(&error, CdtError::InvalidGenerationParameters {
        issue: GenerationParameterIssue::SimplexCountOverflow,
        provided_value: ObservedValue::SpatialVertexProfile(actual),
        expected_range: ExpectedConstraint::OpenStripFaceCountFitsU32,
    } if actual == &profile);
    assert_rendered_context(
        &error,
        &["[2147483647, 4]", "open-strip face count", "fit in u32"],
    );
}

#[test]
fn profile_constructors_reject_empty_input_with_the_original_profile() {
    for error in [
        CdtTriangulation::from_cdt_strip_spatial_vertex_profile(&[])
            .expect_err("empty open profile"),
        CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(&[])
            .expect_err("empty toroidal profile"),
    ] {
        assert_matches!(&error, CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::EmptySpatialVertexProfile,
            provided_value: ObservedValue::SpatialVertexProfile(profile),
            expected_range: ExpectedConstraint::AtLeast { minimum: 1 },
        } if profile.is_empty());
        assert_rendered_context(&error, &["[]", "≥ 1"]);
    }
}

#[test]
fn toroidal_simplex_overflows_retain_the_undoubled_vertex_total() {
    // Both paths admit a representable vertex total, then reject doubling it
    // for the simplex count before creating vertices or allocating a mesh.
    for error in [
        CdtTriangulation::from_toroidal_cdt(1_000_000_000, 3)
            .expect_err("regular toroidal simplex count overflows"),
        CdtTriangulation::from_toroidal_cdt_spatial_vertex_profile(&[1_000_000_000; 3])
            .expect_err("profile toroidal simplex count overflows"),
    ] {
        assert_matches!(
            &error,
            CdtError::InvalidGenerationParameters {
                issue: GenerationParameterIssue::SimplexCountOverflow,
                provided_value: ObservedValue::CountProduct {
                    left: 2,
                    right: 3_000_000_000
                },
                expected_range: ExpectedConstraint::FitsU32,
            }
        );
        assert_rendered_context(&error, &["2 × 3000000000", "must fit in u32"]);
    }
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

    let error = CdtTriangulation::from_filtered_delaunay_strip(u32::MAX / 3, 3)
        .expect_err("surplus vertex total cannot fit u32");
    assert_matches!(
        &error,
        CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::VertexCountOverflow,
            provided_value: ObservedValue::CountSum {
                left: u32::MAX,
                right: 2
            },
            expected_range: ExpectedConstraint::FitsU32,
        }
    );
    assert_rendered_context(&error, &["4294967295 + 2", "must fit in u32"]);

    let error = CdtTriangulation::from_cdt_strip(u32::MAX, 2)
        .expect_err("open strip vertex product cannot fit u32");
    assert_matches!(
        &error,
        CdtError::InvalidGenerationParameters {
            issue: GenerationParameterIssue::VertexCountOverflow,
            provided_value: ObservedValue::CountProduct {
                left: u32::MAX,
                right: 2
            },
            expected_range: ExpectedConstraint::FitsU32,
        }
    );
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
