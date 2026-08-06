"""Tests for notebook_check.py notebook linting diagnostics."""

import json
import subprocess
from pathlib import Path
from typing import Any

import pytest

import notebook_check
from notebook_check import (
    LintOptions,
    NotebookDocument,
    code_cell_diagnostics,
    external_tool_diagnostics,
    extract_code,
    lint,
    load_notebook,
    main,
)


def write_notebook(path: Path, cells: list[dict[str, Any]]) -> None:
    """Write a minimal nbformat v4 notebook fixture."""
    notebook = {
        "cells": cells,
        "metadata": {},
        "nbformat": 4,
        "nbformat_minor": 5,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(notebook),
        encoding="utf-8",
    )


def load_notebook_fixture(tmp_path: Path, name: str, cells: list[dict[str, Any]]) -> NotebookDocument:
    """Write and load a minimal notebook through the real parser."""
    path = tmp_path / name
    write_notebook(path, cells)
    return load_notebook(path)


def code_cell(source: Any, *, outputs: Any = None, execution_count: Any = None) -> dict[str, Any]:
    """Return a minimal code cell fixture."""
    return {
        "cell_type": "code",
        "execution_count": execution_count,
        "metadata": {},
        "outputs": [] if outputs is None else outputs,
        "source": source,
    }


def completed_process(stdout: str = "", *, returncode: int = 0) -> subprocess.CompletedProcess[str]:
    """Return a typed subprocess result for command-wrapper mocks."""
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout=stdout, stderr="")


def test_load_notebook_rejects_missing_cells(tmp_path: Path) -> None:
    notebook = tmp_path / "missing-cells.ipynb"
    notebook.write_text('{"nbformat": 4, "nbformat_minor": 5, "metadata": {}}', encoding="utf-8")

    with pytest.raises(TypeError, match="expected notebook cells to be a list"):
        load_notebook(notebook)


def test_load_notebook_rejects_non_object_json(tmp_path: Path) -> None:
    notebook = tmp_path / "list.ipynb"
    notebook.write_text("[]", encoding="utf-8")

    with pytest.raises(TypeError, match="expected notebook JSON to be an object"):
        load_notebook(notebook)


def test_load_notebook_rejects_wrong_nbformat(tmp_path: Path) -> None:
    notebook = tmp_path / "old-format.ipynb"
    notebook.write_text('{"cells": [], "nbformat": 3, "nbformat_minor": 0, "metadata": {}}', encoding="utf-8")

    with pytest.raises(ValueError, match="expected nbformat 4"):
        load_notebook(notebook)


@pytest.mark.parametrize("invalid_nbformat", [4.0, True])
def test_load_notebook_rejects_non_integer_nbformat(tmp_path: Path, invalid_nbformat: object) -> None:
    notebook = tmp_path / "invalid-nbformat-type.ipynb"
    notebook.write_text(
        json.dumps({"cells": [], "nbformat": invalid_nbformat, "nbformat_minor": 5, "metadata": {}}),
        encoding="utf-8",
    )

    with pytest.raises(TypeError, match="expected nbformat to be the integer 4"):
        load_notebook(notebook)


def test_load_notebook_rejects_non_object_cell_metadata(tmp_path: Path) -> None:
    notebook = tmp_path / "bad-cell-metadata.ipynb"
    write_notebook(notebook, [{**code_cell("x = 1"), "metadata": []}])

    with pytest.raises(TypeError, match="cell 1: metadata must be an object"):
        load_notebook(notebook)


def test_load_notebook_rejects_bad_cell_source(tmp_path: Path) -> None:
    notebook = tmp_path / "bad-source.ipynb"
    write_notebook(notebook, [code_cell(["valid", 5])])

    with pytest.raises(TypeError, match="source list must contain only strings"):
        load_notebook(notebook)


def test_load_notebook_rejects_unknown_cell_type(tmp_path: Path) -> None:
    notebook = tmp_path / "bad-cell-type.ipynb"
    write_notebook(notebook, [{**code_cell("x = 1"), "cell_type": "python"}])

    with pytest.raises(ValueError, match="expected cell_type to be one of"):
        load_notebook(notebook)


