#!/usr/bin/env python3
"""Measure, retain, and publish release-to-release Criterion evidence."""

import argparse
import contextlib
import gzip
import hashlib
import html
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from collections.abc import Iterator, Mapping, Sequence  # noqa: TC003
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath
from typing import Literal, TypeAlias, cast

from hardware_utils import HardwareInfo
from performance_artifacts import (
    ArtifactBundle,
    ArtifactValidationError,
    JsonValue,
    PerformanceRow,
    TimingEstimate,
    build_provenance,
    load_bundle,
    replace_files_atomically,
    rows_to_csv,
    write_bundle,
)
from subprocess_utils import find_project_root, run_cargo_command, run_git_command, run_safe_command

REPOSITORY = "acgetchell/causal-triangulations"
STABLE_TAG = re.compile(r"^v(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)$")
CRITERION_SAMPLE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")
MAX_ARCHIVE_FILES = 100_000
MAX_ARCHIVE_BYTES = 10 * 1024 * 1024 * 1024
CSV_RELATIVE_PATH = Path("target/bench-reports/performance.csv")
PROVENANCE_RELATIVE_PATH = Path("target/bench-reports/performance.provenance.json")
REPORT_RELATIVE_PATH = Path("docs/PERFORMANCE.md")
ARCHIVE_INDEX_RELATIVE_PATH = Path("docs/archive/performance/README.md")
VISUAL_RELATIVE_PATH = Path("docs/assets/performance-comparison.svg")
README_START = "<!-- performance-summary:start -->"
README_END = "<!-- performance-summary:end -->"
MEASUREMENT_COMMAND = "cargo bench --locked --profile perf --bench ci_performance_suite -- --noplot"
CORRECTNESS_COMMAND = "cargo test --locked --release --test integration_tests --test physics_integration"

RunMode: TypeAlias = Literal["local", "release"]  # noqa: UP040
BenchmarkResults: TypeAlias = dict[str, TimingEstimate]  # noqa: UP040


@dataclass(frozen=True, slots=True)
class ReleasePair:
    """Resolved current and baseline stable release labels."""

    current_tag: str
    baseline_tag: str


@dataclass(frozen=True, slots=True)
class CheckoutMeasurement:
    """One isolated source state and its parsed Criterion results."""

    results: BenchmarkResults
    source: dict[str, JsonValue]
    criterion_root: Path


def _stable_version(tag: str) -> tuple[int, int, int]:
    match = STABLE_TAG.fullmatch(tag)
    if match is None:
        raise ArtifactValidationError(f"release tag must use stable vMAJOR.MINOR.PATCH form: {tag!r}")
    return int(match.group("major")), int(match.group("minor")), int(match.group("patch"))


