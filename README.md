# causal-triangulations

[![CI](https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml/badge.svg)](https://github.com/acgetchell/causal-triangulations/actions/workflows/ci.yml) [![rust-clippy analyze](https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml/badge.svg)](https://github.com/acgetchell/causal-triangulations/actions/workflows/rust-clippy.yml) [![Codecov](https://codecov.io/gh/acgetchell/causal-triangulations/graph/badge.svg?token=CsbOJBypGC)](https://codecov.io/gh/acgetchell/causal-triangulations) [![Audit dependencies](https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml/badge.svg)](https://github.com/acgetchell/causal-triangulations/actions/workflows/audit.yml)

Causal Dynamical Triangulations for quantum gravity in [Rust], built on fast Delaunay triangulation primitives.

## 🌌 Introduction

This library implements **Causal Dynamical Triangulations (CDT)** in [Rust]. CDT is a non-perturbative approach to quantum gravity that constructs discrete spacetime as triangulated manifolds with causal structure, providing a computational framework for studying quantum gravity phenomenology.

For an introduction to Causal Dynamical Triangulations, see [this paper](https://arxiv.org/abs/hep-th/0105267).

The library leverages high-performance [Delaunay triangulation] backends and provides a foundational toolkit for CDT research and exploration.

## ✨ Features

- [x] 2D Causal Dynamical Triangulations with time-foliation (early implementation)
- [x] Initial Metropolis–Hastings Monte Carlo prototype (subject to extraction into a standalone crate)
- [x] Regge action calculation with configurable coupling constants (experimental)
- [x] Ergodic moves (Alexander/Pachner moves) with causal constraints (experimental)
- [x] Command-line interface for simulation workflows (early)
- [x] Benchmarking and performance analysis infrastructure (in progress)
- [x] Cross-platform compatibility (Linux, macOS, Windows)

## 🚧 Project Status

🚧 **Pre-release (0.0.x)** — This crate is under active development and **not yet ready for production use**. APIs, data structures, and module boundaries may change without notice.

The library currently supports an initial 2D CDT implementation, with planned extensions to 3D and 4D.

**Why Rust for CDT?**

- **Memory safety** for large-scale simulations
- **Zero-cost abstractions** for performance-critical geometry operations
- **Rich ecosystem** for scientific computing and parallel processing

## 🧩 Ecosystem (Planned)

This crate is part of a broader Rust ecosystem for computational geometry and simulation:

- [`delaunay`](https://crates.io/crates/delaunay) — geometric primitives and triangulations
- `la-stack` — linear algebra utilities
- `markov-chain-monte-carlo` (planned) — a composable MCMC engine to which the current Metropolis–Hastings implementation will migrate

The long-term design separates:

- **Geometry** (triangulations and invariants)
- **Sampling** (MCMC algorithms)
- **Physics** (CDT-specific dynamics and observables)

This crate focuses on the CDT (physics + domain) layer.

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
just fix             # Apply formatters/auto-fixes
just check           # Run all linters/validators
just --list          # See all available development commands

# Run examples
just run-example     # Basic simulation
./examples/scripts/basic_simulation.sh      # Shell script example
./examples/scripts/parameter_sweep.sh       # Temperature sweep study
./examples/scripts/performance_test.sh      # Performance benchmarking across system sizes
```

`just setup` prints a checklist of external tools used by repository workflows (for example: `uv`, `taplo`, `actionlint`, `shfmt`, `shellcheck`, `jq`) and how to install them.

**Just Workflows:**

- `just fix` - Apply formatters/auto-fixes (mutating)
- `just check` - Run linters/validators (non-mutating)
- `just ci` - CI parity (mirrors GitHub Actions workflow [`ci.yml`](.github/workflows/ci.yml))
- `just commit-check` - Comprehensive pre-commit validation

**Repository tooling (via `just`):**

- `just changelog` - Regenerate `CHANGELOG.md`
- `just changelog-tag v0.1.0` - Create an annotated git tag from changelog content
- `just perf-help` - Show performance analysis commands (`perf-baseline`, `perf-check`, etc.)

## 📋 Examples

### Library Usage

See [`examples/basic_cdt.rs`](examples/basic_cdt.rs) for a complete working example:

```rust
use causal_triangulations::{
    CdtConfig, MetropolisConfig, ActionConfig, MetropolisAlgorithm,
    geometry::CdtTriangulation2D,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create triangulation from random points
    let triangulation = CdtTriangulation2D::from_random_points(20, 1, 2)?;
    
    // Configure Monte Carlo simulation
    let metropolis_config = MetropolisConfig::new(1.0, 1000, 100, 10);
    let action_config = ActionConfig::default();
    let mut algorithm = MetropolisAlgorithm::new(metropolis_config, action_config);
    
    // Run simulation
    let results = algorithm.run(triangulation);
    
    println!("Acceptance rate: {:.3}", results.acceptance_rate());
    println!("Average action: {:.3}", results.average_action());
    Ok(())
}
```

### Command Line Interface

```bash
# Build the binary
cargo build --release

# Run a basic simulation
./target/release/cdt --vertices 20 --timeslices 10 --steps 2000 --simulate

# Parameter sweep for phase transition studies
./target/release/cdt \
  --vertices 50 --timeslices 12 \
  --temperature 1.5 --coupling-0 0.8 \
  --steps 5000 --simulate
```

### Ready-to-Use Scripts

The `examples/scripts/` directory contains research workflows:

- **`basic_simulation.sh`** - Simple test run and validation
- **`parameter_sweep.sh`** - Temperature sweep for phase transition analysis
- **`performance_test.sh`** - Performance benchmarking across system sizes

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

# Performance regression testing
just perf-check          # Check for performance regressions
just perf-baseline       # Save performance baseline
just perf-report         # Generate detailed performance report
just perf-trends 7       # Analyze performance trends over 7 days
```

See [`benches/README.md`](benches/README.md) for benchmark details and [`docs/PERFORMANCE_TESTING.md`](docs/PERFORMANCE_TESTING.md) for comprehensive performance testing workflow documentation.

## 🛣️ Roadmap

- [x] Integrate an existing Rust **Delaunay** triangulation library (e.g., [`delaunay`](https://crates.io/crates/delaunay))
- [x] 2D Delaunay triangulation scaffold
- [ ] 1+1 foliation (causal time‑slicing)
- [ ] 2D ergodic moves (Alexander/Pachner moves with causal constraints, fully validated)
- [ ] 2D Metropolis–Hastings (stabilized and moved to `markov-chain-monte-carlo`)
- [ ] Diffusion‑accelerated MCMC (exploration)
- [ ] Basic visualization hooks (export to common mesh formats)
- [ ] 3D Delaunay + 2+1 foliation + moves + M–H
- [ ] 4D Delaunay + 3+1 foliation + moves + M–H
- [ ] Mass initialization via **Constrained Delaunay** in 3D/4D
- [ ] Shortest paths & geodesic distance
- [ ] Curvature estimates / Einstein tensor (discrete Regge‑like observables)

## Design notes

- **Separation of concerns**: geometry primitives (Delaunay/Voronoi) are decoupled from CDT dynamics.
- **Foliation‑aware data model**: explicit time labels; space‑like vs time‑like edges encoded in types.
- **Testing**: unit + property tests for invariants (e.g., move reversibility, manifoldness).

For comprehensive guidelines on contributing, development environment setup, testing, and project structure, please see [CONTRIBUTING.md](CONTRIBUTING.md).

This includes information about:

- Building and testing the library
- Running benchmarks and performance analysis
- Code style and standards
- Submitting changes and pull requests
- Project structure and development tools

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