@pytest.mark.parametrize("execution_count", ["1", True, -1])
def test_load_notebook_rejects_bad_execution_count(tmp_path: Path, execution_count: object) -> None:
    notebook = tmp_path / "bad-execution-count.ipynb"
    write_notebook(notebook, [code_cell("x = 1", execution_count=execution_count)])

    with pytest.raises(TypeError, match="execution_count must be a nonnegative integer or null"):
        load_notebook(notebook)


def test_load_notebook_rejects_bad_outputs_shape(tmp_path: Path) -> None:
    notebook = tmp_path / "bad-outputs.ipynb"
    write_notebook(notebook, [code_cell("x = 1", outputs={"output_type": "stream"})])

    with pytest.raises(TypeError, match="outputs must be a list"):
        load_notebook(notebook)


def test_code_cell_diagnostics_report_dirty_outputs_and_syntax(tmp_path: Path) -> None:
    notebook_path = tmp_path / "dirty.ipynb"
    write_notebook(
        notebook_path,
        [
            code_cell("x = 1", outputs=[{"output_type": "stream", "name": "stdout", "text": "1"}], execution_count=7),
            code_cell("def broken(:\n    pass"),
        ],
    )
    notebook = load_notebook(notebook_path)

    diagnostics = code_cell_diagnostics(notebook_path, notebook, LintOptions(run_ruff=False, run_format=False, run_ty=False))

    messages = [diagnostic.message for diagnostic in diagnostics]
    assert "has 1 output block(s); clear outputs before committing" in messages
    assert "execution_count=7; clear execution counts" in messages
    assert any(message.startswith("syntax error:") for message in messages)


def test_lint_allow_outputs_accepts_rendered_notebook(tmp_path: Path) -> None:
    notebook = tmp_path / "rendered.ipynb"
    write_notebook(notebook, [code_cell("x = 1", outputs=[{"output_type": "stream", "name": "stdout", "text": "1"}], execution_count=1)])

    result = lint(notebook, LintOptions(allow_outputs=True, run_ruff=False, run_format=False, run_ty=False))

    assert result == 0


def test_lint_strict_fails_on_notebook_warnings(tmp_path: Path) -> None:
    notebook = tmp_path / "warnings.ipynb"
    write_notebook(notebook, [code_cell("import pandas\n\ndef helper(value):\n    return value\n")])

    result = lint(notebook, LintOptions(strict=True, run_ruff=False, run_format=False, run_ty=False))

    assert result == 1


def test_lint_without_external_tools_passes_clean_notebook(tmp_path: Path) -> None:
    notebook = tmp_path / "clean.ipynb"
    write_notebook(notebook, [code_cell("def helper(value: int) -> int:\n    return value + 1\n")])

    result = lint(notebook, LintOptions(run_ruff=False, run_format=False, run_ty=False))

    assert result == 0