def _package_tag(root: Path) -> str:
    with (root / "Cargo.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    package = manifest.get("package")
    if not isinstance(package, dict) or not isinstance(package.get("version"), str):
        raise ArtifactValidationError("Cargo.toml is missing package.version")
    tag = f"v{package['version']}"
    _stable_version(tag)
    return tag


def _published_stable_tags() -> tuple[str, ...]:
    result = run_safe_command(
        "gh",
        ["release", "list", "--repo", REPOSITORY, "--limit", "100", "--json", "tagName,isDraft,isPrerelease,publishedAt"],
    )
    try:
        releases = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ArtifactValidationError("GitHub release listing returned invalid JSON") from error
    if not isinstance(releases, list):
        raise ArtifactValidationError("GitHub release listing must be a JSON array")
    tags: list[str] = []
    for release in releases:
        if not isinstance(release, dict):
            continue
        tag = release.get("tagName")
        if isinstance(tag, str) and STABLE_TAG.fullmatch(tag) and release.get("isDraft") is False and release.get("isPrerelease") is False:
            tags.append(tag)
    return tuple(sorted(set(tags), key=_stable_version, reverse=True))


def resolve_release_pair(
    root: Path,
    *,
    current_tag: str | None,
    baseline_tag: str | None,
    mode: RunMode,
    published_tags: Sequence[str] | None = None,
) -> ReleasePair:
    """Resolve explicit or inferred tags and enforce release ordering."""
    current = current_tag or _package_tag(root)
    current_version = _stable_version(current)
    if baseline_tag is None:
        candidates = tuple(published_tags) if published_tags is not None else _published_stable_tags()
        eligible = tuple(tag for tag in candidates if _stable_version(tag) < current_version)
        if not eligible:
            raise ArtifactValidationError(f"no published stable release predates {current}; pass an explicit baseline tag")
        baseline = max(eligible, key=_stable_version)
    else:
        baseline = baseline_tag
    baseline_version = _stable_version(baseline)
    if current == baseline:
        raise ArtifactValidationError("current and baseline release tags must be distinct")
    if mode == "release" and current_version <= baseline_version:
        raise ArtifactValidationError("release publication requires current_tag to be newer than baseline_tag")
    return ReleasePair(current, baseline)


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _tracked_paths(root: Path) -> tuple[Path, ...]:
    result = run_git_command(["ls-files", "-z"], cwd=root)
    paths = tuple(Path(value) for value in result.stdout.split("\0") if value)
    for path in paths:
        candidate = (root / path).resolve()
        if not candidate.is_relative_to(root.resolve()):
            raise ArtifactValidationError(f"tracked path escapes repository root: {path}")
    return paths


def _source_state_sha256(root: Path) -> str:
    digest = hashlib.sha256()
    for relative in sorted(_tracked_paths(root), key=lambda path: path.as_posix()):
        path = root / relative
        if not path.exists():
            continue
        if not path.is_file() or path.is_symlink():
            raise ArtifactValidationError(f"release benchmark source must contain regular tracked files: {relative}")
        encoded = relative.as_posix().encode("utf-8")
        payload = path.read_bytes()
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    return digest.hexdigest()


def _combined_sha256(root: Path, paths: Sequence[Path]) -> str:
    digest = hashlib.sha256()
    present = 0
    for relative in sorted(paths, key=lambda path: path.as_posix()):
        candidate = root / relative
        if not candidate.is_file():
            continue
        present += 1
        encoded = relative.as_posix().encode("utf-8")
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(candidate.read_bytes())
    if present == 0:
        raise ArtifactValidationError("benchmark contract files are missing from source checkout")
    return digest.hexdigest()


def _criterion_version(root: Path) -> str:
    with (root / "Cargo.lock").open("rb") as handle:
        lock = tomllib.load(handle)
    packages = lock.get("package")
    if not isinstance(packages, list):
        raise ArtifactValidationError("Cargo.lock package records are missing")
    for package in packages:
        if isinstance(package, dict) and package.get("name") == "criterion" and isinstance(package.get("version"), str):
            return cast("str", package["version"])
    raise ArtifactValidationError("Cargo.lock does not contain Criterion")


def _copy_current_tracked_changes(source: Path, destination: Path) -> None:
    changed = run_git_command(["diff", "--name-only", "--no-renames", "-z", "HEAD"], cwd=source).stdout
    for raw in (value for value in changed.split("\0") if value):
        relative = Path(raw)
        source_path = source / relative
        destination_path = destination / relative
        if not destination_path.resolve().is_relative_to(destination.resolve()):
            raise ArtifactValidationError(f"changed path escapes worktree: {relative}")
        if source_path.is_file() and not source_path.is_symlink():
            destination_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_path, destination_path)
        elif not source_path.exists():
            destination_path.unlink(missing_ok=True)
        else:
            raise ArtifactValidationError(f"changed benchmark source must be a regular file: {relative}")


@contextlib.contextmanager
def _detached_worktree(root: Path, ref: str, *, include_current_changes: bool) -> Iterator[Path]:
    with tempfile.TemporaryDirectory(prefix="cdt-performance-worktree-") as temporary:
        checkout = Path(temporary) / "checkout"
        run_git_command(["worktree", "add", "--detach", str(checkout), ref], cwd=root, timeout=120)
        try:
            if include_current_changes:
                _copy_current_tracked_changes(root, checkout)
            yield checkout
        finally:
            run_git_command(["worktree", "remove", "--force", str(checkout)], cwd=root, timeout=120)


def _run_cargo(args: list[str], *, cwd: Path, target_dir: Path, timeout: float) -> None:
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    result = run_cargo_command(args, cwd=cwd, env=environment, timeout=timeout)
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)


def _git_output(root: Path, args: list[str]) -> str:
    return run_git_command(args, cwd=root).stdout.strip()


def _source_record(root: Path, *, label: str, ref: str, source_clean: bool) -> dict[str, JsonValue]:
    rustc = run_safe_command("rustc", ["--version", "--verbose"], cwd=root).stdout.strip()
    return {
        "tag": label,
        "ref": ref,
        "commit": _git_output(root, ["rev-parse", "HEAD"]),
        "revision_timestamp": _git_output(root, ["show", "-s", "--format=%cI", "HEAD"]),
        "git_clean": source_clean,
        "source_state_sha256": _source_state_sha256(root),
        "benchmark_harness_sha256": _sha256_file(root / "benches/ci_performance_suite.rs"),
        "benchmark_contract_sha256": _combined_sha256(
            root,
            (
                Path("Cargo.toml"),
                Path("Cargo.lock"),
                Path("benches/ci_performance_suite.rs"),
                Path("benches/support/or_abort.rs"),
            ),
        ),
        "cargo_lock_sha256": _sha256_file(root / "Cargo.lock"),
        "rustc": rustc,
        "criterion": _criterion_version(root),
        "measurement_command": MEASUREMENT_COMMAND,
        "correctness_command": CORRECTNESS_COMMAND,
        "benchmark_host": cast("dict[str, JsonValue]", HardwareInfo().get_hardware_info(cwd=root)),
    }


