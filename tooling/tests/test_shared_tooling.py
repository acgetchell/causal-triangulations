"""Consumer contracts for the pinned maintenance CLI and CDT command wiring."""

import json
import os
import re
import shlex
import tomllib
from importlib.metadata import version
from pathlib import Path

import pytest
import yaml
from research_repo_tools.cli import main
from research_repo_tools.criterion import COMPARISON_SCHEMA, Estimate, Sample, compare_samples, serialize_comparison
from research_repo_tools.evidence import Evidence, Provenance, publish_evidence
from research_repo_tools.just_inspect import dry_run, inspect_justfile
from research_repo_tools.measurement import MeasurementConfig, load_measurement
from research_repo_tools.process import run_safe_command
from research_repo_tools.selection import select_files

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
    assert set(policy["uv"]["files"]) == {"pyproject.toml", "uv.lock", "tooling/semgrep/pyproject.toml", "tooling/semgrep/uv.lock"}
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


def test_independent_scanner_contract_and_update_inventory() -> None:
    """Python adoption and ordinary updates preserve the scanner's supported environment."""
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    assert project["tool"]["research-repo-tools"]["deps"]["python-projects"] == ["tooling/semgrep"]
    assert all(not (isinstance(item, str) and item.startswith("semgrep")) for item in project["dependency-groups"]["dev"])
    scanner = ROOT / "tooling/semgrep"
    manifest = tomllib.loads((scanner / "pyproject.toml").read_text(encoding="utf-8"))
    assert manifest["project"]["requires-python"] == ">=3.14,<3.15"
    assert (scanner / ".python-version").read_text(encoding="utf-8").strip() == "3.14"
    assert manifest["project"]["dependencies"] == ["semgrep==1.182.1"]
    assert manifest["tool"]["uv"]["no-build-package"] == ["semgrep"]
    assert manifest["tool"]["uv"]["override-dependencies"] == ["mcp>=1.30.0,<2"]
    dependabot = yaml.safe_load((ROOT / ".github/dependabot.yml").read_text(encoding="utf-8"))
    updates = next(item for item in dependabot["updates"] if item["package-ecosystem"] == "uv")
    assert set(updates["directories"]) == {"/", "/tooling/semgrep"}
    for recipe in ("semgrep", "semgrep-test"):
        commands = dry_run(ROOT, recipe).stderr
        assert "--project tooling/semgrep uv run --locked --group dev research-repo-tools" in commands


def test_python_rules_cover_relocated_tooling_tests(tmp_path: Path) -> None:
    """Actual scanner path filters still reject violations beside the relocated tests."""
    directory = tmp_path / "tooling/tests"
    directory.mkdir(parents=True)
    (tmp_path / "semgrep.yaml").write_bytes((ROOT / "semgrep.yaml").read_bytes())
    (directory / "probe.py").write_text(
        "from pathlib import Path\n"
        "try:\n"
        '    Path("bad.txt").read_text()\n'
        "except Exception:\n"
        "    pass\n"
        "try:\n"
        '    Path("good.txt").read_text(encoding="utf-8")\n'
        "except OSError:\n"
        "    pass\n",
        encoding="utf-8",
    )
    result = run_safe_command(
        "uv",
        [
            "run",
            "--locked",
            "--managed-python",
            "--project",
            str(ROOT / "tooling/semgrep"),
            "semgrep",
            "scan",
            "--config",
            "semgrep.yaml",
            "--json",
            "--quiet",
            "--strict",
            "--metrics",
            "off",
            "--disable-version-check",
            "--no-rewrite-rule-ids",
            "--no-git-ignore",
            "tooling/tests/probe.py",
        ],
        cwd=tmp_path,
    )
    findings = json.loads(result.stdout)
    assert not findings["errors"]
    assert findings["paths"]["scanned"], findings
    assert {(item["check_id"], item["start"]["line"]) for item in findings["results"]} == {
        ("causal-triangulations.python.no-broad-exception", 4),
        ("causal-triangulations.python.explicit-path-text-encoding-in-tests", 3),
    }


