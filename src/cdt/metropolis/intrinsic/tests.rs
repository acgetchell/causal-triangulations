#![forbid(unsafe_code)]

//! Independent cyclic-word enumeration of marked CDT transitions.

use super::*;
use crate::cdt::volume_fixing::VolumeFixing;
use crate::{
    ActionConfig, CdtProposal, CdtTarget, CdtTriangulation2D, StepOutcome, TriangulationQuery,
};
use approx::assert_relative_eq;
use markov_chain_monte_carlo::Chain;
use rand::{SeedableRng, rngs::StdRng};
use std::{assert_matches, cmp::Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Up(usize),
    Down(usize),
}

/// Expands each slab into an explicit cyclic sequence of labeled triangles.
fn word(slab: &Slab, upper_length: usize) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut upper = slab.upper_start;
    for (lower, &run_length) in slab.down_after_up.iter().enumerate() {
        steps.push(Step::Up(lower));
        for _ in 0..run_length {
            steps.push(Step::Down(upper));
            upper = (upper + 1) % upper_length;
        }
    }
    steps
}

/// Encodes an explicit word by counting Down steps after each Up edge label.
fn slab_from_word(steps: &[Step]) -> Slab {
    let origin = steps
        .iter()
        .position(|step| *step == Step::Up(0))
        .expect("one indexed Up triangle is the word origin");
    let steps = steps[origin..].iter().chain(&steps[..origin]);
    let upper_start = steps
        .clone()
        .find_map(|step| match step {
            Step::Down(edge) => Some(*edge),
            Step::Up(_) => None,
        })
        .expect("the slab contains Down triangles");
    let mut down_after_up = Vec::new();
    for step in steps {
        match step {
            Step::Up(edge) => {
                assert_eq!(*edge, down_after_up.len());
                down_after_up.push(0);
            }
            Step::Down(_) => {
                *down_after_up
                    .last_mut()
                    .expect("the canonical word starts with an Up triangle") += 1;
            }
        }
    }
    Slab {
        down_after_up,
        upper_start,
    }
}

/// Splits a named oriented edge and renumbers its successors in the word.
fn split_step(steps: &[Step], edge: usize, kind: fn(usize) -> Step) -> Vec<Step> {
    let mut edited = Vec::new();
    for &step in steps {
        let (Step::Up(index) | Step::Down(index)) = step;
        if step == kind(index) && index > edge {
            edited.push(kind(index + 1));
        } else {
            edited.push(step);
        }
        if step == kind(edge) {
            edited.push(kind(edge + 1));
        }
    }
    edited
}

/// Removes a marked-index vertex only if its two edges are consecutive.
fn merge_steps(steps: &[Step], vertex: usize, kind: fn(usize) -> Step) -> Option<Vec<Step>> {
    let consecutive = steps
        .iter()
        .zip(steps.iter().cycle().skip(1))
        .any(|(&a, &b)| a == kind(vertex - 1) && b == kind(vertex));
    consecutive.then(|| {
        steps
            .iter()
            .filter_map(|&step| {
                let (Step::Up(index) | Step::Down(index)) = step;
                match (step == kind(index), index.cmp(&vertex)) {
                    (true, Ordering::Equal) => None,
                    (true, Ordering::Greater) => Some(kind(index - 1)),
                    _ => Some(step),
                }
            })
            .collect()
    })
}

