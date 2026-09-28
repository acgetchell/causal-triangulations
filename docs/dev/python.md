# Python Development Guidelines

Guidance for all repository-owned Python, including scripts, tests, and negative Semgrep fixtures.

The Rust library is the primary product. Generic maintenance and performance evidence come from the pinned `research-repo-tools` package.
CDT's Python files are consumer integration tests and deliberate static-analysis fixtures; keep both typed and predictable.

---

## Validation

Run Python validators through the repository toolchain:

```bash
just python-check
just python-typecheck
just test-python
```

Shared file discovery covers every tracked and nonignored `.py` and `.pyi` file. Ruff check, Ruff format, and `ty check --error all` receive explicit paths
with force-exclusion disabled. Both ordinary code and negative fixtures run the full configured Ruff policy; no recipe narrows it with `--select`.
`python-fixtures-check` is an explicit CI prerequisite, with exact fixture/rule exceptions in `pyproject.toml` for deliberate violations.

`just check` also runs Python formatting checks, Ruff, `ty`, and repository-owned Semgrep rules as part of the normal validation bundle.

---

## Typing

- Annotate arguments and returns, including test fixtures, methods, and variadic parameters. Ruff ANN rules enforce this inventory.
- Use Python 3.14's native deferred annotations; do not add `from __future__ import annotations`.
- Move imports used only by annotations into `if TYPE_CHECKING:` blocks. Strict Ruff TC rules enforce this separation.
- Prefer concrete standard-library types over `Any`, `dict`, or bare `Mock` when the shape is known.
- Keep helper signatures precise enough that `ty` can validate call sites.
- Avoid growing type-checker configuration unless a demonstrated false positive cannot be solved cleanly in code.

---

## Subprocess Mocks

When mocking command wrappers such as `run_git_command()`, `run_cargo_command()`, or `run_safe_command()`, prefer real typed subprocess results:

```python
import subprocess


def completed_process(stdout: str = "", *, returncode: int = 0) -> subprocess.CompletedProcess[str]:
    """Return a typed subprocess result for command-wrapper mocks."""
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout=stdout, stderr="")
```

Use that helper instead of ad-hoc mocks such as:

```python
mock_result = Mock()
mock_result.stdout = "..."
mock_result.returncode = 0
```

Structured results keep tests close to production behavior and give `ty` real attributes to check.

---

## Exceptions

- Catch specific recoverable error families in production code. Avoid `except Exception`.
- In tests, raise concrete exceptions that match the production recovery path (`OSError`, `RuntimeError`, `subprocess.CalledProcessError`,
  `subprocess.TimeoutExpired`, etc.).
- Do not use raw `Exception` in mocks just to force a fallback branch; doing so weakens the contract that the production code is meant to enforce.

---

## Test Helpers

Put reusable typed test helpers near the top of the test module or in `scripts/tests/conftest.py` when they are shared. Prefer one helper that returns the real
structured type over repeating partially configured mocks throughout a file.

## Parser and File-Format Contracts

When a script both writes and parses a text format, add a focused round-trip test that writes representative records and parses them back. The test should cover
stable identifiers, optional sections, units, and numeric forms such as scientific notation when those values can be emitted by production code.

For parser refactors, keep malformed-input regression tests for behavior that callers depend on, such as skipping incomplete sections or failing loudly on
invalid numerical data.
