"""Require a common release across CDT's three CodeQL Action identities."""

import sys
import tomllib
from pathlib import Path

from research_repo_tools.action_policy import load_policy
from research_repo_tools.workflow_uses import references, workflow_paths

CODEQL = {"github/codeql-action/init", "github/codeql-action/analyze", "github/codeql-action/upload-sarif"}


def check_codeql_release(root: Path, inputs: list[str]) -> None:
    """Check the cross-subpath contract after RRT has validated each individual pin."""
    policy = load_policy(root / ".github/action-updates.toml")
    tags = {policy.targets.get(action) for action in CODEQL}
    if not CODEQL.issubset(policy.consistency) or len(tags) != 1 or tags & {None, "latest"}:
        message = ".github/action-updates.toml: all three CodeQL Actions must select one explicit release tag"
        raise ValueError(message)
    reviewed = tomllib.loads((root / ".github/codeql-release.toml").read_text(encoding="utf-8"))
    if tags != reviewed.keys():
        message = "The selected CodeQL tag must match the reviewed tag in .github/codeql-release.toml"
        raise ValueError(message)
    tag, commit = next(iter(reviewed.items()))
    for path in workflow_paths(root, inputs):
        for use in references(path, path.read_bytes().decode("utf-8-sig")):
            if use.action in CODEQL and use.ref is not None and use.ref.lower() != commit:
                raise ValueError(f"{use.location}: {use.action}@{use.ref} differs from the reviewed CodeQL release {tag}@{commit}")


if __name__ == "__main__":
    try:
        check_codeql_release(Path.cwd(), sys.argv[1:])
    except (OSError, ValueError) as error:
        sys.exit(str(error))
