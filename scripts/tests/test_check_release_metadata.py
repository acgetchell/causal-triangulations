"""Tests for release-date and Python package metadata validation."""

# Keep runtime annotation resolution available during test collection.
from pathlib import Path  # noqa: TC003

import pytest

import check_release_metadata


def _write_repository(
    root: Path,
    *,
    citation_dates: tuple[str, ...] = ("2026-06-02",),
    changelog_headings: tuple[str, ...] = ("## [0.1.0] - 2026-06-02",),
    python_version: str = "0.1.0",
    python_readme: str = "scripts/README.md",
) -> None:
    (root / "Cargo.toml").write_text('[package]\nname = "causal-triangulations"\nversion = "0.1.0"\n', encoding="utf-8")
    (root / "CITATION.cff").write_text(
        "cff-version: 1.2.0\nversion: 0.1.0\n" + "".join(f"date-released: {release_date}\n" for release_date in citation_dates),
        encoding="utf-8",
    )
    (root / "CHANGELOG.md").write_text("# Changelog\n\n" + "\n\n".join(changelog_headings) + "\n", encoding="utf-8")
    (root / "scripts").mkdir()
    (root / "scripts" / "README.md").write_text("# Support scripts\n", encoding="utf-8")
    (root / "pyproject.toml").write_text(
        f'[project]\nname = "causal-triangulations-scripts"\nversion = "{python_version}"\nreadme = "{python_readme}"\n',
        encoding="utf-8",
    )


def test_matching_release_dates_and_python_readme_pass(tmp_path: Path) -> None:
    _write_repository(tmp_path)

    check_release_metadata.validate_release_metadata(tmp_path)


def test_matching_inline_linked_release_dates_pass(tmp_path: Path) -> None:
    _write_repository(
        tmp_path,
        changelog_headings=("## [0.1.0](https://example.com/releases/0.1.0) - 2026-06-02",),
    )

    check_release_metadata.validate_release_metadata(tmp_path)


def test_missing_current_version_heading_still_validates_citation_date(tmp_path: Path) -> None:
    _write_repository(tmp_path, changelog_headings=("## [Unreleased]",))

    check_release_metadata.validate_release_metadata(tmp_path)


@pytest.mark.parametrize("release_date", ["2026-6-02", "June 2, 2026", "'2026-06-02\"", ""])
def test_malformed_citation_date_reports_file_and_line(tmp_path: Path, release_date: str) -> None:
    _write_repository(tmp_path, citation_dates=(release_date,))

    with pytest.raises(ValueError, match=r"CITATION\.cff:3: top-level date-released must use ISO YYYY-MM-DD"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_invalid_citation_calendar_date_reports_file_and_line(tmp_path: Path) -> None:
    _write_repository(tmp_path, citation_dates=("2026-02-30",))

    with pytest.raises(ValueError, match=r"CITATION\.cff:3: invalid date-released"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_missing_citation_date_reports_file_context(tmp_path: Path) -> None:
    _write_repository(tmp_path, citation_dates=())

    with pytest.raises(ValueError, match=r"CITATION\.cff: expected exactly one top-level date-released; found 0"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_duplicate_citation_dates_report_both_lines(tmp_path: Path) -> None:
    _write_repository(tmp_path, citation_dates=("2026-06-02", "2026-06-02"))

    with pytest.raises(ValueError, match=r"CITATION\.cff: expected exactly one top-level date-released; found 2 at lines 3, 4"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_mismatched_release_dates_report_both_sources(tmp_path: Path) -> None:
    _write_repository(tmp_path, changelog_headings=("## [0.1.0] - 2026-06-03",))

    with pytest.raises(ValueError, match=r"CITATION\.cff:3 has 2026-06-02, but .*CHANGELOG\.md:3 has 2026-06-03"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_mismatched_inline_linked_release_dates_report_both_sources(tmp_path: Path) -> None:
    _write_repository(
        tmp_path,
        changelog_headings=("## [0.1.0](https://example.com/releases/0.1.0) - 2026-06-03",),
    )

    with pytest.raises(ValueError, match=r"CITATION\.cff:3 has 2026-06-02, but .*CHANGELOG\.md:3 has 2026-06-03"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_malformed_current_release_heading_reports_line(tmp_path: Path) -> None:
    _write_repository(tmp_path, changelog_headings=("## [0.1.0] - June 2, 2026",))

    with pytest.raises(ValueError, match=r"CHANGELOG\.md:3: release heading for 0\.1\.0 must end with ISO YYYY-MM-DD"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_malformed_inline_linked_release_heading_reports_line(tmp_path: Path) -> None:
    _write_repository(
        tmp_path,
        changelog_headings=("## [0.1.0](https://example.com/releases/0.1.0) - June 2, 2026",),
    )

    with pytest.raises(ValueError, match=r"CHANGELOG\.md:3: release heading for 0\.1\.0 must end with ISO YYYY-MM-DD"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_duplicate_current_release_headings_report_lines(tmp_path: Path) -> None:
    _write_repository(
        tmp_path,
        changelog_headings=("## [0.1.0] - 2026-06-02", "## [0.1.0] - 2026-06-02"),
    )

    with pytest.raises(ValueError, match=r"CHANGELOG\.md: duplicate release headings for 0\.1\.0 at lines 3, 5"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_python_package_readme_must_target_support_documentation(tmp_path: Path) -> None:
    _write_repository(tmp_path, python_readme="README.md")

    with pytest.raises(ValueError, match=r"pyproject\.toml: \[project\]\.readme must be 'scripts/README\.md'"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_python_package_version_must_match_cargo_package(tmp_path: Path) -> None:
    _write_repository(tmp_path, python_version="0.2.0")

    with pytest.raises(ValueError, match=r"\[project\]\.version must match Cargo \[package\]\.version '0\.1\.0', got '0\.2\.0'"):
        check_release_metadata.validate_release_metadata(tmp_path)


def test_cli_reports_validation_failures_on_stderr(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    _write_repository(tmp_path, citation_dates=())

    assert check_release_metadata.main([str(tmp_path)]) == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "Release metadata validation failed:" in captured.err
