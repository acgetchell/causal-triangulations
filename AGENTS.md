# AGENTS.md

Essential guidance for AI assistants working in this repository.

## Core Rules

### Git Operations

- **NEVER** run `git commit`, `git push`, `git tag`, or any git commands that modify version control state
- **ALLOWED**: Run read-only git commands (e.g. `git --no-pager status`, `git --no-pager diff`, `git --no-pager log`, `git --no-pager show`, `git --no-pager blame`) to inspect changes/history
- **ALWAYS** use `git --no-pager` when reading git output
- Suggest git commands that modify version control state for the user to run manually

### Commit Messages

When user requests commit message generation:

1. Run `git --no-pager diff --cached --stat`
2. Generate conventional commit format: `<type>: <brief summary>`
3. Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `chore`, `style`, `ci`, `build`
4. Include body with organized bullet points and test results
5. Present in code block (no language) - user will commit manually

### Code Quality

- **ALLOWED**: Run formatters/linters: `cargo fmt`, `cargo clippy`, `cargo doc`, `taplo fmt`, `taplo lint`, `uv run ruff check --fix`, `uv run ruff format`, `shfmt -w`, `shellcheck -x`, `dprint fmt`, `typos`, `actionlint`
- **NEVER**: Use `sed`, `awk`, `perl` for code edits
- **ALWAYS**: Use `edit_files` for edits (and `create_file` for new files)
- **EXCEPTION**: Shell text tools OK for read-only analysis only

### Validation

- **JSON**: Validate with `jq empty <file>.json` after editing (or `just validate-json`)
- **TOML**: Lint/format with taplo: `just toml-lint`, `just toml-fmt-check`, `just toml-fmt`
- **GitHub Actions**: Validate workflows with `just action-lint` (uses `actionlint`)
- **Spell check**: Run `just spell-check` (or `just lint-docs`) after editing; add legitimate technical terms to `typos.toml` under `[default.extend-words]`
- **Markdown**: Run `just markdown-check` (uses `dprint`) after editing; fix with `just markdown-fix`
- **Shell scripts**: Run `just shell-check` after editing `.sh` files; fix with `just shell-fmt`

### Rust

- Prefer borrowed APIs by default: take references (`&T`, `&mut T`, `&[T]`) as arguments and return borrowed views (`&T`, `&[T]`) when possible. Only take ownership or return `Vec`/allocated data when required.
- Integration tests in `tests/*.rs` are separate crates; add a crate-level doc comment (`//! ...`) at the top to satisfy clippy `missing_docs` (CI uses `-D warnings`).
- **Module layout**: Never use `mod.rs`. Declare modules in `src/lib.rs` (and `src/main.rs` for binaries), including nested modules via inline `pub mod foo { pub mod bar; }` when needed.

### Python

- Use `uv run` for all Python scripts (never `python3` or `python` directly)
- Use pytest for tests (not unittest)
- **Type checking**: `just python-typecheck` runs `ty check` (blocking - all code must pass)
- Add type hints to new code

## Common Commands

```bash
just fix              # Apply formatters/auto-fixes (mutating)
just check            # Lint/validators (non-mutating)
just ci               # Full CI run (checks + all tests + bench compile)
just commit-check     # Comprehensive pre-commit validation (includes Kani)
just lint             # All linting
just test             # Lib and doc tests
just test-integration # Integration tests (tests/)
just test-all         # All tests (Rust + Python)
just examples         # Run all example scripts
```

### Changelog

- Never edit `CHANGELOG.md` directly - it's auto-generated from git commits
- Use `just changelog` to regenerate

## Project Context

- **Rust** {2,3,4}D Causal Dynamical Triangulations library (MSRV 1.94.0, Edition 2024)
- **No unsafe code**: `#![forbid(unsafe_code)]`
- **Architecture**: CDT physics layered over a pluggable geometry backend (`delaunay` crate)
- **Modules**: `src/cdt/` (CDT logic: moves, action, Metropolis), `src/geometry/` (geometry abstractions and backends), `src/config.rs` (simulation configuration)
- **Ergodic moves**: `attempt_22_move`, `attempt_13_move`, `attempt_31_move`, `attempt_edge_flip` are currently placeholder implementations; full `delaunay::Tds` integration is planned
- **Formal verification**: Kani proofs in `src/cdt/action.rs` under `#[cfg(kani)]`
- **Python scripts**: `scripts/` contains benchmark, changelog, and hardware utilities; tests in `scripts/tests/` run via pytest
- **When adding/removing files**: Update `docs/project.md`

## Test Execution

- **tests/ changes**: Run `just test-integration` (or `just ci`)
- **examples/ changes**: Run `just examples`
- **benches/ changes**: Run `just bench-compile`
- **src/ changes**: Run `just test`
- **scripts/ changes**: Run `just test-python`
- **Any Rust changes**: Run `just doc-check`

## Formal Verification

```bash
just kani       # Run all Kani proofs (slow)
just kani-fast  # Run fast harness (verify_action_config only)
```

Kani ships its own pinned nightly and does not read `rust-toolchain.toml`. Regular builds and tests use the workspace MSRV toolchain.
