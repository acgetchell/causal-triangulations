# Metropolis Move Ordering And Detailed Balance

`MetropolisAlgorithm::run()` uses a proposal-before-mutation ordering for CDT Monte Carlo steps.

For each step:

1. Select a move type with `ErgodicsSystem::select_random_move()`.
2. Enumerate the sampleable local sites for that move type and select one site uniformly from that same site universe. If no site exists, record the step as a
   self-loop proposal and continue without changing the live triangulation.
3. Compute the proposed action change from the concrete proposal's simplex-count delta:
   - `(2,2)` / edge flip: `ΔN0 = 0`, `ΔN1 = 0`, `ΔN2 = 0`
   - `(1,3)`: `ΔN0 = +1`, `ΔN1 = +3`, `ΔN2 = +2`
   - `(3,1)`: `ΔN0 = -1`, `ΔN1 = -3`, `ΔN2 = -2`
4. Apply the selected site once on a cloned proposed state. Ordinary causal, geometric, or backend edit failures are self-loop proposal outcomes; the live
   triangulation is unchanged.
5. Count the forward sites for the selected move type and the reverse sites for the inverse move type on the proposed state.
6. Accept successful transitions with the Metropolis-Hastings probability
   `min(1, exp(-ΔS / T + log(q(current | proposed) / q(proposed | current))))`. For equal move-family weights this adds
   `log(forward_site_count / reverse_site_count)` to the ordinary action term, so `(1,3)` and `(3,1)` proposals account for their different site
   multiplicities.
7. Only after acceptance, replace the live triangulation with the planned proposed state.
8. If proposal planning hits a hard backend or invariant failure, return `CdtError::MetropolisMoveApplicationFailed` with the step, move type, retry count, and
   lower-level failure.

This ordering avoids mutating the live triangulation for Metropolis-rejected moves while still binding each proposal to a concrete local transition before the
acceptance draw. Rollback remains inside the move kernels while planning on the cloned state because a backend edit can fail after a site has been selected or
after partial invariant refresh work. The recorded `SimulationResultsBackend::move_stats` counts Metropolis-level attempted and applied moves, while
`SimulationResultsBackend::proposal_stats` records proposal-kernel telemetry such as no-site outcomes, sampled-site failures, Metropolis rejections, and
accepted transitions.

## Proposal-Ratio Correction

The proposal-ratio correction is not computed from the number of moves accepted during a run. It is a property of the concrete proposal distribution at the
current step:

```text
q(current | proposed) / q(proposed | current)
```

For the current sampler, move families are selected with equal probability. Once a move family is selected, the kernel chooses uniformly among its valid local
application sites. For a concrete transition from `current` to `proposed`:

```text
q(proposed | current) = P(forward move family) * 1 / N_forward
q(current | proposed) = P(reverse move family) * 1 / N_reverse
```

The equal move-family probabilities cancel for inverse move pairs, leaving:

```text
q(current | proposed) / q(proposed | current) = N_forward / N_reverse
log_q_ratio = log(N_forward) - log(N_reverse)
```

`N_forward` is the number of valid local sites for the selected move type in the current triangulation. `N_reverse` is the number of valid local sites for the
inverse move type in the proposed triangulation. For example, a `(1,3)` proposal counts valid `(1,3)` sites before the move and valid reverse `(3,1)` sites
after the move. This corrects proposal asymmetry for detailed balance without adding volume fixing or changing the target action.

The proposal-site universe must be the same for sampling and counting. If the sampler can choose a site, that site belongs in the denominator used for the
proposal probability, even if applying it later returns an ordinary geometric, causal, or backend rejection.

This is the ordinary Metropolis-Hastings proposal-ratio correction: it uses the actual proposal kernel, not empirical acceptance counts from the run. Hastings'
original Markov-chain sampling paper gives the general acceptance rule, and Brunekreef, Görlich, and Loll describe CDT Monte Carlo moves together with their
detailed-balance equations. See [REFERENCES.md](../REFERENCES.md) for the full citations.

The integration test `test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology` covers this contract by running a seeded S¹×S¹ simulation,
requiring at least one accepted periodic move, and checking final topology, foliation, causality, simplex classification, and Euler characteristic.

## Proposal Sites

A CDT proposal is represented by an explicit local site selected from the current triangulation:

1. Choose a move family `m` from the configured move-family distribution.
2. Enumerate the sampleable local sites `S_m(x)` for the current triangulation `x`.
3. Select one site uniformly and apply that exact site to the cloned proposed state.
4. Treat ordinary local or backend rejections as self-loop proposals, not hard failures.
5. Score successful transitions with the Metropolis-Hastings proposal ratio for the actual proposal kernel.

For state `x`, move family `m`, and a sampleable site set `S_m(x)`, a uniformly sampled site contributes:

```text
p(m | x) / |S_m(x)|
```

to the proposal probability. The full transition probability from `x` to a distinct proposed state `y` is the sum over every sampled site that produces that
same `y`:

```text
q(x -> y) = sum over (m, s) where apply(x, m, s) = y of p(m | x) / |S_m(x)|
```

For equal move-family weights and one proposal site per resulting state, this reduces to the familiar site-count correction:

```text
log_q_ratio = log(|S_forward(x)|) - log(|S_reverse(y)|)
```

If multiple proposal sites can produce the same final triangulation, that multiplicity must be included:

```text
log_q_ratio =
    log(p(reverse | y)) - log(p(forward | x))
  + log(multiplicity(y -> x)) - log(multiplicity(x -> y))
  + log(|S_forward(x)|) - log(|S_reverse(y)|)
```

The implementation should prefer canonical proposal-site definitions that make the multiplicity term equal to one. When that is not possible, the concrete
proposal plan must carry enough identity to compute the forward and reverse transition multiplicities explicitly.

## Ordinary Rejections

Ordinary proposal failures are self-loop probability mass. They do not alter the acceptance formula for successful transitions and must not be fed back into
the proposal ratio from empirical counters.

Ordinary rejections include:

- selected sites that fail CDT-local geometric or causal checks
- selected backend edits that fail atomically before committing a new CDT state
- Metropolis rejections of otherwise valid proposed transitions

Hard failures are different. A hard failure means a mutation partially committed or a required rollback/finalization invariant failed. Hard failures are
diagnostic errors, not normal MCMC rejection outcomes.

## Proposal Statistics

`ProposalStatistics` is telemetry, not part of the Markov kernel. It records:

- selected move-family proposals
- sampleable forward-site denominators observed during planning
- no-site outcomes
- ordinary geometric, causal, and backend proposal rejections
- Metropolis rejections
- successful committed transitions
- hard failures

These counters explain chain stickiness and backend behavior, but detailed balance is maintained by the proposal probability used at the current step, not by
empirical frequencies accumulated earlier in the run.

The explicit-site model avoids dry-run cloning every candidate during site counting. The only required full triangulation clone is the planned proposed state
used by delayed Metropolis acceptance, plus a narrow rollback snapshot around composite mutations whose intermediate backend steps can partially commit.