/// Enumerates the outcomes of a family by editing words, including rejected atoms.
fn word_outcomes(source: &[Slab], family: MoveType) -> Vec<Option<Vec<Slab>>> {
    let profile: Vec<_> = source.iter().map(|slab| slab.down_after_up.len()).collect();
    let words: Vec<_> = source
        .iter()
        .zip(profile.iter().cycle().skip(1))
        .map(|(slab, &upper)| word(slab, upper))
        .collect();
    let mut outcomes = Vec::new();
    for (slice, (&length, steps)) in profile.iter().zip(&words).enumerate() {
        let incoming = (slice + source.len() - 1) % source.len();
        match family {
            MoveType::Move22 | MoveType::EdgeFlip => {
                for left in 0..steps.len() {
                    let right = (left + 1) % steps.len();
                    if matches!(steps[left], Step::Up(_)) == matches!(steps[right], Step::Up(_)) {
                        continue;
                    }
                    let mut flipped = steps.clone();
                    flipped.swap(left, right);
                    let mut target = source.to_vec();
                    target[slice] = slab_from_word(&flipped);
                    outcomes.push(Some(target));
                }
            }
            MoveType::Move13Add => {
                for edge in 0..length {
                    let mut target = source.to_vec();
                    target[slice] = slab_from_word(&split_step(steps, edge, Step::Up));
                    target[incoming] =
                        slab_from_word(&split_step(&words[incoming], edge, Step::Down));
                    outcomes.push(Some(target));
                }
            }
            MoveType::Move31Remove if length > 3 => {
                for vertex in 1..length {
                    let up = merge_steps(steps, vertex, Step::Up);
                    let down = merge_steps(&words[incoming], vertex, Step::Down);
                    outcomes.push(up.zip(down).map(|(up, down)| {
                        let mut target = source.to_vec();
                        target[slice] = slab_from_word(&up);
                        target[incoming] = slab_from_word(&down);
                        target
                    }));
                }
            }
            MoveType::Move31Remove => {}
        }
    }
    outcomes
}

/// Sums family masses independently from the production transition calculation.
fn word_probability(
    distribution: CdtMoveFamilyDistribution,
    source: &[Slab],
    destination: &[Slab],
) -> f64 {
    MoveType::REVERSIBLE_1P1
        .iter()
        .map(|&family| {
            let destinations = word_outcomes(source, family);
            if destinations.is_empty() {
                return distribution.probability(family) * f64::from(source == destination);
            }
            let count = destinations
                .iter()
                .filter(|outcome| outcome.as_deref().unwrap_or(source) == destination)
                .count();
            distribution.probability(family) * count_to_f64(count)
                / count_to_f64(destinations.len())
        })
        .sum()
}

/// Compares the complete offered atom multisets and both endpoint masses.
fn check_transition_oracle(state: &IntrinsicCdtTriangulation) {
    let distribution = CdtMoveFamilyDistribution::from_weights([1.0, 2.0, 4.0, 1.0])
        .expect("valid asymmetric family weights");
    let mut proposal = IntrinsicCdtProposal::new(distribution);
    let target = CdtTarget::new(ActionConfig::default(), 1.0).expect("valid test temperature");
    let mut self_loop = 0.0;
    for family in MoveType::REVERSIBLE_1P1 {
        let sites = offered_sites(&state.slabs, family);
        let mut expected = word_outcomes(&state.slabs, family);
        assert_eq!(sites.len(), expected.len());
        if sites.is_empty() {
            self_loop += distribution.probability(family);
        }
        for &site in &sites {
            let plan = proposal
                .plan_selected_site(state, family, &sites, site)
                .expect("offered-site evaluation succeeds");
            let observed = plan.as_ref().map(|plan| plan.proposed.slabs.as_ref());
            let position = expected
                .iter()
                .position(|outcome| outcome.as_deref() == observed)
                .expect("production atom has an independently enumerated counterpart");
            expected.swap_remove(position);
            if let Some(plan) = plan {
                let (forward, reverse) = plan.transition_probabilities();
                let next = &plan.proposed;
                assert!(reverse > 0.0);
                assert_relative_eq!(
                    forward,
                    word_probability(distribution, &state.slabs, &next.slabs),
                    epsilon = 1e-15
                );
                assert_relative_eq!(
                    reverse,
                    word_probability(distribution, &next.slabs, &state.slabs),
                    epsilon = 1e-15
                );
                let old_mass = target.log_prob(state).exp();
                let new_mass = target.log_prob(next).exp();
                let accepted_forward = (target.log_prob(next) - target.log_prob(state)
                    + plan.log_proposal_ratio())
                .min(0.0)
                .exp();
                let accepted_reverse = (target.log_prob(state) - target.log_prob(next)
                    + (forward / reverse).ln())
                .min(0.0)
                .exp();
                assert_relative_eq!(
                    old_mass * forward * accepted_forward,
                    new_mass * reverse * accepted_reverse,
                    max_relative = 1e-12
                );
                let mut committed = state.clone();
                let mut rng = StdRng::seed_from_u64(17);
                proposal
                    .commit(&mut committed, plan.clone(), &mut rng)
                    .expect("commit a plan at its source");
                assert_eq!(committed, *next);
            } else {
                self_loop += distribution.probability(family) / count_to_f64(sites.len());
            }
        }
        assert_eq!(expected, []);
    }
    assert_relative_eq!(
        self_loop,
        word_probability(distribution, &state.slabs, &state.slabs),
        epsilon = 1e-15
    );
}

