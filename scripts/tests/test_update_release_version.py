"""Tests for transactional release-version updates."""

import json
import subprocess

# Keep runtime annotation resolution available during test collection.
from pathlib import Path  # noqa: TC003

import pytest

import check_release_metadata
import update_release_version

_CONCEPT_DOI = "10.5281/zenodo.20513228"


def _write_project(root: Path, *, metadata_version: str = "0.1.0", dependency_version: str = "0.1.0") -> None:
    files = {
        "Cargo.toml": f'[package]\nname = "causal-triangulations"\nversion = "{metadata_version}"\n',
        "Cargo.lock": (
            f'version = 4\n\n[[package]]\nname = "either"\nversion = "1.17.0"\n\n[[package]]\nname = "causal-triangulations"\nversion = "{metadata_version}"\n'
        ),
        "pyproject.toml": (
            f'[project]\nname = "causal-triangulations-scripts"\nversion = "{metadata_version}"\nreadme = "scripts/README.md"\nrequires-python = ">=3.14"\n'
        ),
        "uv.lock": (f'version = 1\n\n[[package]]\nname = "causal-triangulations-scripts"\nversion = "{metadata_version}"\nsource = {{ editable = "." }}\n'),
        "CITATION.cff": (
            f"cff-version: 1.2.0\nversion: {metadata_version}\ndoi: {_CONCEPT_DOI}\n"
            "identifiers:\n"
            "  - type: doi\n"
            "    value: 10.5281/zenodo.20513229\n"
            f"    description: Zenodo DOI for version {metadata_version}\n"
            "date-released: 2026-06-02\n"
        ),
        "README.md": (
            f"cargo add causal-triangulations@{dependency_version}\n"
            f'causal-triangulations = "{dependency_version}"\n'
            f'causal-triangulations = {{ version = "{dependency_version}", features = ["slow-tests"] }}\n'
            f"[license](https://github.com/acgetchell/causal-triangulations/blob/v{metadata_version}/LICENSE)\n"
            f"[performance](https://github.com/acgetchell/causal-triangulations/blob/v{metadata_version}/docs/PERFORMANCE.md)\n"
            f"[csv](https://github.com/acgetchell/causal-triangulations/blob/v{metadata_version}/docs/assets/bench/release.csv)\n"
        ),
        "CHANGELOG.md": f"# Changelog\n\n## [{metadata_version}] - 2026-06-02\n",
    }
    for filename, content in files.items():
        (root / filename).write_text(content, encoding="utf-8")
    scripts = root / "scripts"
    scripts.mkdir()
    (scripts / "README.md").write_text("# Support scripts\n", encoding="utf-8")
    docs = root / "docs"
    docs.mkdir()
    (docs / "performance-testing.md").write_text(
        f"just performance-release v{metadata_version} v0.0.9\nHistorical v0.0.9 behavior remains documented.\n",
        encoding="utf-8",
    )


def _previous() -> update_release_version.ReleaseTag:
    return update_release_version.parse_release_tag("v0.1.0")


def _snapshots(root: Path) -> dict[Path, bytes]:
    return {path: path.read_bytes() for path in root.rglob("*") if path.is_file()}


def test_update_release_version_synchronizes_owned_surfaces_and_removes_record_doi(tmp_path: Path) -> None:
    _write_project(tmp_path)

    summary = update_release_version.update_release_version(
        tmp_path,
        "v0.1.1",
        previous=_previous(),
        release_date="2026-08-31",
    )

    assert summary.target.tag == "v0.1.1"
    assert summary.previous.tag == "v0.1.0"
    assert summary.release_date == "2026-08-31"
    assert summary.changed_paths
    assert 'name = "either"\nversion = "1.17.0"' in (tmp_path / "Cargo.lock").read_text(encoding="utf-8")
    assert 'name = "causal-triangulations"\nversion = "0.1.1"' in (tmp_path / "Cargo.lock").read_text(encoding="utf-8")
    assert 'name = "causal-triangulations-scripts"\nversion = "0.1.1"' in (tmp_path / "uv.lock").read_text(encoding="utf-8")
    citation = (tmp_path / "CITATION.cff").read_text(encoding="utf-8")
    assert "version: 0.1.1" in citation
    assert "date-released: 2026-08-31" in citation
    assert f"doi: {_CONCEPT_DOI}" in citation
    assert "identifiers:" not in citation
    assert "20513229" not in citation
    readme = (tmp_path / "README.md").read_text(encoding="utf-8")
    assert "cargo add causal-triangulations@0.1.1" in readme
    assert 'causal-triangulations = "0.1.1"' in readme
    assert 'version = "0.1.1"' in readme
    assert "blob/v0.1.1/LICENSE" in readme
    assert "blob/v0.1.0/docs/PERFORMANCE.md" in readme
    assert "blob/v0.1.0/docs/assets/bench/release.csv" in readme
    performance = (tmp_path / "docs" / "performance-testing.md").read_text(encoding="utf-8")
    assert "just performance-release v0.1.1 v0.1.0" in performance
    assert "Historical v0.0.9 behavior remains documented." in performance
    assert check_release_metadata.find_version_mismatches(tmp_path) == []


