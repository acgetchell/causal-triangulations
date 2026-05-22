# Contributing to Causal Dynamical Triangulations

Thank you for your interest in contributing to the [**causal-triangulations**][cdt-lib] library! This document provides comprehensive guidelines for
contributors, from first-time contributors to experienced developers looking to contribute significant features.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Environment Setup](#development-environment-setup)
- [Code Organization](#code-organization)
- [Development Workflow](#development-workflow)
- [Just Command Runner](#just-command-runner)
- [Code Style and Standards](#code-style-and-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Performance and Benchmarking](#performance-and-benchmarking)
- [Submitting Changes](#submitting-changes)
- [Types of Contributions](#types-of-contributions)
- [Release Process](#release-process)
- [Getting Help](#getting-help)

## Code of Conduct

This project and everyone participating in it is governed by our commitment to creating an inclusive and welcoming environment for quantum gravity research and
computational physics development.

Our community is built on the principles of:

- **Respectful collaboration** in quantum gravity research and computational physics
- **Inclusive participation** regardless of background or experience level
- **Excellence in scientific computing** and algorithm implementation
- **Open knowledge sharing** about CDT and discrete approaches to quantum gravity

## Getting Started

### Prerequisites

Before you begin, ensure you have:

1. **Rust 1.95.0** (pinned via `rust-toolchain.toml` - automatically handled by rustup)
2. **Git** for version control
3. **Just** (command runner): `cargo install just`

### Quick Start

1. **Fork and clone** the repository:
   - Fork this repository to your GitHub account using the "Fork" button
   - Clone your fork locally:

   ```bash
   git clone https://github.com/yourusername/causal-triangulations.git
   cd causal-triangulations
   ```

2. **Setup development environment**:

   ```bash
   # Comprehensive setup (recommended)
   just setup           # Installs tools and builds project

   # Manual setup
   cargo build
   ```

3. **Run tests**:

   ```bash
   # Basic tests
   cargo test            # Library tests
   cargo test --test cli # CLI tests
   cargo test --test integration_tests  # Integration tests

   # Or use convenient workflows:
   just check           # Run non-mutating checks
   just fix             # Apply formatters/auto-fixes when needed
   just test-all        # All tests
   ```

4. **Try the examples**:

   ```bash
   cargo run --example basic_cdt
   ./examples/scripts/basic_simulation.sh
   ```

5. **Run benchmarks** (optional):

   ```bash
   # Compile benchmarks without running
   cargo bench --no-run

   # Run all benchmarks
   cargo bench
   ```

6. **Code quality checks**:

   ```bash
   cargo fmt            # Format code
   cargo clippy --all-targets -- -D warnings  # Linting
   just check           # Run all non-mutating checks
   just fix             # Apply formatters/auto-fixes when needed
   just lint            # Lint code, docs, and config (checks only)
   ```

7. **Use Just for comprehensive workflows** (recommended):

   ```bash
   # See all available commands
   just --list

   # Common workflows
   just check           # Run all linters/validators
   just fix             # Apply formatters/auto-fixes when needed
   just commit-check    # Full pre-commit validation
   just ci              # CI parity (mirrors .github/workflows/ci.yml)
   ```

## Development Environment Setup

### Automatic Toolchain Management

**🔧 This project uses automatic Rust toolchain management via `rust-toolchain.toml`**

When you enter the project directory, `rustup` will automatically:

- **Install the correct Rust version** (1.95.0) if you don't have it
- **Switch to the pinned version** for this project
- **Install required components** (clippy, rustfmt, rust-docs, rust-src, rust-analyzer)
- **Add cross-compilation targets** for supported platforms

**What this means for contributors:**

1. **No manual setup needed** - Just have `rustup` installed ([rustup.rs][rustup])
2. **Consistent environment** - Everyone uses the same Rust version automatically
3. **Reproducible builds** - Eliminates "works on my machine" issues
4. **CI compatibility** - Your local environment matches our CI exactly

**First time in the project?** You'll see:

```text
info: syncing channel updates for '1.95.0-<your-platform>'
info: downloading component 'cargo'
info: downloading component 'clippy'
...
```

This is normal and only happens once.

## Code Organization

The source/module layout and architecture-sensitive boundaries live in [docs/code_organization.md](docs/code_organization.md). Keep that file current when
adding, removing, or moving source files, examples, or architecture-significant modules.

## Development Workflow

### Just Command Runner

This project uses [Just] as the primary task automation tool. Just provides better workflow organization than traditional shell scripts or cargo aliases.

**Essential Just Commands:**

```bash
just setup          # Complete environment setup
just check          # Run linters/validators (non-mutating)
just fix            # Apply formatters/auto-fixes (mutating)
just ci             # CI parity (mirrors .github/workflows/ci.yml)
just commit-check   # Comprehensive pre-commit validation (recommended before pushing)
just lint           # Lint code, docs, and config (checks only)
just test-all       # All test suites
just bench          # Run performance benchmarks
just clean          # Clean build artifacts
```

**Workflow Help:**

```bash
just --list          # Show all available commands
just help-workflows  # Detailed workflow guidance
```

### Repository Tooling Map

```text
.github/workflows/codeql.yml # CodeQL analysis for Actions and Rust
.github/workflows/semgrep-sarif.yml # Repository Semgrep rule SARIF upload
rustfmt.toml           # Stable Rust formatting settings
cliff.toml             # git-cliff changelog template and commit grouping
semgrep.yaml           # Repository-owned Semgrep rules
docs/dev/python.md     # Python script style and validation guidance
docs/dev/tooling-alignment.md # Tooling comparison and issue #112 decisions
docs/roadmap.md        # High-level release direction and non-goals
tests/semgrep/         # Semgrep rule fixtures run by `just semgrep-test`
scripts/archive_changelog.py # Split completed changelog minor series into archive files
scripts/coverage_report.py # Cobertura coverage summary helper
scripts/postprocess_changelog.py # Markdown hygiene for git-cliff changelogs
scripts/tag_release.py # Annotated release tags from root or archived changelog sections
```

### Typical Development Cycle

1. **Start working on a feature/fix**:

   ```bash
   git checkout -b fix/307-topology-validation
   ```

   Branch names should follow `{type}/{issue}-descriptor-or-two`, such as `fix/307-topology-validation` or `perf/315-bench-profile`.

2. **Development cycle**:

   ```bash
   # Make changes to code
   just test            # Run fast tests (lib + doc)
   just fix             # Apply formatters/auto-fixes when needed
   # Repeat until satisfied
   ```

3. **Pre-commit validation**:

   ```bash
   just commit-check    # Full validation including all tests
   ```

4. **Submit**:

   ```bash
   git commit -m "Your descriptive commit message"
   git push origin feature/your-feature-name
   # Create pull request
   ```

## Code Style and Standards

### Rust Code Style

- **Edition**: Rust 2024
- **MSRV**: Rust 1.95.0 (pinned in `rust-toolchain.toml`)
- **Formatting**: Use `rustfmt` (configured in `rustfmt.toml`)
- **Linting**: Strict clippy with warnings as errors

### Linting Configuration

The project uses comprehensive linting rules:

```bash
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W clippy::cargo
```

Key areas of focus:

- **Performance**: Zero-cost abstractions, avoid unnecessary allocations
- **Safety**: Leverage Rust's type system for mathematical correctness
- **Documentation**: All public APIs must be documented
- **Testing**: Comprehensive test coverage including property-based tests

### Code Organization

- **Separation of concerns**: Geometry backends decoupled from CDT algorithms
- **Type safety**: Use strong types for mathematical concepts (e.g., time vs space coordinates)
- **Error handling**: Comprehensive error types with context
- **Performance**: Profile-guided optimization for hot paths

## Testing

### Test Categories

1. **Unit Tests**: Test individual functions and methods

   ```bash
   cargo test --lib
   ```

2. **Integration Tests**: Test component interactions

   ```bash
   cargo test --test integration_tests
   ```

3. **CLI Tests**: Test command-line interface

   ```bash
   cargo test --test cli
   ```

4. **Documentation Tests**: Ensure examples in docs compile

   ```bash
   cargo test --doc
   ```

5. **Benchmark Tests**: Verify benchmarks compile

   ```bash
   cargo bench --no-run
   ```

### Property-Based Testing

For mathematical algorithms, use property-based testing:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_triangulation_invariant(vertices in 3u32..100) {
        let triangulation = create_test_triangulation(vertices);
        // Test Euler characteristic invariant
        prop_assert!(triangulation.satisfies_euler_formula());
    }
}
```

### Test Data and Fixtures

- Use deterministic test data when possible
- For randomized tests, use seeded generators for reproducibility
- Keep test execution time reasonable (< 1 second for unit tests)

## Documentation

### Documentation Standards

- **Public APIs**: All public functions, structs, and traits must have rustdoc comments
- **Examples**: Include usage examples in documentation
- **Mathematical Context**: Explain the physics/mathematics behind algorithms
- **Performance Notes**: Document time/space complexity where relevant

### Documentation Generation

```bash
# Generate documentation
cargo doc --no-deps --open

# Check documentation builds without warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

### Contributing to Docs

- Update `docs/` directory for comprehensive guides
- Ensure examples in documentation actually compile
- Link to relevant papers in [REFERENCES.md](REFERENCES.md)

## Performance and Benchmarking

### Benchmark Organization

Benchmarks are organized in `benches/` directory:

- **Triangulation creation**: `triangulation_creation`
- **Geometry operations**: `edge_counting`, `geometry_queries`
- **Monte Carlo simulation**: `metropolis_simulation`
- **Action calculations**: `action_calculations`

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark group
cargo bench triangulation_creation

# HTML reports are automatically generated at target/criterion/report/index.html
# Open the report in your browser
open target/criterion/report/index.html
```

### Performance Guidelines

- Profile before optimizing
- Use criterion for statistical analysis
- Consider memory allocation patterns
- Document performance characteristics

### Memory Management

- Prefer stack allocation for small, fixed-size data
- Use arena allocation for temporary geometry data
- Profile memory usage for large simulations
- Consider cache-friendly data layouts

## Submitting Changes

### Pull Request Process

1. **Fork and create feature branch**
2. **Make changes following coding standards**
3. **Add tests for new functionality**
4. **Run full validation**: `just commit-check`
5. **Update documentation** if needed
6. **Create descriptive pull request**

### Commit Message Guidelines

Use conventional commit format:

```text
type(scope): description

Longer description if needed.

- List specific changes
- Reference issues: Closes #123
```

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`

### Pull Request Checklist

- [ ] Tests pass (`just test-all`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Documentation updated
- [ ] Benchmarks still compile (`cargo bench --no-run`)
- [ ] Changes are described in PR

## Types of Contributions

### Bug Reports

- Use GitHub issues
- Provide minimal reproduction case
- Include system information
- Reference relevant physics/mathematics

### Feature Requests

- Discuss in GitHub issues first
- Consider breaking changes carefully
- Provide use case and motivation
- Consider implementation complexity

### Code Contributions

- Start with smaller changes to understand codebase
- Focus on one feature/fix per PR
- Consider performance implications
- Add comprehensive tests

### Documentation Contributions

- Fix typos and improve clarity
- Add examples and tutorials
- Improve API documentation
- Update mathematical explanations

### Research Integration

- Implement new CDT algorithms
- Add support for different geometries
- Contribute benchmarks from literature

## Release Process

### Version Numbering

This project follows [Semantic Versioning](https://semver.org/):

- **MAJOR**: Breaking API changes
- **MINOR**: New features (backward compatible)
- **PATCH**: Bug fixes and improvements

### Release Checklist

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Run full test suite
4. Create release tag
5. Publish to crates.io (when ready)

## Getting Help

### Resources

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: General questions and community discussion
- **Documentation**: Comprehensive guides in `docs/` directory
- **Just workflows**: Run `just help-workflows` for guidance

### Physics and Mathematics

For questions about the underlying physics and mathematics:

- See [REFERENCES.md](REFERENCES.md) for foundational papers
- Consult CDT literature for theoretical background
- Ask in GitHub Discussions for concept clarification

### Development Questions

- Check existing issues and discussions
- Ask specific, focused questions
- Provide context about what you're trying to achieve
- Include relevant code snippets or error messages

---

Thank you for contributing to advancing computational quantum gravity research! 🌌

[cdt-lib]: https://github.com/acgetchell/causal-triangulations
[rustup]: https://rustup.rs/
[Just]: https://github.com/casey/just
