"""Exercise CDT's release-series acquisition and checkout-free workflow with a local GitHub fixture."""

import hashlib
import html
import json
import os
import shutil
import sys
import tomllib
from dataclasses import replace
from importlib.metadata import version
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
import tomli_w
import yaml
from research_repo_tools.candidate_reuse import parse_candidate_policy, serialize_candidate_policy, verify_candidate_archive, verify_candidate_release
from research_repo_tools.cli import main
from research_repo_tools.evidence import parse_evidence, serialize_evidence
from research_repo_tools.process import run_git_command, run_safe_command
from research_repo_tools.release_assets import parse_release_target
from research_repo_tools.release_interfaces import load_evidence_policy
from research_repo_tools.release_staging import stage_release_assets

if TYPE_CHECKING:
    import subprocess
    from collections.abc import Callable

    from research_repo_tools.candidate_reuse import CandidatePolicy

ROOT = Path(__file__).resolve().parents[2]
FLOW = "tooling/performance-release.toml"
SERIES = "benchmark-series/cdt-release-signal.toml"
COMMIT = "a" * 40
OTHER_COMMIT = "b" * 40
REPOSITORY = "acgetchell/causal-triangulations"
TAG = "v0.2.0"
ASSET = f"causal-triangulations-{TAG}-cdt-series.tar.gz"