def test_same_tag_same_day_rerun_is_content_idempotent(tmp_path: Path) -> None:
    _write_project(tmp_path)
    kwargs = {"previous": _previous(), "release_date": "2026-08-31"}

    first = update_release_version.update_release_version(tmp_path, "v0.1.1", **kwargs)
    second = update_release_version.update_release_version(tmp_path, "v0.1.1", **kwargs)

    assert first.changed_paths
    assert second.changed_paths == ()


def test_same_tag_new_utc_day_advances_citation_and_changelog_together(tmp_path: Path) -> None:
    _write_project(tmp_path)
    update_release_version.update_release_version(
        tmp_path,
        "v0.1.1",
        previous=_previous(),
        release_date="2026-08-31",
    )
    (tmp_path / "CHANGELOG.md").write_text("# Changelog\n\n## [0.1.1] - 2026-08-31\n", encoding="utf-8")

    summary = update_release_version.update_release_version(
        tmp_path,
        "v0.1.1",
        previous=_previous(),
        release_date="2026-09-01",
    )

    assert summary.changed_paths == (tmp_path / "CHANGELOG.md", tmp_path / "CITATION.cff")
    assert "date-released: 2026-09-01" in (tmp_path / "CITATION.cff").read_text(encoding="utf-8")
    assert "## [0.1.1] - 2026-09-01" in (tmp_path / "CHANGELOG.md").read_text(encoding="utf-8")


@pytest.mark.parametrize("target", ["0.1.1", "v0.1", "v00.1.1", "v0.1.1-rc.1"])
def test_parse_release_tag_rejects_nonstable_forms(target: str) -> None:
    with pytest.raises(ValueError, match=r"stable tag in vX\.Y\.Z form"):
        update_release_version.parse_release_tag(target)


def test_select_previous_release_ignores_draft_prerelease_and_nonsemver_names() -> None:
    target = update_release_version.parse_release_tag("v0.2.0")

    previous = update_release_version.select_previous_release_tag(
        ["not-a-release", "v0.1.1-rc.1", "v0.1.0", "v0.1.1"],
        target,
    )

    assert previous.tag == "v0.1.1"


def test_select_previous_release_rejects_missing_history() -> None:
    target = update_release_version.parse_release_tag("v0.1.1")

    with pytest.raises(ValueError, match="no published stable"):
        update_release_version.select_previous_release_tag([], target)


def test_select_previous_release_rejects_older_target() -> None:
    target = update_release_version.parse_release_tag("v0.1.1")

    with pytest.raises(ValueError, match="older than published"):
        update_release_version.select_previous_release_tag(["v0.1.0", "v0.2.0"], target)