def _estimate_from_json(path: Path) -> TimingEstimate:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
        median = document["median"]
        interval = median["confidence_interval"]
        return TimingEstimate(float(median["point_estimate"]), float(interval["lower_bound"]), float(interval["upper_bound"]))
    except (OSError, json.JSONDecodeError, KeyError, TypeError, ValueError) as error:
        raise ArtifactValidationError(f"{path}: malformed Criterion median estimate") from error


def load_criterion_results(criterion_root: Path, sample: str = "new") -> BenchmarkResults:
    """Load one named Criterion sample without trusting report HTML."""
    if CRITERION_SAMPLE.fullmatch(sample) is None:
        raise ArtifactValidationError(f"invalid Criterion sample name: {sample!r}")
    results: BenchmarkResults = {}
    for estimate_path in sorted(criterion_root.glob(f"**/{sample}/estimates.json")):
        relative = estimate_path.relative_to(criterion_root)
        parts = relative.parts
        if len(parts) < 3 or parts[-2] != sample:
            continue
        benchmark_parts = parts[:-2]
        benchmark_id = "/".join(benchmark_parts)
        if benchmark_id in results:
            raise ArtifactValidationError(f"duplicate Criterion benchmark identifier: {benchmark_id}")
        results[benchmark_id] = _estimate_from_json(estimate_path)
    if not results:
        raise ArtifactValidationError(f"no Criterion {sample!r} estimates found below {criterion_root}")
    return results


def compare_results(current: Mapping[str, TimingEstimate], baseline: Mapping[str, TimingEstimate]) -> tuple[PerformanceRow, ...]:
    """Create exhaustive comparable/current-only/baseline-only CSV rows."""
    rows: list[PerformanceRow] = []
    for benchmark_id in sorted(set(current) | set(baseline)):
        current_estimate = current.get(benchmark_id)
        baseline_estimate = baseline.get(benchmark_id)
        if current_estimate is not None and baseline_estimate is not None:
            status: Literal["comparable", "current-only", "baseline-only"] = "comparable"
            note = "measured in both releases"
        elif current_estimate is not None:
            status = "current-only"
            note = "benchmark is present only in the current release"
        else:
            status = "baseline-only"
            note = "benchmark is present only in the baseline release"
        parts = benchmark_id.split("/")
        rows.append(
            PerformanceRow(
                benchmark_id=benchmark_id,
                benchmark_group=parts[0],
                benchmark_name="/".join(parts[1:]) or parts[0],
                coverage_status=status,
                coverage_note=note,
                baseline=baseline_estimate,
                current=current_estimate,
            )
        )
    return tuple(rows)


def _measure_checkout(root: Path, *, label: str, ref: str, include_current_changes: bool) -> CheckoutMeasurement:
    with _detached_worktree(root, ref, include_current_changes=include_current_changes) as checkout:
        target_dir = checkout.parent / "target"
        _run_cargo(
            ["test", "--locked", "--release", "--test", "integration_tests", "--test", "physics_integration"],
            cwd=checkout,
            target_dir=target_dir,
            timeout=3600,
        )
        _run_cargo(
            ["bench", "--locked", "--profile", "perf", "--bench", "ci_performance_suite", "--", "--noplot"],
            cwd=checkout,
            target_dir=target_dir,
            timeout=7200,
        )
        criterion_root = target_dir / "criterion"
        results = load_criterion_results(criterion_root)
        source = _source_record(checkout, label=label, ref=ref, source_clean=not include_current_changes)
        retained = Path(tempfile.mkdtemp(prefix=f"cdt-criterion-{label}-"))
        shutil.copytree(criterion_root, retained / "criterion")
        return CheckoutMeasurement(results, source, retained / "criterion")


def _remove_measurement_copy(measurement: CheckoutMeasurement) -> None:
    parent = measurement.criterion_root.parent
    if parent.name.startswith("cdt-criterion-") and parent.resolve().parent == Path(tempfile.gettempdir()).resolve():
        shutil.rmtree(parent)


def _ensure_git_ref(root: Path, ref: str) -> None:
    result = run_git_command(["rev-parse", "--verify", f"{ref}^{{commit}}"], cwd=root, check=False)
    if result.returncode != 0:
        raise ArtifactValidationError(f"git ref does not resolve to a commit: {ref}")


