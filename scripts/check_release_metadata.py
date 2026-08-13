#!/usr/bin/env -S uv run
"""Validate release dates and Python support-package metadata."""

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import TypeGuard

_CITATION_DATE_RE = re.compile(r"^date-released:\s*(?P<quote>['\"]?)(?P<date>\d{4}-\d{2}-\d{2})(?P=quote)\s*(?:#.*)?$")
_EXPECTED_PYTHON_README = "scripts/README.md"

type ParsedObject = dict[str, object]


@dataclass(frozen=True, slots=True)
class ReleaseDate:
    """One validated release date with source context."""

    path: Path
    line: int
    value: str


def _is_parsed_object(value: object) -> TypeGuard[ParsedObject]:
    """Return whether a parsed value is an object with string keys."""
    return isinstance(value, dict) and all(isinstance(key, str) for key in value)


def _required_table(data: ParsedObject, key: str, path: Path) -> ParsedObject:
    """Return a required top-level TOML table."""
    table = data.get(key)
    if not _is_parsed_object(table):
        msg = f"{path}: missing [{key}] table"
        raise TypeError(msg)
    return table


def _read_toml(path: Path) -> ParsedObject:
    """Parse a TOML file and require an object at its root."""
    data: object = tomllib.loads(path.read_text(encoding="utf-8"))
    if not _is_parsed_object(data):
        msg = f"{path}: expected a TOML object"
        raise TypeError(msg)
    return data


def _package_version(path: Path) -> str:
    """Read the authoritative Cargo package version."""
    package = _required_table(_read_toml(path), "package", path)
    version = package.get("version")
    if not isinstance(version, str) or not version:
        msg = f"{path}: [package].version must be a non-empty string"
        raise TypeError(msg)
    return version


def _citation_release_date(path: Path) -> ReleaseDate:
    """Read exactly one top-level ISO ``date-released`` value."""
    matches: list[ReleaseDate] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.startswith("date-released:"):
            continue
        match = _CITATION_DATE_RE.fullmatch(line)
        if match is None:
            msg = f"{path}:{line_number}: top-level date-released must use ISO YYYY-MM-DD"
            raise ValueError(msg)
        value = match.group("date")
        try:
            date.fromisoformat(value)
        except ValueError as exc:
            msg = f"{path}:{line_number}: invalid date-released {value!r}"
            raise ValueError(msg) from exc
        matches.append(ReleaseDate(path=path, line=line_number, value=value))

    if len(matches) != 1:
        lines = ", ".join(str(match.line) for match in matches) or "none"
        msg = f"{path}: expected exactly one top-level date-released; found {len(matches)} at lines {lines}"
        raise ValueError(msg)
    return matches[0]


def _changelog_release_date(path: Path, version: str) -> ReleaseDate | None:
    """Read the current package-version heading date when that heading exists."""
    heading_start = rf"## \[v?{re.escape(version)}\]"
    heading_prefix = re.compile(rf"^{heading_start}")
    heading = re.compile(rf"^{heading_start}(?:\([^)]+\))? - (?P<date>\d{{4}}-\d{{2}}-\d{{2}})$")
    matches: list[ReleaseDate] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if heading_prefix.match(line) is None:
            continue
        match = heading.fullmatch(line)
        if match is None:
            msg = f"{path}:{line_number}: release heading for {version} must end with ISO YYYY-MM-DD"
            raise ValueError(msg)
        value = match.group("date")
        try:
            date.fromisoformat(value)
        except ValueError as exc:
            msg = f"{path}:{line_number}: invalid release heading date {value!r}"
            raise ValueError(msg) from exc
        matches.append(ReleaseDate(path=path, line=line_number, value=value))

    if len(matches) > 1:
        lines = ", ".join(str(match.line) for match in matches)
        msg = f"{path}: duplicate release headings for {version} at lines {lines}"
        raise ValueError(msg)
    return matches[0] if matches else None


def _validate_python_readme(root: Path) -> None:
    """Require package metadata to ship the support-tooling README."""
    pyproject = root / "pyproject.toml"
    project = _required_table(_read_toml(pyproject), "project", pyproject)
    readme = project.get("readme")
    if readme != _EXPECTED_PYTHON_README:
        msg = f"{pyproject}: [project].readme must be {_EXPECTED_PYTHON_README!r}, got {readme!r}"
        raise ValueError(msg)
    readme_path = root / _EXPECTED_PYTHON_README
    if not readme_path.is_file():
        msg = f"{pyproject}: [project].readme points to missing file {readme_path}"
        raise FileNotFoundError(msg)


def validate_release_metadata(root: Path) -> None:
    """Validate release metadata rooted at one repository checkout."""
    version = _package_version(root / "Cargo.toml")
    citation_date = _citation_release_date(root / "CITATION.cff")
    _validate_python_readme(root)

    changelog = root / "CHANGELOG.md"
    if not changelog.is_file():
        return
    changelog_date = _changelog_release_date(changelog, version)
    if changelog_date is not None and citation_date.value != changelog_date.value:
        msg = (
            f"release date mismatch: {citation_date.path}:{citation_date.line} has {citation_date.value}, "
            f"but {changelog_date.path}:{changelog_date.line} has {changelog_date.value}; both must use the generated UTC release date"
        )
        raise ValueError(msg)


def main(argv: list[str] | None = None) -> int:
    """Validate release metadata and translate expected failures for the CLI."""
    parser = argparse.ArgumentParser(
        prog="check-release-metadata",
        description="Validate release dates and Python support-package metadata.",
    )
    parser.add_argument(
        "root",
        nargs="?",
        default=Path.cwd(),
        type=Path,
        help="Repository root to check (default: current directory).",
    )
    root = parser.parse_args(argv).root.resolve()
    try:
        validate_release_metadata(root)
    except (OSError, TypeError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"Release metadata validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