GITHUB_STUB = """import hashlib
import json
import os
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

root = Path(os.environ["RUNNER_TEMP"])
state_path = root / "github.json"
state = json.loads(state_path.read_bytes())
args = sys.argv[1:]
with (root / "github-calls.jsonl").open("a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\\n")
repository = os.environ["GH_REPO"]
tag = os.environ["RELEASE_TAG"]
release_id = state.get("release_id", 17)
assets = state.setdefault("assets", [])
release = {
    "id": release_id, "tag_name": tag, "name": state.get("title", tag),
    "url": f"https://api.github.com/repos/{repository}/releases/{release_id}",
    "draft": True, "immutable": False, "prerelease": False, "assets": assets,
}
if "POST" in args:
    url = urlsplit(args[args.index("POST") + 1])
    assert url.netloc == "uploads.github.com" and url.path == f"/repos/{repository}/releases/{release_id}/assets"
    name = parse_qs(url.query)["name"][0]
    assert all(asset["name"] != name for asset in assets)
    data = Path(args[args.index("--input") + 1]).read_bytes()
    identifier = 101 + len(assets)
    asset = {"id": identifier, "name": name, "size": len(data), "state": "uploaded",
             "digest": "sha256:" + hashlib.sha256(data).hexdigest()}
    (root / f"asset-{identifier}").write_bytes(data)
    assets.append(asset)
    state_path.write_bytes(json.dumps(state).encode())
    print(json.dumps(asset))
    sys.exit(0)
endpoint = next(arg for arg in args if arg.startswith("repos/"))
releases = f"repos/{repository}/releases"
git = f"repos/{repository}/git"
tag_object = "c" * 40
if endpoint == f"{releases}?per_page=100":
    print(json.dumps([[release]]))
elif endpoint == f"{releases}/{release_id}/assets?per_page=100":
    print(json.dumps([assets]))
elif endpoint in {f"{releases}/assets/{asset['id']}" for asset in assets}:
    sys.stdout.buffer.write((root / ("asset-" + endpoint.rsplit("/", 1)[1])).read_bytes())
elif endpoint == f"{releases}/{release_id}":
    print(json.dumps(release))
elif endpoint == f"{git}/ref/tags/{tag}":
    obj = {"sha": state.get("remote_commit", state["commit"]), "type": "commit"}
    if state.get("annotated"):
        obj = {"sha": tag_object, "type": "tag"}
    print(json.dumps({"ref": f"refs/tags/{tag}", "object": obj}))
elif state.get("annotated") and endpoint == f"{git}/tags/{tag_object}":
    print(json.dumps({"sha": tag_object, "object": {"sha": state.get("remote_commit", state["commit"]), "type": "commit"}}))
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
    tmp_path = tmp_path / "workflow"
    tmp_path.mkdir()
    monkeypatch.setenv("PATH", tmp_path.as_posix() + os.pathsep + os.environ["PATH"])
    monkeypatch.setenv("TOOLING_PYTHON", Path(sys.executable).as_posix())
    for name, value in {
        "RUNNER_TEMP": tmp_path.as_posix(),
        "GITHUB_OUTPUT": (tmp_path / "outputs").as_posix(),
        "GITHUB_REF": f"refs/tags/{TAG}",
        "GITHUB_SHA": COMMIT,
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
            f'test "$2" = "research-repo-tools=={version("research-repo-tools")}" || exit 42\n'
            "shift 2\n"
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
    return tmp_path


def _capture(directory: Path, commit: str = COMMIT) -> str:
    result = _run_step(directory, "validate-release", "Capture verified draft GitHub Release target")
    assert result.returncode == 0, result.stderr
    outputs = dict(line.split("=", 1) for line in (directory / "outputs").read_text(encoding="utf-8").splitlines())
    target = parse_release_target(outputs["target"].encode())
    assert (target.repository, target.tag, target.release_id, target.commit, target.expected_title) == (REPOSITORY, TAG, 17, commit, TAG)
    assert outputs["commit"] == commit
    return outputs["target"]


def _exercise_staged_attachment(directory: Path, owner: Path, head: str, monkeypatch: pytest.MonkeyPatch) -> None:
    """Stage original inert bytes, then run the actual receiver, including retries and identity drift."""
    state_path = directory / "github.json"
    state_path.write_bytes(json.dumps({"commit": head, "annotated": True}).encode())
    monkeypatch.setenv("GITHUB_SHA", head)
    serialized = _capture(directory, head)
    monkeypatch.setenv("RELEASE_TARGET", serialized)
    assets = directory / "inputs"
    assets.mkdir()
    for source, name in (("benchmark.tar.gz", ASSET), ("final.json", "policy.json"), ("accepted.json", "acceptance.json")):
        shutil.copyfile(owner / source, assets / name)
    (assets / "causal-triangulations-0.2.0.crate").write_bytes(b"signed Cargo fixture")
    transfer = stage_release_assets(
        directory,
        parse_release_target(serialized.encode()),
        tuple(sorted(assets.iterdir())),
        journal=directory / "journal.json",
        destination=directory / "original.json",
        approve=True,
    )
    monkeypatch.setenv("TRANSFER", transfer.decode())
    monkeypatch.setenv("TRANSFER_SHA256", hashlib.sha256(transfer).hexdigest())

    def verify() -> subprocess.CompletedProcess[str]:
        return _run_step(directory, "verify-attachment", "Verify the staged archive against its accepted acquisition")

    for _ in range(2):
        result = verify()
        assert result.returncode == 0, result.stderr
        assert (directory / "staged" / ASSET).read_bytes() == (owner / "benchmark.tar.gz").read_bytes()
    original = state_path.read_bytes()
    state = json.loads(original)
    archive = next(asset for asset in state["assets"] if asset["name"] == ASSET)
    archive["id"] += 100
    state_path.write_bytes(json.dumps(state).encode())
    replaced = verify()
    assert replaced.returncode != 0
    assert "original staged asset identity changed" in replaced.stderr
    state_path.write_bytes(original)
    monkeypatch.setenv("TRANSFER_SHA256", "0" * 64)
    tampered = verify()
    assert tampered.returncode != 0
    assert "trusted transfer" in tampered.stderr
    calls = [json.loads(line) for line in (directory / "github-calls.jsonl").read_text(encoding="utf-8").splitlines()]
    assert sum("POST" in call for call in calls) == len(list(assets.iterdir()))
    assert not any("PATCH" in call for call in calls)


@pytest.mark.parametrize(("commit", "remote"), [("", COMMIT), ("abc123", COMMIT), (COMMIT, OTHER_COMMIT)])
def test_release_preflight_rejects_unverified_commit(commit: str, remote: str, release_environment: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Bad resolution or tag movement during preflight must not produce trusted outputs."""
    directory = release_environment
    monkeypatch.setenv("GITHUB_SHA", commit)
    (directory / "github.json").write_text(json.dumps({"commit": remote}), encoding="utf-8")
    result = _run_step(directory, "validate-release", "Capture verified draft GitHub Release target")
    assert result.returncode != 0
    assert not (directory / "outputs").exists()


