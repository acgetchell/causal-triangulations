# Releasing causal-triangulations

## Contents

- [Prepare the environment](#prepare-the-environment)
- [Step 1: Prepare the release PR](#step-1-prepare-the-release-pr)
  - [Prepare metadata](#create-the-branch-and-prepare-the-metadata)
  - [Acquire benchmarks](#acquire-benchmarks-and-review-the-readme)
  - [Validate and commit](#validate-and-commit-the-candidate)
  - [Review and open the PR](#open-and-merge-the-release-pr)
- [Step 2: Publish after the PR is merged](#step-2-publish-after-the-pr-is-merged)
  - [Wait for main CI](#synchronize-main-and-wait-for-ci)
  - [Accept the evidence and tag](#accept-the-merged-commit-and-create-the-tag)
  - [Stage and inspect the draft](#wait-for-the-draft-and-review-its-assets)
  - [Publish the GitHub Release](#publish-the-reviewed-draft)
  - [Approve and verify the crates.io upload](#verify-registry-publication)
- [Recovering a failed run](#recovering-a-failed-run)

Follow the [RRT release process][rrt-releasing] for release preparation, PR review, tagging, draft publication and registry verification.
CDT adds a scientific benchmark acquisition, reviews its signed Cargo evidence before tagging, and requires a protected crates.io upload.
If a run stops, [resume from the retained evidence](#recovering-a-failed-run).

Use the [release tracker][release-issues] for the milestone's scientific claims, approvals, run links and checksums.
Compare those claims with the [scientific basis](scientific-basis.md) and [roadmap](roadmap.md) before opening the release PR.

[rrt-releasing]: https://github.com/acgetchell/research-repo-tools/blob/main/docs/RELEASING.md
[release-issues]: https://github.com/acgetchell/causal-triangulations/issues?q=is%3Aissue+label%3Arelease

## Prepare the environment

Start on a clean, current `main` with the [development environment](../CONTRIBUTING.md#getting-started) and an authenticated GitHub CLI.
Use the same published RRT distribution for the project and isolated release jobs. Land dependency and tool updates before starting release preparation.
Set the intended tag once per shell:

```bash
TAG=vX.Y.Z
```

## Step 1: Prepare the release PR

### Create the branch and prepare the metadata

```bash
git switch -c "release/$TAG"
just update-version "$TAG"
just changelog-release "$TAG"
```

Review the Cargo metadata, lockfile, citation date, installation examples and generated changelog. The updater looks up the previous stable release.
The changelog reads the prepared date from `CITATION.cff`.

### Acquire benchmarks and review the README

```bash
just release-dependency-builds
just performance-release
```

The first command prepares dependency receipts. The second includes RRT's release dependency check, prepares the measurement and runs the scientific
gates. The first release in a series reports absolute timings and uncertainty. A compatible successor uses paired trials.
Review the workload inventory, uncertainty, host and compiler before interpreting the [report](BENCHMARKING.md#fresh-release-evidence).

```bash
just performance-readme --preview
just performance-readme
```

Review the README changes alongside the promoted report and raw run records. Back up the external acquisition directory printed by
`just release-status`. Record its acquisition and publication digests in the release tracker.

### Validate and commit the candidate

```bash
just release-version-check
```

Inspect and stage the intended release files, including the generated outputs, then commit:

```bash
git --no-pager diff --cached --check
git --no-pager diff --cached
git commit -m "chore(release): prepare $TAG"
```

### Open and merge the release PR

These checks verify the original measurement, the additional scientific test and both Cargo packages on the final, clean release commit.

```bash
just release-preflight
just test-slow
just publish-check
```

RRT and CDT require the same three gates before pushing.

```bash
just security
just ci
just review origin/main
```

Resolve any findings before pushing. When the candidate changes, repeat the affected preparation steps and run the push gates on the final changes.

```bash
git push -u origin "release/$TAG" &&
  gh pr create --base main --head "release/$TAG" --title "chore(release): release $TAG"
```

Link the tracker, scientific review and local results in the PR. Merge after the required hosted checks and CodeRabbit review pass. Keep the PR number.

## Step 2: Publish after the PR is merged

### Synchronize main and wait for CI

Set `RELEASE_PR` to the merged PR number and start in a clean checkout. The PR identifies the merge commit and the **CI** run to watch.

```bash
git switch main
git pull --ff-only origin main
RELEASE_COMMIT=$(gh pr view "${RELEASE_PR:?Set the release PR number}" --json state,baseRefName,mergeCommit \
  --jq 'select(.state == "MERGED" and .baseRefName == "main") | .mergeCommit.oid')
MAIN_RUN_ID=$(gh run list --workflow ci.yml --event push --branch main --commit "${RELEASE_COMMIT:?Check the merged PR}" \
  --limit 2 --json databaseId --jq 'select(length == 1) | .[0].databaseId')
gh run watch "${MAIN_RUN_ID:?Expected one main-push CI run for the reviewed commit}" --exit-status
```

If main has advanced, run `git switch --detach "$RELEASE_COMMIT"` in the clean checkout. Wait for the required OSV, Gitleaks, CodeQL, Clippy SARIF,
repository-rule SARIF and workflow security checks on that SHA. For a missing or ambiguous run, see [CI recovery](#missing-ci-or-interrupted-draft-preparation).

### Accept the merged commit and create the tag

Review the signed crate and the original benchmark acquisition before creating the tag.

```bash
just release-evidence
```

Check the printed SHA, CI run and attempt, signed crate checksum and benchmark acquisition. The main-push evidence covers repository tooling, the three
native target jobs, the library-only build, slow tests, both clean Cargo package checks and the all-feature packaged public docs.
Record these identities before creating the tag.

```bash
just tag "$TAG" --commit "$RELEASE_COMMIT"
```

Inspect the printed annotation and commit, then push the tag explicitly:

```bash
git push origin "refs/tags/$TAG"
```

### Wait for the draft and review its assets

Wait for **Prepare GitHub release** to succeed on the original tag push. `just release-stage` calls `release-draft` to capture the authenticated draft
target and checks it against the native evidence you accepted before tagging. It then stages the crate and benchmark assets.

```bash
just release-stage
just release-benchmarks "$TAG"
```

Once **Release Benchmarks** verifies the staged attachment, inspect the draft title, notes and asset digests.

```bash
gh release view "$TAG"
gh release view "$TAG" --json assets --jq '.assets[] | [.name, .state, .size, .digest] | @tsv'
```

Check the title, generated notes, signed crate, benchmark archive, policy and acceptance against the retained records.
To inspect the authenticated target on its own after draft preparation finishes, run `just release-draft "$TAG"`.

### Publish the reviewed draft

After inspecting the inventory, ensure all draft writers have finished and retain exclusive control through confirmation:

```bash
just release-review "$TAG" release-publish "$TAG"
```

In one invocation, Just validates the original candidate and draft receipt once. The review freezes the verified inventory digest. Publication
checks current source, required checks and asset identities immediately before changing the release and verifies the response.
To pause after review, run the recipes separately. On resumption, publication revalidates the original inputs.

### Verify registry publication

Wait for **Publish to crates.io** to verify the published event, reproduce the signed crate and retain its receipt. From the same reviewed checkout:

```bash
just release-upload "$TAG"
```

The dispatched run appears under the reviewed tag and commit.

```bash
gh run list --workflow publish-crates.yml --event workflow_dispatch --branch "$TAG" --commit "$RELEASE_COMMIT" --json displayTitle,url
```

Open the run titled `Publish <tag> to crates.io` and wait for `verify` to pass. Under **Review deployments**, select **crates-io**, choose
**Approve and deploy**, and wait for `publish` to succeed. The publisher rechecks the original benchmark, native
selection, tool identities, exact Cargo bytes and registry absence before obtaining a short-lived upload token. Cargo packages the same source again
when it uploads. Record the original run and attempt in the [release tracker][release-issues]. After upload:

```bash
just release-verify "$TAG"
```

Verify that the registry checksum matches the immutable GitHub crate asset. Test ordinary resolution from a fresh directory:

```bash
consumer=$(mktemp -d)
dependency="causal-triangulations@=${TAG#v}"
cargo new --quiet --vcs none --name cdt_release_smoke "$consumer/consumer"
cargo add --manifest-path "$consumer/consumer/Cargo.toml" "$dependency"
cargo check --locked --manifest-path "$consumer/consumer/Cargo.toml"
```

Record the registry, downstream check and Zenodo version reached from the concept DOI in `CITATION.cff` before closing the release tracker.

## Recovering a failed run

Start in the prepared checkout, set `TAG` again and inspect the local inventory:

```bash
just release-status
```

The default owner is `$HOME/.local/state/causal-triangulations/$TAG`, inferred from the prepared Cargo version. If you set `CDT_RELEASE_OWNER` during
acquisition, restore that same absolute path. File presence helps locate the last step. Each recovery command authenticates the evidence it consumes.

### Metadata or source corrections

For offline metadata preparation, pass the independently confirmed stable tag to `just update-version "$TAG" --previous-release "$PREVIOUS_TAG"`.
To deliberately change the citation date, use `just update-version "$TAG" --date YYYY-MM-DD` and rerun `just changelog-release "$TAG"`.
Check the staged diff after an interruption. Restore a backup or repeat preparation after reconciling partial edits.

If preflight reports differences, inspect the named paths. Regenerate declared outputs from the retained acquisition, review and commit the corrections,
then repeat the clean preflight. For changes to measured source, harness, inputs or other bound files, preserve the original checkout and owner.
Prepare a fresh checkout, choose a fresh external owner with `CDT_RELEASE_OWNER`, regenerate the notes and repeat Step 1.

### Interrupted benchmark or promotion

For an interrupted measurement with an intact checkpoint, resume the same acquisition:

```bash
just performance-resume
```

If acquisition used a custom owner directory, pass the original checkpoint and owner as `just performance-resume "$CHECKPOINT" "$OWNER"`.
For an interrupted capture or promotion, retain the original scratch payload and manifest. Use the same owner with `performance-capture`.

```bash
just performance-capture
just performance-readme --preview
just performance-readme
just performance-check
```

Restore missing original evidence from its independent backup, checking the retained digests. Keep partial or conflicting records for inspection.

### Missing CI or interrupted draft preparation

Restore `RELEASE_COMMIT` from the reviewed merge PR and inspect the main-push runs for that SHA.

```bash
gh run list --workflow ci.yml --event push --branch main --commit "$RELEASE_COMMIT" --json databaseId,attempt,status,conclusion,url
```

Inspect the original run and its exact attempt. Artifacts have 30-day retention. Pending jobs must finish. A new complete attempt requires
fresh review of the SHA, signer, artifact IDs and digests. Preserve the old selection. If the original push can no longer be rerun, use a newly reviewed
commit. PR runs, partial reruns and manual dispatches cannot supply main-push evidence.

For an interrupted draft, locate the original tag-push run by its workflow, tag and commit.

```bash
gh run list --workflow prepare-release.yml --event push --branch "$TAG" --commit "$RELEASE_COMMIT" --json databaseId,attempt,status,conclusion,url
```

For a failed tag-push draft writer, retain the original handoff, journal and target. Inspect the exact release ID after any uncertain write.
Use the [authenticated draft recovery procedure][draft-recovery] before continuing. Keep the original tag and draft during diagnosis.

### Interrupted staging, publication or registry visibility

If you need the original target path, run `just release-draft "$TAG"` after the tag-push workflow succeeds.
Retry `just release-stage` against the same draft and journal. It reconciles the original IDs and bytes. Retry `just release-benchmarks "$TAG"`
to verify the staged attachment. Once all writers finish, inspect the draft again before running review and publication together.

If GitHub publication times out, inspect the captured release ID, lifecycle, immutable state, notes and asset digests. Resume draft publication only
after confirming the original draft is still unpublished and intact. If the published-event check failed, rerun its complete workflow before
dispatching from the retained owner.

If dispatch verification rejected incorrect inputs, restore the originally approved evidence and inspect the prior runs. Pass the latest failed
dispatch ID to request a corrected approval:

```bash
just release-upload "$TAG" "$FAILED_RUN_ID"
```

Recovery requires every prior attempt to have failed in `verify` and skipped `publish`. It checks registry absence before dispatch.
If any attempt reached the publisher, inspect that original upload. If Cargo times out after upload, the publisher succeeds when the registry
exposes the reviewed crate checksum.
If that check fails, run `just release-verify "$TAG"` and inspect the original upload. Confirm absence independently before rerunning all jobs of that
dispatch. The regular upload command detects an existing dispatch. An unexpected checksum or conflicting release identity requires maintainer reconciliation.

[draft-recovery]: https://github.com/acgetchell/research-repo-tools/blob/main/docs/PREPARING_DRAFTS.md#retry-and-recovery