def _validate_release_source_label(root: Path, tag: str) -> None:
    """Prevent an existing release tag from labeling a different tracked source state."""
    resolved = run_git_command(["rev-parse", "--verify", f"{tag}^{{commit}}"], cwd=root, check=False)
    if resolved.returncode != 0:
        return
    head = _git_output(root, ["rev-parse", "HEAD"])
    tracked_clean = run_git_command(["diff", "--quiet", "HEAD"], cwd=root, check=False).returncode == 0
    if resolved.stdout.strip() != head or not tracked_clean:
        raise ArtifactValidationError(f"existing tag {tag} cannot label a different or modified tracked source state")


def _host_record(root: Path) -> dict[str, JsonValue]:
    return cast("dict[str, JsonValue]", HardwareInfo().get_hardware_info(cwd=root))


def measure_pair(root: Path, pair: ReleasePair, *, publish: bool) -> ArtifactBundle:
    """Measure isolated source states, persist evidence, reload it, then publish."""
    _ensure_git_ref(root, pair.baseline_tag)
    current = _measure_checkout(root, label=pair.current_tag, ref="HEAD", include_current_changes=True)
    try:
        baseline = _measure_checkout(root, label=pair.baseline_tag, ref=pair.baseline_tag, include_current_changes=False)
        try:
            rows = compare_results(current.results, baseline.results)
            csv_payload = rows_to_csv(rows)
            provenance = build_provenance(
                rows=rows,
                csv_payload=csv_payload,
                current_tag=pair.current_tag,
                baseline_tag=pair.baseline_tag,
                current_source=current.source,
                baseline_source=baseline.source,
                host=_host_record(root),
                generated_at=datetime.now(UTC).isoformat(),
            )
            csv_path = root / CSV_RELATIVE_PATH
            provenance_path = root / PROVENANCE_RELATIVE_PATH
            write_bundle(csv_path, provenance_path, rows, provenance)
            bundle = load_bundle(csv_path, provenance_path)
            if publish:
                publish_documentation(root, bundle, include_report=True, include_readme=True)
            return bundle
        finally:
            _remove_measurement_copy(baseline)
    finally:
        _remove_measurement_copy(current)


def _safe_extract_criterion_archive(archive: Path, destination: Path) -> Path:
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, mode="r:gz") as bundle:
        members = bundle.getmembers()
        if not members:
            raise ArtifactValidationError(f"{archive}: empty Criterion archive")
        if len(members) > MAX_ARCHIVE_FILES:
            raise ArtifactValidationError(f"{archive}: Criterion archive contains too many members")
        if sum(member.size for member in members if member.isfile()) > MAX_ARCHIVE_BYTES:
            raise ArtifactValidationError(f"{archive}: Criterion archive exceeds the extraction size limit")
        destinations: set[PurePosixPath] = set()
        for member in members:
            relative = PurePosixPath(member.name)
            if relative.is_absolute() or ".." in relative.parts or not relative.parts or relative.parts[0] != "criterion":
                raise ArtifactValidationError(f"{archive}: unsafe archive member {member.name!r}")
            if member.isdir():
                continue
            if not member.isfile():
                raise ArtifactValidationError(f"{archive}: archive members must be regular files")
            if relative in destinations:
                raise ArtifactValidationError(f"{archive}: duplicate archive member {member.name!r}")
            destinations.add(relative)
            target = destination.joinpath(*relative.parts)
            if not target.resolve().is_relative_to(destination.resolve()):
                raise ArtifactValidationError(f"{archive}: archive member escapes extraction root")
            target.parent.mkdir(parents=True, exist_ok=True)
            source = bundle.extractfile(member)
            if source is None:
                raise ArtifactValidationError(f"{archive}: cannot read archive member {member.name!r}")
            with source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
    return destination / "criterion"


def criterion_asset_name(tag: str) -> str:
    """Return the stable GitHub Release asset name for a tag."""
    _stable_version(tag)
    return f"causal-triangulations-{tag}-criterion-baseline.tar.gz"


def package_criterion_baseline(root: Path, tag: str, output: Path) -> None:
    """Create a path-safe, deterministic native Criterion archive."""
    _stable_version(tag)
    _ensure_git_ref(root, tag)
    if _git_output(root, ["rev-parse", "HEAD"]) != _git_output(root, ["rev-parse", f"{tag}^{{commit}}"]):
        raise ArtifactValidationError(f"native Criterion asset must be packaged from the exact {tag} commit")
    criterion_root = root / "target/criterion"
    load_criterion_results(criterion_root, tag)
    files = sorted(path for path in criterion_root.rglob("*") if path.is_file() and path.name != "release-provenance.json")
    if not files:
        raise ArtifactValidationError("Criterion baseline directory contains no regular files")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp")
    source_clean = run_git_command(["diff", "--quiet", "HEAD"], cwd=root, check=False).returncode == 0
    source_record = _source_record(root, label=tag, ref=tag, source_clean=source_clean)
    source_payload = (json.dumps(source_record, indent=2, sort_keys=True) + "\n").encode("utf-8")
    try:
        with (
            temporary.open("wb") as raw,
            gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as compressed,
            tarfile.open(fileobj=compressed, mode="w") as archive,
        ):
            for path in files:
                if path.is_symlink():
                    raise ArtifactValidationError(f"Criterion baseline contains a symbolic link: {path}")
                relative = Path("criterion") / path.relative_to(criterion_root)
                payload = path.read_bytes()
                member = tarfile.TarInfo(relative.as_posix())
                member.size = len(payload)
                member.mode = path.stat().st_mode & 0o777
                member.mtime = 0
                archive.addfile(member, io.BytesIO(payload))
            source_member = tarfile.TarInfo("criterion/release-provenance.json")
            source_member.size = len(source_payload)
            source_member.mode = 0o644
            source_member.mtime = 0
            archive.addfile(source_member, io.BytesIO(source_payload))
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)


