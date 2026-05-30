"""Validate repository-owned Semgrep fixture annotations."""

from __future__ import annotations

import collections
import json
import os
import re
import sys
from pathlib import Path

RULE_ANNOTATION = re.compile(r"\b(?:ruleid|todoruleid):\s*([A-Za-z0-9_.-]+)")


def main() -> int:
    path = Path(sys.argv[1])
    expected: collections.Counter[str] = collections.Counter()
    for line in path.read_text(encoding="utf-8").splitlines():
        expected.update(match.group(1) for match in RULE_ANNOTATION.finditer(line))

    data = json.loads(os.environ["SEMGREP_JSON"])
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
