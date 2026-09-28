"""Consumer contracts for the pinned maintenance CLI and CDT command wiring."""

import json
import re
import tomllib
from importlib.metadata import version
from pathlib import Path

import pytest
import yaml
from research_repo_tools.cli import main
from research_repo_tools.criterion import COMPARISON_SCHEMA, Estimate, Sample, compare_samples, serialize_comparison
from research_repo_tools.evidence import Evidence, Provenance, publish_evidence
from research_repo_tools.measurement import load_measurement
from research_repo_tools.process import run_safe_command

ROOT = Path(__file__).resolve().parents[2]


def test_dependabot_caller_only_delegates_with_complete_cdt_file_policy() -> None:
    """The privileged trigger delegates only to the pinned, token-free shared workflow."""
    workflow = yaml.safe_load((ROOT / ".github/workflows/dependabot-auto-merge.yml").read_text(encoding="utf-8"))
    events = workflow.get("on", workflow.get(True))  # PyYAML treats unquoted on as a YAML 1.1 boolean.
    assert set(events) == {"pull_request_target"}
    assert events["pull_request_target"]["branches"] == ["main"]
    assert workflow["permissions"] == {}
    assert set(workflow["jobs"]) == {"approve-and-enable-auto-merge"}
    job = workflow["jobs"]["approve-and-enable-auto-merge"]
    assert set(job) == {"uses", "permissions", "with"}
    assert re.fullmatch(r"acgetchell/research-repo-tools/\.github/workflows/dependabot-approve\.yml@[0-9a-f]{40}", job["uses"])
    assert job["permissions"] == {"contents": "write", "pull-requests": "write"}
    assert set(job["with"]) == {"repository", "policy"}
    assert job["with"]["repository"] == "acgetchell/causal-triangulations"
    policy = json.loads(job["with"]["policy"])
    assert set(policy) == {"cargo", "uv", "github_actions"}
    assert set(policy["cargo"]["files"]) == {"Cargo.toml", "Cargo.lock"}
    assert set(policy["uv"]["files"]) == {"pyproject.toml", "uv.lock"}
    action_files = {
        path.relative_to(ROOT).as_posix()
        for directory in (ROOT / ".github/workflows", ROOT / ".github/actions")
        for path in directory.rglob("*")
        if path.suffix in {".yml", ".yaml"}
    }
    assert set(policy["github_actions"]["files"]) == action_files


def test_published_tooling_pin_and_consumer_metadata() -> None:
    """Exercise the installed package against the real release configuration."""
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    assert project["dependency-groups"]["tooling"] == [f"research-repo-tools=={version('research-repo-tools')}"]
    assert main(["--root", str(ROOT), "release", "check", "--final-release"]) == 0


def test_python_fixtures_are_in_the_ci_gate() -> None:
    """A new negative fixture must receive the complete configured lint policy."""
    result = run_safe_command("just", ["--dry-run", "ci"], cwd=ROOT)
    commands = result.stderr
    for command in ("ruff check --no-fix --no-force-exclude", "ruff format --check --no-force-exclude", "ty check --no-force-exclude --error all"):
        assert f"files run --include 'tests/semgrep/**/*.py' -- {command}" in commands
    assert "--select" not in commands
    assert "review branch" not in commands
    assert "review uncommitted" not in commands


def test_update_order_and_independent_scopes() -> None:
    """The aggregate updates tools first; component commands retain their scope."""
    aggregate = run_safe_command("just", ["--dry-run", "update"], cwd=ROOT).stderr
    assert aggregate.index("deps update-uv") < aggregate.index("toolchain upgrade") < aggregate.index("cargo upgrade")
    assert aggregate.index("cargo update") < aggregate.index("deps update-python") < aggregate.index("uv lock --upgrade")
    dependencies = run_safe_command("just", ["--dry-run", "update-dependencies"], cwd=ROOT).stderr
    assert "toolchain upgrade" not in dependencies
    assert "deps update-uv" not in dependencies
    tools = run_safe_command("just", ["--dry-run", "update-tools"], cwd=ROOT).stderr
    assert "cargo upgrade" not in tools
    assert "deps update-python" not in tools
    assert "uv lock --upgrade" not in tools
    assert "uv sync --locked --managed-python --group dev" in dependencies


def test_review_recipes_forward_arguments() -> None:
    """Just preserves the base argument and keeps uncommitted review independent."""
    branch = run_safe_command("just", ["--dry-run", "review"], cwd=ROOT)
    assert 'review branch --base="$1"' in branch.stderr
    uncommitted = run_safe_command("just", ["--dry-run", "review-uncommitted"], cwd=ROOT)
    assert "review uncommitted" in uncommitted.stderr


def test_changelog_recipe_forwards_explicit_release_date() -> None:
    """CDT's release recipe forwards the supplied tag and date to the shared CLI."""
    result = run_safe_command("just", ["--dry-run", "changelog-release", "v0.2.0", "2026-10-01"], cwd=ROOT)
    assert '--tag "$1" --date "$2"' in result.stderr