def _download_release_asset(tag: str, destination: Path) -> Path:
    asset = criterion_asset_name(tag)
    destination.mkdir(parents=True, exist_ok=True)
    run_safe_command("gh", ["release", "download", tag, "--repo", REPOSITORY, "--pattern", asset, "--dir", str(destination)])
    path = destination / asset
    if not path.is_file():
        raise ArtifactValidationError(f"GitHub release {tag} did not provide {asset}")
    return path


def compare_github_assets(root: Path, pair: ReleasePair) -> ArtifactBundle:
    """Compare durable native Criterion assets without invoking Cargo."""
    with tempfile.TemporaryDirectory(prefix="cdt-performance-assets-") as temporary:
        temporary_root = Path(temporary)
        current_archive = _download_release_asset(pair.current_tag, temporary_root / "downloads-current")
        baseline_archive = _download_release_asset(pair.baseline_tag, temporary_root / "downloads-baseline")
        current_root = _safe_extract_criterion_archive(current_archive, temporary_root / "current")
        baseline_root = _safe_extract_criterion_archive(baseline_archive, temporary_root / "baseline")
        rows = compare_results(load_criterion_results(current_root, pair.current_tag), load_criterion_results(baseline_root, pair.baseline_tag))
        csv_payload = rows_to_csv(rows)
        current_source = _asset_source_record(pair.current_tag, current_archive, current_root)
        baseline_source = _asset_source_record(pair.baseline_tag, baseline_archive, baseline_root)
        provenance = build_provenance(
            rows=rows,
            csv_payload=csv_payload,
            current_tag=pair.current_tag,
            baseline_tag=pair.baseline_tag,
            current_source=current_source,
            baseline_source=baseline_source,
            host=_host_record(root),
            generated_at=datetime.now(UTC).isoformat(),
        )
        write_bundle(root / CSV_RELATIVE_PATH, root / PROVENANCE_RELATIVE_PATH, rows, provenance)
    return load_bundle(root / CSV_RELATIVE_PATH, root / PROVENANCE_RELATIVE_PATH)


