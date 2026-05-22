#!/bin/bash
# Basic CDT simulation example script
# This script demonstrates running a simple CDT simulation with the cdt binary

set -e # Exit on any error

echo "=== Basic CDT Simulation Example ==="
echo

# Build the project in release mode for optimal performance
echo "Building cdt binary..."
cargo build --release

# Check if binary was built successfully
if [ ! -f "./target/release/cdt" ]; then
	echo "Error: cdt binary not found. Build may have failed."
	exit 1
fi

echo "✓ Binary built successfully"
echo

# Run a basic simulation with logging
echo "Running basic CDT simulation..."
echo "Parameters: 4 vertices/slice, 5 timeslices, 1000 MC steps"
echo

RUST_LOG=info ./target/release/cdt \
	--vertices-per-slice 4 \
	--timeslices 5 \
	--temperature 1.0 \
	--steps 1000 \
	--thermalization-steps 100 \
	--measurement-frequency 10 \
	--simulate

echo
echo "✓ Simulation completed successfully!"
echo
echo "Next steps:"
echo "  - Try modifying parameters in this script"
echo "  - Run parameter_sweep.sh for systematic studies"
echo "  - Check docs/CLI_EXAMPLES.md for more advanced usage"
