# Releasing causal-triangulations

This guide documents the exact commands for performing a clean release using a dedicated release PR, followed by tagging, publishing to crates.io, and creating a GitHub release.

Applies to versions vX.Y.Z. Prefer updating documentation before publishing to crates.io.

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

Ensure your local main is up to date before beginning:

```bash
git checkout main
git pull --ff-only
```

---

## Step 1: Create a clean release PR

This PR should primarily include: version bumps, changelog updates, and documentation updates. All major code changes should already be on main.

**Exception:** Small, critical fixes discovered during the release process (e.g., documentation errors, script bugs, formatting issues) may be included but should be minimal and release-critical only.

1. Create the release branch

```bash
git checkout -b release/$TAG
```

2. Bump versions

Preferred (if cargo-edit is installed):

```bash
# Bump package version in Cargo.toml
cargo set-version $VERSION
```

Alternative: edit `Cargo.toml` manually (update `version = "..."` under `[package]`).

Update references in documentation (search, then manually edit as needed):

```bash
# List occurrences of version-like strings to review
rg -n "\bv?[0-9]+\.[0-9]+\.[0-9]+\b" README.md docs/ || true
```

3. Generate changelog using a temporary local tag (DO NOT PUSH this tag)

```bash
# Create a temporary annotated tag locally to enable changelog generation
# Do not push this tag; it will be recreated later after merge
git tag -a "$TAG" -m "causal-triangulations $TAG"

# Generate changelog using the Python-based tool
just changelog
```

4. Stage and commit release artifacts

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md docs/

git commit -m "chore(release): release $TAG

- Bump version to $TAG
- Update changelog with latest changes
- Update documentation for release"
```

5. Push the branch and open a PR

```bash
git push -u origin "release/$TAG"
```

PR metadata:

- Title: chore(release): release $TAG
- Description: Clean release PR with version bump, changelog, and documentation updates. No code changes.

Note: Do NOT push the temporary tag created in step 3.

### Handling fixes discovered during release process

If you discover issues (bugs, formatting problems, etc.) after creating the changelog:

1. **For critical fixes that must be in this release:**

   ```bash
   # Make your fixes
   # Run code quality tools
   # Commit the fixes
   git add .
   git commit -m "fix: [description of fix]"

   # Delete the temporary tag and regenerate changelog
   git tag -d "$TAG"
   git tag -a "$TAG" -m "causal-triangulations $TAG"
   just changelog

   # Commit updated changelog
   git add CHANGELOG.md
   git commit -m "docs: update changelog with release fixes"
   ```

2. **For non-critical fixes:**
   - Document them as known issues in the release notes
   - Include them in the next release
   - This avoids the changelog regeneration loop

---

## Step 2: After the PR is merged into main

1. Sync your local main to the merge commit

```bash
git checkout main
git pull --ff-only
```

2. Recreate the final annotated tag using the changelog content

```bash
# Remove the temporary local tag if it exists
git tag -d "$TAG" 2>/dev/null || true

# Create the final annotated tag with the changelog section as the tag message
# Note: For large changelogs (>125KB), this automatically creates an annotated tag
# with a reference message pointing to CHANGELOG.md instead of the full content
just changelog-tag "$TAG"
```

3. (Optional) Verify tag message content

```bash
git tag -l --format='%(contents)' "$TAG"
```

4. Push the tag

```bash
git push origin "$TAG"
```

5. Create the GitHub release with notes from the tag annotation

```bash
# Requires GitHub CLI (gh) and authenticated session
gh release create "$TAG" --notes-from-tag
```

6. Publish to crates.io

```bash
# Sanity check before publishing
cargo publish --dry-run

# Publish the crate (ensure docs are already updated on main via the PR)
cargo publish
```

---

## Kani verification before release

Before tagging, verify that formal proofs still pass:

```bash
# Fast verification (recommended minimum before release)
just kani-fast

# Full verification (recommended for major/minor releases)
just kani
```

---

## Notes and tips

- Never push the temporary tag created for changelog generation; only push the final tag after the PR is merged.
- Keep the release PR strictly to version + changelog + documentation to maintain a clean history.
- If multiple crates or files reference the version, confirm all of them are updated consistently.
- Run `just ci` before opening the release PR to catch any issues.
- For future convenience, parts of this document can be automated into a release script.