@pytest.mark.parametrize("ref", ["refs/heads/main", "refs/tags/v0.1.2"])
def test_release_preflight_rejects_wrong_ref(ref: str, release_environment: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A branch or another tag cannot dispatch the selected release workflow."""
    monkeypatch.setenv("GITHUB_REF", ref)
    result = _run_step(release_environment, "validate-release", "Capture verified draft GitHub Release target")
    assert result.returncode != 0
    assert "Dispatch ref must match the release tag." in result.stderr
    assert not (release_environment / "github-calls.jsonl").exists()
    assert not (release_environment / "outputs").exists()


def test_release_preflight_rejects_wrong_draft_title(release_environment: Path) -> None:
    """The captured stable draft must also have the reviewed tag title."""
    state = {"commit": COMMIT, "title": "Different draft title"}
    (release_environment / "github.json").write_text(json.dumps(state), encoding="utf-8")
    result = _run_step(release_environment, "validate-release", "Capture verified draft GitHub Release target")
    assert result.returncode != 0
    assert "title" in result.stderr
    assert not (release_environment / "outputs").exists()


def test_release_workflow_has_no_measurement_or_checkout() -> None:
    """Each workflow job runs only its declared installed tools against inert bytes."""
    workflow = yaml.safe_load((ROOT / ".github/workflows/release-benchmarks.yml").read_bytes())
    prepare = yaml.safe_load((ROOT / ".github/workflows/prepare-release.yml").read_bytes())
    assert prepare["concurrency"] == {"group": "release-draft-${{ github.repository }}-${{ github.ref_name }}", "cancel-in-progress": False}
    assert "concurrency" not in workflow
    assert set(workflow[True]["workflow_dispatch"]["inputs"]) == {"tag", "transfer", "digest"}
    jobs = workflow["jobs"]
    assert jobs.keys() == {"validate-release", "verify-attachment"}
    assert jobs["verify-attachment"]["needs"] == "validate-release"
    receiver = next(step for step in jobs["verify-attachment"]["steps"] if "release assets receive" in step.get("run", ""))
    assert receiver["env"]["TRANSFER"] == "${{ inputs.transfer }}"
    assert receiver["env"]["TRANSFER_SHA256"] == "${{ inputs.digest }}"
    for job in jobs.values():
        for step in job["steps"]:
            if "uses" in step:
                assert step["uses"].split("@")[0] in {"astral-sh/setup-uv", "actions/upload-artifact"}
            else:
                assert "cargo " not in step["run"]
                assert "performance release " not in step["run"]


# Substitute only the native process boundary. RRT discovers the complete declared inventory,
# checks each fresh/saved byte pair, captures provenance, and renders the real CDT reports.
CRITERION_FIXTURE = """
import json
import sys
import tomllib
from pathlib import Path

mode, log = sys.argv[1:]
if mode == 'gate':
    assert Path('tests/physics_integration.rs').is_file()
    sys.exit(0)
declaration = tomllib.loads(Path('benchmark-series/cdt-release-signal.toml').read_text(encoding='utf-8'))
cases = declaration['benchmark-series']['workloads']['cdt']
if mode == 'list':
    for case in cases:
        print(case + ': benchmark')
    sys.exit(0)
assert mode == 'measure'
with Path(log).open('ab') as stream:
    stream.write(b'measure\\n')
estimate = {'point_estimate': 10.0, 'confidence_interval': {'confidence_level': 0.95, 'lower_bound': 9.0, 'upper_bound': 11.0}}
for case in cases:
    for sample in ('new', 'retained'):
        directory = Path('target/criterion') / case / sample
        directory.mkdir(parents=True)
        for name, data in {
            'benchmark': {'full_id': case}, 'sample': {'iters': [1] * 10, 'times': [10] * 10},
            'estimates': {'mean': estimate, 'median': estimate}, 'tukey': [1, 2, 3, 4],
        }.items():
            (directory / (name + '.json')).write_bytes(json.dumps(data).encode() + b'\\r\\n')
"""


def test_release_dependency_probes_read_the_lock_with_an_empty_cargo_cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Exercise the exported-tree probes before Cargo has fetched any dependencies."""
    monkeypatch.setenv("CARGO_HOME", str(tmp_path))
    plan = tomllib.loads((ROOT / SERIES).read_text(encoding="utf-8"))["suites"][0]["measurement"]
    packages = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    for name in ("criterion", "delaunay", "la-stack", "markov-chain-monte-carlo"):
        (package,) = [entry for entry in packages if entry["name"] == name]
        command, *args = plan["probes"][name]
        identity = run_safe_command(command, args, cwd=ROOT).stdout.strip()
        assert identity == f"{package['source']}#{name}@{package['version']}"


def _write_suite(checkout: Path, log: Path) -> None:
    """Use CDT's actual suite declaration, replacing only the expensive processes."""
    for name in (FLOW, SERIES, "tooling/series-readme.toml", "tooling/series-report.toml", "README.md", "Cargo.lock", "rust-toolchain.toml"):
        dest = checkout / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, dest)
    for name in ("src/lib.rs", "tests/physics_integration.rs", "benches/ci_performance_suite.rs", "benches/support/or_abort.rs"):
        dest = checkout / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(b"// representative fixture\n")
    cargo = checkout / "Cargo.toml"
    cargo.write_bytes(b'[package]\nname = "causal-triangulations"\nversion = "0.2.0"\n')
    (checkout / "fixture.py").write_bytes(CRITERION_FIXTURE.encode())
    declaration = checkout / SERIES
    document = tomllib.loads(declaration.read_text(encoding="utf-8"))
    plan = document["suites"][0]["measurement"]
    for phase in plan["phases"].values():
        for label, action in (("command", "measure"), ("gate", "gate")):
            assert phase[label][0] == "cargo"
            phase[label] = [sys.executable, "fixture.py", action, str(log)]
        phase["discovery"]["suites"][0]["command"] = [sys.executable, "fixture.py", "list", str(log)]
    # Preserve the build profile/flags probe; replace only native tool-version queries.
    for name in plan["probes"].keys() - {"build-policy"}:
        plan["probes"][name] = [sys.executable, "--version"]
    declaration.write_bytes(tomli_w.dumps(document).encode())


