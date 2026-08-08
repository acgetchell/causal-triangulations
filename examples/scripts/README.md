# CLI Example Scripts

This directory contains maintained shell scripts for common `cdt` command-line workflows. For a first manual run, see
[`notebooks/00_quickstart.ipynb`](../../notebooks/00_quickstart.ipynb). For individual CLI patterns, see
[`docs/cli-examples.md`](../../docs/cli-examples.md).

Commands that pass `--simulate` run the 2D CDT Metropolis-Hastings loop. Remove `--simulate` when you only want triangulation construction and the initial
measurement.

## Scripts

### `basic_simulation.sh`

Runs a small open-boundary simulation with logging enabled.

```bash
./examples/scripts/basic_simulation.sh
```

Use this to check that the release binary builds and the CLI can run a short simulation.

### `parameter_sweep.sh`

Runs a small temperature sweep and writes per-temperature logs to `sweep_results/`.

```bash
./examples/scripts/parameter_sweep.sh
```

Use this as a starting point for acceptance, action, and volume diagnostics. Temperature rescales the complete action through `exp(-S/T)`, so changing it
changes the sampled ensemble even when it is not interpreted as a physical heat bath.

### `performance_test.sh`

Runs CLI timing checks across several system sizes and writes `performance_results.txt`.

```bash
./examples/scripts/performance_test.sh
```

Use this for quick command-level scaling checks. For regression-quality benchmarking, use `just bench-ci`, `just perf-check`, and the Criterion suites described
in [`benches/README.md`](../../benches/README.md) and [`docs/performance-testing.md`](../../docs/performance-testing.md).

## Requirements

- Bash or zsh
- Rust toolchain
- `bc` for `performance_test.sh`
- Project dependencies available to Cargo

The scripts build the release binary before running:

```bash
cargo build --release
```

## Customization

Each script keeps editable parameters near the top. Common knobs are:

```bash
VERTICES_PER_SLICE=4
TIMESLICES=5
STEPS=1000
TEMPERATURE=1.0
```

For sweeps, edit the temperature list and fixed lattice parameters:

```bash
TEMPERATURES=(0.5 0.8 1.0 1.2 1.5 2.0 2.5 3.0)
VERTICES_PER_SLICE=4
TIMESLICES=8
STEPS=2000
```

For performance checks, edit the size tuples:

```bash
TEST_CONFIGS=(
    "10 5 1000"
    "20 8 2000"
    "50 10 3000"
)
```

## Outputs

- `sweep_results/`: one log per sweep point
- `performance_results.txt`: command-level timing summary
- Terminal logs from `RUST_LOG=info`

Typical successful logs include dimensionality, vertex count, time-slice count, topology, triangulation construction, output-file writes when configured, and
completion status.

## Troubleshooting

Permission denied:

```bash
chmod +x examples/scripts/*.sh
```

`bc: command not found` on macOS:

```bash
brew install bc
```

Slow or memory-heavy runs:

- Use the release build; the scripts already do this.
- Reduce vertices per slice, time slices, or steps.
- Increase measurement frequency for long simulations.
- Prefer Criterion benchmarks for repeatable performance comparisons.
