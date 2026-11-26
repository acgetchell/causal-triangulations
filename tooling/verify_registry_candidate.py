"""Authenticate CDT's published-event handoff and its four reviewed assets before Cargo upload."""

import os
import re
from dataclasses import dataclass
from functools import partial
from hashlib import file_digest
from pathlib import Path
from typing import TYPE_CHECKING, Any

from research_repo_tools.candidate_reuse import parse_candidate_policy, verify_candidate_archive, verify_candidate_release
from research_repo_tools.config import load
from research_repo_tools.evidence import deterministic_json, sha256
from research_repo_tools.release_assets import download_release_asset, parse_release_target, require_release_checkout, serialize_release_target
from research_repo_tools.release_interfaces import load_evidence_policy
from research_repo_tools.release_publishing import check_reviewed_release
from research_repo_tools.release_validation import ValidationPolicy, api, artifact_identity, download_artifact, parse_release_json, read_pages, validate_jobs

if TYPE_CHECKING:
    from research_repo_tools.release_assets import GitHubRelease

REPOSITORY = "acgetchell/causal-triangulations"
PACKAGE = "causal-triangulations"
WORKFLOW = ".github/workflows/publish-crates.yml"
EVENT_RECEIPT_LIMIT = 65_536
EVENT_GATE = ValidationPolicy(
    REPOSITORY,
    WORKFLOW,
    "Publish to crates.io",
    (
        (
            "published-release",
            (
                "Verify the published event, source, required checks and crate asset",
                "Reproduce the signed crate without building or uploading",
                "Retain the published release identity for registry approval",
            ),
        ),
    ),
    runner_labels=(("published-release", ("ubuntu-latest",)),),
)


def require(condition: bool, message: str) -> None:
    """Reject inconsistent provider or operator evidence before requesting a token."""
    if not condition:
        raise ValueError(message)


@dataclass(frozen=True, slots=True)
class RegistryProof:
    """Operator-reviewed identifiers and SHA-256 digests supplied by release-upload."""

    run: int
    attempt: int
    artifact: int
    digest: str
    transfer: str
    acceptance: str
    acquisition: str
    publication: str

    @classmethod
    def parse(cls, value: object) -> RegistryProof:
        """Validate the complete dispatch before contacting the provider."""
        if not isinstance(value, dict):
            message = "registry proof must be an object"
            raise TypeError(message)

        def identifier(field: str) -> int:
            item = value.get(field)
            if type(item) is not int or item <= 0:
                raise ValueError(f"invalid registry proof {field} ID")
            return item

        def digest(field: str, prefix: str = "") -> str:
            item = value.get(field)
            if not isinstance(item, str) or re.fullmatch(prefix + r"[0-9a-f]{64}", item) is None:
                raise ValueError(f"invalid registry proof {field} digest")
            return item

        return cls(
            run=identifier("run"),
            attempt=identifier("attempt"),
            artifact=identifier("artifact"),
            digest=digest("digest", "sha256:"),
            transfer=digest("transfer"),
            acceptance=digest("acceptance"),
            acquisition=digest("acquisition"),
            publication=digest("publication"),
        )


def matches_fields(record: object, expected: dict[str, Any]) -> bool:
    """Match JSON scalars without equating integer IDs with booleans or floats."""
    return isinstance(record, dict) and all(type(record.get(key)) is type(value) and record[key] == value for key, value in expected.items())


def published_run(proof: RegistryProof, tag: str, commit: str, release_id: int) -> dict[str, Any]:
    """Require the same successful release, workflow, repository and attempt on each read."""
    read = partial(api, repository=REPOSITORY)
    workflow = read("actions/workflows/publish-crates.yml")
    run = read(f"actions/runs/{proof.run}")
    expected = {
        "id": proof.run,
        "run_attempt": proof.attempt,
        "workflow_id": workflow["id"],
        "name": EVENT_GATE.workflow_name,
        "display_title": f"Verify published release {tag} (release {release_id})",
        "event": "release",
        "head_branch": tag,
        "head_sha": commit,
        "status": "completed",
        "conclusion": "success",
    }
    require(type(workflow["id"]) is int and workflow["id"] > 0, "invalid publisher workflow ID")
    require(matches_fields(workflow, {"path": WORKFLOW, "state": "active"}), "unexpected publisher workflow")
    require(matches_fields(run, expected), "published event selection changed")
    require(run.get("path") in (WORKFLOW, f"{WORKFLOW}@{tag}", f"{WORKFLOW}@refs/tags/{tag}"), "published event workflow selection changed")
    require(all(run[key]["full_name"] == REPOSITORY for key in ("repository", "head_repository")), "unexpected event repository")
    return run


def published_artifact(proof: RegistryProof, run: dict[str, Any]) -> dict[str, Any]:
    """Bind the artifact from the selected attempt to the operator-reviewed digest."""
    data = api(f"actions/artifacts/{proof.artifact}", repository=REPOSITORY)
    identity = artifact_identity(data, run, f"published-release-{proof.run}-{proof.attempt}", max_bytes=EVENT_RECEIPT_LIMIT)
    require((identity["id"], identity["digest"]) == (proof.artifact, proof.digest), "published-event artifact changed")
    return identity


