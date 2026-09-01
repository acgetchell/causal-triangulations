"""Update deterministic release-version references from one target Git tag."""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from datetime import UTC, date, datetime
from pathlib import Path
from typing import TYPE_CHECKING, TypeAlias, TypeGuard

import check_release_metadata as release_check
from subprocess_utils import ExecutableNotFoundError, run_safe_command

if TYPE_CHECKING:
    from collections.abc import Callable

_STABLE_TAG_RE = re.compile(r"^v(?P<major>0|[1-9][0-9]*)\.(?P<minor>0|[1-9][0-9]*)\.(?P<patch>0|[1-9][0-9]*)$")
_TOML_VERSION_RE = re.compile(r'^(?P<prefix>\s*version\s*=\s*")(?P<version>[^"]+)(?P<suffix>"\s*(?:#.*)?)$')
_CITATION_VERSION_RE = re.compile(
    r"^(?P<prefix>version:\s*(?P<quote>['\"]?))"
    r"(?P<version>[0-9A-Za-z][0-9A-Za-z.+-]*)"
    r"(?P<suffix>(?P=quote)\s*(?:#.*)?)$"
)
_CITATION_DATE_RE = re.compile(
    r"^(?P<prefix>date-released:\s*(?P<quote>['\"]?))"
    r"(?P<date>\d{4}-\d{2}-\d{2})"
    r"(?P<suffix>(?P=quote)\s*(?:#.*)?)$"
)
_IDENTIFIERS_START_RE = re.compile(r"^identifiers:\s*(?:#.*)?$")
_BENCHMARK_TAG_PAIR_RE = re.compile(
    r"(?P<prefix>just performance-(?:github-assets|release)[ \t]+)"
    r"(?P<current>v[0-9]+\.[0-9]+\.[0-9]+)"
    r"(?P<separator>[ \t]+)"
    r"(?P<baseline>v[0-9]+\.[0-9]+\.[0-9]+)"
    r"(?=[ \t\r]|\x60|$)",
    re.MULTILINE,
)

# Semgrep 1.175 cannot parse the Python 3.12 `type` statement in strict scans.
ParsedObject: TypeAlias = dict[str, object]  # noqa: UP040


@dataclass(frozen=True, order=True, slots=True)
class ReleaseTag:
    """A stable release tag with SemVer ordering."""

    major: int
    minor: int
    patch: int
    tag: str = field(compare=False)

    def __post_init__(self) -> None:
        """Reject direct construction with contradictory tag components."""
        match = _STABLE_TAG_RE.fullmatch(self.tag)
        if match is None:
            msg = f"release tag must be a stable tag in vX.Y.Z form, got {self.tag!r}"
            raise ValueError(msg)
        parsed = (int(match.group("major")), int(match.group("minor")), int(match.group("patch")))
        supplied = (self.major, self.minor, self.patch)
        if parsed != supplied:
            msg = f"release tag components {supplied} contradict emitted tag {self.tag!r} with components {parsed}"
            raise ValueError(msg)

    @property
    def version(self) -> str:
        """Return the package version without the leading v."""
        return self.tag.removeprefix("v")


@dataclass(frozen=True, slots=True)
class UpdateSummary:
    """Files and release identities produced by one update."""

    target: ReleaseTag
    previous: ReleaseTag
    changed_paths: tuple[Path, ...]
    release_date: str


@dataclass(frozen=True, slots=True)
class LineReplacement:
    """One fail-closed scalar replacement on a known source line."""

    line_number: int
    pattern: re.Pattern[str]
    group: str
    replacement: str
    allowed: frozenset[str]
    context: str


def _is_parsed_object(value: object) -> TypeGuard[ParsedObject]:
    """Return whether JSON data is an object with string keys."""
    return isinstance(value, dict) and all(isinstance(key, str) for key in value)