#[test]
fn oversized_intrinsic_profile_returns_a_typed_error() {
    // The torus cell counts fit usize, but even one slab's run vector cannot
    // fit into the addressable allocation range.
    let length = usize::MAX / 4;
    assert!(
        length
            .checked_add(6)
            .and_then(|count| count.checked_mul(3))
            .is_some()
    );
    assert_matches!(
        IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[length, 3, 3]),
        Err(IntrinsicCdtError::CellCountOverflow)
    );
}

#[test]
fn all_small_marked_strips_have_an_independently_checked_transition_row() {
    let base = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 3, 3])
        .expect("valid shortest torus");
    let mut words = 0;
    for down_mask in 0_u32..(1 << 5) {
        if down_mask.count_ones() != 3 {
            continue;
        }
        words += 1;
        for upper_start in 0..3 {
            let mut slabs = base.slabs.to_vec();
            let mut steps = vec![Step::Up(0)];
            let mut lower = 1;
            let mut upper = upper_start;
            for position in 0..5 {
                if down_mask & (1 << position) == 0 {
                    steps.push(Step::Up(lower));
                    lower += 1;
                } else {
                    steps.push(Step::Down(upper));
                    upper = (upper + 1) % 3;
                }
            }
            slabs[2] = slab_from_word(&steps);
            let state = IntrinsicCdtTriangulation::from_slabs(slabs)
                .expect("each enumerated strip forms a valid torus");
            assert_eq!(state.counts(), (9, 27, 18));
            if down_mask == 0b1_1100 && upper_start == 0 {
                assert_eq!(state.gluing.edges_with_endpoints([6, 0]), 2);
            }
            check_transition_oracle(&state);
        }
    }
    assert_eq!(words, 10);
}

#[test]
fn evolved_seam_volume_edits_have_multiplicity_and_both_inverses() {
    let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 3, 3])
        .expect("valid shortest torus");
    let mut slabs = state.slabs.to_vec();
    slabs[0].down_after_up = vec![0, 0, 3];
    slabs[2].down_after_up = vec![3, 0, 0];
    let state =
        IntrinsicCdtTriangulation::from_slabs(slabs).expect("valid torus with parallel edges");
    let proposal = IntrinsicCdtProposal::default();
    let family = MoveType::Move13Add;
    let sites = offered_sites(&state.slabs, family);
    let plan = proposal
        .plan_selected_site(&state, family, &sites, Site::Insert { slice: 0, edge: 2 })
        .expect("seam insertion planning succeeds")
        .expect("the seam insertion is offered");
    let (forward, reverse) = plan.transition_probabilities();
    assert_relative_eq!(forward, 0.25 * 3.0 / 9.0, epsilon = 1e-15);
    assert_relative_eq!(reverse, 0.25 * 3.0 / 3.0, epsilon = 1e-15);
    check_transition_oracle(&plan.proposed);

    let family = MoveType::Move31Remove;
    let sites = offered_sites(&plan.proposed.slabs, family);
    let removal = proposal
        .plan_selected_site(
            &plan.proposed,
            family,
            &sites,
            Site::Remove {
                slice: 0,
                vertex: 3,
            },
        )
        .expect("seam removal planning succeeds")
        .expect("the inserted non-origin vertex is removable");
    assert_eq!(removal.proposed, state);
    assert_relative_eq!(
        removal.log_proposal_ratio(),
        -plan.log_proposal_ratio(),
        epsilon = 1e-15
    );
}