def _asset_source_record(tag: str, archive: Path, criterion_root: Path) -> dict[str, JsonValue]:
    digest = _sha256_file(archive)
    provenance_path = criterion_root / "release-provenance.json"
    try:
        record = json.loads(provenance_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ArtifactValidationError(f"{archive}: missing or malformed release provenance") from error
    if not isinstance(record, dict) or record.get("tag") != tag:
        raise ArtifactValidationError(f"{archive}: release provenance tag does not match {tag}")
    source = cast("dict[str, JsonValue]", record)
    source["asset_sha256"] = digest
    return source


def _release_pair(bundle: ArtifactBundle) -> ReleasePair:
    value = bundle.provenance.get("release_pair")
    if not isinstance(value, dict):
        raise ArtifactValidationError("validated provenance lost its release_pair object")
    current = value.get("current_tag")
    baseline = value.get("baseline_tag")
    if not isinstance(current, str) or not isinstance(baseline, str):
        raise ArtifactValidationError("validated provenance release tags are invalid")
    _stable_version(current)
    _stable_version(baseline)
    return ReleasePair(current, baseline)


def _format_duration(nanoseconds: float | None) -> str:
    if nanoseconds is None:
        return "—"
    units = ((1_000_000_000.0, "s"), (1_000_000.0, "ms"), (1_000.0, "µs"))
    for scale, suffix in units:
        if nanoseconds >= scale:
            return f"{nanoseconds / scale:.3g} {suffix}"
    return f"{nanoseconds:.3g} ns"


def _confidence_relation(row: PerformanceRow) -> str:
    if row.current is None or row.baseline is None:
        return "not comparable"
    if row.current.ci_upper_ns < row.baseline.ci_lower_ns:
        return "lower interval"
    if row.current.ci_lower_ns > row.baseline.ci_upper_ns:
        return "higher interval"
    return "intervals overlap"


def _provenance_object(value: JsonValue, field: str) -> dict[str, JsonValue]:
    if not isinstance(value, dict):
        raise ArtifactValidationError(f"validated provenance lost its {field} object")
    return value


def _provenance_string(value: JsonValue) -> str:
    if not isinstance(value, str):
        raise ArtifactValidationError("validated provenance contains a non-string report field")
    return value


def _provenance_section(
    heading: str,
    source: Mapping[str, JsonValue],
    host: Mapping[str, JsonValue],
) -> list[str]:
    fields = (
        ("Tag", "tag", True),
        ("Commit", "commit", True),
        ("Revision timestamp", "revision_timestamp", False),
        ("Source state SHA-256", "source_state_sha256", True),
        ("Harness SHA-256", "benchmark_harness_sha256", True),
        ("Contract SHA-256", "benchmark_contract_sha256", True),
        ("Cargo.lock SHA-256", "cargo_lock_sha256", True),
        ("Criterion", "criterion", False),
    )
    lines = [f"### {heading}", ""]
    for label, key, code in fields:
        value = _provenance_string(source.get(key)).replace("|", "\\|")
        rendered = f"`{value}`" if code else value
        lines.append(f"- {label}: {rendered}")
    for label, key in (
        ("Operating system", "OS"),
        ("CPU", "CPU"),
        ("CPU cores", "CPU_CORES"),
        ("CPU threads", "CPU_THREADS"),
        ("Memory", "MEMORY"),
        ("Target", "TARGET"),
    ):
        value = _provenance_string(host.get(key)).replace("|", "\\|")
        lines.append(f"- {label}: {value}")
    lines.extend(["", "Rust toolchain:", "", "```text"])
    lines.extend(_provenance_string(source.get("rustc")).splitlines())
    lines.extend(["```", ""])
    return lines


def render_report(bundle: ArtifactBundle) -> str:
    """Render Markdown using only values from a validated retained bundle."""
    pair = _release_pair(bundle)
    comparable = sum(row.coverage_status == "comparable" for row in bundle.rows)
    current_only = sum(row.coverage_status == "current-only" for row in bundle.rows)
    baseline_only = sum(row.coverage_status == "baseline-only" for row in bundle.rows)
    sources = _provenance_object(bundle.provenance.get("sources"), "sources")
    current_source = _provenance_object(sources.get("current"), "sources.current")
    baseline_source = _provenance_object(sources.get("baseline"), "sources.baseline")
    current_host = _provenance_object(current_source.get("benchmark_host"), "sources.current.benchmark_host")
    baseline_host = _provenance_object(baseline_source.get("benchmark_host"), "sources.baseline.benchmark_host")
    lines = [
        "# Release performance",
        "",
        f"This report compares `{pair.current_tag}` with `{pair.baseline_tag}` using the retained `ci_performance_suite` CSV and matching provenance.",
        "It is generated without rerunning Cargo; lower timings are better.",
        "",
        "## Coverage",
        "",
        "| Comparable | Current only | Baseline only |",
        "| ---: | ---: | ---: |",
        f"| {comparable} | {current_only} | {baseline_only} |",
        "",
        "## Measurements",
        "",
        "| Benchmark | Baseline median | Current median | Change | Confidence intervals | Coverage |",
        "| --- | ---: | ---: | ---: | --- | --- |",
    ]
    for row in bundle.rows:
        change = "—" if row.change_percent is None else f"{row.change_percent:+.2f}%"
        benchmark = row.benchmark_id.replace("|", "\\|")
        note = row.coverage_note.replace("|", "\\|")
        lines.append(
            f"| `{benchmark}` | {_format_duration(row.baseline.median_ns if row.baseline else None)} | "
            f"{_format_duration(row.current.median_ns if row.current else None)} | {change} | {_confidence_relation(row)} | {note} |"
        )
    lines.extend(
        [
            "",
            "## Provenance",
            "",
            *_provenance_section("Baseline", baseline_source, baseline_host),
            *_provenance_section("Current", current_source, current_host),
            "## Evidence contract",
            "",
            "The CSV stores every timing, calculated change, and coverage classification shown above. The provenance file binds that CSV by SHA-256 and",
            "records both source states, benchmark harness and configuration hashes, commands, Rust and Criterion versions, and host metadata. Native",
            "Criterion archives remain attached to their GitHub Releases.",
            "",
        ]
    )
    return "\n".join(lines)


def render_visual(bundle: ArtifactBundle) -> str:
    """Render a compact SVG whose plotted values come only from CSV rows."""
    comparable = [row for row in bundle.rows if row.change_percent is not None][:16]
    height = 72 + 28 * max(1, len(comparable))
    pair = _release_pair(bundle)
    elements = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="900" height="{height}" viewBox="0 0 900 {height}" role="img" aria-labelledby="title desc">',
        f'<title id="title">Performance change for {html.escape(pair.current_tag)} versus {html.escape(pair.baseline_tag)}</title>',
        '<desc id="desc">Bars left of center are faster and bars right of center are slower. Values come from the retained performance CSV.</desc>',
        '<rect width="900" height="100%" fill="#fff"/>',
        (
            "<style>text{font:13px system-ui,sans-serif;fill:#24292f}.label{font-size:12px}.good{fill:#2da44e}.bad{fill:#cf222e}"
            ".axis{stroke:#57606a;stroke-width:1}</style>"
        ),
        f'<text x="20" y="28" font-size="18" font-weight="600">{html.escape(pair.current_tag)} vs {html.escape(pair.baseline_tag)}</text>',
        '<text x="20" y="50">Median timing change (lower is better)</text>',
        f'<line class="axis" x1="650" y1="62" x2="650" y2="{height - 10}"/>',
    ]
    if not comparable:
        elements.append('<text x="20" y="82">No comparable benchmarks in the retained CSV.</text>')
    else:
        largest = max(abs(cast("float", row.change_percent)) for row in comparable) or 1.0
        for index, row in enumerate(comparable):
            change = cast("float", row.change_percent)
            width = min(210.0, abs(change) / largest * 210.0)
            y = 72 + index * 28
            x = 650 - width if change < 0 else 650
            css_class = "good" if change <= 0 else "bad"
            anchor = "start" if change >= 0 else "end"
            label = html.escape(row.benchmark_id[:72])
            elements.append(f'<text class="label" x="20" y="{y + 13}">{label}</text>')
            elements.append(f'<rect class="{css_class}" x="{x:.2f}" y="{y}" width="{width:.2f}" height="16" rx="2"/>')
            elements.append(f'<text x="{665 if change >= 0 else 640}" y="{y + 13}" text-anchor="{anchor}">{change:+.2f}%</text>')
    elements.append("</svg>")
    return "\n".join(elements) + "\n"


