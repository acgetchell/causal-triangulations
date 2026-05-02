# CDT-RS Command Line Interface Examples

This document provides examples for using the `cdt` binary, the command-line interface for Causal Dynamical Triangulations simulations.

> **Current simulation status:** examples that pass `--simulate` are unsupported until the real CDT move kernels in PRs #55/#56 are merged. Today those commands validate configuration and initial triangulation construction, then fail with `UnsupportedOperation` from `MetropolisAlgorithm::run`. Run the examples without `--simulate` for triangulation generation and initial measurement output.

## Basic Usage

The `cdt` binary accepts various command-line arguments to configure and run CDT simulations.

### Quick Start

```bash
# Basic 2D CDT triangulation with default parameters
./target/release/cdt --vertices 10 --timeslices 5

# Build with custom simulation settings; omit --simulate while moves are pending
./target/release/cdt --vertices 20 --timeslices 10 --temperature 1.5 --steps 2000
```

## Command Line Arguments

### Required Arguments

- `--vertices <N>`: Number of vertices in the triangulation (minimum 3)
- `--timeslices <N>`: Number of time slices in the CDT foliation (minimum 1)

### Optional Simulation Parameters

- `--dimension <D>`: Dimensionality (2-3, default: 2)
- `--temperature <T>`: Temperature for Metropolis algorithm (default: 1.0)
- `--steps <N>`: Number of Monte Carlo steps (default: 1000)
- `--thermalization-steps <N>`: Thermalization steps before measurements (default: 100)
- `--measurement-frequency <N>`: Take measurement every N steps (default: 10)

### Physics Parameters

- `--coupling-0 <κ₀>`: Coupling constant for vertices (default: 1.0)
- `--coupling-2 <κ₂>`: Coupling constant for triangles (default: 1.0)
- `--cosmological-constant <λ>`: Cosmological constant (default: 0.1)

### Additional Options

- `--simulate`: Request full Monte Carlo simulation (default: false). **Unsupported today:** until real CDT move kernels land in PRs #55/#56, this raises `UnsupportedOperation` from `MetropolisAlgorithm::run` instead of returning a zero-move simulation. Omit `--simulate` for triangulation generation and initial measurements.

## Example Usage Scenarios

### 1. Small Test Simulation

```bash
# Quick triangulation-generation test with minimal parameters
./target/release/cdt --vertices 5 --timeslices 2
```

**Expected Output:**

- Creates a 5-vertex, 2-timeslice triangulation
- Does not run Monte Carlo steps unless `--simulate` is passed
- Reports the initial triangulation measurement

### 2. Medium-Scale Physics Study

> **Unsupported with `--simulate`:** the physics-study examples in this section currently raise `UnsupportedOperation` until the CDT move kernels in PRs #55/#56 are merged. Omit `--simulate` to generate the initial triangulation, or wait for those PRs before treating these as Monte Carlo studies.

```bash
# Currently unsupported as a simulation; remove --simulate to build only
./target/release/cdt \
  --vertices 50 \
  --timeslices 10 \
  --temperature 1.2 \
  --steps 5000 \
  --thermalization-steps 500 \
  --measurement-frequency 25 \
  --simulate
```

**Use Case:** Study phase transitions or scaling behavior

### 3. High-Temperature Simulation

> **Unsupported with `--simulate`:** this currently reaches the `MetropolisAlgorithm::run` guardrail. Remove `--simulate` for triangulation generation until PRs #55/#56 land.

```bash
# High temperature configuration; --simulate is currently unsupported
./target/release/cdt \
  --vertices 30 \
  --timeslices 8 \
  --temperature 10.0 \
  --steps 3000 \
  --simulate
```

**Use Case:** Explore classical geometry limit

### 4. Low-Temperature Simulation

> **Unsupported with `--simulate`:** this currently raises `UnsupportedOperation` instead of running a Monte Carlo chain. Remove `--simulate` or wait for PRs #55/#56.

```bash
# Low temperature configuration; --simulate is currently unsupported
./target/release/cdt \
  --vertices 25 \
  --timeslices 12 \
  --temperature 0.5 \
  --steps 8000 \
  --thermalization-steps 1000 \
  --simulate
```

**Use Case:** Study quantum fluctuations and crumpled phase

### 5. Custom Physics Parameters

> **Unsupported with `--simulate`:** custom couplings are validated, but full simulation still stops at the unsupported-operation guardrail until PRs #55/#56 provide real moves.

```bash
# Modified coupling constants; --simulate is currently unsupported
./target/release/cdt \
  --vertices 40 \
  --timeslices 8 \
  --coupling-0 0.8 \
  --coupling-2 1.2 \
  --cosmological-constant 0.05 \
  --simulate
```

**Use Case:** Explore modified gravity or different action formulations

### 6. Triangulation-Only Mode

```bash
# Generate triangulation without running Monte Carlo simulation
./target/release/cdt --vertices 100 --timeslices 20
```

