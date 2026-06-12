#!/bin/bash
# Parameter sweep example for CDT simulations
# This script runs multiple simulations across different temperatures
# to inspect acceptance, action, and volume diagnostics

set -e # Exit on any error

echo "=== CDT Parameter Sweep Example ==="
echo

# Configuration
VERTICES_PER_SLICE=4
TIMESLICES=8
STEPS=2000
OUTPUT_DIR="sweep_results"

# Temperature range to sweep
TEMPERATURES=(0.5 0.8 1.0 1.2 1.5 2.0 2.5 3.0)

# Build the project
echo "Building cdt binary..."
cargo build --release

# Create output directory
mkdir -p "$OUTPUT_DIR"
echo "Results will be saved to: $OUTPUT_DIR/"
echo

# Run parameter sweep
echo "Starting parameter sweep over ${#TEMPERATURES[@]} temperature values..."
echo "Fixed parameters: $VERTICES_PER_SLICE vertices/slice, $TIMESLICES timeslices, $STEPS steps"
echo

for temp in "${TEMPERATURES[@]}"; do
	echo "Running simulation at T = $temp"

	# Create output filename
	output_file="${OUTPUT_DIR}/simulation_T${temp}.log"
	success_file="${output_file}.success"
	status_file="${output_file}.status"
	rm -f "$success_file" "$status_file"

	# Run simulation and save output
	if RUST_LOG=info ./target/release/cdt \
		--vertices-per-slice $VERTICES_PER_SLICE \
		--timeslices $TIMESLICES \
		--temperature "$temp" \
		--steps $STEPS \
		--thermalization-steps 200 \
		--measurement-frequency 20 \
		--simulate \
		>"$output_file" 2>&1; then
		printf '0\n' >"$status_file"
		touch "$success_file"
		echo "  ✓ T = $temp completed, saved to $output_file"
	else
		status=$?
		printf '%s\n' "$status" >"$status_file"
		echo "  FAILED: T = $temp exited with status $status; see $output_file"
	fi
done

echo
echo "=== Parameter Sweep Complete ==="
echo "Results saved in: $OUTPUT_DIR/"
echo

# Generate a simple summary
echo "=== Summary ==="
echo "Temperature | Status"
echo "------------|--------"

for temp in "${TEMPERATURES[@]}"; do
	output_file="${OUTPUT_DIR}/simulation_T${temp}.log"
	success_file="${output_file}.success"
	status_file="${output_file}.status"
	if [[ -f "$success_file" ]]; then
		echo "    $temp    | SUCCESS"
	elif [[ -f "$status_file" ]]; then
		status="$(<"$status_file")"
		echo "    $temp    | FAILED (${status})"
	else
		echo "    $temp    | FAILED"
	fi
done

echo
echo "Analysis suggestions:"
echo "  - Plot acceptance rates vs temperature"
echo "  - Compare action and volume diagnostics across temperatures"
echo "  - Repeat with larger systems before drawing scaling conclusions"
echo "  - Use data from $OUTPUT_DIR/ for further analysis"
