#!/usr/bin/env -S uv run
"""Archive completed minor series from CHANGELOG.md into per-minor files.

Parses the full CHANGELOG.md (produced by git-cliff + postprocess-changelog)
into version blocks, groups them by minor series (X.Y), and writes:

  - ``docs/archive/changelog/X.Y.md`` for each completed minor series
  - A trimmed ``CHANGELOG.md`` containing only the preamble, Unreleased,
    the active minor series, and an Archives link section

The active minor is detected from the first tagged release heading after
Unreleased.  All other minors are archived.

Usage:
    archive-changelog                      # default: CHANGELOG.md
    archive-changelog path/to/CHANGELOG.md
    archive-changelog --archive-dir docs/archive/changelog
"""

import argparse
import logging
import os
import re
import sys
import tempfile
from pathlib import Path

from postprocess_changelog import normalize_entry_headings_text, postprocess_text

# Matches ``## [X.Y.Z]`` or ``## [Unreleased]``
_VERSION_HEADING_RE = re.compile(r"^## \[")

# Extracts a semver version from a ``## [X.Y.Z]`` heading (linked or plain).
_VERSION_RE = re.compile(r"^## \[(\d+\.\d+\.\d+[^\]]*)\]")

# Matches a reference-style link definition: ``[label]: URL``
_LINK_DEF_RE = re.compile(r"^\[([^\]]+)\]:\s+\S+")

# Archive directory relative to the repository root.
_DEFAULT_ARCHIVE_DIR = "docs/archive/changelog"

LOGGER = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Parsing helpers
# ---------------------------------------------------------------------------


def _minor_key(version: str) -> str:
    """Return the ``X.Y`` minor key for a semver version string.

    Parameters:
        version: A version string like ``0.7.2`` or ``1.2.3-rc.1``.

    Returns:
        The first two numeric components joined by a dot (e.g. ``0.7``).

    Raises:
        ValueError: If *version* does not contain at least two dot-separated components.
    """
    parts = version.split(".")
    if len(parts) < 2:
        msg = f"Expected a version with at least two components (X.Y), got: {version!r}"
        raise ValueError(msg)
    return f"{parts[0]}.{parts[1]}"


def _version_sort_key(label: str) -> tuple[bool, tuple[int, ...], tuple[tuple[int, int | str], ...]]:
    """Return a sort key for a version label that orders by semantic version.

    Non-numeric labels (e.g. ``unreleased``) sort after all numeric versions.
    Numeric parts are compared as integers so that ``0.10`` sorts after ``0.9``.

    Parameters:
        label: A version label like ``0.7.2``, ``0.10``, or ``unreleased``.

    Returns:
        A tuple suitable for use as a sort key.
    """
    label_without_build = label.split("+", 1)[0]
    core, separator, prerelease = label_without_build.partition("-")
    parts = core.split(".")
    try:
        nums = tuple(int(p) for p in parts)
    except ValueError:
        # Non-numeric labels ("unreleased") sort last (True > False).
        return (True, (), ())

    if not separator:
        prerelease_key: tuple[tuple[int, int | str], ...] = ((2, ""),)
    else:
        prerelease_key = tuple((0, int(part)) if part.isdecimal() else (1, part) for part in prerelease.split("."))

    return (False, nums, prerelease_key)


def _extract_link_defs(text: str) -> tuple[str, dict[str, str]]:
    """Separate trailing reference-style link definitions from changelog text.

    git-cliff appends reference-style link definitions at the bottom of
    CHANGELOG.md for every version heading.  When the changelog is split
    into per-version blocks these definitions must be distributed to the
    correct output files so that headings like ``## [0.7.2]`` resolve and
    no unused definitions trigger markdownlint MD053.

    Parameters:
        text: The full changelog text.

    Returns:
        A 2-tuple of (*cleaned_text*, *link_defs*) where *link_defs* maps
        lowercase labels to their full definition lines.
    """
    lines = text.rstrip("\n").split("\n")
    link_defs: dict[str, str] = {}

    # Walk backwards from the end, collecting link-def and blank lines.
    i = len(lines) - 1
    while i >= 0:
        line = lines[i]
        m = _LINK_DEF_RE.match(line)
        if m:
            link_defs[m.group(1).lower()] = line
            i -= 1
        elif line.strip() == "":
            i -= 1
        else:
            break

    cleaned = "\n".join(lines[: i + 1])
    return cleaned.rstrip("\n") + "\n", link_defs


