"""Tests for coverage_report.py XML loading diagnostics."""

from pathlib import Path
from xml.etree import ElementTree as ET

import pytest

from coverage_report import coverage_entries, load_report


def test_load_report_parse_error_includes_xml_diagnostic(tmp_path: Path) -> None:
    report = tmp_path / "cobertura.xml"
    report.write_text("<coverage></broken>", encoding="utf-8")

    with pytest.raises(SystemExit) as exc_info:
        load_report(report)

    message = str(exc_info.value)
    assert str(report) in message
    assert "mismatched tag" in message
    assert "line 1" in message


def test_coverage_entries_reports_malformed_hit_counts() -> None:
    root = ET.Element("coverage")
    packages = ET.SubElement(root, "packages")
    package = ET.SubElement(packages, "package")
    classes = ET.SubElement(package, "classes")
    class_element = ET.SubElement(classes, "class", {"filename": "src/lib.rs"})
    lines = ET.SubElement(class_element, "lines")
    ET.SubElement(lines, "line", {"number": "7", "hits": "NaN"})

    with pytest.raises(SystemExit) as exc_info:
        list(coverage_entries(root))

    message = str(exc_info.value)
    assert "src/lib.rs:7" in message
    assert "NaN" in message
