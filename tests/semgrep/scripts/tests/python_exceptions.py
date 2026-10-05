import subprocess
import unittest.mock
from typing import TYPE_CHECKING
from unittest import mock
from unittest.mock import MagicMock, Mock

from research_repo_tools.process import run_safe_command

if TYPE_CHECKING:
    from pathlib import Path


def catches_broad_exception() -> None:
    try:
        pass
    # ruleid: causal-triangulations.python.no-broad-exception
    except Exception:
        pass


def catches_broad_exception_with_alias() -> None:
    try:
        pass
    # ruleid: causal-triangulations.python.no-broad-exception
    except Exception as exc:
        print(exc)


def catches_specific_exception() -> None:
    try:
        pass
    # ok: causal-triangulations.python.no-broad-exception
    except OSError:
        pass


def implicit_path_read_text_encoding(path: Path) -> None:
    # ruleid: causal-triangulations.python.explicit-path-text-encoding-in-tests
    path.read_text()


def implicit_path_write_text_encoding(path: Path) -> None:
    # ruleid: causal-triangulations.python.explicit-path-text-encoding-in-tests
    path.write_text("Time: [1.0, 1.0, 1.0] µs\n")


def explicit_path_text_encoding(path: Path) -> None:
    # ok: causal-triangulations.python.explicit-path-text-encoding-in-tests
    path.read_text(encoding="utf-8")
    # ok: causal-triangulations.python.explicit-path-text-encoding-in-tests
    path.write_text("Time: [1.0, 1.0, 1.0] µs\n", encoding="utf-8")
    # ok: causal-triangulations.python.explicit-path-text-encoding-in-tests
    path.read_text(encoding='utf-8')  # fmt: skip
    # ok: causal-triangulations.python.explicit-path-text-encoding-in-tests
    path.write_text("Time: [1.0, 1.0, 1.0] µs\n", encoding='utf-8')  # fmt: skip


def adhoc_mock_stdout() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    result = Mock()
    result.stdout = "ok"


def adhoc_mock_returncode() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    result = MagicMock()
    result.returncode = 0


def adhoc_mock_stdout_constructor() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    Mock(stdout="ok")


def adhoc_unittest_mock_returncode_constructor() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    unittest.mock.Mock(returncode=0)


def adhoc_mock_magic_stdout_constructor() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    mock.MagicMock(stdout="ok")


def typed_completed_process() -> subprocess.CompletedProcess[str]:
    # ok: causal-triangulations.python.no-adhoc-completedprocess-mock
    return subprocess.CompletedProcess(args=[], returncode=0, stdout="ok", stderr="")


def direct_subprocess_run() -> None:
    # ruleid: causal-triangulations.python.no-direct-subprocess-run-outside-wrapper
    subprocess.run(["git", "status"], check=True)


def shared_subprocess_run() -> None:
    # ok: causal-triangulations.python.no-direct-subprocess-run-outside-wrapper
    run_safe_command("git", ["--no-pager", "status"])
