#!/usr/bin/env -S uv run
"""Validate synchronized release versions, dates, DOI, and package metadata."""

import argparse
import os
import re
import sys
import tomllib
from dataclasses import dataclass
from datetime import date
from enum import StrEnum
from pathlib import Path
from typing import TypeAlias, TypeGuard

_EXPECTED_PYTHON_README = "scripts/README.md"
_ZENODO_CONCEPT_DOI = "10.5281/zenodo.20513228"
_SKIP_DIRS = frozenset({".git", ".mypy_cache", ".pytest_cache", ".ruff_cache", ".tmp_pycache", ".venv", "archive", "target", "tests"})
_CITATION_VERSION_RE = re.compile(r"^version:\s*(?P<quote>['\"]?)(?P<version>[0-9A-Za-z][0-9A-Za-z.+-]*)(?P=quote)\s*(?:#.*)?$")
_CITATION_DATE_RE = re.compile(r"^date-released:\s*(?P<quote>['\"]?)(?P<date>\d{4}-\d{2}-\d{2})(?P=quote)\s*(?:#.*)?$")
_CITATION_DOI_RE = re.compile(r"^doi:\s*(?P<quote>['\"]?)(?P<doi>[^\s'\"]+)(?P=quote)\s*(?:#.*)?$")
_README_TAG_LINK_RE = re.compile(
    r"https://(?:github\.com/acgetchell/causal-triangulations/(?:blob|raw|tree)/|"
    r"raw\.githubusercontent\.com/acgetchell/causal-triangulations/)"
    r"(?:v(?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)|"
    r"(?P<revision>[0-9a-f]{7,40}))(?P<path>/[^)\s]*)?(?=$|[)\s])"
)
README_TAG_LINK_RE = _README_TAG_LINK_RE

# Semgrep 1.175 cannot parse the Python 3.12 `type` statement in strict scans.
ParsedObject: TypeAlias = dict[str, object]  # noqa: UP040


@dataclass(frozen=True, slots=True)
class ReleaseDate:
    """One validated release date with source context."""

    path: Path
    line: int
    value: str


@dataclass(frozen=True, slots=True)
class PackageInfo:
    """Cargo package identity that owns the release version."""

    name: str
    version: str


@dataclass(frozen=True, slots=True)
class PythonProjectInfo:
    """Python support-package identity used to locate its uv lock entry."""

    name: str
    version: str


class ReferenceKind(StrEnum):
    """A release surface whose version must match Cargo.toml."""

    CARGO_ADD = "cargo add command"
    CARGO_LOCK = "Cargo.lock root package"
    CITATION = "CITATION.cff version"
    DEPENDENCY_SNIPPET = "documentation dependency snippet"
    PYPROJECT = "pyproject.toml project"
    README_TAG_LINK = "README tag-pinned link"
    UV_LOCK = "uv.lock editable package"


@dataclass(frozen=True, slots=True)
class VersionReference:
    """One parsed current-release reference with source context."""

    path: Path
    line: int
    version: str
    kind: ReferenceKind
    text: str


@dataclass(frozen=True, slots=True)
class VersionMismatch:
    """A release-version reference that differs from Cargo.toml."""

    reference: VersionReference
    package: PackageInfo


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


def _required_string(data: ParsedObject, key: str, context: str) -> str:
    """Return one required non-empty string."""
    value = data.get(key)
    if not isinstance(value, str) or not value:
        msg = f"{context}: {key} must be a non-empty string"
        raise TypeError(msg)
    return value


def _read_toml(path: Path) -> ParsedObject:
    """Parse a TOML file and require an object at its root."""
    data: object = tomllib.loads(path.read_text(encoding="utf-8"))
    if not _is_parsed_object(data):
        msg = f"{path}: expected a TOML object"
        raise TypeError(msg)
    return data


def read_cargo_package_info(path: Path) -> PackageInfo:
    """Read the authoritative Cargo package identity."""
    package = _required_table(_read_toml(path), "package", path)
    return PackageInfo(
        name=_required_string(package, "name", f"{path} [package]"),
        version=_required_string(package, "version", f"{path} [package]"),
    )