def parse_changelog(text: str) -> tuple[str, str, list[tuple[str, str]]]:
    """Split a full changelog into preamble, unreleased block, and version blocks.

    Parameters:
        text: The full contents of CHANGELOG.md.

    Returns:
        A 3-tuple of (preamble, unreleased_block, version_blocks). The
        ``unreleased_block`` is the complete ``## [Unreleased]`` block,
        including its heading. Each item in ``version_blocks`` is a
        ``(semver_label, full_heading_block)`` pair in the order it appears
        (newest first), where ``semver_label`` is only the parsed version text
        (for example ``"0.7.2"``) and ``full_heading_block`` still includes
        the raw ``## [...]`` heading and body.
    """
    lines = text.split("\n")

    # Locate all ``## [`` headings.
    headings: list[int] = []
    for i, line in enumerate(lines):
        if _VERSION_HEADING_RE.match(line):
            headings.append(i)

    if not headings:
        return text, "", []

    preamble = "\n".join(lines[: headings[0]])

    unreleased = ""
    version_blocks: list[tuple[str, str]] = []

    for idx, start in enumerate(headings):
        end = headings[idx + 1] if idx + 1 < len(headings) else len(lines)
        block = "\n".join(lines[start:end])

        heading_line = lines[start]
        if "Unreleased" in heading_line:
            unreleased = block
        else:
            m = _VERSION_RE.match(heading_line)
            if not m:
                # Skip headings that don't contain a recognisable semver.
                continue
            version_blocks.append((m.group(1), block))

    return preamble, unreleased, version_blocks


def group_by_minor(
    version_blocks: list[tuple[str, str]],
) -> dict[str, list[tuple[str, str]]]:
    """Group version blocks by their ``X.Y`` minor key.

    Preserves insertion order (newest first within each minor).

    Parameters:
        version_blocks: List of ``(version, block_text)`` pairs.

    Returns:
        An ordered dict mapping minor keys to their version blocks.
    """
    groups: dict[str, list[tuple[str, str]]] = {}
    for ver, block in version_blocks:
        key = _minor_key(ver)
        groups.setdefault(key, []).append((ver, block))
    return groups


# ---------------------------------------------------------------------------
# Writers
# ---------------------------------------------------------------------------


def _format_link_defs(link_defs: dict[str, str], labels: set[str]) -> str:
    """Return the subset of *link_defs* whose labels are in *labels*.

    The definitions are returned in reverse-sorted order (matching the
    convention that git-cliff uses: ``[unreleased]`` first, then newest
    version to oldest).
    """
    relevant = [link_defs[label] for label in sorted(link_defs, key=_version_sort_key, reverse=True) if label in labels]
    return "\n".join(relevant) if relevant else ""


def _stage_output(path: Path, content: bytes) -> Path:
    """Write *content* to a durable temporary peer of *path*."""
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=path.parent,
    )
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        mode = path.stat().st_mode & 0o777 if path.exists() else 0o644
        temporary_path.chmod(mode)
    except BaseException:
        temporary_path.unlink(missing_ok=True)
        raise
    return temporary_path


def _rollback_outputs(
    published: list[Path],
    backups: dict[Path, Path | None],
) -> list[tuple[Path, OSError]]:
    """Restore already-published paths and return any rollback failures."""
    rollback_errors: list[tuple[Path, OSError]] = []
    for path in reversed(published):
        backup = backups[path]
        try:
            if backup is None:
                path.unlink(missing_ok=True)
            else:
                backup.replace(path)
        except OSError as rollback_error:
            rollback_errors.append((path, rollback_error))
        else:
            backups.pop(path)
    return rollback_errors


