//! Public-contract tests for the borrowed CDT proposal-policy view.

use causal_triangulations::prelude::geometry::{DelaunayBackend2D, build_delaunay2_with_data};
use causal_triangulations::prelude::moves::{ErgodicsSystem, MoveResult, MoveType};
use causal_triangulations::prelude::simulation::{
    ActionConfig, CdtProposal, CdtProposalSiteIdError,
};
use causal_triangulations::prelude::triangulation::{CdtResult, CdtTriangulation};
use std::assert_matches;

fn single_triangle() -> causal_triangulations::geometry::CdtTriangulation2D {
    let triangulation =
        build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("build labeled triangle");
    let backend = DelaunayBackend2D::from_triangulation(triangulation)
        .expect("test Delaunay triangle should validate");
    CdtTriangulation::from_labeled_delaunay(backend, 2, 2).expect("wrap labeled triangle")
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
        assert!(view.offered_site_count() > 1);
        assert_eq!(view.simplex_counts()?.vertex_count(), 16);
        assert_eq!(view.slice_sizes(), &[4, 4, 4, 4]);
        view.offered_sites().collect::<Vec<_>>()
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
    assert_matches!(
        wrong_family.validate_site(site),
        Err(CdtProposalSiteIdError::FamilyMismatch {
            expected: MoveType::Move31Remove,
            actual: MoveType::Move13Add,
        })
    );

    let foreign_view = moves.proposal_policy_view(&foreign, MoveType::Move13Add);
    assert_matches!(
        foreign_view.validate_site(site),
        Err(CdtProposalSiteIdError::ForeignTriangulation { .. })
    );

    let current = moves.proposal_policy_view(&triangulation, MoveType::Move13Add);
    let offered_site_count = current.offered_site_count();
    assert_matches!(
        current.site_id(offered_site_count),
        Err(CdtProposalSiteIdError::OrdinalOutOfRange {
            family: MoveType::Move13Add,
            ordinal,
            offered_site_count: actual_count,
        }) if ordinal == offered_site_count && actual_count == offered_site_count
    );
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
    assert_matches!(
        current.validate_site(stale_site),
        Err(CdtProposalSiteIdError::StaleState { .. })
    );

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