def read_python_project_info(path: Path) -> PythonProjectInfo:
    """Read the Python support-package identity."""
    project = _required_table(_read_toml(path), "project", path)
    return PythonProjectInfo(
        name=_required_string(project, "name", f"{path} [project]"),
        version=_required_string(project, "version", f"{path} [project]"),
    )


def toml_table_key_line(path: Path, table_name: str, key: str) -> int:
    """Return the source line for one key in a TOML table."""
    current_table: str | None = None
    key_re = re.compile(rf"^{re.escape(key)}\s*=")
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            current_table = stripped.strip("[]")
        elif current_table == table_name and key_re.match(stripped):
            return line_number
    msg = f"{path}: [{table_name}] is missing {key}"
    raise TypeError(msg)


def _version_reference(path: Path, line: int, version: str, kind: ReferenceKind) -> VersionReference:
    """Build a version reference and retain its source text."""
    lines = path.read_text(encoding="utf-8").splitlines()
    if not 1 <= line <= len(lines):
        msg = f"{path}: missing line {line} for {kind}"
        raise TypeError(msg)
    return VersionReference(path=path, line=line, version=version, kind=kind, text=lines[line - 1].strip())


def _package_entries(path: Path) -> list[ParsedObject]:
    """Return parsed package array-table lockfile entries."""
    packages = _read_toml(path).get("package")
    if not isinstance(packages, list):
        msg = f"{path}: missing package array-table entries"
        raise TypeError(msg)
    entries: list[ParsedObject] = []
    for index, package in enumerate(packages, start=1):
        if not _is_parsed_object(package):
            msg = f"{path}: package entry {index} is not a TOML object"
            raise TypeError(msg)
        entries.append(package)
    return entries


def _array_table_key_line(path: Path, table_name: str, table_index: int, key: str) -> int:
    """Return a key line inside one array-table entry."""
    current_index = -1
    in_target = False
    key_re = re.compile(rf"^{re.escape(key)}\s*=")
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if stripped == f"[[{table_name}]]":
            current_index += 1
            in_target = current_index == table_index
        elif stripped.startswith("[["):
            in_target = False
        elif in_target and key_re.match(stripped):
            return line_number
    msg = f"{path}: [[{table_name}]] entry {table_index + 1} is missing {key}"
    raise TypeError(msg)


def _single_package_reference(
    path: Path,
    entries: list[ParsedObject],
    indices: list[int],
    package_name: str,
    kind: ReferenceKind,
) -> VersionReference:
    """Return the only matching lockfile package reference."""
    if len(indices) != 1:
        msg = f"{path}: expected exactly one {kind} named {package_name!r}; found {len(indices)}"
        raise TypeError(msg)
    index = indices[0]
    version = _required_string(entries[index], "version", f"{path} [[package]] entry {index + 1}")
    return _version_reference(path, _array_table_key_line(path, "package", index, "version"), version, kind)


def cargo_lock_reference(path: Path, package: PackageInfo) -> VersionReference:
    """Return the root Cargo package entry from Cargo.lock."""
    entries = _package_entries(path)
    indices = [index for index, entry in enumerate(entries) if entry.get("name") == package.name and "source" not in entry]
    return _single_package_reference(path, entries, indices, package.name, ReferenceKind.CARGO_LOCK)


def pyproject_reference(path: Path, project: PythonProjectInfo) -> VersionReference:
    """Return the support-package version from pyproject.toml."""
    return _version_reference(path, toml_table_key_line(path, "project", "version"), project.version, ReferenceKind.PYPROJECT)


def uv_lock_reference(path: Path, project: PythonProjectInfo) -> VersionReference:
    """Return the root editable support-package entry from uv.lock."""
    entries = _package_entries(path)
    indices: list[int] = []
    for index, entry in enumerate(entries):
        source = entry.get("source")
        if entry.get("name") == project.name and _is_parsed_object(source) and source.get("editable") == ".":
            indices.append(index)
    return _single_package_reference(path, entries, indices, project.name, ReferenceKind.UV_LOCK)


