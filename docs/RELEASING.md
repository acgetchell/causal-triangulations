# Releasing causal-triangulations

This is the canonical release flow for a stable `vX.Y.Z` release. Release preparation is content-idempotent: rerunning the deterministic commands on the
same UTC day produces the same tracked files. Performance measurements are deliberately separate because measurement output is not idempotent.

## Prerequisites

Install these external tools before running `just setup`:

- Rustup and Cargo, with the repository toolchain available
- [uv](https://docs.astral.sh/uv/)
- [GitHub CLI](https://cli.github.com/) authenticated for this repository
- `jq`
- `just`

`just setup-tools` checks `uv`, `rustup`, `cargo`, `gh`, and `jq` before installing or changing any managed tool.

Start from an up-to-date `main` and choose the exact stable tag:

```bash
git switch main
git pull --ff-only
git remote -v
gh auth status

TAG=vX.Y.Z
```

Do not set a second version variable. The release commands parse and validate `TAG` themselves.

## 1. Refresh dependencies separately

Run the canonical repository refresh before opening the release branch:

```bash
just update
```

`just update` updates Rust dependencies and lockfiles, resolves exact Python development pins in one transaction, upgrades managed Cargo tools, synchronizes
the root tool pins, refreshes `uv.lock`, and syncs the development environment. Review and land these changes separately from the release PR.

Do not continue until `main` contains the reviewed dependency/tool refresh.

## 2. Prepare the release PR

Create a focused release branch:

```bash
git switch -c "release/$TAG"
```

Apply the deterministic version and metadata transaction:

```bash
just update-version "$TAG"
```

The updater:

- accepts only a stable `vX.Y.Z` tag;
- discovers the latest published stable GitHub release while excluding drafts, prereleases, and malformed tags;
- synchronizes `Cargo.toml`, the root package in `Cargo.lock`, `pyproject.toml`, the editable project in `uv.lock`, and `CITATION.cff`;
- uses the current UTC date for `date-released`;
- preserves the permanent Zenodo concept DOI and removes the recognized legacy version-record DOI block;
- updates active dependency snippets and non-artifact release links while preserving historical benchmark artifact links;
- validates the complete candidate tree before writing and restores every changed file byte-for-byte if a later write fails.

The command fails without writing when release history is missing or malformed, the target is older than the latest stable release, an unexpected version
reference is present, or citation metadata has an unrecognized DOI structure.

Generate the release changelog:

```bash
just changelog-unreleased "$TAG"
```

This runs `git-cliff` without creating a temporary tag, applies Markdown hygiene, synchronizes the generated heading to the same UTC release date, and
archives completed minor release series.

Generate the retained performance evidence and tracked publication files:

```bash
just performance-release "$TAG"
```

The baseline defaults to the newest published stable release older than `$TAG`; pass it as the second argument when the comparison must be explicit. The
command measures the baseline tag and current tracked source in isolated worktrees, writes the CSV/provenance pair under `target/bench-reports/`, reloads
and validates it after measurement, then atomically updates `docs/PERFORMANCE.md`, its archived release-pair report and index, the performance SVG, and the
owned README summary. Inspect the retained pair and tracked outputs before continuing. Untracked files are intentionally excluded from the measured source
state.

Run the release gates:

```bash
just ci
just release-version-check
cargo publish --locked --allow-dirty --dry-run
```

`release-version-check` requires all package versions, the CFF version/date, the active changelog heading, dependency snippets, and non-artifact release links
to agree. It also requires the permanent concept DOI and rejects version-specific top-level citation identifiers.

`performance-doc` and `performance-readme` are deterministic render-only recovery commands. They require both retained pair members, validate their digest
and release identity, and never invoke Cargo or create worktrees. If either member is missing, rerun `performance-release`; do not combine files from
different comparisons.

Review the complete diff, then stage the actual release artifacts and commit them manually:

```bash
git status --short
git diff --check
git diff

git add Cargo.toml Cargo.lock pyproject.toml uv.lock CITATION.cff CHANGELOG.md README.md docs/
git commit -m "chore(release): release $TAG"
git push -u origin "release/$TAG"
```

Open a PR titled `chore(release): release $TAG`. Keep feature work and ordinary dependency updates out of this PR.

If a release-critical fix lands on the branch, rerun `just update-version "$TAG"` and `just changelog-unreleased "$TAG"`, then repeat every release gate.

## 3. Publish after merge

Synchronize to the exact merged `main`:

```bash
git switch main
git pull --ff-only
just release-version-check
```

Create and inspect the annotated tag:

```bash
just tag "$TAG"
git tag -l --format='%(contents)' "$TAG"
git push origin "$TAG"
```

Publish the locked crate and create the GitHub release:

```bash
cargo publish --locked
gh release create "$TAG" --title "$TAG" --notes-from-tag
```

Publishing the GitHub release triggers `.github/workflows/release-benchmarks.yml`. Wait for that workflow to pass and verify that
`causal-triangulations-$TAG-criterion-baseline.tar.gz` is attached to the release. The workflow checks out the exact tag, reruns the correctness-gated
release signal, embeds source and host provenance, and publishes the native Criterion archive through a separate least-privilege job. To reconstruct a
comparison from two published assets without Cargo or worktrees, run `just performance-github-assets "$TAG" "$PREVIOUS_TAG"`.

Verify the Zenodo record through the permanent concept DOI in `CITATION.cff`. The citation file must continue to contain the concept DOI rather than a
release-record DOI; Zenodo resolves the concept record to the latest release while retaining the version history.

Only after the tag, crates.io package, GitHub release, benchmark assets, and Zenodo record are verified should the release branch be removed:

```bash
git push origin --delete "release/$TAG"
git branch -d "release/$TAG"
```

## Reruns and failure recovery

- A same-day rerun of `just update-version "$TAG"` is content-idempotent.
- A rerun on a later UTC day intentionally advances `date-released` and the matching changelog heading.
- Version preparation writes nothing unless every planned file passes validation.
- An interrupted multi-file write restores the original bytes, including original newline style.
- Dependency updates and performance measurements are separate operations and have their own rollback or evidence rules.
