#!/usr/bin/env python3
"""Schema, validation, and atomic publication for release performance evidence."""

import csv
import hashlib
import io
import json
import math
import os
import re
import tempfile
from collections.abc import Mapping, Sequence  # noqa: TC003
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal, TypeAlias, cast

SCHEMA_VERSION = 1
CSV_COLUMNS = (
    "schema_version",
    "suite",
    "scope",
    "benchmark_id",
    "benchmark_group",
    "benchmark_name",
    "coverage_status",
    "coverage_note",
    "baseline_median_ns",
    "baseline_ci_lower_ns",
    "baseline_ci_upper_ns",
    "current_median_ns",
    "current_ci_lower_ns",
    "current_ci_upper_ns",
    "change_percent",
)
RECOVERY_HINT = "run `just performance-release` to regenerate the retained artifact pair"
COVERAGE_NOTES = {
    "comparable": "measured in both releases",
    "current-only": "benchmark is present only in the current release",
    "baseline-only": "benchmark is present only in the baseline release",
}

CoverageStatus: TypeAlias = Literal["comparable", "current-only", "baseline-only"]  # noqa: UP040
JsonValue: TypeAlias = bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"] | None  # noqa: UP040


class ArtifactValidationError(ValueError):
    """Raised when retained benchmark evidence is missing or inconsistent."""


@dataclass(frozen=True, slots=True)
class TimingEstimate:
    """Criterion median point estimate and confidence interval in nanoseconds."""

    median_ns: float
    ci_lower_ns: float
    ci_upper_ns: float

    def __post_init__(self) -> None:
        values = (self.median_ns, self.ci_lower_ns, self.ci_upper_ns)
        if not all(math.isfinite(value) and value > 0 for value in values):
            raise ArtifactValidationError("timing estimates must be finite positive numbers")
        if not self.ci_lower_ns <= self.median_ns <= self.ci_upper_ns:
            raise ArtifactValidationError("timing confidence interval must contain its median")


@dataclass(frozen=True, slots=True)
class PerformanceRow:
    """One benchmark comparison row retained in the canonical CSV."""

    benchmark_id: str
    benchmark_group: str
    benchmark_name: str
    coverage_status: CoverageStatus
    coverage_note: str
    baseline: TimingEstimate | None
    current: TimingEstimate | None
    suite: str = "ci_performance_suite"
    scope: str = "release-signal"

    def __post_init__(self) -> None:
        if self.suite != "ci_performance_suite" or self.scope != "release-signal":
            raise ArtifactValidationError("performance rows must belong to the ci_performance_suite release signal")
        for label, value in (
            ("benchmark_id", self.benchmark_id),
            ("benchmark_group", self.benchmark_group),
            ("benchmark_name", self.benchmark_name),
            ("coverage_note", self.coverage_note),
        ):
            if not value or value.strip() != value or "\n" in value or "\r" in value:
                raise ArtifactValidationError(f"{label} must be non-empty, trimmed, and single-line")
        expected = {
            "comparable": (True, True),
            "current-only": (False, True),
            "baseline-only": (True, False),
        }[self.coverage_status]
        actual = (self.baseline is not None, self.current is not None)
        if actual != expected:
            raise ArtifactValidationError(f"coverage status {self.coverage_status!r} does not match available estimates")
        if self.coverage_note != COVERAGE_NOTES[self.coverage_status]:
            raise ArtifactValidationError(f"coverage note does not match status {self.coverage_status!r}")

    @property
    def change_percent(self) -> float | None:
        """Return the point-estimate change from baseline, when comparable."""
        if self.baseline is None or self.current is None:
            return None
        return ((self.current.median_ns / self.baseline.median_ns) - 1.0) * 100.0


@dataclass(frozen=True, slots=True)
class ArtifactBundle:
    """Validated retained CSV rows and their matching provenance."""

    rows: tuple[PerformanceRow, ...]
    provenance: dict[str, JsonValue]


def _format_float(value: float | None) -> str:
    return "" if value is None else format(value, ".17g")


def _estimate_columns(estimate: TimingEstimate | None) -> tuple[str, str, str]:
    if estimate is None:
        return "", "", ""
    return _format_float(estimate.median_ns), _format_float(estimate.ci_lower_ns), _format_float(estimate.ci_upper_ns)


