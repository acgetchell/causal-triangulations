"""Exercise CDT's signed Cargo selection and approval boundary with an inert provider."""

import hashlib
import io
import json
import os
import runpy
import shlex
import tarfile
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest
import yaml
from research_repo_tools import release_evidence
from research_repo_tools.cli import main
from research_repo_tools.process import run_safe_command
from research_repo_tools.release_assets import GitHubRelease, ReleaseTarget, serialize_release_target
from research_repo_tools.release_interfaces import load_evidence_policy
from research_repo_tools.release_receipts import DraftReceipt

ROOT = Path(__file__).resolve().parents[2]
POLICY = load_evidence_policy(ROOT / "tooling/release-evidence.toml")
SHA = "a" * 40
REPOSITORY = "acgetchell/causal-triangulations"
VERSION = "0.2.0"
CRATE = f"causal-triangulations-{VERSION}.crate"


def test_registry_approval_contract() -> None:
    """Bind the actual read-only event steps, approval permissions and single native upload."""
    script = runpy.run_path(str(ROOT / "tooling/verify_registry_candidate.py"))
    workflow = yaml.safe_load((ROOT / ".github/workflows/publish-crates.yml").read_bytes())
    jobs = workflow["jobs"]
    events = workflow.get("on", workflow.get(True))
    assert events["release"] == {"types": ["published"]}
    assert set(events["workflow_dispatch"]["inputs"]) == {"tag", "transfer", "proof"}
    for name in ("verify", "published-release"):
        assert jobs[name]["permissions"] == {"contents": "read", "checks": "read", "actions": "read"}
    event_job, required = script["EVENT_GATE"].required_steps[0]
    assert set(required) <= {step.get("name") for step in jobs[event_job]["steps"]}
    publish = jobs["publish"]
    assert publish["environment"] == "crates-io"
    assert publish["needs"] == "verify"
    assert publish["permissions"] == {**jobs["verify"]["permissions"], "id-token": "write"}
    recheck, auth, upload, observe = publish["steps"][-4:]
    assert "docker run" in recheck["run"]
    assert "--env GH_TOKEN" in recheck["run"]
    assert "ACTIONS_ID_TOKEN" not in recheck["run"]
    assert auth["uses"] == "rust-lang/crates-io-auth-action@3268eb413133e2960fa4ae0e963ca76380c54cd9"
    assert upload["env"] == {"CARGO_REGISTRY_TOKEN": "${{ steps.auth.outputs.token }}"}
    assert upload["run"].split()[-8:] == ["cargo", "publish", "--locked", "--no-verify", "--package", "causal-triangulations", "--registry", "crates-io"]
    assert "--env CARGO_REGISTRY_TOKEN" in upload["run"]
    assert upload["continue-on-error"] is True
    assert observe["if"] == "${{ !cancelled() && steps.auth.outcome == 'success' }}"
    assert not observe.get("continue-on-error", False)
    assert '--publication-only --target "$RUNNER_TEMP/approved/target.json" --require-crate-asset' in observe["run"]


