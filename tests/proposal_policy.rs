//! Public-contract tests for the borrowed CDT proposal-policy view.

use approx::assert_relative_eq;
use causal_triangulations::CdtError;
use causal_triangulations::prelude::geometry::{DelaunayBackend2D, build_delaunay2_with_data};
use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveResult, MoveType};
use causal_triangulations::prelude::simulation::{
    ActionConfig, CdtMoveFamilyDistribution, CdtMoveFamilyPolicy, CdtMoveFamilyPolicyError,
    CdtProposal, CdtProposalError, CdtProposalInfo, CdtProposalPlanningOutcome,
    CdtProposalPolicyView, CdtProposalSiteId, CdtProposalSiteIdError, CdtTopology, DelayedProposal,
    MetropolisAlgorithm, MetropolisConfig, UniformCdtMoveFamilyPolicy,
};
use causal_triangulations::prelude::triangulation::{CdtResult, CdtTriangulation};
use rand::{SeedableRng, rngs::StdRng};
use std::assert_matches;

fn single_triangle() -> causal_triangulations::geometry::CdtTriangulation2D {
    let triangulation =
        build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("build labeled triangle");
    let backend = DelaunayBackend2D::from_triangulation(triangulation)
        .expect("test Delaunay triangle should validate");
    CdtTriangulation::from_labeled_delaunay(backend, 2, 2).expect("wrap labeled triangle")
}

fn proposal_info(
    proposal: &mut impl DelayedProposal<
        causal_triangulations::geometry::CdtTriangulation2D,
        Plan = causal_triangulations::CdtProposalPlan,
        Info = CdtProposalInfo,
        Error = CdtProposalError,
    >,
    plan: &Option<causal_triangulations::CdtProposalPlan>,
) -> CdtProposalInfo {
    match plan {
        Some(plan) => proposal.info(plan),
        None => proposal
            .no_plan_info()
            .expect("CDT self-loop proposals should retain typed family telemetry"),
    }
}

struct VolumeDependentPolicy;

impl CdtMoveFamilyPolicy for VolumeDependentPolicy {
    fn family_weight(
        &self,
        view: &CdtProposalPolicyView<'_>,
    ) -> Result<f64, CdtMoveFamilyPolicyError> {
        let vertices = view
            .simplex_counts()
            .map_err(|error| CdtMoveFamilyPolicyError::EvaluationFailed {
                family: view.family(),
                detail: error.to_string(),
            })?
            .vertex_count();
        let vertices = u32::try_from(vertices).map_err(|error| {
            CdtMoveFamilyPolicyError::EvaluationFailed {
                family: view.family(),
                detail: error.to_string(),
            }
        })?;

        Ok(match view.family() {
            MoveType::Move13Add => f64::from(vertices),
            MoveType::Move31Remove => 1.0,
            MoveType::Move22 | MoveType::EdgeFlip => 0.0,
        })
    }
}

#[test]
fn move_family_identifiers_and_reverse_mapping_are_stable() {
    assert_eq!(
        MoveType::REVERSIBLE_1P1.map(MoveType::identifier),
        ["move-2-2", "move-1-3-add", "move-3-1-remove", "edge-flip",]
    );

    for family in MoveType::REVERSIBLE_1P1 {
        assert_eq!(family.reverse().reverse(), family);
    }
}

#[test]
fn fixed_family_distribution_normalizes_and_rejects_invalid_support() {
    let distribution = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 0.0, 2.0])
        .expect("finite nonnegative weights with positive support should normalize");
    for (actual, expected) in
        distribution
            .probabilities()
            .into_iter()
            .zip([1.0 / 6.0, 0.5, 0.0, 1.0 / 3.0])
    {
        assert_relative_eq!(actual, expected, epsilon = f64::EPSILON);
    }
    assert_eq!(distribution.probability(MoveType::Move31Remove), 0.0);

    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        assert_matches!(
            CdtMoveFamilyDistribution::from_weights([1.0, invalid, 1.0, 1.0]),
            Err(CdtMoveFamilyPolicyError::InvalidWeight {
                family: MoveType::Move13Add,
                weight,
            }) if weight.to_bits() == invalid.to_bits()
        );
    }
    assert_matches!(
        CdtMoveFamilyDistribution::from_weights([0.0; 4]),
        Err(CdtMoveFamilyPolicyError::EmptySupport)
    );
    assert_matches!(
        CdtMoveFamilyDistribution::from_weights([f64::MAX; 4]),
        Err(CdtMoveFamilyPolicyError::NonFiniteTotalWeight { total_weight })
            if total_weight == f64::INFINITY
    );
}

