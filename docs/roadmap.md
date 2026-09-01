# Roadmap

This roadmap records likely directions for the `causal-triangulations` crate. It is not a stability promise; release scope depends on scientific need, API
maturity, and validation quality. Concrete implementation tasks should be tracked as GitHub issues; keep this document focused on release direction, candidate
themes, and non-goals.

## v0.1.0 CDT Foundations

The v0.1.0 foundation release centers on validated 1+1 CDT construction, including circular spatial slices with periodic time, volume-changing moves,
nonuniform initial spatial-vertex profiles, upstream-backed MCMC sampler mechanics, trace/summary exports, resumable checkpoints, and CI-aligned validation
tooling.

Current scope: higher-dimensional CDT, production volume fixing, automated λ scans, visualization/export workflows, and full ensemble-analysis tooling remain
roadmap work.

Completed foundation work:

- [x] Trait-based geometry backend boundary around the `delaunay` crate.
- [x] Validated open-boundary 1+1 strip constructors with spatial-interval slices, plus periodic-time constructors with circular S¹ spatial slices, including
  explicit per-slice spatial-vertex profiles. The latter have full spacetime topology S¹(time) × S¹(space), a torus.
- [x] Per-vertex foliation labels, adjacent-slice causality checks, topology validation, and strict Up/Down simplex classification.
- [x] Real 2D local CDT move kernels over checked Delaunay edit operations, with rollback or rejection on invariant failures.
- [x] Causality-filtering Delaunay construction path inspired by CDT++ ([#192](https://github.com/acgetchell/causal-triangulations/issues/192)), with repeated
  removal of vertices incident to non-strict causal simplices until the strict violation count converges to zero.
- [x] Upstream Level 1-4 embedded/non-overlapping validation for evolved states
  ([#195](https://github.com/acgetchell/causal-triangulations/issues/195)), separated from optional Level 5 Delaunay validation and mandatory strict causal
  profiles ([#196](https://github.com/acgetchell/causal-triangulations/issues/196)).
- [x] Planned-proposal Metropolis execution through `markov-chain-monte-carlo`, including proposal-site weighting and resumable chunked sweeps.
- [x] CLI support, trace/summary exports, checkpoints, and core observables for downstream analysis.
- [x] Repository validation loop covering Rust, Python support scripts, Semgrep rules, documentation, notebooks, examples, and benchmarks.
- [x] Release-readiness work for Rust 1.96.0, public doctest fallibility, CI/security alignment, and the first DOI-backed foundation release.

## v0.1.1 Maturity And API Polish

The post-v0.1.0 patch track can improve the current 1+1 surface without making physics-validation claims that belong to v0.2.0:

- Evaluate move-family weighting by available application sites while preserving the exact Hastings correction.
- Broaden per-kernel tests around spatial and temporal wrap-around simplices.
- Accept fixed triangle simplices directly in explicit-simplex generator APIs to remove per-triangle `Vec` adaptation.
- Add manual foliation assignment APIs with the same validation and synchronization guarantees as constructor-assigned labels.
- Add tutorial-style examples for open-time strips, periodic-time runs, observables, and interpreting Metropolis acceptance behavior.
- Retain dependency-gated geometry cleanup and typed-error redesign as API maturity work rather than evidence about the sampled CDT ensemble.

## v0.2.0 Validated 1+1 CDT Baseline

The v0.2.0 objective is the lowest-dimensional nontrivial physics baseline: **validated 1+1 CDT with circular spatial topology (S¹)**. This replaces the
previous plan to make 2+1 CDT with toroidal spatial slices the v0.2.0 target.

Topology terminology is part of the scientific contract:

- **Spatial topology:** each one-dimensional spatial slice is S¹.
- **Temporal boundary condition:** time may be open or periodic; the primary benchmark uses periodic time.
- **Full spacetime topology with periodic time:** S¹(time) × S¹(space), a torus.

Use “1+1 CDT with circular spatial topology (S¹)” for the model. Do not call it “spherical 1+1 CDT”: that wording can confuse the topology of a
spatial slice with the topology of the complete spacetime.

The release gate is tracked by the exact transfer-matrix validation issue
([#238](https://github.com/acgetchell/causal-triangulations/issues/238)). Its release-blocking prerequisites are spatial-loop measurement and artifacts
([#237](https://github.com/acgetchell/causal-triangulations/issues/237)), soft volume fixing
([#142](https://github.com/acgetchell/causal-triangulations/issues/142)), autocorrelation analysis
([#165](https://github.com/acgetchell/causal-triangulations/issues/165)), and ESS/R-hat diagnostics
([#166](https://github.com/acgetchell/causal-triangulations/issues/166)). Coupling-sensitive grand-canonical validation
([#143](https://github.com/acgetchell/causal-triangulations/issues/143)) and public topology/boundary API separation
([#236](https://github.com/acgetchell/causal-triangulations/issues/236)) are v0.2.x follow-ups.

### Validation hierarchy

The project distinguishes four different claims:

1. **Structural validation.** Existing validators and tests establish combinatorial, manifold, topology, foliation, causality, embedding, rollback, and
   geometric invariants. These checks prove that states are admissible CDT triangulations; they do not prove that the Markov chain samples the intended
   probability distribution.
2. **Analytic physics validation — v0.2.0.** The conventional 1+1 sampler must reproduce an exact CDT distribution within predeclared statistical tolerances.
   This is an implementation-independent test of the sampled ensemble.
3. **1+1 follow-up validation — v0.2.x.** After the fixed-volume analytic result is established, validate the cosmological-coupling normalization in the
   grand-canonical ensemble, separate public spatial-topology and temporal-boundary contracts, and compare the conventional sampler with Brunekreef's
   `2d-cdt`. This broadens the evidence without making the reference code the definition of the ensemble.
4. **2+1 reference-implementation validation — v0.3.x.** Conventional simulations with S² spatial slices and periodic time must be compared with
   Brunekreef's `3d-cdt` implementation under matched conventions.

### Primary exact benchmark

The strongest practical benchmark is the finite-lattice spatial-loop-length distribution derived from the exact one-step transfer matrix. In the convention
with one marked entrance loop, Ambjørn and Loll give

```text
M[l, l'](g) = g^(l + l') binomial(l + l' - 1, l - 1),  g = exp(-lambda_triangle).
```

The primary source is J. Ambjørn and R. Loll, “Non-perturbative Lorentzian quantum gravity, causality and topology change,” Nuclear Physics B 536 (1998),
[doi:10.1016/S0550-3213(98)00692-0](https://doi.org/10.1016/S0550-3213(98)00692-0). The validation oracle must document and independently test the exact
rooting, marking, labeling, and symmetry-factor mapping used by this crate before comparing probabilities.

For periodic time with `T` slices, the finite transfer-matrix partition function is `Z_T(g) = Tr(M(g)^T)`. At fixed triangle count `N2`, the exact probability
of a periodic spatial-volume profile `l = (l_0, ..., l_(T-1))` is proportional to

```text
indicator(2 sum_t l_t = N2) product_t M[l_t, l_(t+1)],  l_T = l_0,
```

restricted to the implementation's admissible minimum loop length. Here `l_t` is the number of spatial links in the S¹ slice and is the one-dimensional
spatial volume. It is not the existing slab-triangle profile `N2(t)`, which counts two-dimensional simplices between adjacent slices.

This exact finite-state distribution is preferable to a continuum-limit curve as the primary release gate: it is directly measurable, exercises the full
statistical ensemble, can be enumerated at several lattice sizes, and does not require extrapolating away finite-size effects. Secondary cross-checks should
include the exact critical vertex-order distribution `p(j) = (j - 3) / 2^(j - 2)` for `j >= 4` from J. Ambjørn, K. N. Anagnostopoulos, and R. Loll,
“A new perspective on matter coupling in 2d quantum gravity,” Physical Review D 60 (1999),
[doi:10.1103/PhysRevD.60.104035](https://doi.org/10.1103/PhysRevD.60.104035), after verifying that the observable and ensemble conventions match.
Grand-canonical volume probabilities derived from the same transfer matrix provide a coupling-sensitive secondary benchmark.
That comparison is tracked by [#143](https://github.com/acgetchell/causal-triangulations/issues/143) for v0.2.x because the fixed-volume primary distribution
cannot test an action contribution that is constant within each fixed-`N2` sector.

### Simulation and measurement contract

The reproducible benchmark should use:

- periodic time, circular S¹ spatial slices, MCMC temperature 1, and the conventional reversible move policy;
- fixed-volume conditions at `T = 4` with `N2 = 32, 40, 48` and at `T = 8` with `N2 = 64`, subject to a preflight ergodicity/reachability check for the
  implementation's minimum slice length;
- at least eight independent seeded chains per condition, including both flat and strongly nonuniform initial spatial profiles;
- a declared burn-in rule and state-independent measurement cadence;
- an opt-in soft volume-fixing action where needed for efficient sampling, with samples conditioned offline on the target `N2`; the fixing term and the
  conditioning rule must be reported because volume fixing changes the marginal ensemble;
- an explicit limitation stating that this fixed-volume gate does not validate the cosmological-coupling normalization, which is tested at multiple
  supercritical couplings by [#143](https://github.com/acgetchell/causal-triangulations/issues/143) in v0.2.x; and
- machine-readable configuration, seed, starting profile, measurement, diagnostic, exact-oracle, and comparison artifacts.

Measurement infrastructure must export the per-slice spatial loop lengths `l_t` at the measurement cadence, not infer them from slab-triangle counts. Exact
enumeration and synthetic draws from the oracle must test the comparison harness independently of the CDT sampler.

### v0.2.0 release criteria

The conventional sampler is declared analytically validated only when all of the following hold for the predeclared benchmark matrix:

- every chain passes structural validation and the combined chains satisfy split rank-normalized `R-hat <= 1.01` for the tested scalar summaries;
- uncertainty and goodness-of-fit calculations account for autocorrelation through integrated autocorrelation time or effective sample size, and a condition
  is reported as inconclusive rather than passing when effective information is insufficient;
- the 95% upper confidence bound on total-variation distance from the exact fixed-volume profile distribution is at most `0.05`;
- calibrated multinomial or likelihood-ratio goodness-of-fit tests do not reject the exact distribution at familywise `alpha = 0.05`, with multiple conditions
  corrected by the Holm procedure;
- spatial-volume means agree with the exact values within 1%, and nonzero variances and covariances agree within 5%, with simultaneous confidence intervals
  containing the exact values;
- a deliberately biased synthetic sampler is rejected by the same harness, while exact synthetic draws pass at the calibrated rate; and
- a clean rerun from the recorded commands and seeds regenerates the validation report and hashes or versions every input artifact.

Because the primary oracle is exact at the same finite `T`, `N2`, and minimum loop length as the simulation, unexplained disagreement is not a generic
finite-size or discretization effect. Such effects are expected only when comparing to continuum-limit or large-volume asymptotics and must be reported
separately.

Passing structural tests, producing plausible plots, obtaining a high acceptance rate, or matching only one mean is not sufficient for v0.2.0. The release
claim is that `causal-triangulations` samples the expected fixed-volume conditional 1+1 CDT ensemble at the declared finite conditions. The broader
coupling-sensitive claim belongs to v0.2.x.

### Architecture path

The v0.2.0 design should prepare the direct topology progression without prematurely implementing the complete 2+1 model:

```text
1+1: S¹ spatial slices
  ↓
2+1: S² spatial slices with periodic time — Brunekreef 3d-cdt comparison
  ↓
3+1: S³ spatial slices
```

Represent spatial topology separately from temporal boundary conditions and derive the full spacetime topology from both. Keep actions, simplex classes, move
sets, and volume observables dimension-qualified. Prefer concrete per-dimension domain types behind narrow shared interfaces over a speculative generic
higher-dimensional rewrite. Preserve the existing CDT-to-geometry backend boundary and delegate generic sampler mechanics and diagnostics to
`markov-chain-monte-carlo`.

## v0.2.x 1+1 Follow-Up And Brunekreef Reference Validation

The v0.2.x series broadens the 1+1 evidence and prepares the higher-dimensional topology API after the narrow v0.2.0 fixed-volume gate passes:

- [#143](https://github.com/acgetchell/causal-triangulations/issues/143) validates the cosmological-coupling convention against exact grand-canonical volume
  probabilities at multiple supercritical couplings.
- [#236](https://github.com/acgetchell/causal-triangulations/issues/236) separates spatial topology from temporal boundary conditions in public configuration,
  metadata, serialization, and validation dispatch before the 2+1 topology API is fixed.
- [#240](https://github.com/acgetchell/causal-triangulations/issues/240) compares the analytically validated conventional sampler with Brunekreef's
  [`JorenB/2d-cdt`](https://github.com/JorenB/2d-cdt) under the same periodic 1+1 topology: circular S¹ spatial slices and full spacetime topology
  S¹(time) × S¹(space).

Issues #143 and #236 are blocked by [#238](https://github.com/acgetchell/causal-triangulations/issues/238). Both then block #240, making the Brunekreef
comparison the v0.2.x validation capstone. Issue #236 also blocks the first 2+1 topology foundation in #241.

Both implementations must pass the exact finite transfer-matrix oracle independently before pairwise agreement is interpreted. The reference comparison must
also distinguish Brunekreef's unmodified return-to-target measurement protocol from an ensemble-safe, state-independent-cadence instrumentation path. The
analytic result remains authoritative when the two implementations or measurement protocols disagree.

This release validates the external comparison harness, convention matrix, negative controls, and reproducible artifact workflow in the exactly solvable
dimension before they are reused for 2+1 CDT.

## v0.3.x 2+1 S² Development And Brunekreef Validation

The v0.3.x series develops conventional 2+1 CDT with compact spherical S² spatial slices and periodic time. The full spacetime topology is
S¹(time) × S²(space), matching Brunekreef's `3d-cdt` implementation. The primary implementation references are
[`JorenB/3d-cdt`](https://github.com/JorenB/3d-cdt) and J. Brunekreef, R. Loll, and A. Görlich, “Simulating CDT quantum gravity,” Computer Physics
Communications 300 (2024), [doi:10.1016/j.cpc.2024.109170](https://doi.org/10.1016/j.cpc.2024.109170).

### v0.3.0 foundation

The initial v0.3.0 release establishes the dimension-qualified scientific foundation without claiming a complete sampler or reference comparison. The public
topology/boundary separation in [#236](https://github.com/acgetchell/causal-triangulations/issues/236) must land before its 2+1 topology contract:

- [#241](https://github.com/acgetchell/causal-triangulations/issues/241) adds the 2+1 S² domain model, periodic foliation, manifold/topology checks, and
  explicit `(3,1)`, `(1,3)`, and `(2,2)` causal tetrahedron types.
- [#242](https://github.com/acgetchell/causal-triangulations/issues/242) adds the Brunekreef-compatible Euclidean Regge action
  `S_E = -k_0 N_0 + k_3 N_3`, with action deltas and the labeled-measure/proposal factors kept separate.

### Subsequent v0.3.x conventional-sampler series

Later v0.3.x releases complete the conventional simulation and evidence surface in dependency order:

- [#243](https://github.com/acgetchell/causal-triangulations/issues/243) implements the reversible `(2,6)/(6,2)`, `(4,4)`, and `(2,3)/(3,2)` move families
  with exact forward/reverse proposal accounting, typed self-loops, rollback, and shared conventional/external family-policy execution.
- [#244](https://github.com/acgetchell/causal-triangulations/issues/244) adds explicit `N_3`/`N_31` volume conventions, quadratic volume fixing, staged
  coupling tuning, and raw volume diagnostics.
- [#245](https://github.com/acgetchell/causal-triangulations/issues/245) adds the per-slice spatial two-volume profile `N_2^SL(t)`, causal-simplex counts,
  action components, proposal diagnostics, and reproducible comparison artifacts.
- [#246](https://github.com/acgetchell/causal-triangulations/issues/246) extends the invariant-safe family-policy, typed telemetry, chunking, timing, and
  versioned checkpoint contracts needed for external orchestration.

Issue #246 is external-policy integration infrastructure. `causal-triangulations` owns valid states, proposal support, action and volume deltas, checked
mutation, physical observables, and CDT-owned checkpoints. `markov-chain-monte-carlo` owns generic sampler mechanics and diagnostics. Downstream clients own
their policy implementations, artifacts, orchestration, and analysis. The contingent site-level contract in
[#232](https://github.com/acgetchell/causal-triangulations/issues/232) remains outside this series unless downstream family-only evidence justifies it.

### v0.3.x validation capstone

The final feature in the series is [#239](https://github.com/acgetchell/causal-triangulations/issues/239), the quantitative comparison with Brunekreef's
`3d-cdt`. It is blocked by the v0.2.x `2d-cdt` rehearsal and the 2+1 prerequisites above; it does not reimplement their scope.

The conventional Metropolis-Hastings comparison should match, as closely as practical:

- spatial topology and temporal periodicity;
- action normalization and coupling conventions;
- allowed 2+1 CDT simplex types and local move set;
- target volume, chosen volume observable, and volume-fixing action;
- initial triangulation and randomization procedure;
- thermalization rule, independent-chain strategy, and measurement cadence; and
- reported volume profiles, simplex counts, acceptance statistics, and other observables available from both implementations.

Every unavoidable convention or implementation difference must be recorded before interpreting the comparison. The capstone requires quantitative uncertainty,
convergence, and reproducibility evidence, not visual similarity. Its purpose is an implementation-to-implementation cross-check of the conventional ensemble.

## Alternative And Later Topology Tracks

- Keep 2+1 CDT with toroidal T² spatial slices ([#144](https://github.com/acgetchell/causal-triangulations/issues/144)) as an optional later topology track.
  It is scientifically valid, but it is not on the shortest validation path to Brunekreef's S² implementation and should not block v0.2.x or v0.3.x.
- Advance to 3+1 CDT with S³ spatial slices only after the 2+1 S² reference comparison establishes the higher-dimensional architecture and validation
  workflow.
- Treat toroidal T³ spatial slices and other boundary-condition families as separate tracks with their own topology, action, move-set, and validation
  contracts.
- Add periodic-time variants only where the full spacetime topology and backend invariants are explicit.

## Later Distribution Ergonomics

- Ship prebuilt `cdt` release binaries
  ([#169](https://github.com/acgetchell/causal-triangulations/issues/169)). This should make the command-line tool easier to try once the notebook and local
  source-build path are stable, while keeping `cargo install` and local release builds supported.

## Observables and Dual Geometry

CDT observables should remain user-facing analysis APIs and should grow in lockstep with validation:

- Extend slab-volume observables from 1+1 triangle profiles to spatial-simplex profiles in 2+1 and 3+1 dimensions; the first 2+1 comparison surface is tracked
  by [#245](https://github.com/acgetchell/causal-triangulations/issues/245)
- Add geodesic-distance distributions, shell-volume curves, two-point functions, and finite-size scaling helpers
- Add curvature-oriented Regge observables when the local simplex data is sufficiently validated
- Keep the current finite-window effective Hausdorff and spectral estimators available on combinatorial dual adjacency graphs
- Reuse Voronoi tessellation support from the `delaunay` crate when it lands, so observables can opt into full dual/Voronoi regions rather than rebuilding only
  face- or simplex-adjacency graphs
- Preserve a clear distinction between combinatorial dual graphs, geometric Voronoi tessellations, and visualization/export representations

## Visualization and Workflow Support

Visualization should help inspect CDT structure without turning this crate into a plotting package:

- Export mesh and graph data in common interchange formats for external visualization tools
- Provide lightweight examples for rendering foliated triangulations, slice volumes, dual graphs, and sampled histories
- Add optional diagnostic outputs for move acceptance, topology preservation, volume evolution, and diffusion-return curves
- Keep publication-quality plotting and broad downstream statistical analysis in companion tools unless needed for core CDT validation

## Longer-Term Ideas

Exploratory directions:

- Support additional topology and boundary-condition families when geometry backend invariants can validate them cleanly
- Add ensemble-analysis helpers for CDT research workflows beyond the core estimators
- Integrate parallel-chain workflows while keeping random-stream management explicit
- Explore alternative discrete gravity actions once the Regge-action path is well covered

## Non-Goals

The crate should remain a focused CDT physics library layered over a geometry backend. General-purpose mesh editing, a replacement Delaunay implementation,
publication plotting, broad MCMC diagnostics, and domain-specific downstream analyses belong in separate crates or tools unless they are needed to validate core
CDT behavior.
