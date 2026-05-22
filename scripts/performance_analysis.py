#!/usr/bin/env python3
"""Advanced performance analysis and reporting for CDT benchmarks.

Provides detailed statistics, trend analysis, and regression detection.

Requires Python 3.12+.
"""

import argparse
import json
import math
import re
import shutil
import statistics
import subprocess
import sys
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import TYPE_CHECKING, Literal, NotRequired, TypedDict, cast

if TYPE_CHECKING:
    from subprocess_utils import ExecutableNotFoundError, run_cargo_command
else:
    try:
        # When executed as a script from scripts/
        from subprocess_utils import ExecutableNotFoundError, run_cargo_command
    except ModuleNotFoundError:
        # When imported as a module (e.g., scripts.performance_analysis)
        from scripts.subprocess_utils import ExecutableNotFoundError, run_cargo_command


class CriterionEstimate(TypedDict):
    mean_ns: float
    std_dev_ns: float
    median_ns: float
    mad_ns: float
    timestamp: str
    mean_ci_lower: NotRequired[float]
    mean_ci_upper: NotRequired[float]


class NewBenchmark(TypedDict):
    benchmark: str
    mean_ns: float


class BenchmarkChange(TypedDict):
    benchmark: str
    change_percent: float
    current_ns: float
    baseline_ns: float
    current_std: float
    baseline_std: float


class ComparisonSummary(TypedDict):
    total_benchmarks: int
    regressions: int
    improvements: int
    stable: int
    new: int
    avg_change: float
    median_change: float
    max_regression: float
    max_improvement: float


class ComparisonResult(TypedDict):
    regressions: list[BenchmarkChange]
    improvements: list[BenchmarkChange]
    new_benchmarks: list[NewBenchmark]
    stable: list[BenchmarkChange]
    summary: ComparisonSummary | None


# --- Trend analysis types ---
TrendDirection = Literal["improving", "degrading", "stable"]


class TrendInfo(TypedDict):
    slope: float
    trend: TrendDirection
    data_points: int
    first_value: float
    last_value: float
    change_percent: float


class TrendAnalysisSuccess(TypedDict):
    period_days: int
    baselines_analyzed: int
    trends: dict[str, TrendInfo]


class TrendAnalysisError(TypedDict):
    error: str


TrendAnalysisResult = TrendAnalysisSuccess | TrendAnalysisError


# LoadedBaseline for trend analysis
class LoadedBaseline(TypedDict):
    timestamp: datetime
    file: Path
    data: dict[str, CriterionEstimate]


_BASELINE_REQUIRED_KEYS: set[str] = {
    "mad_ns",
    "mean_ns",
    "median_ns",
    "std_dev_ns",
    "timestamp",
}
_BASELINE_NUMERIC_KEYS: set[str] = {"mad_ns", "mean_ns", "median_ns", "std_dev_ns"}
_BASELINE_OPTIONAL_NUMERIC_KEYS: set[str] = {"mean_ci_lower", "mean_ci_upper"}


def _baseline_entry_validation_error(
    benchmark: object,
    estimate: object,
    *,
    baseline_path: Path,
) -> str | None:
    error: str | None = None

    if not isinstance(benchmark, str):
        error = f"Warning: Invalid benchmark name {benchmark!r} in baseline {baseline_path}"
    elif not isinstance(estimate, dict):
        error = f"Warning: Invalid entry '{benchmark}' in baseline {baseline_path}"
    else:
        estimate_dict = cast("dict[str, object]", estimate)

        missing = _BASELINE_REQUIRED_KEYS.difference(estimate_dict.keys())
        if missing:
            missing_display = ", ".join(sorted(missing))
            error = f"Warning: Missing required keys in '{benchmark}' in baseline {baseline_path}: {missing_display}"
        else:
            for key in _BASELINE_NUMERIC_KEYS:
                value = estimate_dict.get(key)
                if not isinstance(value, (int, float)):
                    error = f"Warning: Invalid '{key}' in '{benchmark}' in baseline {baseline_path}"
                    break

            if error is None:
                timestamp = estimate_dict.get("timestamp")
                if not isinstance(timestamp, str):
                    error = f"Warning: Invalid 'timestamp' in '{benchmark}' in baseline {baseline_path}"
                else:
                    for key in _BASELINE_OPTIONAL_NUMERIC_KEYS:
                        if key in estimate_dict and not isinstance(estimate_dict.get(key), (int, float)):
                            error = f"Warning: Invalid '{key}' in '{benchmark}' in baseline {baseline_path}"
                            break

    return error