#[test]
fn positive_weight_empty_family_is_typed_self_loop_without_mutation() {
    let triangulation = single_triangle();
    let counts_before = (
        triangulation.vertex_count(),
        triangulation.edge_count(),
        triangulation.face_count(),
    );
    let policy = CdtMoveFamilyDistribution::from_weights([1.0, 0.0, 0.0, 0.0])
        .expect("single-family support should be valid");
    let mut proposal = CdtProposal::with_seed_and_policy(ActionConfig::default(), 7, policy);
    let mut rng = StdRng::seed_from_u64(11);

    let plan = proposal
        .propose_plan(&triangulation, &mut rng)
        .expect("empty offered-site support is an ordinary self-loop");
    let info = proposal_info(&mut proposal, &plan);

    assert!(plan.is_none());
    assert_eq!(info.move_type, MoveType::Move22);
    assert_eq!(info.reverse_move_type, MoveType::Move22);
    assert_eq!(info.forward_family_probability, 1.0);
    assert_eq!(info.forward_site_count, 0);
    assert_eq!(
        info.planning_outcome,
        CdtProposalPlanningOutcome::NoOfferedSite
    );
    assert_eq!(
        (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        ),
        counts_before
    );
}

#[test]
fn uniform_injected_policy_matches_the_conventional_checked_path() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let mut conventional = CdtProposal::with_seed(ActionConfig::default(), 29);
    let mut injected =
        CdtProposal::with_seed_and_policy(ActionConfig::default(), 29, UniformCdtMoveFamilyPolicy);
    let mut conventional_rng = StdRng::seed_from_u64(31);
    let mut injected_rng = StdRng::seed_from_u64(31);

    let conventional_plan = conventional.propose_plan(&triangulation, &mut conventional_rng)?;
    let injected_plan = injected.propose_plan(&triangulation, &mut injected_rng)?;
    let conventional_info = proposal_info(&mut conventional, &conventional_plan);
    let injected_info = proposal_info(&mut injected, &injected_plan);

    assert_eq!(injected_info, conventional_info);
    assert_eq!(injected_plan.is_some(), conventional_plan.is_some());
    Ok(())
}

#[test]
fn uniform_injected_runner_reproduces_seeded_conventional_trajectory() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let algorithm = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 24, 0, 1)?.with_seed(37),
        ActionConfig::default(),
    );
    let conventional = algorithm.run(triangulation.clone())?;
    let injected = algorithm.run_with_policy(triangulation, &UniformCdtMoveFamilyPolicy)?;

    assert_eq!(injected.steps().len(), conventional.steps().len());
    for (injected_step, conventional_step) in injected.steps().iter().zip(conventional.steps()) {
        assert_eq!(injected_step.move_type(), conventional_step.move_type());
        assert_eq!(injected_step.outcome(), conventional_step.outcome());
        assert_eq!(
            injected_step.proposal_telemetry(),
            conventional_step.proposal_telemetry()
        );
    }
    assert_eq!(
        (
            injected.triangulation().vertex_count(),
            injected.triangulation().edge_count(),
            injected.triangulation().face_count(),
        ),
        (
            conventional.triangulation().vertex_count(),
            conventional.triangulation().edge_count(),
            conventional.triangulation().face_count(),
        )
    );
    Ok(())
}