def test_semgrep_aggregates_the_complete_eligible_inventory(tmp_path: Path) -> None:
    """The actual recipe retains all scanned inputs and rules across native batches."""
    command = dry_run(ROOT, "semgrep").stderr.splitlines()[-1]
    arguments = shlex.split(command)
    include = tuple(arguments[index + 1] for index, item in enumerate(arguments) if item == "--include")
    exclude = tuple(arguments[index + 1] for index, item in enumerate(arguments) if item == "--exclude")
    names = select_files(ROOT, include=include, exclude=exclude)
    assert "tooling/tests/test_shared_tooling.py" in names
    reports = tmp_path / "reports"
    run_safe_command("just", ["semgrep", str(reports)], cwd=ROOT, timeout=300)
    data = json.loads((reports / "semgrep.json").read_text(encoding="utf-8"))
    assert len(data["batches"]) > 1, "The consumer regression must exercise aggregation."
    assert {Path(name).resolve() for name in data["paths"]["scanned"]} == {(ROOT / name).resolve() for name in names}
    assert not data["errors"]
    assert not data["results"]
    assert all(not batch["errors"] and not batch["results"] for batch in data["batches"])
    sarif = json.loads((reports / "semgrep.sarif").read_text(encoding="utf-8"))
    assert len(sarif["runs"]) == 1
    run = sarif["runs"][0]
    rules = yaml.safe_load((ROOT / "semgrep.yaml").read_text(encoding="utf-8"))["rules"]
    assert {rule["id"] for rule in run["tool"]["driver"]["rules"]} == {rule["id"] for rule in rules}
    assert run["automationDetails"]["id"] == "semgrep-repository-rules/"
    workflow = yaml.safe_load((ROOT / ".github/workflows/semgrep-sarif.yml").read_text(encoding="utf-8"))
    upload = next(step for step in workflow["jobs"]["semgrep-sarif"]["steps"] if "upload-sarif@" in step.get("uses", ""))
    assert upload["with"] == {
        "sarif_file": "target/security/semgrep/semgrep.sarif",
        "category": "semgrep-repository-rules",
        "wait-for-processing": True,
    }
    assert upload["with"]["sarif_file"] in upload["if"]


def test_registry_only_release_jobs_match_adopted_tooling() -> None:
    """Privileged jobs install reviewed registry tooling without running checked-out code."""
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    workflow = yaml.safe_load((ROOT / ".github/workflows/release-benchmarks.yml").read_text(encoding="utf-8"))
    cutoffs = set()
    for name in ("validate-release", "publish-baseline"):
        job = workflow["jobs"][name]
        assert not any("checkout" in step.get("uses", "") for step in job["steps"])
        setup = next(step for step in job["steps"] if "setup-uv@" in step.get("uses", ""))
        assert "==" + setup["with"]["version"] == project["tool"]["uv"]["required-version"]
        assert setup["with"]["enable-cache"] is False
        command = next(step["run"] for step in job["steps"] if "--from research-repo-tools==" in step.get("run", ""))
        assert "--from research-repo-tools==" + version("research-repo-tools") in command
        assert "--no-config --isolated --managed-python --python 3.15" in command
        cutoffs.add(command.split("--exclude-newer ", 1)[1].split()[0])
    assert len(cutoffs) == 1


def test_python_fixtures_are_in_the_ci_gate() -> None:
    """CI selects the complete shared policy once, including compatibility aliases."""
    result = dry_run(ROOT, "ci")
    commands = result.stderr
    assert commands.count("research-repo-tools python check") == 1
    metadata = inspect_justfile(ROOT)
    for alias in ("python-fixtures-check", "python-source-check"):
        assert metadata.aliases[alias] == "python-check"
    assert "tests/semgrep/scripts/tests/python_exceptions.py" in select_files(ROOT, include=("*.py", "*.pyi"))
    assert "--select" not in commands
    assert "review branch" not in commands
    assert "review uncommitted" not in commands


