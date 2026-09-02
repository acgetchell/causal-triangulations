"""Tests for release benchmark orchestration and tracked publication."""

import io
import subprocess
import tarfile
from pathlib import Path

import pytest

import release_performance
from performance_artifacts import ArtifactBundle, ArtifactValidationError, PerformanceRow, TimingEstimate


def _estimate(value: float) -> TimingEstimate:
    return TimingEstimate(value, value * 0.9, value * 1.1)


def _bundle() -> ArtifactBundle:
    rows = (
        PerformanceRow(
            benchmark_id="generation/strip/64",
            benchmark_group="generation",
            benchmark_name="strip/64",
            coverage_status="comparable",
            coverage_note="measured in both releases",
            baseline=_estimate(100.0),
            current=_estimate(80.0),
        ),
        PerformanceRow(
            benchmark_id="validation/current/64",
            benchmark_group="validation",
            benchmark_name="current/64",
            coverage_status="current-only",
            coverage_note="benchmark is present only in the current release",
            baseline=None,
            current=_estimate(25.0),
        ),
    )
    host = dict.fromkeys(("OS", "CPU", "CPU_CORES", "CPU_THREADS", "MEMORY", "RUST", "TARGET"), "fixture")
    source = {
        "tag": "v0.2.0",
        "commit": "a" * 40,
        "revision_timestamp": "2026-08-31T12:00:00+00:00",
        "source_state_sha256": "b" * 64,
        "benchmark_harness_sha256": "c" * 64,
        "benchmark_contract_sha256": "d" * 64,
        "cargo_lock_sha256": "e" * 64,
        "rustc": "rustc 1.98.0\nbinary: rustc\nhost: fixture",
        "criterion": "0.8.2",
        "benchmark_host": host,
    }
    baseline = {**source, "tag": "v0.1.0", "commit": "f" * 40}
    return ArtifactBundle(
        rows,
        {
            "release_pair": {"current_tag": "v0.2.0", "baseline_tag": "v0.1.0"},
            "sources": {"current": source, "baseline": baseline},
        },
    )


def _write_manifest(root: Path, version: str = "0.2.0") -> None:
    (root / "Cargo.toml").write_text(f'[package]\nname = "fixture"\nversion = "{version}"\n', encoding="utf-8")


def _write_readme(root: Path) -> str:
    original = f"# Fixture\n\n{release_performance.README_START}\nPending retained comparison.\n{release_performance.README_END}\n\n## More\n"
    (root / "README.md").write_text(original, encoding="utf-8")
    return original


def _complete_source(tag: str) -> dict[str, release_performance.JsonValue]:
    host = dict.fromkeys(("OS", "CPU", "CPU_CORES", "CPU_THREADS", "MEMORY", "RUST", "TARGET"), "fixture")
    return {
        "tag": tag,
        "ref": tag,
        "commit": "a" * 40,
        "revision_timestamp": "2026-08-31T12:00:00+00:00",
        "git_clean": True,
        "source_state_sha256": "b" * 64,
        "benchmark_harness_sha256": "c" * 64,
        "benchmark_contract_sha256": "d" * 64,
        "cargo_lock_sha256": "e" * 64,
        "rustc": "rustc 1.98.0",
        "criterion": "0.8.2",
        "measurement_command": release_performance.MEASUREMENT_COMMAND,
        "correctness_command": release_performance.CORRECTNESS_COMMAND,
        "benchmark_host": host,
    }