#[test]
fn uniform_policy_concrete_pair_satisfies_independent_detailed_balance() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 83);
    let mut rng = StdRng::seed_from_u64(89);
    let plan = (0..64)
        .find_map(|_| {
            proposal
                .propose_plan(&triangulation, &mut rng)
                .expect("uniform planning should not fail")
        })
        .expect("representative uniform policy should realize a concrete plan");
    let mut proposed = triangulation.clone();
    proposal.commit(&mut proposed, plan.clone(), &mut rng)?;

    let mut inspection = ErgodicsSystem::new();
    let forward_sites = inspection
        .proposal_policy_view(&triangulation, plan.move_type())
        .offered_site_count();
    let reverse_sites = inspection
        .proposal_policy_view(&proposed, plan.reverse_move_type())
        .offered_site_count();
    let forward_sites =
        u32::try_from(forward_sites).expect("test fixture forward count should fit u32");
    let reverse_sites =
        u32::try_from(reverse_sites).expect("test fixture reverse count should fit u32");
    let forward_q = 0.25 / f64::from(forward_sites);
    let reverse_q = 0.25 / f64::from(reverse_sites);
    let expected_ratio = reverse_q.ln() - forward_q.ln();

    assert_relative_eq!(plan.log_proposal_ratio(), expected_ratio, epsilon = 1e-12);

    let log_target_forward = plan.action_before() - plan.action_after();
    let log_accept_forward = (log_target_forward + expected_ratio).min(0.0);
    let log_accept_reverse = (-log_target_forward - expected_ratio).min(0.0);
    let log_forward_flux = -plan.action_before() + forward_q.ln() + log_accept_forward;
    let log_reverse_flux = -plan.action_after() + reverse_q.ln() + log_accept_reverse;
    assert_relative_eq!(log_forward_flux, log_reverse_flux, epsilon = 1e-12);
    Ok(())
}

#[test]
fn fixed_policy_checkpoint_resume_matches_uninterrupted_rng_stream() -> CdtResult<()> {
    let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])
        .expect("fixed checkpoint policy should be valid");
    let action = ActionConfig::default();
    let initial = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let uninterrupted = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 12, 0, 1)?.with_seed(97),
        action.clone(),
    )
    .run_with_policy(initial.clone(), &policy)?;

    let prefix = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 5, 0, 1)?.with_seed(97),
        action.clone(),
    )
    .run_to_checkpoint_with_policy(initial, &policy)?;
    let resumed =
        MetropolisAlgorithm::new(MetropolisConfig::new(1.0, 7, 0, 1)?.with_seed(999), action)
            .resume_from_checkpoint_with_policy(prefix, &policy)?;

    assert_eq!(resumed.config().steps().get(), 12);
    assert_eq!(resumed.steps().len(), uninterrupted.steps().len());
    for (resumed_step, uninterrupted_step) in resumed.steps().iter().zip(uninterrupted.steps()) {
        assert_eq!(resumed_step.step(), uninterrupted_step.step());
        assert_eq!(resumed_step.move_type(), uninterrupted_step.move_type());
        assert_eq!(
            resumed_step.action_before(),
            uninterrupted_step.action_before()
        );
        assert_eq!(resumed_step.outcome(), uninterrupted_step.outcome());
        assert_eq!(
            resumed_step.proposal_telemetry(),
            uninterrupted_step.proposal_telemetry()
        );
    }
    assert_eq!(resumed.proposal_stats(), uninterrupted.proposal_stats());
    for family in MoveType::REVERSIBLE_1P1 {
        assert_eq!(
            resumed.move_stats().attempted(family),
            uninterrupted.move_stats().attempted(family)
        );
        assert_eq!(
            resumed.move_stats().accepted(family),
            uninterrupted.move_stats().accepted(family)
        );
        assert_eq!(
            resumed.move_stats().hard_failed(family),
            uninterrupted.move_stats().hard_failed(family)
        );
    }
    assert_eq!(
        serde_json::to_value(resumed.measurements()).expect("measurements should serialize"),
        serde_json::to_value(uninterrupted.measurements()).expect("measurements should serialize")
    );
    assert_eq!(
        (
            resumed.triangulation().vertex_count(),
            resumed.triangulation().edge_count(),
            resumed.triangulation().face_count(),
            resumed.triangulation().slice_sizes(),
        ),
        (
            uninterrupted.triangulation().vertex_count(),
            uninterrupted.triangulation().edge_count(),
            uninterrupted.triangulation().face_count(),
            uninterrupted.triangulation().slice_sizes(),
        )
    );
    assert_eq!(
        resumed.triangulation().volume_profile()?,
        uninterrupted.triangulation().volume_profile()?
    );
    resumed.triangulation().validate()?;
    uninterrupted.triangulation().validate()?;
    Ok(())
}

