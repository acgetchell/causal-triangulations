# AGENTS.md

Essential guidance for AI assistants working in this repository.

This file is the **entry point for all coding agents**. Detailed rules are split into additional documents under `docs/dev/`. Agents MUST read the referenced files before making changes.

---

## Required Reading

Before modifying code, agents MUST read:

- `AGENTS.md` (this file)
- **All files in `docs/dev/*.md`** – repository development rules
- `docs/code_organization.md` – module layout and architecture

The `docs/dev/` directory contains the authoritative development guidance for this repository. Agents must load every file in that directory before making changes.

---

## Core Rules

### Git Operations

- **NEVER** run `git commit`, `git push`, `git tag`, or any git commands that modify version control state
- **ALLOWED**: read‑only git commands (`git --no-pager status`, `git --no-pager diff`, `git --no-pager log`, `git --no-pager show`, `git --no-pager blame`)
- **ALWAYS** use `git --no-pager` when reading git output
- Suggest git commands that modify version control state for the user to run manually
- When suggesting branch names, prefer `{type}/{issue}-descriptor-or-two`, e.g. `fix/307-topology-validation`, `perf/315-bench-profile`, or `doc/329-branch-guidance`. If an environment requires an owner/tool prefix, keep this structure after the prefix, e.g. `codex/fix/307-topology-validation`.

### GitHub CLI (`gh`)

When using the `gh` CLI to view issues, PRs, or other GitHub objects:

- **ALWAYS** use `--json` with `| cat` to avoid pager and scope errors:

  ```bash
  gh issue view 42 --repo acgetchell/causal-triangulations --json title,body | cat
  ```

- To extract specific fields cleanly, combine `--json` with `--jq`:

  ```bash
  gh issue view 42 --repo acgetchell/causal-triangulations --json title,body --jq '.title + "\n" + .body' | cat
  ```

- **AVOID** plain `gh issue view N` — it may fail with `read:project` scope errors or open a pager.