@pytest.mark.parametrize(
    ("reference", "allowed"),
    [
        ("acgetchell/research-repo-tools/.github/workflows/dependabot-approve.yml", True),
        ("acgetchell/research-repo-tools/.github/workflows/unapproved.yml", False),
        ("taiki-e/install-action", False),
        ("unapproved/example-action", False),
    ],
)
def test_actions_policy_preserves_exact_approved_identities(
    reference: str,
    allowed: bool,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The installed checker enforces CDT's policy, including the nested workflow boundary."""
    policy = tmp_path / "selected-actions.json"
    policy.write_bytes((ROOT / "tooling/selected-actions.json").read_bytes())
    workflow = tmp_path / "workflow.yml"
    workflow.write_text(f"jobs:\n  approval:\n    uses: {json.dumps(reference + '@' + 'a' * 40)}\n", encoding="utf-8")
    assert main(["--root", str(tmp_path), "actions", "allowlist", "--policy", str(policy), str(workflow)]) == (0 if allowed else 1)
    if not allowed:
        assert "unapproved reference" in capsys.readouterr().err


@pytest.mark.parametrize(
    ("recipe", "failure"),
    [
        ("ci", ""),
        ("ci", "actions allowlist"),
        ("ci", "python check"),
        ("ci", "notebooks lint"),
        ("ci", "cargo clippy"),
        ("notebook-check", ""),
        ("notebook-check", "notebooks lint"),
    ],
)
def test_validation_graph_orders_static_checks_and_stops_on_failure(
    recipe: str,
    failure: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Execute the real validation graph without running host validators or workloads."""
    stub_script = (
        "#!/bin/sh\n"
        'printf "%s\\n" "$*" >> "$VALIDATION_LOG"\n'
        'if [ -n "$VALIDATION_FAIL" ]; then\n'
        '  case "$*" in *"$VALIDATION_FAIL"*) echo "Injected validation failure: $VALIDATION_FAIL" >&2; exit 23;; esac\n'
        "fi\n"
    )
    for executable in ("uv", "uvx", "jq"):
        stub = tmp_path / executable
        stub.write_text(stub_script, encoding="utf-8", newline="\n")
        stub.chmod(0o700)
    (tmp_path / "justfile").write_bytes((ROOT / "justfile").read_bytes())
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    log = tmp_path / "validation.log"
    monkeypatch.setenv("VALIDATION_LOG", log.as_posix())
    monkeypatch.setenv("VALIDATION_FAIL", failure)
    result = run_safe_command("just", [recipe], cwd=tmp_path, check=False)
    assert result.returncode == (23 if failure else 0), result.stderr
    calls = log.read_text(encoding="utf-8").splitlines()
    assert sum("notebooks lint" in call for call in calls) == (0 if failure in {"actions allowlist", "python check"} else 1)
    workloads = ("python -m pytest", "cargo nextest run", "cargo test --doc", "notebooks execute", "cargo bench --profile perf", "validation run")
    runtime_positions = [index for index, call in enumerate(calls) if any(workload in call for workload in workloads)]
    if failure:
        assert f"Injected validation failure: {failure}" in result.stderr
        assert not runtime_positions, calls
    elif recipe == "ci":
        first_runtime = min(runtime_positions)
        for check in (
            "actions allowlist",
            "python check",
            "notebooks lint",
            "shellcheck -x",
            "semgrep scan",
            "cargo fmt",
            "cargo clippy",
            "cargo doc",
            "cargo bench --workspace --no-run",
        ):
            assert next(index for index, call in enumerate(calls) if check in call) < first_runtime, (check, calls)
    else:
        assert "notebooks lint" in calls[0]
        assert "notebooks execute" in calls[1]


def test_release_validation_coalesces_runnable_tests_in_commit_check() -> None:
    """Pre-commit retains debug/release doctests without replaying the release suite."""
    commit = dry_run(ROOT, "commit-check").stderr
    assert commit.count("cargo nextest run") == 1
    assert commit.index("cargo bench --workspace --no-run") < commit.index("cargo nextest run")
    assert "cargo test --doc --verbose" in commit
    assert "cargo test --doc --release" in commit
    release = dry_run(ROOT, "test-release").stderr
    assert release.count("cargo nextest run --release --profile ci --lib --tests --verbose") == 1
    assert release.index("cargo nextest run") < release.index("cargo test --doc --release")


UPDATE_STEPS = (
    "deps update-uv",
    "toolchain upgrade",
    "research-repo-tools setup",
    "cargo upgrade --incompatible allow",
    "cargo update",
    "deps update-python --all-projects",
)


@pytest.mark.parametrize(
    ("recipe", "steps", "failure"),
    [
        ("update", UPDATE_STEPS, ""),
        ("update-tools", UPDATE_STEPS[:3], ""),
        ("update-dependencies", UPDATE_STEPS[3:], ""),
        ("update-cargo-dependencies", UPDATE_STEPS[3:5], ""),
        ("update-python-dependencies", UPDATE_STEPS[5:], ""),
        ("update-python-deps", UPDATE_STEPS[5:], ""),
        *[("update", UPDATE_STEPS[: index + 1], step) for index, step in enumerate(UPDATE_STEPS)],
    ],
)
def test_update_execution_order_scope_and_failure(
    recipe: str,
    steps: tuple[str, ...],
    failure: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Execute the merged Just graph with an external uv stub, never host upgrades."""
    stub = tmp_path / "uv"
    stub.write_text(
        "#!/bin/sh\n"
        'printf "%s\\n" "$*" >> "$UPDATE_LOG"\n'
        'if [ -n "$UPDATE_FAIL" ]; then\n'
        '  case "$*" in *"$UPDATE_FAIL"*) echo "Injected updater failure: $UPDATE_FAIL" >&2; exit 23;; esac\n'
        "fi\n",
        encoding="utf-8",
        newline="\n",
    )
    stub.chmod(0o700)
    # Just uses the configured POSIX shell on Windows as well as Unix.
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    log = tmp_path / "update.log"
    monkeypatch.setenv("UPDATE_LOG", log.as_posix())
    monkeypatch.setenv("UPDATE_FAIL", failure)
    (tmp_path / "Cargo.toml").write_bytes((ROOT / "Cargo.toml").read_bytes())
    result = run_safe_command("just", ["--justfile", str(ROOT / "justfile"), "--working-directory", str(tmp_path), recipe], cwd=tmp_path, check=False)
    assert result.returncode == (23 if failure else 0), result.stderr
    calls = log.read_text(encoding="utf-8").splitlines()
    assert len(calls) == len(steps)
    for call, step in zip(calls, steps, strict=True):
        assert step in call
    if failure:
        assert f"Injected updater failure: {failure}" in result.stderr


def test_review_recipes_forward_arguments() -> None:
    """Just preserves the base argument and keeps uncommitted review independent."""
    branch = dry_run(ROOT, "review")
    assert 'review branch --base="$1"' in branch.stderr
    uncommitted = dry_run(ROOT, "review-uncommitted")
    assert "review uncommitted" in uncommitted.stderr


@pytest.mark.parametrize("recipe", [("review", "HEAD"), ("review-uncommitted",)])
@pytest.mark.parametrize("status", [0, 23])
def test_review_uses_consumer_instructions_and_propagates_failure(
    recipe: tuple[str, ...],
    status: int,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Run the actual Just/shared CLI boundary against a local reviewer stub."""
    stub = tmp_path / ("coderabbit.cmd" if os.name == "nt" else "coderabbit")
    content = f"@echo off\necho REVIEW_STUB %*\nexit /b {status}\n" if os.name == "nt" else f'#!/bin/sh\nprintf "REVIEW_STUB\\n%s\\n" "$@"\nexit {status}\n'
    stub.write_text(content, encoding="utf-8")
    stub.chmod(0o700)
    monkeypatch.setenv("PATH", str(tmp_path) + os.pathsep + os.environ["PATH"])
    result = run_safe_command("just", list(recipe), cwd=ROOT, check=False)
    assert result.returncode == status
    assert "REVIEW_STUB" in result.stdout
    for argument in ("--agent", "--include-untracked", "--config", "AGENTS.md", ".coderabbit.yml"):
        assert argument in result.stdout
    assert ("--base=HEAD" if recipe[0] == "review" else "--uncommitted") in result.stdout


def test_security_covers_maintained_lockfiles_and_stays_opt_in() -> None:
    """New resolution roots must join the explicit consumer audit inventory."""
    files = run_safe_command("git", ["--no-pager", "ls-files", "--cached", "--others", "--exclude-standard"], cwd=ROOT).stdout.splitlines()
    lockfiles = {name for name in files if Path(name).name in {"Cargo.lock", "uv.lock"}}
    audit = dry_run(ROOT, "audit").stderr
    assert set(audit.split("security osv ", 1)[1].split()) == lockfiles
    security = dry_run(ROOT, "security").stderr
    assert "security osv" in security
    assert "security secrets" in security
    for recipe in ("check", "ci"):
        commands = dry_run(ROOT, recipe).stderr
        assert "security osv" not in commands
        assert "security secrets" not in commands


def test_changelog_recipe_forwards_explicit_release_date() -> None:
    """CDT's release recipe forwards the supplied tag and date to the shared CLI."""
    result = dry_run(ROOT, "changelog-release", ("v0.2.0", "2026-10-01"))
    assert '--tag "$1" --date "$2"' in result.stderr


def test_changelog_regeneration_rotation_and_archived_notes(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    """The installed generator must accept CDT history before and after rotation."""
    for name in ("pyproject.toml", "rumdl.toml", "CHANGELOG.md"):
        (tmp_path / name).write_bytes((ROOT / name).read_bytes())
    archive = tmp_path / "docs/archives/changelog"
    archive.mkdir(parents=True)
    for source in (ROOT / "docs/archives/changelog").glob("*.md"):
        (archive / source.name).write_bytes(source.read_bytes())
    # Read real commits/tags without creating commits or modifying the checkout.
    git_dir = run_safe_command("git", ["--no-pager", "rev-parse", "--absolute-git-dir"], cwd=ROOT).stdout.strip()
    (tmp_path / ".git").write_text(f"gitdir: {Path(git_dir).as_posix()}\n", encoding="utf-8")
    args = ["--root", str(tmp_path), "changelog"]
    assert main([*args, "generate"]) == 0
    sources = [tmp_path / "CHANGELOG.md", *archive.glob("*.md")]
    generated = {path: path.read_bytes() for path in sources}
    assert main([*args, "generate"]) == 0
    assert {path: path.read_bytes() for path in sources} == generated

    # Every current release must remain accessible when the next minor is prepared.
    current = (tmp_path / "CHANGELOG.md").read_text(encoding="utf-8")
    releases = re.findall(r"^## \[([0-9.]+)\] - (\d{4}-\d{2}-\d{2})$", current, re.MULTILINE)
    assert releases, "Full Git history and release tags are required (fetch-depth: 0)."
    major, minor, _patch = releases[0][0].split(".")
    prospective = ["--tag", f"v{major}.{int(minor) + 1}.0", "--date", "2099-01-01"]
    assert main([*args, "generate", *prospective, "--dry-run"]) == 0
    assert {path: path.read_bytes() for path in sources} == generated
    assert set(archive.glob("*.md")) == set(sources[1:])
    assert main([*args, "generate", *prospective]) == 0
    assert main([*args, "check"]) == 0
    rotated = archive / f"{major}.{minor}.md"
    assert rotated.is_file()
    for release, date in releases:
        assert f"## [{release}] - {date}" in rotated.read_text(encoding="utf-8")
        capsys.readouterr()
        assert main([*args, "notes", f"v{release}"]) == 0
        assert "https://github.com/acgetchell/causal-triangulations/commit/" in capsys.readouterr().out
    assert f"## [{major}.{int(minor) + 1}.0] - 2099-01-01" in (tmp_path / "CHANGELOG.md").read_text(encoding="utf-8")


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
    assert isinstance(config, MeasurementConfig)
    assert config.command == ("just", "bench-latest")
    commands = dry_run(ROOT, "bench-latest").stderr
    assert commands.index("--test physics_integration") < commands.index("--bench ci_performance_suite")
    assert dict(config.dependencies).keys() == {"criterion", "delaunay", "la-stack", "markov-chain-monte-carlo"}


@pytest.mark.parametrize(
    "orphan",
    [None, "docs/performance/v2/pair.comparison.json", "docs/performance/v2/pair.evidence.json", "tooling/performance-readme.toml"],
)
def test_performance_check_accepts_only_empty_initial_inventory(orphan: str | None, tmp_path: Path) -> None:
    """Missing reports are valid before measurement, never after evidence is retained."""
    if orphan is not None:
        path = tmp_path / orphan
        path.parent.mkdir(parents=True)
        path.write_text("orphaned evidence", encoding="utf-8")
    result = run_safe_command(
        "just",
        ["--justfile", str(ROOT / "justfile"), "--working-directory", str(tmp_path), "performance-check"],
        cwd=tmp_path,
        check=False,
    )
    assert result.returncode == (0 if orphan is None else 1), result.stderr
    assert ("No release comparison yet" in result.stdout) == (orphan is None)
    if orphan is not None:
        assert "Release evidence exists without its current report" in result.stderr


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
    report_check = [
        "--justfile",
        str(ROOT / "justfile"),
        "--working-directory",
        str(tmp_path),
        "--set",
        "_tools",
        "research-repo-tools --root .",
        "performance-check",
    ]
    assert run_safe_command("just", report_check, cwd=tmp_path).returncode == 0
    report = tmp_path / "docs/performance/v2/performance.md"
    original_report = report.read_bytes()
    report.write_text("stale report\n", encoding="utf-8")
    assert run_safe_command("just", report_check, cwd=tmp_path, check=False).returncode != 0
    report.write_bytes(original_report)
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