#[test]
fn unequal_fixed_weights_match_analytic_family_and_site_ratio() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let policy = CdtMoveFamilyDistribution::from_weights([0.0, 3.0, 1.0, 0.0])
        .expect("fixed test weights should be valid");
    let mut proposal = CdtProposal::with_seed_and_policy(ActionConfig::default(), 41, policy);
    let mut rng = StdRng::seed_from_u64(43);

    let plan = (0..64)
        .find_map(|_| {
            proposal
                .propose_plan(&triangulation, &mut rng)
                .expect("fixed policy planning should not fail")
        })
        .expect("representative toroidal policy should realize a concrete plan");
    let mut proposed = triangulation.clone();
    proposal.commit(&mut proposed, plan.clone(), &mut rng)?;

    let expected_forward_family = match plan.move_type() {
        MoveType::Move13Add => 0.75,
        MoveType::Move31Remove => 0.25,
        MoveType::Move22 | MoveType::EdgeFlip => {
            panic!("zero-weight flip family produced a concrete plan")
        }
    };
    let expected_reverse_family = match plan.reverse_move_type() {
        MoveType::Move13Add => 0.75,
        MoveType::Move31Remove => 0.25,
        MoveType::Move22 | MoveType::EdgeFlip => {
            panic!("zero-weight flip family appeared as an inverse")
        }
    };
    let mut inspection = ErgodicsSystem::new();
    let independently_counted_forward = inspection
        .proposal_policy_view(&triangulation, plan.move_type())
        .offered_site_count();
    let independently_counted_reverse = inspection
        .proposal_policy_view(&proposed, plan.reverse_move_type())
        .offered_site_count();
    let forward_sites = u32::try_from(independently_counted_forward)
        .expect("test fixture forward count should fit u32");
    let reverse_sites = u32::try_from(independently_counted_reverse)
        .expect("test fixture reverse count should fit u32");
    let forward_q = expected_forward_family / f64::from(forward_sites);
    let reverse_q = expected_reverse_family / f64::from(reverse_sites);
    let expected_family = (expected_reverse_family / expected_forward_family).ln();
    let expected_sites = (f64::from(forward_sites) / f64::from(reverse_sites)).ln();
    let expected_ratio = reverse_q.ln() - forward_q.ln();

    assert_eq!(plan.forward_site_count(), independently_counted_forward);
    assert_eq!(plan.reverse_site_count(), independently_counted_reverse);
    assert_relative_eq!(
        plan.forward_family_probability(),
        expected_forward_family,
        epsilon = f64::EPSILON
    );
    assert_relative_eq!(
        plan.reverse_family_probability(),
        expected_reverse_family,
        epsilon = f64::EPSILON
    );

    assert_relative_eq!(
        plan.log_family_probability_ratio(),
        expected_family,
        epsilon = 1e-12
    );
    assert_relative_eq!(plan.log_site_count_ratio(), expected_sites, epsilon = 1e-12);
    assert_relative_eq!(plan.log_proposal_ratio(), expected_ratio, epsilon = 1e-12);
    assert_relative_eq!(
        proposal.log_q_ratio(&triangulation, &plan)?,
        expected_ratio,
        epsilon = 1e-12
    );

    let log_target_forward = plan.action_before() - plan.action_after();
    let log_accept_forward = (log_target_forward + expected_ratio).min(0.0);
    let log_accept_reverse = (-log_target_forward - expected_ratio).min(0.0);
    let log_forward_flux = -plan.action_before() + forward_q.ln() + log_accept_forward;
    let log_reverse_flux = -plan.action_after() + reverse_q.ln() + log_accept_reverse;
    assert_relative_eq!(log_forward_flux, log_reverse_flux, epsilon = 1e-12);
    Ok(())
}

