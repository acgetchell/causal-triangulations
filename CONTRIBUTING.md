# Contributing to Causal Dynamical Triangulations

Thank you for your interest in contributing to the [**causal-triangulations**][cdt-lib] library! This document provides comprehensive guidelines for contributors, from first-time contributors to experienced developers looking to contribute significant features.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Environment Setup](#development-environment-setup)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Just Command Runner](#just-command-runner)
- [Code Style and Standards](#code-style-and-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Formal Verification](#formal-verification)
- [Performance and Benchmarking](#performance-and-benchmarking)
- [Submitting Changes](#submitting-changes)
- [Types of Contributions](#types-of-contributions)
- [Release Process](#release-process)
- [Getting Help](#getting-help)

## Code of Conduct

This project and everyone participating in it is governed by our commitment to creating an inclusive and welcoming environment for quantum gravity research and computational physics development.

Our community is built on the principles of:

- **Respectful collaboration** in quantum gravity research and computational physics
- **Inclusive participation** regardless of background or experience level
- **Excellence in scientific computing** and algorithm implementation
- **Open knowledge sharing** about CDT and discrete approaches to quantum gravity

## Getting Started

### Prerequisites

Before you begin, ensure you have:

1. **Rust 1.94.0** (pinned via `rust-toolchain.toml` - automatically handled by rustup)
2. **Git** for version control
3. **Just** (command runner): `cargo install just`
4. **Kani verifier** (for formal verification): See setup instructions below

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
   just setup           # Installs Kani verifier and builds project

   # Manual setup (Kani is optional unless you're running verification)
   cargo install --locked --force --version 0.66.0 kani-verifier
   cargo kani --version
   cargo build
   ```

3. **Run tests**:

   ```bash
   # Basic tests
   cargo test            # Library tests
   cargo test --test cli # CLI tests
   cargo test --test integration_tests  # Integration tests

   # Or use convenient workflows:
   just fix             # Apply formatters/auto-fixes
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
   just fix             # Apply formatters/auto-fixes (recommended)
   just check           # Run all non-mutating checks
   just lint            # Lint code, docs, and config (checks only)
   ```

7. **Use Just for comprehensive workflows** (recommended):

   ```bash
   # See all available commands
   just --list

   # Common workflows
   just fix             # Apply formatters/auto-fixes
   just check           # Run all linters/validators
   just commit-check    # Full pre-commit validation
   just ci              # CI parity (mirrors .github/workflows/ci.yml)
   ```

## Development Environment Setup

### Automatic Toolchain Management

**🔧 This project uses automatic Rust toolchain management via `rust-toolchain.toml`**

When you enter the project directory, `rustup` will automatically:

- **Install the correct Rust version** (1.94.0) if you don't have it
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
info: syncing channel updates for '1.94.0-<your-platform>'
info: downloading component 'cargo'
info: downloading component 'clippy'
...
```

This is normal and only happens once.

### Kani Verifier Setup

This project uses [Kani] for formal verification of critical mathematical properties:

```bash
# Install Kani verifier (optional unless you're running verification)
cargo install --locked --force --version 0.66.0 kani-verifier

# Verify installation
cargo kani --version
```

## Project Structure

```text
causal-triangulations/
├── src/                    # Core library code
│   ├── cdt/               # CDT-specific implementations
│   │   ├── action.rs      # Regge action calculations
│   │   ├── metropolis.rs  # Monte Carlo simulation
│   │   ├── ergodic_moves.rs # Pachner moves
│   │   └── triangulation.rs # CDT triangulation wrapper
│   ├── geometry/          # Geometry abstraction layer
│   │   ├── backends/      # Geometry backend implementations
│   │   ├── mesh.rs        # Mesh data structures
│   │   ├── operations.rs  # High-level operations
│   │   └── traits.rs      # Geometry traits
│   ├── config.rs          # Configuration management
│   ├── errors.rs          # Error types
│   ├── util.rs            # Utility functions
│   ├── lib.rs             # Library root
│   └── main.rs            # CLI binary
├── examples/              # Usage examples
│   ├── basic_cdt.rs       # Library usage example
│   └── scripts/           # Ready-to-use simulation scripts
├── tests/                 # Test suite
│   ├── cli.rs             # CLI integration tests
│   └── integration_tests.rs # System integration tests
├── benches/               # Performance benchmarks
├── docs/                  # Documentation
└── justfile               # Task automation
```

## Development Workflow

### Just Command Runner

This project uses [Just] as the primary task automation tool. Just provides better workflow organization than traditional shell scripts or cargo aliases.

**Essential Just Commands:**

```bash
just setup          # Complete environment setup
just fix            # Apply formatters/auto-fixes (mutating)
just check          # Run linters/validators (non-mutating)
just ci             # CI parity (mirrors .github/workflows/ci.yml)
just commit-check   # Comprehensive pre-commit validation (recommended before pushing)
just lint           # Lint code, docs, and config (checks only)
just test-all       # All test suites
just kani           # Run all formal verification proofs
just kani-fast      # Run subset of Kani proofs (faster)
just bench          # Run performance benchmarks
just clean          # Clean build artifacts
```

**Workflow Help:**

```bash
just --list          # Show all available commands
just help-workflows  # Detailed workflow guidance
```

### Typical Development Cycle

1. **Start working on a feature/fix**:

   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Development cycle**:

   ```bash
   # Make changes to code
   just fix             # Apply formatters/auto-fixes
   just test            # Run fast tests (lib + doc)
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
- **MSRV**: Rust 1.94.0 (pinned in `rust-toolchain.toml`)
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

## Formal Verification

This project uses [Kani] model checker to formally verify critical properties of mathematical algorithms.

### Verification Harnesses

Create verification harnesses for critical properties:

```rust
#[cfg(kani)]
#[kani::proof]
fn verify_action_calculation() {
    let vertices = kani::any();
    let edges = kani::any();
    let faces = kani::any();
    
    kani::assume(vertices > 0);
    kani::assume(edges > 0); 
    kani::assume(faces > 0);
    
    let config = ActionConfig::default();
    let action = config.calculate_action(vertices, edges, faces);
    
    // Action should be finite
    assert!(action.is_finite());
}
```

### Running Verification

```bash
# Run all verification harnesses
just kani

# Run specific harness
cargo kani --harness verify_action_calculation

# Quick verification (subset of proofs)
just kani-fast
```

**Toolchain note:** Kani bundles its own nightly and ignores `rust-toolchain.toml`. We install `kani-verifier` 0.66.0 (bundled rustc 1.94.0-nightly) for consistency; normal builds/tests still use the workspace MSRV (1.94.0).

**Not a Cargo dependency:** The verifier is installed as a binary (`cargo install ... kani-verifier`), not as a crate dependency, so you will not see it in `Cargo.toml`.

### Verification Guidelines

- Verify mathematical invariants (e.g., Euler characteristic preservation)
- Check for arithmetic overflow/underflow
- Ensure no undefined behavior in unsafe code
- Verify causal structure constraints

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
- [ ] Kani verification passes (`just kani-fast`)
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
- Add formal verification of properties

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
[Kani]: https://model-checking.github.io/kani/