def parse_release_tag(value: str, *, label: str = "release tag") -> ReleaseTag:
    """Parse one stable vX.Y.Z release tag."""
    match = _STABLE_TAG_RE.fullmatch(value)
    if match is None:
        msg = f"{label} must be a stable tag in vX.Y.Z form, got {value!r}"
        raise ValueError(msg)
    return ReleaseTag(
        major=int(match.group("major")),
        minor=int(match.group("minor")),
        patch=int(match.group("patch")),
        tag=value,
    )


def select_previous_release_tag(tag_names: list[str], target: ReleaseTag) -> ReleaseTag:
    """Select the newest published stable release before target."""
    stable_tags = [parse_release_tag(tag) for tag in tag_names if _STABLE_TAG_RE.fullmatch(tag) is not None]
    if not stable_tags:
        msg = "repository has no published stable vX.Y.Z GitHub releases"
        raise ValueError(msg)
    newer = [tag for tag in stable_tags if tag > target]
    if newer:
        latest = max(newer)
        msg = f"target {target.tag} is older than published stable GitHub release {latest.tag}"
        raise ValueError(msg)
    previous = [tag for tag in stable_tags if tag < target]
    if not previous:
        msg = f"could not find a published stable GitHub release before {target.tag}"
        raise ValueError(msg)
    return max(previous)


def published_stable_release_tags(root: Path) -> list[str]:
    """Return stable tags from published, non-draft GitHub Releases."""
    result = run_safe_command(
        "gh",
        ["release", "list", "--limit", "100", "--json", "tagName,isDraft,isPrerelease,publishedAt"],
        cwd=root,
    )
    data: object = json.loads(result.stdout)
    if not isinstance(data, list):
        msg = "expected GitHub release list to be a JSON array"
        raise TypeError(msg)
    tags: list[str] = []
    seen: set[str] = set()
    for index, raw_release in enumerate(data):
        if not _is_parsed_object(raw_release):
            msg = f"GitHub release entry {index} must be an object"
            raise TypeError(msg)
        tag = raw_release.get("tagName")
        is_draft = raw_release.get("isDraft")
        is_prerelease = raw_release.get("isPrerelease")
        published_at = raw_release.get("publishedAt")
        if not isinstance(tag, str) or not isinstance(is_draft, bool) or not isinstance(is_prerelease, bool):
            msg = f"GitHub release entry {index} has malformed tag/draft/prerelease fields"
            raise TypeError(msg)
        if tag in seen:
            msg = f"duplicate GitHub release tag {tag!r}"
            raise ValueError(msg)
        seen.add(tag)
        if is_draft or is_prerelease or _STABLE_TAG_RE.fullmatch(tag) is None:
            continue
        if not isinstance(published_at, str) or not published_at:
            msg = f"published GitHub release {tag!r} is missing publishedAt"
            raise ValueError(msg)
        tags.append(tag)
    return tags


def infer_previous_release_tag(root: Path, target: ReleaseTag) -> ReleaseTag:
    """Infer the previous version from published stable GitHub Releases."""
    return select_previous_release_tag(published_stable_release_tags(root), target)


def _current_utc_date() -> str:
    """Return today's canonical UTC calendar date."""
    return datetime.now(UTC).date().isoformat()


def _validated_date(value: str) -> str:
    """Require one canonical real ISO calendar date."""
    try:
        parsed = date.fromisoformat(value)
    except ValueError as error:
        msg = f"release date must use YYYY-MM-DD, got {value!r}"
        raise ValueError(msg) from error
    if parsed.isoformat() != value:
        msg = f"release date must use canonical YYYY-MM-DD form, got {value!r}"
        raise ValueError(msg)
    return value


def _read_text(path: Path) -> str:
    """Read UTF-8 text without normalizing newline sequences."""
    with path.open(encoding="utf-8", newline="") as stream:
        return stream.read()


