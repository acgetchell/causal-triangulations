# Releasing causal-triangulations

## Contents

- [Step 1: Prepare the release PR](#step-1-prepare-the-release-pr)
  - [Prepare the environment](#1-prepare-the-environment)
  - [Update metadata and generate notes](#2-prepare-the-release-branch-and-metadata)
  - [Run the release checks](#3-run-the-release-checks)
  - [Run the slow scientific test](#4-run-the-slow-scientific-test)
  - [Acquire benchmarks and update the README](#5-acquire-benchmarks-and-update-the-readme)
  - [Commit the candidate](#6-commit-the-candidate)
  - [Validate the release commit](#7-validate-the-release-commit)
  - [Run the push gates](#8-run-the-push-gates)
  - [Open and merge the release PR](#9-open-and-merge-the-release-pr)
- [Step 2: Publish after the PR is merged](#step-2-publish-after-the-pr-is-merged)
  - [Wait for main CI](#1-synchronize-main-and-wait-for-ci)
  - [Accept the evidence and push the tag](#2-accept-the-evidence-and-push-the-annotated-tag)
  - [Stage the prepared draft](#3-stage-the-prepared-draft)
  - [Review the draft](#4-review-the-draft)
  - [Publish the GitHub Release](#5-publish-the-github-release)
  - [Dispatch and approve publishing to crates.io](#6-dispatch-and-approve-publishing-to-cratesio)
  - [Verify the published crate](#7-verify-the-published-crate)

Use this procedure to retain the release benchmarks and signed crate, publish the GitHub Release and approve the crates.io upload.

Merge library and tool updates in a separate PR before starting the release. Run `just update` on the maintenance branch. Use the reviewed library
versions and pinned `uv` version throughout the release. If an update runs during the release,
[recover the pinned environment](RECOVERING_RELEASES.md#recover-after-an-unintended-update).

Use the milestone's [release tracker][release-issues] to identify its scientific acceptance criteria. Compare the claims with the
[scientific basis](scientific-basis.md) and [roadmap](roadmap.md). Verify results in the workflow runs and generated reports for the release commit.

[release-issues]: https://github.com/acgetchell/causal-triangulations/issues?q=is%3Aissue+label%3Arelease

## Step 1: Prepare the release PR

### 1. Prepare the environment

Start on a clean, current `main` with the [development environment](../CONTRIBUTING.md#getting-started). Verify the pinned tools with `just tools-check`
and GitHub CLI access with `gh auth status`. Use the same published version of `research-repo-tools` for the project and isolated release jobs.
Set `TAG` in Bash or Zsh, replacing `X.Y.Z` with the intended version. Use it throughout the release.

```bash
TAG=vX.Y.Z
```

If setup or GitHub access fails, [recover the release environment](RECOVERING_RELEASES.md#recover-the-release-environment).

### 2. Prepare the release branch and metadata

```bash
git switch -c "release/$TAG"
just update-version "$TAG"
just changelog-release "$TAG"
```

Review the Cargo metadata, lockfile, citation date, installation examples and generated changelog. The updater looks up the previous stable release.
The changelog reads the prepared date from `CITATION.cff`.
If branch creation or preparation stops, [recover the branch, metadata and notes](RECOVERING_RELEASES.md#recover-metadata-and-notes).

### 3. Run the release checks

```bash
just release-dependency-builds
just release-dependencies
just release-version-check
```

These commands prepare dependency receipts, verify published upstream dependencies and check the package version and dated notes.
If a check fails, [recover the release checks](RECOVERING_RELEASES.md#recover-the-release-checks).

### 4. Run the slow scientific test

Run the toroidal stress test on the prepared working tree.

```bash
just test-slow
```

If the test fails or stops, [recover the slow scientific test](RECOVERING_RELEASES.md#recover-the-slow-scientific-test). Start the benchmarks once it passes.

### 5. Acquire benchmarks and update the README

```bash
just performance-release
```

The first release in a series reports absolute timings and uncertainty. A compatible successor uses paired trials.
Review the workload inventory, uncertainty, host and compiler before interpreting the [report](BENCHMARKING.md#fresh-release-evidence).

```bash
just performance-readme --preview
just performance-readme
```

Review the README changes alongside the promoted report and raw run records. Back up the external acquisition directory printed by
`just release-status`, including its acquisition and publication digests.
If acquisition or promotion stops, [resume from the original evidence](RECOVERING_RELEASES.md#resume-benchmarks-or-report-promotion).

### 6. Commit the candidate

Stage the reviewed release files, including the generated outputs. Use the commit subject below so later changelog generation skips this commit.

```bash
git --no-pager diff --cached --check
git --no-pager diff --cached
git commit -m "chore(release): prepare $TAG"
```

If the staged diff check or commit fails, [recover the candidate commit](RECOVERING_RELEASES.md#recover-the-candidate-commit).

### 7. Validate the release commit

Use the clean release commit to verify the original benchmark evidence and check both Cargo packages.

```bash
just release-preflight
just publish-check
```

For v0.2.0, run the [packaged external 1+1 consumer test](VALIDATING.md#test-the-release-candidate) on the same reviewed source.
Record the package identity, contract version and report in [#335](https://github.com/acgetchell/causal-triangulations/issues/335) before tagging.

If preflight or packaging fails, [recover the release commit checks](RECOVERING_RELEASES.md#recover-the-release-commit-checks).

### 8. Run the push gates

Run these gates on the final release commit so they cover the full outgoing changes, including the benchmark report and README update.

```bash
just security
just ci
just review origin/main
```

If a gate fails or review is incomplete, [recover the push gates](RECOVERING_RELEASES.md#recover-the-push-gates) before pushing.

### 9. Open and merge the release PR

Once the push gates pass, push the branch and open the PR.

```bash
git push -u origin "release/$TAG" &&
  gh pr create --base main --head "release/$TAG" --title "chore(release): release $TAG"
```

Link the tracker, scientific review and local results in the PR. Merge after the required hosted checks and CodeRabbit review pass. Keep the PR number.
If the push, PR creation or merge is interrupted, [inspect the existing branch and PR](RECOVERING_RELEASES.md#recover-the-release-pr).

## Step 2: Publish after the PR is merged

### 1. Synchronize main and wait for CI

From a clean checkout, update your local `main` after the release PR merges.

```bash
git switch main && git pull --ff-only origin main
```

In **Actions → CI**, open the run triggered by the push to `main` for the merge commit shown on the release PR. Wait for **Success**, including the
Linux, macOS and Windows builds, Cargo packaging and signing. Confirm the run's commit matches `git --no-pager rev-parse HEAD`. If synchronization fails
or `main` has advanced, [check out the release commit](RECOVERING_RELEASES.md#check-out-the-release-commit).

Wait for the required OSV, Gitleaks, CodeQL, Clippy SARIF, repository-rule SARIF and workflow security checks on that same commit. If the CI run is
missing or ambiguous, [inspect the original CI evidence](RECOVERING_RELEASES.md#recover-signed-ci-evidence).

### 2. Accept the evidence and push the annotated tag

Review the signed crate and the original benchmark acquisition before creating the tag.

```bash
just release-evidence
```

Check the printed SHA, CI run and attempt, signed crate checksum and benchmark acquisition. The main-push evidence covers repository tooling, the three
native target jobs, the library-only build, slow tests, both clean Cargo package checks and the all-feature packaged public docs.
Before creating the tag, [download and back up the signed CI artifact](RECOVERING_RELEASES.md#preserve-the-original-signed-evidence).
If the artifact is unavailable or the selected attempt changed, [recover the signed CI evidence](RECOVERING_RELEASES.md#recover-signed-ci-evidence).

```bash
just tag "$TAG"
```

If tag creation fails, [inspect the tag and evidence state](RECOVERING_RELEASES.md#reconcile-the-tag-push).

Inspect the printed annotation and commit, then push the tag explicitly:

```bash
git push origin "refs/tags/$TAG"
```

If the push result is uncertain, [reconcile the tag push](RECOVERING_RELEASES.md#reconcile-the-tag-push).

### 3. Stage the prepared draft

Wait for **Prepare GitHub release** to succeed on the original tag push. `just release-stage` calls `release-draft` to capture the authenticated draft
target and checks it against the native evidence you accepted before tagging. It then stages the crate and benchmark assets.

```bash
just release-stage
just release-benchmarks "$TAG"
```

Wait for **Release Benchmarks** to verify the staged attachment. If a run stops, follow the recovery procedure for
[draft preparation](RECOVERING_RELEASES.md#recover-draft-preparation) or [staging](RECOVERING_RELEASES.md#recover-staging-and-inspect-the-draft).

### 4. Review the draft

Open the draft in GitHub Releases. Review its title and notes, then compare the signed crate, benchmark archive, policy and acceptance with the retained
records. Use the [draft asset digest check](RECOVERING_RELEASES.md#inspect-the-draft-asset-digests) for that comparison.

### 5. Publish the GitHub Release

After inspecting the inventory, ensure all draft writers have finished and retain exclusive control through confirmation:

```bash
just release-review "$TAG" release-publish "$TAG"
```

In one invocation, Just validates the original candidate and draft receipt once. The review freezes the verified inventory digest. Publication
checks current source, required checks and asset identities immediately before changing the release and verifies the response.
To pause after review, run the recipes separately. On resumption, publication revalidates the original inputs.
If either command fails or the result is uncertain, [recover GitHub publication](RECOVERING_RELEASES.md#recover-github-publication).

### 6. Dispatch and approve publishing to crates.io

Wait for **Publish to crates.io** to verify the published event, reproduce the signed crate and retain its receipt. If this check fails,
[recover the original published-event run](RECOVERING_RELEASES.md#recover-github-publication). From the same reviewed checkout:

```bash
just release-upload "$TAG"
```

In **Actions → Publish to crates.io**, open the run titled `Publish <tag> to crates.io` for the reviewed tag and commit. Wait for `verify` to pass.
Under **Review deployments**, select **crates-io**, choose
**Approve and deploy**, and wait for `publish` to succeed. The publisher rechecks the original benchmark, native
selection, tool identities, exact Cargo bytes and registry absence before obtaining a short-lived upload token. Cargo packages the same source again
when it uploads.
If dispatch, approval, or upload stops, [recover crates.io publication](RECOVERING_RELEASES.md#recover-cratesio-publication).

### 7. Verify the published crate

```bash
just release-verify "$TAG"
```

Verify that the registry checksum matches the immutable GitHub crate asset. If verification is inconclusive,
[inspect the original upload and registry state](RECOVERING_RELEASES.md#recover-cratesio-publication). Test ordinary resolution from a fresh directory:

```bash
consumer=$(mktemp -d)
dependency="causal-triangulations@=${TAG#v}"
cargo new --quiet --vcs none --name cdt_release_smoke "$consumer/consumer"
cargo add --manifest-path "$consumer/consumer/Cargo.toml" "$dependency"
cargo check --locked --manifest-path "$consumer/consumer/Cargo.toml"
```

Verify the Zenodo version reached from the concept DOI in `CITATION.cff`. Close the release tracker once its publication and verification criteria pass.
For a failed consumer check or a missing Zenodo record, [resume downstream verification](RECOVERING_RELEASES.md#recover-downstream-verification).