def test_release_benchmarks_dispatches_the_reviewed_tag_and_transfer(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Exercise the real Just recipe with a recording GitHub CLI and an already verified draft."""
    tag = f"v{VERSION}"
    owner = tmp_path / "release owner"
    state = owner / "final" / SHA
    state.mkdir(parents=True)
    (state / "policy.json").write_bytes(json.dumps({"tag": tag, "commit": SHA}).encode())
    (state / "selection.json").write_bytes(b"native selection")
    transfer = state / "transfer.json"
    transfer.write_bytes(b"reviewed transfer")
    provider = tmp_path / "gh"
    provider.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> "$TRACE"\nexit "$DISPATCH_STATUS"\n', encoding="utf-8", newline="\n")
    provider.chmod(0o755)
    trace = tmp_path / "dispatch"
    monkeypatch.setenv("TRACE", trace.as_posix())
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    monkeypatch.setenv("CDT_RELEASE_OWNER", owner.as_posix())
    args = ["--set", "_commit", SHA, "--set", "_release_tools", "true", "release-benchmarks"]
    assert run_safe_command("just", args, cwd=ROOT, check=False).returncode != 0
    assert not trace.exists()
    for status in (0, 23):
        trace.write_bytes(b"")
        monkeypatch.setenv("DISPATCH_STATUS", str(status))
        result = run_safe_command("just", [*args, tag], cwd=ROOT, check=False)
        assert result.returncode == status, result.stderr
        assert trace.read_text(encoding="utf-8").splitlines() == [
            "workflow",
            "run",
            "release-benchmarks.yml",
            "--repo",
            REPOSITORY,
            "--ref",
            tag,
            "-f",
            f"tag={tag}",
            "-F",
            f"transfer=@{transfer.as_posix()}",
            "-f",
            f"digest={hashlib.sha256(transfer.read_bytes()).hexdigest()}",
        ]


def test_upload_refuses_to_redispatch_existing_attempt(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Even an inconclusive previous upload requires operator inspection before retry."""
    tag = f"v{VERSION}"
    owner = tmp_path / "release owner"
    draft = owner / "final" / SHA / "draft"
    draft.mkdir(parents=True)
    (draft / "target.json").write_bytes(serialize_release_target(ReleaseTarget(REPOSITORY, tag, 7, SHA)))
    provider = tmp_path / "gh"
    result = [{"workflow_runs": [{"display_title": f"Publish {tag} to crates.io", "html_url": "https://example.invalid/run/17"}]}]
    provider.write_text(
        '#!/usr/bin/env bash\nset -eu\nprintf "%s\\n" "$1" >> "$TRACE"\n[[ "$1" == api ]]\nprintf "%s\\n" "$PROVIDER"\n',
        encoding="utf-8",
        newline="\n",
    )
    provider.chmod(0o755)
    trace = tmp_path / "calls"
    monkeypatch.setenv("CDT_RELEASE_OWNER", owner.as_posix())
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    monkeypatch.setenv("TRACE", trace.as_posix())
    monkeypatch.setenv("PROVIDER", json.dumps(result))
    attempt = run_safe_command("just", ["--set", "_commit", SHA, "release-upload", tag], cwd=ROOT, check=False)
    assert attempt.returncode != 0
    assert "A registry dispatch already exists" in attempt.stderr
    assert trace.read_bytes().splitlines() == [b"api"]


@pytest.mark.parametrize("scenario", ["verified", "wrong-run", "active", "previous-upload", "registry-present"])
def test_corrected_upload_checks_every_attempt_and_registry(scenario: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Only a reviewed transfer whose previous attempts stopped before publication can be dispatched again."""
    tag = f"v{VERSION}"
    owner = tmp_path / "release owner"
    state = owner / "final" / SHA
    (state / "draft").mkdir(parents=True)
    (state / "draft/target.json").write_bytes(serialize_release_target(ReleaseTarget(REPOSITORY, tag, 7, SHA)))
    transfer = state / "transfer.json"
    transfer.write_bytes(b"reviewed transfer")
    (state / "reviewed.sha256").write_bytes(hashlib.sha256(transfer.read_bytes()).hexdigest().encode())
    (state / "acceptance.json").write_bytes(b"accepted")
    (owner / "acquisition.sha256").write_bytes(b"b" * 64)
    (owner / "publication.sha256").write_bytes(b"c" * 64)
    run = {"id": 19, "display_title": f"Publish {tag} to crates.io", "run_attempt": 2, "status": "in_progress" if scenario == "active" else "completed"}
    monkeypatch.setenv("PRIOR", json.dumps([{"workflow_runs": [{**run, "conclusion": "failure"}]}]))
    for attempt in (1, 2):
        job = {"run_id": 19, "run_attempt": attempt, "status": "completed"}
        publish = "success" if scenario == "previous-upload" and attempt == 1 else "skipped"
        jobs = [{**job, "name": "verify", "conclusion": "failure"}, {**job, "name": "publish", "conclusion": publish}]
        monkeypatch.setenv(f"JOBS_{attempt}", json.dumps([{"jobs": jobs}]))
    event = {**run, "id": 17, "display_title": f"Verify published release {tag} (release 7)", "run_attempt": 1, "conclusion": "success"}
    monkeypatch.setenv("EVENT", json.dumps([{"workflow_runs": [event]}]))
    artifact = {"id": 21, "name": "published-release-17-1", "expired": False, "digest": "sha256:" + "a" * 64}
    monkeypatch.setenv("ARTIFACT", json.dumps([{"artifacts": [artifact]}]))
    provider = tmp_path / "gh"
    provider.write_text(
        """#!/usr/bin/env bash
set -eu
case "$*" in
  *'event=workflow_dispatch&'*) echo "$PRIOR";;
  *'/runs/19/attempts/1/jobs?'*) echo "$JOBS_1";;
  *'/runs/19/attempts/2/jobs?'*) echo "$JOBS_2";;
  *'event=release&'*) echo "$EVENT";;
  *'/runs/17/artifacts?'*) echo "$ARTIFACT";;
  'release registry '*) printf '%s' "$*" > "$REGISTRY"; exit "$REGISTRY_STATUS";;
  'workflow run '*) printf '%s\\n' "$@" > "$DISPATCH";;
  *) exit 23;;
