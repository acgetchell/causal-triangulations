# Finite-volume validation of the intrinsic 1+1 CDT sampler

Does the sampler assign the right probability to each sequence of spatial loop lengths? This benchmark compares fresh Rust chains with
the finite-lattice distribution derived from Ambjørn and Loll's 1998 transfer matrix. Each of four tori has its own exact probability law.

## Results

### Independent production confirmation

**All four fixed-volume conditions passed.** After fixing the measurement budgets, calibration, and joint decision rule, we ran the 32 reserved
production chains once. These chains alone supplied the verdict. They recorded 17,203,373 target-volume visits in 72,351,744 scheduled measurements.
The [final confirmation report](../benches/finite_volume/final-confirmation-report.json) records the seed ranges, executable and input hashes,
chain diagnostics, and outcomes under the [frozen rule](#frozen-rule-for-the-final-confirmation).

| Slices | Triangles | Target visits | Observed TV | 95% TV upper bound | Raw / calibrated Holm p | Moments passing | Result |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 4 | 32 | 546,196 | 0.00400 | 0.00946 | 1.000 / 1.000 | 14 / 14 | Passed |
| 4 | 40 | 269,338 | 0.01223 | 0.02656 | 1.000 / 1.000 | 14 / 14 | Passed |
| 4 | 48 | 533,905 | 0.01464 | 0.02936 | 1.000 / 1.000 | 14 / 14 | Passed |
| 8 | 64 | 15,853,934 | 0.00931 | 0.01876 | 1.000 / 0.986 | 44 / 44 | Passed |

All 86 labeled moments met their point tolerances and their four-torus simultaneous intervals covered the exact values. Each calibrated 95% upper
bound on total-variation distance (TV) was below 0.05. The scalar chain diagnostics and both synthetic-control panels passed. At eight slices and
64 triangles the covariance of slices 2 and 3 was −0.039590, with simultaneous interval [−0.041699, −0.037481]. The exact value was −0.039838,
giving a relative error of 0.62%. These results support agreement with the exact conditional loop-length distribution at the four tested tori.

#### What changed after the first confirmation

The first independent confirmation failed its joint rule. Its only miss was the same
64-triangle covariance, measured at −0.037401 with a 6.12% error against the 5% tolerance. At that budget,
43 of 100 [exact-law cohorts](#how-often-do-exact-law-samples-miss-the-moment-rule) missed at least one covariance tolerance, compared with
1 of 100 at four times the budget. Before using fresh production seeds, we increased the 64-triangle budget from 2,097,152 to 8,388,608 measurements
per chain and fixed the [simultaneous interval rule across all four tori](#simultaneous-coverage-across-four-tori).
Both confirmations used the same point tolerances. The controls show that sampling variation can produce such a miss. Its individual cause remains uncertain.

### Development acquisition

The intrinsic sampler passed the four tested conditions on the development acquisition. Its 32 chains produced 22,020,096 scheduled measurements and
5,309,273 target-volume visits. All 86 labeled moments satisfied their point tolerances and simultaneous interval criteria.
The calibrated upper bound on total-variation distance was below 0.05 at every size. The [archived development report](#retained-evidence)
records the measurements, uncertainty estimates, control decisions and input hashes.

| Slices | Triangles | Target visits | Observed TV | 95% TV upper bound | Raw / calibrated Holm p | Null rejections per 100, original / confirmation | Result |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 4 | 32 | 545,321 | 0.00430 | 0.00971 | 0.7035 / 0.747 | 2 / 5 | Passed |
| 4 | 40 | 270,359 | 0.01263 | 0.02698 | 0.705 / 0.755 | 4 / 3 | Passed |
| 4 | 48 | 532,674 | 0.01342 | 0.02810 | 0.705 / 0.755 | 3 / 6 | Passed |
| 8 | 64 | 3,960,919 | 0.01888 | 0.03781 | 0.256 / 0.196 | 7 / 7 | Passed |

Each holdout panel detected all 400 biased datasets and rejected four of 100 exact-law families after Holm adjustment.
The check flags 12 or more false rejections in a panel of 100. The reported comparison uses the same 32 production chains used during
calibration development. The holdout panels test the fitted calibration on synthetic data. Each independent confirmation used its own fresh sampler chains.

#### Chain diagnostics and moments

| Slices | Triangles | Largest R-hat | Smallest bulk ESS¹ | Max mean error | Max variance or covariance error |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 4 | 32 | 1.000069 | 148,405 | 0.03% | 1.75% |
| 4 | 40 | 1.000252 | 35,964 | 0.38% | 2.31% |
| 4 | 48 | 1.000161 | 47,447 | 0.14% | 3.26% |
| 8 | 64 | 1.000022 | 521,512 | 0.10% | 4.08% |

¹ Bulk effective sample sizes are rounded down.

For labeled slices 2 and 3 at 32 triangles, the exact covariance is −43,662/159,757 (−0.27330). The measured covariance was −0.27549,
a relative error of 0.80%. Its simultaneous 95% interval was [−0.28117, −0.26982].

At 48 triangles, the profile (3, 3, 3, 15) had 1.49 expected visits and zero observed. Its probability mass is included in the TV calculation.

## Exact reference distribution

**Ambjørn and Loll supply the analytic formula. We compute the reference numbers.** Equation (9) marks the entrance loop, and equations
(10)–(11) compose successive strips. Equation (17) gives the one-step generating function for an entrance of length $a$ and an exit of length $b$.
[Original paper, §2, eq. (17)](https://arxiv.org/pdf/hep-th/9805108#page=6)

$$
G(x,y;g;1)=\frac{g^2xy}{(1-gx)(1-gx-gy)},\qquad
[x^a y^b]\,G(x,y;g;1)=g^{a+b}\binom{a+b-1}{a-1}.
$$

The marked entrance fixes the first up triangle. Choosing the positions of the other $a-1$ up triangles gives the binomial coefficient.
For example, a strip from length 3 to 4 contributes $15g^7$. We compose $T$ strips around periodic time on an oriented torus, keeping slice zero
distinguished. Write $\ell_t$ for the number of spacelike links on slice $t$. The allowed profiles satisfy $\ell_T=\ell_0$, $\ell_t\ge 3$,
and $2\sum_t\ell_t=N_2$. Every profile then carries the same
factor $g^{N_2}$ and the same quadratic volume-fixing weight. Both cancel when we normalize over those labeled profiles.

$$
w(\ell)=\prod_{t=0}^{T-1}\binom{\ell_t+\ell_{t+1}-1}{\ell_t-1},\qquad
Z_{T,N_2}=\sum_{\ell}w(\ell),\qquad P(\ell\mid T,N_2)=\frac{w(\ell)}{Z_{T,N_2}}.
$$

The production target corrects for its computational origins on the slices. Varying-volume coupling normalization, continuum scaling
and topology change require other observables.
See [the sampler's marking convention](scientific-basis.md#intrinsic-periodic-cdt-measure) and
[the varying-volume comparison](https://github.com/acgetchell/causal-triangulations/issues/143).

The [Rust oracle](../benches/finite_volume/oracle.rs) enumerates integer compositions and computes each weight using binomial coefficients.
A second calculation recursively counts strip words and extracts the fixed-volume coefficient of the periodic transfer-matrix trace.
The two calculations must agree exactly on each profile weight and on the integer normalizer. The benchmark computes labeled means and
population covariances as exact rational numbers and compares relative point errors before rounding for display.

For eight slices and 64 triangles, an independent coefficient extraction from equation (17) gives
$Z_{8,64}=4{,}207{,}880{,}073{,}023{,}875$. Inserting $\ell_2\ell_3$ in the periodic trace gives
$67{,}158{,}448{,}320{,}793{,}750$. Each labeled slice has mean $64/(2\cdot8)=4$ by time-translation symmetry. Therefore

$$
\operatorname{Cov}(\ell_2,\ell_3)=\frac{67{,}158{,}448{,}320{,}793{,}750}{4{,}207{,}880{,}073{,}023{,}875}-4^2
=-\frac{1{,}341{,}062{,}780{,}706}{33{,}663{,}040{,}584{,}191}\approx -0.03983784.
$$

The measured values come from the Rust chains. Each target-volume visit contributes one observation to the empirical covariance.
The [known-answer tests](../tests/finite_volume/validation.rs) check the support, normalizer and labeled covariance at all four sizes.

## Sampling protocol

The development run and first confirmation used the original measurement budgets below. The
[final confirmation](#frozen-rule-for-the-final-confirmation) used a third set of seeds and a larger budget at 64 triangles.

| Time slices | Triangles | Admissible profiles | Exact normalizer | Development seeds | First confirmation seeds | Cadence rows per chain |
| ---: | ---: | ---: | ---: | --- | --- | ---: |
| 4 | 32 | 35 | 27,957,475 | 320101–320108 | 1000320101–1000320108 | 262,144 |
| 4 | 40 | 165 | 11,608,941,420 | 400101–400108 | 1000400101–1000400108 | 131,072 |
| 4 | 48 | 455 | 3,312,100,041,142 | 480101–480108 | 1000480101–1000480108 | 262,144 |
| 8 | 64 | 6,435 | 4,207,880,073,023,875 | 640101–640108 | 1000640101–1000640108 | 2,097,152 |

The [acquisition](../benches/finite_volume/acquire.rs) constructs a reversible insertion path from the minimum torus to each admissible profile.
It uses positive forward and reverse transition probabilities from the production proposal. This establishes reachability of the measured profiles.

Each row of the table represents eight independently seeded Xoshiro256++ chains. Four start flat. The other four start with all excess
length on one slice, rotating the peak through slices 0, 1, 2, 3 at four slices or 0, 2, 4, 6 at eight slices. The production chains use
uniform move-family selection, unit temperature, the critical action and quadratic total-triangle fixing with strength 0.05.

Each chain discards 16,384 proposals and records one measurement every 16 proposals thereafter, including holds and excursions from the target.
The benchmark verifies the cadence and the closed-torus identities `N0 = sum(l)`, `N1 = 3 N0`, `N2 = 2 N0` on every row before conditioning
on the target triangle count. The scalar diagnostics use the full cadence.

### Independent confirmation protocol

The [archived first-confirmation protocol](#retained-evidence) governed the first independent production confirmation under the
within-torus bands. It specified 32 production chains, the code and executable hashes, and the joint decision rule. Each chain's seed was its
development counterpart plus one billion. The [final confirmation](#frozen-rule-for-the-final-confirmation) used a separate cohort.
The flat and peaked starts, burn-in, cadence, chain lengths and sampler settings are fixed for each torus. The reachability, bootstrap
and synthetic-control streams are fixed across acquisitions. The scorer verifies a SHA-256 digest of each complete fitted calibration,
including both holdout panels, before applying it to the measurements.

The first confirmation scored its 32 chains alone. Its protocol required all four conditions and both family-level control checks to pass
under the within-torus bands. The first complete acquisition supplied the reported decision.

Before collecting the first confirmation chains, the scorer replayed the development acquisition. All four condition detail files matched the originals
byte for byte, including every fitted calibration, its holdouts and the trace hashes. The joint report reproduced every scientific field.
Only the executable and the two modified scoring source hashes differed. This replay and the first confirmation used the same executable.

## Uncertainty and acceptance

| Quantity | Predeclared criterion |
| --- | --- |
| Information in total volume and every slice length | Combined rank-normalized/folded split R-hat ≤ 1.01 and bulk ESS ≥ 400 for each scalar. |
| Full profile distribution | Calibrated 95% upper bound on total-variation distance ≤ 0.05. All admissible profiles contribute, including unvisited ones. |
| Likelihood-ratio goodness of fit | Both raw and calibrated bootstrap p-values > 0.05 after separate Holm adjustments across the four tori. |
| Labeled moments | Means within 1%, nonzero variances and covariances within 5% of exact. The joint 95% band covers all 86 exact values. |

Total-variation distance is the largest probability difference for any set of profiles. We estimate it from pooled target visits. The
[bootstrap](../benches/finite_volume/bootstrap.rs) resamples nonoverlapping 256-row proposal-clock blocks within each chain, with replacement.
Each of 1,999 replicates recomputes its target-hit denominator, conditional histogram, and labeled moments. In ascending triangle count the frozen
bootstrap seeds are 320201, 400201, 480201 and 640201. Each replicate receives a distinct stream before parallel execution.

The TV radius is the nearest-rank 95th percentile of bootstrap distances from the empirical histogram. The reported upper bound adds the
calibrated radius to the observed TV and caps the result at one. The likelihood-ratio test compares observed deviance against the exact law
with bootstrap deviances about the empirical law. Moment intervals use the [four-torus procedure](#simultaneous-coverage-across-four-tori).
The development run and first confirmation used a 95% band within each torus. Missing diagnostics, an empty target sector in a chain
or replicate, or unresolved moment uncertainty gives an
inconclusive scientific verdict.

### Simultaneous coverage across four tori

The final confirmation used one joint 95% band for all 86 labeled moments. At each torus, the benchmark resamples
whole blocks within each chain to retain short-range dependence. Each bootstrap replicate supplies one number, the largest absolute deviation from
the observed estimate among that torus's moments, with each deviation divided by its bootstrap standard error. Using the maximum accounts for the
dependence among means and covariances measured on the same configurations. Block-bootstrap coverage depends on how well the blocks represent
dependence in the trajectories. For block resampling see [Künsch (1989)](https://doi.org/10.1214/aos/1176347265).
For the maximum-standardized-deviation (sup-$t$) band see [Montiel Olea and Plagborg-Møller (2019)](https://doi.org/10.1002/jae.2656).

Bonferroni allocates one quarter of the 5% error budget to each torus. The critical value for each torus is the 98.75th percentile of its bootstrap
maxima, rank 1,975 of 1,999 in ascending order. Each interval is the observed moment plus or minus this critical value times its standard error.
Let $E_t$ be the event that any interval on torus $t$ misses. If each torus achieves its target coverage, the union bound gives

$$
\Pr\!\left(\bigcup_{t=1}^4 E_t\right) \le \sum_{t=1}^4 \Pr(E_t) \le 4(0.0125)=0.05.
$$

The inequality holds for any dependence between tori. The bootstrap supplies an approximate within-torus band, so measured family coverage on
exact-law samples is a separate check. A point estimate can still miss its 1% or 5% tolerance even when every interval covers. The 95% target
applies to the intervals alone. Holm adjustment continues to apply to the four goodness-of-fit p-values.
[NIST, Bonferroni’s method](https://itl.nist.gov/div898/handbook/prc/section4/prc473.htm)

**One family is one complete four-torus experiment.** It contains eight synthetic chain sequences for each of the four sizes and checks
$14+14+14+44=86$ labeled moments. One missed interval counts as one family miss, as do several misses in the same family.
We generated 200 independent families from the exact laws at the revised budgets. For a torus with $N_2$ triangles in synthetic family
$i=0,\ldots,199$, the generator used seed $20{,}000{,}000{,}000 + 10{,}000 N_2 + i$. The two interval methods used the same samples and
bootstrap replicates within each family. The run fixed the seed ranges, endpoints, and two panels of 100 before sampling.

| Families with a miss | First 100 | Second 100 | All 200 |
| --- | ---: | ---: | ---: |
| Any interval, historical within-torus bands | 16 | 17 | 33 |
| Any interval, new four-torus band | 4 | 6 | 10 |
| Any point tolerance, both methods | 2 | 1 | 3 |
| Full moment rule, new band | 4 | 6 | 10 |

The new band covered all 86 exact values in **190/200 families (95.0%)**, with a 95% Wilson interval of **91.0%–97.3%** for the coverage rate.
Both panels fell below the prespecified alarm of 12 misses per 100 for interval coverage and the full rule. This is a one-sided exact binomial
check at the 1% level in each panel against a 5% miss rate. All three point-tolerance misses coincided with interval misses under the new band.
The experiment measures performance under the specified four-row dependence model. Other dependence patterns in production can change coverage.
[Wilson (1927)](https://doi.org/10.1080/01621459.1927.10502953) · [Archived family control report and seed plan](#retained-evidence)

<!-- rumdl-disable-next-line MD033 -->
<a id="frozen-rule-for-the-next-production-confirmation"></a>

### Frozen rule for the final confirmation

The protocol fixed these inclusive production seed ranges before sampling. The final confirmation used each seed once.

| Time slices | Triangles | Final confirmation seeds | Cadence rows per chain |
| ---: | ---: | --- | ---: |
| 4 | 32 | 2000320101–2000320108 | 262,144 |
| 4 | 40 | 2000400101–2000400108 | 131,072 |
| 4 | 48 | 2000480101–2000480108 | 262,144 |
| 8 | 64 | 2000640101–2000640108 | 8,388,608 |

1. Run these 32 production chains once, eight per torus, using the flat and peaked starts, burn-in, and measurement cadence in the
   [sampling protocol](#sampling-protocol).
2. Require every labeled mean to be within 1% of exact and every nonzero labeled variance or covariance to be within 5% of exact.
   Require all 86 exact moments to lie inside their four-torus simultaneous intervals. A missed point tolerance or interval fails the moment rule.
3. Apply the other [acceptance criteria](#uncertainty-and-acceptance), including both Holm-adjusted distribution tests and the calibrated TV
   bounds. Record the joint verdict from this cohort alone, including a failure or insufficient information.

The earlier holdouts that informed changes to the benchmark are development evidence for the revised procedure. The 200 fresh synthetic families
evaluated the fixed intervals using known truths. If the method changes in response to those families, the changed method needs fresh holdouts.

### Synthetic controls

The [calibration](../benches/finite_volume/calibration.rs) uses 1,999 training datasets from the exact law per condition. At a 5% tail probability,
the Monte Carlo standard error is about 0.0049, below one step of the 1/200 bootstrap grid. Two holdout panels each test the fitted calibration
on 100 null and 100 biased datasets per condition. Each dataset matches the production measurement budget and uses 256-row blocks in eight chains.
An independent draw hits the target with probability 0.25 and repeats for four rows. The alternative redirects 15% of the conditional mass to the
flat profile. Each control uses 199 bootstrap replicates. Calibration starts at the production bootstrap seed plus 100,000.

The multiplier is the larger of one and the nearest-rank 95th percentile of training ratios of actual TV error to estimated radius. Calibration rounds the raw
p-value upward to the 1/200 grid and ranks it among training p-values, counting ties conservatively. A one-sided exact binomial test at 0.01
flags excess null rejection or radius noncoverage against a nominal 5% rate. Each condition must detect at least 90 of 100 biased holdouts.
The joint assessment also applies Holm to each four-condition null family and checks its rejection count. These controls assess the
histogram tests and TV bounds under the specified four-row dependence pattern. Bootstrap ranks, grid rounding, Holm adjustment and the binomial
tail test use exact arithmetic.
The likelihood-ratio logarithms and bootstrap interval calculations use binary64. For the multiple-testing procedure, see
[Sture Holm's 1979 paper](https://www.jstor.org/stable/4615733). The [method bibliography](../REFERENCES.md#block-resampling-and-monte-carlo-coverage)
collects the publication records for the uncertainty calculations.

### How often do exact-law samples miss the moment rule?

At 64 triangles, **43 of 100** exact-law cohorts missed the 5% point tolerance for at least one covariance at the original budget.
All of those misses involved one of the eight neighboring pairs, including the time seam. The particular pair of slices 2 and 3 missed twice.
With four times as many measurements and fresh seeds, **1 of 100** cohorts missed any covariance tolerance. The 95% Wilson intervals for
these two rates are 33.7%–52.8% and 0.2%–5.4%. [Wilson (1927)](https://doi.org/10.1080/01621459.1927.10502953)

| Slices | Triangles | Rows per chain | Any covariance outside 5% | Slices 2, 3 outside 5% | Any interval missed | Full moment rule missed |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 4 | 32 | 262,144 | 0/100 | 0/100 | 6/100 | 6/100 |
| 4 | 40 | 131,072 | 1/100 | 0/100 | 6/100 | 6/100 |
| 4 | 48 | 262,144 | 0/100 | 0/100 | 4/100 | 4/100 |
| 8 | 64 | 2,097,152 | 43/100 | 2/100 | 3/100 | 43/100 |
| 8 | 64 | 8,388,608 | 1/100 | 0/100 | 5/100 | 5/100 |

Each trial used eight chains of synthetic cadence rows. Every independent group of four rows hit the target with probability 1/4.
On a hit, all four rows shared one profile drawn with its exact integer transfer-matrix weight. The production scorer applied the 1% mean and
5% covariance limits and the simultaneous interval check using all 1,999 bootstrap replicates. Every sample mean met its point tolerance.
A trial can miss both a point tolerance and an interval. Pairing trials by index across the four tori, 52/100 original-budget and
19/100 revised-budget families missed the full rule. All 19 missed an interval, and two also missed a point tolerance. This budget study used
separate 95% bands for each torus. Independent tori with exactly 95% coverage each would cover together with probability $0.95^4 \approx 0.8145$.

The fourfold panel was specified before observing the results for any size with 12 or more covariance misses per 100 exact-law cohorts.
The synthetic dependence model approximates the precision of the difficult observable. For slices 2 and 3 at 64 triangles, it gives a
first-order standard error of 0.00108 at the original budget. There are $8(2{,}097{,}152)/16=1{,}048{,}576$ expected independent target
profiles. Dividing the exact-law variance of $(\ell_2-4)(\ell_3-4)$ by this count and taking the square root gives the estimate.
Across 256-row blocks, the corresponding estimates from the existing
development and first confirmation traces were 0.00116 and 0.00115. Production chains can have other dependence patterns.
The controls establish a precision problem in the original benchmark design. The cause of an individual production miss remains uncertain.

The final confirmation used the revised budget at 64 triangles and the original budgets at the other sizes.
Its production seed ranges started 2,000,000,000 above the development seeds. The labeled observables and point tolerances retained their original definitions.
The [archived moment-budget report](#retained-evidence) records the synthetic results.
The refitted histogram calibration at 64 triangles detected 100/100 biased holdouts in each panel and rejected 6/100 null holdouts in each.
Across all four sizes the two panels rejected 2/100 and 6/100 null families after Holm adjustment. The scorer checks the fitted calibration
against its budget-specific SHA-256 digest before issuing a production verdict.

## Retained evidence

The repository keeps the [frozen rule and seed ranges](#frozen-rule-for-the-final-confirmation) and the
[final confirmation report](../benches/finite_volume/final-confirmation-report.json). The Praxis experiment archive under
`experiment-runs/causal-triangulations/finite-volume-238/` retains the original files and SHA-256 manifests.

| Evidence | Path within the archive |
| --- | --- |
| Development report | `development/library-wiring-assessment/report.json` |
| First confirmation protocol and report | `first-confirmation/confirmation-protocol.json` and `first-confirmation/assessment/report.json` |
| Covariance budget study | `calibration/moment-budget/report.json` |
| Four-torus interval controls and seed plan | `calibration/four-torus-intervals/report.json` and `plan.json` in the same directory |
| Final protocol as recorded before sampling | `reserved-production-confirmation/protocol-at-launch.md` |
| Final acquisition, traces and assessment | `reserved-production-confirmation/` |

The SHA-256 of the pre-run protocol snapshot is `709c7fbc90801ab2d2f6ef9d67b958839f0c883f8173cecc654cc26571ad60e8`.

The published final report preserves the original assessment byte for byte. Its acquisition manifest records uncommitted changes at revision
`3fa7684bce421efba711a635f738ece9c97edbee`. Its analysis hashes identify the scorer used for that run, before the provenance changes to
`assessment.rs` and `data.rs`. Replaying those traces with the revised scorer reproduced all four per-torus files byte for byte and all 86 moment
results, input hashes and verdicts. The replay records the revised scorer's own executable, compiler and source fingerprints.

## Reproducing the results

Use the repository's pinned Rust toolchain. The default benchmark reproduces the final confirmation using its 32 seeds and frozen rule.
Repeating that command reproduces the same cohort. A new independent confirmation would require fresh, prespecified seeds.

```bash
cargo bench --locked --bench finite_volume
```

The benchmark prints its new output directory. To choose the path used in the replay command below, append `-- run target/finite-volume`.
The original acquisitions used about 270 MiB each. The final acquisition and assessment used about 880 MiB.
Each torus has its own manifest, eight cadence traces and full-cadence diagnostics.
New `acquisition.json` manifests (v4) record the executable digest, Cargo's build compiler, lockfile digest and source-tree digest.
The source-tree digest hashes one UTF-8 line per file: lowercase SHA-256, two spaces, slash-separated relative path, then LF, sorted by path.
Inputs are `Cargo.toml`, `Cargo.lock` and all Rust files under `src/` and `benches/finite_volume/`. The Git checkout is observed at run time.
In the archived acquisitions, `compiler` identifies the rustc found at collection time and `collection_source_sha256` covers only `acquire.rs`.
The joint decision is the top-level `passed` field in `assessment/report.json`. The default acquisition records its interval scope before sampling.
The scorer writes schema `finite-volume-benchmark/v4` for the four-torus band. Historical acquisitions retain their within-torus scope and v3 reports.
Each condition records its status and its null and biased rejection counts in the panel order declared in `panels`.
The per-torus detail files contain exact weights, visit counts, control scores and SHA-256 hashes of the inputs.
The benchmark exits successfully when the four conditions and both family-level control checks pass.

The development acquisition used Rust 1.100.0 on Apple silicon. Its manifest records the original collector and executable digests.
Each joint report identifies the scoring executable and analysis sources separately. To reproduce the first confirmation at its original budget, run

```bash
cargo bench --locked --bench finite_volume -- confirm target/finite-volume-confirmation
```

That historical run exits with a failed scientific verdict and writes its report.

To reproduce the synthetic moment panels or the revised histogram calibration without collecting production trajectories, run

```bash
cargo bench --locked --bench finite_volume -- moment-controls target/finite-volume-moment-controls
cargo bench --locked --bench finite_volume -- budget-controls target/finite-volume-budget-controls
cargo bench --locked --bench finite_volume -- calibrate target/finite-volume-calibration
```

To recompute the statistics, supply the original acquisition path and a new report directory. If you used the default output path,
substitute the directory printed by the benchmark for `target/finite-volume` below. The scorer reads the saved measurements and their scalar diagnostics,
recomputes the bootstrap and seeded synthetic controls, and starts zero production chains. After a scorer edit, its own build fingerprints can change.
Compare the four condition files, acquisition hashes, statistics and verdicts with the originals.

```bash
cargo bench --locked --bench finite_volume -- assess target/finite-volume target/finite-volume/rescored
```

For the bounded oracle, corruption and calibration tests, run `cargo test --locked --lib finite_volume::`.
The [v0.2.0 tracker](https://github.com/acgetchell/causal-triangulations/issues/335) owns the decision about the milestone's scientific claim.
