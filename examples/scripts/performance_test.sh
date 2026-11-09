#!/usr/bin/env bash
# Compatibility entry point for the declared exploratory CLI workloads.
set -euo pipefail

if (($# != 1)); then
	printf 'Usage: %s OUTPUT.json (choose a new output path for each run)\n' "$0" >&2
	exit 2
fi

exec just performance-command-time "$1"