def _validated_baseline_data(
    data: object,
    *,
    baseline_path: Path,
) -> dict[str, CriterionEstimate] | None:
    if not isinstance(data, dict):
        return None

    for benchmark, estimate in data.items():
        error = _baseline_entry_validation_error(
            benchmark,
            estimate,
            baseline_path=baseline_path,
        )
        if error is not None:
            print(error)
            return None

    return cast("dict[str, CriterionEstimate]", data)


class PerformanceAnalyzer:
    """Advanced performance analysis for CDT benchmarks."""

    def __init__(self, project_root: Path):
        self.project_root = project_root
        self.baseline_dir = project_root / "performance_baselines"
        self.results_dir = project_root / "target" / "criterion"
        self.reports_dir = project_root / "performance_reports"

        # Ensure directories exist
        self.baseline_dir.mkdir(exist_ok=True)
        self.reports_dir.mkdir(exist_ok=True)

    def run_benchmarks(self) -> bool:
        """Run cargo bench and return success status."""
        print("🏃 Running benchmarks...")
        try:
            result = run_cargo_command(
                [
                    "bench",
                    "--profile",
                    "perf",
                    "--bench",
                    "ci_performance_suite",
                    "--message-format=json",
                ],
                cwd=self.project_root,
                check=False,
                timeout=600,  # 10 minute timeout
            )

            if result.returncode != 0:
                print("❌ Benchmark execution failed:")
                print(result.stderr)
                return False

            print("✅ Benchmarks completed successfully")
            return True

        except ExecutableNotFoundError as exc:
            print(f"❌ {exc}")
            return False
        except subprocess.TimeoutExpired:
            print("❌ Benchmark execution timed out")
            return False
        except OSError as exc:
            print(f"❌ Error running benchmarks: {exc}")
            return False

    @staticmethod
    def _extract_confidence_intervals(
        data_dict: dict[str, object],
        estimate: CriterionEstimate,
    ) -> None:
        """Extract confidence intervals from mean section if available."""
        mean_section = data_dict.get("mean")
        if not isinstance(mean_section, dict):
            return

        mean_dict = cast("dict[str, object]", mean_section)
        mean_ci = mean_dict.get("confidence_interval")
        if not isinstance(mean_ci, dict):
            return

        mean_ci_dict = cast("dict[str, object]", mean_ci)

        lower = mean_ci_dict.get("lower_bound")
        if isinstance(lower, (int, float)):
            estimate["mean_ci_lower"] = float(lower)

        upper = mean_ci_dict.get("upper_bound")
        if isinstance(upper, (int, float)):
            estimate["mean_ci_upper"] = float(upper)

    def extract_criterion_results(self) -> dict[str, CriterionEstimate]:
        """Extract benchmark results from criterion output directory."""
        results: dict[str, CriterionEstimate] = {}

        if not self.results_dir.exists():
            print(f"Warning: Criterion results directory not found: {self.results_dir}")
            return results

        # Recursively find all estimates.json files (Criterion writes current runs to "new")
        for estimates_file in self.results_dir.rglob("estimates.json"):
            if estimates_file.parent.name not in {"new", "base"}:
                continue
            try:
                with open(estimates_file, encoding="utf-8") as f:
                    data = json.load(f)

                if not isinstance(data, dict):
                    continue

                data_dict = cast("dict[str, object]", data)

                # Build benchmark name from path structure
                # e.g., action_calculations/calculate_action/50/base/estimates.json
                # becomes "action_calculations/calculate_action/50"
                path_parts = estimates_file.relative_to(self.results_dir).parts[:-2]  # Remove '<run_type>/estimates.json'
                benchmark_name: str = "/".join(path_parts)

                mean_ns = self._point_estimate(data_dict, "mean", estimates_file, benchmark_name)
                std_dev_ns = self._point_estimate(data_dict, "std_dev", estimates_file, benchmark_name)
                median_ns = self._point_estimate(data_dict, "median", estimates_file, benchmark_name)
                mad_ns = self._point_estimate(data_dict, "median_abs_dev", estimates_file, benchmark_name)
                if mean_ns is None or std_dev_ns is None or median_ns is None or mad_ns is None:
                    continue

                estimate: CriterionEstimate = {
                    "mean_ns": mean_ns,
                    "std_dev_ns": std_dev_ns,
                    "median_ns": median_ns,
                    "mad_ns": mad_ns,
                    "timestamp": datetime.now(UTC).isoformat(timespec="seconds"),
                }

                # Add confidence intervals if available
                self._extract_confidence_intervals(data_dict, estimate)

                results[benchmark_name] = estimate

            except (json.JSONDecodeError, KeyError) as e:
                print(f"Warning: Could not parse {estimates_file}: {e}")

        return results

    @staticmethod
    def _point_estimate(data: dict[str, object], section_name: str, estimates_file: Path, benchmark_name: str) -> float | None:
        """Extract a required Criterion point_estimate, warning on malformed input."""
        section = data.get(section_name)
        if not isinstance(section, dict):
            print(f"Warning: Missing '{section_name}' section for '{benchmark_name}' in {estimates_file}")
            return None

        section_dict = cast("dict[str, object]", section)
        point_estimate = section_dict.get("point_estimate")
        if not isinstance(point_estimate, (int, float)):
            print(f"Warning: Missing numeric '{section_name}.point_estimate' for '{benchmark_name}' in {estimates_file}")
            return None

        return float(point_estimate)

    def save_baseline(self, results: dict[str, CriterionEstimate], tag: str | None = None) -> Path:
        """Save current results as a baseline."""
        timestamp = datetime.now(UTC).strftime("%Y%m%d_%H%M%S")
        filename = f"baseline_{tag}_{timestamp}.json" if tag else f"baseline_{timestamp}.json"

        baseline_file = self.baseline_dir / filename
        with open(baseline_file, "w", encoding="utf-8") as f:
            json.dump(results, f, indent=2)

        # Update latest symlink (with Windows fallback)
        # On Windows, symlinks require elevated privileges, so we fall back to file copying
        latest_link = self.baseline_dir / "latest.json"
        try:
            latest_link.unlink(missing_ok=True)
            latest_link.symlink_to(filename)
        except (OSError, NotImplementedError):
            # Fallback for Windows or systems without symlink support
            # Use file copy instead of symlink to ensure cross-platform compatibility
            if latest_link.exists():
                latest_link.unlink()
            shutil.copy2(baseline_file, latest_link)

        print(f"✅ Saved baseline: {baseline_file}")
        return baseline_file

    def load_baseline(self, baseline_path: Path | None = None) -> dict[str, CriterionEstimate]:
        """Load baseline results."""
        if baseline_path is None:
            baseline_path = self.baseline_dir / "latest.json"

        if not baseline_path.exists():
            return {}

        try:
            with open(baseline_path, encoding="utf-8") as f:
                data: object = json.load(f)
        except (json.JSONDecodeError, FileNotFoundError) as e:
            print(f"Warning: Could not load baseline {baseline_path}: {e}")
            return {}

        validated = _validated_baseline_data(data, baseline_path=baseline_path)
        if validated is None:
            return {}

        return validated

    def compare_results(
        self,
        current: dict[str, CriterionEstimate],
        baseline: dict[str, CriterionEstimate],
        threshold: float = 10.0,
    ) -> ComparisonResult:
        """Compare current results with baseline and categorize changes."""
        comparison: ComparisonResult = {
            "regressions": [],
            "improvements": [],
            "new_benchmarks": [],
            "stable": [],
            "summary": None,
        }

        for benchmark, current_data in current.items():
            if benchmark not in baseline:
                comparison["new_benchmarks"].append({"benchmark": benchmark, "mean_ns": current_data["mean_ns"]})
                continue

            current_mean = current_data["mean_ns"]
            baseline_mean = baseline[benchmark]["mean_ns"]

            if baseline_mean == 0:
                continue

            change_percent = ((current_mean - baseline_mean) / baseline_mean) * 100

            change_data: BenchmarkChange = {
                "benchmark": benchmark,
                "change_percent": change_percent,
                "current_ns": current_mean,
                "baseline_ns": baseline_mean,
                "current_std": current_data["std_dev_ns"],
                "baseline_std": baseline[benchmark]["std_dev_ns"],
            }

            if change_percent > threshold:
                comparison["regressions"].append(change_data)
            elif change_percent < -threshold:
                comparison["improvements"].append(change_data)
            else:
                comparison["stable"].append(change_data)

        # Calculate summary statistics
        all_changes = [item["change_percent"] for item in (comparison["regressions"] + comparison["improvements"] + comparison["stable"])]

        if all_changes:
            comparison["summary"] = {
                "total_benchmarks": len(current),
                "regressions": len(comparison["regressions"]),
                "improvements": len(comparison["improvements"]),
                "stable": len(comparison["stable"]),
                "new": len(comparison["new_benchmarks"]),
                "avg_change": statistics.mean(all_changes),
                "median_change": statistics.median(all_changes),
                "max_regression": max([r["change_percent"] for r in comparison["regressions"]], default=0),
                "max_improvement": max(
                    (abs(i["change_percent"]) for i in comparison["improvements"]),
                    default=0,
                ),
            }

        return comparison

    def format_time_ns(self, nanoseconds: float) -> str:
        """Format nanoseconds into human-readable time units."""
        if nanoseconds < 1000:
            return f"{nanoseconds:.1f}ns"
        if nanoseconds < 1_000_000:
            return f"{nanoseconds / 1000:.1f}µs"
        if nanoseconds < 1_000_000_000:
            return f"{nanoseconds / 1_000_000:.1f}ms"
        return f"{nanoseconds / 1_000_000_000:.2f}s"

    def print_comparison_results(self, comparison: ComparisonResult) -> None:
        """Print comparison results to console with colors."""
        summary = comparison["summary"]

        if comparison["regressions"]:
            print("🔴 PERFORMANCE REGRESSIONS DETECTED:")
            for reg in sorted(
                comparison["regressions"],
                key=lambda x: x["change_percent"],
                reverse=True,
            ):
                current_time = self.format_time_ns(reg["current_ns"])
                baseline_time = self.format_time_ns(reg["baseline_ns"])
                print(f"  {reg['benchmark']}: +{reg['change_percent']:.1f}% slower")
                print(f"    Current: {current_time}, Baseline: {baseline_time}")
            print()

        if comparison["improvements"]:
            print("🟢 PERFORMANCE IMPROVEMENTS:")
            for imp in sorted(
                comparison["improvements"],
                key=lambda x: abs(x["change_percent"]),
                reverse=True,
            ):
                current_time = self.format_time_ns(imp["current_ns"])
                baseline_time = self.format_time_ns(imp["baseline_ns"])
                improvement_pct = abs(imp["change_percent"])
                print(f"  {imp['benchmark']}: +{improvement_pct:.1f}% faster")
                print(f"    Current: {current_time}, Baseline: {baseline_time}")
            print()

        if comparison["new_benchmarks"]:
            print("🆕 NEW BENCHMARKS:")
            for bench in comparison["new_benchmarks"]:
                time_str = self.format_time_ns(bench["mean_ns"])
                print(f"  {bench['benchmark']}: {time_str}")
            print()

        if summary:
            print("📈 SUMMARY:")
            print(f"  Total benchmarks: {summary.get('total_benchmarks', 0)}")
            print(f"  Regressions: {summary.get('regressions', 0)}")
            print(f"  Improvements: {summary.get('improvements', 0)}")
            print(f"  Stable: {summary.get('stable', 0)}")
            print(f"  New: {summary.get('new', 0)}")
            avg_change = summary.get("avg_change")
            median_change = summary.get("median_change")
            if avg_change is not None and median_change is not None:
                print(f"  Average change: {avg_change:.1f}%")
                print(f"  Median change: {median_change:.1f}%")
            if summary.get("max_regression"):
                print(f"  Max regression: +{summary['max_regression']:.1f}%")
            if summary.get("max_improvement"):
                print(f"  Max improvement: +{summary['max_improvement']:.1f}%")
            print()

        if not comparison["regressions"] and not comparison["improvements"] and not comparison["new_benchmarks"]:
            print("✅ No significant performance changes detected")

    def generate_report(self, comparison: ComparisonResult, output_file: Path | None = None) -> str:
        """Generate a detailed performance report."""
        generated_at = datetime.now(UTC).isoformat(timespec="seconds")
        lines: list[str] = [
            "# CDT Performance Analysis Report",
            f"Generated: {generated_at}",
            "",
        ]

        summary = comparison["summary"]
        if summary:
            lines.extend(
                [
                    "## Summary",
                    f"- Total benchmarks: {summary['total_benchmarks']}",
                    f"- Regressions: {summary['regressions']}",
                    f"- Improvements: {summary['improvements']}",
                    f"- Stable: {summary['stable']}",
                    f"- New benchmarks: {summary['new']}",
                    f"- Average change: {summary['avg_change']:.1f}%",
                    f"- Median change: {summary['median_change']:.1f}%",
                    "",
                ]
            )

        if comparison["regressions"]:
            lines.extend(
                [
                    "## 🔴 Performance Regressions",
                    "| Benchmark | Change | Current | Baseline | Ratio |",
                    "|-----------|--------|---------|----------|-------|",
                ]
            )

            for reg in sorted(
                comparison["regressions"],
                key=lambda x: x["change_percent"],
                reverse=True,
            ):
                current_time = self.format_time_ns(reg["current_ns"])
                baseline_time = self.format_time_ns(reg["baseline_ns"])
                ratio = reg["current_ns"] / reg["baseline_ns"] if reg["baseline_ns"] != 0 else float("inf")
                ratio_display = "∞" if math.isinf(ratio) else f"{ratio:.2f}"

                lines.append(f"| {reg['benchmark']} | +{reg['change_percent']:.1f}% | {current_time} | {baseline_time} | {ratio_display}x |")
            lines.append("")

        if comparison["improvements"]:
            lines.extend(
                [
                    "## 🟢 Performance Improvements",
                    "| Benchmark | Change | Current | Baseline | Ratio |",
                    "|-----------|--------|---------|----------|-------|",
                ]
            )

            for imp in sorted(
                comparison["improvements"],
                key=lambda x: abs(x["change_percent"]),
                reverse=True,
            ):
                current_time = self.format_time_ns(imp["current_ns"])
                baseline_time = self.format_time_ns(imp["baseline_ns"])
                ratio = imp["baseline_ns"] / imp["current_ns"] if imp["current_ns"] != 0 else float("inf")
                ratio_display = "∞" if math.isinf(ratio) else f"{ratio:.2f}"
                improvement_pct = abs(imp["change_percent"])

                lines.append(f"| {imp['benchmark']} | -{improvement_pct:.1f}% | {current_time} | {baseline_time} | {ratio_display}x |")
            lines.append("")

        if comparison["new_benchmarks"]:
            lines.append("## 🆕 New Benchmarks")
            for bench in comparison["new_benchmarks"]:
                time_str = self.format_time_ns(bench["mean_ns"])
                lines.append(f"- {bench['benchmark']}: {time_str}")
            lines.append("")

        if comparison["stable"]:
            lines.extend(
                [
                    "## ✅ Stable Benchmarks",
                    f"No significant changes detected in {len(comparison['stable'])} benchmarks.",
                    "",
                ]
            )

        report_content = "\n".join(lines)

        if output_file:
            with open(output_file, "w", encoding="utf-8") as f:
                f.write(report_content)
            print(f"📄 Report saved to: {output_file}")

        return report_content

    @staticmethod
    def _baseline_timestamp_from_filename(
        baseline_file: Path,
        timestamp_pattern: re.Pattern[str],
    ) -> datetime | None:
        match = timestamp_pattern.search(baseline_file.stem)
        if not match:
            return None

        timestamp_str = match.group(0)
        try:
            # Baseline filenames do not encode a timezone; treat them as UTC.
            return datetime.strptime(f"{timestamp_str}+0000", "%Y%m%d_%H%M%S%z")
        except ValueError:
            return None

    @staticmethod
    def _load_baseline_for_trend(baseline_file: Path) -> dict[str, CriterionEstimate] | None:
        try:
            with baseline_file.open(encoding="utf-8") as f:
                data: object = json.load(f)
        except json.JSONDecodeError:
            return None

        return _validated_baseline_data(data, baseline_path=baseline_file)

    def _load_trend_baselines(self, cutoff_date: datetime) -> list[LoadedBaseline]:
        baselines: list[LoadedBaseline] = []
        timestamp_pattern = re.compile(r"\d{8}_\d{6}$")

        for baseline_file in self.baseline_dir.glob("baseline_*.json"):
            timestamp = self._baseline_timestamp_from_filename(baseline_file, timestamp_pattern)
            if timestamp is None or timestamp < cutoff_date:
                continue

            data = self._load_baseline_for_trend(baseline_file)
            if data is None:
                continue

            baselines.append(
                {
                    "timestamp": timestamp,
                    "file": baseline_file,
                    "data": data,
                }
            )

        baselines.sort(key=lambda baseline: baseline["timestamp"])
        return baselines

    @staticmethod
    def _compute_trend_info(values: list[float]) -> TrendInfo:
        n = len(values)

        # Calculate trend (simple linear regression slope over observation index)
        sum_x = sum(range(n))
        sum_y = sum(values)
        sum_xy = sum(i * v for i, v in enumerate(values))
        sum_xx = sum(i * i for i in range(n))

        denominator = n * sum_xx - sum_x * sum_x
        # All data points at same position - treat as stable
        slope = 0 if denominator == 0 else (n * sum_xy - sum_x * sum_y) / denominator

        # Use small epsilon for floating point comparisons
        epsilon = 1e-9

        direction: TrendDirection
        if slope < -epsilon:
            direction = "improving"
        elif slope > epsilon:
            direction = "degrading"
        else:
            direction = "stable"

        return {
            "slope": slope,
            "trend": direction,
            "data_points": n,
            "first_value": values[0],
            "last_value": values[-1],
            "change_percent": (((values[-1] - values[0]) / values[0]) * 100 if abs(values[0]) > epsilon else 0.0),
        }

    def analyze_trends(self, days: int = 30) -> TrendAnalysisResult:
        """Analyze performance trends over the specified number of days."""
        cutoff_date = datetime.now(UTC) - timedelta(days=days)
        baselines = self._load_trend_baselines(cutoff_date)

        if len(baselines) < 2:
            error_result: TrendAnalysisError = {
                "error": "Not enough historical data for trend analysis",
            }
            return error_result

        trends: dict[str, TrendInfo] = {}
        benchmark_names: set[str] = set()
        for baseline in baselines:
            benchmark_names.update(baseline["data"].keys())

        for benchmark in benchmark_names:
            values: list[float] = []
            for baseline in baselines:
                estimate = baseline["data"].get(benchmark)
                if estimate is not None:
                    values.append(float(estimate["mean_ns"]))

            if len(values) < 2:
                continue

            trends[benchmark] = self._compute_trend_info(values)

        success_result: TrendAnalysisSuccess = {
            "period_days": days,
            "baselines_analyzed": len(baselines),
            "trends": trends,
        }
        return success_result


