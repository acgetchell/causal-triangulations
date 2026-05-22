# Metropolis Move Ordering

`MetropolisAlgorithm::run()` uses a proposal-before-mutation ordering for CDT Monte Carlo steps.

For each step:

1. Select a move type with `ErgodicsSystem::select_random_move()`.
2. Compute the proposed action change from the move type's simplex-count delta:
   - `(2,2)` / edge flip: `ΔN0 = 0`, `ΔN1 = 0`, `ΔN2 = 0`
   - `(1,3)`: `ΔN0 = +1`, `ΔN1 = +3`, `ΔN2 = +2`
   - `(3,1)`: `ΔN0 = -1`, `ΔN1 = -3`, `ΔN2 = -2`
3. Accept the proposed move if `ΔS <= 0`, or with probability `exp(-ΔS / T)`.
4. Only after acceptance, apply the move through the ergodic move kernel.
5. If an accepted application fails at its chosen local site, the move kernel restores its pre-mutation triangulation snapshot and the simulation retries at
   another randomly selected site for a bounded number of attempts. For toroidal triangulations, this includes candidate edits that would violate χ = 0 or break
   a spatial slice's closed-S¹ ring after mutation.
6. If all retries fail because no local site is valid, record the step as rejected and continue; the triangulation has been restored to its pre-application
   state.
7. If a backend edit reports a hard mutation failure, return `CdtError::MetropolisMoveApplicationFailed` with the step, move type, retry count, and lower-level
   failure.

This ordering avoids mutating the triangulation for Metropolis-rejected moves. Rollback is still required for accepted moves because a backend edit can fail
after a site has been selected or after partial invariant refresh work. Ergodic kernels snapshot lazily, after candidate selection and immediately before
mutation, so ordinary local-site rejections avoid cloning the triangulation. Exhausted local-site retries are ordinary proposal rejections because the selected
move type did not bind to a realizable local site within the bounded search. The recorded `SimulationResultsBackend::move_stats` counts Metropolis-level
attempted and applied moves, while `ErgodicsSystem::stats` remains local to the lower-level move kernel.

The integration test `test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology` covers this contract by running a seeded S¹×S¹ simulation,
requiring at least one accepted periodic move, and checking final topology, foliation, causality, simplex classification, and Euler characteristic.