def test_external_tool_diagnostics_report_missing_tools(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("notebook_check.shutil.which", lambda _command: None)
    notebook = NotebookDocument(path=Path("missing-tools.ipynb"), nbformat=4, nbformat_minor=5, cells=())

    diagnostics = external_tool_diagnostics(Path("missing-tools.ipynb"), notebook, LintOptions())

    messages = [diagnostic.message for diagnostic in diagnostics]
    assert "ruff is required for notebook linting; run through `uv run` or install Ruff" in messages
    assert "ty is required for notebook linting; run through `uv run` or install ty" in messages


def test_ruff_diagnostics_run_from_project_root_and_map_cell(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    notebook_path = tmp_path / "notebooks" / "ruff.ipynb"
    write_notebook(notebook_path, [code_cell("import os\n")])
    notebook = load_notebook(notebook_path)
    calls: list[Path | None] = []

    def fake_run_safe_command(
        command: str,
        args: list[str],
        cwd: Path | None = None,
        **kwargs: Any,
    ) -> subprocess.CompletedProcess[str]:
        assert command == "ruff"
        assert args[0] == "check"
        assert kwargs["input"]
        calls.append(cwd)
        return completed_process(
            "F401 [*] `os` imported but unused\n  --> ruff_notebook.py:2:8\nFound 1 error.\n",
            returncode=1,
        )

    monkeypatch.setattr(notebook_check, "run_safe_command", fake_run_safe_command)

    diagnostics = notebook_check.ruff_lint_diagnostics(notebook_path, notebook, tmp_path)

    assert calls == [tmp_path]
    assert diagnostics == [notebook_check.Diagnostic("error", 1, "ruff check: F401 [*] `os` imported but unused")]


def test_main_lint_reports_errors_to_stderr(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    notebook = tmp_path / "dirty.ipynb"
    write_notebook(notebook, [code_cell("x = 1", outputs=[{"output_type": "stream", "name": "stdout", "text": "1"}], execution_count=3)])

    result = main(["--lint", "--no-ruff", "--no-format", "--no-ty", str(notebook)])

    captured = capsys.readouterr()
    assert result == 1
    assert captured.out == ""
    assert f"{notebook}: cell 1: error: has 1 output block(s); clear outputs before committing" in captured.err
    assert f"{notebook}: cell 1: error: execution_count=3; clear execution counts" in captured.err


def test_lint_notebooks_continues_after_invalid_json(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    invalid = tmp_path / "invalid.ipynb"
    invalid.write_text("not-json", encoding="utf-8")
    valid = tmp_path / "valid.ipynb"
    write_notebook(valid, [code_cell("value = 1\n")])
    options = LintOptions(run_ruff=False, run_format=False, run_ty=False, project_root=tmp_path)

    result = notebook_check.lint_notebooks([invalid, valid], options)

    captured = capsys.readouterr()
    assert result == 1
    assert f"{invalid}: notebook: error:" in captured.err
    assert f"OK linted {valid}" in captured.out


def test_main_reports_missing_notebook_without_traceback(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    missing_notebook = tmp_path / "missing.ipynb"

    result = main(["--summary", str(missing_notebook)])

    captured = capsys.readouterr()
    assert result == 1
    assert captured.out == ""
    assert "error: notebook does not exist or is not a file:" in captured.err
    assert "Traceback" not in captured.err


def test_main_rejects_non_positive_timeout(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc_info:
        main(["--execute", "--timeout", "0", "notebook.ipynb"])

    captured = capsys.readouterr()
    assert exc_info.value.code == 2
    assert "expected a positive integer" in captured.err


def test_extract_code_maps_lines_to_source_cells(tmp_path: Path) -> None:
    notebook = load_notebook_fixture(
        tmp_path,
        "line-map.ipynb",
        [
            {"cell_type": "markdown", "metadata": {}, "source": "context"},
            code_cell("first = 1\nsecond = 2\n"),
            code_cell("third = 3"),
        ],
    )

    snapshot = extract_code(notebook)

    assert "# %% notebook cell 2" in snapshot.source
    assert "# %% notebook cell 3" in snapshot.source
    assert snapshot.line_to_cell[2] == 2
    assert snapshot.line_to_cell[5] == 3


def test_discover_notebooks_sorts_sources_and_ignores_checkpoints(tmp_path: Path) -> None:
    second = tmp_path / "notebooks" / "02_second.ipynb"
    first = tmp_path / "notebooks" / "01_first.ipynb"
    checkpoint = tmp_path / "notebooks" / ".ipynb_checkpoints" / "01_first-checkpoint.ipynb"
    for path in (second, first, checkpoint):
        write_notebook(path, [code_cell("value = 1\n")])

    assert notebook_check.discover_notebooks(tmp_path) == [first, second]


def test_main_discovers_repository_notebooks(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    notebook = tmp_path / "notebooks" / "valid.ipynb"
    write_notebook(notebook, [code_cell("value = 1\n")])

    result = main(["--lint", "--repo-root", str(tmp_path), "--no-ruff", "--no-format", "--no-ty"])

    assert result == 0
    assert f"OK linted {notebook}" in capsys.readouterr().out


def test_main_reports_empty_discovery(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    result = main(["--lint", "--repo-root", str(tmp_path), "--no-ruff", "--no-format", "--no-ty"])

    assert result == 0
    assert capsys.readouterr().out == "No notebooks found.\n"


def test_selected_notebooks_resolves_relative_paths(tmp_path: Path) -> None:
    paths = notebook_check.selected_notebooks([Path("notebooks/valid.ipynb")], tmp_path)

    assert paths == [tmp_path / "notebooks" / "valid.ipynb"]