esac
""",
        encoding="utf-8",
        newline="\n",
    )
    provider.chmod(0o755)
    registry, dispatch = tmp_path / "registry", tmp_path / "dispatch"
    monkeypatch.setenv("REGISTRY", registry.as_posix())
    monkeypatch.setenv("REGISTRY_STATUS", "1" if scenario == "registry-present" else "0")
    monkeypatch.setenv("DISPATCH", dispatch.as_posix())
    monkeypatch.setenv("CDT_RELEASE_OWNER", owner.as_posix())
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    args = ["--set", "_commit", SHA, "--set", "_release_tools", "gh", "release-upload", tag, "18" if scenario == "wrong-run" else "19"]
    result = run_safe_command("just", args, cwd=ROOT, check=False)
    if scenario in {"verified", "registry-present"}:
        assert registry.read_text(encoding="utf-8") == f"release registry {tag} --expect absent"
    else:
        assert not registry.exists()
    if scenario == "verified":
        assert result.returncode == 0, result.stderr
        args = dispatch.read_text(encoding="utf-8").splitlines()
        assert args[:-1] == [
            "workflow",
            "run",
            "publish-crates.yml",
            "--repo",
            REPOSITORY,
            "--ref",
            tag,
            "-f",
            f"tag={tag}",
            "-F",
            f"transfer=@{transfer.as_posix()}",
            "-f",
        ]
        assert json.loads(args[-1].removeprefix("proof=")) == {**registry_proof(transfer.read_bytes()), "acceptance": hashlib.sha256(b"accepted").hexdigest()}
    else:
        assert result.returncode != 0
        assert not dispatch.exists()
        if scenario in {"wrong-run", "active"}:
            assert "expected the latest failed dispatch and only completed failed runs" in result.stderr
        elif scenario == "previous-upload":
            assert "a skipped publisher in run 19 attempt 1" in result.stderr


@pytest.mark.parametrize("invalid", ["action", "tag", "draft", "prerelease", "repository"])
def test_published_event_rejects_invalid_source_before_registry(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    invalid: str,
) -> None:
    """Exercise the installed release gate with CDT's real policy and an inert provider event."""
    tag = f"v{VERSION}"
    release: dict[str, Any] = {"id": 7, "draft": invalid == "draft", "prerelease": invalid == "prerelease", "tag_name": "wrong" if invalid == "tag" else tag}
    event = {
        "action": "created" if invalid == "action" else "published",
        "release": release,
        "repository": {"full_name": "other/fork" if invalid == "repository" else REPOSITORY},
    }
    event_path = tmp_path / "event.json"
    event_path.write_bytes(json.dumps(event).encode())
    for key, value in {
        "GITHUB_EVENT_NAME": "release",
        "GITHUB_EVENT_PATH": str(event_path),
        "GITHUB_REF": f"refs/tags/{tag}",
        "GITHUB_SHA": SHA,
        "GITHUB_REPOSITORY": REPOSITORY,
    }.items():
        monkeypatch.setenv(key, value)
    monkeypatch.setattr("research_repo_tools.release_publishing.check_reviewed_release", lambda *_a, **_kw: pytest.fail("invalid event reached source gate"))
    assert main(["--root", str(ROOT), "release", "gate", tag, "--require-crate-asset"]) != 0