def _replace_line_group(text: str, edit: LineReplacement) -> str:
    """Replace one validated capture group while preserving its newline."""
    lines = text.splitlines(keepends=True)
    if not 1 <= edit.line_number <= len(lines):
        msg = f"{edit.context} has no line {edit.line_number}"
        raise ValueError(msg)
    original_line = lines[edit.line_number - 1]
    body = original_line.rstrip("\r\n")
    ending = original_line[len(body) :]
    match = edit.pattern.fullmatch(body)
    if match is None:
        msg = f"{edit.context}:{edit.line_number} has an unsupported scalar assignment: {body!r}"
        raise ValueError(msg)
    current = match.group(edit.group)
    if current not in edit.allowed:
        msg = f"{edit.context}:{edit.line_number} has unexpected value {current!r}; expected one of {sorted(edit.allowed)}"
        raise ValueError(msg)
    start, end = match.span(edit.group)
    lines[edit.line_number - 1] = f"{body[:start]}{edit.replacement}{body[end:]}{ending}"
    return "".join(lines)


def _replace_match_groups(match: re.Match[str], replacements: dict[str, str]) -> str:
    """Replace named groups in one regex match without rebuilding context."""
    updated = match.group(0)
    spans = sorted(
        ((match.start(group) - match.start(), match.end(group) - match.start(), value) for group, value in replacements.items()),
        reverse=True,
    )
    for start, end, value in spans:
        updated = f"{updated[:start]}{value}{updated[end:]}"
    return updated


def _remove_version_identifiers(text: str, path: Path) -> str:
    """Remove the one legacy version-specific DOI block without data loss."""
    lines = text.splitlines(keepends=True)
    starts = [index for index, line in enumerate(lines) if _IDENTIFIERS_START_RE.fullmatch(line.rstrip("\r\n")) is not None]
    if not starts:
        return text
    if len(starts) != 1:
        msg = f"{path}: expected at most one top-level identifiers block; found {len(starts)}"
        raise ValueError(msg)
    start = starts[0]
    end = start + 1
    while end < len(lines):
        body = lines[end].rstrip("\r\n")
        if body and not body[0].isspace() and not body.startswith("#"):
            break
        end += 1
    block = "".join(lines[start:end])
    significant = [line for line in block.splitlines() if line.strip() and not line.lstrip().startswith("#")]
    recognized = (
        len(significant) == 4
        and _IDENTIFIERS_START_RE.fullmatch(significant[0]) is not None
        and re.fullmatch(r"  - type:\s*(?:['\"])?doi(?:['\"])?\s*(?:#.*)?", significant[1]) is not None
        and re.fullmatch(r"    value:\s*(?:['\"])?10\.5281/zenodo\.[0-9]+(?:['\"])?\s*(?:#.*)?", significant[2]) is not None
        and re.fullmatch(
            r"    description:\s*(?:['\"])?Zenodo DOI for version [0-9]+\.[0-9]+\.[0-9]+(?:['\"])?\s*(?:#.*)?",
            significant[3],
        )
        is not None
    )
    if not recognized:
        msg = f"{path}: refusing to remove an unrecognized identifiers block"
        raise ValueError(msg)
    return "".join((*lines[:start], *lines[end:]))


def _replace_dependency_versions(text: str, package_name: str, target: ReleaseTag, previous: ReleaseTag, path: Path) -> str:
    """Advance active Cargo dependency snippets only from allowed versions."""
    allowed = frozenset({target.version, previous.version})

    def replace(match: re.Match[str]) -> str:
        group = next(name for name in ("plain", "plain_literal", "table", "table_literal") if match.group(name) is not None)
        current = match.group(group)
        if current not in allowed:
            msg = f"{path} has unexpected {package_name} dependency version {current!r}; expected one of {sorted(allowed)}"
            raise ValueError(msg)
        return _replace_match_groups(match, {group: target.version})

    return release_check.dependency_regex(package_name).sub(replace, text)


def _replace_cargo_add_versions(text: str, package_name: str, target: ReleaseTag, previous: ReleaseTag, path: Path) -> str:
    """Advance active cargo-add examples only from allowed versions."""
    allowed = frozenset({target.version, previous.version})

    def replace(match: re.Match[str]) -> str:
        current = match.group("version")
        if current not in allowed:
            msg = f"{path} has unexpected cargo add version {current!r}; expected one of {sorted(allowed)}"
            raise ValueError(msg)
        return _replace_match_groups(match, {"version": target.version})

    return release_check.cargo_add_regex(package_name).sub(replace, text)