def test_example_contract_covers_every_cargo_example() -> None:
    """Adding a Cargo example requires an explicit output contract."""
    configuration = tomllib.loads((ROOT / "tooling/examples.toml").read_text(encoding="utf-8"))
    actual = {path.stem for path in (ROOT / "examples").glob("*.rs")}
    actual.update(path.parent.name for path in (ROOT / "examples").glob("*/main.rs"))
    assert {check["name"] for check in configuration["checks"]} == actual
    assert all(check["expect"] and check["timeout"] == 600 for check in configuration["checks"])


def test_benchmark_configuration_uses_current_correctness_gates() -> None:
    """The shared measurement config binds the actual CDT workload and dependencies."""
    config = load_measurement(ROOT, "tooling/benchmark.toml")
    assert config.command == ("just", "bench-latest")
    commands = run_safe_command("just", ["--dry-run", "bench-latest"], cwd=ROOT).stderr
    assert commands.index("--test physics_integration") < commands.index("--bench ci_performance_suite")
    assert dict(config.dependencies).keys() == {"criterion", "delaunay", "la-stack", "markov-chain-monte-carlo"}


def test_shared_report_and_readme_publication_use_consumer_configuration(tmp_path: Path) -> None:
    """New evidence survives promotion and publishes the configured absolute table."""
    tooling = tmp_path / "tooling"
    tooling.mkdir()
    for name in ("performance-report.toml", "performance-interpretation.md"):
        (tooling / name).write_bytes((ROOT / "tooling" / name).read_bytes())
    template = (ROOT / "tooling/performance-readme.example.toml").read_text(encoding="utf-8")
    config = tomllib.loads(template)
    baseline = Sample(tuple((row["benchmark"], Estimate(100, 90, 110, 0.95)) for row in config["rows"]), unit=config["unit"])
    current = Sample(tuple((row["benchmark"], Estimate(80, 70, 90, 0.95)) for row in config["rows"]), unit=config["unit"])
    evidence = Evidence(
        serialize_comparison(compare_samples(baseline, current)),
        COMPARISON_SCHEMA,
        (
            ("baseline", Provenance("a" * 40, context=(("release", "v0.1.0"),))),
            ("current", Provenance("b" * 40, context=(("release", "v0.1.1"),))),
        ),
    )
    publish_evidence(evidence, tmp_path / "comparison.json", tmp_path / "evidence.json")
    (tmp_path / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.1.1"\n', encoding="utf-8")
    readme = tmp_path / "README.md"
    readme.write_text("# Fixture\n\n<!-- performance-summary:start -->\nPending.\n<!-- performance-summary:end -->\n", encoding="utf-8")
    assert (
        main(
            [
                "--root",
                str(tmp_path),
                "performance",
                "promote",
                "tooling/performance-report.toml",
                "--payload",
                "comparison.json",
                "--manifest",
                "evidence.json",
            ]
        )
        == 0
    )
    publication = template.replace("REPLACE-pair", "v0.1.1-vs-v0.1.0")
    for placeholder, value in (
        ("REPLACE_WITH_BASELINE_COMMIT", "a" * 40),
        ("REPLACE_WITH_CURRENT_COMMIT", "b" * 40),
        ("REPLACE_WITH_BASELINE_TAG", "v0.1.0"),
        ("REPLACE_WITH_CURRENT_TAG", "v0.1.1"),
    ):
        publication = publication.replace(placeholder, value)
    (tooling / "performance-readme.toml").write_text(publication, encoding="utf-8")
    args = ["--root", str(tmp_path), "performance", "publish", "tooling/performance-readme.toml"]
    assert main(args) == 0
    published = readme.read_text(encoding="utf-8")
    for expected in ("100 ns", "80 ns", "1.25", "20", "[90, 110] (0.95 confidence)", "[70, 90] (0.95 confidence)"):
        assert expected in published
    assert main([*args, "--check"]) == 0
    original = readme.read_bytes()
    (tooling / "performance-readme.toml").write_text(publication.replace("b" * 40, "c" * 40), encoding="utf-8")
    assert main(args) == 1
    assert readme.read_bytes() == original


@pytest.mark.parametrize("citation_change", ["wrong-doi", "version-identifiers"])
def test_release_policy_rejects_citation_drift(citation_change: str, tmp_path: Path) -> None:
    """The shared checker preserves CDT's permanent concept DOI contract."""
    for name in ("Cargo.toml", "Cargo.lock", "pyproject.toml", "uv.lock", "CITATION.cff", "CHANGELOG.md", "README.md"):
        (tmp_path / name).write_bytes((ROOT / name).read_bytes())
    citation = tmp_path / "CITATION.cff"
    content = citation.read_text(encoding="utf-8")
    if citation_change == "wrong-doi":
        content = content.replace("10.5281/zenodo.20513228", "10.5281/zenodo.99999999")
    else:
        content += '\nidentifiers:\n  - type: doi\n    value: "10.5281/zenodo.99999999"\n'
    citation.write_text(content, encoding="utf-8")
    assert main(["--root", str(tmp_path), "release", "check"]) == 1