#[test]
fn reverse_policy_probability_is_evaluated_on_planned_post_state() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(8, 8)?;
    let mut inspection = ErgodicsSystem::new();
    let pre_forward_site_count = inspection
        .proposal_policy_view(&triangulation, MoveType::Move13Add)
        .offered_site_count();
    let pre_reverse_site_count = inspection
        .proposal_policy_view(&triangulation, MoveType::Move31Remove)
        .offered_site_count();
    let vertices_before = triangulation.simplex_counts()?.vertex_count();
    let vertices_before =
        u32::try_from(vertices_before).expect("test fixture vertex count should fit u32");
    let mut proposal =
        CdtProposal::with_seed_and_policy(ActionConfig::default(), 47, VolumeDependentPolicy);
    let mut rng = StdRng::seed_from_u64(53);

    let plan = (0..128)
        .find_map(|_| {
            proposal
                .propose_plan(&triangulation, &mut rng)
                .expect("state-dependent policy planning should not fail")
                .filter(|plan| plan.move_type() == MoveType::Move13Add)
        })
        .expect("representative policy should realize a volume-add plan");
    let expected_forward = f64::from(vertices_before) / f64::from(vertices_before + 1);
    let expected_reverse = 1.0 / f64::from(vertices_before + 2);
    let incorrect_pre_state_reverse = 1.0 / f64::from(vertices_before + 1);

    assert_relative_eq!(
        plan.forward_family_probability(),
        expected_forward,
        epsilon = 1e-12
    );
    assert_relative_eq!(
        plan.reverse_family_probability(),
        expected_reverse,
        epsilon = 1e-12
    );
    assert_eq!(plan.forward_site_count(), pre_forward_site_count);
    assert_ne!(
        plan.reverse_site_count(),
        pre_reverse_site_count,
        "reverse-site count must come from the volume-increased planned state"
    );
    assert!((plan.reverse_family_probability() - incorrect_pre_state_reverse).abs() > 1e-12);
    Ok(())
}

#[test]
fn invalid_policy_aborts_before_mutating_the_chain() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
    let counts_before = (
        triangulation.vertex_count(),
        triangulation.edge_count(),
        triangulation.face_count(),
    );
    let policy = CdtMoveFamilyDistribution::from_weights([0.0; 4])
        .expect_err("all-zero policy must be rejected before construction");
    assert_matches!(policy, CdtMoveFamilyPolicyError::EmptySupport);

    struct EmptyPolicy;
    impl CdtMoveFamilyPolicy for EmptyPolicy {
        fn family_weight(
            &self,
            _view: &CdtProposalPolicyView<'_>,
        ) -> Result<f64, CdtMoveFamilyPolicyError> {
            Ok(0.0)
        }
    }

    let algorithm = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(59),
        ActionConfig::default(),
    );
    let error = algorithm
        .run_with_policy(triangulation.clone(), &EmptyPolicy)
        .expect_err("empty-support runtime policy should be typed");
    assert_matches!(
        error,
        CdtError::MetropolisProposalPolicyFailed {
            step: 1,
            source: CdtMoveFamilyPolicyError::EmptySupport,
        }
    );
    assert_eq!(
        (
            triangulation.vertex_count(),
            triangulation.edge_count(),
            triangulation.face_count(),
        ),
        counts_before
    );
    Ok(())
}

