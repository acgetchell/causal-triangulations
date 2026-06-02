# Metropolis Move Ordering And Detailed Balance

> **MCMC backend boundary:** this page describes the current CDT production Metropolis runner. Its proposal-before-mutation contract should be preserved, but
> generic Metropolis-Hastings mechanics should live behind `markov-chain-monte-carlo` adapters rather than CDT-local sampler logic. Chunked continuation now
> uses upstream proposal planning and checkpoint-compatible continuation from `markov-chain-monte-carlo` v0.4. Repository-owned Semgrep rules enforce the
> production boundary by rejecting CDT-local generic acceptance draws and manual accepted/rejected sampler counters; any further upstream planned-step hooks and
> telemetry needs are tracked by [`markov-chain-monte-carlo#61`](https://github.com/acgetchell/markov-chain-monte-carlo/issues/61).

`MetropolisAlgorithm::run()` uses a proposal-before-mutation ordering for CDT Monte Carlo steps.

For each step:

1. Select a move type with `ErgodicsSystem::select_random_move()`.
2. Read the cached sampleable local sites for that move type, rebuilding the cache first if the triangulation cache key changed, and select one site uniformly
   from that same site universe. If no site exists, record the step as a self-loop proposal and continue without changing the live triangulation.
3. Compute the proposed action change from the concrete proposal's simplex-count delta:
   - `(2,2)` / edge flip: `ΔN0 = 0`, `ΔN1 = 0`, `ΔN2 = 0`
   - `(1,3)`: `ΔN0 = +1`, `ΔN1 = +3`, `ΔN2 = +2`
   - `(3,1)`: `ΔN0 = -1`, `ΔN1 = -3`, `ΔN2 = -2`
4. Apply the selected site once on a cloned proposed state. Ordinary causal, geometric, or backend edit failures are self-loop proposal outcomes; the live
   triangulation is unchanged.
5. Count the forward sites from the cached selected move-family set and count the reverse sites for the inverse move type on the proposed state.
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

## Scientific Calibration And Geometry Backend Role

The Delaunay backend is an implementation substrate, not the sampled physics ensemble. It gives the crate a robust way to construct an initial piecewise-linear
manifold, validate topology and adjacency, and perform checked local edit primitives. After initialization, the simulation does not enforce the Delaunay
condition as part of the Markov-chain state. The sampled ensemble is defined by the CDT move set, foliation/topology/causality validation, the configured
action, and the Metropolis-Hastings acceptance rule.

The default action constants are calibrated for the non-volume-fixed 1+1 CDT baseline. In pure 1+1 gravity the curvature/Newton term is topological at fixed
topology, so the default vertex and triangle couplings are zero:

```text
kappa_0 = 0
kappa_2 = 0
```

The exactly solved 2D CDT model has critical cosmological coupling `lambda_c = ln 2` in the triangle-volume convention where configurations are weighted by
`exp(-lambda N2)`. This crate's historical action writes the cosmological term as `lambda_edge N1`. For closed toroidal 1+1 triangulations,
`N1 = 3 N2 / 2`, so the default edge-count coupling is

```text
lambda_edge = (2 / 3) ln 2 ~= 0.46209812037329684
```

With these defaults, toroidal 1+1 runs map the edge-count action convention to the standard critical triangle-volume coupling. Non-volume-fixed finite runs may
still drift because the critical point is a continuum-limit statement and there is no volume-fixing penalty. The follow-up volume-fixing work in
[`causal-triangulations#142`](https://github.com/acgetchell/causal-triangulations/issues/142) should add an explicit modified-action ensemble for production
fixed-volume studies.

The calibration is based on Ambjørn and Loll's original 2D CDT construction and the Ambjørn, Görlich, Jurkiewicz, and Loll review. The
volume-fixing follow-up should cite the higher-dimensional CDT simulation literature where quadratic fixing terms are added deliberately. See
[REFERENCES.md](../REFERENCES.md) for the full citations.

## Chunked Sweeps

`MetropolisAlgorithm::resume_to_checkpoint()` continues a stored `CdtMcmcCheckpoint` for the configured additional step count and returns another resumable
checkpoint. This keeps the checkpointed triangulation, Metropolis acceptance RNG, ergodic proposal RNG, counters, measurements, and elapsed-time telemetry
together between chunks.

Chunk execution is implemented through `markov-chain-monte-carlo::Sampler::step_delayed` on the CDT proposal-plan adapter. The upstream sampler owns the
Metropolis-Hastings accept/reject draw, log-probability cache, chain counters, and checkpoint-compatible continuation view. CDT keeps domain-specific state
outside that generic sampler: action and schedule metadata, proposal telemetry, move statistics, measurements, elapsed time, and the serialized ergodic proposal
RNG.

## Trace CSV Diagnostics

Completed Metropolis steps are recorded as upstream `markov-chain-monte-carlo::Trace` records. `SimulationResultsBackend::scalar_trace()` builds the in-memory
trace, and `SimulationResultsBackend::write_trace_csv()` writes the same rectangular table through the upstream CSV writer. The configured `--output-csv` path
therefore exports one row per completed step, not one row per scheduled measurement.

The fixed upstream columns are `chain_id`, `step`, `accepted`, `proposed`, and `log_prob`. CDT adds numeric observable columns for the current action,
vertex/edge/triangle counts, stable move-family code, action delta and before/after action fields with presence flags, optional seed split into exactly
representable `u32` halves, and zero-filled `volume_profile_*` columns. The `proposed` column distinguishes no-site/no-plan self-loops from concrete proposals
that were rejected by Metropolis-Hastings or CDT-local proposal checks.

CSV is the core export format so downstream tools such as Polars can load the data without coupling the crate to a dataframe or Parquet dependency. Plotting,
Parquet conversion, and wider ensemble analysis belong downstream of this typed trace export.

The large-scale 1+1 debug harness uses this upstream-backed chunking path to run one Metropolis sweep at a time:

1. Read the current number of top-dimensional simplices at the start of the sweep.
2. Run exactly that many Metropolis proposal steps.
3. Inspect and log the checkpointed state.
4. Enforce wall-clock caps between sweeps.

Those debug runs are unfixed-volume Metropolis sweeps. The cosmological constant in the action controls volume growth or shrinkage; the harness does not impose
a fixed-volume constraint or reuse the initial simplex count as a fixed step budget for later sweeps.

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

`ErgodicsSystem` owns this proposal-site universe as a per-move-family cache keyed by `(instance_id, modification_count)`. The instance identity prevents
cross-instance reuse when distinct triangulations have colliding modification counts, while the modification count keeps ordinary self-loop outcomes cheap.
Forward counts are therefore the cached set length, and forward sampling is uniform over the same cached vector. Accepted mutations replace or mutate the
triangulation, causing the next proposal to rebuild before sampling stale handles. Proposed-state reverse counts are computed from a fresh cache for the cloned
proposed state using that same `(instance_id, modification_count)` validity check.

This is the ordinary Metropolis-Hastings proposal-ratio correction: it uses the actual proposal kernel, not empirical acceptance counts from the run. Hastings'
original Markov-chain sampling paper gives the general acceptance rule, and Brunekreef, Görlich, and Loll describe CDT Monte Carlo moves together with their
detailed-balance equations. See [REFERENCES.md](../REFERENCES.md) for the full citations.

The integration test `test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology` covers this contract by running a seeded S¹×S¹ simulation,
requiring at least one accepted periodic move, and checking final topology, foliation, causality, simplex classification, and Euler characteristic.

## Proposal Sites

A CDT proposal is represented by an explicit local site selected from the current triangulation:

1. Choose a move family `m` from the configured move-family distribution.
2. Read or rebuild the cached sampleable local sites `S_m(x)` for the current triangulation `x`.
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
used by planned Metropolis acceptance, plus a narrow rollback snapshot around composite mutations whose intermediate backend steps can partially commit.