def _replace_readme_links(text: str, target: ReleaseTag, previous: ReleaseTag, path: Path) -> str:
    """Advance non-performance release-pinned README links."""
    allowed = frozenset({target.version, previous.version})

    def replace(match: re.Match[str]) -> str:
        if release_check.readme_tag_link_is_performance_asset(match):
            return match.group(0)
        version = match.group("version")
        if version is not None and version not in allowed:
            msg = f"{path} has unexpected release-pinned link version {version!r}; expected one of {sorted(allowed)}"
            raise ValueError(msg)
        group = "version" if version is not None else "revision"
        replacement = target.version if version is not None else target.tag
        return _replace_match_groups(match, {group: replacement})

    return release_check.README_TAG_LINK_RE.sub(replace, text)


def _replace_benchmark_tag_pairs(text: str, target: ReleaseTag, previous: ReleaseTag, path: Path) -> str:
    """Keep explicit release-performance examples on the adjacent pair."""
    allowed_current = frozenset({target.tag, previous.tag})

    def replace(match: re.Match[str]) -> str:
        current = match.group("current")
        if current not in allowed_current:
            msg = f"{path} has unexpected benchmark current tag {current}; expected {target.tag} or {previous.tag}"
            raise ValueError(msg)
        return _replace_match_groups(match, {"current": target.tag, "baseline": previous.tag})

    return _BENCHMARK_TAG_PAIR_RE.sub(replace, text)


def _metadata_updates(root: Path, target: ReleaseTag, previous: ReleaseTag, release_date: str) -> dict[Path, str]:
    """Build every package and citation metadata edit in memory."""
    allowed = frozenset({target.version, previous.version})
    cargo_toml = root / "Cargo.toml"
    cargo_lock = root / "Cargo.lock"
    pyproject = root / "pyproject.toml"
    uv_lock = root / "uv.lock"
    citation = root / "CITATION.cff"
    package = release_check.read_cargo_package_info(cargo_toml)
    project = release_check.read_python_project_info(pyproject)
    cargo_lock_ref = release_check.cargo_lock_reference(cargo_lock, package)
    pyproject_ref = release_check.pyproject_reference(pyproject, project)
    uv_lock_ref = release_check.uv_lock_reference(uv_lock, project)
    citation_ref = release_check.citation_reference(citation)
    citation_date = release_check.citation_release_date(citation)
    updates = {
        cargo_toml: _replace_line_group(
            _read_text(cargo_toml),
            LineReplacement(
                release_check.toml_table_key_line(cargo_toml, "package", "version"),
                _TOML_VERSION_RE,
                "version",
                target.version,
                allowed,
                str(cargo_toml),
            ),
        ),
        cargo_lock: _replace_line_group(
            _read_text(cargo_lock),
            LineReplacement(cargo_lock_ref.line, _TOML_VERSION_RE, "version", target.version, allowed, str(cargo_lock)),
        ),
        pyproject: _replace_line_group(
            _read_text(pyproject),
            LineReplacement(pyproject_ref.line, _TOML_VERSION_RE, "version", target.version, allowed, str(pyproject)),
        ),
        uv_lock: _replace_line_group(
            _read_text(uv_lock),
            LineReplacement(uv_lock_ref.line, _TOML_VERSION_RE, "version", target.version, allowed, str(uv_lock)),
        ),
        citation: _replace_line_group(
            _read_text(citation),
            LineReplacement(citation_ref.line, _CITATION_VERSION_RE, "version", target.version, allowed, str(citation)),
        ),
    }
    updates[citation] = _replace_line_group(
        updates[citation],
        LineReplacement(
            citation_date.line,
            _CITATION_DATE_RE,
            "date",
            release_date,
            frozenset({citation_date.value, release_date}),
            str(citation),
        ),
    )
    updates[citation] = _remove_version_identifiers(updates[citation], citation)
    return updates