def rows_to_csv(rows: Sequence[PerformanceRow]) -> str:
    """Serialize rows deterministically with a fixed schema and ordering."""
    ordered = sorted(rows, key=lambda row: row.benchmark_id)
    if not ordered:
        raise ArtifactValidationError("performance CSV must contain at least one benchmark row")
    if len({row.benchmark_id for row in ordered}) != len(ordered):
        raise ArtifactValidationError("performance CSV benchmark identifiers must be unique")

    output = io.StringIO(newline="")
    writer = csv.DictWriter(output, fieldnames=CSV_COLUMNS, lineterminator="\n")
    writer.writeheader()
    for row in ordered:
        baseline_median, baseline_lower, baseline_upper = _estimate_columns(row.baseline)
        current_median, current_lower, current_upper = _estimate_columns(row.current)
        writer.writerow(
            {
                "schema_version": SCHEMA_VERSION,
                "suite": row.suite,
                "scope": row.scope,
                "benchmark_id": row.benchmark_id,
                "benchmark_group": row.benchmark_group,
                "benchmark_name": row.benchmark_name,
                "coverage_status": row.coverage_status,
                "coverage_note": row.coverage_note,
                "baseline_median_ns": baseline_median,
                "baseline_ci_lower_ns": baseline_lower,
                "baseline_ci_upper_ns": baseline_upper,
                "current_median_ns": current_median,
                "current_ci_lower_ns": current_lower,
                "current_ci_upper_ns": current_upper,
                "change_percent": _format_float(row.change_percent),
            }
        )
    return output.getvalue()


def _parse_float(row: Mapping[str, str], column: str, row_number: int) -> float | None:
    raw = row[column]
    if raw == "":
        return None
    try:
        value = float(raw)
    except ValueError as error:
        raise ArtifactValidationError(f"CSV row {row_number}: {column} is not a number") from error
    if not math.isfinite(value):
        raise ArtifactValidationError(f"CSV row {row_number}: {column} must be finite")
    return value


def _parse_estimate(row: Mapping[str, str], prefix: str, row_number: int) -> TimingEstimate | None:
    values = tuple(_parse_float(row, f"{prefix}_{suffix}_ns", row_number) for suffix in ("median", "ci_lower", "ci_upper"))
    if all(value is None for value in values):
        return None
    if any(value is None for value in values):
        raise ArtifactValidationError(f"CSV row {row_number}: {prefix} estimate is incomplete")
    median, lower, upper = cast("tuple[float, float, float]", values)
    return TimingEstimate(median, lower, upper)


def rows_from_csv(payload: str) -> tuple[PerformanceRow, ...]:
    """Parse and validate canonical performance CSV bytes."""
    reader = csv.DictReader(io.StringIO(payload))
    if tuple(reader.fieldnames or ()) != CSV_COLUMNS:
        raise ArtifactValidationError("performance CSV columns do not match schema version 1")
    rows: list[PerformanceRow] = []
    for row_number, raw_row in enumerate(reader, start=2):
        if None in raw_row or any(value is None for value in raw_row.values()):
            raise ArtifactValidationError(f"CSV row {row_number}: field count does not match the schema")
        row = cast("dict[str, str]", raw_row)
        if row["schema_version"] != str(SCHEMA_VERSION):
            raise ArtifactValidationError(f"CSV row {row_number}: unsupported schema version")
        status_raw = row["coverage_status"]
        if status_raw not in {"comparable", "current-only", "baseline-only"}:
            raise ArtifactValidationError(f"CSV row {row_number}: invalid coverage status")
        baseline = _parse_estimate(row, "baseline", row_number)
        current = _parse_estimate(row, "current", row_number)
        parsed = PerformanceRow(
            benchmark_id=row["benchmark_id"],
            benchmark_group=row["benchmark_group"],
            benchmark_name=row["benchmark_name"],
            coverage_status=status_raw,
            coverage_note=row["coverage_note"],
            baseline=baseline,
            current=current,
            suite=row["suite"],
            scope=row["scope"],
        )
        stored_change = _parse_float(row, "change_percent", row_number)
        expected_change = parsed.change_percent
        if (stored_change is None) != (expected_change is None):
            raise ArtifactValidationError(f"CSV row {row_number}: change_percent does not match coverage")
        if stored_change is not None and expected_change is not None and not math.isclose(stored_change, expected_change, rel_tol=1e-12, abs_tol=1e-12):
            raise ArtifactValidationError(f"CSV row {row_number}: change_percent does not match timing estimates")
        rows.append(parsed)
    if not rows:
        raise ArtifactValidationError("performance CSV must contain at least one benchmark row")
    if tuple(row.benchmark_id for row in rows) != tuple(sorted(row.benchmark_id for row in rows)):
        raise ArtifactValidationError("performance CSV rows must be sorted by benchmark_id")
    if len({row.benchmark_id for row in rows}) != len(rows):
        raise ArtifactValidationError("performance CSV benchmark identifiers must be unique")
    return tuple(rows)