def citation_reference(path: Path) -> VersionReference:
    """Return the only top-level CITATION.cff version."""
    references: list[VersionReference] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.startswith("version:"):
            continue
        match = _CITATION_VERSION_RE.fullmatch(line)
        if match is None:
            msg = f"{path}:{line_number}: top-level version must be a non-empty scalar"
            raise TypeError(msg)
        references.append(_version_reference(path, line_number, match.group("version"), ReferenceKind.CITATION))
    if len(references) != 1:
        msg = f"{path}: expected exactly one top-level version; found {len(references)}"
        raise TypeError(msg)
    return references[0]


def citation_release_date(path: Path) -> ReleaseDate:
    """Read exactly one top-level ISO date-released value."""
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


def _validate_citation_doi(path: Path) -> None:
    """Require the single Zenodo concept DOI and no rotating identifiers block."""
    matches: list[tuple[int, str]] = []
    identifier_lines: list[int] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if line.startswith("identifiers:"):
            identifier_lines.append(line_number)
        if not line.startswith("doi:"):
            continue
        match = _CITATION_DOI_RE.fullmatch(line)
        if match is None:
            msg = f"{path}:{line_number}: top-level doi must be a non-empty scalar"
            raise ValueError(msg)
        matches.append((line_number, match.group("doi")))
    if len(matches) != 1:
        msg = f"{path}: expected exactly one top-level Zenodo concept DOI; found {len(matches)}"
        raise ValueError(msg)
    line_number, value = matches[0]
    if value != _ZENODO_CONCEPT_DOI:
        msg = f"{path}:{line_number}: top-level doi must remain the Zenodo concept DOI {_ZENODO_CONCEPT_DOI}; found {value!r}"
        raise ValueError(msg)
    if identifier_lines:
        rendered = ", ".join(str(line) for line in identifier_lines)
        msg = f"{path}: version-specific identifiers are forbidden; found top-level identifiers at lines {rendered}"
        raise ValueError(msg)


def changelog_release_date(path: Path, version: str) -> ReleaseDate | None:
    """Return the current package-version heading date when it exists."""
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


def iter_active_markdown_files(root: Path) -> list[Path]:
    """Return deterministic active Markdown files that may carry release references."""
    markdown_files: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(dirname for dirname in dirnames if not (set((Path(dirpath) / dirname).relative_to(root).parts) & _SKIP_DIRS))
        markdown_files.extend(Path(dirpath) / filename for filename in sorted(filenames) if filename.endswith(".md") and filename != "CHANGELOG.md")
    return sorted(markdown_files)


def dependency_regex(package_name: str) -> re.Pattern[str]:
    """Build a regex for Cargo dependency snippets naming the package."""
    escaped = re.escape(package_name)
    return re.compile(
        rf"(?<![\w.-]){escaped}\s*=\s*(?:"
        rf'"(?P<plain>[^"]+)"|\'(?P<plain_literal>[^\']+)\'|'
        rf"\{{[^}}]*version\s*=\s*(?:\"(?P<table>[^\"]+)\"|'(?P<table_literal>[^']+)')[^}}]*\}})"
    )


def cargo_add_regex(package_name: str) -> re.Pattern[str]:
    """Build a regex for cargo add package-at-version examples."""
    escaped = re.escape(package_name)
    return re.compile(rf"(?<![\w.-])cargo\s+add\b[^\x60\n]*?(?<![\w.-]){escaped}@(?P<version>[^\s\x60]+)")


def readme_tag_link_is_performance_asset(match: re.Match[str]) -> bool:
    """Return whether a README link is owned by performance publication."""
    path = str(match.group("path") or "")
    return path.startswith(("/docs/assets/bench/", "/docs/archive/performance/data/")) or path == "/docs/PERFORMANCE.md"