def registry_proof(transfer: bytes) -> dict[str, object]:
    """Match the operator proof emitted by release-upload."""
    return {
        "run": 17,
        "attempt": 1,
        "artifact": 21,
        "digest": "sha256:" + "a" * 64,
        "transfer": hashlib.sha256(transfer).hexdigest(),
        "acceptance": "a" * 64,
        "acquisition": "b" * 64,
        "publication": "c" * 64,
    }


@pytest.mark.parametrize("change", ["transfer", "tag", "commit", "repository", "state", "missing-acquisition", "null-publication", "invalid-run"])
def test_registry_candidate_rejects_changed_dispatch_before_remote_io(change: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """An altered transfer or mismatched dispatch cannot start acquisition, provider access or Cargo."""
    tag = f"v{VERSION}"
    target = ReleaseTarget("other/fork" if change == "repository" else REPOSITORY, tag, 7, SHA)
    transfer = json.dumps(
        {
            "schema": "research-repo-tools/release-asset-batch/v1",
            "state": "staging" if change == "state" else "complete",
            "target": json.loads(serialize_release_target(target)),
        }
    ).encode()
    proof = registry_proof(transfer)
    if change == "transfer":
        proof["transfer"] = "0" * 64
    if change == "invalid-run":
        proof["run"] = True
    if change == "null-publication":
        proof["publication"] = None
    if change == "missing-acquisition":
        del proof["acquisition"]
    monkeypatch.setattr("research_repo_tools.release_assets.require_release_checkout", lambda *_a, **_kw: pytest.fail("invalid dispatch reached checkout"))
    script = runpy.run_path(str(ROOT / "tooling/verify_registry_candidate.py"))
    with pytest.raises(ValueError, match=r"transfer|target|acquisition|publication|invalid.*run"):
        script["verify_registry_candidate"](tmp_path, tag if change != "tag" else "v0.3.0", SHA if change != "commit" else "b" * 40, transfer, proof)


@pytest.mark.parametrize(
    "change", ["none", "branch", "attempt", "boolean-attempt", "artifact", "release-id", "float-release-id", "missing-digest", "invalid-digest"]
)
def test_registry_dispatch_authenticates_published_event(change: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Use the published-event job contract and GitHub artifact shape, replacing only provider I/O."""
    tag = f"v{VERSION}"
    repo = {"id": 9, "full_name": REPOSITORY}
    workflow = {"id": 8, "path": ".github/workflows/publish-crates.yml", "name": "Publish to crates.io", "state": "active"}
    run = {
        "id": 17,
        "run_attempt": 2 if change == "attempt" else 1,
        "workflow_id": 8,
        "head_sha": SHA,
        "head_branch": "main" if change == "branch" else tag,
        "repository": repo,
        "head_repository": repo,
        "path": workflow["path"],
        "name": workflow["name"],
        "event": "release",
        "status": "completed",
        "conclusion": "success",
        "display_title": f"Verify published release {tag} (release 7)",
    }
    artifact = {
        "id": 22 if change == "artifact" else 21,
        "name": "published-release-17-1",
        "digest": "sha256:" + "a" * 64,
        "expired": False,
        "size_in_bytes": 200,
        "workflow_run": {"id": 17, "head_sha": SHA, "head_branch": tag, "repository_id": 9, "head_repository_id": 9},
    }
    events = yaml.safe_load((ROOT / ".github/workflows/publish-crates.yml").read_bytes())["jobs"]["published-release"]["steps"]
    job = {
        "id": 19,
        "name": "published-release",
        "run_id": 17,
        "run_attempt": 1,
        "head_sha": SHA,
        "head_branch": tag,
        "status": "completed",
        "conclusion": "success",
        "labels": ["ubuntu-latest"],
        "steps": [{"name": step["name"], "status": "completed", "conclusion": "success"} for step in events if "name" in step],
    }
    provider = {
        "actions/workflows/publish-crates.yml": workflow,
        "actions/runs/17": run,
        "actions/artifacts/21": artifact,
        "actions/runs/17/attempts/1/jobs?per_page=100&page=1": {"jobs": [job], "total_count": 1},
    }
    receipt: dict[str, object] = {
        "repository": REPOSITORY,
        "tag": tag,
        "commit": SHA,
        "release_id": 8 if change == "release-id" else 7,
        "run": "17",
        "attempt": "1",
        "crate_sha256": "c" * 64,
        "acceptance_sha256": "A" * 64 if change == "invalid-digest" else "a" * 64,
    }
    if change == "missing-digest":
        del receipt["crate_sha256"]
    if change == "float-release-id":
        receipt["release_id"] = 7.0
    if change == "boolean-attempt":
        run["run_attempt"] = True

    def download(repository: str, identity: dict[str, Any], work: Path, names: tuple[str, ...], **_options: int) -> None:
        assert repository == REPOSITORY
        assert identity["id"] == 21
        assert names == ("published-release.json",)
        work.mkdir()
        (work / names[0]).write_bytes(json.dumps(receipt).encode())

    monkeypatch.setattr("research_repo_tools.release_validation.api", lambda endpoint, **_kw: provider[endpoint])
    monkeypatch.setattr("research_repo_tools.release_validation.download_artifact", download)
    script = runpy.run_path(str(ROOT / "tooling/verify_registry_candidate.py"))
    verify = script["published_event"]
    proof = script["RegistryProof"].parse(registry_proof(b"transfer"))
    if change == "none":
        assert verify(proof, tag, SHA, 7, tmp_path) == receipt
    else:
        with pytest.raises(ValueError, match=r"selection|artifact|receipt"):
            verify(proof, tag, SHA, 7, tmp_path)


def test_registry_inventory_rechecks_upload_states_in_any_order() -> None:
    check = runpy.run_path(str(ROOT / "tooling/verify_registry_candidate.py"))["require_uploaded_inventory"]
    crate = (CRATE, 21, 500, "c" * 64)
    policy = ("policy.json", 22, 300, "d" * 64)
    inventory = {name: {"id": identifier, "size": size, "sha256": digest} for name, identifier, size, digest in (crate, policy)}
    release = GitHubRelease(
        REPOSITORY, f"v{VERSION}", 7, draft=False, immutable=True, prerelease=False, assets=(policy, crate), asset_states=((21, "uploaded"), (22, "uploaded"))
    )
    check(release, inventory)
    check(replace(release, assets=(crate, policy)), inventory)
    with pytest.raises(ValueError, match="incomplete release asset"):
        check(replace(release, asset_states=((21, "uploaded"), (22, "starter"))), inventory)
    with pytest.raises(ValueError, match="inventory differs"):
        check(replace(release, assets=(crate,)), inventory)
    with pytest.raises(ValueError, match="inventory differs"):
        check(release, {**inventory, CRATE: {**inventory[CRATE], "id": 21.0}})
    for size, digest in ((0, "d" * 64), (300, None)):
        incomplete = replace(release, assets=(crate, ("policy.json", 22, size, digest)))
        with pytest.raises(ValueError, match="empty or undigested"):
            check(incomplete, {**inventory, "policy.json": {"id": 22, "size": size, "sha256": digest}})


def make_crate(destination: Path, source: bytes) -> None:
    """Use real Cargo manifest and VCS member paths in a bounded tarball."""
    with tarfile.open(destination, "w:gz") as archive:
        for name, content in {
            ".cargo_vcs_info.json": json.dumps({"git": {"sha1": SHA}, "path_in_vcs": ""}).encode(),
            "Cargo.toml": f'[package]\nname = "causal-triangulations"\nversion = "{VERSION}"\n'.encode(),
            "src/lib.rs": source,
        }.items():
            member = tarfile.TarInfo(f"causal-triangulations-{VERSION}/{name}")
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))


@pytest.mark.parametrize("change", ["none", "attempt", "slow", "skipped", "cancelled", "failure", "signature", "archive", "expired", "artifact-id"])
def test_native_selection_requires_original_complete_signed_attempt(change: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Only GitHub transport and cryptographic tooling are replaced in the installed verifier."""
    run: dict[str, Any] = {
        "id": 17,
        "run_attempt": 1,
        "head_sha": SHA,
        "head_branch": "main",
        "repository": {"id": 9, "full_name": REPOSITORY},
        "head_repository": {"id": 9, "full_name": REPOSITORY},
        "event": "push",
        "name": "CI",
        "path": ".github/workflows/ci.yml",
        "workflow_id": 8,
        "status": "completed",
        "conclusion": "success",
    }
    assert POLICY.signing_job is not None
    jobs: list[dict[str, Any]] = [
        {
            "id": index,
            "name": name,
            "run_id": 17,
            "run_attempt": 1,
            "head_sha": SHA,
            "head_branch": "main",
            "status": "completed",
            "conclusion": "success",
            "labels": list(dict(POLICY.validation.runner_labels).get(name, ())),
            "steps": [{"name": step, "status": "completed", "conclusion": "success"} for step in steps],
        }
        for index, (name, steps) in enumerate((*POLICY.validation.required_steps, POLICY.signing_job), 1)
    ]
    artifact = {
        "id": 21,
        "name": "cargo-package-17-1",
        "size_in_bytes": 512,
        "digest": "sha256:" + "c" * 64,
        "expired": False,
        "workflow_run": {"id": 17, "head_sha": SHA, "head_branch": "main", "repository_id": 9, "head_repository_id": 9},
    }

    def read(endpoint: str) -> dict[str, Any]:
        if endpoint == "actions/workflows/ci.yml":
            return {"id": 8, "name": "CI", "path": ".github/workflows/ci.yml", "state": "active"}
        if endpoint == "actions/runs/17":
            return run
        if endpoint.startswith("actions/workflows/8/runs?"):
            return {"workflow_runs": [run], "total_count": 1}
        if "/jobs?" in endpoint:
            return {"jobs": jobs, "total_count": len(jobs)}
        if "/artifacts?" in endpoint:
            return {"artifacts": [artifact], "total_count": 1}
        assert endpoint == "actions/artifacts/21"
        return artifact

    crate = tmp_path / CRATE
    make_crate(crate, b"pub const COUNT: usize = 1;")
    changed_crate = tmp_path / "replacement.crate"
    make_crate(changed_crate, b"pub const COUNT: usize = 2;")
    manifest = release_evidence.create_evidence_manifest(
        POLICY, VERSION, SHA, 17, 1, read, archive=crate, tools={"cargo": "cargo 1.100", "rustc": "rustc 1.100"}
    )
    modified = False

    def download(repo: str, identity: dict[str, Any], destination: Path, names: tuple[str, ...], **_limits: int) -> None:
        assert (repo, identity["id"], set(names)) == (REPOSITORY, 21, {"validation.json", "validation.sigstore.json", CRATE})
        destination.mkdir()
        (destination / "validation.json").write_bytes(manifest)
        (destination / "validation.sigstore.json").write_bytes(b"invalid" if modified and change == "signature" else b"signed")
        (destination / CRATE).write_bytes((changed_crate if modified and change == "archive" else crate).read_bytes())

    def attest(path: Path, bundle: Path, *identity: object, repository: str) -> None:
        assert (path.read_bytes(), repository) == (manifest, REPOSITORY)
        assert identity == (SHA, ".github/workflows/ci.yml", "refs/heads/main", 17, 1)
        if bundle.read_bytes() != b"signed":
            message = "invalid signature"
            raise ValueError(message)

    monkeypatch.setattr(release_evidence, "download_artifact", download)
    monkeypatch.setattr(release_evidence, "verify_attestation", attest)
    original = release_evidence.verify_release_evidence(POLICY, VERSION, SHA, read)
    assert json.loads(original)["artifact"]["id"] == 21
    modified = True
    if change == "attempt":
        run["run_attempt"] = 2
    elif change == "slow":
        next(step for job in jobs for step in job["steps"] if step["name"] == "Run slow scientific tests")["conclusion"] = "skipped"
    elif change in {"skipped", "cancelled", "failure"}:
        next(job for job in jobs if job["name"] == "repository-tooling")["conclusion"] = change
    elif change == "expired":
        artifact["expired"] = True
    elif change == "artifact-id":
        artifact["id"] = 22
    if change == "none":
        assert release_evidence.verify_release_evidence(POLICY, VERSION, SHA, read, expected=original) == original
    else:
        with pytest.raises(ValueError, match=r"attempt|job|step|signed|signature|evidence|artifact|expired"):
            release_evidence.verify_release_evidence(POLICY, VERSION, SHA, read, expected=original)


def test_policy_names_match_workflow_coverage() -> None:
    """Require the actual CI jobs, named steps, runner labels and checkout-free signer."""
    ci = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8"))["jobs"]
    matrix = ci["build"]["strategy"]["matrix"]["os"]
    assert {name for name, _ in POLICY.validation.required_steps} == {"repository-tooling", "package", *(f"build ({os})" for os in matrix)}
    for name, required in POLICY.validation.required_steps:
        job = ci["build"] if name.startswith("build (") else ci[name]
        assert set(required) <= {step.get("name") for step in job["steps"]}
    assert ci["package"]["needs"] == "build"
    assert ci["build"]["needs"] == "repository-tooling"
    assert ci["build"]["if"] == "${{ always() }}"
    guard = ci["build"]["steps"][0]
    assert guard["if"] == "${{ cancelled() || needs.repository-tooling.result != 'success' }}"
    assert guard["run"] == "exit 1"
    assert ci["sign-package"]["needs"] == "package"
    assert not any("checkout" in step.get("uses", "") for step in ci["sign-package"]["steps"])
    assert ci["sign-package"]["permissions"]["id-token"] == "write"
    assert all("id-token" not in ci[name].get("permissions", {}) for name in ("build", "package"))


def test_draft_receipt_binds_independent_native_selection(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A valid target for the right commit can still reflect an unreviewed CI rerun."""
    tag = f"v{VERSION}"
    target = ReleaseTarget(REPOSITORY, tag, 66, SHA, tag)
    selected = hashlib.sha256(b"original native selection").hexdigest()
    receipt = DraftReceipt(target, b"draft selection", serialize_release_target(target), json.dumps({"selection_sha256": selected}).encode())
    monkeypatch.setattr("research_repo_tools.release_receipts.retrieve_draft_receipt", lambda *_args, **_kwargs: receipt)
    args = [
        "--root",
        str(ROOT),
        "release",
        "assets",
        "retrieve",
        "tooling/draft-receipt.toml",
        "--tag",
        tag,
        "--commit",
        SHA,
        "--output",
        str(tmp_path / "draft"),
        "--expected-native-sha256",
    ]
    assert main([*args, "0" * 64]) == 1
    assert not (tmp_path / "draft").exists()
    assert main([*args, selected]) == 0
    assert (tmp_path / "draft/target.json").read_bytes() == serialize_release_target(target)


def test_review_and_publish_share_evidence_and_stop_on_failure(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Exercise the operator recipes. The strict CLI double fails on any build, measurement or unexpected command."""
    tag = f"v{VERSION}"
    owner = tmp_path / "trusted owner"
    state = owner / "final" / SHA
    (state / "package").mkdir(parents=True)
    (state / "reviewed").mkdir()
    (state / "draft").mkdir()
    for filename in ("acquisition.sha256", "publication.sha256"):
        (owner / filename).write_bytes(b"f" * 64)
    policy = json.dumps({"tag": tag, "commit": SHA}).encode()
    for name, content in (("policy.json", policy), ("acceptance.json", b"original"), (CRATE, b"signed")):
        (state / "reviewed" / name).write_bytes(content)
        ((state / "package" if name == CRATE else state) / name).write_bytes(content)
    (state / "selection.json").write_bytes(b"native")
    (state / "transfer.json").write_bytes(b'{"target":{},"assets":{}}')
    log = tmp_path / "commands.log"
    stub = tmp_path / "release-tool"
    stub.write_text(
        "#!/usr/bin/env bash\nset -euo pipefail\n"
        'printf "%s\\n" "$*" >> "$TEST_LOG"\n'
        'case "$1 $2 ${3-}" in\n'
        '  "release evidence candidate") [[ " $* " == *" --acceptance $TEST_STATE/acceptance.json "* ]] ;;\n'
        '  "release assets retrieve") [[ " $* " == *" --expected-native-sha256 $TEST_NATIVE "* ]] ;;\n'
        '  "release assets receive"|"release evidence archive") ;;\n'
        '  "release publish v0.2.0") [[ "${!#}" == "$TEST_REVIEWED" ]] ;;\n'
        "  *) exit 41 ;;\nesac\n",
        encoding="utf-8",
        newline="\n",
    )
    monkeypatch.setenv("CDT_RELEASE_OWNER", owner.as_posix())
    monkeypatch.setenv("TEST_LOG", log.as_posix())
    monkeypatch.setenv("TEST_STATE", state.as_posix())
    monkeypatch.setenv("TEST_NATIVE", hashlib.sha256(b"native").hexdigest())
    monkeypatch.setenv("TEST_REVIEWED", hashlib.sha256((state / "transfer.json").read_bytes()).hexdigest())
    args = ["--set", "_release_cargo", "", "--set", "_release_tools", "bash " + shlex.quote(stub.as_posix()), "--set", "_commit", SHA]
    combined = ["release-review", tag, "release-publish", tag]
    result = run_safe_command("just", [*args, *combined], cwd=ROOT, check=False)
    assert result.returncode == 0, result.stderr
    commands = log.read_text(encoding="utf-8").splitlines()
    assert [line.split()[:3] for line in commands] == [
        ["release", "evidence", "candidate"],
        ["release", "assets", "retrieve"],
        ["release", "assets", "receive"],
        ["release", "evidence", "archive"],
        ["release", "publish", tag],
    ]
    assert (state / "reviewed.sha256").read_text(encoding="utf-8").strip() == os.environ["TEST_REVIEWED"]
    log.write_bytes(b"")
    (state / "reviewed/acceptance.json").write_bytes(b"other")
    assert run_safe_command("just", [*args, *combined], cwd=ROOT, check=False).returncode != 0
    assert "release publish" not in log.read_text(encoding="utf-8")
    log.write_bytes(b"")
    assert run_safe_command("just", [*args, "release-publish", "v0.2.1"], cwd=ROOT, check=False).returncode != 0
    assert "release publish" not in log.read_text(encoding="utf-8")


def test_pre_tag_inspection_freezes_one_selection_and_reuses_it(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Only the candidate verifier and an explicitly requested tag preview can run at this boundary."""
    owner = tmp_path / "review owner"
    state = owner / "final" / SHA
    state.mkdir(parents=True)
    for entry in ("acquisition.sha256", "publication.sha256"):
        (owner / entry).write_bytes(b"a" * 64)
    (state / "policy.json").write_bytes(json.dumps({"tag": f"v{VERSION}", "commit": SHA}).encode())
    trace = tmp_path / "verified"
    stub = tmp_path / "verify.sh"
    stub.write_text(
        "set -eu\n"
        'case "$1 $2 ${3-}" in\n'
        '  "release evidence candidate")\n'
        "    shift 3; accepted=; output=\n"
        '    while (($#)); do case "$1" in --acceptance) accepted=$2; shift;; --output) output=$2; shift;; esac; shift; done\n'
        '    if [[ -z "$accepted" ]]; then [[ "$output" == "$STATE/acceptance.json" && ! -e "$output" ]];\n'
        '    else [[ "$accepted" == "$STATE/acceptance.json" && "$(cat "$accepted")" == original && "$output" == "$STATE/revalidated.json" ]]; fi\n'
        '    printf original >"$output"; echo verified >>"$TRACE";;\n'
        '  "changelog tag v0.2.0") [[ "$4" == --commit && "$5" == "$SHA" && "$6" == --dry-run ]]; echo preview >>"$TRACE";;\n'
        "  *) exit 41;;\nesac\n",
        encoding="utf-8",
        newline="\n",
    )
    for key, value in {"CDT_RELEASE_OWNER": owner.as_posix(), "STATE": state.as_posix(), "TRACE": trace.as_posix(), "SHA": SHA}.items():
        monkeypatch.setenv(key, value)
    args = ["--set", "_release_cargo", "", "--set", "_release_tools", "bash " + shlex.quote(stub.as_posix()), "--set", "_commit", SHA]
    for invocation in (["release-evidence"], ["release-evidence"], ["tag", f"v{VERSION}", "--commit", SHA, "--dry-run"]):
        result = run_safe_command("just", [*args, *invocation], cwd=ROOT, check=False)
        assert result.returncode == 0, result.stderr
    assert (state / "acceptance.json").read_bytes() == (state / "revalidated.json").read_bytes() == b"original"
    assert trace.read_bytes().splitlines() == [b"verified", b"verified", b"verified", b"preview"]