def csv_sha256(payload: str) -> str:
    """Return the digest that binds provenance to the exact CSV bytes."""
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def build_provenance(
    *,
    rows: Sequence[PerformanceRow],
    csv_payload: str,
    current_tag: str,
    baseline_tag: str,
    current_source: Mapping[str, JsonValue],
    baseline_source: Mapping[str, JsonValue],
    host: Mapping[str, JsonValue],
    generated_at: str,
) -> dict[str, JsonValue]:
    """Build provenance that is cryptographically bound to the CSV."""
    return {
        "schema_version": SCHEMA_VERSION,
        "release_pair": {"current_tag": current_tag, "baseline_tag": baseline_tag},
        "generated_at": generated_at,
        "csv": {
            "sha256": csv_sha256(csv_payload),
            "row_count": len(rows),
            "columns": list(CSV_COLUMNS),
        },
        "sources": {"current": dict(current_source), "baseline": dict(baseline_source)},
        "host": dict(host),
    }


def _json_object(payload: str, source: Path) -> dict[str, JsonValue]:
    try:
        parsed = json.loads(payload)
    except json.JSONDecodeError as error:
        raise ArtifactValidationError(f"{source}: invalid provenance JSON: {error.msg}") from error
    if not isinstance(parsed, dict) or not all(isinstance(key, str) for key in parsed):
        raise ArtifactValidationError(f"{source}: provenance root must be an object")
    return cast("dict[str, JsonValue]", parsed)


def _mapping(value: JsonValue, field: str) -> dict[str, JsonValue]:
    if not isinstance(value, dict):
        raise ArtifactValidationError(f"provenance {field} must be an object")
    return value


