# Quick Start

This guide is for physicists who want to run a small 1+1 CDT calculation without first learning Rust. It assumes macOS or Linux and uses the `cdt` command-line
binary.

The current release is a validated 1+1 CDT foundation. It can build open-boundary and toroidal initial triangulations, run the Metropolis move loop, and write
CSV/JSON output for later analysis. It does not yet provide production ensemble-analysis tooling, automatic λ scans, or higher-dimensional CDT.

## Install

Prebuilt release binaries are planned in [#169](https://github.com/acgetchell/causal-triangulations/issues/169). When those assets exist, download the archive
for your platform from [GitHub Releases](https://github.com/acgetchell/causal-triangulations/releases), unpack it, and put the `cdt` binary somewhere on your
`PATH`. Until then, install the binary with Cargo:

```bash
cargo install causal-triangulations
```

Then check that the binary is on your path:

```bash
cdt --help
```

If you are working from a local clone instead of the published crate, build the release binary and replace `cdt` below with `./target/release/cdt`:

```bash
cargo build --release
./target/release/cdt --help
```

## First Run

Copy and paste this command:

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

The command builds a small 1+1-dimensional CDT strip, runs 100 Metropolis-Hastings proposal steps, and writes results under `cdt-runs/quickstart/`. Parent
directories are created automatically.

The physics and algorithm parameters are:

- `--dimension 2`: two-dimensional spacetime, i.e. 1 spatial dimension plus 1 time dimension. Higher CDT dimensions are roadmap work.
- `--vertices-per-slice 4`: the initial spatial volume on each time slice.
- `--timeslices 5`: the number of discrete proper-time slices in the foliation.
- `--topology open-boundary`: an open strip in time. Use `toroidal` for periodic S¹×S¹ initial data.
- `--steps 100`: the number of Metropolis proposals to evaluate.
- `--thermalization-steps 10`: early steps excluded from the measurement schedule.
- `--measurement-frequency 10`: record scheduled measurements every ten steps after thermalization.
- `--temperature 1.0`: the Metropolis temperature in the sampler acceptance rule. This is an algorithmic tempering parameter, not a physical heat bath.
- `--cosmological-constant 0.46209812037329684`: the edge-count cosmological coupling λ. In the current unfixed-volume ensemble it controls volume growth or
  shrinkage.
- `--seed 105`: fixes the random number streams so the run can be repeated exactly on the same version.
- `--simulate`: run the Markov chain after constructing the initial triangulation.
- `--output-csv` and `--output-json`: write a scalar trace and a structured summary/final state.

## What You Should See

With `RUST_LOG=info`, a successful run prints lines like this. Timestamps are omitted here:

```text
[INFO  causal_triangulations] Dimensionality: 2
[INFO  causal_triangulations] Number of vertices: 20
[INFO  causal_triangulations] Number of timeslices: 5
[INFO  causal_triangulations] Topology: OpenBoundary
[INFO  causal_triangulations] Wrote trace CSV to cdt-runs/quickstart/trace.csv
[INFO  causal_triangulations] Wrote simulation JSON summary to cdt-runs/quickstart/summary.json
[INFO  cdt] CDT simulation completed successfully
```

The two output files are:

- `cdt-runs/quickstart/trace.csv`: one row per completed Metropolis step. The fixed columns include `chain_id`, `step`, `accepted`, `proposed`, and
  `log_prob`; CDT adds the current action, vertex/edge/triangle counts, move-family code, action-delta diagnostics, seed halves, and volume-profile columns.
- `cdt-runs/quickstart/summary.json`: run configuration, summary statistics, final triangulation data, move/proposal statistics, and scheduled measurements.

The most useful quick checks are:

- `accepted`: whether the proposed transition changed the chain state.
- `proposed`: whether the step had a concrete local proposal; false entries are self-loops such as no available site for the selected move family.
- `action`: the Regge action value used by the sampler.
- `vertices`, `edges`, `triangles`: the current lattice size. In this release, volume-changing moves are allowed and no volume-fixing term is applied.
- `volume_profile_*`: spatial slice volumes at the recorded step, padded with zeros for rectangular CSV output.

Resumable CDT/MCMC checkpoints are available from the Rust library API and demonstrated in `examples/output_and_checkpoint.rs`. The quickstart CLI command
writes trace and summary files, but it does not yet write a separate checkpoint file.

## Tweak Physics

Common changes map directly to CLI flags:

- Increase the initial lattice size with `--vertices-per-slice` and `--timeslices`. For nonuniform initial slice volumes, use `--volume-profile 4,6,5`
  instead of `--vertices-per-slice` and let the profile length set the time-slice count.
- Change boundary conditions with `--topology open-boundary` or `--topology toroidal`. Open-boundary runs need at least 4 vertices per slice; toroidal runs
  need at least 3 vertices per slice and at least 3 time slices.
- Tune the unfixed-volume ensemble with `--cosmological-constant`. Values too far from the useful finite-volume window may drive the run toward
  minimum-volume configurations or rapid growth.
- Change the action couplings with `--coupling-0` and `--coupling-2`. In pure 1+1 gravity these terms are topological at fixed topology, so the defaults are
  zero.
- Run longer chains with `--steps`, raise `--thermalization-steps` for a longer burn-in period, and use `--measurement-frequency` to control scheduled
  measurements in the JSON output.
- Use `--temperature` for tempered sampler diagnostics. Keep `1.0` unless you are deliberately studying sampler behavior.
- Use `--seed` for reproducible runs. Change the seed to sample a different random trajectory.
- Use distinct `--output-csv` and `--output-json` paths for each run so results are not overwritten.

## Troubleshooting

`cdt: command not found`

: Cargo usually installs binaries into `~/.cargo/bin`. Add that directory to your `PATH`, or use the local release binary path `./target/release/cdt` from a
  repository clone.

`cargo: command not found`

: Install Rust with [rustup](https://rustup.rs/), then reopen your terminal and run `cargo --version`.

`Unsupported dimension`

: The CLI currently supports only `--dimension 2`, meaning 1+1 CDT.

`vertices must be >= ...`

: The requested initial triangulation is too small for the chosen topology. Increase `--vertices-per-slice`, or use `--topology open-boundary` for the
  smallest quick checks.

The run grows or shrinks quickly

: This is expected in an unfixed-volume ensemble when λ is outside a useful finite-volume window. Shorten the run, start from a smaller lattice, and adjust
  `--cosmological-constant`.

The CSV has many rows but the JSON has fewer measurements

: CSV trace output records every completed Metropolis step. Scheduled JSON measurements follow `--thermalization-steps` and `--measurement-frequency`.

The command is slow

: Use the release binary, reduce `--vertices-per-slice`, reduce `--timeslices`, or reduce `--steps`.
