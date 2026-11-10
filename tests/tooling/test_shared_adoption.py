"""Published shared-tooling contracts exercised with CDT's actual declarations."""

import json
import os
import subprocess
import sys
import tomllib
from importlib.metadata import metadata, version
from pathlib import Path
from typing import TYPE_CHECKING, Any
from urllib.parse import quote

import pytest
import yaml
from packaging.requirements import Requirement
from research_repo_tools import command_timing, notebook_workflows
from research_repo_tools.cli import main
from research_repo_tools.just_inspect import dry_run
from research_repo_tools.process import run_safe_command

if TYPE_CHECKING:
    from collections.abc import Sequence

ROOT = Path(__file__).resolve().parents[2]


def test_shared_python_profile_owns_declarations_lock_and_executables() -> None:
    """The supplier's installed metadata is the one version authority."""
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    assert project["tool"]["research-repo-tools"]["toolchain"]["inherit-python-tools"] is True
    requirements = [Requirement(raw) for raw in metadata("research-repo-tools").get_all("Requires-Dist", [])]
    profile = {item.name: item for item in requirements if item.marker is not None and item.marker.evaluate({"extra": "python-tools"})}
    for name in ("pytest", "ruff", "ty"):
        assert version(name) in profile[name].specifier
        assert not any(isinstance(raw, str) and Requirement(raw).name == name for raw in project["dependency-groups"]["dev"])
    for group in ("tooling", "notebooks"):
        selected = [Requirement(raw) for raw in project["dependency-groups"][group] if isinstance(raw, str)]
        assert "python-tools" in next(item.extras for item in selected if item.name == "research-repo-tools")
    assert main(["--root", str(ROOT), "toolchain", "python-tools-check"]) == 0