def _readme_block(bundle: ArtifactBundle) -> str:
    pair = _release_pair(bundle)
    comparable = sum(row.coverage_status == "comparable" for row in bundle.rows)
    current_only = sum(row.coverage_status == "current-only" for row in bundle.rows)
    baseline_only = sum(row.coverage_status == "baseline-only" for row in bundle.rows)
    report_url = f"https://github.com/{REPOSITORY}/blob/{pair.current_tag}/docs/PERFORMANCE.md"
    asset_url = f"https://github.com/{REPOSITORY}/releases/download/{pair.current_tag}/{criterion_asset_name(pair.current_tag)}"
    return "\n".join(
        (
            README_START,
            f"Latest retained comparison: `{pair.current_tag}` against `{pair.baseline_tag}`.",
            "",
            "| Comparable benchmarks | Current only | Baseline only |",
            "| ---: | ---: | ---: |",
            f"| {comparable} | {current_only} | {baseline_only} |",
            "",
            "![Release benchmark comparison](docs/assets/performance-comparison.svg)",
            "",
            f"[Tag-pinned full report]({report_url}) ·  ",
            f"[Native Criterion baseline]({asset_url})",
            README_END,
        )
    )


def _replace_readme_block(readme: str, block: str) -> str:
    start = readme.find(README_START)
    end = readme.find(README_END)
    if start < 0 or end < 0 or end < start:
        raise ArtifactValidationError("README performance summary markers are missing or malformed")
    end += len(README_END)
    return readme[:start] + block + readme[end:]


def _archive_index(root: Path, archive_relative: Path, pair: ReleasePair) -> str:
    index_path = root / ARCHIVE_INDEX_RELATIVE_PATH
    existing: list[str] = []
    if index_path.is_file():
        existing = [line for line in index_path.read_text(encoding="utf-8").splitlines() if line.startswith("- [")]
    entry = f"- [{pair.current_tag} vs {pair.baseline_tag}]({archive_relative.name})"
    entries = sorted({*existing, entry}, reverse=True)
    return "# Archived performance reports\n\nThese reports are generated from retained CSV/provenance pairs.\n\n" + "\n".join(entries) + "\n"