def _acquire_initial_series(checkout: Path, owner: Path) -> tuple[bytes, bytes]:
    """Verify the first acquisition, report publication and independent rendering retries."""
    args = ["--root", str(checkout), "performance"]
    assert main([*args, "prepare", FLOW]) == 0
    assert main([*args, "release", FLOW, "--owner", str(owner), "--progress", "none"]) == 0

    raw = (owner / "run.json").read_bytes(), (owner / "evidence.json").read_bytes()
    assert main([*args, "capture", FLOW, "--owner", str(owner), "--promote"]) == 0
    assert main([*args, "readme", FLOW]) == 0
    assert main([*args, "readme", FLOW, "--check"]) == 0
    report = html.unescape((checkout / "docs/performance/current.md").read_text(encoding="utf-8"))
    inventory = tomllib.loads((checkout / SERIES).read_text(encoding="utf-8"))["benchmark-series"]["workloads"]["cdt"]
    assert len(inventory) == 54
    assert all(case in report for case in inventory)
    assert "Baseline / current" not in report
    assert "[9, 11]" in (checkout / "README.md").read_text(encoding="utf-8")
    return raw


def _acquire_successor(checkout: Path, owner: Path) -> None:
    """Enable the paired schedule only once a compatible baseline has been published."""
    cargo = checkout / "Cargo.toml"
    declaration = checkout / SERIES
    document = tomllib.loads(declaration.read_text(encoding="utf-8"))
    args = ["--root", str(checkout), "performance"]
    cargo.write_bytes(cargo.read_bytes().replace(b"0.2.0", b"0.2.1"))
    document.update(schema=6, schedule={"order": "alternating", "trials": 2})
    declaration.write_bytes(tomli_w.dumps(document).encode())
    (checkout / FLOW).write_bytes((checkout / FLOW).read_bytes().replace(b"target/bench-reports/", b"target/successor/"))
    assert main([*args, "prepare", FLOW]) == 0
    assert main([*args, "release", FLOW, "--owner", str(owner), "--progress", "none"]) == 0
    report = (checkout / "docs/performance/current.md").read_text(encoding="utf-8")
    first, second = report.split("## Trial 2: current → baseline")
    assert "## Trial 1: baseline → current" in first
    for trial in (first, second):
        assert "Baseline / current" in trial
        assert "<td>1</td>" in trial


def _verify_archive_recovery(owner: Path, policy: CandidatePolicy, accepted: bytes, raw: tuple[bytes, bytes]) -> None:
    """Both archive derivations must reproduce the original raw payload and manifest."""
    (owner / "final.json").write_bytes(serialize_candidate_policy(policy))
    (owner / "accepted.json").write_bytes(accepted)
    archive = [
        "--root",
        str(owner),
        "release",
        "evidence",
        "archive",
        "derive",
        "--policy",
        "final.json",
        "--acceptance",
        "accepted.json",
        "--payload",
        "run.json",
        "--manifest",
        "evidence.json",
        "--output",
        "benchmark.tar.gz",
    ]
    for _ in range(2):
        assert main(archive) == 0
        assert serialize_evidence(verify_candidate_archive(owner / "benchmark.tar.gz", policy, accepted)) == raw


