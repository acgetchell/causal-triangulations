#!/bin/bash
# Performance testing script for CDT simulations
# This script benchmarks the cdt binary performance across different system sizes

set -e # Exit on any error

echo "=== CDT Performance Testing ==="
echo

# System size configurations to test
declare -a TEST_CONFIGS=(
	"4 5 1000"  # Small: 4 vertices/slice, 5 slices, 1000 steps
	"4 8 2000"  # Medium: 4 vertices/slice, 8 slices, 2000 steps
	"5 10 3000" # Large: 5 vertices/slice, 10 slices, 3000 steps
	"8 15 5000" # Extra Large: 8 vertices/slice, 15 slices, 5000 steps
)

declare -a SIZE_NAMES=("Small" "Medium" "Large" "Extra Large")

# Build optimized binary
echo "Building optimized cdt binary..."
cargo build --release

echo "✓ Binary built successfully"
echo

# Run performance tests
echo "Running performance tests..."
echo

results_file="performance_results.txt"
echo "# CDT Performance Test Results - $(date)" >"$results_file"
echo "# Format: Size | Vertices/Slice | Slices | Steps | Runtime (s) | Memory (MB)" >>"$results_file"
echo

for i in "${!TEST_CONFIGS[@]}"; do
	read -ra config <<<"${TEST_CONFIGS[$i]}"
	vertices_per_slice=${config[0]}
	slices=${config[1]}
	steps=${config[2]}
	size_name=${SIZE_NAMES[$i]}

	echo "Testing $size_name configuration: $vertices_per_slice vertices/slice, $slices slices, $steps steps"

	# Measure execution time and memory usage
	start_time=$(date +%s.%N)

	# Run simulation with minimal logging to reduce I/O overhead
	RUST_LOG=error ./target/release/cdt \
		--vertices-per-slice "$vertices_per_slice" \
		--timeslices "$slices" \
		--steps "$steps" \
		--thermalization-steps $((steps / 10)) \
		--measurement-frequency $((steps / 50)) \
		--simulate \
		>/dev/null 2>&1

	end_time=$(date +%s.%N)
	runtime=$(echo "$end_time - $start_time" | bc -l)

	# Format runtime to 2 decimal places
	runtime_formatted=$(printf "%.2f" "$runtime")

	echo "  ✓ Completed in ${runtime_formatted}s"

	# Log results
	echo "$size_name | $vertices_per_slice | $slices | $steps | $runtime_formatted | N/A" >>"$results_file"
done

echo
echo "=== Performance Test Complete ==="
echo

# Display summary
echo "=== Performance Summary ==="
echo "Configuration  | Runtime (s) | Throughput (steps/s)"
echo "---------------|-------------|-------------------"

i=0
for config_line in "${TEST_CONFIGS[@]}"; do
	read -ra config <<<"$config_line"
	steps=${config[2]}
	size_name=${SIZE_NAMES[$i]}

	# Extract runtime from results file
	runtime=$(grep "^$size_name" "$results_file" | cut -d'|' -f5 | tr -d ' ')

	# Calculate throughput
	if [ "$runtime" != "N/A" ] && [ "$(echo "$runtime > 0" | bc -l)" = "1" ]; then
		throughput=$(echo "scale=1; $steps / $runtime" | bc -l)
		printf "%-14s | %11s | %17.1f\n" "$size_name" "$runtime" "$throughput"
	else
		printf "%-14s | %11s | %17s\n" "$size_name" "$runtime" "N/A"
	fi

	((i++))
done

echo
echo "Results saved to: $results_file"
echo

# Performance analysis recommendations
echo "=== Performance Analysis ==="
echo "• Check if runtime scales linearly with system size"
echo "• Monitor memory usage for large systems"
echo "• Compare with benchmark results from 'cargo bench'"
echo "• Consider optimizations if throughput is below expectations"
echo
echo "Optimization tips:"
echo "• Ensure using release build (--release flag)"
echo "• Reduce measurement frequency for long simulations"
echo "• Profile with 'cargo bench' for detailed performance metrics"
