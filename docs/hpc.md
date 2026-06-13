# HPC Notebook Workflows

This guide covers running the CDT notebook workflow on clusters, including Slurm batch jobs and Open OnDemand Jupyter sessions. For a local first run, start
from the repository root with:

```bash
just notebook-setup
just notebook
```

The `justfile` owns those recipes. Inspect it if you want to see exactly which `uv`, Jupyter, and notebook commands are being run.

## Headless Batch Runs

For clusters, set up the notebook environment on a login node or in a job prologue, then run notebooks headlessly on a compute node:

```bash
just notebook-setup
just notebook-execute
```

If the cluster restricts home-directory caches, put uv's cache on project scratch:

```bash
export UV_CACHE_DIR="$SCRATCH/uv-cache"
just notebook-setup
```

A minimal Slurm script looks like:

```bash
#!/usr/bin/env bash
#SBATCH --job-name=cdt-notebook
#SBATCH --time=00:20:00
#SBATCH --cpus-per-task=2
#SBATCH --mem=4G

set -euo pipefail
cd /path/to/causal-triangulations
export UV_CACHE_DIR="${SCRATCH:-$PWD/target}/uv-cache"
just notebook-setup
just notebook-execute
```

For fully offline compute nodes, run `just notebook-setup` where package downloads are allowed first, then submit jobs with the populated `.venv`,
`uv.lock`, and uv cache available on the cluster filesystem.

## Open OnDemand And Slurm

On Open OnDemand, start a Jupyter session backed by a compute allocation, open a terminal in the session, and launch from the repository root:

```bash
just notebook
```

If the cluster provides a packaged `cdt` binary through a module, point the notebook at it before launching:

```bash
module load causal-triangulations
export CDT_BINARY="$(command -v cdt)"
just notebook
```

For longer runs, use the notebook as the control surface and submit the `cdt` command to Slurm with `sbatch` or `srun`, writing `trace.csv` and
`summary.json` to a run directory on scratch. When the job finishes, load those artifacts back into the notebook for Polars analysis and plotting. This keeps
expensive simulation work in Slurm while preserving the notebook as the interactive experiment record.

A minimal handoff script from an OnDemand terminal or notebook-created run directory looks like:

```bash
RUN_DIR="${SCRATCH:-$PWD/target}/cdt-runs/slurm-demo"
mkdir -p "$RUN_DIR"

cat > "$RUN_DIR/run-cdt.sbatch" <<EOF
#!/usr/bin/env bash
#SBATCH --job-name=cdt-run
#SBATCH --time=00:30:00
#SBATCH --cpus-per-task=2
#SBATCH --mem=4G

set -euo pipefail
cd "$PWD"
cdt \
  --vertices-per-slice 8 \
  --timeslices 16 \
  --topology toroidal \
  --steps 5000 \
  --measurement-frequency 100 \
  --seed 105 \
  --simulate \
  --output-csv "$RUN_DIR/trace.csv" \
  --output-json "$RUN_DIR/summary.json"
EOF

sbatch "$RUN_DIR/run-cdt.sbatch"
```

After the job completes, open `RUN_DIR/trace.csv` and `RUN_DIR/summary.json` from the notebook instead of rerunning the simulation interactively.