def _check_published_acceptance(verify: Callable[[bytes], bytes], accepted: bytes, published: dict[str, object]) -> None:
    """Registry approval must reproduce the pre-tag acceptance only for an immutable stable release."""
    assert verify(accepted) == accepted
    with pytest.MonkeyPatch.context() as patch:
        patch.setitem(published, "immutable", value=False)
        with pytest.raises(ValueError, match="immutable stable release"):
            verify(accepted)


def _check_changed_source(verify: Callable[[bytes | None], bytes], accepted: bytes) -> None:
    """Reject both reuse of the earlier acceptance and a fresh binding to changed inputs."""
    with pytest.raises(ValueError, match="candidate acceptance policy/identity changed"):
        verify(accepted)
    with pytest.raises(ValueError, match=r"performance-relevant or undeclared final-tree differences: \['src/lib\.rs'\]"):
        verify(None)


def test_release_series_uses_original_acquisition(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, release_environment: Path) -> None:
    """Use the current-only series after v0.1.1 and acquire a pair only for its successor."""
    checkout = tmp_path.resolve() / "checkout"
    checkout.mkdir()
    for key in tuple(os.environ):
        if key.upper().startswith("GIT_"):
            monkeypatch.delenv(key)
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", os.devnull)
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")

    def git(*args: str) -> str:
        return run_git_command(["--no-pager", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", *args], cwd=checkout).stdout.strip()

    def commit(message: str) -> str:
        git("add", ".")
        git("commit", "-m", message)
        return git("rev-parse", "HEAD")

    git("init", "--template=", "-b", "main")
    git("config", "core.autocrlf", "false")
    (checkout / ".gitignore").write_bytes(b"target/\n")
    releases = {"v0.1.1": commit("Previous release")}
    git("tag", "v0.1.1")
    published: dict[str, object] = {
        "id": 17,
        "tag_name": TAG,
        "url": f"https://api.github.com/repos/{REPOSITORY}/releases/17",
        "draft": False,
        "prerelease": False,
        "immutable": True,
    }

    def provider(endpoint: str, **_options: object) -> object:
        if endpoint == "releases?per_page=100&page=1":
            return [
                {"id": 17 if tag == TAG else 1, "tag_name": tag, "draft": False, "prerelease": False, "published_at": "2026-01-01T00:00:00Z"}
                for tag in releases
            ]
        if endpoint == f"releases/tags/{TAG}":
            return published
        tag = endpoint.removeprefix("git/ref/tags/")
        return {"ref": f"refs/tags/{tag}", "object": {"type": "commit", "sha": releases[tag]}}

    monkeypatch.setattr("research_repo_tools.release_validation.api", provider)
    # Registry attestations and the signed CI run belong to separate native integration gates.
    monkeypatch.setattr("research_repo_tools.dependency_provenance.check_release_dependencies", lambda *_args, **_kwargs: ())
    log = tmp_path / "measurements"
    _write_suite(checkout, log)
    commit("Declare the release series")
    owner = tmp_path / "trusted owner"
    raw = _acquire_initial_series(checkout, owner)
    head = commit("Promote the original report and README")
    policy = replace(parse_candidate_policy((owner / "policy.json").read_bytes()), commit=head)
    evidence = parse_evidence(*raw)
    native = load_evidence_policy(ROOT / "tooling/release-evidence.toml")
    monkeypatch.setattr("research_repo_tools.candidate_reuse.verify_release_evidence", lambda *_args, **_kwargs: b"{}")

    def verify(expected: bytes | None = None) -> bytes:
        return verify_candidate_release(
            checkout,
            evidence,
            policy,
            native,
            provider,
            expected=expected,
            acquisition_sha256=(owner / "acquisition.sha256").read_text(encoding="utf-8").strip(),
            publication_receipt=(owner / "publication.json").read_bytes(),
            publication_sha256=(owner / "publication.sha256").read_text(encoding="utf-8").strip(),
        )

    accepted = verify()
    assert verify(accepted) == accepted
    _verify_archive_recovery(owner, policy, accepted, raw)
    _exercise_staged_attachment(release_environment, owner, head, monkeypatch)
    git("tag", TAG, head)
    releases[TAG] = head
    _check_published_acceptance(verify, accepted, published)
    assert log.read_bytes().splitlines() == [b"measure"]
    (checkout / "src/lib.rs").write_bytes(b"// changed input\n")
    changed = commit("Change performance-relevant source")
    policy = replace(policy, commit=changed)
    _check_changed_source(verify, accepted)
    _acquire_successor(checkout, tmp_path / "successor")
    assert log.read_bytes().splitlines() == [b"measure"] * 5
    assert ((owner / "run.json").read_bytes(), (owner / "evidence.json").read_bytes()) == raw