#[test]
fn runtime_policy_outputs_are_checked_before_family_selection() -> CdtResult<()> {
    struct ConstantPolicy(f64);
    impl CdtMoveFamilyPolicy for ConstantPolicy {
        fn family_weight(
            &self,
            _view: &CdtProposalPolicyView<'_>,
        ) -> Result<f64, CdtMoveFamilyPolicyError> {
            Ok(self.0)
        }
    }

    let triangulation = CdtTriangulation::from_cdt_strip(4, 3)?;
    let mut rng = StdRng::seed_from_u64(71);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let mut proposal =
            CdtProposal::with_seed_and_policy(ActionConfig::default(), 73, ConstantPolicy(invalid));
        assert_matches!(
            proposal.propose_plan(&triangulation, &mut rng),
            Err(CdtProposalError::Policy {
                source: CdtMoveFamilyPolicyError::InvalidWeight {
                    family: MoveType::Move22,
                    weight,
                },
            }) if weight.to_bits() == invalid.to_bits()
        );
    }
    Ok(())
}

#[test]
fn fixed_policy_shortcut_skips_state_dependent_family_evaluation() -> CdtResult<()> {
    struct FixedShortcut(CdtMoveFamilyDistribution);

    impl CdtMoveFamilyPolicy for FixedShortcut {
        fn fixed_distribution(&self) -> Option<CdtMoveFamilyDistribution> {
            Some(self.0)
        }

        fn family_weight(
            &self,
            view: &CdtProposalPolicyView<'_>,
        ) -> Result<f64, CdtMoveFamilyPolicyError> {
            Err(CdtMoveFamilyPolicyError::EvaluationFailed {
                family: view.family(),
                detail: "fixed policies must not evaluate family views".to_string(),
            })
        }
    }

    let distribution = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])
        .expect("fixed shortcut distribution should be valid");
    let policy = FixedShortcut(distribution);
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let mut proposal = CdtProposal::with_seed_and_policy(ActionConfig::default(), 73, policy);
    let mut rng = StdRng::seed_from_u64(79);

    let plan = proposal.propose_plan(&triangulation, &mut rng)?;
    let info = proposal_info(&mut proposal, &plan);

    assert_relative_eq!(
        info.forward_family_probability,
        distribution.probability(info.move_type),
        epsilon = f64::EPSILON
    );
    Ok(())
}

#[test]
fn algorithm_exposes_typed_policy_audit_telemetry_after_each_step() -> CdtResult<()> {
    let policy = CdtMoveFamilyDistribution::from_weights([1.0, 3.0, 1.0, 1.0])
        .expect("fixed audit policy should be valid");
    let algorithm = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 8, 0, 1)?.with_seed(61),
        ActionConfig::default(),
    );
    let results = algorithm.run_with_policy(CdtTriangulation::from_toroidal_cdt(4, 4)?, &policy)?;

    for step in results.steps() {
        let telemetry = step
            .proposal_telemetry()
            .expect("fresh policy-driven steps should expose proposal telemetry");
        assert_eq!(telemetry.selected_family(), step.move_type());
        assert_eq!(telemetry.reverse_family(), step.move_type().reverse());
        assert_relative_eq!(
            telemetry.forward_family_probability(),
            policy.probability(step.move_type()),
            epsilon = 1e-15
        );

        if telemetry.planning_outcome() == CdtProposalPlanningOutcome::ConcretePlan {
            let family = telemetry
                .log_family_probability_ratio()
                .expect("concrete plan should expose its family component");
            let sites = telemetry
                .log_site_count_ratio()
                .expect("concrete plan should expose its site component");
            assert_relative_eq!(
                telemetry
                    .log_proposal_ratio()
                    .expect("concrete plan should expose its complete ratio"),
                family + sites,
                epsilon = 1e-12
            );
        } else {
            assert_eq!(telemetry.reverse_family_probability(), None);
            assert_eq!(telemetry.reverse_site_count(), None);
            assert_eq!(telemetry.log_proposal_ratio(), None);
        }
    }
    Ok(())
}

