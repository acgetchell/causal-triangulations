# CDT-RS Command Line Interface Examples

This document provides examples for using the `cdt` binary, the command-line interface for Causal Dynamical Triangulations simulations.

> **Current simulation status:** examples that pass `--simulate` run the Metropolis-Hastings loop over the 2D CDT move kernels. Omit `--simulate` when you only
> want triangulation generation and the initial measurement.

## Basic Usage

The `cdt` binary accepts various command-line arguments to configure and run CDT simulations.

### Quick Start

```bash
# Basic 2D CDT triangulation with default parameters
./target/release/cdt --vertices-per-slice 4 --timeslices 5

# Run with custom simulation settings
./target/release/cdt --vertices-per-slice 4 --timeslices 10 --temperature 1.5 --steps 2000 --simulate
```

## Command Line Arguments

### Required Arguments

- `--vertices-per-slice <N>`: Vertices on each initial spatial slice. Prefer this for regular CDT initial data; the binary computes
  `total vertices = vertices-per-slice × timeslices`.
- `--vertices <N>`: Total initial vertex count. Use this only when the total is already known; it must divide evenly by `--timeslices`.
- `--timeslices <N>`: Number of time slices in the CDT foliation (minimum 1)

### Optional Simulation Parameters

- `--dimension <D>`: Dimensionality (currently only 2, default: 2)
- `--temperature <T>`: Temperature for Metropolis algorithm (default: 1.0)
- `--steps <N>`: Number of Monte Carlo steps (default: 1000)
- `--thermalization-steps <N>`: Thermalization steps before measurements (default: 100)
- `--measurement-frequency <N>`: Take measurement every N steps (default: 10)

### Physics Parameters

- `--coupling-0 <κ₀>`: Coupling constant for vertices (default: 1.0)
- `--coupling-2 <κ₂>`: Coupling constant for triangles (default: 1.0)
- `--cosmological-constant <λ>`: Cosmological constant (default: 0.1)

### Additional Options

- `--simulate`: Request full Monte Carlo simulation (default: false). Omit `--simulate` for triangulation generation and initial measurements only.

## Example Usage Scenarios

### 1. Small Test Simulation

```bash
# Quick triangulation-generation test with minimal parameters
./target/release/cdt --vertices-per-slice 4 --timeslices 2
```

**Expected Output:**

- Creates an 8-vertex, 2-timeslice open-boundary triangulation
- Does not run Monte Carlo steps unless `--simulate` is passed
- Reports the initial triangulation measurement

### 2. Medium-Scale Physics Study

```bash
./target/release/cdt \
  --vertices-per-slice 5 \
  --timeslices 10 \
  --temperature 1.2 \
  --steps 5000 \
  --thermalization-steps 500 \
  --measurement-frequency 25 \
  --simulate
```

**Use Case:** Study phase transitions or scaling behavior

### 3. High-Temperature Simulation

```bash
# High temperature configuration
./target/release/cdt \
  --vertices-per-slice 4 \
  --timeslices 8 \
  --temperature 10.0 \
  --steps 3000 \
  --simulate
```

**Use Case:** Explore classical geometry limit

### 4. Low-Temperature Simulation

```bash
# Low temperature configuration
./target/release/cdt \
  --vertices-per-slice 4 \
  --timeslices 12 \
  --temperature 0.5 \
  --steps 8000 \
  --thermalization-steps 1000 \
  --simulate
```

**Use Case:** Study quantum fluctuations and crumpled phase

### 5. Custom Physics Parameters

```bash
# Modified coupling constants
./target/release/cdt \
  --vertices-per-slice 5 \
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
./target/release/cdt --vertices-per-slice 5 --timeslices 20
```

**Use Case:** Generate initial configurations for other analysis tools

## Advanced Usage Patterns

### Batch Processing with Shell Scripts

Create a script to run parameter sweeps:

```bash
#!/bin/bash
# parameter_sweep.sh

for temp in 0.5 1.0 1.5 2.0 2.5; do
    echo "Running simulation at temperature $temp"
    ./target/release/cdt \
        --vertices-per-slice 4 \
        --timeslices 10 \
        --temperature $temp \
        --steps 2000 \
        --simulate \
        > "results_T${temp}.log" 2>&1
done
```

### Performance Testing

```bash
# Large simulation for performance testing
./target/release/cdt \
  --vertices-per-slice 8 \
  --timeslices 25 \
  --steps 10000 \
  --measurement-frequency 100 \
  --simulate
```

### Logging and Output

Enable detailed logging:

```bash
# Set log level for detailed triangulation output
RUST_LOG=debug ./target/release/cdt --vertices-per-slice 4 --timeslices 5

# Show detailed simulation logging
RUST_LOG=debug ./target/release/cdt --vertices-per-slice 4 --timeslices 5 --simulate

# Log only errors and warnings
RUST_LOG=warn ./target/release/cdt --vertices-per-slice 5 --timeslices 10 --simulate

# Save output to file
./target/release/cdt --vertices-per-slice 4 --timeslices 8 --simulate > simulation.log 2>&1
```

## Expected Output Format

### Successful Run

```text
[INFO] Dimensionality: 2
[INFO] Number of vertices: 20
[INFO] Number of timeslices: 5
[INFO] Topology: OpenBoundary
[INFO] Using trait-based backend system
[INFO] Triangulation created with 20 vertices, <edge-count> edges, <face-count> faces
[INFO] CDT simulation completed successfully
```

### Error Cases

```bash
# Invalid parameters
./target/release/cdt --vertices-per-slice 2 --timeslices 2
# Error: vertices must be >= 4 · timeslices (8) for open-boundary topology

# Unsupported dimension
./target/release/cdt --vertices-per-slice 4 --timeslices 5 --dimension 4
# Error: unsupported dimension
```

## Performance Considerations

### Memory Usage

- Small (≤20 vertices): <10 MB
- Medium (20-100 vertices): 10-100 MB
- Large (100+ vertices): 100+ MB

### Runtime

- Triangulation construction and validation: seconds for small examples
- Full simulation timing with `--simulate`: use benchmarks or representative CLI runs

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
   - Check minimum values (open-boundary vertices per slice ≥ 4, toroidal vertices per slice ≥ 3)
   - Verify dimension is 2

## Integration with Other Tools

### Data Analysis

```bash
# Pipe triangulation-only output to analysis tools
./target/release/cdt --vertices-per-slice 5 --timeslices 10 | \
  python analysis_script.py
```

### Automation

```bash
# Use in makefiles or CI/CD
make run-simulation: 
 ./target/release/cdt --vertices-per-slice $(VERTICES_PER_SLICE) --timeslices $(SLICES)
```

## Help and Documentation

```bash
# Display all available options
./target/release/cdt --help

# Version information  
./target/release/cdt --version
```

This CLI interface provides a way to explore CDT triangulation construction, validate simulation parameters, and run short Monte Carlo simulations from the
command line.
