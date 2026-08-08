# Metropolis Move Ordering And Detailed Balance

> **MCMC backend boundary:** this page describes the current CDT production Metropolis runner. Its proposal-before-mutation contract should be preserved, but
> generic Metropolis-Hastings mechanics should live behind `markov-chain-monte-carlo` adapters rather than CDT-local sampler logic. Chunked continuation now
> uses upstream proposal planning, weighted discrete proposal ratios, and checkpoint-compatible continuation from `markov-chain-monte-carlo` v0.4.1.
> Repository-owned Semgrep rules enforce the
> production boundary by rejecting CDT-local generic acceptance draws and manual accepted/rejected sampler counters; any further upstream planned-step hooks and
> telemetry needs are tracked by [`markov-chain-monte-carlo#61`](https://github.com/acgetchell/markov-chain-monte-carlo/issues/61).

`MetropolisAlgorithm::run()` uses a proposal-before-mutation ordering for CDT Monte Carlo steps.

For each step:

1. Obtain the configured `CdtMoveFamilyPolicy` distribution. Fixed policies return their checked distribution directly; state-dependent policies are
   evaluated once for every family in `MoveType::REVERSIBLE_1P1`, using one invariant-safe borrowed policy view per family. Validate returned finite
   nonnegative weights, normalize them, and sample one family without renormalizing around empty offered-site sets. The built-in conventional path uses
   `UniformCdtMoveFamilyPolicy` through this same boundary.
2. Read the cached sampleable local sites for that move type, rebuilding the cache first if the triangulation cache key changed, and select one site uniformly
   from that same site universe. If no site exists, record the step as a self-loop proposal and continue without changing the live triangulation.
3. Compute the proposed action change from the concrete proposal's simplex-count delta:
   - `(2,2)` / edge flip: `ΔN0 = 0`, `ΔN1 = 0`, `ΔN2 = 0`
   - `(1,3)`: `ΔN0 = +1`, `ΔN1 = +3`, `ΔN2 = +2`
   - `(3,1)`: `ΔN0 = -1`, `ΔN1 = -3`, `ΔN2 = -2`
4. Apply the selected site once on a cloned proposed state. Ordinary causal, geometric, or backend edit failures are self-loop proposal outcomes; the live
   triangulation is unchanged.
5. Count the forward sites from the cached selected move-family set. Re-evaluate the policy on the planned post-move state and count the reverse sites for the
   inverse family from the same canonical cache implementation.
6. Build the path-conditioned weighted family/site correction with `markov-chain-monte-carlo::DiscreteProposalRatio` and accept successful transitions with
   `min(1, exp(-ΔS / T + log(q(current | proposed) / q(proposed | current))))`. For equal move-family weights this adds
   `log(forward_site_count / reverse_site_count)` to the ordinary action term. Unequal or state-dependent policies additionally contribute
   `log(p(reverse(m) | y) / p(m | x))`.
7. Only after acceptance, replace the live triangulation with the planned proposed state.
8. If proposal planning hits a hard backend or invariant failure, return `CdtError::MetropolisMoveApplicationFailed` with the step, move type, retry count, and
   lower-level failure.

This ordering avoids mutating the live triangulation for Metropolis-rejected moves while still binding each proposal to a concrete local transition before the
acceptance draw. The speculative clone is itself the rollback boundary during planning, so move kernels do not clone it again; direct public move attempts own
one explicit rollback snapshot because a backend edit can fail after selection or partial invariant refresh. The recorded `SimulationResultsBackend::move_stats`
counts Metropolis-level attempted and applied moves, while `SimulationResultsBackend::proposal_stats` records proposal-kernel telemetry such as no-site
outcomes, sampled-site failures, Metropolis rejections, and accepted transitions.

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

### Serialized Checkpoint Compatibility

In-memory continuation and Serde round trips produced and consumed by the same build are supported. Serialized checkpoint files are otherwise version-bound:
their representation includes internal state from `causal-triangulations`, `delaunay`, and `markov-chain-monte-carlo`, and compatibility is not guaranteed
across crate or dependency upgrades. Read a serialized checkpoint with the same build that wrote it, or with a release that explicitly documents checkpoint
compatibility. In particular, checkpoints written through Delaunay 0.7 cannot be deserialized after the Delaunay 0.8 upgrade.
Checkpoints written before `initial_vertex_count` became required cannot be read by this release and must be regenerated; the field remains required because it
reconstructs the initial `SimulationEvent::Created` vertex count rather than guessing from the final triangulation.

Use trace CSV and JSON summary exports for durable cross-version analysis artifacts. A future CDT-owned, versioned checkpoint wire format is tracked in
[`causal-triangulations#218`](https://github.com/acgetchell/causal-triangulations/issues/218).

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
representable `u32` halves, and zero-filled `slab_triangle_profile_*` columns. The `proposed` column distinguishes no-site/no-plan self-loops from concrete
proposals that were rejected by Metropolis-Hastings or CDT-local proposal checks.

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

Under the default `UniformCdtMoveFamilyPolicy`, move families are selected with equal probability. Once a move family is selected, the kernel chooses uniformly
among its offered local application sites. For a successful concrete proposal from `current` to `proposed`:

```text
q(proposed | current) = P(forward move family) * 1 / N_forward
q(current | proposed) = P(reverse move family) * 1 / N_reverse
```

For that default policy, the equal move-family probabilities cancel for inverse move pairs, leaving:

```text
q(current | proposed) / q(proposed | current) = N_forward / N_reverse
log_q_ratio = log(N_forward) - log(N_reverse)
```

`N_forward` is the number of offered local sites for the selected move type in the current triangulation. `N_reverse` is the number of offered local sites for
the inverse move type in the proposed triangulation. For example, a `(1,3)` proposal counts offered `(1,3)` sites before the move and offered reverse `(3,1)`
sites after the move. This corrects proposal asymmetry for detailed balance without adding volume fixing or changing the target action.

The proposal-site universe must be the same for sampling and counting. If the sampler can choose a site, that site belongs in the denominator used for the
proposal probability, even if applying it later returns an ordinary geometric, causal, or backend rejection.

`ErgodicsSystem` owns this proposal-site universe as a per-move-family cache keyed by `(instance_id, modification_count)`. The instance identity prevents
cross-instance reuse when distinct triangulations have colliding modification counts, while the modification count keeps ordinary self-loop outcomes cheap.
Forward counts are therefore the cached set length, and forward sampling is uniform over the same cached vector. Accepted mutations replace or mutate the
triangulation, causing the next proposal to rebuild before sampling stale handles. Proposed-state reverse counts refresh the reverse-family entry in the same
`ErgodicsSystem` cache for the cloned proposed state using that `(instance_id, modification_count)` validity check.

Before a site enters that cache, the geometry adapter runs Delaunay 0.8's immutable k=1/k=2 feasibility validator for every primitive that can be checked on the
current state. These exact deterministic preflights come from
[`delaunay#419`](https://github.com/acgetchell/delaunay/issues/419) and avoid clone-and-try scans. They do not replace CDT's causal or topology checks, nor do
they promise that the later primitive in a composite foliated move or post-mutation CDT finalization cannot produce an ordinary self-loop rejection.

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

### Borrowed Proposal-Policy View

`CdtProposal::policy_view()` and `ErgodicsSystem::proposal_policy_view()` expose the canonical offered-site universe without exposing Delaunay handles or a
mutable triangulation. Each `CdtProposalPolicyView` is scoped to one move family so the conventional sampler can inspect only the selected family; an external
state-dependent policy receives one short-lived view for each entry in `MoveType::REVERSIBLE_1P1`. Fixed policies bypass unrelated family views and reuse
their checked distribution directly.

The policy view and opaque site-ID types are available from `prelude::simulation`; `prelude::moves` remains focused on local move kernels, move families, and
move statistics. `MoveType` intentionally appears in both scoped preludes because it identifies both kernel operations and simulation proposal families. The
stable family order and identifiers are:

| Family | Identifier | Reverse family |
| ------ | ---------- | -------------- |
| `Move22` | `move-2-2` | `Move22` |
| `Move13Add` | `move-1-3-add` | `Move31Remove` |
| `Move31Remove` | `move-3-1-remove` | `Move13Add` |
| `EdgeFlip` | `edge-flip` | `EdgeFlip` |

`Move22` and `EdgeFlip` remain distinct policy identifiers for compatibility with the conventional four-family distribution, although both use the same 2D
k=2 flip kernel and therefore expose the same site universe. The sampler treats them as distinct auxiliary mixture components and conditions acceptance on
the family/site path that was actually selected.

### Injected Family Policies

`CdtMoveFamilyPolicy::family_weight()` is the state-dependent family-only injection boundary. CDT calls it four times per evaluated state, once with each
`CdtProposalPolicyView`. Outputs are relative normalization inputs, not site weights: every value must be finite and nonnegative, and the complete array must
have a positive finite normalization total. Custom state-dependent implementations may use topology, simplex counts, slice sizes, family identity, and
offered-site information from the view.

`CdtMoveFamilyPolicy::fixed_distribution()` is the opt-in fast path for state-independent policies. `CdtMoveFamilyDistribution::from_weights()` provides a
checked fixed policy, while `UniformCdtMoveFamilyPolicy` supplies the conventional baseline; both reuse their distribution without enumerating all four
family-site caches. Effective probabilities are quantized to the proposal RNG's 53-bit categorical draw grid. Every positive input retains at least one draw
atom, and the same exact integer masses drive family sampling and the reported Hastings probabilities.

Individual zero family weights are allowed. All-zero output is `CdtMoveFamilyPolicyError::EmptySupport`; negative or non-finite output and non-finite
normalization totals are separate typed errors. A positive-weight family with zero offered sites remains in the family distribution and becomes a typed
`NoOfferedSite` self-loop if selected. CDT never conditions the family distribution on site availability after selection. For a successful forward plan,
zero reverse-family probability or zero reverse offered-site count is valid input to the MCMC ratio and yields a `-∞` log correction, so the transition is
rejected without mutating the chain.

Use `CdtProposal::new(action).with_seed(seed).with_policy(policy)` with an upstream delayed chain directly. The production facade binds the policy once through
`MetropolisAlgorithm::with_policy()`, after which `run()`, `run_with_checkpoint()`, `run_to_checkpoint()`, `resume_from_checkpoint()`, and
`resume_to_checkpoint()` all use that policy. CDT checkpoints preserve the proposal RNG stream but do not serialize external policy or model state; experiment
code must persist and restore that state separately, then bind the restored policy to the runner used for continuation.

The view exposes the selected and reverse families, CDT topology, invariant-bearing simplex counts, borrowed slice sizes, the offered-site count, and an
exact-size iterator of opaque `CdtProposalSiteId` values. Empty families return count zero and an empty iterator. IDs use deterministic ascending ordinals for
an unchanged triangulation version; the ordinal is not a persistent geometry key and no private backend handle is part of the public contract.

The view borrows both the triangulation and the versioned family cache, so Rust prevents state or cache mutation while inspection is active. Creating a view
does not clone `CdtTriangulation2D`. Synchronizing an uncached family may allocate its canonical site vector, while subsequent counting and ID iteration
allocate nothing. A detached site ID records triangulation identity and modification version: callers must validate it against a fresh view before reuse, and
receive a typed foreign-state, stale-state, family-mismatch, or ordinal error when it is no longer valid. Accepted mutations make earlier IDs stale for that
state; clones, deserialized values, and replacement triangulations have a different identity and reject those IDs as foreign.

The conventional sampler now reads counts and selected private site descriptors through this view over `MoveSiteCache`; there is no second site-enumeration
implementation for policy consumers.

#### Offered Sites Versus Eligible Sites

- An **offered site** passed the deterministic pre-mutation guards, can be sampled by the checked planner, and contributes to the proposal denominator below.
  A later composite backend edit, allocation failure, or post-mutation CDT validation may still reject it as ordinary self-loop probability, as described
  under [Ordinary Rejections](#ordinary-rejections).
- An **eligible site**, also called an executable site, would satisfy a stronger contract: for an unchanged state, all deterministic backend mutation
  preconditions and CDT postconditions needed by the move are known before sampling. This would support an executable-only action mask, although it still
  could not guarantee resource availability.

The current policy view exposes offered sites, not eligible sites. Downstream proposal policies must therefore use its IDs and counts as the actual proposal
support and must not interpret them as a guarantee that execution will succeed.

For state `x`, move family `m`, and a sampleable site set `S_m(x)`, one sampled auxiliary proposal atom `c = (m, s)` contributes:

```text
q_c(x -> y) = p(m | x) / |S_m(x)|
```

The Metropolis-Hastings decision is conditioned on this selected atom instead of marginalizing it away before acceptance. The current canonical local-move
representation pairs every successful forward atom with one reverse-family atom, so the implemented reverse component is:

```text
q_reverse(c)(y -> x) = p(reverse(m) | y) / |S_reverse(m)(y)|
```

The reverse probability is always evaluated from the realized planned state `y`; it is never copied from `x`. The resulting pathwise correction is:

```text
log_q_ratio =
    log(p(reverse(m) | y)) - log(p(m | x))
  + log(|S_m(x)|) - log(|S_reverse(m)(y)|)
```

For equal move-family weights, this reduces to the familiar site-count correction:

```text
log_q_ratio = log(|S_forward(x)|) - log(|S_reverse(y)|)
```

Multiple auxiliary atoms may reach the same endpoint `y`, including the same flip exposed through the distinct `Move22` and `EdgeFlip` families. They remain
separate mixture components: each paired forward/reverse path satisfies detailed balance independently, and summing those balanced accepted fluxes preserves
detailed balance for the state transition. Their probabilities are therefore not aggregated into the per-plan correction. A future proposal workflow that
marginalizes paths before acceptance, or that cannot pair each sampled atom with one reverse atom, would instead need the full endpoint probability summed
over all contributing paths and explicit multiplicities.

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

Each fresh in-memory `MonteCarloStep` also exposes `ProposalKernelTelemetry` through `proposal_telemetry()`. Successful plans report selected and reverse
families, both family probabilities, both canonical offered-site counts, the independently inspectable family and site log-ratio components, and the complete
Hastings correction. Self-loops retain the selected family, pre-state probability, original denominator, and typed `CdtProposalPlanningOutcome`. Pair this
with `MonteCarloStepOutcome` to distinguish planning rejection, Metropolis rejection, and acceptance without parsing strings or accessing private state.

Policy/model persistence and external audit logging remain outside the timed transition. Callers that serialize checkpoints should therefore persist required
policy state and per-step policy telemetry in their experiment artifact layer; legacy-compatible serialized CDT step records do not embed the optional
in-memory proposal audit field.

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

`tests/proposal_policy.rs` independently reconstructs forward and reverse family/site probabilities for concrete uniform and unequal-weight transitions,
checks the resulting Metropolis-Hastings flux equality, and compares a fixed-policy checkpoint/resume run with its uninterrupted seeded trajectory. These are
pairwise kernel and same-build reproducibility checks; they do not by themselves establish ergodicity, mixing quality, or physical convergence.

The explicit-site model avoids dry-run cloning every candidate during site counting. Planned Metropolis acceptance creates one proposed-state clone; direct
move attempts create one rollback snapshot. Both paths pass caller-owned rollback into the primitive backend edits, so applying a selected site does not clone
the full backend again, including for composite mutations whose intermediate steps can partially commit. Standalone `TriangulationMut` calls retain their
backend-owned rollback guard because they have no enclosing CDT transaction.
