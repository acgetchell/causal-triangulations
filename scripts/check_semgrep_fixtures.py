"""Validate repository-owned Semgrep fixture annotations."""

from __future__ import annotations

import collections
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

RULE_ANNOTATION = re.compile(r"\b(?:ruleid|todoruleid):\s*([A-Za-z0-9_.-]+(?:\s*,\s*[A-Za-z0-9_.-]+)*)")


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


def _semgrep_results() -> dict[str, Any] | None:
    semgrep_json = os.environ.get("SEMGREP_JSON")
    if semgrep_json is None:
        print("Missing required SEMGREP_JSON environment variable", file=sys.stderr)
        return None
    try:
        return json.loads(semgrep_json)
    except json.JSONDecodeError as error:
        print(f"Invalid JSON in SEMGREP_JSON: {error}", file=sys.stderr)
        return None


def main() -> int:
    path = _path_argument(sys.argv)
    if path is None:
        return 1

    expected: collections.Counter[str] = collections.Counter()
    for line in path.read_text(encoding="utf-8").splitlines():
        for match in RULE_ANNOTATION.finditer(line):
            expected.update(rule_id.strip() for rule_id in match.group(1).split(",") if rule_id.strip())

    data = _semgrep_results()
    if data is None:
        return 1

    actual: collections.Counter[str] = collections.Counter(result["check_id"] for result in data["results"])
    if actual == expected:
        return 0

    print(f"Semgrep fixture mismatch in {path}")
    for rule in sorted(expected.keys() | actual.keys()):
        if expected[rule] != actual[rule]:
            print(f"  {rule}: expected {expected[rule]}, got {actual[rule]}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
