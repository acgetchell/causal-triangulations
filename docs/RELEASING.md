# Releasing causal-triangulations

Release preparation uses the pinned shared CLI. Choose one stable tag, predecessor, and UTC date; reruns with those same inputs are deterministic.
Benchmark measurements are separate and are not idempotent.

## v0.1.1 scientific claim checklist

The v0.1.1 release establishes the internal scientific correctness of the implemented 1+1 CDT foundation: validated open-time strips and periodic
S¹(time) × S¹(space) triangulations, topology and foliation preservation, strict causal-simplex classification, reversible local-move bookkeeping,
failure-atomic mutation, Regge-action deltas, complete family/site Hastings factors, exact compatible-build checkpoint continuation, and the documented
finite combinatorial diagnostics.

The release evidence includes:

- Delaunay 0.8.1 Level 1–5 validation for fresh toroidal initializers and topology-aware Level 1–4 reconstruction for evolved or checkpoint-restored states;
- independent counts, incidence, connectivity, boundary, Euler-characteristic, periodic-seam, and strict Up/Down classification checks on open and toroidal
  fixtures, including nonuniform profiles and minimum legal slices;
- spatial- and temporal-seam applications for both k=2 identifiers, exact `(1,3)`/`(3,1)` seam-local inverses, unchanged self-loops, and canonical-state,
  derived-profile, simplex-count, and action restoration after simulated hard failure;
- full action recomputation for every successful move family under ordinary and extreme finite couplings, plus finite target scoring at a matching extreme
  temperature;
- independent forward/reverse flux reconstruction for uniform, unequal fixed, and state-dependent family policies, including distinct `Move22` and
  `EdgeFlip` mixture components and zero reverse support;
- deterministic chunked continuation of both RNG streams, counters, proposal telemetry, measurements, traces, policy binding, and exact triangulation state;
  and
- benchmark correctness gates that identify the workload being measured.

This is a structural and transition-kernel claim, not an analytic ensemble-validation claim. v0.1.1 does **not** establish ergodicity, mixing,
thermalization, continuum-limit behavior, finite-size scaling, fixed-volume production sampling, or agreement with the exact 1+1 transfer-matrix
distribution. That stronger quantitative gate remains [issue #238](https://github.com/acgetchell/causal-triangulations/issues/238) for v0.2.0. Level 5
Delaunay validation is likewise not required after valid CDT evolution because Delaunayhood is not part of the sampled ensemble.

Historical benchmark reports were deliberately retired in September 2026. New evidence uses the shared v2 workflow described below.

## Prerequisites

Install Git, Rustup, the declared uv version, authenticated GitHub CLI, and jq. Bootstrap the managed tools:

```bash
uv run --locked --only-group tooling research-repo-tools setup
just tools-check
```

Prepare a focused release branch manually from reviewed `main`. Choose:

```bash
TAG=vX.Y.Z
PREVIOUS_TAG=vA.B.C
RELEASE_DATE=YYYY-MM-DD
```

## Prepare metadata and validate

Land dependency/tool refreshes separately before release preparation:

```bash
just update
```

Update metadata with explicit inputs, then generate the prospective dated changelog without creating a tag:

```bash
just update-version "$TAG" --previous-release "$PREVIOUS_TAG" --date "$RELEASE_DATE"
just changelog-unreleased "$TAG" "$RELEASE_DATE"
just ci
just ci-slow
just release-version-check
just publish-check
```

The shared release transaction validates candidate files before writing. It synchronizes Cargo metadata, the CFF version/date, and active dependency examples.
Python is a dependency-only environment, so its placeholder version is not another release version.
The permanent Zenodo concept DOI is fixed by policy; top-level version-record identifiers are rejected.
Changelog archives use `docs/archives/changelog/`. Caught write failures trigger rollback; interruption is not a crash-atomic transaction.

Explicit `--previous-release` keeps preparation offline. Omitting it discovers the latest published stable GitHub release.
Use `--dry-run` to preview metadata edits, or `just changelog-preview --tag "$TAG" --date "$RELEASE_DATE"` to preview notes.
Reruns preserve the chosen date; advancing it requires changing the explicit input.

## Fresh performance evidence

The first post-migration tagged release establishes the new baseline. Do not compare it with the retired legacy asset format.
After that baseline exists, a prospective release may run:

```bash
just performance-release "$TAG" "$PREVIOUS_TAG"
```

This user-invoked command creates temporary Git worktrees, runs the configured correctness gate and measurements, and promotes shared evidence and reports.
Agents must respect this repository's prohibition on Git mutations. Review host, source, dependency, and harness provenance before accepting a comparison.

For README publication, prepare the independently pinned configuration from `tooling/performance-readme.example.toml` and run:

```bash
just performance-readme tooling/performance-readme.toml --preview
just performance-readme tooling/performance-readme.toml
```

The table includes absolute baseline/current times, units and intervals alongside relative changes.
See [performance testing](performance-testing.md) for evidence paths and render-only recovery.
Keep the README's explicit pending state until a real new comparison exists.

Review and commit the candidate manually, open the release PR, and merge it only after the required hosted checks pass.

## Tag, attach baseline, and publish

After merging, synchronize to the exact reviewed commit and verify metadata:

```bash
just release-version-check
just tag "$TAG"
git tag -l --format='%(contents)' "$TAG"
git push origin "$TAG"
gh release create "$TAG" --draft --title "$TAG" --notes-from-tag
```

Dispatch the release-baseline workflow against the new tag:

```bash
gh workflow run release-benchmarks.yml -f release_tag="$TAG"
```

The workflow first validates a mutable draft, measures the exact tag with read-only permissions and caches disabled, then attaches
`causal-triangulations-TAG-cdt-baseline-v2.tar.gz` from a separate writer job. Writer jobs install the exact published tooling package and never check out
benchmark source. An identical attachment retry succeeds; different bytes fail rather than overwriting evidence. The release remains a draft.

Verify the baseline attachment and all release gates before publishing:

```bash
cargo publish --locked
gh release edit "$TAG" --draft=false
```

Verify the crates.io package, GitHub release, asset, and Zenodo record through the permanent concept DOI before removing the release branch.
These Git and publication commands are manual maintainer operations.
