# 2d-cdt Sphere Mode Audit

**Decision: Outcome A of [#258][issue].** At pinned `JorenB/2d-cdt` revision `412d0b887532c46e4445581b1a2bd509f65aaaec`, initialization and legal local moves
retain a toroidal stored complex under both values of `sphere`. The option changes move eligibility and observable neighbor lists. It does not implement
closed simplicial S² construction. This is a reference-interpretation result, not evidence that `causal-triangulations` is a more correct sampler.

## Contents

- [Claim boundary](#claim-boundary)
- [Source paths](#source-paths)
- [Independent census](#independent-census)
- [Import boundary](#import-boundary)
- [Documentation search](#documentation-search)
- [Reproduction](#reproduction)

## Claim Boundary

The implementation paper's [section 2.2][paper] derives the labelled 1+1 toroidal ensemble. The pinned [README][readme] separately describes `sphere` as
“effective spherical topology.” Both statements must be preserved; neither makes the option a closed simplicial S² sampler.

The planned [#240 reference comparison][parity] covers periodic 1+1 CDT with circular spatial slices, using `sphere=false`. It excludes `sphere=true`
compatibility and genuine S² support. S² would require a separately justified topology, move, boundary-amplitude, and analytic-validation contract. The
toroidal transfer-matrix trace is not a spherical oracle. This decision does not affect the distinct 2+1 model with S² **spatial slices** in `3d-cdt`.

No ensemble agreement, mixing, timing advantage, or relative scientific superiority is inferred here. Our own [#316 reverse-support defect][support] and
[#238 exact-ensemble gate][exact] remain unresolved. The intended continuum or coarse-grained meaning of the reference authors' adjective remains unknown.

## Source Paths

All locators below use the pinned revision. Searching its complete source for `sphere` finds the branches below, plus the unrelated `Observable::sphere`
and `sphereDual` metric-ball routines. The retained source archive includes its original MIT license and copyright notices.

| Path | Behavior relevant to the audit |
| --- | --- |
| [config.hpp:23][config-source], [main.cpp:30–49][main-source] | Requires the option; sets the flag before choosing import or construction. |
| [universe.cpp:21–85][initialize] | Width 3, `T` slices, `3T` vertices, `6T` triangles; modulo space and time gluing in both modes. |
| [simulation.cpp:41–67,87–183][moves] | Attempts add/delete/flip; sphere rejects additions with base time 0; deletion forbids slice length below 3. |
| [universe.cpp:87–195][edits] | Insert/delete change `(N0,N1,N2)` by `±(1,3,2)`; flips preserve counts. No cap, quotient, or pole is introduced. |
| [universe.cpp:257–312][vertex-data] | Sphere removes end-slice temporal neighbors from the reconstructed vertex lists. |
| [universe.cpp:364–389][triangle-data] | Sphere drops center entries for up triangles at time 0 and down triangles at time `T-1`. |
| [universe.cpp:315–362][link-data] | Links still use stored triangle pointers, with an assertion `N1 = 3 N0`; no sphere branch. |
| [universe.cpp:391–455][export-source] | Exports all vertex times, triangle vertex triples, and all three stored neighbors; no sphere branch. |
| [universe.cpp:457–537][import-source] | Imports supplied labels/pointers, checks bookkeeping, requires slice 0 length 3 in sphere mode; filename adds `-sphere`. |
| [simulation.cpp:14–39,69–85,185–189,235–276][measurement] | Growth, thermalization and sweeps; prepares helper lists before measurement. |
| [observable.cpp:36–176][observables] | Distances/metric balls follow reconstructed lists, so the sphere graph edits affect measurements. |

`VolumeProfile` reads `sliceSizes`; Hausdorff and Ricci observables use direct or dual metric-ball/distance helpers. Thermalization also reads reconstructed
vertex neighbors. Thus the option affects more than display. `Simulation::sweep` attempts `100 * targetVolume` moves, then waits for exact target volume
before measuring. That measurement protocol is outside this topology audit and still needs the separate #240 ensemble comparison.

The regular seed is a triangulation of a rectangle with both pairs of opposite sides identified. Local insert/delete operations subdivide or undo a
two-triangle disk, and the flip replaces the diagonal of a disk. Legal applications preserve its topology. Pinning a three-vertex slice does not collapse
it to a point. The count identity `N2 = 2 N0` is preserved, and independent edge reconstruction below gives `N1 = 3 N0`, hence `chi = 0`.

## Independent Census

The [configuration](config.json) fixes four slices, initial width 3, `lambda=0.693147`, target 96, epsilon 0.02, move weights `[1,1]`, simulation seed 1,
and the upstream bag RNG seed 0. Each mode has one run of 4,096 attempted moves. The [harness](harness.cpp) links the original move kernels and uses the
public `start(0,...)` configuration path with growth skipped. This is a finite transition exercise, with no thermalization or equilibrium claim.

The [census](checks/topology.py) derives edges from exported vertex triples. It tests duplicate/degenerate faces, edge incidence, connectivity, boundary
components, every vertex link, and orientation consistency. It does not rely on `Universe` to classify topology. Only afterward does it compare the result
with stored neighbor pointers and helper lists. Endpoint-pair edge identity is valid for the retained simplicial fixtures because those stored pointers and
link multiplicities agree with it; this census is not an oracle for arbitrary parallel-edge triangle gluings. Known-answer tests cover a sphere, disk,
cylinder, torus, Klein bottle, disconnected spheres, an isolated
vertex, a pinched vertex with closed edges, and a three-face edge. Directed-adjacency and malformed-export controls cover the other audit boundaries.

| Mode/state | N0 | N1 | N2 | chi | Components | Boundary components | Accepted add/delete/flip |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| false, initial | 12 | 36 | 24 | 0 | 1 | 0 | — |
| false, evolved | 77 | 231 | 154 | 0 | 1 | 0 | 935 / 870 / 2053 |
| true, initial | 12 | 36 | 24 | 0 | 1 | 0 | — |
| true, evolved | 15 | 45 | 30 | 0 | 1 | 0 | 758 / 755 / 2003 |

All four stored complexes are connected, closed, orientable simplicial 2-manifolds: every edge has incidence 2, every vertex link is one cycle, and every
triangle has three distinct dual neighbors. Their Euler characteristic therefore identifies genus 1, not S². All stored pointer slots and reconstructed
links agree with the independently derived edges. The evolved spatial profiles are `[24,15,16,22]` and `[3,4,3,5]` respectively. Rejections number 238 and
580. Different volumes are expected from distinct kernels; the target is soft and these endpoints are not target-conditioned measurements.

In sphere mode the **observable** dual graph has six degree-two rows initially and eight after evolution. The reverse arcs remain, giving six and eight
nonreciprocal arcs. For example, initially triangle 0 omits center neighbor 19 while 19 retains 0. Removing one directed arc does not cut a shared edge in a
simplicial surface. The degree-two rows are boundary-like but do not certify an undirected cylinder, a capped surface, or S². Vertex neighbor lists instead
omit both directions of temporal-seam edges: 12 directed arcs initially and 16 after evolution. No stored simplices are removed by either reconstruction.

All labels, pointer slots, adjacency differences, degree histograms, move counts and census fields are retained in [evidence/census.json](evidence/census.json)
and its `.dat`/`.json` inputs. This is endpoint evidence supported by the source-level local-move argument, not an exhaustive census of all possible states.

## Import Boundary

Fresh-process imports of both evolved exports reproduce their topology and observable graphs. Importing a sphere-mode export does not cap its temporal seam.

Import is broader than the initializer's admissible **labelled** state set. The retained `gap-input.dat` changes only initial time labels from `[0,1,2,3]`
to `[0,1,3,4]`. The unmodified importer accepts it in sphere mode, including `Universe::check` and all reconstruction routines. Independent analysis finds
profile `[3,3,0,3,3]` and six edges between nonadjacent times. Initialization plus legal moves cannot produce that empty slice under the minimum-size rule.
Its underlying abstract complex is still a torus; this counterexample is not a new topology or a valid causal benchmark state.

The import format supplies arbitrary triples and pointers, with no explicit sphere construction, Euler/manifold test, or general causal-label validation.
The audit does not classify every malformed file that may pass partial checks. It establishes that successful import alone cannot certify topology or
admissibility, and provides no supported S² import contract. Arbitrary imports require independent validation; no claim of their exhaustive rejection is made.

## Documentation Search

The public search on 26 October 2026 covered the implementation paper, pinned README and source, the 63-commit ancestor history, the sphere-related
[8599b0d commit][sphere-history], [6bae388 README update][readme-history], and the v1.0.0 release. The [Zenodo archive][archive] and
[GitHub release][release] describe a torus implementation. The release tag resolves to `b5f64c7ce8c7ca63d2654a3902d3f66bb9db986a`, distinct from the audit pin.
History and release metadata are retained under `evidence/`. The README promises further sphere comments but supplies no formal definition afterward.

Public searches for `Brunekreef "effective spherical topology"`, `"2d-cdt" "sphere"`, and `"Joren Brunekreef" "effective" "sphere" topology` located no
author definition linking this switch to a spherical quotient, cap prescription, target measure, or continuum limit. This is a bounded negative search,
not proof that no such explanation exists. Author clarification could resolve the intended effective interpretation; it is unnecessary to determine the
topology of the stored complexes audited here. No author contact was made.

## Reproduction

`source.tar.gz` retains the unmodified pinned source. `instrumentation.patch` reduces vertex/triangle pool capacity to 100,000/200,000 and link capacity
to 300,000; it changes no move, probability, gluing, or reconstruction logic. The run has at most 4,108 vertices even if every attempt adds one, below every
capacity. Assertions remain enabled. The harness replaces the executable entry point, exports raw data, and runs upstream bookkeeping checks after each
attempt. It does not call the production crate.

The build used Apple clang 21.0.0, C++14, `-O3 -Wno-format`, on Darwin 27 arm64 with libc++. Compiler/platform output and compiler/executable SHA-256 digests
are retained under `evidence/`. `SHA256SUMS` binds the source, patch, harness, configuration, analysis, and generated artifacts. C++ standard-library random
engines/distributions need not produce byte-identical trajectories across toolchains; cross-toolchain runs must preserve their own provenance.

From the repository root, verify and analyze the retained data without another simulation:

```bash
(cd docs/audits/2d-cdt_sphere && shasum -a 256 -c SHA256SUMS)
uv run --locked --group dev pytest docs/audits/2d-cdt_sphere
uv run --locked --group dev python docs/audits/2d-cdt_sphere/checks/topology.py \
  docs/audits/2d-cdt_sphere/evidence target/2d-cdt-sphere-census.json
```

To deliberately regenerate the workload, choose a fresh output directory. The following commands leave retained evidence untouched:

```bash
AUDIT=docs/audits/2d-cdt_sphere
RUN=target/2d-cdt-sphere-reproduction
mkdir -p "$RUN/source" "$RUN/evidence"
tar -xzf "$AUDIT/source.tar.gz" --strip-components=1 -C "$RUN/source"
patch -p1 -d "$RUN/source" < "$AUDIT/instrumentation.patch"
clang++ -std=c++14 -O3 -Wno-format -I "$RUN/source" "$AUDIT/harness.cpp" \
  "$RUN/source/universe.cpp" "$RUN/source/triangle.cpp" "$RUN/source/vertex.cpp" \
  "$RUN/source/link.cpp" "$RUN/source/simulation.cpp" "$RUN/source/observable.cpp" -o "$RUN/census"
"$RUN/census" false "$RUN/evidence/false" 4096
"$RUN/census" true "$RUN/evidence/true" 4096
"$RUN/census" false "$RUN/evidence/import-false" 0 "$RUN/evidence/false-evolved.dat"
"$RUN/census" true "$RUN/evidence/import-true" 0 "$RUN/evidence/true-evolved.dat"
"$RUN/census" true "$RUN/evidence/import-gap" 0 "$AUDIT/evidence/gap-input.dat"
uv run --locked --group dev python "$AUDIT/checks/topology.py" "$RUN/evidence" "$RUN/census.json"
```

Regeneration is opt-in; routine CI tests the independent oracle and retained evidence without downloading or executing the C++ simulator. No performance
samples were collected. The measurement ownership/provenance guidance from `scientific-benchmarking` was applied to this one bounded topology workload.

[issue]: https://github.com/acgetchell/causal-triangulations/issues/258
[parity]: https://github.com/acgetchell/causal-triangulations/issues/240
[exact]: https://github.com/acgetchell/causal-triangulations/issues/238
[support]: https://github.com/acgetchell/causal-triangulations/issues/316
[paper]: https://arxiv.org/html/2310.16744v1#S2.SS2
[archive]: https://zenodo.org/record/5572409
[release]: https://github.com/JorenB/2d-cdt/releases/tag/v1.0.0
[sphere-history]: https://github.com/JorenB/2d-cdt/commit/8599b0d31e9ea1418cd3387cb1a42c8d232cef63
[readme-history]: https://github.com/JorenB/2d-cdt/commit/6bae38820aac3be54d2014c44b0d0ee100ea88d4
[readme]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/README.md#L33
[config-source]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/config.hpp#L23
[main-source]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/main.cpp#L30-L49
[initialize]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L21-L85
[moves]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/simulation.cpp#L41-L183
[edits]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L87-L195
[vertex-data]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L257-L312
[triangle-data]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L364-L389
[link-data]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L315-L362
[export-source]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L391-L455
[import-source]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/universe.cpp#L457-L537
[measurement]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/simulation.cpp#L14-L276
[observables]: https://github.com/JorenB/2d-cdt/blob/412d0b887532c46e4445581b1a2bd509f65aaaec/observable.cpp#L36-L176