def _checked_publication_paths(root: Path, pair: ReleasePair) -> dict[str, Path]:
    archive_name = f"{pair.current_tag}-vs-{pair.baseline_tag}.md"
    relative = {
        "report": REPORT_RELATIVE_PATH,
        "archive_index": ARCHIVE_INDEX_RELATIVE_PATH,
        "archive_report": Path("docs/archive/performance") / archive_name,
        "visual": VISUAL_RELATIVE_PATH,
        "readme": Path("README.md"),
    }
    resolved_root = root.resolve()
    artifact_paths = {(root / CSV_RELATIVE_PATH).resolve(), (root / PROVENANCE_RELATIVE_PATH).resolve()}
    paths: dict[str, Path] = {}
    for name, path in relative.items():
        resolved = (root / path).resolve()
        if not resolved.is_relative_to(resolved_root):
            raise ArtifactValidationError(f"tracked publication destination escapes repository root: {path}")
        if resolved in artifact_paths:
            raise ArtifactValidationError(f"tracked publication destination overlaps retained artifacts: {path}")
        paths[name] = resolved
    if len(set(paths.values())) != len(paths):
        raise ArtifactValidationError("tracked publication destinations must be distinct")
    return paths


def publish_documentation(root: Path, bundle: ArtifactBundle, *, include_report: bool, include_readme: bool) -> None:
    """Atomically publish only the explicit tracked destination set."""
    pair = _release_pair(bundle)
    paths = _checked_publication_paths(root, pair)
    updates: dict[Path, bytes] = {}
    visual = render_visual(bundle).encode("utf-8")
    if include_report:
        report = render_report(bundle).encode("utf-8")
        updates[paths["report"]] = report
        updates[paths["archive_report"]] = report
        archive_relative = paths["archive_report"].relative_to(paths["archive_index"].parent)
        updates[paths["archive_index"]] = _archive_index(root, archive_relative, pair).encode("utf-8")
        updates[paths["visual"]] = visual
    if include_readme:
        readme = paths["readme"].read_text(encoding="utf-8")
        updates[paths["readme"]] = _replace_readme_block(readme, _readme_block(bundle)).encode("utf-8")
        updates[paths["visual"]] = visual
    replace_files_atomically(updates)


def _load_retained(root: Path, bundle_dir: Path | None = None) -> ArtifactBundle:
    directory = root / CSV_RELATIVE_PATH.parent if bundle_dir is None else bundle_dir.resolve()
    return load_bundle(directory / CSV_RELATIVE_PATH.name, directory / PROVENANCE_RELATIVE_PATH.name)


def _print_comparison(rows: Sequence[PerformanceRow]) -> None:
    print("benchmark\tbaseline\tcurrent\tchange\tcoverage")
    for row in rows:
        change = "n/a" if row.change_percent is None else f"{row.change_percent:+.2f}%"
        print(
            f"{row.benchmark_id}\t{_format_duration(row.baseline.median_ns if row.baseline else None)}\t"
            f"{_format_duration(row.current.median_ns if row.current else None)}\t{change}\t{row.coverage_status}"
        )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    compare = subparsers.add_parser("compare", help="compare current Criterion output with a named saved baseline")
    compare.add_argument("--criterion-root", type=Path, default=Path("target/criterion"))
    compare.add_argument("--baseline", default="last")
    for name in ("local", "release", "github-assets"):
        command = subparsers.add_parser(name)
        command.add_argument("--current-tag")
        command.add_argument("--baseline-tag")
    document = subparsers.add_parser("document", help="render tracked reports from retained evidence only")
    document.add_argument("--bundle-dir", type=Path)
    document.add_argument("--readme-only", action="store_true")
    package = subparsers.add_parser("package-baseline", help="package a named Criterion baseline for a GitHub Release")
    package.add_argument("--tag", required=True)
    package.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Run the requested release-performance operation."""
    arguments = _parser().parse_args(argv)
    root = find_project_root()
    try:
        if arguments.command == "compare":
            current = load_criterion_results(root / arguments.criterion_root, "new")
            baseline = load_criterion_results(root / arguments.criterion_root, arguments.baseline)
            _print_comparison(compare_results(current, baseline))
        elif arguments.command == "package-baseline":
            package_criterion_baseline(root, arguments.tag, root / arguments.output)
        elif arguments.command == "document":
            bundle = _load_retained(root, arguments.bundle_dir)
            publish_documentation(root, bundle, include_report=not arguments.readme_only, include_readme=arguments.readme_only)
        else:
            mode: RunMode = "release" if arguments.command in {"release", "github-assets"} else "local"
            pair = resolve_release_pair(
                root,
                current_tag=arguments.current_tag,
                baseline_tag=arguments.baseline_tag,
                mode=mode,
            )
            if arguments.command != "github-assets" and pair.current_tag != _package_tag(root):
                raise ArtifactValidationError("measured current tag must match Cargo.toml package.version")
            if arguments.command == "release":
                _validate_release_source_label(root, pair.current_tag)
            if arguments.command == "github-assets":
                bundle = compare_github_assets(root, pair)
                _print_comparison(bundle.rows)
            else:
                bundle = measure_pair(root, pair, publish=arguments.command == "release")
                _print_comparison(bundle.rows)
        return 0
    except (ArtifactValidationError, OSError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
