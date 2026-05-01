import subprocess
from unittest.mock import MagicMock, Mock


def catches_broad_exception() -> None:
    try:
        pass
    # ruleid: causal-triangulations.python.no-broad-exception
    except Exception:
        pass


def catches_specific_exception() -> None:
    try:
        pass
    # ok: causal-triangulations.python.no-broad-exception
    except OSError:
        pass


def raises_raw_exception() -> None:
    # ruleid: causal-triangulations.python.no-raw-exception-in-tests
    raise Exception("too broad")


def raises_specific_exception() -> None:
    # ok: causal-triangulations.python.no-raw-exception-in-tests
    raise RuntimeError("specific failure")


def adhoc_mock_stdout() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    result = Mock()
    result.stdout = "ok"


def adhoc_mock_returncode() -> None:
    # ruleid: causal-triangulations.python.no-adhoc-completedprocess-mock
    result = MagicMock()
    result.returncode = 0


def typed_completed_process() -> subprocess.CompletedProcess[str]:
    # ok: causal-triangulations.python.no-adhoc-completedprocess-mock
    return subprocess.CompletedProcess(args=[], returncode=0, stdout="ok", stderr="")


# ruleid: causal-triangulations.python.no-untyped-defs-in-scripts
def missing_return_annotation():
    return None


# ok: causal-triangulations.python.no-untyped-defs-in-scripts
def explicit_return_annotation() -> None:
    return None