def published_event(proof: RegistryProof, tag: str, commit: str, release_id: int, work: Path) -> dict[str, Any]:
    """Download the receipt from the independently selected successful release event."""
    read = partial(api, repository=REPOSITORY)
    run_id, attempt = proof.run, proof.attempt
    run = published_run(proof, tag, commit, release_id)
    jobs = read_pages(read, f"actions/runs/{run_id}/attempts/{attempt}/jobs", "jobs")
    validate_jobs(EVENT_GATE, run, jobs)
    identity = published_artifact(proof, run)
    receipt_dir = work / "event"
    download_artifact(REPOSITORY, identity, receipt_dir, ("published-release.json",), max_bytes=EVENT_RECEIPT_LIMIT)
    event = parse_release_json((receipt_dir / "published-release.json").read_bytes())
    expected_receipt = {"repository": REPOSITORY, "tag": tag, "commit": commit, "release_id": release_id, "run": str(run_id), "attempt": str(attempt)}
    require(matches_fields(event, expected_receipt), "published release receipt changed")
    for field in ("crate_sha256", "acceptance_sha256"):
        digest = event.get(field)
        require(isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest) is not None, f"invalid published release receipt {field}")
    return event


def require_uploaded_inventory(release: GitHubRelease, staged: dict[str, Any]) -> None:
    """Match the reviewed IDs and bytes regardless of provider ordering, with every upload complete."""
    require(all(size > 0 and digest is not None for _, _, size, digest in release.assets), "empty or undigested release asset")
    observed = {name: {"id": identifier, "size": size, "sha256": digest} for name, identifier, size, digest in release.assets}
    require(deterministic_json(observed) == deterministic_json(staged), "published asset inventory differs from review")
    require(dict(release.asset_states) == {identifier: "uploaded" for _, identifier, _, _ in release.assets}, "incomplete release asset")


def verify_registry_candidate(work: Path, tag: str, commit: str, transfer: bytes, dispatch: object) -> None:
    """Recheck the original CDT release, benchmark, signed Cargo selection and reproduced archive."""
    root = Path.cwd()
    proof = RegistryProof.parse(dispatch)
    require(sha256(transfer) == proof.transfer, "staging transfer differs from local review")
    batch = parse_release_json(transfer)
    target = parse_release_target(deterministic_json(batch.get("target")))
    require((target.repository, target.tag, target.commit) == (REPOSITORY, tag, commit), "wrong release target")
    require(batch.get("state") == "complete" and batch.get("schema") == "research-repo-tools/release-asset-batch/v1", "incomplete staging transfer")
    crate = f"{PACKAGE}-{tag.removeprefix('v')}.crate"
    benchmark = f"{PACKAGE}-{tag}-cdt-series.tar.gz"
    names = {"acceptance.json", "policy.json", crate, benchmark}
    assets = batch.get("assets")
    if not isinstance(assets, dict) or assets.keys() != names:
        message = "unexpected staging transfer asset inventory"
        raise ValueError(message)
    require_release_checkout(root, commit)
    event = published_event(proof, tag, commit, target.release_id, work)
    config = load(root=root)
    release = check_reviewed_release(config, tag, target=target, require_crate_asset=True)
    require_uploaded_inventory(release, assets)
    require(event["acceptance_sha256"] == proof.acceptance == assets["acceptance.json"]["sha256"], "original acceptance differs")
    require(event["crate_sha256"] == assets[crate]["sha256"], "signed event crate differs")
    for name in ("acceptance.json", "policy.json", benchmark):
        download_release_asset(root, REPOSITORY, tag, name, work / name, expected_sha256=assets[name]["sha256"])
    accepted = (work / "acceptance.json").read_bytes()
    policy = parse_candidate_policy((work / "policy.json").read_bytes())
    evidence = verify_candidate_archive(work / benchmark, policy, accepted, target=target)
    verified = verify_candidate_release(
        root,
        evidence,
        policy,
        load_evidence_policy(root / "tooling/release-evidence.toml"),
        partial(api, repository=REPOSITORY),
        acquisition_sha256=proof.acquisition,
        publication_sha256=proof.publication,
        publication_receipt=deterministic_json(parse_release_json(accepted)["binding"]["publication"]),
        expected=accepted,
        reproduced_archive=work / "package" / crate,
        reproduced_tools=parse_release_json((work / "tools.json").read_bytes()),
        dependencies=config.release.published_dependencies,
    )
    with (work / "package" / crate).open("rb") as stream:
        require(verified == accepted and file_digest(stream, "sha256").hexdigest() == event["crate_sha256"], "Cargo reproduction differs from review")
    require_uploaded_inventory(check_reviewed_release(config, tag, target=target, require_crate_asset=True), assets)
    current = published_run(proof, tag, commit, target.release_id)
    published_artifact(proof, current)
    (work / "target.json").write_bytes(serialize_release_target(target))


if __name__ == "__main__":
    verify_registry_candidate(
        Path(os.environ["REGISTRY_WORK"]),
        os.environ["RELEASE_TAG"],
        os.environ["RELEASE_COMMIT"],
        os.environ["RELEASE_TRANSFER"].encode("utf-8"),
        parse_release_json(os.environ["RELEASE_PROOF"]),
    )