def _replace_changelog_release_date(
    changelog: Path,
    target: ReleaseTag,
    *,
    line: int,
    current_date: str,
    release_date: str,
) -> str:
    """Return a changelog with one target heading date synchronized."""
    heading_re = re.compile(rf"^(?P<prefix>## \[v?{re.escape(target.version)}\](?:\([^)]+\))? - )(?P<date>\d{{4}}-\d{{2}}-\d{{2}})$")
    return _replace_line_group(
        _read_text(changelog),
        LineReplacement(
            line,
            heading_re,
            "date",
            release_date,
            frozenset({current_date, release_date}),
            str(changelog),
        ),
    )


def _prepare_updates(root: Path, target: ReleaseTag, previous: ReleaseTag, release_date: str) -> dict[Path, str]:
    """Plan metadata and active-documentation edits without writing."""
    updates = _metadata_updates(root, target, previous, release_date)
    changelog = root / "CHANGELOG.md"
    changelog_match = release_check.changelog_release_date(changelog, target.version)
    if changelog_match is not None:
        updates[changelog] = _replace_changelog_release_date(
            changelog,
            target,
            line=changelog_match.line,
            current_date=changelog_match.value,
            release_date=release_date,
        )
    package = release_check.read_cargo_package_info(root / "Cargo.toml")
    for path in release_check.iter_active_markdown_files(root):
        original = _read_text(path)
        updated = _replace_dependency_versions(original, package.name, target, previous, path)
        updated = _replace_cargo_add_versions(updated, package.name, target, previous, path)
        updated = _replace_benchmark_tag_pairs(updated, target, previous, path)
        if path == root / "README.md":
            updated = _replace_readme_links(updated, target, previous, path)
        updates[path] = updated
    return updates


def _validate_updated_root(root: Path, target: ReleaseTag, previous: ReleaseTag) -> None:
    """Validate the synchronized tree and explicit benchmark pair."""
    release_check.validate_release_metadata(root)
    for path in release_check.iter_active_markdown_files(root):
        for match in _BENCHMARK_TAG_PAIR_RE.finditer(_read_text(path)):
            if match.group("current") != target.tag or match.group("baseline") != previous.tag:
                msg = f"{path} contains a benchmark tag pair that does not match {target.tag} against {previous.tag}"
                raise ValueError(msg)


def _validate_planned_updates(root: Path, updates: dict[Path, str], target: ReleaseTag, previous: ReleaseTag) -> None:
    """Validate a complete candidate tree before replacing repository files."""
    sources = {
        root / "Cargo.toml",
        root / "Cargo.lock",
        root / "pyproject.toml",
        root / "uv.lock",
        root / "CITATION.cff",
        root / "scripts" / "README.md",
        *release_check.iter_active_markdown_files(root),
    }
    changelog = root / "CHANGELOG.md"
    if changelog.is_file():
        sources.add(changelog)
    with tempfile.TemporaryDirectory(prefix="cdt-release-plan-") as temporary_directory:
        candidate_root = Path(temporary_directory)
        for source in sources:
            destination = candidate_root / source.relative_to(root)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(updates.get(source, _read_text(source)), encoding="utf-8", newline="")
        _validate_updated_root(candidate_root, target, previous)


def _write_bytes_atomic(path: Path, content: bytes) -> None:
    """Replace one file atomically with exact bytes and its original mode."""
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
        temporary.chmod(path.stat().st_mode)
        temporary.replace(path)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def _write_text_atomic(path: Path, text: str) -> None:
    """Atomically write UTF-8 text."""
    _write_bytes_atomic(path, text.encode("utf-8"))