def _markdown_references(root: Path, package: PackageInfo) -> list[VersionReference]:
    """Collect active documentation versions owned by release metadata."""
    references: list[VersionReference] = []
    dependency_re = dependency_regex(package.name)
    cargo_add_re = cargo_add_regex(package.name)
    for path in iter_active_markdown_files(root):
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            for match in dependency_re.finditer(line):
                version = next(match.group(name) for name in ("plain", "plain_literal", "table", "table_literal") if match.group(name) is not None)
                references.append(VersionReference(path, line_number, version, ReferenceKind.DEPENDENCY_SNIPPET, line.strip()))
            references.extend(
                VersionReference(path, line_number, match.group("version"), ReferenceKind.CARGO_ADD, line.strip()) for match in cargo_add_re.finditer(line)
            )
            if path == root / "README.md":
                references.extend(
                    VersionReference(
                        path,
                        line_number,
                        match.group("version") or match.group("revision"),
                        ReferenceKind.README_TAG_LINK,
                        line.strip(),
                    )
                    for match in _README_TAG_LINK_RE.finditer(line)
                    if not readme_tag_link_is_performance_asset(match)
                )
    return references


def find_version_mismatches(root: Path, *, final_release: bool = False) -> list[VersionMismatch]:
    """Return all deterministic release surfaces that differ from Cargo.toml."""
    package = read_cargo_package_info(root / "Cargo.toml")
    project_path = root / "pyproject.toml"
    project = read_python_project_info(project_path)
    citation = root / "CITATION.cff"
    citation_date = citation_release_date(citation)
    _validate_citation_doi(citation)
    references = [
        cargo_lock_reference(root / "Cargo.lock", package),
        pyproject_reference(project_path, project),
        uv_lock_reference(root / "uv.lock", project),
        citation_reference(citation),
        *_markdown_references(root, package),
    ]
    changelog = root / "CHANGELOG.md"
    changelog_date = changelog_release_date(changelog, package.version) if changelog.is_file() else None
    if final_release and changelog_date is None:
        msg = f"{changelog}: final release validation requires exactly one generated heading for {package.version}"
        raise ValueError(msg)
    if changelog_date is not None and citation_date.value != changelog_date.value:
        msg = (
            f"release date mismatch: {citation_date.path}:{citation_date.line} has {citation_date.value}, "
            f"but {changelog_date.path}:{changelog_date.line} has {changelog_date.value}; both must use the generated UTC release date"
        )
        raise ValueError(msg)
    return [VersionMismatch(reference=reference, package=package) for reference in references if reference.version != package.version]


def _validate_python_readme(root: Path) -> None:
    """Require the support package to ship its own README."""
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


def validate_release_metadata(root: Path, *, final_release: bool = False) -> None:
    """Validate release metadata rooted at one repository checkout."""
    _validate_python_readme(root)
    mismatches = find_version_mismatches(root, final_release=final_release)
    if mismatches:
        details = "; ".join(
            f"{mismatch.reference.path.relative_to(root)}:{mismatch.reference.line} has {mismatch.reference.version!r} ({mismatch.reference.kind})"
            for mismatch in mismatches
        )
        expected = mismatches[0].package.version
        msg = f"release-version references must match Cargo [package].version {expected!r}: {details}"
        raise ValueError(msg)


def main(argv: list[str] | None = None) -> int:
    """Validate release metadata and translate expected failures for the CLI."""
    parser = argparse.ArgumentParser(
        prog="check-release-metadata",
        description="Validate synchronized release versions, dates, DOI, and package metadata.",
    )
    parser.add_argument("root", nargs="?", default=Path.cwd(), type=Path, help="Repository root to check (default: current directory).")
    parser.add_argument("--final-release", action="store_true", help="Require the generated current-version changelog heading.")
    args = parser.parse_args(argv)
    try:
        validate_release_metadata(args.root.resolve(), final_release=args.final_release)
    except (OSError, TypeError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"Release metadata validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