**Use Case:** Generate initial configurations for other analysis tools

## Advanced Usage Patterns

### Batch Processing with Shell Scripts

> **Unsupported with `--simulate`:** batch sweeps that include `--simulate` currently collect the same `UnsupportedOperation` error for each run. Remove `--simulate` to batch-generate initial triangulations, or wait for PRs #55/#56 before using this for simulation sweeps.

Create a script to run parameter sweeps:

```bash
#!/bin/bash
# parameter_sweep.sh

for temp in 0.5 1.0 1.5 2.0 2.5; do
    echo "Building triangulation at temperature $temp"
    ./target/release/cdt \
        --vertices 30 \
        --timeslices 10 \
        --temperature $temp \
        --steps 2000 \
        > "results_T${temp}.log" 2>&1
done
```

### Performance Testing

> **Unsupported with `--simulate`:** full simulation performance testing is blocked on PRs #55/#56. Run without `--simulate` for construction/validation timing, or use the benchmark suite's `metropolis_errors` group to measure the current guardrail.

```bash
# Large triangulation construction for performance testing
./target/release/cdt \
  --vertices 200 \
  --timeslices 25 \
  --steps 10000 \
  --measurement-frequency 100
```

### Logging and Output

> **Unsupported with `--simulate`:** logging examples that include `--simulate` currently log the `UnsupportedOperation` guardrail after configuration and triangulation setup. Remove `--simulate` for successful triangulation-only runs.

Enable detailed logging:

```bash
# Set log level for detailed triangulation output
RUST_LOG=debug ./target/release/cdt --vertices 10 --timeslices 5

# Show the current unsupported-operation guardrail explicitly
RUST_LOG=debug ./target/release/cdt --vertices 10 --timeslices 5 --simulate

# Log only errors and warnings
RUST_LOG=warn ./target/release/cdt --vertices 50 --timeslices 10 --simulate

# Save output to file
./target/release/cdt --vertices 25 --timeslices 8 --simulate > simulation.log 2>&1
```

## Expected Output Format

### Successful Run

> **Successful runs currently omit `--simulate`:** commands with `--simulate` raise `UnsupportedOperation` until PRs #55/#56 merge.

```text
[INFO] Dimensionality: 2
[INFO] Number of vertices: 10
[INFO] Number of timeslices: 5
[INFO] Topology: OpenBoundary
[INFO] Using trait-based backend system
[INFO] Triangulation created with 10 vertices, <edge-count> edges, <face-count> faces
[INFO] CDT simulation completed successfully
```

### Unsupported `--simulate` Run

```text
[ERROR] CDT simulation failed: Unsupported operation [MetropolisAlgorithm::run]: real CDT ergodic moves are not implemented yet (#55); refusing to return a zero-move simulation result
```

### Error Cases

```bash
# Invalid parameters
./target/release/cdt --vertices 2 --timeslices 1
# Error: vertices must be >= 3

# Unsupported dimension
./target/release/cdt --vertices 10 --timeslices 5 --dimension 4
# Error: unsupported dimension
```

## Performance Considerations

### Memory Usage

- Small (≤20 vertices): <10 MB
- Medium (20-100 vertices): 10-100 MB
- Large (100+ vertices): 100+ MB

### Runtime

- Triangulation construction and validation: seconds for small examples
- Full simulation timing with `--simulate`: unsupported until PRs #55/#56 merge
- Physics studies and large simulation runs: wait for the real CDT move kernels

### Optimization Tips

1. **Use release builds** for performance: `cargo build --release`
2. **Adjust measurement frequency** for long runs
3. **Monitor memory** for large vertex counts
4. **Use appropriate thermalization** (typically 10-20% of total steps)

## Troubleshooting

### Common Issues

1. **Binary not found**

   ```bash
   cargo build --release
   ./target/release/cdt --help
   ```

2. **Insufficient memory**
   - Reduce vertex count or steps
   - Monitor system resources

3. **Slow performance**
   - Ensure using release build
   - Check system load
   - Consider reducing measurement frequency

4. **Parameter validation errors**
   - Check minimum values (vertices ≥ 3, timeslices ≥ 1)
   - Verify dimension is 2 or 3

## Integration with Other Tools

### Data Analysis

```bash
# Pipe triangulation-only output to analysis tools
./target/release/cdt --vertices 50 --timeslices 10 | \
  python analysis_script.py
```

### Automation

```bash
# Use in makefiles or CI/CD
make run-simulation: 
 ./target/release/cdt --vertices $(VERTICES) --timeslices $(SLICES)
```

## Help and Documentation

```bash
# Display all available options
./target/release/cdt --help

# Version information  
./target/release/cdt --version
```

This CLI interface provides a way to explore CDT triangulation construction and validate simulation parameters from the command line while full Monte Carlo simulation remains blocked on PRs #55/#56.
