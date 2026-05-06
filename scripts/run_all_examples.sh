#!/usr/bin/env bash
set -euo pipefail

# Error handling function
error_exit() {
	local message="$1"
	local code="${2:-1}"
	echo "ERROR: $message" >&2
	exit "$code"
}

# Help function
show_help() {
	cat <<EOF
run_all_examples.sh - Run all examples in the causal-triangulations project

USAGE:
    ./scripts/run_all_examples.sh [OPTIONS]

DESCRIPTION:
    Automatically discovers and runs Cargo examples in examples/:
      - examples/<name>.rs
      - examples/<name>/main.rs
    All examples run in release mode (--release).

OPTIONS:
    -h, --help       Show this help message and exit
    --validate       Require stable output markers for known examples

EXAMPLES:
    # Run all examples
    ./scripts/run_all_examples.sh

    # Run all examples and validate stable output markers
    ./scripts/run_all_examples.sh --validate

    # Show help
    ./scripts/run_all_examples.sh --help

NOTES:
    - All examples run in release mode for better performance
    - Examples are discovered automatically from the examples/ directory
    - Output is shown in real-time as examples execute
    - Script exits with error code if any example fails
    - Set EXAMPLE_TIMEOUT to bound per-example runtime (supports units like 30s, 5m; default 600s)
    - On macOS, install coreutils and ensure gtimeout is available (auto-detected)

SEE ALSO:
    examples/README.md - Detailed documentation for each example
    cargo run --example <name> -- Run a specific example manually
EOF
}

# Script to run all examples in the causal-triangulations project

VALIDATE_OUTPUT=false

# Parse command line arguments
for arg in "$@"; do
	case $arg in
	-h | --help)
		show_help
		exit 0
		;;
	--validate)
		VALIDATE_OUTPUT=true
		;;
	*)
		error_exit "Unknown option: $arg. Use --help for usage information."
		;;
	esac
done

# Dependency checking function
check_dependencies() {
	# Array of required commands
	local required_commands=("cargo" "find" "sort")

	# Check each required command
	for cmd in "${required_commands[@]}"; do
		if ! command -v "$cmd" >/dev/null 2>&1; then
			error_exit "$cmd is required but not found. Please install it to proceed."
		fi
	done
}

# Run dependency checks
check_dependencies

# Find project root (directory containing Cargo.toml)
PROJECT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && cd .. && pwd)

# Ensure we're executing from the project root
cd "${PROJECT_ROOT}"

echo "Running all examples for causal-triangulations project..."
echo "=============================================="

# Discover Cargo examples deterministically:
# - Top-level files: examples/foo.rs           -> example name "foo"
# - Nested dirs:    examples/bar/main.rs       -> example name "bar"
all_examples=()
if [[ ! -d "${PROJECT_ROOT}/examples" ]]; then
	error_exit "Examples directory not found at ${PROJECT_ROOT}/examples"
fi
example_names=$(
	{
		# top-level *.rs files
		while IFS= read -r -d '' f; do basename "$f" .rs; done \
			< <(find "${PROJECT_ROOT}/examples" -maxdepth 1 -type f -name '*.rs' -print0)
		# nested example directories with main.rs
		while IFS= read -r -d '' f; do basename "$(dirname "$f")"; done \
			< <(find "${PROJECT_ROOT}/examples" -mindepth 2 -maxdepth 2 -type f -name 'main.rs' -print0)
	} | LC_ALL=C sort -u
)
# Load names into array
while IFS= read -r name; do
	[[ -n "$name" ]] && all_examples+=("$name")
done <<<"$example_names"

# Guard against zero discovered examples
if [ ${#all_examples[@]} -eq 0 ]; then
	error_exit "No examples found under ${PROJECT_ROOT}/examples"
fi

TIMEOUT_CMD=""
if command -v timeout >/dev/null 2>&1; then
	TIMEOUT_CMD="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
	TIMEOUT_CMD="gtimeout"
fi

run_cargo_example() {
	local example="$1"

	if [[ -n "$TIMEOUT_CMD" ]]; then
		DURATION="${EXAMPLE_TIMEOUT:-600s}"
		# If DURATION has no unit suffix, assume seconds
		case "$DURATION" in *[a-zA-Z]) ;; *) DURATION="${DURATION}s" ;; esac
		"$TIMEOUT_CMD" --preserve-status --signal=TERM --kill-after=10s "$DURATION" \
			cargo run --release --example "$example"
	else
		cargo run --release --example "$example"
	fi
}

require_marker() {
	local example="$1"
	local output="$2"
	local marker="$3"

	case "$output" in
	*"$marker"*) ;;
	*)
		error_exit "Example $example output did not contain required marker: $marker"
		;;
	esac
}

validate_example_output() {
	local example="$1"
	local output="$2"

	case "$example" in
	basic_cdt)
		require_marker "$example" "$output" "Simulation completed!"
		require_marker "$example" "$output" "Example completed successfully!"
		;;
	observables)
		require_marker "$example" "$output" "Initial volume profile"
		require_marker "$example" "$output" "Final Hausdorff-dimension estimate"
		require_marker "$example" "$output" "Final spectral-dimension estimate"
		;;
	output_and_checkpoint)
		require_marker "$example" "$output" "CSV output rows"
		require_marker "$example" "$output" "JSON summary measurements"
		require_marker "$example" "$output" "Resumed MCMC checkpoint steps"
		require_marker "$example" "$output" "Output and checkpoint example completed successfully"
		;;
	find_good_seeds)
		require_marker "$example" "$output" "SEED VALIDATION"
		require_marker "$example" "$output" "ADDITIONAL SEED TESTING"
		;;
	*)
		echo "No stable output markers configured for $example; success-only validation applied."
		;;
	esac
}

# Run all examples
for example in "${all_examples[@]}"; do
	echo "=== Running $example ==="
	if [[ "$VALIDATE_OUTPUT" == true ]]; then
		if output=$(run_cargo_example "$example" 2>&1); then
			printf '%s\n' "$output"
			validate_example_output "$example" "$output"
		else
			printf '%s\n' "$output"
			error_exit "Example $example failed!"
		fi
	else
		run_cargo_example "$example" || error_exit "Example $example failed!"
	fi
done

echo
echo "=============================================="
if [[ "$VALIDATE_OUTPUT" == true ]]; then
	echo "All examples completed successfully with validated output markers!"
else
	echo "All examples completed successfully!"
fi