#[test]
fn upstream_chain_records_zero_reverse_support_and_empty_families() {
    let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 3, 3])
        .expect("valid shortest torus");
    let target = CdtTarget::new(ActionConfig::default(), 1.0).expect("valid test temperature");
    let distribution = CdtMoveFamilyDistribution::from_weights([0.0, 1.0, 0.0, 0.0])
        .expect("valid insertion-only distribution");
    let mut proposal = IntrinsicCdtProposal::new(distribution);
    let mut rng = StdRng::seed_from_u64(17);
    let plan = proposal
        .propose_plan(&state, &mut rng)
        .expect("insertion planning succeeds")
        .expect("shortest torus offers insertion");
    assert!(
        plan.log_proposal_ratio().is_infinite() && plan.log_proposal_ratio().is_sign_negative()
    );
    let mut chain = Chain::new(state.clone(), &target).expect("finite initial target weight");
    let step = chain
        .step_delayed(&target, &mut proposal, &mut rng)
        .expect("unsupported reverse is an ordinary rejection");
    assert_eq!(step.outcome(), StepOutcome::RejectedProposal);
    assert_eq!(*chain.state(), state);

    let distribution = CdtMoveFamilyDistribution::from_weights([0.0, 0.0, 1.0, 0.0])
        .expect("valid removal-only distribution");
    let mut proposal = IntrinsicCdtProposal::new(distribution);
    let step = chain
        .step_delayed(&target, &mut proposal, &mut rng)
        .expect("empty family is an ordinary self-loop");
    assert_eq!(step.outcome(), StepOutcome::NoProposal);
    assert_eq!(step.info(), Some(&MoveType::Move31Remove));
    assert_eq!(proposal.no_plan_info(), None);
}

#[test]
fn intrinsic_plan_cannot_commit_to_a_different_marked_source() {
    let source =
        IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 3, 3]).expect("minimum torus");
    let mut altered = source.slabs.to_vec();
    altered[0].upper_start = 1;
    let mut other = IntrinsicCdtTriangulation::from_slabs(altered)
        .expect("same profile with a different gluing");
    let unchanged = other.clone();
    let distribution = CdtMoveFamilyDistribution::from_weights([0.0, 1.0, 0.0, 0.0])
        .expect("insertion-only test policy");
    let mut proposal = IntrinsicCdtProposal::new(distribution);
    let mut rng = StdRng::seed_from_u64(17);
    let plan = proposal
        .propose_plan(&source, &mut rng)
        .expect("plan evaluation")
        .expect("valid intrinsic insertion");
    assert_matches!(
        proposal.log_q_ratio(&other, &plan),
        Err(IntrinsicCdtError::SourceMismatch)
    );
    assert_matches!(
        proposal.commit(&mut other, plan, &mut rng),
        Err(IntrinsicCdtError::SourceMismatch)
    );
    assert_eq!(other, unchanged);
}

#[test]
fn upstream_chain_accepts_growth_and_shrinkage_in_evolved_states() {
    let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[4, 4, 4, 4])
        .expect("valid flat torus");
    let target = CdtTarget::new(ActionConfig::default(), 1.0).expect("valid test temperature");
    let mut proposal = IntrinsicCdtProposal::default();
    let mut rng = StdRng::seed_from_u64(316);
    let mut chain = Chain::new(state, &target).expect("finite initial target weight");
    let mut growth = 0;
    let mut shrinkage = 0;
    for _ in 0..400 {
        let step = chain
            .step_delayed(&target, &mut proposal, &mut rng)
            .expect("intrinsic step succeeds");
        if step.outcome() == StepOutcome::Accepted {
            match step.info() {
                Some(MoveType::Move13Add) => growth += 1,
                Some(MoveType::Move31Remove) => shrinkage += 1,
                _ => {}
            }
        }
    }
    assert!(growth > 0 && shrinkage > 0);
    check_transition_oracle(chain.state());
}