def _publish_outputs(outputs: dict[Path, str]) -> None:
    """Atomically replace a rendered output set, rolling back partial publication."""
    if not outputs:
        return

    staged: dict[Path, Path] = {}
    backups: dict[Path, Path | None] = {}
    published: list[Path] = []
    preserved_backups: set[Path] = set()
    try:
        for path, text in outputs.items():
            staged[path] = _stage_output(path, text.encode("utf-8"))
        for path in outputs:
            if path.exists() and not path.is_file():
                message = f"output path exists but is not a file: {path}"
                raise IsADirectoryError(message)
            backups[path] = _stage_output(path, path.read_bytes()) if path.exists() else None

        try:
            for path in outputs:
                staged[path].replace(path)
                staged.pop(path)
                published.append(path)
        except OSError as publish_error:
            rollback_errors = _rollback_outputs(published, backups)
            if rollback_errors:
                preserved_backups = {backup for path, _error in rollback_errors if (backup := backups.get(path)) is not None}
                recovery_locations = ", ".join(str(path) for path in sorted(preserved_backups))
                recovery_detail = f"; original content retained at {recovery_locations}" if recovery_locations else "; no original-content backup was available"
                message = f"failed to publish changelog outputs and encountered {len(rollback_errors)} rollback error(s){recovery_detail}"
                raise RuntimeError(message) from publish_error
            raise
    finally:
        backup_paths = (path for path in backups.values() if path is not None and path not in preserved_backups)
        for temporary_path in (*staged.values(), *backup_paths):
            temporary_path.unlink(missing_ok=True)


def _render_archive(
    minor: str,
    blocks: list[tuple[str, str]],
    link_defs: dict[str, str] | None = None,
) -> str:
    """Render one normalized minor-series archive without publishing it."""
    parts = [f"# Changelog - {minor}.x\n"]
    for _ver, block in blocks:
        parts.append(block)

    text = "\n".join(parts)

    if link_defs:
        versions = {ver.lower() for ver, _ in blocks}
        defs_text = _format_link_defs(link_defs, versions)
        if defs_text:
            text = text.rstrip("\n") + "\n\n" + defs_text

    return postprocess_text(text)


def write_archive(
    archive_dir: Path,
    minor: str,
    blocks: list[tuple[str, str]],
    link_defs: dict[str, str] | None = None,
) -> Path:
    """Write an archive file for a single minor series.

    Parameters:
        archive_dir: Directory for archive files.
        minor: The ``X.Y`` minor key.
        blocks: Version blocks belonging to this minor, newest first, using
            the ``(semver_label, full_heading_block)`` shape returned by
            ``parse_changelog``. The archive writer preserves each provided
            block verbatim after the generated archive title.
        link_defs: Optional mapping of lowercase labels to reference-style
            link definition lines.  Only definitions matching versions in
            *blocks* are included.

    Returns:
        The path of the written archive file.
    """
    path = archive_dir / f"{minor}.md"
    _publish_outputs({path: _render_archive(minor, blocks, link_defs)})
    return path


def _normalized_existing_archives(archive_dir: Path, excluded: set[Path]) -> dict[Path, str]:
    """Render normalization updates for historical archives without publishing them."""
    if not archive_dir.is_dir():
        return {}

    outputs: dict[Path, str] = {}
    for path in sorted(archive_dir.glob("*.md")):
        if path in excluded:
            continue
        text = path.read_text(encoding="utf-8")
        normalized = normalize_entry_headings_text(text)
        if normalized != text:
            outputs[path] = normalized
    return outputs


def build_root(
    preamble: str,
    unreleased: str,
    active_blocks: list[tuple[str, str]],
    archived_minors: list[str],
    archive_dir_rel: str,
) -> str:
    """Assemble the trimmed root CHANGELOG.md content.

    Parameters:
        preamble: Text before the first ``## `` heading.
        unreleased: The full Unreleased block (empty string if absent).
        active_blocks: Version blocks for the active minor series.
        archived_minors: Sorted list of archived ``X.Y`` minor keys.
        archive_dir_rel: Relative path to the archive directory from the changelog file.

    Returns:
        The full text for the trimmed CHANGELOG.md.
    """
    parts: list[str] = [preamble]

    if unreleased:
        parts.append(unreleased)

    for _ver, block in active_blocks:
        parts.append(block)

    if archived_minors:
        # Build the Archives section.
        archive_lines = ["## Archives\n"]
        archive_lines.append("Older releases are archived by minor series:\n")
        archive_lines.extend(f"- [{minor}.x]({archive_dir_rel}/{minor}.md)" for minor in archived_minors)
        archive_lines.append("")
        parts.append("\n".join(archive_lines))

    return postprocess_text("\n".join(parts))