#[test]
fn zero_reverse_family_probability_forces_rejection_without_mutation() -> CdtResult<()> {
    let policy = CdtMoveFamilyDistribution::from_weights([0.0, 1.0, 0.0, 0.0])
        .expect("add-only policy should have valid forward support");
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let counts_before = (
        triangulation.vertex_count(),
        triangulation.edge_count(),
        triangulation.face_count(),
    );
    let algorithm = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 4, 0, 1)?.with_seed(67),
        ActionConfig::default(),
    );
    let results = algorithm.run_with_policy(triangulation, &policy)?;
    let concrete_steps = results
        .steps()
        .iter()
        .filter(|step| {
            step.proposal_telemetry().is_some_and(|telemetry| {
                telemetry.planning_outcome() == CdtProposalPlanningOutcome::ConcretePlan
            })
        })
        .collect::<Vec<_>>();

    assert!(
        !concrete_steps.is_empty(),
        "representative add-only run should realize at least one proposal"
    );
    for step in concrete_steps {
        let telemetry = step
            .proposal_telemetry()
            .expect("filtered concrete step should retain telemetry");
        assert_eq!(telemetry.reverse_family_probability(), Some(0.0));
        assert_eq!(telemetry.log_proposal_ratio(), Some(f64::NEG_INFINITY));
        assert!(!step.accepted());
    }
    assert_eq!(
        (
            results.triangulation().vertex_count(),
            results.triangulation().edge_count(),
            results.triangulation().face_count(),
        ),
        counts_before
    );
    Ok(())
}

#[test]
fn policy_view_has_deterministic_order_and_explicit_empty_families() -> CdtResult<()> {
    let empty_triangulation = single_triangle();
    let mut empty_moves = ErgodicsSystem::with_seed(7);

    let empty = empty_moves.proposal_policy_view(&empty_triangulation, MoveType::Move22);
    assert_eq!(empty.offered_site_count(), 0);
    assert_eq!(empty.offered_sites().len(), 0);
    assert_eq!(empty.offered_sites().next(), None);

    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let mut first_moves = ErgodicsSystem::with_seed(7);
    let first_order = {
        let view = first_moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
        assert_eq!(view.family(), MoveType::Move13Add);
        assert_eq!(view.reverse_family(), MoveType::Move31Remove);
        assert_eq!(view.topology(), CdtTopology::Toroidal);
        assert!(view.offered_site_count() > 1);
        assert_eq!(view.simplex_counts()?.vertex_count(), 16);
        assert_eq!(view.slice_sizes(), &[4, 4, 4, 4]);
        let sites = view.offered_sites().collect::<Vec<_>>();
        let reverse_ordinals = view
            .offered_sites()
            .rev()
            .map(CdtProposalSiteId::ordinal)
            .collect::<Vec<_>>();
        assert_eq!(reverse_ordinals, (0..sites.len()).rev().collect::<Vec<_>>());
        sites
    };
    let mut second_moves = ErgodicsSystem::with_seed(99);
    let second_order = second_moves
        .proposal_policy_view(&triangulation, MoveType::Move13Add)
        .offered_sites()
        .collect::<Vec<_>>();
    let mut proposal = CdtProposal::with_seed(ActionConfig::default(), 7);
    let proposal_count = proposal
        .policy_view(&triangulation, MoveType::Move13Add)
        .offered_site_count();

    assert_eq!(first_order, second_order);
    assert_eq!(first_order[0].ordinal(), 0);
    assert_eq!(
        first_order.last().map(|site| site.ordinal()),
        Some(first_order.len() - 1)
    );
    assert_eq!(proposal_count, first_order.len());
    Ok(())
}