/// Projects the two regular-grid regression endpoints using only their faces.
/// Increasing x fixes a common oriented circle and retains vertex zero in both
/// endpoints.
fn projected_grid_slabs(mesh: &CdtTriangulation2D) -> Vec<Slab> {
    let geometry = mesh.geometry();
    let times = mesh.metadata().time_slices().get() as usize;
    let mut rings = vec![Vec::new(); times];
    for vertex in geometry.vertices() {
        let time = mesh
            .time_label(&vertex)
            .expect("live vertex label lookup")
            .expect("fixture vertex has a time label") as usize;
        let x = geometry
            .vertex_coordinates(&vertex)
            .expect("fixture vertex has coordinates")[0];
        rings[time].push((x, vertex));
    }
    for ring in &mut rings {
        ring.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    }
    let mut slabs: Vec<_> = rings
        .iter()
        .map(|ring| Slab {
            down_after_up: vec![0; ring.len()],
            upper_start: usize::MAX,
        })
        .collect();
    for face in geometry.faces() {
        let mut corners: Vec<_> = geometry
            .face_vertices(&face)
            .expect("live face has vertices")
            .map(|vertex| {
                let time = mesh
                    .time_label(&vertex)
                    .expect("live corner label lookup")
                    .expect("fixture corner has a time label") as usize;
                let index = rings[time]
                    .iter()
                    .position(|(_, v)| *v == vertex)
                    .expect("face corner belongs to its oriented ring");
                (time, index)
            })
            .collect();
        corners.sort_unstable();
        let [(t0, a), (t1, b), (t2, c)] = corners[..] else {
            panic!("three corners");
        };
        let (pair_time, first, second, apex_time, apex) = if t0 == t1 {
            (t0, a, b, t2, c)
        } else {
            assert_eq!(t1, t2);
            (t1, b, c, t0, a)
        };
        if (pair_time + 1) % times == apex_time {
            if first == 0 && second == 1 {
                slabs[pair_time].upper_start = apex;
            }
        } else {
            assert_eq!((apex_time + 1) % times, pair_time);
            let preceding_edge = (apex + rings[apex_time].len() - 1) % rings[apex_time].len();
            slabs[apex_time].down_after_up[preceding_edge] += 1;
        }
    }
    slabs
}

#[test]
fn original_coordinate_removal_is_reversible_as_an_intrinsic_transition() {
    let mut mesh = CdtTriangulation2D::from_toroidal_cdt(4, 4).expect("original 4x4 torus builds");
    let distribution = CdtMoveFamilyDistribution::from_weights([0.0, 1.0, 1.0, 0.0])
        .expect("original family weights are valid");
    let mut embedded = CdtProposal::new(ActionConfig::default())
        .with_seed(1)
        .with_policy(distribution);
    let mut rng = StdRng::seed_from_u64(17);
    let plan = embedded
        .propose_plan(&mesh, &mut rng)
        .expect("original removal planning succeeds")
        .expect("original removal realizes");
    assert_eq!(plan.move_type(), MoveType::Move31Remove);
    let source = IntrinsicCdtTriangulation::from_slabs(projected_grid_slabs(&mesh))
        .expect("source projects to a valid gluing");
    embedded
        .commit(&mut mesh, plan, &mut rng)
        .expect("original removal applies to its source");
    let destination = IntrinsicCdtTriangulation::from_slabs(projected_grid_slabs(&mesh))
        .expect("destination projects to a valid gluing");
    let proposal = IntrinsicCdtProposal::new(distribution);
    let family = MoveType::Move31Remove;
    let sites = offered_sites(&source.slabs, family);
    let removal = sites
        .iter()
        .find_map(|&site| {
            proposal
                .plan_selected_site(&source, family, &sites, site)
                .expect("embedded projection has a valid intrinsic plan")
                .filter(|plan| plan.proposed == destination)
        })
        .expect("actual removal reaches the projected endpoint");
    let (forward, reverse) = removal.transition_probabilities();
    assert!(forward > 0.0 && reverse > 0.0);
    assert_relative_eq!(
        reverse,
        word_probability(distribution, &destination.slabs, &source.slabs),
        epsilon = 1e-15
    );
}

#[test]
fn intrinsic_target_checks_volume_fixing_even_at_zero_strength() {
    let state = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3; 4])
        .expect("valid four-slice torus");
    for strength in [0.0, 0.5] {
        let action = ActionConfig::default()
            .with_volume_fixing(Some(
                VolumeFixing::quadratic_triangles(22, strength).expect("positive even target"),
            ))
            .expect("finite action range");
        let target = CdtTarget::new(action, 1.0).expect("finite scaled action");
        assert_matches!(
            target.intrinsic_log_prob(&state),
            Err(IntrinsicCdtError::VolumeFixing(
                VolumeFixingError::TargetBelowMinimum {
                    target: 22,
                    minimum: 24
                }
            ))
        );
        assert_matches!(
            Chain::new(state.clone(), &target),
            Err(crate::McmcError::NanInitialLogProb)
        );
    }
}