def _build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="CDT Performance Analysis Tool",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Run benchmarks and compare with baseline (equivalent to check_performance.sh)
  ./performance_analysis.py

  # Save current results as baseline
  ./performance_analysis.py --save-baseline

  # Save baseline with tag
  ./performance_analysis.py --save-baseline --tag "v1.0.0"

  # Compare with custom threshold
  ./performance_analysis.py --threshold 5.0

  # Generate detailed report
  ./performance_analysis.py --report performance_report.md

  # Analyze trends over last 7 days
  ./performance_analysis.py --trends 7
        """,
    )
    parser.add_argument("--save-baseline", action="store_true", help="Save current results as baseline")
    parser.add_argument("--tag", help="Tag for saved baseline")
    parser.add_argument(
        "--threshold",
        type=float,
        default=10.0,
        help="Regression threshold percentage (default: 10.0)",
    )
    parser.add_argument("--compare", help="Compare with specific baseline file")
    parser.add_argument("--project-root", type=Path, help="Path to the project root directory")
    parser.add_argument("--report", help="Generate detailed report to specified file")
    parser.add_argument("--trends", type=int, metavar="DAYS", help="Analyze trends over N days")
    parser.add_argument(
        "--no-run",
        action="store_true",
        help="Skip running benchmarks, use existing results",
    )
    parser.add_argument("--verbose", action="store_true", help="Verbose output")
    return parser


def _find_project_root(provided: Path | None) -> Path:
    if provided is not None:
        provided_root = provided.resolve()
        if not ((provided_root / "Cargo.toml").exists() or (provided_root / ".git").exists()):
            msg = "Provided project root does not contain Cargo.toml or .git"
            raise ValueError(msg)
        return provided_root

    current = Path(__file__).resolve().parent
    while current != current.parent:
        if (current / "Cargo.toml").exists() or (current / ".git").exists():
            return current
        current = current.parent

    msg = "Could not detect project root (no Cargo.toml or .git found)"
    raise ValueError(msg)


def _print_performance_summary(comparison: ComparisonResult) -> None:
    summary = comparison["summary"]
    if not summary:
        return

    print("\n📈 Performance Summary:")
    print(f"   Total benchmarks: {summary['total_benchmarks']}")
    print(f"   Regressions: {summary['regressions']}")
    print(f"   Improvements: {summary['improvements']}")
    print(f"   Stable: {summary['stable']}")
    print(f"   New: {summary['new']}")

    if summary["regressions"] > 0:
        print(f"   Max regression: +{summary['max_regression']:.1f}%")

    if summary["improvements"] > 0:
        print(f"   Max improvement: +{summary['max_improvement']:.1f}%")


def _handle_trends(analyzer: PerformanceAnalyzer, days: int) -> int:
    print(f"📊 Analyzing performance trends over {days} days...")
    trends = analyzer.analyze_trends(days)

    error = trends.get("error")
    if isinstance(error, str):
        print(f"❌ {error}")
        return 1

    success = cast("TrendAnalysisSuccess", trends)

    print(f"Analyzed {success['baselines_analyzed']} baselines over {success['period_days']} days")

    degrading = [name for name, trend in success["trends"].items() if trend["trend"] == "degrading"]
    improving = [name for name, trend in success["trends"].items() if trend["trend"] == "improving"]

    if degrading:
        print(f"\n🔴 Degrading trends ({len(degrading)} benchmarks):")
        for bench in degrading:
            change = success["trends"][bench]["change_percent"]
            print(f"  {bench}: {change:+.1f}% over period")

    if improving:
        print(f"\n🟢 Improving trends ({len(improving)} benchmarks):")
        for bench in improving:
            change = success["trends"][bench]["change_percent"]
            print(f"  {bench}: {change:+.1f}% over period")

    return 0


def _collect_current_results(analyzer: PerformanceAnalyzer, no_run: bool) -> dict[str, CriterionEstimate]:
    if not no_run and not analyzer.run_benchmarks():
        return {}

    print("🔍 Extracting benchmark results...")
    current_results = analyzer.extract_criterion_results()
    if not current_results:
        print("❌ No benchmark results found. Run 'cargo bench' first or remove --no-run flag.")
        return {}

    print(f"Found {len(current_results)} benchmark results")
    return current_results


def main(argv: list[str] | None = None) -> int:
    args = _build_arg_parser().parse_args(argv)

    try:
        project_root = _find_project_root(args.project_root)
    except ValueError as exc:
        print(f"❌ {exc}")
        print("   Please run this script from within the project or specify --project-root")
        return 1

    analyzer = PerformanceAnalyzer(project_root)

    if args.trends is not None:
        return _handle_trends(analyzer, args.trends)

    current_results = _collect_current_results(analyzer, args.no_run)
    if not current_results:
        return 1

    if args.save_baseline:
        analyzer.save_baseline(current_results, args.tag)
        return 0

    baseline_file = Path(args.compare) if args.compare else None
    baseline = analyzer.load_baseline(baseline_file)

    if not baseline:
        print("⚠️  No baseline found for comparison.")
        print("   Run with --save-baseline to create an initial baseline.")
        return 0

    print("📊 Comparing with baseline...")
    comparison = analyzer.compare_results(current_results, baseline, args.threshold)
    analyzer.print_comparison_results(comparison)

    if args.report:
        analyzer.generate_report(comparison, Path(args.report))

    _print_performance_summary(comparison)
    return 1 if comparison["regressions"] else 0


if __name__ == "__main__":
    sys.exit(main())
