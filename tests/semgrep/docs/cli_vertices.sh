#!/usr/bin/env bash
# shellcheck disable=SC2215

# ok: causal-triangulations.cli.prefer-vertices-per-slice-in-user-facing-runs
./target/release/cdt --vertices-per-slice 4 --timeslices 8

# ok: causal-triangulations.cli.prefer-vertices-per-slice-in-user-facing-runs
cargo run --bin cdt -- --vertices-per-slice 4 --timeslices 8

# ruleid: causal-triangulations.cli.prefer-vertices-per-slice-in-user-facing-runs
./target/release/cdt --vertices 32 --timeslices 8

# ruleid: causal-triangulations.cli.prefer-vertices-per-slice-in-user-facing-runs
cargo run --bin cdt -- --vertices 32 --timeslices 8

# ruleid: causal-triangulations.cli.prefer-vertices-per-slice-in-user-facing-runs
--vertices 32 \
	--timeslices 8
