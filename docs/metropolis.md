# Metropolis Move Ordering

`MetropolisAlgorithm::run()` uses a proposal-before-mutation ordering for CDT Monte Carlo steps.

For each step:

1. Select a move type with `ErgodicsSystem::select_random_move()`.
2. Build a concrete delayed proposal for that move type by selecting and applying a realizable local site on a cloned triangulation. If no concrete site exists,
   record the step as a rejected proposal and continue without changing the live triangulation.
3. Compute the proposed action change from the concrete proposal's simplex-count delta:
   - `(2,2)` / edge flip: `ΔN0 = 0`, `ΔN1 = 0`, `ΔN2 = 0`
   - `(1,3)`: `ΔN0 = +1`, `ΔN1 = +3`, `ΔN2 = +2`
   - `(3,1)`: `ΔN0 = -1`, `ΔN1 = -3`, `ΔN2 = -2`
4. Count the forward sites for the selected move type and the reverse sites for the inverse move type on the proposed state.
5. Accept with the Metropolis-Hastings probability
   `min(1, exp(-ΔS / T + log(q(current | proposed) / q(proposed | current))))`. For equal move-family weights this adds
   `log(forward_site_count / reverse_site_count)` to the ordinary action term, so `(1,3)` and `(3,1)` proposals account for their different site
   multiplicities.
6. Only after acceptance, replace the live triangulation with the planned proposed state.
7. If proposal planning hits a hard backend or invariant failure, return `CdtError::MetropolisMoveApplicationFailed` with the step, move type, retry count, and
   lower-level failure.

This ordering avoids mutating the live triangulation for Metropolis-rejected moves while still binding each proposal to a concrete local transition before the
acceptance draw. Rollback remains inside the move kernels while planning on the cloned state because a backend edit can fail after a site has been selected or
after partial invariant refresh work. Exhausted local-site retries are ordinary proposal rejections because the selected move type did not bind to a realizable
local site within the bounded search. The recorded `SimulationResultsBackend::move_stats` counts Metropolis-level attempted and applied moves, while
`ErgodicsSystem::stats` remains local to the lower-level move kernel.

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

This is the ordinary Metropolis-Hastings proposal-ratio correction: it uses the actual proposal kernel, not empirical acceptance counts from the run. Hastings'
original Markov-chain sampling paper gives the general acceptance rule, and Brunekreef, Görlich, and Loll describe CDT Monte Carlo moves together with their
detailed-balance equations. See [REFERENCES.md](../REFERENCES.md) for the full citations.

The integration test `test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology` covers this contract by running a seeded S¹×S¹ simulation,
requiring at least one accepted periodic move, and checking final topology, foliation, causality, simplex classification, and Euler characteristic.