def test_explicit_release_pair_does_not_query_github(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    _write_manifest(tmp_path)
    monkeypatch.setattr(release_performance, "_published_stable_tags", lambda: pytest.fail("unexpected GitHub query"))

    pair = release_performance.resolve_release_pair(
        tmp_path,
        current_tag="v0.2.0",
        baseline_tag="v0.1.0",
        mode="release",
    )

    assert pair == release_performance.ReleasePair("v0.2.0", "v0.1.0")


def test_inferred_baseline_uses_latest_preceding_stable_release(tmp_path: Path) -> None:
    _write_manifest(tmp_path, "0.3.0")

    pair = release_performance.resolve_release_pair(
        tmp_path,
        current_tag=None,
        baseline_tag=None,
        mode="release",
        published_tags=("v0.1.0", "v0.2.0", "v0.4.0"),
    )

    assert pair == release_performance.ReleasePair("v0.3.0", "v0.2.0")


@pytest.mark.parametrize(("current", "baseline"), [("0.2.0", "v0.1.0"), ("v0.1.0-rc.1", "v0.1.0"), ("v0.1.0", "v0.1.0")])
def test_invalid_or_equal_release_pairs_are_rejected(tmp_path: Path, current: str, baseline: str) -> None:
    _write_manifest(tmp_path)

    with pytest.raises(ArtifactValidationError):
        release_performance.resolve_release_pair(tmp_path, current_tag=current, baseline_tag=baseline, mode="release")


def test_existing_release_tag_cannot_label_modified_tracked_source(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    def git_command(args: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        if args[:2] == ["rev-parse", "--verify"]:
            return subprocess.CompletedProcess(args, 0, "a" * 40 + "\n", "")
        if args[:2] == ["diff", "--quiet"]:
            return subprocess.CompletedProcess(args, 1, "", "")
        raise AssertionError(f"unexpected git arguments: {args}")

    monkeypatch.setattr(release_performance, "run_git_command", git_command)
    monkeypatch.setattr(release_performance, "_git_output", lambda *_args: "a" * 40)

    with pytest.raises(ArtifactValidationError, match="cannot label a different or modified"):
        release_performance._validate_release_source_label(tmp_path, "v0.2.0")


def test_criterion_parser_and_coverage_union(tmp_path: Path) -> None:
    current_path = tmp_path / "current" / "criterion" / "generation" / "strip" / "64" / "new"
    baseline_path = tmp_path / "baseline" / "criterion" / "generation" / "strip" / "64" / "last"
    only_path = tmp_path / "current" / "criterion" / "validation" / "only" / "new"
    for path, value in ((current_path, 80.0), (baseline_path, 100.0), (only_path, 25.0)):
        path.mkdir(parents=True)
        path.joinpath("estimates.json").write_text(
            f'{{"median":{{"point_estimate":{value},"confidence_interval":{{"lower_bound":{value * 0.9},"upper_bound":{value * 1.1}}}}}}}',
            encoding="utf-8",
        )

    current = release_performance.load_criterion_results(tmp_path / "current/criterion")
    baseline = release_performance.load_criterion_results(tmp_path / "baseline/criterion", "last")
    rows = release_performance.compare_results(current, baseline)

    assert [row.benchmark_id for row in rows] == ["generation/strip/64", "validation/only"]
    assert [row.coverage_status for row in rows] == ["comparable", "current-only"]
    assert rows[0].change_percent == pytest.approx(-20.0)


def test_release_measurement_persists_and_reloads_before_cleanup(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    _write_readme(tmp_path)
    current_results = {"generation/strip": _estimate(80.0), "validation/new": _estimate(25.0)}
    baseline_results = {"generation/strip": _estimate(100.0)}
    measurements = {
        "v0.2.0": release_performance.CheckoutMeasurement(current_results, _complete_source("v0.2.0"), tmp_path / "current/criterion"),
        "v0.1.0": release_performance.CheckoutMeasurement(baseline_results, _complete_source("v0.1.0"), tmp_path / "baseline/criterion"),
    }
    monkeypatch.setattr(release_performance, "_ensure_git_ref", lambda *_args: None)

    def measure(
        _root: Path,
        *,
        label: str,
        ref: str,
        include_current_changes: bool,
    ) -> release_performance.CheckoutMeasurement:
        del ref, include_current_changes
        return measurements[label]

    monkeypatch.setattr(release_performance, "_measure_checkout", measure)
    host = _complete_source("v0.2.0")["benchmark_host"]
    assert isinstance(host, dict)
    monkeypatch.setattr(release_performance, "_host_record", lambda _root: host)
    cleaned: list[str] = []

    def cleanup(measurement: release_performance.CheckoutMeasurement) -> None:
        assert (tmp_path / release_performance.CSV_RELATIVE_PATH).is_file()
        assert (tmp_path / release_performance.PROVENANCE_RELATIVE_PATH).is_file()
        tag = measurement.source["tag"]
        assert isinstance(tag, str)
        cleaned.append(tag)

    monkeypatch.setattr(release_performance, "_remove_measurement_copy", cleanup)

    bundle = release_performance.measure_pair(
        tmp_path,
        release_performance.ReleasePair("v0.2.0", "v0.1.0"),
        publish=True,
    )

    assert [row.coverage_status for row in bundle.rows] == ["comparable", "current-only"]
    assert cleaned == ["v0.1.0", "v0.2.0"]
    assert "-20.00%" in (tmp_path / "docs/PERFORMANCE.md").read_text(encoding="utf-8")


def test_safe_archive_extraction_rejects_traversal(tmp_path: Path) -> None:
    archive = tmp_path / "unsafe.tar.gz"
    with tarfile.open(archive, "w:gz") as bundle:
        member = tarfile.TarInfo("criterion/../../README.md")
        member.size = 4
        bundle.addfile(member, io.BytesIO(b"oops"))

    with pytest.raises(ArtifactValidationError, match="unsafe archive member"):
        release_performance._safe_extract_criterion_archive(archive, tmp_path / "output")

    assert not (tmp_path / "README.md").exists()


def test_safe_archive_extraction_rejects_links(tmp_path: Path) -> None:
    archive = tmp_path / "link.tar.gz"
    with tarfile.open(archive, "w:gz") as bundle:
        member = tarfile.TarInfo("criterion/link")
        member.type = tarfile.SYMTYPE
        member.linkname = "../../README.md"
        bundle.addfile(member)

    with pytest.raises(ArtifactValidationError, match="regular files"):
        release_performance._safe_extract_criterion_archive(archive, tmp_path / "output")


def test_native_archive_round_trip_is_deterministic_and_retains_source_provenance(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    sample = tmp_path / "target/criterion/generation/strip/v0.2.0"
    sample.mkdir(parents=True)
    sample.joinpath("estimates.json").write_text(
        '{"median":{"point_estimate":80,"confidence_interval":{"lower_bound":72,"upper_bound":88}}}',
        encoding="utf-8",
    )
    source = {"tag": "v0.2.0", "commit": "a" * 40}
    monkeypatch.setattr(release_performance, "_source_record", lambda *_args, **_kwargs: source)
    monkeypatch.setattr(
        release_performance,
        "run_git_command",
        lambda *_args, **_kwargs: subprocess.CompletedProcess([], 0, "", ""),
    )
    first = tmp_path / "first.tar.gz"
    second = tmp_path / "second.tar.gz"

    release_performance.package_criterion_baseline(tmp_path, "v0.2.0", first)
    release_performance.package_criterion_baseline(tmp_path, "v0.2.0", second)
    criterion_root = release_performance._safe_extract_criterion_archive(first, tmp_path / "extracted")

    assert first.read_bytes() == second.read_bytes()
    assert release_performance.load_criterion_results(criterion_root, "v0.2.0")["generation/strip"].median_ns == 80
    retained_source = release_performance._asset_source_record("v0.2.0", first, criterion_root)
    assert retained_source["commit"] == "a" * 40
    assert retained_source["asset_sha256"] == release_performance._sha256_file(first)


def test_documentation_is_rendered_only_from_bundle(tmp_path: Path) -> None:
    _write_readme(tmp_path)

    release_performance.publish_documentation(tmp_path, _bundle(), include_report=True, include_readme=True)

    report = (tmp_path / "docs/PERFORMANCE.md").read_text(encoding="utf-8")
    readme = (tmp_path / "README.md").read_text(encoding="utf-8")
    assert "80 ns" in report
    assert "-20.00%" in report
    assert "rustc 1.98.0\nbinary: rustc\nhost: fixture" in report
    assert "### Baseline" in report
    assert "### Current" in report
    assert "<br>" not in report
    assert "| 1 | 1 | 0 |" in readme
    assert "blob/v0.2.0/docs/PERFORMANCE.md" in readme
    assert ") ·\n[Native Criterion baseline]" in readme
    assert (tmp_path / "docs/archive/performance/v0.2.0-vs-v0.1.0.md").read_text(encoding="utf-8") == report
    assert (tmp_path / "docs/assets/performance-comparison.svg").is_file()


def test_publication_failure_rolls_back_every_tracked_destination(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    original_readme = _write_readme(tmp_path)
    report = tmp_path / "docs/PERFORMANCE.md"
    report.parent.mkdir(parents=True)
    report.write_text("old report\n", encoding="utf-8")
    original_replace = Path.replace
    readme = tmp_path / "README.md"

    def fail_readme(source: Path, target: Path) -> Path:
        if Path(target) == readme and source.name.startswith(".README.md."):
            message = "injected README failure"
            raise OSError(message)
        return original_replace(source, target)

    monkeypatch.setattr(Path, "replace", fail_readme)

    with pytest.raises(OSError, match="injected README failure"):
        release_performance.publish_documentation(tmp_path, _bundle(), include_report=True, include_readme=True)

    assert readme.read_text(encoding="utf-8") == original_readme
    assert report.read_text(encoding="utf-8") == "old report\n"
    assert not (tmp_path / "docs/archive/performance/v0.2.0-vs-v0.1.0.md").exists()
    assert not (tmp_path / "docs/assets/performance-comparison.svg").exists()


def test_readme_requires_owned_markers(tmp_path: Path) -> None:
    (tmp_path / "README.md").write_text("# No generated block\n", encoding="utf-8")

    with pytest.raises(ArtifactValidationError, match="markers are missing"):
        release_performance.publish_documentation(tmp_path, _bundle(), include_report=False, include_readme=True)


def test_asset_names_are_tag_pinned_and_path_safe() -> None:
    assert release_performance.criterion_asset_name("v1.2.3") == "causal-triangulations-v1.2.3-criterion-baseline.tar.gz"
    with pytest.raises(ArtifactValidationError):
        release_performance.criterion_asset_name("../../latest")