- To manage **issue dependencies** (Blocks / Is Blocked By), use the GitHub REST API via `gh api`. The endpoint requires the **internal issue ID** (not the issue number).

  To get an issue's internal ID:

  ```bash
  gh api repos/acgetchell/causal-triangulations/issues/42 --jq '.id'
  ```

  To add a "blocked by" dependency (e.g. #10 is blocked by #42):

  ```bash
  gh api repos/acgetchell/causal-triangulations/issues/10/dependencies/blocked_by \
    -X POST -F issue_id=<BLOCKING_ISSUE_ID>
  ```

  To list existing blocked‑by dependencies:

  ```bash
  gh api repos/acgetchell/causal-triangulations/issues/10/dependencies/blocked_by \
    --jq '[.[].number]' | cat
  ```

  **Note**: Use `-F` (not `-f`) for `issue_id` so it is sent as an integer. The API returns HTTP 422 if the dependency already exists.

- When updating issues, use explicit `comment`/`edit` commands. For **arbitrary Markdown** (backticks, quotes, special characters), prefer `--body-file -` with a heredoc:

  ```bash
  gh issue comment 42 --repo acgetchell/causal-triangulations --body-file - <<'EOF'
  ## Heading

  Body with `backticks`, **bold**, and apostrophes that's safe.
  EOF
  ```

  For **simple text only** (no apostrophes or special characters), single‑quoted `--body` is fine:

  ```bash
  gh issue comment 42 --repo acgetchell/causal-triangulations --body 'Simple update text'
  ```

### Code Editing

- **NEVER** use `sed`, `awk`, `perl`, or `python` to modify code
- **ALWAYS** use the patch editing mechanism provided by the agent
- Shell text tools may be used for **read‑only analysis only**

### Rust Error Handling

- Do not introduce `Box<dyn std::error::Error>`, `Box<dyn Error>`, or `anyhow::Error` as fallible return types in production `src/` code, public doctests, examples, or benchmarks that demonstrate user-facing workflows
- Prefer `CdtResult<T>` and narrow `CdtError` variants with structured context for distinct I/O, serialization, validation, backend, checkpoint, or output failure modes
- `&dyn Error` is acceptable for `std::error::Error::source`, tests that verify standard error trait behavior, and lint fixtures that intentionally exercise forbidden generic-error patterns
- Detailed error-type guidance lives in `docs/dev/rust.md`

### Rust Import Hygiene

- Keep production module preambles free of test-only imports; place `#[cfg(test)]` imports inside the relevant `tests` module instead
- Detailed import guidance lives in `docs/dev/rust.md`

### Public API Preludes

- Keep `prelude::*` small and focused on common quick-start workflows.
- Keep scoped preludes minimal and orthogonal; do not duplicate specialized APIs across scoped preludes unless the overlap is intentionally documented.
- `prelude::observables` is the user-facing analysis surface for measuring triangulations and derived physical observables.
- `prelude::simulation` is for running, inspecting, and debugging simulations; it may expose telemetry and proposal/result types, but should not become the home for user-facing observable estimators.
- Detailed prelude boundary guidance lives in `docs/dev/rust.md`.

### Commit Message Generation

When generating commit messages:

1. Run `git --no-pager diff --cached --stat`
2. Use conventional commits: `<type>: <summary>`
3. Valid types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `chore`, `style`, `ci`, `build`
4. Include bullet‑point body describing key changes
5. Present inside a code block so the user can commit manually

#### Changelog‑Aware Body Text

Commit bodies appear **verbatim** in `CHANGELOG.md` (indented by git‑cliff's template). Write them as clean, readable prose:

- Keep the **subject line** concise — it becomes the changelog entry.
- The **type** determines the changelog section (`feat` → Added, `fix` → Fixed, `refactor`/`test`/`style` → Changed, `perf` → Performance, `docs` → Documentation, `build`/`chore`/`ci` → Maintenance).
- Include **PR references** as `(#N)` in the subject — cliff auto‑links them (e.g. `feat: add foo (#42)`).
- **Avoid headings** `#`–`###` in the body — they conflict with changelog structure (`##` = release, `###` = section). Use `####` if a heading is truly needed.
- Body text should be **plain prose or simple lists**. Numbered lists and sub‑items are fine but avoid deep nesting.

#### Breaking Changes

Breaking changes **must** use one of these conventional commit markers so that `git‑cliff` can detect them and generate the `### ⚠️ Breaking Changes` section in `CHANGELOG.md`:

- **Bang notation**: `feat!: remove deprecated API` (append `!` after the type/scope)
- **Footer trailer**: add `BREAKING CHANGE: <description>` as a [git trailer](https://git-scm.com/docs/git-interpret-trailers) at the end of the commit body

Examples of breaking changes: removing/renaming public API items, changing default behaviour, bumping MSRV, altering serialisation formats.

---

## Validation Workflow

After modifying files, run appropriate validators.

Common commands:

```bash
just fix
just check
just ci
```

Refer to `docs/dev/commands.md` for full details.

When adding or renaming Cargo examples, update `just examples-validate` markers as needed so CI keeps validating the user-facing example contracts.

For tooling-alignment work, update `docs/dev/tooling-alignment.md` with the comparison and rationale before adding or changing config, workflow, or repository-rule files.

---

## Testing Rules

Testing guidance lives in:

```text
docs/dev/testing.md
```

Key principle:

- Rust changes must pass unit tests, integration tests, and documentation builds.

---

## Project Context

- **Language**: Rust
- **Project**: {2,3,4}D Causal Dynamical Triangulations library
- **MSRV**: 1.95.0
- **Edition**: 2024
- **Unsafe code**: forbidden (`#![forbid(unsafe_code)]`)
- **Architecture**: `src/geometry/` is the backend interface layer for the `delaunay` crate; `src/cdt/` is the CDT domain layer. Direct `use delaunay::` imports are restricted to `src/geometry/` (`backends/delaunay.rs` and `generators.rs`); CDT modules use the trait-based abstractions, crate-owned Delaunay handles, generator utilities, and `DelaunayBackend2D` type alias (see `docs/dev/rust.md § Geometry Backend Isolation`)
- **Modules**: `src/cdt/` (CDT logic: moves, action, Metropolis, foliation, observables, results, triangulation child modules), `src/geometry/` (geometry abstractions and backends), `src/config.rs` (simulation configuration)
- **Foliation**: `src/cdt/foliation.rs` defines foliation bookkeeping and edge/simplex classification. Time labels are stored as vertex data; `from_cdt_strip` and `from_toroidal_cdt` construct labeled CDT triangulations; `validate_causality` enforces adjacent-slice edges (with circular distance on toroidal time). Design documented in `docs/foliation.md`
- **Ergodic moves**: `attempt_22_move`, `attempt_13_move`, `attempt_31_move`, `attempt_edge_flip` are Delaunay-backed, foliation-aware move kernels. They mutate through narrow CDT-owned edit operations, roll back failed finalized mutations, and preserve topology/foliation invariants
- **Python scripts**: `scripts/` contains benchmark, changelog, and hardware utilities; tests in `scripts/tests/` run via pytest
- **When adding/removing files**: Update `docs/code_organization.md`

Architecture details are documented in:

```text
docs/code_organization.md
```

---

## Python

Detailed Python guidance lives in `docs/dev/python.md`.

- Use `uv run` for all Python scripts (never `python3` or `python` directly)
- Use pytest for tests (not unittest)
- **Type checking**: `just python-typecheck` runs `ty check` (blocking — all code must pass)
- Add type hints to new code

---

## Documentation Maintenance

- Never edit `CHANGELOG.md` directly — it's auto-generated from git commits
- Run `just changelog` to regenerate

---

## Agent Behavior Expectations

Agents should:

- Prefer small, focused patches
- Follow Rust idioms and borrowing conventions
- Avoid introducing allocations unless necessary
- Avoid panics in library code
- Search documentation under `docs/` when unsure

If multiple solutions exist, prefer the one that:

1. Preserves API stability
2. Keeps code simple and maintainable
3. Maintains the CDT ↔ geometry backend separation