def _require_string(value: JsonValue, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ArtifactValidationError(f"provenance {field} must be a non-empty string")
    return value


def _validate_host(value: JsonValue, field: str) -> None:
    host = _mapping(value, field)
    for key in ("OS", "CPU", "CPU_CORES", "CPU_THREADS", "MEMORY", "RUST", "TARGET"):
        _require_string(host.get(key), f"{field}.{key}")


def _validate_source(value: JsonValue, field: str, expected_tag: str) -> None:
    source = _mapping(value, field)
    for key in (
        "tag",
        "ref",
        "commit",
        "revision_timestamp",
        "source_state_sha256",
        "benchmark_harness_sha256",
        "benchmark_contract_sha256",
        "cargo_lock_sha256",
        "rustc",
        "criterion",
        "measurement_command",
        "correctness_command",
    ):
        _require_string(source.get(key), f"{field}.{key}")
    if source.get("tag") != expected_tag:
        raise ArtifactValidationError(f"retained CSV/provenance pair mismatch: {field} tag differs from release pair")
    if not isinstance(source.get("git_clean"), bool):
        raise ArtifactValidationError(f"provenance {field}.git_clean must be a Boolean")
    commit = cast("str", source["commit"])
    if re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", commit) is None:
        raise ArtifactValidationError(f"provenance {field}.commit must be a full hexadecimal object ID")
    for key in (
        "source_state_sha256",
        "benchmark_harness_sha256",
        "benchmark_contract_sha256",
        "cargo_lock_sha256",
    ):
        digest = cast("str", source[key])
        if re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            raise ArtifactValidationError(f"provenance {field}.{key} must be a SHA-256 digest")
    _validate_host(source.get("benchmark_host"), f"{field}.benchmark_host")


def validate_provenance(provenance: dict[str, JsonValue], csv_payload: str, row_count: int) -> None:
    """Validate schema, release pair, source records, and CSV binding."""
    if provenance.get("schema_version") != SCHEMA_VERSION:
        raise ArtifactValidationError("provenance uses an unsupported schema version")
    release_pair = _mapping(provenance.get("release_pair"), "release_pair")
    current_tag = _require_string(release_pair.get("current_tag"), "release_pair.current_tag")
    baseline_tag = _require_string(release_pair.get("baseline_tag"), "release_pair.baseline_tag")
    if current_tag == baseline_tag:
        raise ArtifactValidationError("provenance release pair must contain distinct tags")
    csv_record = _mapping(provenance.get("csv"), "csv")
    if csv_record.get("sha256") != csv_sha256(csv_payload):
        raise ArtifactValidationError("retained CSV/provenance pair mismatch: CSV digest differs")
    if csv_record.get("row_count") != row_count:
        raise ArtifactValidationError("retained CSV/provenance pair mismatch: row count differs")
    if csv_record.get("columns") != list(CSV_COLUMNS):
        raise ArtifactValidationError("provenance CSV columns do not match schema version 1")
    sources = _mapping(provenance.get("sources"), "sources")
    for side, expected_tag in (("current", current_tag), ("baseline", baseline_tag)):
        _validate_source(sources.get(side), f"sources.{side}", expected_tag)
    _validate_host(provenance.get("host"), "host")
    _require_string(provenance.get("generated_at"), "generated_at")


def provenance_to_json(provenance: Mapping[str, JsonValue]) -> str:
    """Serialize provenance deterministically."""
    return json.dumps(provenance, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def _write_temp(path: Path, payload: bytes, mode: int | None) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        if mode is not None:
            temporary.chmod(mode)
        return temporary
    except OSError:
        temporary.unlink(missing_ok=True)
        raise


def replace_files_atomically(updates: Mapping[Path, bytes]) -> None:
    """Replace an explicit file set and restore every member if promotion fails."""
    if not updates:
        return
    originals: dict[Path, tuple[bytes, int] | None] = {}
    temporaries: dict[Path, Path] = {}
    promoted: list[Path] = []
    try:
        for path, payload in updates.items():
            originals[path] = (path.read_bytes(), path.stat().st_mode) if path.exists() else None
            original = originals[path]
            mode = original[1] if original is not None else None
            temporaries[path] = _write_temp(path, payload, mode)
        for path in updates:
            temporaries[path].replace(path)
            promoted.append(path)
    except OSError:
        for path in reversed(promoted):
            original = originals[path]
            if original is None:
                path.unlink(missing_ok=True)
            else:
                rollback = _write_temp(path, original[0], original[1])
                rollback.replace(path)
        raise
    finally:
        for temporary in temporaries.values():
            temporary.unlink(missing_ok=True)


def write_bundle(csv_path: Path, provenance_path: Path, rows: Sequence[PerformanceRow], provenance: Mapping[str, JsonValue]) -> None:
    """Validate and atomically retain the canonical CSV/provenance pair."""
    if csv_path.resolve() == provenance_path.resolve():
        raise ArtifactValidationError("CSV and provenance paths must be distinct")
    csv_payload = rows_to_csv(rows)
    normalized = dict(provenance)
    validate_provenance(normalized, csv_payload, len(rows))
    replace_files_atomically(
        {
            csv_path: csv_payload.encode("utf-8"),
            provenance_path: provenance_to_json(normalized).encode("utf-8"),
        }
    )


def load_bundle(csv_path: Path, provenance_path: Path) -> ArtifactBundle:
    """Load the retained pair or report missing members separately from mismatch."""
    missing = [str(path) for path in (csv_path, provenance_path) if not path.is_file()]
    if missing:
        names = ", ".join(missing)
        raise ArtifactValidationError(f"retained performance artifact pair is incomplete; missing: {names}; {RECOVERY_HINT}")
    csv_payload = csv_path.read_text(encoding="utf-8")
    rows = rows_from_csv(csv_payload)
    provenance = _json_object(provenance_path.read_text(encoding="utf-8"), provenance_path)
    validate_provenance(provenance, csv_payload, len(rows))
    return ArtifactBundle(rows, provenance)


def timing_as_dict(estimate: TimingEstimate) -> dict[str, JsonValue]:
    """Return a JSON-compatible timing object for tests and diagnostics."""
    return cast("dict[str, JsonValue]", asdict(estimate))