#[test]
fn policy_view_rejects_invalid_family_ordinals_and_foreign_ids() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(4, 4)?;
    let foreign = triangulation.clone();
    let mut moves = ErgodicsSystem::new();
    let site = moves
        .proposal_policy_view(&triangulation, MoveType::Move13Add)
        .site_id(0)
        .expect("toroidal fixture should expose insertion site zero");

    let wrong_family = moves.proposal_policy_view(&triangulation, MoveType::Move31Remove);
    let wrong_family_error = wrong_family
        .validate_site(site)
        .expect_err("a site from another move family must be rejected");
    let wrong_family_diagnostic = wrong_family_error.to_string();
    assert_matches!(
        wrong_family_error,
        CdtProposalSiteIdError::FamilyMismatch {
            expected: MoveType::Move31Remove,
            actual: MoveType::Move13Add,
        }
    );
    assert!(wrong_family_diagnostic.contains("expected move-3-1-remove"));
    assert!(wrong_family_diagnostic.contains("received move-1-3-add"));

    let foreign_view = moves.proposal_policy_view(&foreign, MoveType::Move13Add);
    let foreign_error = foreign_view
        .validate_site(site)
        .expect_err("a site from another triangulation must be rejected");
    let foreign_diagnostic = foreign_error.to_string();
    assert_matches!(
        foreign_error,
        CdtProposalSiteIdError::ForeignTriangulation {
            family: MoveType::Move13Add,
            ordinal: 0,
        }
    );
    assert!(foreign_diagnostic.contains("move-1-3-add[0]"));
    assert!(foreign_diagnostic.contains("another triangulation"));

    let current = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
    let offered_site_count = current.offered_site_count();
    let ordinal_error = current
        .site_id(offered_site_count)
        .expect_err("the first ordinal after the offered set must be rejected");
    let ordinal_diagnostic = ordinal_error.to_string();
    assert_matches!(
        ordinal_error,
        CdtProposalSiteIdError::OrdinalOutOfRange {
            family: MoveType::Move13Add,
            ordinal,
            offered_site_count: actual_count,
        } if ordinal == offered_site_count && actual_count == offered_site_count
    );
    assert!(ordinal_diagnostic.contains(&format!("move-1-3-add[{offered_site_count}]")));
    assert!(ordinal_diagnostic.contains(&format!("{offered_site_count}-site offered set")));
    Ok(())
}

#[test]
fn accepted_toroidal_sequence_invalidates_old_site_ids() -> CdtResult<()> {
    let mut triangulation = CdtTriangulation::from_toroidal_cdt(8, 8)?;
    let mut moves = ErgodicsSystem::with_seed(0);
    let stale_site = moves
        .proposal_policy_view(&triangulation, MoveType::Move13Add)
        .site_id(0)
        .expect("toroidal fixture should expose insertion site zero");

    let mut inserted = false;
    for _ in 0..64 {
        match moves.attempt_13_move(&mut triangulation) {
            MoveResult::Success => {
                inserted = true;
                break;
            }
            MoveResult::CausalityViolation
            | MoveResult::GeometricViolation
            | MoveResult::Rejected(_) => {}
            MoveResult::HardFailure(error) => {
                panic!("unexpected hard failure during toroidal insertion: {error}")
            }
        }
    }
    assert!(inserted, "representative toroidal insertion should succeed");

    let current = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
    let stale_error = current
        .validate_site(stale_site)
        .expect_err("an accepted mutation must make an earlier site ID stale");
    let stale_diagnostic = stale_error.to_string();
    let CdtProposalSiteIdError::StaleState {
        family,
        ordinal,
        identifier_version,
        current_version,
    } = stale_error
    else {
        panic!("expected stale-state error, received {stale_error:?}");
    };
    assert_eq!(family, MoveType::Move13Add);
    assert_eq!(ordinal, 0);
    assert!(identifier_version < current_version);
    assert!(stale_diagnostic.contains("move-1-3-add[0] is stale"));
    assert!(stale_diagnostic.contains(&format!("identifier version {identifier_version}")));
    assert!(stale_diagnostic.contains(&format!("current version {current_version}")));

    let mut removed = false;
    for _ in 0..64 {
        match moves.attempt_31_move(&mut triangulation) {
            MoveResult::Success => {
                removed = true;
                break;
            }
            MoveResult::CausalityViolation
            | MoveResult::GeometricViolation
            | MoveResult::Rejected(_) => {}
            MoveResult::HardFailure(error) => {
                panic!("unexpected hard failure during toroidal removal: {error}")
            }
        }
    }
    assert!(
        removed,
        "representative toroidal inverse move should succeed"
    );
    triangulation.validate()?;
    Ok(())
}
