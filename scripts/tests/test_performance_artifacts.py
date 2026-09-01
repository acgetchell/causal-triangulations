"""Tests for retained release-performance CSV and provenance evidence."""

import json
from collections.abc import Callable  # noqa: TC003
from pathlib import Path

import pytest

import performance_artifacts
from performance_artifacts import ArtifactValidationError, PerformanceRow, TimingEstimate


def _estimate(value: float) -> TimingEstimate:
    return TimingEstimate(value, value * 0.9, value * 1.1)


def _rows() -> tuple[PerformanceRow, ...]:
    return (
        PerformanceRow(
            benchmark_id="generation/strip/64",
            benchmark_group="generation",
            benchmark_name="strip/64",
            coverage_status="comparable",
            coverage_note="measured in both releases",
            baseline=_estimate(100.0),
            current=_estimate(90.0),
        ),
        PerformanceRow(
            benchmark_id="validation/new/64",
            benchmark_group="validation",
            benchmark_name="new/64",
            coverage_status="current-only",
            coverage_note="benchmark is present only in the current release",
            baseline=None,
            current=_estimate(50.0),
        ),
    )


def _source(tag: str) -> dict[str, performance_artifacts.JsonValue]:
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
        "measurement_command": "cargo bench",
        "correctness_command": "cargo test",
        "benchmark_host": _host(),
    }


def _host() -> dict[str, performance_artifacts.JsonValue]:
    return dict.fromkeys(("OS", "CPU", "CPU_CORES", "CPU_THREADS", "MEMORY", "RUST", "TARGET"), "fixture")


def _provenance(rows: tuple[PerformanceRow, ...]) -> dict[str, performance_artifacts.JsonValue]:
    csv_payload = performance_artifacts.rows_to_csv(rows)
    return performance_artifacts.build_provenance(
        rows=rows,
        csv_payload=csv_payload,
        current_tag="v0.2.0",
        baseline_tag="v0.1.0",
        current_source=_source("v0.2.0"),
        baseline_source=_source("v0.1.0"),
        host=_host(),
        generated_at="2026-08-31T12:00:00+00:00",
    )


def test_bundle_round_trip_preserves_rows_and_binds_exact_csv(tmp_path: Path) -> None:
    rows = _rows()
    csv_path = tmp_path / "performance.csv"
    provenance_path = tmp_path / "performance.provenance.json"

    performance_artifacts.write_bundle(csv_path, provenance_path, rows, _provenance(rows))
    loaded = performance_artifacts.load_bundle(csv_path, provenance_path)

    assert loaded.rows == rows
    assert loaded.provenance["csv"] == {
        "columns": list(performance_artifacts.CSV_COLUMNS),
        "row_count": 2,
        "sha256": performance_artifacts.csv_sha256(csv_path.read_text(encoding="utf-8")),
    }


def test_missing_pair_reports_recovery_before_any_publication(tmp_path: Path) -> None:
    csv_path = tmp_path / "performance.csv"
    csv_path.write_text("incomplete\n", encoding="utf-8")

    with pytest.raises(ArtifactValidationError, match=r"pair is incomplete.*performance\.provenance\.json.*just performance-release"):
        performance_artifacts.load_bundle(csv_path, tmp_path / "performance.provenance.json")


def test_existing_but_mismatched_pair_has_distinct_diagnostic(tmp_path: Path) -> None:
    rows = _rows()
    csv_path = tmp_path / "performance.csv"
    provenance_path = tmp_path / "performance.provenance.json"
    performance_artifacts.write_bundle(csv_path, provenance_path, rows, _provenance(rows))
    csv_path.write_text(csv_path.read_text(encoding="utf-8").replace("90", "91", 1), encoding="utf-8")

    with pytest.raises(ArtifactValidationError, match="pair mismatch: CSV digest differs"):
        performance_artifacts.load_bundle(csv_path, provenance_path)


@pytest.mark.parametrize(
    ("mutator", "message"),
    [
        (lambda text: text.replace("schema_version", "wrong", 1), "columns do not match"),
        (lambda text: text.replace("measured in both releases", ""), "coverage_note"),
        (lambda text: text.replace(",-9.9999999999999982\n", ",-999\n", 1), "change_percent does not match"),
    ],
)
def test_malformed_csv_is_rejected(mutator: Callable[[str], str], message: str) -> None:
    payload = performance_artifacts.rows_to_csv(_rows())

    with pytest.raises(ArtifactValidationError, match=message):
        performance_artifacts.rows_from_csv(mutator(payload))


def test_coverage_status_must_match_available_estimates() -> None:
    with pytest.raises(ArtifactValidationError, match="does not match available estimates"):
        PerformanceRow(
            benchmark_id="group/name",
            benchmark_group="group",
            benchmark_name="name",
            coverage_status="comparable",
            coverage_note="measured in both releases",
            baseline=None,
            current=_estimate(1.0),
        )


def test_atomic_multi_file_promotion_rolls_back_first_file(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    first = tmp_path / "first.md"
    second = tmp_path / "second.md"
    first.write_text("old first", encoding="utf-8")
    second.write_text("old second", encoding="utf-8")
    original_replace = Path.replace

    def fail_second(source: Path, target: Path) -> Path:
        if Path(target) == second and source.name.startswith(".second.md."):
            message = "injected second-file failure"
            raise OSError(message)
        return original_replace(source, target)

    monkeypatch.setattr(Path, "replace", fail_second)

    with pytest.raises(OSError, match="injected second-file failure"):
        performance_artifacts.replace_files_atomically({first: b"new first", second: b"new second"})

    assert first.read_text(encoding="utf-8") == "old first"
    assert second.read_text(encoding="utf-8") == "old second"


def test_provenance_json_is_deterministic() -> None:
    provenance = _provenance(_rows())

    first = performance_artifacts.provenance_to_json(provenance)
    second = performance_artifacts.provenance_to_json(json.loads(first))

    assert first == second
