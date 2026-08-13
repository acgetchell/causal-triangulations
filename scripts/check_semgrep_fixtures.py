"""Validate repository-owned Semgrep fixture annotations."""

import collections
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TypeGuard

RULE_ANNOTATION = re.compile(r"\b(?:ruleid|todoruleid):\s*([A-Za-z0-9_.-]+(?:\s*,\s*[A-Za-z0-9_.-]+)*)")


type ParsedObject = dict[str, object]
type ExpectedFinding = tuple[str, int]
type ActualFinding = tuple[str, int, int]


@dataclass(frozen=True, slots=True)
class SemgrepResults:
    """Validated subset of Semgrep JSON needed by fixture checks."""

    results: tuple[ParsedObject, ...]


def _is_parsed_object(value: object) -> TypeGuard[ParsedObject]:
    return isinstance(value, dict) and all(isinstance(key, str) for key in value)


def _path_argument(argv: list[str]) -> Path | None:
    if len(argv) <= 1:
        print("Missing required path argument: sys.argv[1]", file=sys.stderr)
        return None

    path = Path(argv[1])
    if not path.exists():
        print(f"path argument does not exist: {path}", file=sys.stderr)
        return None
    if not path.is_file():
        print(f"path argument is not a file: {path}", file=sys.stderr)
        return None
    if not os.access(path, os.R_OK):
        print(f"path argument is not readable: {path}", file=sys.stderr)
        return None
    return path


def _semgrep_results() -> SemgrepResults | None:
    semgrep_json = os.environ.get("SEMGREP_JSON")
    if semgrep_json is None:
        print("Missing required SEMGREP_JSON environment variable", file=sys.stderr)
        return None
    try:
        data: object = json.loads(semgrep_json)
    except json.JSONDecodeError as error:
        print(f"Invalid JSON in SEMGREP_JSON: {error}", file=sys.stderr)
        return None

    if not _is_parsed_object(data):
        print("Invalid SEMGREP_JSON shape: expected a JSON object", file=sys.stderr)
        return None
    results = data.get("results")
    if not isinstance(results, list):
        print("Invalid SEMGREP_JSON shape: expected 'results' to be a list", file=sys.stderr)
        return None

    parsed_results: list[ParsedObject] = []
    malformed_results: list[str] = []
    for index, result in enumerate(results):
        if _is_parsed_object(result):
            parsed_results.append(result)
        else:
            malformed_results.append(f"result {index} is not an object")

    if malformed_results:
        print("Invalid SEMGREP_JSON shape:", file=sys.stderr)
        for malformed in malformed_results:
            print(f"  {malformed}", file=sys.stderr)
        return None

    return SemgrepResults(results=tuple(parsed_results))


def _expected_findings(path: Path) -> collections.Counter[ExpectedFinding]:
    expected: collections.Counter[ExpectedFinding] = collections.Counter()
    lines = path.read_text(encoding="utf-8").splitlines()
    for line_number, line in enumerate(lines, start=1):
        for match in RULE_ANNOTATION.finditer(line):
            finding_line = line_number + 1
            while finding_line <= len(lines):
                candidate = lines[finding_line - 1].strip()
                if candidate and not candidate.startswith("```"):
                    break
                finding_line += 1
            expected.update((rule_id.strip(), finding_line) for rule_id in match.group(1).split(",") if rule_id.strip())
    return expected


def _actual_findings(semgrep: SemgrepResults) -> tuple[ActualFinding, ...] | None:
    actual: list[ActualFinding] = []
    malformed_results: list[str] = []
    for index, result in enumerate(semgrep.results):
        check_id = result.get("check_id")
        start = result.get("start")
        end = result.get("end")
        start_line = start.get("line") if _is_parsed_object(start) else None
        end_line = end.get("line") if _is_parsed_object(end) else None

        check_id_valid = isinstance(check_id, str) and bool(check_id)
        start_line_valid = isinstance(start_line, int) and not isinstance(start_line, bool) and start_line >= 1
        end_line_valid = isinstance(end_line, int) and not isinstance(end_line, bool) and end_line >= 1
        if not check_id_valid:
            malformed_results.append(f"result {index} is missing non-empty string field 'check_id'")
        if not start_line_valid:
            malformed_results.append(f"result {index} is missing positive integer field 'start.line'")
        if not end_line_valid:
            malformed_results.append(f"result {index} is missing positive integer field 'end.line'")
        if start_line_valid and end_line_valid and end_line < start_line:
            malformed_results.append(f"result {index} has end.line {end_line} before start.line {start_line}")

        if (
            isinstance(check_id, str)
            and check_id
            and isinstance(start_line, int)
            and not isinstance(start_line, bool)
            and start_line >= 1
            and isinstance(end_line, int)
            and not isinstance(end_line, bool)
            and end_line >= start_line
        ):
            actual.append((check_id, start_line, end_line))

    if not malformed_results:
        return tuple(actual)

    print("Invalid SEMGREP_JSON shape:", file=sys.stderr)
    for malformed in malformed_results:
        print(f"  {malformed}", file=sys.stderr)
    return None


def _finding_mismatches(
    expected: collections.Counter[ExpectedFinding],
    actual: tuple[ActualFinding, ...],
) -> tuple[str, ...]:
    unmatched_actual = list(actual)
    mismatches: list[str] = []

    for (rule_id, line), expected_count in sorted(expected.items()):
        for _ in range(expected_count):
            match_index = min(
                (
                    index
                    for index, (actual_rule_id, start_line, end_line) in enumerate(unmatched_actual)
                    if actual_rule_id == rule_id and start_line <= line <= end_line
                ),
                key=lambda index: unmatched_actual[index][2] - unmatched_actual[index][1],
                default=None,
            )
            if match_index is None:
                mismatches.append(f"{rule_id} at line {line}: expected finding not reported")
            else:
                unmatched_actual.pop(match_index)

    for rule_id, start_line, end_line in sorted(unmatched_actual):
        span = str(start_line) if start_line == end_line else f"{start_line}-{end_line}"
        mismatches.append(f"{rule_id} at lines {span}: unexpected finding")

    return tuple(mismatches)


def main() -> int:
    """Compare expected fixture annotations with the supplied Semgrep results."""
    path = _path_argument(sys.argv)
    if path is None:
        return 1

    expected = _expected_findings(path)
    semgrep = _semgrep_results()
    if semgrep is None:
        return 1
    actual = _actual_findings(semgrep)
    if actual is None:
        return 1

    mismatches = _finding_mismatches(expected, actual)
    if not mismatches:
        return 0

    print(f"Semgrep fixture mismatch in {path}", file=sys.stderr)
    for mismatch in mismatches:
        print(f"  {mismatch}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