@pytest.mark.parametrize("approved", [False, True])
def test_composite_allowlist_checks_the_real_setup_action(approved: bool, tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    """Composite coverage uses the installed parser and the exact consumer policy."""
    policy = tmp_path / "selected-actions.json"
    policy.write_bytes((ROOT / "tooling/selected-actions.json").read_bytes())
    composite = yaml.safe_load((ROOT / ".github/actions/setup-toolchain/action.yml").read_bytes())
    if not approved:
        composite["runs"]["steps"][0]["uses"] = "unapproved/setup@" + "a" * 40
    action = tmp_path / "action.yml"
    action.write_text(yaml.safe_dump(composite), encoding="utf-8")
    assert main(["--root", str(tmp_path), "actions", "allowlist", "--policy", str(policy), str(action)]) == (0 if approved else 1)
    if not approved:
        assert "unapproved reference" in capsys.readouterr().err
    command = dry_run(ROOT, "actions-allowlist-check").stderr
    assert command.rstrip().endswith(".github/workflows .github/actions")


@pytest.mark.parametrize("selection", ["notebooks/00_quickstart.ipynb", "notebooks/analysis with spaces λ.ipynb"])
@pytest.mark.parametrize("status", [0, 23, 130])
def test_notebook_launch_preserves_selection_environment_and_exit_status(selection: str, status: int, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Inspect real launch arguments without starting Jupyter or synchronizing files."""
    for name in ("pyproject.toml", "uv.lock", ".python-version", "rust-toolchain.toml"):
        (tmp_path / name).write_bytes((ROOT / name).read_bytes())
    notebook = tmp_path / selection
    notebook.parent.mkdir()
    notebook.write_bytes((ROOT / "notebooks/00_quickstart.ipynb").read_bytes())
    calls: list[list[str]] = []

    def launch(executable: str, arguments: list[str], **kwargs: Any) -> subprocess.CompletedProcess[None]:
        calls.append(arguments)
        assert arguments[:3] == ["run", "--locked", "--managed-python"]
        assert "--ServerApp.open_browser=True" in arguments
        assert f"--ServerApp.default_url=/lab/tree/{quote(selection, safe='/')}" in arguments
        assert kwargs["cwd"] == tmp_path
        assert kwargs["timeout"] is None
        assert kwargs["check"] is False
        environment = kwargs["env"]
        for name in ("IPYTHONDIR", "MPLCONFIGDIR", "JUPYTER_RUNTIME_DIR"):
            assert Path(environment[name]).is_relative_to(tmp_path / "target/notebooks")
            assert Path(environment[name]).is_dir()
        assert "VIRTUAL_ENV" not in environment
        if status == 130:
            raise KeyboardInterrupt
        return subprocess.CompletedProcess([executable, *arguments], status)

    monkeypatch.setattr(notebook_workflows, "run_command_live", launch)
    assert main(["--root", str(tmp_path), "notebooks", "launch", selection]) == status
    assert len(calls) == 1
    assert list((tmp_path / "target/notebooks").iterdir()) == []
    assert 'notebooks launch "$1"' in dry_run(ROOT, "notebook", [selection]).stderr


@pytest.mark.parametrize("selection", ["notebooks/missing.ipynb", "../outside.ipynb"])
def test_notebook_selection_fails_before_launch(selection: str, monkeypatch: pytest.MonkeyPatch) -> None:
    def unexpected(*_args: object, **_kwargs: object) -> None:
        pytest.fail("invalid notebook selection launched Jupyter")

    monkeypatch.setattr(notebook_workflows, "run_command_live", unexpected)
    assert main(["--root", str(ROOT), "notebooks", "launch", selection]) == 1


@pytest.mark.parametrize("fail", [False, True])
def test_command_timing_preserves_all_four_workloads_and_complete_reports(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, fail: bool) -> None:
    """Use the shared acquisition/report contract with inert process boundaries."""
    configuration = "tooling/command-timing.toml"
    (tmp_path / "tooling").mkdir()
    (tmp_path / configuration).write_bytes((ROOT / configuration).read_bytes())
    for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
        (tmp_path / name).write_bytes((ROOT / name).read_bytes())
    (tmp_path / "src").mkdir()
    (tmp_path / "src/main.rs").write_bytes((ROOT / "src/main.rs").read_bytes())
    calls: list[tuple[str, ...]] = []
    executable = Path(sys.executable).resolve()

    def resolve(command: str, **kwargs: Any) -> Path:
        assert command in {"cargo", "rustc", "target/release/cdt"}
        assert kwargs["cwd"] == tmp_path
        return executable

    def execute(program: Path, arguments: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[None]:
        assert program == executable
        assert kwargs["env"]["RUST_LOG"] == "error"
        assert kwargs["env"]["CARGO_TARGET_DIR"] == "target"
        assert kwargs["timeout"] == 600
        calls.append(tuple(arguments))
        if fail and arguments[0] == "--vertices-per-slice":
            raise subprocess.CalledProcessError(23, [str(program), *arguments])
        return subprocess.CompletedProcess([str(program), *arguments], 0)

    def probe(program: Path, arguments: Sequence[str], **_kwargs: object) -> subprocess.CompletedProcess[bytes]:
        assert program == executable
        return subprocess.CompletedProcess([str(program), *arguments], 0, b"fixture version\n", b"")

    monkeypatch.setattr(command_timing, "resolve_executable", resolve)
    monkeypatch.setattr(command_timing, "run_command_live", execute)
    monkeypatch.setattr(command_timing, "run_command_bytes", probe)
    output = "timings.json"
    if fail:
        with pytest.raises(subprocess.CalledProcessError) as error:
            command_timing.publish_command_timing(tmp_path, configuration, output)
        assert error.value.returncode == 23
        assert not (tmp_path / output).exists()
        return
    payload = command_timing.publish_command_timing(tmp_path, configuration, output)
    report = json.loads(payload)
    assert (tmp_path / output).read_bytes() == payload
    assert report["purpose"] == "exploratory-only"
    assert calls[0] == ("build", "--locked", "--release", "--bin", "cdt")
    sizes = {"small": (4, 5, 1000), "medium": (4, 8, 2000), "large": (5, 10, 3000), "extra-large": (8, 15, 5000)}
    for name, arguments in zip(sorted(sizes), calls[1:], strict=True):
        vertices, slices, steps = sizes[name]
        assert arguments == (
            "--vertices-per-slice",
            str(vertices),
            "--timeslices",
            str(slices),
            "--steps",
            str(steps),
            "--thermalization-steps",
            str(steps // 10),
            "--measurement-frequency",
            str(steps // 50),
            "--simulate",
        )
    assert len(report["observations"]) == 5
    assert report["policy"]["warmups"] == 0
    assert report["policy"]["repeats"] == 1
    summary = command_timing.summarize_commands(payload)
    assert all(name in summary for name in sizes)
    with pytest.raises(ValueError, match="exist"):
        command_timing.publish_command_timing(tmp_path, configuration, output)
    assert len(calls) == 5


@pytest.mark.parametrize("serial", [False, True])
@pytest.mark.parametrize("failure", [False, True])
def test_ci_runtime_lanes_overlap_only_when_enabled_and_preserve_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, serial: bool, failure: bool
) -> None:
    """Real Just scheduling preserves the serial fallback and each lane's status."""
    (tmp_path / "justfile").write_bytes((ROOT / "justfile").read_bytes())
    stub = tmp_path / "uv"
    stub.write_text(
        "#!/bin/sh\nset -eu\n"
        'test "$UV_NO_SYNC" = 1\n'
        'printf "%s\\n" "$*" >> "$LANE_COMMANDS"\n'
        'case "$*" in\n'
        '  *"python -m pytest"*) lane=python; other=rust ;;\n'
        '  *"cargo nextest run"*) lane=rust; other=python ;;\n'
        "  *) exit 0 ;;\n"
        "esac\n"
        'printf "start %s\\n" "$lane" >> "$LANE_LOG"\n'
        'touch "$LANE_ROOT/$lane"\n'
        'if [ "$LANE_SERIAL" = 0 ]; then\n'
        "  attempts=0\n"
        '  while [ ! -f "$LANE_ROOT/$other" ]; do\n'
        '    attempts=$((attempts + 1)); test "$attempts" -lt 200; sleep 0.02\n'
        "  done\n"
        "fi\n"
        'printf "end %s\\n" "$lane" >> "$LANE_LOG"\n'
        'if [ "$lane" = python ]; then exit "$LANE_FAILURE"; fi\n',
        encoding="utf-8",
        newline="\n",
    )
    stub.chmod(0o700)
    log = tmp_path / "lanes.log"
    for name, value in {
        "PATH": str(tmp_path) + os.pathsep + os.environ["PATH"],
        "LANE_ROOT": ".",
        "LANE_LOG": "lanes.log",
        "LANE_COMMANDS": "commands.log",
        "LANE_SERIAL": str(int(serial)),
        "LANE_FAILURE": "23" if failure else "0",
    }.items():
        monkeypatch.setenv(name, value)
    jobs = "1" if serial else "2"
    result = run_safe_command("just", ["--jobs", jobs, "_ci-runtime"], cwd=tmp_path, check=False)
    assert result.returncode == (23 if failure else 0), result.stderr
    commands = (tmp_path / "commands.log").read_text(encoding="utf-8").splitlines()
    assert len(commands) == 6
    expected = (
        "cargo nextest run --release --profile ci --lib --tests --verbose",
        "cargo test --doc --verbose",
        "notebooks execute --output-dir target/notebooks notebooks/00_quickstart.ipynb notebooks/01_spacetime_visualization.ipynb",
        "cargo bench --profile perf --bench allocation_profile",
        "validation cargo-examples tooling/examples.toml",
    )
    native = [command for command in commands if "python -m pytest" not in command]
    assert all(command.endswith(suffix) for command, suffix in zip(native, expected, strict=True))
    events = log.read_text(encoding="utf-8").splitlines()
    if serial:
        # Either dependency can acquire Just's single job slot first.
        assert events in (
            ["start python", "end python", "start rust", "end rust"],
            ["start rust", "end rust", "start python", "end python"],
        )
    else:
        assert set(events[:2]) == {"start python", "start rust"}
        assert set(events[2:]) == {"end python", "end rust"}
