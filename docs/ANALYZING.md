# Analyzing Periodic 1+1 CDT Chains

Measure circular spatial loops and analyze complete cadence-aligned chains through `prelude::observables`. Generic autocorrelation, binning,
ESS, MCSE, and R-hat calculations delegate to the published `markov-chain-monte-carlo` dependency.

## Contents

- [Volume fixing](#volume-fixing)
- [Spatial loop measurements](#spatial-loop-measurements)
- [Chain artifacts](#chain-artifacts)
- [Diagnostic policies](#diagnostic-policies)
- [Timing](#timing)
- [Runnable example](#runnable-example)
- [Evidence boundaries](#evidence-boundaries)

## Volume fixing

The default ensemble remains bare and volume changing. Opt in explicitly with both CLI arguments:

```bash
just run -- --topology toroidal --vertices 16 --timeslices 4 --simulate --seed 41 \
  --steps 1000 --thermalization-steps 100 --measurement-frequency 10 \
  --target-triangles 32 --volume-fixing-strength 0.1 --output-json target/fixed-summary.json
```

The equivalent action configuration uses `VolumeFixing::quadratic_triangles(target, strength)` and
`ActionConfig::with_volume_fixing(Some(fixing))`. `CdtConfig::volume_fixing` carries the same validated value into CLI-backed runs.

This initial mode supports periodic 1+1 CDT only. The target is positive, even, and at least `6 * timeslices`; strength is finite and nonnegative.
Setup checks that the combined action divided by temperature stays finite over the supported count range. It also checks the sampler’s evaluation order,
which divides the physical and fixing terms by temperature separately and then adds them.

The target is

```text
S_combined = S_physical + epsilon * (N2 - target)^2
log weight = -S_combined / temperature
Delta S_fix = epsilon * d * (2 * (N2 - target) + d), d = Delta N2
```

CDT supplies physical and fixing components to upstream `AdditiveTarget` and computes the marginal endpoint probabilities. The MCMC backend uses those
probabilities in the Hastings correction and owns the acceptance draw.
The quadratic bias and its move delta scale exact integer count expressions and round once to binary64. Volume-preserving moves have zero fixing delta.
No hard clamp, extra rejection rule, or state-dependent measurement cadence is introduced.
Measurements, summary JSON, and scalar trace CSV distinguish `physical_action`, `fixing_action`, and the combined `action`. The new CSV component columns
follow the existing slab-profile columns, preserving earlier column positions.

Version 5 checkpoints preserve bare and volume-fixed runs using the corrected embedded proposal kernel. Validation-chain export checks the declared start
against the recorded trajectory.
Checkpoints retain the bias configuration and action components. Changing target, strength, or enabled/disabled mode cannot silently resume the same chain.
JSON float round trips preserve parameter bits. Exact geometry snapshots preserve the inputs to the
[Hilbert proposal-site order](scientific-basis.md#hilbert-order-for-embedded-proposal-sites), so rebuilding incidence tables preserves site ordinals.
Exact continuation covers RNG streams, counters, measurements, and canonical geometry. New backend UUIDv4 identities are independent of the sampler seed.
Version 5 rejects earlier checkpoint formats. Keep the original executable and lockfile to reproduce an earlier seeded study.

## Spatial loop measurements

`spatial_loop_lengths(&triangulation)` returns spacelike-link counts in time-label order for circular S¹ slices with periodic time. It first uses the
existing foliation validator to establish one connected degree-two ring per slice, so the vertex/link identity is justified. This domain check does not
replace upstream structural geometry validation. Open interval slices and missing/stale foliation produce explicit errors.

Production constructors and finalized moves already establish that invariant. Their measurements reuse current slice counts and immutable snapshots,
including across unchanged states and checkpoint continuation. Every periodic measurement satisfies

```text
N2 = 2 * sum_t spatial_loop_lengths[t]
slab_triangle_profile[t] = spatial_loop_lengths[t] + spatial_loop_lengths[(t + 1) mod T]
```

`Measurement::spatial_loop_lengths()` is optional for compatibility with open or older measurements. It is required by the periodic validation artifact.
The existing slab-triangle profile remains a separate observable.

## Chain artifacts

`ValidationChain::from_results(run_id, chain_id, &configuration, &results)` captures canonical scheduled measurements plus initialization metadata.
`write_json` exports schema version 2 for the corrected embedded proposal kernel. `read_json` rejects schema 1 because those chains used different sampling
semantics. Keep the producing revision with older data when interpreting it.
This export takes embedded `SimulationResultsBackend`. The intrinsic periodic kernel exposes its live profile through
`IntrinsicCdtTriangulation::spatial_loop_lengths()`.

The JSON contains:

- independent run/chain identity, an explicit seed, producer crate version, and schema version;
- explicit `S1` spatial topology, periodic time, minimum loop length, and the edge-count physical action convention;
- full configuration, including the initial profile or regular-profile inputs, time extent, temperature, bias, warmup, and cadence;
- every scheduled post-warmup measurement, with its original proposal step, counts, both action components, loop lengths, and slab profile.

Loading rejects unknown schemas, invalid configurations, missing profiles, count/action inconsistencies, reordered rows, missing cadence rows, and
truncated JSON. Chunk/checkpoint boundaries do not create independent chains. Independent-chain analysis rejects repeated identities or RNG seeds and
requires matching physical conditions, warmup, cadence, producer version, and sample counts; initial profiles and independent seeds may differ.

`at_exact_triangles(target)` selects exact target-volume rows offline and retains their original steps. It never includes neighboring volumes.
A bias depending only on total N2 is constant within each exact-N2 sector: its factor cancels between the conditional distribution's numerator and
normalizer. This establishes why conditioning removes this bias mathematically; agreement of the implemented chain with that conditional ensemble still
requires the independent oracle in #238.

Filtered target-volume visits generally have irregular spacing. Do not feed them to a regular-cadence ACF estimator as if they were adjacent draws.
The adapter exposes full-cadence volume, profile, and joint target/profile indicators for the dependence-aware ratio analysis planned in #238.

## Diagnostic policies

`analyze_cdt_chains` analyzes one predeclared `CdtDiagnosticObservable` under a `CdtDiagnosticPolicy`. Select actions, N2, slice loop lengths,
maximum loop length, loop products, or exact profile/volume indicators. Errors retain the observable's original units.

Each chain retains its own ACF and integrated-time window, and its complete upstream power-of-two binning hierarchy. Binning reports completed-block
counts, block-mean variance, and blocked mean errors, excluding incomplete trailing blocks. Selection requires the declared completed-block minimum and
several consecutive error levels within the declared relative tolerance. For consecutive reported errors `a` and `b`, the inclusive test is
`abs(a - b) <= tolerance * max(abs(a), abs(b))`. The comparison resolves rounding ambiguity using the exact values of the binary64 inputs.
The coarsest level alone never proves stabilization. Constant traces may have valid zero binning errors while their mixing diagnostics remain unavailable.

Multi-chain reports distinguish raw-mean, rank-normalized bulk, and both tail ESS values, including original/retained counts, relative ESS, and upstream
regularization. Diagnostic report version 2 records each quantile's indicator ESS alongside its MCSE. A tied quantile can retain a valid indicator ESS
when its uncertainty interval collapses. A resolved quantile carries its type-7 estimate, interval, and standard error. Mean MCSE uses raw-scale
mean ESS. The optional timing record groups elapsed seconds with the timed chain and draw counts.

Reports preserve both rank-normalized split location R-hat and folded scale R-hat. The combined maximum is unavailable if either component is unavailable.
An available combined value above 1.01 yields `not-converged`. Missing required estimators, too few chains, insufficient ESS, excessive required MCSE,
or required unresolved binning yield `inconclusive`. Otherwise the verdict is `sufficient-information`, which is not a physics-validation pass.

There is no universal default ESS or MCSE threshold. Declare thresholds, binning policy, lag cap, required quantile probabilities, and chain count before
examining validation results. Structurally constant summaries need a predeclared exclusion policy in the eventual oracle workflow; their raw unavailable
diagnostics must remain recorded. The v0.2.0 matrix requires at least eight independent chains per condition, including both start classes.

## Timing

ESS rate is supplementary and does not establish target correctness. Supply upstream `DiagnosticTiming` only for the actual post-warmup workload
represented by the analyzed draws. Record exactly the transitions that generated those measurements, excluding setup, serialization, hashing, and plotting.
Parallel chains use actual elapsed wall time. The adapter records timing counts and duration and rejects mismatched counts as an unavailable rate.
It does not infer production-only time from a whole-run duration, or reuse timings after selecting a subset. Missing timing remains explicitly unavailable.

## Runnable example

```bash
cargo run --release --example validation_diagnostics -- target/validation-diagnostics
```

The [example](../examples/validation_diagnostics.rs) runs one flat and one nonuniform short chain, writes and reloads separate JSON artifacts, and writes
`diagnostics.json`. Its small workloads demonstrate the interface and deliberately do not satisfy the eight-chain production policy. They establish
no mixing or ensemble-agreement claim. The external bundle/process contract will be frozen separately in #276.

## Evidence boundaries

The focused integration tests independently count spacelike edges, test the action/bias/temperature boundary, retain rejected states through complete
cadence, exercise fixed and unfixed production fixtures, and compare serialized continuation with uninterrupted seeded runs. Synthetic controls cover
independence, positive correlation, location separation, scale separation, tied data, and explicit unavailable outcomes. Generic estimator mathematics
remains tested upstream.

[The exact comparison harness](https://github.com/acgetchell/causal-triangulations/issues/238) owns the four-condition, eight-chain-per-condition
benchmark and calibrated total-variation and goodness-of-fit calculations. The [release tracker](https://github.com/acgetchell/causal-triangulations/issues/335)
owns the resulting evidence for #165 and #166. See the [roadmap](roadmap.md#v020-validated-11-cdt-baseline) and [scientific basis](scientific-basis.md).

The geometry adapter uses published Delaunay 0.8.5. Periodic containment, local edits, realization
validation, and exact snapshot restoration remain delegated through the geometry adapter. Raw TDS Delaunay reports are Euclidean; periodic Level 5
checks retain the topology-aware upstream promotion path. Preserve the resolved Cargo lockfile with validation evidence.

The filtered-strip initializer ranks surplus-removal candidates across all invalid faces by disagreement between the time label and embedded time.
When local cavity deletion is unavailable, its geometry adapter rebuilds the unchanged surviving point set through upstream Delaunay construction.
This construction fallback preserves vertex identities, coordinate bits, labels, and validation policy; simulation proposals retain their local edit
and rollback contracts.