#[test]
fn intrinsic_target_resolves_marking_change_or_reports_precision_loss() {
    let source =
        IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 3, 3]).expect("minimal torus");
    let next =
        IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[4, 3, 3]).expect("one longer loop");
    // At V=9 and V=10, -2V + (2V-18)^2/2 is exactly -18.
    let action = ActionConfig::new(2.0, 0.0, 0.0)
        .expect("finite vertex coupling")
        .with_volume_fixing(Some(
            VolumeFixing::quadratic_triangles(18, 0.5).expect("legal minimum volume"),
        ))
        .expect("finite combined action");
    let resolved = CdtTarget::new(action.clone(), 1.0).expect("unit temperature");
    assert_relative_eq!(
        (resolved.log_prob(&next) - resolved.log_prob(&source)).exp(),
        0.75,
        epsilon = 2e-15
    );
    let unresolved = CdtTarget::new(action, 1e-20).expect("finite scaled action");
    for state in [source, next] {
        assert_matches!(
            unresolved.intrinsic_log_prob(&state),
            Err(IntrinsicCdtError::MarkingPrecision { .. })
        );
        assert_matches!(
            Chain::new(state, &unresolved),
            Err(crate::McmcError::NanInitialLogProb)
        );
    }
}

#[test]
fn unresolved_proposed_marking_stops_chain_before_commit() {
    let state =
        IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3; 3]).expect("minimal torus");
    let action = ActionConfig::new(0.0, 0.0, 0.0)
        .expect("zero physical action")
        .with_volume_fixing(Some(
            VolumeFixing::quadratic_triangles(18, 0.5).expect("legal minimum volume"),
        ))
        .expect("finite action range");
    let target = CdtTarget::new(action, 1e-20).expect("finite scaled action");
    let mut chain = Chain::new(state.clone(), &target).expect("zero initial fixing action");
    let mut proposal = IntrinsicCdtProposal::new(
        CdtMoveFamilyDistribution::from_weights([0.0, 1.0, 0.0, 0.0])
            .expect("insertion-only policy"),
    );
    let mut rng = StdRng::seed_from_u64(316);
    assert_matches!(
        chain.step_delayed(&target, &mut proposal, &mut rng),
        Err(markov_chain_monte_carlo::DelayedStepError::Mcmc(
            crate::McmcError::NanProposedLogProb
        ))
    );
    assert_eq!(chain.state(), &state);
    assert_eq!(chain.total_steps(), 0);
}

#[test]
fn summing_computational_markings_recovers_inverse_automorphism_weight() {
    let base = IntrinsicCdtTriangulation::from_spatial_loop_lengths(&[3, 3, 3])
        .expect("valid shortest torus");
    let mut asymmetric = base.slabs.to_vec();
    asymmetric[1].down_after_up = vec![3, 0, 0];
    let no_action = ActionConfig::new(0.0, 0.0, 0.0).expect("zero action is valid");
    let target = CdtTarget::new(no_action, 1.0).expect("valid test temperature");

    for (slabs, stabilizer) in [(base.slabs.as_ref(), 3), (&asymmetric, 1)] {
        let mut distinct = Vec::new();
        let mut fixes = 0;
        for rotation in 0..27 {
            let roots = [rotation % 3, (rotation / 3) % 3, rotation / 9];
            let marked: Vec<_> = slabs
                .iter()
                .enumerate()
                .map(|(time, slab)| {
                    let steps: Vec<_> = word(slab, 3)
                        .into_iter()
                        .map(|step| match step {
                            Step::Up(edge) => Step::Up((edge + 3 - roots[time]) % 3),
                            Step::Down(edge) => Step::Down((edge + 3 - roots[(time + 1) % 3]) % 3),
                        })
                        .collect();
                    slab_from_word(&steps)
                })
                .collect();
            fixes += u32::from(marked == slabs);
            if !distinct.contains(&marked) {
                distinct.push(marked);
            }
        }
        assert_eq!(fixes, stabilizer);
        let mass = distinct
            .into_iter()
            .map(|slabs| {
                target
                    .log_prob(
                        &IntrinsicCdtTriangulation::from_slabs(slabs)
                            .expect("re-rooting preserves the gluing"),
                    )
                    .exp()
            })
            .sum::<f64>();
        assert_relative_eq!(mass, 1.0 / f64::from(stabilizer), epsilon = 1e-14);
    }
}
