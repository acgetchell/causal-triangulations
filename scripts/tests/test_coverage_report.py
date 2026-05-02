"""Tests for coverage_report.py XML loading diagnostics."""

from pathlib import Path

import pytest

from coverage_report import load_report


def test_load_report_parse_error_includes_xml_diagnostic(tmp_path: Path) -> None:
    report = tmp_path / "cobertura.xml"
    report.write_text("<coverage></broken>", encoding="utf-8")

    with pytest.raises(SystemExit) as exc_info:
        load_report(report)

    message = str(exc_info.value)
    assert str(report) in message
    assert "mismatched tag" in message
    assert "line 1" in message