def _publish_transaction(updates: dict[Path, str], validate: Callable[[], None]) -> tuple[Path, ...]:
    """Publish a multi-file update with byte-exact rollback."""
    original_bytes = {path: path.read_bytes() for path in updates}
    original_text = {path: _read_text(path) for path in updates}
    changed = tuple(sorted((path for path, text in updates.items() if text != original_text[path]), key=str))
    replaced: list[Path] = []
    try:
        for path in changed:
            _write_text_atomic(path, updates[path])
            replaced.append(path)
        validate()
    except BaseException as primary:
        rollback_errors: list[str] = []
        for path in reversed(replaced):
            try:
                _write_bytes_atomic(path, original_bytes[path])
            except OSError as error:
                rollback_errors.append(f"{path}: {error}")
        if rollback_errors:
            msg = f"release-version update failed ({primary}); rollback also failed: {'; '.join(rollback_errors)}"
            raise RuntimeError(msg) from primary
        raise
    return changed


def update_release_version(
    root: Path,
    tag: str,
    *,
    previous: ReleaseTag | None = None,
    release_date: str | None = None,
) -> UpdateSummary:
    """Update release references transactionally and return a summary."""
    resolved_root = root.resolve()
    target = parse_release_tag(tag, label="target tag")
    previous_release = previous or infer_previous_release_tag(resolved_root, target)
    if previous_release >= target:
        msg = f"previous release {previous_release.tag} must be older than target {target.tag}"
        raise ValueError(msg)
    prepared_date = _validated_date(release_date or _current_utc_date())
    updates = _prepare_updates(resolved_root, target, previous_release, prepared_date)
    _validate_planned_updates(resolved_root, updates, target, previous_release)
    changed = _publish_transaction(updates, lambda: _validate_updated_root(resolved_root, target, previous_release))
    return UpdateSummary(target=target, previous=previous_release, changed_paths=changed, release_date=prepared_date)


def sync_changelog_release_date(root: Path, tag: str) -> tuple[tuple[Path, ...], str]:
    """Synchronize a generated target heading from CITATION.cff."""
    resolved_root = root.resolve()
    target = parse_release_tag(tag, label="target tag")
    package = release_check.read_cargo_package_info(resolved_root / "Cargo.toml")
    if package.version != target.version:
        msg = f"Cargo.toml version {package.version} does not match target {target.tag}"
        raise ValueError(msg)
    citation_date = release_check.citation_release_date(resolved_root / "CITATION.cff")
    changelog = resolved_root / "CHANGELOG.md"
    changelog_match = release_check.changelog_release_date(changelog, target.version)
    if changelog_match is None:
        msg = f"{changelog} has no generated release heading for {target.tag}"
        raise ValueError(msg)
    updated = _replace_changelog_release_date(
        changelog,
        target,
        line=changelog_match.line,
        current_date=changelog_match.value,
        release_date=citation_date.value,
    )
    changed = _publish_transaction(
        {changelog: updated},
        lambda: release_check.validate_release_metadata(resolved_root, final_release=True),
    )
    return changed, citation_date.value


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="Target stable release tag in vX.Y.Z form")
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="Repository root to update (default: current directory)")
    parser.add_argument(
        "--sync-changelog-date",
        action="store_true",
        help="Synchronize the generated changelog heading from CITATION.cff instead of updating metadata",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Update deterministic release metadata with fail-closed diagnostics."""
    args = parse_args(argv)
    try:
        if args.sync_changelog_date:
            changed_paths, release_date = sync_changelog_release_date(args.root, args.tag)
            if changed_paths:
                print(f"Synchronized CHANGELOG.md release date to {release_date}.")
            else:
                print(f"CHANGELOG.md release date already matches {release_date}.")
            return 0
        summary = update_release_version(args.root, args.tag)
    except (
        ExecutableNotFoundError,
        json.JSONDecodeError,
        OSError,
        RuntimeError,
        subprocess.SubprocessError,
        TypeError,
        ValueError,
        tomllib.TOMLDecodeError,
    ) as error:
        print(f"failed to update release version: {error}", file=sys.stderr)
        return 1
    if summary.changed_paths:
        for path in summary.changed_paths:
            print(f"Updated {path.relative_to(args.root.resolve())}")
    else:
        print(f"Release-version references already match {summary.target.tag}.")
    print(f"Previous release: {summary.previous.tag}")
    print(f"CITATION.cff release date: {summary.release_date} (UTC update date)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
