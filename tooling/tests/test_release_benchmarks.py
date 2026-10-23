"""Exercise the release workflow's trusted handoff through installed registry tooling."""

import json
import os
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
import yaml
from research_repo_tools.criterion import Estimate, Sample
from research_repo_tools.evidence import Provenance
from research_repo_tools.process import run_safe_command
from research_repo_tools.release_assets import package_baseline, parse_release_target

if TYPE_CHECKING:
    import subprocess

ROOT = Path(__file__).resolve().parents[2]
COMMIT = "a" * 40
OTHER_COMMIT = "b" * 40
REPOSITORY = "acgetchell/causal-triangulations"
TAG = "v0.1.1"
ASSET = f"causal-triangulations-{TAG}-cdt-baseline-v2.tar.gz"

# Only the external GitHub service is replaced. The workflow shell, CLI parser,
# target validation, baseline reader and upload safeguards are the installed code.
GITHUB_STUB = """import hashlib
import json
import os
import sys
from pathlib import Path

root = Path(os.environ["RUNNER_TEMP"])
state = json.loads((root / "github.json").read_text(encoding="utf-8"))
args = sys.argv[1:]
with (root / "github-calls.jsonl").open("a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\\n")
repository = "acgetchell/causal-triangulations"
release_id = state.get("release_id", 17)
release = {
    "id": release_id, "tag_name": "v0.1.1", "name": "v0.1.1",
    "url": f"https://api.github.com/repos/{repository}/releases/{release_id}",
    "draft": True, "immutable": False, "prerelease": False, "assets": [],
}
uploaded = root / "uploaded.bin"
if "POST" in args:
    uploaded.write_bytes(Path(args[args.index("--input") + 1]).read_bytes())
    print("{}")
    sys.exit(0)
if uploaded.exists():
    data = uploaded.read_bytes()
    release["assets"] = [{
        "name": os.environ["RELEASE_ASSET"], "id": 99, "size": len(data),
        "digest": "sha256:" + hashlib.sha256(data).hexdigest(), "state": "uploaded",
    }]
endpoint = next(arg for arg in args if arg.startswith("repos/"))
if "/commits/" in endpoint:
    print(state["commit"])
elif endpoint.endswith("/releases?per_page=100"):
    print(json.dumps([[release]]))
elif endpoint.endswith("/assets?per_page=100"):
    print(json.dumps([release["assets"]]))
elif endpoint.endswith("/releases/assets/99"):
    sys.stdout.buffer.write(uploaded.read_bytes())
elif "/releases/" in endpoint:
    print(json.dumps(release))
elif "/git/ref/tags/" in endpoint:
    obj = {"sha": state.get("remote_commit", state["commit"]), "type": "commit"}
    if state.get("annotated"):
        obj = {"sha": "c" * 40, "type": "tag"}
    print(json.dumps({"ref": "refs/tags/v0.1.1", "object": obj}))
elif "/git/tags/" in endpoint:
    print(json.dumps({"sha": "c" * 40, "object": {"sha": state.get("remote_commit", state["commit"]), "type": "commit"}}))
else:
    raise SystemExit(f"Unexpected GitHub request: {args}")
"""


def _step(job: str, name: str) -> str:
    workflow = yaml.safe_load((ROOT / ".github/workflows/release-benchmarks.yml").read_text(encoding="utf-8"))
    command = next(step["run"] for step in workflow["jobs"][job]["steps"] if step.get("name") == name)
    assert isinstance(command, str)
    return command


def _run_step(directory: Path, job: str, name: str) -> subprocess.CompletedProcess[str]:
    return run_safe_command("bash", ["-e", "-o", "pipefail", "-c", _step(job, name)], cwd=directory, check=False)


