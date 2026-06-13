# CDT CLI Examples

This page collects `cdt` command-line patterns for users who want scriptable runs. For the notebook-first beginner path, start with
[`notebooks/00_quickstart.ipynb`](../notebooks/00_quickstart.ipynb).

The examples use the installed `cdt` binary. From a repository clone, build with `cargo build --release` and replace `cdt` with `./target/release/cdt`.

> **Current simulation status:** commands with `--simulate` run the Metropolis-Hastings loop over the 2D CDT move kernels. Omit `--simulate` when you only want
> triangulation generation and the initial measurement.

## Argument Groups

Initial spatial volume can be specified in one of three ways:

- `--vertices-per-slice <N>` with `--timeslices <T>`: regular equal-slice initial data; total vertices are `N × T`.
- `--vertices <N>` with `--timeslices <T>`: total regular initial vertex count; `N` must divide evenly by `T`.
- `--volume-profile <N0,N1,...>`: explicit nonuniform initial spatial volumes. If `--timeslices` is omitted, the profile length sets it.

Core simulation flags:

- `--dimension <D>`: currently only `2`.
- `--topology <open-boundary|toroidal>`: open strip or periodic S¹×S¹ initial data.
- `--steps <N>`: number of Metropolis proposals to evaluate when `--simulate` is set.
- `--thermalization-steps <N>`: early steps excluded from scheduled measurements.
- `--measurement-frequency <N>`: scheduled measurement interval for JSON output.
- `--seed <N>`: reproducible RNG seed.
- `--output-csv <PATH>` and `--output-json <PATH>`: write scalar trace rows and structured summary/final state.

Physics and sampler flags:

- `--temperature <T>`: Metropolis sampler temperature; keep `1.0` unless deliberately studying sampler behavior.
- `--coupling-0 <κ₀>` and `--coupling-2 <κ₂>`: vertex and triangle action couplings.
- `--cosmological-constant <λ>`: edge-count cosmological coupling. In unfixed-volume runs it controls volume growth or shrinkage.

## Tiny Smoke Run

This is the same small run used by the beginner notebook:

```bash
RUST_LOG=info cdt \
  --dimension 2 \
  --vertices-per-slice 4 \
  --timeslices 5 \
  --topology open-boundary \
  --steps 100 \
  --thermalization-steps 10 \
  --measurement-frequency 10 \
  --temperature 1.0 \
  --cosmological-constant 0.46209812037329684 \
  --seed 105 \
  --simulate \
  --output-csv cdt-runs/quickstart/trace.csv \
  --output-json cdt-runs/quickstart/summary.json
```

The command builds a small 1+1-dimensional CDT strip, runs 100 Metropolis-Hastings proposal steps, and writes analysis artifacts under
`cdt-runs/quickstart/`. Parent directories are created automatically.

## Topology Examples

### Open-Boundary Strip

```bash
cdt \
  --vertices-per-slice 4 \
  --timeslices 5 \
  --topology open-boundary \
  --output-json open-boundary-summary.json
```

Open-boundary runs require at least 4 vertices per slice and at least 2 time slices.

### Periodic Toroidal Run

```bash
cdt \
  --vertices-per-slice 8 \
  --timeslices 6 \
  --topology toroidal \
  --steps 200 \
  --thermalization-steps 20 \
  --measurement-frequency 10 \
  --seed 105 \
  --simulate \
  --output-csv toroidal-trace.csv \
  --output-json toroidal-summary.json
```

Toroidal runs require at least 3 vertices per slice and at least 3 time slices. The constructor builds periodic S¹×S¹ initial data and validates topology,
foliation, causality, and simplex classification before simulation.

### Nonuniform Initial Volume Profile

```bash
cdt \
  --volume-profile 4,6,5 \
  --steps 100 \
  --thermalization-steps 10 \
  --measurement-frequency 10 \
  --seed 42 \
  --simulate \
  --output-csv profile-trace.csv \
  --output-json profile-summary.json
```

Use `--volume-profile` for explicit initial slice volumes `N(t)`. The run remains an unfixed-volume run unless a future volume-fixing action term is added
explicitly.

## Output Patterns

### CSV And JSON For Analysis

```bash
cdt \
  --vertices-per-slice 5 \
  --timeslices 10 \
  --steps 200 \
  --thermalization-steps 20 \
  --measurement-frequency 10 \
  --simulate \
  --output-csv trace.csv \
  --output-json summary.json
```

The CSV trace records one row per completed Metropolis step. The JSON file records configuration, summary statistics, move/proposal statistics, scheduled
measurements, and the final triangulation state.

### Logging

```bash
RUST_LOG=info cdt --vertices-per-slice 4 --timeslices 5 --simulate
RUST_LOG=debug cdt --vertices-per-slice 4 --timeslices 5 --simulate
RUST_LOG=warn cdt --vertices-per-slice 5 --timeslices 10 --simulate
```

Use `info` for normal run summaries, `debug` for detailed diagnostic output, and `warn` when you only want warnings and errors.

## Batch Runs

For maintained shell-script examples, see [`examples/scripts/README.md`](../examples/scripts/README.md). A minimal parameter sweep looks like:

```bash
for temp in 0.5 1.0 1.5 2.0; do
  cdt \
    --vertices-per-slice 4 \
    --timeslices 8 \
    --temperature "$temp" \
    --steps 2000 \
    --simulate \
    --output-csv "sweep_T${temp}.csv" \
    --output-json "sweep_T${temp}.json"
done
```

## Performance Runs

Use release builds for representative timings. For benchmark-quality comparisons, use [benches/README.md](../benches/README.md) and
[performance-testing.md](performance-testing.md) rather than ad hoc CLI timings.

```bash
cdt \
  --vertices-per-slice 8 \
  --timeslices 25 \
  --steps 10000 \
  --measurement-frequency 100 \
  --simulate
```

## Troubleshooting

`cdt: command not found`

: Install with `cargo install causal-triangulations`, ensure `~/.cargo/bin` is on `PATH`, or use `./target/release/cdt` from a local clone.

Parameter validation errors

: Check minimum topology requirements: open-boundary runs need at least 4 vertices per slice, toroidal runs need at least 3 vertices per slice and at least
  3 time slices. `--dimension` must be `2`.

Unexpected volume growth or shrinkage

: Current simulations are grand-canonical, unfixed-volume runs. Adjust `--cosmological-constant`, reduce the run length, or start from a smaller initial
  volume while tuning.

Slow runs

: Use a release binary, reduce `--vertices-per-slice`, reduce `--timeslices`, increase `--measurement-frequency`, or use the Criterion benchmarks to isolate
  the bottleneck.
