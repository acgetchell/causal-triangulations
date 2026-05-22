# causal-triangulations

[![Crates.io](https://img.shields.io/crates/v/causal-triangulations.svg)](https://crates.io/crates/causal-triangulations) [![Downloads](https://img.shields.io/crates/d/causal-triangulations.svg)](https://crates.io/crates/causal-triangulations) [![Docs.rs](https://docs.rs/causal-triangulations/badge.svg)](https://docs.rs/causal-triangulations) [![CI](https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml/badge.svg)](https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml) [![rust-clippy analyze](https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml/badge.svg)](https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml) [![Codecov](https://codecov.io/gh/acgetchell/causal-triangulations/graph/badge.svg?token=CsbOJBypGC)](https://codecov.io/gh/acgetchell/causal-triangulations) [![Audit dependencies](https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml/badge.svg)](https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml)

Causal Dynamical Triangulations for quantum gravity in [Rust], built on fast Delaunay triangulation primitives.

## 🌌 Introduction

This library implements **Causal Dynamical Triangulations (CDT)** in [Rust]. CDT is a non-perturbative approach to quantum gravity that constructs discrete spacetime as triangulated manifolds with causal structure, providing a computational framework for studying quantum gravity phenomenology.

For an introduction to Causal Dynamical Triangulations, see [this paper](https://arxiv.org/abs/hep-th/0105267).

The library leverages high-performance [Delaunay triangulation] backends and provides a foundational toolkit for CDT research and exploration.

## ✨ Features

- [x] Delaunay-built 1+1 CDT strip and periodic toroidal S¹×S¹ constructors with foliation invariants
- [x] Foliation-aware topology, causality, and simplex-classification validation
- [x] Proposal-before-mutation Metropolis-Hastings simulation with rollback on failed accepted moves
- [x] Regge action calculation with configurable coupling constants
- [x] Alexander/Pachner-style local move proposals with causal constraints
- [x] Volume-profile, Hausdorff-dimension, and spectral-dimension observables for CDT analysis
- [x] CSV/JSON simulation output for external analysis workflows
- [x] Resumable serde-backed CDT/MCMC checkpoints for durable chain continuation
- [x] Focused public preludes for simulation, triangulation, geometry, action, and observables
- [x] Command-line interface, examples, Criterion benchmarks, and CI-aligned validation tooling
- [x] Cross-platform compatibility: Linux, macOS, Windows

See [CHANGELOG.md](CHANGELOG.md) for release history.

## 🚧 Project Status

🚧 **Pre-release (0.0.x)** — The 1+1 CDT foundation is implemented and tested, but this crate is still under active development and **not yet ready for production use**. APIs, data structures, and module boundaries may change before v0.1.0.

The library currently supports validated 1+1 CDT construction, foliation checks, Metropolis sampling, and core observables. Higher-dimensional CDT support, full move-kernel maturity, visualization/export workflows, and advanced ensemble-analysis helpers remain roadmap work.

See [`docs/roadmap.md`](docs/roadmap.md) for current direction, near-term candidates, and non-goals.

## ⚙️ Requirements

- Rust 1.95 or newer (required by dependencies such as `delaunay` and `la-stack`)

**Why Rust for CDT?**

- **Memory safety** for large-scale simulations
- **Zero-cost abstractions** for performance-critical geometry operations
- **Rich ecosystem** for scientific computing and parallel processing

## 🧩 Ecosystem

This crate is part of a broader Rust ecosystem for computational geometry and simulation:

- [`delaunay`](https://crates.io/crates/delaunay) — geometric primitives and triangulations
- `la-stack` — linear algebra utilities
- [`markov-chain-monte-carlo`](https://crates.io/crates/markov-chain-monte-carlo) — composable MCMC traits, including delayed-commit proposals for CDT move ordering

The long-term design separates:

- **Geometry** (triangulations and invariants)
- **Sampling** (MCMC algorithms)
- **Physics** (CDT-specific dynamics and observables)

Within this crate, `src/geometry/` is the backend interface layer over `delaunay`, while `src/cdt/` is the CDT domain layer.

## 🤝 How to Contribute

We welcome contributions! Here's a 30-second quickstart:

```bash
# Clone and setup
git clone https://github.com/acgetchell/causal-triangulations.git
cd causal-triangulations

# Traditional approach
cargo build && cargo test

# Modern approach (recommended) - install just command runner
cargo install just
just setup           # Complete environment setup
just check           # Run all linters/validators
just fix             # Apply formatters/auto-fixes
just --list          # See all available development commands

# Run examples
just run-example     # Basic simulation
./examples/scripts/basic_simulation.sh      # Shell script example
./examples/scripts/parameter_sweep.sh       # Temperature sweep study
./examples/scripts/performance_test.sh      # Performance benchmarking across system sizes
```

`just setup` prints a checklist of external tools used by repository workflows (for example: `uv`, `taplo`, `actionlint`, `shfmt`, `shellcheck`, `jq`) and how to install them.

**Just Workflows:**

- `just check` - Run linters/validators (non-mutating)
- `just fix` - Apply formatters/auto-fixes (mutating)
- `just ci` - CI parity (mirrors GitHub Actions workflow [`ci.yml`](.github/workflows/ci.yml))
- `just commit-check` - Comprehensive pre-commit validation

**Repository tooling (via `just`):**

- `just changelog` - Regenerate `CHANGELOG.md`
- `just changelog-unreleased v0.1.0` - Generate a release changelog before the final tag exists
- `just tag v0.1.0` - Create an annotated git tag from changelog content
- `just perf-help` - Show performance analysis commands (`perf-baseline`, `perf-check`, etc.)

## 📋 Examples

### Library Usage

See [`examples/basic_cdt.rs`](examples/basic_cdt.rs) for a complete working example:

```rust
use causal_triangulations::prelude::action::ActionConfig;
use causal_triangulations::prelude::errors::CdtResult;
use causal_triangulations::prelude::simulation::{MetropolisAlgorithm, MetropolisConfig};
use causal_triangulations::prelude::triangulation::CdtTriangulation;

fn main() -> CdtResult<()> {
    // Create a foliated open-boundary CDT strip.
    let triangulation = CdtTriangulation::from_cdt_strip(8, 4)?;

    // Configure and run the Monte Carlo simulation.
    let metropolis_config = MetropolisConfig::new(1.0, 1000, 100, 10);
    let action_config = ActionConfig::default();
    let algorithm = MetropolisAlgorithm::new(metropolis_config, action_config);

    let results = algorithm.run(triangulation)?;
    println!("Acceptance rate: {:.3}", results.acceptance_rate());
    println!("Average action: {:.3}", results.average_action());
    Ok(())
}
```

### Command Line Interface

```bash
# Build the binary
cargo build --release

# Build a triangulation and record the initial measurement
./target/release/cdt --vertices 20 --timeslices 10 --steps 2000

# Configure a parameter sweep with simulation enabled
./target/release/cdt \
  --vertices 50 --timeslices 12 \
  --temperature 1.5 --coupling-0 0.8 \
  --steps 5000 --simulate
```

### Ready-to-Use Scripts

The `examples/scripts/` directory contains research workflows:

- **`basic_simulation.sh`** - Simple simulation command
- **`parameter_sweep.sh`** - Temperature sweep setup
- **`performance_test.sh`** - Construction and simulation timing across system sizes

For detailed documentation, sample output, and usage instructions for each script, see [examples/scripts/README.md](examples/scripts/README.md).

For comprehensive CLI documentation and advanced usage patterns, see [`docs/CLI_EXAMPLES.md`](docs/CLI_EXAMPLES.md).

## 📋 Benchmarking

Comprehensive performance benchmarks using [Criterion]:

```bash
# Run all benchmarks
cargo bench

# Specific benchmark categories
cargo bench triangulation_creation
cargo bench metropolis_simulation
cargo bench action_calculations

# CI regression benchmark contract
just bench-ci

# Performance regression testing
just perf-check          # Check for performance regressions
just perf-baseline       # Save performance baseline
just perf-report         # Generate detailed performance report
just perf-trends 7       # Analyze performance trends over 7 days
```

See [`benches/README.md`](benches/README.md) for benchmark details and [`docs/PERFORMANCE_TESTING.md`](docs/PERFORMANCE_TESTING.md) for comprehensive performance testing workflow documentation.

## 🛣️ Roadmap

The high-level roadmap, including 1+1 maturity work, future 2+1 and 3+1 CDT topology tracks, observables, dual/Voronoi geometry, visualization, and non-goals, lives in [`docs/roadmap.md`](docs/roadmap.md).

## Design notes

- **Separation of concerns**: geometry primitives (Delaunay/Voronoi) are decoupled from CDT dynamics.
- **Foliation‑aware data model**: explicit time labels; space‑like vs time‑like edges encoded in types.
- **Testing**: unit + property tests for invariants (e.g., move reversibility, manifoldness).

For comprehensive guidelines on contributing, development environment setup, testing, and code organization, please see [CONTRIBUTING.md](CONTRIBUTING.md).

This includes information about:

- Building and testing the library
- Running benchmarks and performance analysis
- Code style and standards
- Submitting changes and pull requests
- Code organization and development tools

## 📚 References

For a comprehensive list of academic references and bibliographic citations used throughout the library, see [REFERENCES.md](REFERENCES.md).

This includes foundational work on:

- Causal Dynamical Triangulations theory
- Monte Carlo methods in quantum gravity
- Computational geometry and Delaunay triangulations
- Discrete approaches to general relativity

## 📝 License

This project's license is specified in [LICENSE](LICENSE).

---

[Rust]: https://rust-lang.org
[Delaunay triangulation]: https://crates.io/crates/delaunay
[Criterion]: https://github.com/bheisler/criterion.rs