def test_published_release_lookup_filters_drafts_and_prereleases(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    payload = [
        {"tagName": "v0.0.9", "isDraft": False, "isPrerelease": False, "publishedAt": "2026-01-01T00:00:00Z"},
        {"tagName": "v0.1.0", "isDraft": False, "isPrerelease": False, "publishedAt": "2026-06-02T00:00:00Z"},
        {"tagName": "v0.1.1", "isDraft": True, "isPrerelease": False, "publishedAt": None},
        {"tagName": "v0.2.0-rc.1", "isDraft": False, "isPrerelease": True, "publishedAt": "2026-08-01T00:00:00Z"},
    ]

    def fake_run(command: str, args: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        assert command == "gh"
        assert args[:2] == ["release", "list"]
        assert kwargs["cwd"] == tmp_path
        return subprocess.CompletedProcess([command, *args], 0, stdout=json.dumps(payload), stderr="")

    monkeypatch.setattr(update_release_version, "run_safe_command", fake_run)

    assert update_release_version.published_stable_release_tags(tmp_path) == ["v0.0.9", "v0.1.0"]


def test_unexpected_documentation_version_fails_before_writing(tmp_path: Path) -> None:
    _write_project(tmp_path, dependency_version="0.0.8")
    originals = _snapshots(tmp_path)

    with pytest.raises(ValueError, match="unexpected causal-triangulations dependency version"):
        update_release_version.update_release_version(
            tmp_path,
            "v0.1.1",
            previous=_previous(),
            release_date="2026-08-31",
        )

    assert _snapshots(tmp_path) == originals


def test_unrecognized_identifiers_block_fails_before_writing(tmp_path: Path) -> None:
    _write_project(tmp_path)
    citation = tmp_path / "CITATION.cff"
    citation.write_text(
        citation.read_text(encoding="utf-8").replace(
            "    description: Zenodo DOI for version 0.1.0\n",
            "    description: unrelated external identifier\n",
        ),
        encoding="utf-8",
    )
    originals = _snapshots(tmp_path)

    with pytest.raises(ValueError, match="refusing to remove an unrecognized identifiers block"):
        update_release_version.update_release_version(
            tmp_path,
            "v0.1.1",
            previous=_previous(),
            release_date="2026-08-31",
        )

    assert _snapshots(tmp_path) == originals


def test_planned_validation_failure_precedes_repository_writes(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _write_project(tmp_path)
    originals = _snapshots(tmp_path)
    writes: list[Path] = []

    def fail_validation(*_args: object) -> None:
        msg = "simulated planned validation failure"
        raise ValueError(msg)

    def record_write(path: Path, text: str) -> None:
        writes.append(path)
        path.write_text(text, encoding="utf-8")

    monkeypatch.setattr(update_release_version, "_validate_updated_root", fail_validation)
    monkeypatch.setattr(update_release_version, "_write_text_atomic", record_write)

    with pytest.raises(ValueError, match="simulated planned validation failure"):
        update_release_version.update_release_version(
            tmp_path,
            "v0.1.1",
            previous=_previous(),
            release_date="2026-08-31",
        )

    assert writes == []
    assert _snapshots(tmp_path) == originals


def test_mid_write_failure_rolls_back_exact_bytes(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _write_project(tmp_path)
    cargo_lock = tmp_path / "Cargo.lock"
    cargo_lock.write_bytes(cargo_lock.read_bytes().replace(b"\n", b"\r\n"))
    originals = _snapshots(tmp_path)
    real_write = update_release_version._write_text_atomic
    calls = 0

    def fail_second_write(path: Path, text: str) -> None:
        nonlocal calls
        calls += 1
        if calls == 2:
            msg = "simulated mid-write failure"
            raise OSError(msg)
        real_write(path, text)

    monkeypatch.setattr(update_release_version, "_write_text_atomic", fail_second_write)

    with pytest.raises(OSError, match="simulated mid-write failure"):
        update_release_version.update_release_version(
            tmp_path,
            "v0.1.1",
            previous=_previous(),
            release_date="2026-08-31",
        )

    assert _snapshots(tmp_path) == originals


def test_sync_changelog_date_uses_citation_date(tmp_path: Path) -> None:
    _write_project(tmp_path)
    update_release_version.update_release_version(
        tmp_path,
        "v0.1.1",
        previous=_previous(),
        release_date="2026-08-31",
    )
    changelog = tmp_path / "CHANGELOG.md"
    changelog.write_text("# Changelog\n\n## [0.1.1] - 2026-09-01\n", encoding="utf-8")

    changed, release_date = update_release_version.sync_changelog_release_date(tmp_path, "v0.1.1")

    assert changed == (changelog,)
    assert release_date == "2026-08-31"
    assert "## [0.1.1] - 2026-08-31" in changelog.read_text(encoding="utf-8")


def test_main_supports_help(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit, match="0"):
        update_release_version.main(["--help"])

    assert "Target stable release tag" in capsys.readouterr().out
