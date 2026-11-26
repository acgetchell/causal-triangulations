# Recovering an unfinished release

Follow the [release guide](RELEASING.md) for the normal procedure. To resume, use the original prepared checkout and set `TAG` again. Keep the original
checkpoint, evidence, tag, and draft while you determine where the run stopped.

## Choose a recovery path

```bash
just release-status
```

The default owner directory is `$HOME/.local/state/causal-triangulations/$TAG`, inferred from the prepared Cargo version. If acquisition used
`CDT_RELEASE_OWNER`, restore that same absolute path. `release-status` lists local records and prints the captured release ID if available. Check GitHub
and crates.io separately for any operation whose outcome is uncertain.

| Observed state | Recovery procedure |
| --- | --- |
| Setup or GitHub CLI access failed | [Recover the release environment](#recover-the-release-environment) |
| `uv` changed or `just update` ran during the release | [Recover after an unintended update](#recover-after-an-unintended-update) |
| Release branch already exists or metadata and notes are incomplete | [Recover metadata and notes](#recover-metadata-and-notes) |
| Dependency or version checks failed | [Recover the release checks](#recover-the-release-checks) |
| Slow scientific test failed or stopped | [Recover the slow scientific test](#recover-the-slow-scientific-test) |
| Benchmarking, promotion, or README rendering stopped | [Resume benchmarks or report promotion](#resume-benchmarks-or-report-promotion) |
| Staged diff check or candidate commit failed | [Recover the candidate commit](#recover-the-candidate-commit) |
| Preflight or review found a difference after acquisition | [Correct the candidate after acquisition](#correct-the-candidate-after-acquisition) |
| Preflight or Cargo packaging failed on the release commit | [Recover the release commit checks](#recover-the-release-commit-checks) |
| Security, CI, or local review is incomplete | [Recover the push gates](#recover-the-push-gates) |
| Release branch push, PR creation, or merge was interrupted | [Recover the release PR](#recover-the-release-pr) |
| Synchronizing `main` failed or `main` has advanced | [Check out the release commit](#check-out-the-release-commit) |
| Main CI is pending, ambiguous, or missing its signed artifact | [Recover signed CI evidence](#recover-signed-ci-evidence) |
| Tag creation failed or push was interrupted | [Reconcile the tag push](#reconcile-the-tag-push) |
| Draft preparation stopped | [Recover draft preparation](#recover-draft-preparation) |
| Staging stopped or draft assets differ | [Recover staging and inspect the draft](#recover-staging-and-inspect-the-draft) |
| GitHub publication or published-event verification failed | [Recover GitHub publication](#recover-github-publication) |
| Registry approval, dispatch, upload, or verification stopped | [Recover crates.io publication](#recover-cratesio-publication) |
| Registry checksum agrees, but the consumer check or Zenodo record is pending | [Recover downstream verification](#recover-downstream-verification) |

## Recover the release environment

For missing tools or a version mismatch, follow [development setup](../CONTRIBUTING.md#getting-started), open a new shell if setup changed `PATH`, and run
`just tools-check`. If `gh auth status` fails, restore the maintainer account's GitHub CLI access and repeat that check. Preserve any existing edits and
prepare a clean checkout for the release.

If tools or dependencies changed during an active release, [restore the reviewed environment](#recover-after-an-unintended-update).

Resume at [prepare the environment](RELEASING.md#1-prepare-the-environment) to confirm the checkout and `TAG`. If both are ready, continue at
[prepare the release branch and metadata](RELEASING.md#2-prepare-the-release-branch-and-metadata).

## Recover after an unintended update

Pause the release and inspect the installed `uv` version and changes to the checkout.

```bash
uv --version
git --no-pager status --short
git --no-pager diff HEAD -- Cargo.toml Cargo.lock pyproject.toml uv.lock
```

Reconcile any changes against the intended release state, including prepared version metadata. Restore the reviewed dependency requirements, lockfiles,
and tool pins if the updater changed them. Use the exact `uv` version from `required-version` in the reviewed `pyproject.toml` on `PATH`.
[Version-specific uv installation](https://docs.astral.sh/uv/getting-started/installation/#standalone-installer)

Confirm the restored `uv --version`, synchronize the pinned environment with `just setup` if needed, then run `just tools-check`. If the restored inputs
differ from the recorded acquisition, [reconcile the candidate against the retained evidence](#correct-the-candidate-after-acquisition).

Once the environment and candidate verify, resume at the first unfinished substep in [PR preparation](RELEASING.md#step-1-prepare-the-release-pr) or
[publication](RELEASING.md#step-2-publish-after-the-pr-is-merged). If an operation's outcome is uncertain, [choose a recovery path](#choose-a-recovery-path)
for that operation before retrying it.

## Recover metadata and notes

If `release/$TAG` already exists, confirm it belongs to this release before reusing it. On the intended branch, resume at the first unfinished
command in [prepare the release branch and metadata](RELEASING.md#2-prepare-the-release-branch-and-metadata).

After a partial edit, inspect the working tree and staged diff. Restore a backup or repeat preparation after reconciling the edits. For offline metadata
preparation, pass the independently confirmed stable tag to `just update-version "$TAG" --previous-release "$PREVIOUS_TAG"`. To deliberately change the
citation date, use `just update-version "$TAG" --date YYYY-MM-DD`. If the metadata is ready, rerun `just changelog-release "$TAG"` from the prepared branch.

Resume with the [release checks](RELEASING.md#3-run-the-release-checks) once the metadata and generated notes agree.

## Recover the release checks

- If `just release-dependency-builds` fails, inspect the failed build or tool setup. After restoring its prerequisite, rerun it so the receipts match the
  prepared dependency graph.
- If `just release-dependencies` fails, check the locked dependency and supplier versions and the failing receipt. Restore registry access or the receipt as
  appropriate. Land any required dependency update separately before restarting release preparation.
- If `just release-version-check` fails, [reconcile the metadata and notes](#recover-metadata-and-notes).

Return to the failing command in [run the release checks](RELEASING.md#3-run-the-release-checks). Repeat any earlier checks affected by a correction.
Once all checks pass on the prepared candidate, continue at [run the slow scientific test](RELEASING.md#4-run-the-slow-scientific-test).

## Recover the slow scientific test

For an interrupted run, rerun `just test-slow` on the prepared working tree. For a failed assertion, investigate and fix the cause with the same test
inputs and expected behavior. Repeat any affected [release checks](RELEASING.md#3-run-the-release-checks) and rerun
[the slow test](RELEASING.md#4-run-the-slow-scientific-test) on the corrected candidate.

Once it passes, continue at [acquire benchmarks and update the README](RELEASING.md#5-acquire-benchmarks-and-update-the-readme).

## Resume benchmarks or report promotion

For an interrupted measurement with an intact checkpoint, resume the same acquisition.

```bash
just performance-resume
```

For a custom owner directory, pass the original checkpoint and owner as `just performance-resume "$CHECKPOINT" "$OWNER"`. If capture or promotion stopped,
keep the original scratch payload and manifest. Restore missing evidence from its independent backup, verifying its digests. Keep partial or conflicting
records for inspection. In the acquisition checkout, recover capture or promotion using the same owner.

```bash
just performance-capture
```

Render and validate the README and report from the retained measurements.

```bash
just performance-readme --preview
just performance-readme
just performance-check
```

After reviewing the recovered outputs, resume at [commit the candidate](RELEASING.md#6-commit-the-candidate).

## Recover the candidate commit

Inspect `git --no-pager status --short` and the latest commit before retrying an interrupted commit. If the candidate commit exists with the intended
files and the working tree is clean, continue at [validate the release commit](RELEASING.md#7-validate-the-release-commit).

If the commit was rejected, inspect the staged diff and the reported whitespace or hook failure. For a tracked correction, follow
[candidate correction](#correct-the-candidate-after-acquisition) to determine whether the benchmark evidence can be reused. Stage the reviewed result and
return to the staged diff checks in [commit the candidate](RELEASING.md#6-commit-the-candidate).

## Correct the candidate after acquisition

Inspect the paths named by preflight or review. For a correction limited to declared generated outputs, use the original acquisition checkout and retained
records to [regenerate the outputs](#resume-benchmarks-or-report-promotion). Review and commit the corrections, then resume at
[validate the release commit](RELEASING.md#7-validate-the-release-commit) on the clean candidate.

For a change to measured source, the harness, or another input bound to the acquisition, preserve the original checkout and owner. Prepare a fresh checkout
with the intended corrections and a fresh external `CDT_RELEASE_OWNER`. Resume at [prepare the release PR](RELEASING.md#step-1-prepare-the-release-pr)
and acquire new benchmark evidence for the corrected inputs.

## Recover the release commit checks

If `just release-preflight` cannot find the acquisition records, use `just release-status` to confirm the original owner and checkout. Restore missing
records from the independent backup and verify their digests. If preflight reports a difference,
[reconcile the candidate](#correct-the-candidate-after-acquisition).

If `just publish-check` fails, inspect the Cargo diagnostic and confirm the release commit is clean. Restore the failed prerequisite or follow
[candidate correction](#correct-the-candidate-after-acquisition) if the package needs a tracked change.

Return to the failed command in [validate the release commit](RELEASING.md#7-validate-the-release-commit). Rerun both checks if the commit changed.
Once both pass for the final commit, continue at [run the push gates](RELEASING.md#8-run-the-push-gates).

## Recover the push gates

For a failure in `just security` or `just ci`, inspect the failing scan or test and resolve the cause. For an incomplete `just review origin/main`, use the
[local CodeRabbit review guidance](dev/DEVELOPING.md#local-coderabbit-review) to diagnose the base, authentication, or review result.

If a correction changes tracked files, [reconcile the candidate against the acquisition](#correct-the-candidate-after-acquisition), repeat validation,
then run all three [push gates](RELEASING.md#8-run-the-push-gates) on the corrected commit. For a transient failure, return to the failed or incomplete
gate if the candidate and review base still match the earlier results. Once all three have passed for the final outgoing changes, continue at
[open and merge the release PR](RELEASING.md#9-open-and-merge-the-release-pr).

## Recover the release PR

If a push or PR command was interrupted, compare the remote branch with the reviewed local SHA, then look for an existing PR for that branch and commit.

```bash
git --no-pager rev-parse HEAD
git --no-pager ls-remote origin "refs/heads/release/$TAG"
gh pr list --head "release/$TAG" --base main --state all --json number,state,headRefOid,url | cat
```

- If the branch is absent and the PR has not been created, return to the push in [open and merge the release PR](RELEASING.md#9-open-and-merge-the-release-pr).
- If the branch already matches and the PR is absent, run only the PR creation command in [that substep](RELEASING.md#9-open-and-merge-the-release-pr).
- If the matching PR is open, resume at the hosted checks and review in [that substep](RELEASING.md#9-open-and-merge-the-release-pr). For a correction,
  inspect the failure and follow [candidate correction](#correct-the-candidate-after-acquisition) and [push-gate recovery](#recover-the-push-gates).
- If the matching PR is merged, continue at [synchronize main and wait for CI](RELEASING.md#1-synchronize-main-and-wait-for-ci) using that PR's merge SHA.

If a lookup fails, the branch points at an unexpected commit, or the matching PR was closed without merging, inspect the existing state before another write.

## Check out the release commit

If a switch or fast-forward fails, inspect the local checkout and its relationship to `origin/main`. Preserve local edits and commits and use a clean
checkout with access to the reviewed merge commit. If `main` has advanced, set `RELEASE_COMMIT` to the full merge SHA shown on the release PR and switch
to that commit. If GitHub access fails, [restore the release environment](#recover-the-release-environment) first.

```bash
git switch --detach "$RELEASE_COMMIT"
```

With that SHA checked out, resume at the GitHub Actions check in [wait for main CI](RELEASING.md#1-synchronize-main-and-wait-for-ci).

## Recover signed CI evidence

Set `RELEASE_COMMIT` to the full merge commit SHA shown on the release PR. Inspect the original main-push run and its exact attempt.

```bash
gh run list --workflow ci.yml --event push --branch main --commit "$RELEASE_COMMIT" --json databaseId,attempt,status,conclusion,url | cat
```

GitHub retains the signed Cargo artifact for 30 days. Let an eligible pending run finish. A PR run, manual dispatch, or failed-jobs-only rerun cannot supply
complete main-push evidence.

A complete CI rerun produces a new build record. Reuse the retained benchmark acquisition while its provenance still verifies for the candidate.

- **Before evidence acceptance.** If the original main-push run can be rerun, use **Re-run all jobs**. Wait for the complete attempt on the same SHA.
  Review it as new evidence and record the signer, run, attempt, artifact ID and digests. A matching crate checksum establishes that the bytes match.
  The attempt and attestation still have their own identities. Resume at [accept the evidence](RELEASING.md#2-accept-the-evidence-and-push-the-annotated-tag).
- **After evidence acceptance.** Preserve the existing `selection.json`, `acceptance.json`, and any receipts issued from them. The release recipes recheck
  the selected attempt and artifact against GitHub. A backup preserves the evidence for audit. If the original hosted evidence is unavailable or the attempt
  has changed, pause at the affected release substep for maintainer reconciliation. Keep any existing tag and draft with the original selection. If access
  is restored and the accepted attempt verifies, return to the interrupted substep in [publish after merge](RELEASING.md#step-2-publish-after-the-pr-is-merged).

If the original run can no longer be completed and no evidence has been accepted, use a newly reviewed release commit and follow
[candidate correction](#correct-the-candidate-after-acquisition) for any effects on the benchmark inputs. Resume at
[wait for main CI](RELEASING.md#1-synchronize-main-and-wait-for-ci) for the new merge SHA.

### Preserve the original signed evidence

In GitHub Actions, open the accepted CI run and download `cargo-package-<run>-<attempt>` from **Artifacts**. Its ZIP contains the `.crate`,
`validation.json`, and `validation.sigstore.json`. Compare the ZIP's SHA-256 with `artifact.digest` in the retained `selection.json`.
If download or checksum verification fails, [inspect the original artifact and attempt](#recover-signed-ci-evidence) before tagging.

Back up the verified ZIP, `selection.json`, and `acceptance.json` outside the release owner directory. Record the commit, run and attempt, signer, artifact
ID, ZIP SHA-256, and crate SHA-256. Keep the benchmark acquisition, raw samples, and acquisition and publication digests with their own provenance.

Once the backup verifies, resume at tag creation in [accept the evidence and push the tag](RELEASING.md#2-accept-the-evidence-and-push-the-annotated-tag).
If a tag already exists, [reconcile it](#reconcile-the-tag-push) first.

## Reconcile the tag push

If tagging fails while revalidating the acceptance, [recover the signed CI evidence](#recover-signed-ci-evidence). If the tag may already exist, compare
the remote annotated tag object and its peeled commit with the local tag and reviewed SHA.

```bash
git --no-pager rev-parse "refs/tags/$TAG" "refs/tags/$TAG^{}"
git --no-pager ls-remote origin "refs/tags/$TAG" "refs/tags/$TAG^{}"
```

If both identities match and the tag resolves to the accepted commit, resume at [stage the prepared draft](RELEASING.md#3-stage-the-prepared-draft).
If the remote lookup succeeds and the tag is absent, return to [create or push the tag](RELEASING.md#2-accept-the-evidence-and-push-the-annotated-tag).
Use the push command directly if the correct local annotated tag already exists. Hold for reconciliation if the lookup fails or the identities differ.

## Recover draft preparation

Set `RELEASE_COMMIT` to the full reviewed merge SHA. Locate the original tag-push run by its workflow, tag, and commit.

```bash
gh run list --workflow prepare-release.yml --event push --branch "$TAG" --commit "$RELEASE_COMMIT" --json databaseId,attempt,status,conclusion,url | cat
```

For a failed writer, retain the original handoff, journal and captured target. Inspect the exact release ID after any uncertain write. Use the
[authenticated draft recovery procedure][draft-recovery] for reconciliation. Once the original draft receipt is available and verifies, resume at
[stage the prepared draft](RELEASING.md#3-stage-the-prepared-draft).

## Recover staging and inspect the draft

If you need the original target path, run `just release-draft "$TAG"` after preparation succeeds. For interrupted staging, retry `just release-stage`
against the same draft and journal. It reconciles the original asset IDs and bytes. Inspect any existing **Release Benchmarks** run for the same tag,
commit and staged transfer before dispatching again. Wait for a pending run. If verification still needs to run, use `just release-benchmarks "$TAG"`
with the original transfer. Once verification succeeds, resume at [review the draft](RELEASING.md#4-review-the-draft).

### Inspect the draft asset digests

Set `RELEASE_ID` to the numeric release ID in the captured `target.json`. Compare the numeric REST asset IDs, upload states, sizes, and digests
with the retained transfer and evidence records.

```bash
gh api --paginate "repos/{owner}/{repo}/releases/$RELEASE_ID/assets" \
  --jq '.[] | [.id, .name, .state, .size, .digest] | @tsv'
```

If an asset differs, keep the draft unpublished and [recover staging](#recover-staging-and-inspect-the-draft) against the original target and journal.
When the title, notes, and assets agree with the retained records and all draft writers have finished, resume at
[publish the GitHub Release](RELEASING.md#5-publish-the-github-release).

## Recover GitHub publication

If `just release-review` or `just release-publish` reports a failed prerequisite, follow the diagnostic to the
[required checks on the release SHA](RELEASING.md#1-synchronize-main-and-wait-for-ci), [signed evidence](#recover-signed-ci-evidence), or
[staged assets](#inspect-the-draft-asset-digests). When the original draft and all prerequisites verify, return to
[publish the GitHub Release](RELEASING.md#5-publish-the-github-release).

If publication disconnected or returned an uncertain result, set `RELEASE_ID` to the numeric ID in the captured `target.json`. Inspect that release's
state, notes, and asset digests before another write.

```bash
gh api "repos/{owner}/{repo}/releases/$RELEASE_ID" \
  --jq '{id, tag_name, draft, prerelease, immutable, body, assets: [.assets[] | {id, name, state, size, digest}]}'
```

- If the original mutable draft is intact, return to [review the draft](RELEASING.md#4-review-the-draft) before repeating publication.
- If the original release is immutable and matches the reviewed inventory, inspect the published-event verification. If it failed, rerun the complete
  original workflow once the cause is fixed and the original evidence still verifies. When verification passes, resume at
  [dispatch publishing to crates.io](RELEASING.md#6-dispatch-and-approve-publishing-to-cratesio). If a dispatch already exists,
  [inspect that dispatch](#recover-cratesio-publication) to resume from its current state.
- If the release ID or inventory conflicts, preserve the records and hold for maintainer reconciliation.

## Recover crates.io publication

Inspect the original dispatch and the exact registry version before requesting another upload. Keep the original reviewed transfer and acceptance. If
the publisher is awaiting approval, resume at [approve the existing deployment](RELEASING.md#6-dispatch-and-approve-publishing-to-cratesio) using that
dispatch run.

If verification rejected incorrect dispatch inputs, restore the originally approved inputs and inspect all prior attempts. A corrected dispatch requires
every prior attempt to have failed in `verify` and skipped `publish`. Set `FAILED_RUN_ID` to the latest failed dispatch ID. The recipe checks registry
absence before dispatching.

```bash
just release-upload "$TAG" "$FAILED_RUN_ID"
```

Return to [review and approve the new dispatch](RELEASING.md#6-dispatch-and-approve-publishing-to-cratesio) after corrective dispatch succeeds.

If an attempt reached the publisher, inspect that original upload and use `just release-verify "$TAG"`. Cargo may time out after a successful upload.
If the registry exposes the reviewed checksum, resume at [verify the published crate](RELEASING.md#7-verify-the-published-crate) for the downstream check.
If the original upload has stopped and you independently confirm registry absence, rerun all jobs of that original dispatch and return to
[deployment approval](RELEASING.md#6-dispatch-and-approve-publishing-to-cratesio). An unexpected checksum or conflicting release identity requires
maintainer reconciliation.

## Recover downstream verification

If the registry checksum agrees and the fresh consumer check failed, keep the diagnostic and verify the exact crate pin and supported Rust toolchain.
After resolving a local tool or registry access failure, resume at the fresh Cargo consumer commands in
[verify the published crate](RELEASING.md#7-verify-the-published-crate). If the verified package fails in a supported fresh consumer, record the failure
in the release tracker and prepare the correction under a new version.

If the consumer check already passed, keep its result and resume at the Zenodo check in [verify the published crate](RELEASING.md#7-verify-the-published-crate).
Check for the version under the concept DOI in `CITATION.cff`. Leave the release tracker open while the version record is missing or incorrect.

[draft-recovery]: https://github.com/acgetchell/research-repo-tools/blob/main/docs/PREPARING_DRAFTS.md#retry-and-recovery