@pytest.fixture
def release_environment(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Run workflow commands without network, uv installation or a release checkout."""
    monkeypatch.setenv("PATH", tmp_path.as_posix() + os.pathsep + os.environ["PATH"])
    monkeypatch.setenv("TOOLING_PYTHON", Path(sys.executable).as_posix())
    for name, value in {
        "RUNNER_TEMP": tmp_path.as_posix(),
        "GITHUB_OUTPUT": (tmp_path / "outputs").as_posix(),
        "GH_REPO": REPOSITORY,
        "RELEASE_TAG": TAG,
        "RELEASE_ASSET": ASSET,
        "RELEASE_COMMIT": COMMIT,
    }.items():
        monkeypatch.setenv(name, value)
    (tmp_path / "github.json").write_text(json.dumps({"commit": COMMIT}), encoding="utf-8")
    (tmp_path / "github_stub.py").write_text(GITHUB_STUB, encoding="utf-8")
    stubs = {
        "gh": 'exec "$TOOLING_PYTHON" "$RUNNER_TEMP/github_stub.py" "$@"\n',
        "uvx": (
            'while [ "$1" != "--from" ]; do shift; done\n'
            'test "$2" = "research-repo-tools==0.2.3" || exit 42\n'
            "shift 2\n"
            'if [ "$1" = "python" ]; then shift; exec "$TOOLING_PYTHON" "$@"; fi\n'
            'test "$1" = "research-repo-tools" || exit 43\n'
            'shift; exec "$TOOLING_PYTHON" -I -m research_repo_tools "$@"\n'
        ),
    }
    for name, body in stubs.items():
        path = tmp_path / name
        path.write_text("#!/bin/sh\nset -eu\n" + body, encoding="utf-8", newline="\n")
        path.chmod(0o700)
    if os.name == "nt":
        # Bash uses the extensionless shim; native Python subprocesses resolve PATHEXT.
        (tmp_path / "gh.cmd").write_text('@echo off\n"%TOOLING_PYTHON%" "%RUNNER_TEMP%/github_stub.py" %*\n', encoding="utf-8")
    (tmp_path / "baseline").mkdir()
    package_baseline(
        Sample((("fixture/workload", Estimate(10.0, 9.0, 11.0)),)),
        Provenance(COMMIT, "d" * 64, "e" * 64, (("release", TAG),)),
        tmp_path / "baseline" / ASSET,
    )
    return tmp_path


def _capture(directory: Path) -> str:
    result = _run_step(directory, "validate-release", "Capture verified draft GitHub Release target")
    assert result.returncode == 0, result.stderr
    outputs = dict(line.split("=", 1) for line in (directory / "outputs").read_text(encoding="utf-8").splitlines())
    target = parse_release_target(outputs["target"].encode())
    assert (target.repository, target.tag, target.release_id, target.commit) == (REPOSITORY, TAG, 17, COMMIT)
    assert outputs["commit"] == COMMIT
    return outputs["target"]


def _restore(directory: Path, target: str, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("RELEASE_TARGET", target)
    result = _run_step(directory, "publish-baseline", "Restore trusted preflight target")
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize("annotated", [False, True])
def test_release_handoff_attaches_to_original_draft_and_retries_identically(
    annotated: bool, release_environment: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The actual commands bind both lightweight and annotated tag commits across jobs."""
    directory = release_environment
    (directory / "github.json").write_text(json.dumps({"commit": COMMIT, "annotated": annotated}), encoding="utf-8")
    _restore(directory, _capture(directory), monkeypatch)
    verified = _run_step(directory, "publish-baseline", "Verify native baseline provenance against captured target")
    assert verified.returncode == 0, verified.stderr
    for _ in range(2):
        result = _run_step(directory, "publish-baseline", "Attach verified baseline to draft GitHub Release")
        assert result.returncode == 0, result.stderr
    calls = [json.loads(line) for line in (directory / "github-calls.jsonl").read_text(encoding="utf-8").splitlines()]
    uploads = [call for call in calls if "POST" in call]
    assert len(uploads) == 1
    assert f"https://uploads.github.com/repos/{REPOSITORY}/releases/17/assets?name={ASSET}" in uploads[0]
    assert not any("PATCH" in call for call in calls)
    assert (directory / "uploaded.bin").read_bytes() == (directory / "baseline" / ASSET).read_bytes()


@pytest.mark.parametrize("change", ["moved-tag", "replaced-release"])
def test_release_upload_rejects_identity_changed_after_preflight(change: str, release_environment: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A long benchmark cannot silently approve a newer remote identity at upload time."""
    directory = release_environment
    _restore(directory, _capture(directory), monkeypatch)
    state = {"commit": COMMIT, "remote_commit": OTHER_COMMIT} if change == "moved-tag" else {"commit": COMMIT, "release_id": 18}
    (directory / "github.json").write_text(json.dumps(state), encoding="utf-8")
    result = _run_step(directory, "publish-baseline", "Attach verified baseline to draft GitHub Release")
    assert result.returncode != 0
    assert ("tag commit differs" if change == "moved-tag" else "invalid tag or identity") in result.stderr
    assert not (directory / "uploaded.bin").exists()


@pytest.mark.parametrize(("commit", "remote"), [("", COMMIT), ("abc123", COMMIT), (COMMIT, OTHER_COMMIT)])
def test_release_preflight_rejects_unverified_commit(commit: str, remote: str, release_environment: Path) -> None:
    """Bad resolution or tag movement during preflight must not produce trusted outputs."""
    directory = release_environment
    (directory / "github.json").write_text(json.dumps({"commit": commit, "remote_commit": remote}), encoding="utf-8")
    result = _run_step(directory, "validate-release", "Capture verified draft GitHub Release target")
    assert result.returncode != 0
    assert not (directory / "outputs").exists()


@pytest.mark.parametrize("handoff", ["absent-output", "missing-file", "empty", "malformed", "wrong-repository", "wrong-tag"])
def test_release_upload_requires_original_handoff(handoff: str, release_environment: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Workflow restoration and the installed upload CLI reject absent or mismatched targets."""
    directory = release_environment
    if handoff == "absent-output":
        monkeypatch.setenv("RELEASE_TARGET", "")
        result = _run_step(directory, "publish-baseline", "Restore trusted preflight target")
        assert result.returncode != 0
    elif handoff != "missing-file":
        if handoff in {"empty", "malformed"}:
            payload = "" if handoff == "empty" else "{}"
        else:
            target = json.loads(_capture(directory))
            target["repository" if handoff == "wrong-repository" else "tag"] = "other/repository" if handoff == "wrong-repository" else "v0.1.2"
            payload = json.dumps(target)
        (directory / "release-target.json").write_text(payload, encoding="utf-8")
    verified = _run_step(directory, "publish-baseline", "Verify native baseline provenance against captured target")
    assert verified.returncode != 0
    result = _run_step(directory, "publish-baseline", "Attach verified baseline to draft GitHub Release")
    assert result.returncode != 0
    assert not (directory / "uploaded.bin").exists()


@pytest.mark.parametrize("change", ["revision", "release"])
def test_release_writer_checks_native_baseline_provenance(change: str, release_environment: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A valid archive from another measured source fails before the upload step."""
    directory = release_environment
    _restore(directory, _capture(directory), monkeypatch)
    package_baseline(
        Sample((("fixture/workload", Estimate(10.0, 9.0, 11.0)),)),
        Provenance(OTHER_COMMIT if change == "revision" else COMMIT, context=(("release", "v0.1.2" if change == "release" else TAG),)),
        directory / "baseline" / ASSET,
    )
    result = _run_step(directory, "publish-baseline", "Verify native baseline provenance against captured target")
    assert result.returncode != 0
    assert ("provenance differs" if change == "revision" else "expected release") in result.stderr
    assert not (directory / "uploaded.bin").exists()


@pytest.mark.parametrize(("head", "tag"), [(COMMIT, COMMIT), (OTHER_COMMIT, COMMIT), (COMMIT, OTHER_COMMIT), ("", COMMIT)])
def test_benchmark_checkout_requires_captured_commit(head: str, tag: str, release_environment: Path) -> None:
    """The pre-toolchain shell guard rejects wrong HEAD, missing HEAD and a moved local tag."""
    stub = release_environment / "git"
    stub.write_text(f'#!/bin/sh\ncase "$*" in *HEAD) printf "%s\\n" "{head}";; *) printf "%s\\n" "{tag}";; esac\n', encoding="utf-8", newline="\n")
    stub.chmod(0o700)
    result = _run_step(release_environment, "release-baseline", "Verify captured benchmark checkout")
    assert (result.returncode == 0) == (head == tag == COMMIT), result.stderr


def test_release_workflow_keeps_trusted_data_outside_measured_checkout() -> None:
    """Direct output dependencies cannot be replaced by benchmark-controlled artifact files."""
    workflow = yaml.safe_load((ROOT / ".github/workflows/release-benchmarks.yml").read_text(encoding="utf-8"))
    jobs = workflow["jobs"]
    assert workflow["permissions"] == {"contents": "read"}
    assert workflow["concurrency"]["cancel-in-progress"] is False
    assert jobs["validate-release"]["outputs"] == {
        "target": "${{ steps.release-target.outputs.target }}",
        "commit": "${{ steps.release-target.outputs.commit }}",
    }
    assert set(jobs["publish-baseline"]["needs"]) == {"validate-release", "release-baseline"}
    steps = jobs["release-baseline"]["steps"]
    checkout = steps[0]["with"]
    assert checkout == {"ref": "${{ needs.validate-release.outputs.commit }}", "fetch-depth": 0, "persist-credentials": False}
    assert steps[1]["name"] == "Verify captured benchmark checkout"
    assert steps[2]["with"]["cache"] == "false"
    assert steps[-1]["with"]["path"] == "${{ env.RELEASE_ASSET }}"
    writer = jobs["publish-baseline"]["steps"]
    restore = next(step for step in writer if step.get("name") == "Restore trusted preflight target")
    assert restore["env"]["RELEASE_TARGET"] == "${{ needs.validate-release.outputs.target }}"
    download = next(step for step in writer if "download-artifact@" in step.get("uses", ""))
    assert download["with"]["path"] == "${{ runner.temp }}/baseline"
    names = [step.get("name", "") for step in writer]
    assert names.index("Verify native baseline provenance against captured target") < names.index("Attach verified baseline to draft GitHub Release")
