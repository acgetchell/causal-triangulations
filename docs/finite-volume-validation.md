# Finite-volume validation of the intrinsic 1+1 CDT sampler

Does the intrinsic 1+1 causal dynamical triangulations (CDT) sampler visit spatial loop-length profiles with the frequencies predicted by the
transfer matrix? One profile is an ordered list of loop lengths across the time slices of **one triangulation**. A Markov chain produces
successive triangulations. We compare the distribution of their profiles at a specified triangle count with the exact finite-lattice law.

## Results

### Independent production confirmation

**All four fixed-volume conditions passed.** After fixing the measurement budgets, calibration, and joint decision rule, we ran the 32 reserved
production chains once. These chains supplied all production measurements for the final verdict. They recorded 17,203,373 target-volume visits
in 72,351,744 scheduled measurements. A visit counts each scheduled observation at the target volume, including repeated states.
The [final confirmation report](../benches/finite_volume/final-confirmation-report.json) records the seed ranges, executable and input hashes,
chain diagnostics, and outcomes under the [frozen rule](#frozen-rule-for-the-final-confirmation).

TV denotes total-variation distance. Each reported TV upper bound applies to one torus.

| Slices | Triangles | Target visits | Observed TV | 95% TV upper bound | Raw / calibrated Holm p | Moments passing | Result |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 4 | 32 | 546,196 | 0.00400 | 0.00946 | 1.000 / 1.000 | 14 / 14 | Passed |
| 4 | 40 | 269,338 | 0.01223 | 0.02656 | 1.000 / 1.000 | 14 / 14 | Passed |
| 4 | 48 | 533,905 | 0.01464 | 0.02936 | 1.000 / 1.000 | 14 / 14 | Passed |
| 8 | 64 | 15,853,934 | 0.00931 | 0.01876 | 1.000 / 0.986 | 44 / 44 | Passed |

All 86 labeled moments met their point tolerances and their four-torus simultaneous intervals covered the exact values. Each calibrated 95% upper
bound on TV was below 0.05. The scalar chain diagnostics and both synthetic-control panels passed. At eight slices and
64 triangles the covariance of slices 2 and 3 was −0.039590, with simultaneous interval [−0.041699, −0.037481]. The exact value was −0.039838,
giving a relative error of 0.62%. These results support agreement with the exact conditional loop-length law at the stated precision for these
four tori. The observables describe spatial volumes. Relative frequencies of distinct triangulations with the same profile, the marginal
distribution of total volume, and continuum scaling require separate measurements.

#### What changed after the first confirmation

The first independent confirmation failed its joint rule. Its only miss was the same
64-triangle covariance, measured at −0.037401 with a 6.12% error against the 5% tolerance. Its interval [−0.041106, −0.033696] contained
the exact value. The point-accuracy rule and interval-coverage rule answered different questions. At that budget,
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

The simulator stores a starting vertex on each spatial circle. For a strip ending at length $b$, the upper starting vertex gives $b$ choices of
relative rotation. Around periodic time these factors multiply to $\prod_t\ell_t$. The sampling weight divides by that product, leaving exactly
the strip factors in $w(\ell)$. Time labels and orientation stay fixed.
See [the sampler's marking convention](scientific-basis.md#intrinsic-periodic-cdt-measure).

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

The measured values come from the Rust chains. With $H$ pooled target-volume visits and slice indices starting at zero, the estimator is
$H^{-1}\sum_{h=1}^{H}\ell_{h,i}\ell_{h,j}-\bar\ell_i\bar\ell_j$, where $\bar\ell_i$ is the mean over those same visits. Thus
$\operatorname{Cov}(\ell_2,\ell_3)$ pairs the third and fourth slices of each triangulation. Dependence between successive Markov-chain
observations enters the uncertainty estimate through block resampling.
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
It checks positive forward and reverse transition probabilities from the production proposal along each path. This establishes a reachable
representative for every admissible profile. The preflight places no bound on how long a chain takes to explore them.

Each row of the table represents eight independently seeded Xoshiro256++ chains. Four start flat. The other four start with all excess
length on one slice, rotating the peak through slices 0, 1, 2, 3 at four slices or 0, 2, 4, 6 at eight slices. The production chains use
uniform move-family selection and unit temperature. The edge coupling $2\ln(2)/3$ gives the triangle coupling $\ln(2)$ because
$N_1=3N_2/2$. Quadratic volume fixing with strength 0.05 favors triangulations near the target. The chain can leave and return to it.
At the target, every profile receives the same volume-dependent weight, so the retained visits estimate the conditional law above.

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

| Quantity | Criterion frozen for the final production confirmation |
| --- | --- |
| Information in total volume and every slice length | Combined rank-normalized/folded split R-hat ≤ 1.01 and bulk ESS ≥ 400 for each scalar. |
| Full profile distribution | Calibrated 95% upper bound on total-variation distance ≤ 0.05. All admissible profiles contribute, including unvisited ones. |
| Likelihood-ratio goodness of fit | Both raw and calibrated bootstrap p-values > 0.05 after separate Holm adjustments across the four tori. |
| Labeled moments | Means within 1%, nonzero variances and covariances within 5% of exact. The joint 95% band covers all 86 exact values. |

Split R-hat compares variation within and between split chains. The combined diagnostic checks both ranks and folded ranks, which can reveal
differences in location or scale. Bulk effective sample size (ESS) estimates how much independent information remains in the ranked
observations after accounting for serial dependence. The frozen protocol applied both diagnostics to the full-cadence total volume and
slice lengths. The later audit also checked [information within the target sector](#supplementary-checks-specified-after-confirmation).

Let $P$ be the exact conditional law and $\widehat P$ the histogram of pooled target visits. The observed TV distance is
$\tfrac12\sum_\ell |\widehat P(\ell)-P(\ell)|$, the largest probability difference for any set of profiles. The
[bootstrap](../benches/finite_volume/bootstrap.rs) resamples nonoverlapping blocks of 256 scheduled measurements (4,096 proposals) within each
chain, with replacement. Blocks are formed before conditioning, so stretches away from the target keep their place in the sampling design.
Each of 1,999 replicates recomputes its target-hit denominator, conditional histogram, and labeled moments. In ascending triangle count the frozen
bootstrap seeds are 320201, 400201, 480201 and 640201. Each replicate receives its seed before parallel execution. Interpreting the bounds
for the sampler assumes that the post-burn-in chains represent a common stationary law and that the blocks capture the relevant dependence.

The TV radius is the nearest-rank 95th percentile of bootstrap distances from the empirical histogram. By the triangle inequality, the
underlying sampler-to-reference distance is at most the observed TV plus the histogram's estimation error. The benchmark estimates an upper
bound on that error by multiplying the bootstrap radius by its [calibrated multiplier](#synthetic-controls), then caps the total bound at one.

The goodness-of-fit test compares the observed histogram's likelihood-ratio deviance from the exact law with bootstrap deviances from the
empirical histogram. The raw p-value is the fraction of replicates at least as extreme, with one added to both the exceedance count and the
replicate count. The calibrated p-value ranks that raw score against exact-law training scores. Holm adjustment operates separately on the
four raw and four calibrated p-values. A large adjusted p-value means the test did not detect a discrepancy. The TV bound quantifies the
size of a discrepancy compatible with the data under the calibration model. Moment intervals use the
[four-torus procedure](#simultaneous-coverage-across-four-tori).
The development run and first confirmation used a 95% band within each torus. The scorer treats insufficient information as inconclusive.
Examples include an unavailable chain diagnostic, a chain with no target visits, a bootstrap replicate with zero pooled target visits,
or an interval with unresolved uncertainty.

### Synthetic controls

The [calibration](../benches/finite_volume/calibration.rs) uses 1,999 training datasets from the exact law per condition. At a 5% tail probability,
the Monte Carlo standard error is about 0.0049, below one step of the 1/200 bootstrap grid. Two holdout panels each test the fitted calibration
on 100 null and 100 biased datasets per condition. Each dataset matches the production measurement budget and uses 256-row blocks in eight chains.
An independent draw hits the target with probability 0.25 and repeats for four rows. The alternative redirects 15% of the conditional mass to the
flat profile. The original training and holdout datasets each use 199 bootstrap replicates. Calibration starts at the production bootstrap seed
plus 100,000. These synthetic sequences draw profiles directly from a known law. They exercise the analysis independently of the CDT move kernel.

The multiplier is the larger of one and the nearest-rank 95th percentile of training ratios of actual TV error to estimated radius. Calibration rounds the raw
p-value upward to the 1/200 grid and ranks it among training p-values, counting ties conservatively. A one-sided exact binomial test at 0.01
flags excess null rejection or radius noncoverage against a nominal 5% rate. Each condition must detect at least 90 of 100 biased holdouts.
The frozen family-level control counted null families rejected by calibrated Holm. Production required both the raw and calibrated Holm
tests to pass. The [post-confirmation audit](#supplementary-checks-specified-after-confirmation) checked the combined decision directly.
These controls assess the histogram tests and TV bounds under the specified four-row dependence pattern. Bootstrap ranks, grid rounding,
Holm adjustment and the binomial tail test use exact arithmetic. The likelihood-ratio logarithms and bootstrap interval calculations use
binary64. For the multiple-testing procedure, see
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

Before observing the synthetic results, we set a rule to test a fourfold budget at any size with at least 12 covariance misses per 100 cohorts.
For slices 2 and 3 at 64 triangles, 5% of the absolute exact covariance is only 0.00199. The synthetic model gives a first-order standard
error of 0.00108 at the original budget. There are $8(2{,}097{,}152)/16=1{,}048{,}576$ expected independent target
profiles. Dividing the exact-law variance of $(\ell_2-4)(\ell_3-4)$ by this count and taking the square root gives the estimate.
For comparison, the archived development and first-confirmation traces gave block-based standard errors of 0.00116 and 0.00115 for this
centered product. For each 256-row block, sum the product over target visits and subtract the pooled product mean times the block's
target-hit count. Within each chain multiply the sample variance of these block scores by the number of blocks. Sum across chains, take
the square root, and divide by the pooled number of target visits. The similar error scales support the use of this synthetic control for
this covariance. Dependence can differ for other observables.
The controls establish a precision problem in the original benchmark design. The cause of an individual production miss remains uncertain.

The final confirmation used the revised budget at 64 triangles and the original budgets at the other sizes.
Its production seed ranges started 2,000,000,000 above the development seeds. The labeled observables and point tolerances retained their original definitions.
The [archived moment-budget report](#retained-evidence) records the synthetic results.
The refitted histogram calibration at 64 triangles detected 100/100 biased holdouts in each panel and rejected 6/100 null holdouts in each.
Across all four sizes the two panels rejected 2/100 and 6/100 null families after Holm adjustment. The scorer checks the fitted calibration
against its budget-specific SHA-256 digest before issuing a production verdict.

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
exact-law samples is a separate check. A point estimate can still miss its 1% or 5% tolerance even when every interval covers. The 95% joint
coverage target applies to the 86 moment intervals. The complete pass/fail rule combines those intervals with point tolerances, per-torus TV
bounds, goodness-of-fit decisions, and diagnostics. Its overall false-failure probability has no nominal 5% guarantee.
[NIST, Bonferroni’s method](https://itl.nist.gov/div898/handbook/prc/section4/prc473.htm)

**One family is one complete four-torus experiment.** It contains eight synthetic chains at each of the four sizes. With $T$ slices, the
scorer checks $T$ means and $T(T+1)/2$ distinct entries of the symmetric covariance matrix, including the variances. This gives
$14+14+14+44=86$ labeled moments. The fixed sum of the loop lengths makes some moments dependent. A family counts as a miss if one or
more of its 86 intervals misses.
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

The protocol fixed these inclusive production seed ranges before sampling. They are disjoint from the development and first-confirmation
ranges. The final confirmation used each seed once.

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

### Supplementary checks specified after confirmation

The following checks were specified during review, after the reserved production run. They apply to rescoring and to subsequent runs.
The original confirmation report retains its original rule. The new controls use synthetic seeds independent of the production cohort.

- **Conditional information.** Within each chain require at least 50 distinct 256-row blocks with a target visit (400 across eight chains).
  For every labeled slice monitor the conditional probability that its length exceeds the exact mean. For each proposal-clock block form
  `above - p_hat * hits`, where `above` counts target visits above that mean, `hits` counts all target visits, and `p_hat` is the pooled
  fraction. This is the centered numerator of the conditional proportion, so empty blocks retain their place and unequal denominators are respected.
  Apply the existing split R-hat ≤ 1.01 and bulk ESS ≥ 400 thresholds across the eight chains for each labeled slice's block score.
  The 50-block floor is a benchmark design choice that requires each chain to contribute to the 400-block minimum. Occupied blocks can be
  correlated. An unavailable diagnostic
  or a missed minimum gives an inconclusive result. See [Vehtari et al. on rank-based diagnostics](https://arxiv.org/abs/1903.08008).
- **Bootstrap-budget transfer.** Keep the training fit, p-value ranking, TV multiplier, 256-row blocks and production scoring rule fixed.
  Use two fresh panels of 100 four-torus families. For triangle count `N2` and family index `i = 0..199`, use seed
  `30_000_000_000 + 10_000 * N2 + i` for the exact law and add 200 for the biased control. Pair the 199- and 1,999-replicate scores on identical
  synthetic data using the same bootstrap stream. At each budget count a null family once when either raw or calibrated Holm rejects.
  Apply the existing alarm of 12 or more rejections per 100 families separately to each panel. Apply the same alarm to TV-radius misses per torus,
  panel and distribution at each budget. Record the paired p-values, radius misses and rejection counts for each decision.
  A validated transfer supports using the frozen fit at 1,999 replicates under this dependence model. If transfer fails, a fit trained at
  1,999 replicates needs independent validation at that budget before it can support a new verdict.

**Replay result.** All four per-torus detail files matched the frozen confirmation byte for byte. All 86 moment estimates and intervals,
both Holm-adjusted production p-values for each torus, and all four verdicts reproduced. Every conditioned slice diagnostic passed.
Across the four tori, the smallest number of occupied blocks in any chain was 512, the largest split R-hat was 1.00091 (rounded up),
and the smallest bulk ESS was 3,806 (rounded down).

Each panel contains 100 fresh four-torus null families and 100 biased families. Its two rows rescore the same families at different bootstrap budgets.
The radius columns list misses in ascending triangle count (32, 40, 48, 64). An alarm requires at least 12 misses in an individual count.

| Panel | Bootstrap replicates | Raw Holm rejections | Calibrated Holm rejections | Either Holm rejects | Null radius misses | Biased radius misses |
| ---: | ---: | ---: | ---: | ---: | --- | --- |
| 1 | 199 | 4 | 4 | 5 | 3, 4, 7, 4 | 6, 4, 2, 4 |
| 1 | 1,999 | 4 | 3 | 4 | 3, 4, 8, 3 | 5, 3, 2, 4 |
| 2 | 199 | 3 | 4 | 4 | 4, 6, 9, 8 | 2, 2, 6, 7 |
| 2 | 1,999 | 7 | 6 | 7 | 4, 5, 8, 8 | 2, 3, 7, 8 |

The fixed calibration met the transfer criterion in both panels under the tested four-row dependence model. The original two null panels
still rejected 2 and 6 families when counted with the combined Holm decision. This audit reused the archived production traces.

## Retained evidence

The repository keeps the [frozen rule and seed ranges](#frozen-rule-for-the-final-confirmation) and the
[final confirmation report](../benches/finite_volume/final-confirmation-report.json). The original traces and SHA-256 manifests are in
the maintainer's private Praxis archive under `experiment-runs/causal-triangulations/finite-volume-238/`. To request the traces,
contact the crate maintainer, [Adam Getchell](mailto:adam@adamgetchell.org), and identify the cohort from the table below.
The public repository supplies the oracle, scorer, tests, and seeds for collecting a fresh copy.

| Evidence | Path within the archive |
| --- | --- |
| Development report | `development/library-wiring-assessment/report.json` |
| First confirmation protocol and report | `first-confirmation/confirmation-protocol.json` and `first-confirmation/assessment/report.json` |
| Covariance budget study | `calibration/moment-budget/report.json` |
| Four-torus interval controls and seed plan | `calibration/four-torus-intervals/report.json` and `plan.json` in the same directory |
| Post-confirmation bootstrap-budget audit | `calibration/bootstrap-budget-transfer/report.json` and `bootstrap-budget-controls.json` in the same directory |
| Final protocol as recorded before sampling | `reserved-production-confirmation/protocol-at-launch.md` |
| Final acquisition, traces and assessment | `reserved-production-confirmation/` |

The SHA-256 of the pre-run protocol snapshot is `709c7fbc90801ab2d2f6ef9d67b958839f0c883f8173cecc654cc26571ad60e8`.

The published final report preserves the original assessment byte for byte. Its acquisition manifest records uncommitted changes at revision
`3fa7684bce421efba711a635f738ece9c97edbee`. Its analysis hashes identify the scorer used for that run, before the provenance changes to
`assessment.rs` and `data.rs`. Replaying those traces with the revised scorer reproduced all four per-torus files byte for byte and all 86 moment
results, input hashes and verdicts. Recomputing the full-cadence diagnostics from the 32 archived traces also reproduced all 24
pairs of combined R-hat and bulk ESS values exactly. The replay records the revised scorer's own executable, compiler and source fingerprints.

## Reproducing the results

Use the repository's pinned Rust toolchain. Choose the command according to which part of the evidence you want to check.

### Check the oracle and scorer

`cargo test --locked --lib finite_volume::` runs the bounded known-answer, acquisition-integrity and statistical tests. It checks the
published normalizers and labeled covariances without regenerating the 32-chain cohort.

### Replay the saved production traces

Obtain `reserved-production-confirmation/` from the [experiment archive](#retained-evidence). From that directory, run
`shasum -a 256 -c SHA256SUMS` to verify the original files. Point `acquisition` at the directory containing `acquisition.json`
and the four torus directories. The output directory must be new.

```bash
acquisition=/path/to/reserved-production-confirmation
cargo bench --locked --bench finite_volume -- assess "$acquisition" target/finite-volume-replay
```

The archived per-torus manifests (`native-v1`) predate per-chain hashes. Replay recomputes scalar diagnostics from the full cadence
of all eight traces, including off-target observations. For new acquisitions (`native-v2`), collection writes each compressed trace's
SHA-256 beside its chain seed in the manifest. Replay verifies all eight digests before using the scalar diagnostics saved with them.
The scorer trusts the collector's per-torus `manifest.json` for `native-v2`. An edit to a saved R-hat or bulk ESS value leaves the
trace digests valid and can change the verdict. The replay report records the manifest's SHA-256 for comparison with an independently
retained digest of the original manifest.

Both paths recompute the bootstrap and seeded synthetic controls. Compare the input hashes, per-torus detail files, scalar diagnostics,
moment estimates and intervals, distribution statistics, and verdicts with the frozen report.

### Repeat the synthetic controls or collect chains

These commands regenerate, respectively, the moment-budget study, the four-torus simultaneous-interval controls, and the histogram
calibration and bootstrap-budget transfer checks. They draw synthetic profiles with known probabilities.

```bash
cargo bench --locked --bench finite_volume -- budget-controls target/finite-volume-budget-controls
cargo bench --locked --bench finite_volume -- moment-controls target/finite-volume-moment-controls
cargo bench --locked --bench finite_volume -- calibrate target/finite-volume-calibration
```

To collect the final production cohort again and score it, run `cargo bench --locked --bench finite_volume`. It prints the new output
directory. To choose a new directory, append `-- run target/finite-volume`. This repeats the published seeds. An independent confirmation
would require new seeds and a protocol fixed before collection. The historical acquisitions used about 270 MiB each, and the final
acquisition and assessment used about 880 MiB. To repeat the first confirmation's seeds and original budget, use
`cargo bench --locked --bench finite_volume -- confirm target/finite-volume-confirmation`. The original run failed its point-tolerance
criterion and retained its report.

### Read the output

Each torus has a manifest, eight cadence traces and full-cadence diagnostics. The development acquisition used Rust 1.100.0 on Apple
silicon. In the archived manifests, `compiler` identifies the rustc found at collection time and `collection_source_sha256` covers
`acquire.rs`. New acquisition manifests (v4) record the executable digest, Cargo's build compiler, the lockfile digest and the source-tree
digest. The tree hash covers `Cargo.toml`, `Cargo.lock` and every Rust file under `src/` and `benches/finite_volume/`. Hash input lines
contain the lowercase file SHA-256, two spaces, the slash-separated relative path and a newline, sorted by path. The runtime checkout is
recorded separately.

The top-level `passed` in the assessment's `report.json` gives the joint decision. The current scorer writes
`finite-volume-benchmark/v5` and uses the acquisition's declared moment interval scope. The archived final report (v4) used the
four-torus band, and the two earlier reports (v3) used within-torus bands. Each condition records its status and its null and biased
rejection counts in the declared panel order. The per-torus detail files contain exact weights, visit counts, control scores and input
hashes. The supplementary `bootstrap-budget-controls.json` contains the paired trials and the frozen holdouts' raw and calibrated p-values.

The [v0.2.0 tracker](https://github.com/acgetchell/causal-triangulations/issues/335) owns the decision about the milestone's scientific claim.
