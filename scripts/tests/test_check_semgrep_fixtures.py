"""Tests for Semgrep fixture-annotation validation."""

import collections
import json
from typing import TYPE_CHECKING

import pytest

import check_semgrep_fixtures

if TYPE_CHECKING:
    from pathlib import Path


def _result(check_id: str, line: int, end_line: int | None = None) -> dict[str, object]:
    return {"check_id": check_id, "start": {"line": line}, "end": {"line": line if end_line is None else end_line}}


def _run_main(monkeypatch: pytest.MonkeyPatch, fixture: Path, payload: object) -> int:
    monkeypatch.setenv("SEMGREP_JSON", json.dumps(payload))
    monkeypatch.setattr(check_semgrep_fixtures.sys, "argv", ["check_semgrep_fixtures.py", str(fixture)])
    return check_semgrep_fixtures.main()


def test_semgrep_results_parses_valid_result_objects(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "SEMGREP_JSON",
        json.dumps({"results": [_result("rust.foo", 2), _result("rust.bar", 4)]}),
    )

    results = check_semgrep_fixtures._semgrep_results()

    assert results is not None
    assert len(results.results) == 2


@pytest.mark.parametrize(
    ("payload", "diagnostic"),
    [
        ([], "expected a JSON object"),
        ({}, "expected 'results' to be a list"),
        ({"results": {}}, "expected 'results' to be a list"),
        ({"results": [_result("rust.foo", 2), "bad"]}, "result 1 is not an object"),
    ],
)
def test_semgrep_results_rejects_malformed_container_shapes(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    payload: object,
    diagnostic: str,
) -> None:
    monkeypatch.setenv("SEMGREP_JSON", json.dumps(payload))

    assert check_semgrep_fixtures._semgrep_results() is None
    assert diagnostic in capsys.readouterr().err


def test_main_accepts_matching_rule_and_line_annotations(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    fixture = tmp_path / "fixture.rs"
    fixture.write_text(
        "// ruleid: rust.foo, rust.bar\nbad_one();\n// todoruleid: rust.foo\nbad_two();\n",
        encoding="utf-8",
    )
    payload = {"results": [_result("rust.foo", 2), _result("rust.bar", 2), _result("rust.foo", 4)]}

    assert _run_main(monkeypatch, fixture, payload) == 0


@pytest.mark.parametrize(
    ("result", "diagnostic"),
    [
        ({"start": {"line": 2}, "end": {"line": 2}}, "missing non-empty string field 'check_id'"),
        (_result("", 2), "missing non-empty string field 'check_id'"),
        ({"check_id": "rust.foo", "start": {}, "end": {"line": 2}}, "missing positive integer field 'start.line'"),
        ({"check_id": "rust.foo", "start": {"line": True}, "end": {"line": 2}}, "missing positive integer field 'start.line'"),
        ({"check_id": "rust.foo", "start": {"line": 2}, "end": {}}, "missing positive integer field 'end.line'"),
        ({"check_id": "rust.foo", "start": {"line": 3}, "end": {"line": 2}}, "end.line 2 before start.line 3"),
    ],
)
def test_main_rejects_malformed_finding_fields(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    result: dict[str, object],
    diagnostic: str,
) -> None:
    fixture = tmp_path / "fixture.rs"
    fixture.write_text("// ruleid: rust.foo\nbad();\n", encoding="utf-8")

    assert _run_main(monkeypatch, fixture, {"results": [result]}) == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert diagnostic in captured.err


def test_main_rejects_findings_at_wrong_lines_even_when_rule_counts_match(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    fixture = tmp_path / "fixture.rs"
    fixture.write_text("// ruleid: rust.foo\nbad_one();\n// ruleid: rust.foo\nbad_two();\n", encoding="utf-8")
    payload = {"results": [_result("rust.foo", 2), _result("rust.foo", 5)]}

    assert _run_main(monkeypatch, fixture, payload) == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "rust.foo at line 4: expected finding not reported" in captured.err
    assert "rust.foo at lines 5: unexpected finding" in captured.err


def test_main_matches_overlapping_spans_by_shortest_span(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    fixture = tmp_path / "fixture.rs"
    fixture.write_text("// ruleid: rust.foo\nbad_one();\n// ruleid: rust.foo\nbad_two();\n", encoding="utf-8")
    payload = {"results": [_result("rust.foo", 2, 4), _result("rust.foo", 2)]}

    assert _run_main(monkeypatch, fixture, payload) == 0


def test_finding_mismatches_consumes_shorter_span_when_it_ends_later() -> None:
    expected = collections.Counter({("rust.foo", 4): 1})
    actual = (("rust.foo", 1, 4), ("rust.foo", 3, 5))

    assert check_semgrep_fixtures._finding_mismatches(expected, actual) == ("rust.foo at lines 1-4: unexpected finding",)


def test_main_matches_markdown_finding_after_blank_line_and_code_fence(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    fixture = tmp_path / "fixture.md"
    fixture.write_text("<!-- ruleid: docs.foo -->\n\n```bash\nbad-command\n```\n", encoding="utf-8")

    assert _run_main(monkeypatch, fixture, {"results": [_result("docs.foo", 4)]}) == 0
