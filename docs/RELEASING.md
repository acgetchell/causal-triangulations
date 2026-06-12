# Releasing causal-triangulations

This guide documents the release flow for `vX.Y.Z`: prepare a dedicated release PR, merge it, create the final annotated tag from the generated changelog,
publish to crates.io, and create the GitHub release.

The release changelog is generated with `git-cliff --tag` through `just changelog-unreleased`, so no temporary local tag is needed.

Prefer updating release-facing documentation before publishing to crates.io. README, `docs/`, notebook guidance, and citation metadata are versioned with the
release artifacts that reviewers and downstream users will see.

---

## Conventions and environment

Set these variables to avoid repeating the version string:

```bash
# tag has the leading v, version does not
TAG=vX.Y.Z
VERSION=${TAG#v}
```

Verify your git remotes:

```bash
git remote -v
```

Ensure your local `main` is up to date before beginning:

```bash
git switch main
git pull --ff-only
```

---

## Step 1: Create a clean release PR

This PR should primarily include version bumps, changelog updates, citation metadata updates, and documentation updates. All major code changes should already
be on `main`.

Small, critical fixes discovered during the release process may be included, but keep them minimal and release-critical.

Update release-facing documentation on this PR branch before publishing. Do not defer README, `docs/`, notebook, or citation fixes until after the release:
crates.io, docs.rs, and GitHub release readers will see the merged release artifacts.

1. Create the release branch

```bash
git switch -c "release/$TAG"
```

2. Bump versions

Preferred, if `cargo-edit` is installed:

```bash
cargo set-version "$VERSION"
```

Alternative: edit `Cargo.toml` manually and update `version = "..."` under `[package]`.

Update release metadata to match the crate version:

- `CITATION.cff`: update `version` and `date-released`

Review version references in documentation and metadata:

```bash
rg -n "\bv?[0-9]+\.[0-9]+\.[0-9]+\b" README.md docs/ CITATION.cff pyproject.toml || true
```

3. Generate the release changelog

```bash
# Generates CHANGELOG.md as though TAG already exists, then applies
# markdown hygiene and archives completed minor release series.
just changelog-unreleased "$TAG"
```

`just changelog-unreleased` runs `GIT_CLIFF_OFFLINE=true git-cliff --tag "$TAG" -o CHANGELOG.md`, then `postprocess-changelog`, then
`archive-changelog`. The root changelog keeps Unreleased plus the active minor series; older completed minor series live under `docs/archive/changelog/`.

4. Validate the release branch

```bash
just ci
just publish-check
```

5. Stage and commit release artifacts

```bash
git add Cargo.toml Cargo.lock CITATION.cff CHANGELOG.md README.md docs/ notebooks/

git commit -m "chore(release): release $TAG

- Bump version to $TAG
- Update citation metadata
- Update changelog with latest changes
- Update documentation for release"
```

6. Push the branch and open a PR

```bash
git push -u origin "release/$TAG"
```

PR metadata:

- Title: `chore(release): release $TAG`
- Description: Clean release PR with version bump, changelog, citation metadata, and documentation updates. No feature work.

### Handling fixes discovered during the release process

If you discover issues after generating the changelog:

1. For critical fixes that must be in this release, make and commit the fix, then regenerate the release changelog:

   ```bash
   just changelog-unreleased "$TAG"
   git add CHANGELOG.md docs/archive/changelog/
   git commit -m "docs: update changelog with release fixes"
   ```

2. For non-critical fixes, document them as known issues in the release notes or include them in the next release.

---

## Step 2: After the PR is merged into main

1. Sync your local `main` to the merge commit

```bash
git switch main
git pull --ff-only
```

2. Create the final annotated tag using the changelog content

```bash
# Creates the annotated tag from the matching CHANGELOG.md section.
# Archived versions are read from docs/archive/changelog/ automatically.
# For large changelogs (>125KB), the tag message points to the changelog
# section instead of embedding the full content.
just tag "$TAG"
```

3. Optional: verify the tag message content

```bash
git tag -l --format='%(contents)' "$TAG"
```

4. Push the tag

```bash
git push origin "$TAG"
```

5. Publish to crates.io

```bash
cargo publish --locked
```

6. Create the GitHub release with notes from the tag annotation

```bash
gh release create "$TAG" --title "$TAG" --notes-from-tag
```

Always set the GitHub release title to the exact tag string, including the leading `v`.

7. Clean up the release branch

```bash
git push origin --delete "release/$TAG"
git branch -d "release/$TAG"
```

---

## Notes and tips

- Do not create a temporary local release tag for changelog generation; use `just changelog-unreleased "$TAG"`.
- Keep the release PR scoped to version, changelog, archive, citation metadata, and documentation changes.
- `just changelog` regenerates the current changelog from existing tags and may update `docs/archive/changelog/`.
- `just changelog-unreleased "$TAG"` is for release PR preparation before the final tag exists.
- `just tag "$TAG"` is for the final post-merge annotated tag.
- If multiple files reference the version, confirm all of them are updated consistently.