# ---------------------------------------------------------------------------
# Orchestrator
# ---------------------------------------------------------------------------


def archive_changelog(
    changelog_path: Path,
    archive_dir: Path | None = None,
) -> None:
    """Split a changelog into root + per-minor archive files.

    Parameters:
        changelog_path: Path to the full CHANGELOG.md.
        archive_dir: Directory for archive files.  Defaults to
            ``docs/archive/changelog`` relative to *changelog_path*'s parent.
    """
    if archive_dir is None:
        archive_dir = changelog_path.parent / _DEFAULT_ARCHIVE_DIR

    text = changelog_path.read_text(encoding="utf-8")

    # Separate trailing reference-style link definitions before parsing
    # so they can be distributed to the correct output files.
    text, link_defs = _extract_link_defs(text)

    preamble, unreleased, version_blocks = parse_changelog(text)

    if not version_blocks:
        outputs = _normalized_existing_archives(archive_dir, excluded=set())
        _publish_outputs(outputs)
        return  # nothing to archive

    groups = group_by_minor(version_blocks)
    minor_keys = list(groups.keys())

    # Active minor = first minor that appears (newest release).
    active_minor = minor_keys[0]

    # Archive every minor except the active one.
    archived_minors = minor_keys[1:]

    if not archived_minors:
        outputs = _normalized_existing_archives(archive_dir, excluded=set())
        _publish_outputs(outputs)
        return  # only one minor series — nothing to archive yet

    # Compute relative path from changelog location to archive dir.
    try:
        archive_dir_rel = archive_dir.relative_to(changelog_path.parent).as_posix()
    except ValueError:
        try:
            archive_dir_rel = Path(os.path.relpath(archive_dir, changelog_path.parent)).as_posix()
        except ValueError as err:
            archive_dir_rel = archive_dir.as_posix()
            LOGGER.warning(
                "Could not compute relative archive directory: %s; archive_dir=%s changelog_parent=%s; generated Markdown links use %s",
                err,
                archive_dir,
                changelog_path.parent,
                archive_dir_rel,
            )
        if archive_dir_rel == ".." or archive_dir_rel.startswith("../") or Path(archive_dir_rel).is_absolute():
            LOGGER.warning(
                "Archive directory %s is outside changelog directory %s; generated Markdown links use %s",
                archive_dir,
                changelog_path.parent,
                archive_dir_rel,
            )

    root_text = build_root(
        preamble,
        unreleased,
        groups[active_minor],
        sorted(archived_minors, key=_version_sort_key, reverse=True),
        archive_dir_rel,
    )

    # Append reference-style link definitions for active versions.
    if link_defs:
        labels: set[str] = {ver.lower() for ver, _ in groups[active_minor]}
        if unreleased:
            labels.add("unreleased")
        defs_text = _format_link_defs(link_defs, labels)
        if defs_text:
            root_text = root_text.rstrip("\n") + "\n\n" + defs_text + "\n"

    outputs = {archive_dir / f"{minor}.md": _render_archive(minor, groups[minor], link_defs) for minor in archived_minors}
    outputs[changelog_path] = root_text
    outputs.update(_normalized_existing_archives(archive_dir, excluded=set(outputs)))
    _publish_outputs(outputs)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main() -> None:
    """CLI entry point for ``archive-changelog``."""
    parser = argparse.ArgumentParser(
        prog="archive-changelog",
        description="Archive completed minor series from CHANGELOG.md.",
    )
    parser.add_argument(
        "path",
        nargs="?",
        default="CHANGELOG.md",
        help="Path to CHANGELOG.md (default: CHANGELOG.md)",
    )
    parser.add_argument(
        "--archive-dir",
        default=None,
        help=f"Archive output directory (default: {_DEFAULT_ARCHIVE_DIR})",
    )
    args = parser.parse_args()

    changelog = Path(args.path)
    if not changelog.is_file():
        print(f"Error: {changelog} not found", file=sys.stderr)
        sys.exit(1)

    archive_dir = Path(args.archive_dir) if args.archive_dir else None
    archive_changelog(changelog, archive_dir)


if __name__ == "__main__":
    main()
