# Running external 1+1 validation

- [Run one chain](#run-one-chain)
- [Output contract](#output-contract)
- [Failure behavior](#failure-behavior)
- [Test the release candidate](#test-the-release-candidate)

`cdt-validation` runs one intrinsic periodic 1+1 chain with the conventional policy used in the
[finite-volume transfer-matrix comparison](finite-volume-validation.md#sampling-protocol). The default `cli` feature includes the executable
and the Rust entry points in `causal_triangulations::prelude::simulation::validation`.

## Run one chain

Use the supplier lockfile. After release, install the exact published version with
`cargo install --locked causal-triangulations --version '=0.2.0' --bin cdt-validation`. For an unpacked release candidate, use
`cargo build --locked --release --bin cdt-validation` in the unpacked crate.

Save the following as `flat.json`.

```json
{
  "contract_version": "cdt/periodic-1p1-validation/v1",
  "topology": "periodic_time_spatial_circle",
  "time_slices": 4,
  "minimum_loop_length": 3,
  "initial_spatial_loop_lengths": [4, 4, 4, 4],
  "action": {
    "convention": "regge_counts_v1",
    "coupling_0": 0.0,
    "coupling_2": 0.0,
    "edge_cosmological_constant": 0.46209812037329684
  },
  "temperature": 1.0,
  "move_policy": "uniform_move_family",
  "volume_fixing": { "kind": "quadratic_triangles", "target_n2": 32, "strength": 0.05 },
  "warmup_steps": 16384,
  "measurement_cadence": 16,
  "measurements": 64,
  "run_id": "example",
  "chain_id": "flat-0",
  "seed": "320101"
}
```

```bash
cdt-validation run --config flat.json --output flat-bundle
cdt-validation verify --output flat-bundle
```

From an unpacked crate, run the local executable through Cargo. Supply the paths to your configuration and new bundle directory.

```bash
cargo run --locked --release --bin cdt-validation -- run --config flat.json --output flat-bundle
cargo run --locked --release --bin cdt-validation -- verify --output flat-bundle
```

Each key is required. The runner rejects unknown or duplicate keys. The output directory must be new and its parent must exist.

| Field | Valid values and interpretation |
| --- | --- |
| Contract, topology, conventions and move policy | Use the literal strings above. Both dimensions are periodic, with labeled time slices and spatial circles. |
| Time and start profile | At least three time slices. Include one integer loop length per slice, in time order. Each length is at least three. |
| Target | `target_n2` is a positive even unsigned 32-bit integer equal to twice the sum of the initial loop lengths. |
| Action and bias | The action is `-coupling_0 * n0 - coupling_2 * n2 + edge_cosmological_constant * n1 + strength * (n2 - target_n2)^2`. Couplings are finite, strength is finite and nonnegative, and the target checks the numerical range. |
| Temperature | Positive finite temperature in the supported numerical range. The intrinsic target divides the Boltzmann weight by the product of the loop lengths to account for the marked spatial origins. |
| Run clock | Warmup is nonnegative. Cadence and measurement count are positive. Their product plus warmup fits in an unsigned 32-bit integer. |
| Identity and seed | `run_id` and `chain_id` are nonblank strings. `seed` is an unsigned 64-bit decimal string with no sign or leading zeros. The runner seeds one Xoshiro256++ stream with `seed_from_u64`. |

For the other conditions, set `(time_slices, target_n2)` to `(4, 40)`, `(4, 48)`, or `(8, 64)`. A flat start has each length equal
to `target_n2 / (2 * time_slices)`. For a peaked start, begin with length three in each slice and put the excess on the selected slice.
The example at 32 triangles gives `[7, 3, 3, 3]` for a peak on slice zero. Use the seed ranges, peak rotations and budgets in the
[finite-volume report](finite-volume-validation.md#sampling-protocol) to reproduce its protocol. The example uses a bounded measurement budget.

## Output contract

Contract `cdt/periodic-1p1-validation/v1` uses artifact schema `1`. A bundle contains four UTF-8 files with LF line endings.

| File | Contents |
| --- | --- |
| `configuration.json` | The validated configuration as one compact JSON object followed by LF. Input property order and whitespace do not affect its digest. |
| `measurements.jsonl` | One compact JSON object and LF at each scheduled step. Fields are `step`, `n0`, `n1`, `n2`, and `spatial_loop_lengths`. Counts are unsigned integers. |
| `transitions.csv` | Header `outcome` and one row per proposal step, including warmup. The first data row is step 1. Values are `A` (accepted), `R` (concrete proposal rejected), and `N` (no proposal). |
| `metadata.json` | One JSON object and LF written after the other files are flushed. Its presence marks completion. |

Measurement `i`, counted from one, follows proposal step `warmup_steps + i * measurement_cadence`. `n0`, `n1`, and `n2` are the geometry
counts for vertices, edges, and triangles. The vector gives the number of spacelike edges in each indexed spatial circle. A closed torus
has `n0 = sum(spatial_loop_lengths)`, `n1 = 3 * n0`, and `n2 = 2 * n0`. The triangle count in a slab is the sum of its two bounding loop lengths.

`R` and `N` are self-loops. An accepted proposal can leave the measured profile unchanged. Keep every scheduled row and its step for dependence
analysis. To form the conditional histogram, select rows with `n2 == target_n2` offline and count the labeled profiles, including repeated visits.
The identifier `periodic-labelled-inverse-automorphism/v1` refers to the [exact periodic ensemble](finite-volume-validation.md).

| Metadata field | Contents |
| --- | --- |
| `contract_version`, `artifact_schema_version` | The string and unsigned integer given above. |
| `configuration` | The complete normalized input, including run and chain identity, start profile, and seed. |
| `configuration_sha256`, `measurements_sha256`, `transitions_sha256` | Lowercase hex SHA-256 of the corresponding file bytes, including the final LF. Hash the file directly. |
| `oracle_compatibility`, `seed_policy` | `periodic-labelled-inverse-automorphism/v1` and `xoshiro256plusplus-seed-from-u64-single-stream/v1`. |
| `build` | Strings `crate_version` (also the executable version), `rustc` (verbose compiler identity), `target`, `profile` (Cargo profile family), `opt_level`, `debug`, and `release_lock_sha256`. The object `release_lock_dependencies` contains one nonblank supplier lock version each for `delaunay`, `la-stack`, `markov-chain-monte-carlo`, and `rand`. |
| `timing_ns` | Unsigned integer nanoseconds. `initialization` covers state, chain, proposal and RNG setup. `warmup_transition` and `sampling_transition` sum time inside individual sampler calls. Measurements, serialization, hashing, and external tracking happen outside these intervals. |

The supplier lock identifies the dependencies of the executable built with `--locked`. When embedding the Rust runner, retain the
embedding project's Cargo lock and binary identity as well. The orchestrator can record the package and executable digest, Cargo version,
platform, hardware and container identity alongside the bundle.

Repeating the same configuration and seed with the same build and runtime platform reproduces the configuration, measurement, and
transition bytes. Timing is measured afresh. Contract v1 supports whole-chain restart from the seed.

## Failure behavior

`run` and `verify` print no output on success. On failure they write one JSON object to stderr with `contract_version`, `code`, and
human-readable `message`. A failed transition also reports `step`.

| Exit status | Code |
| ---: | --- |
| 0 | Success |
| 2 | `invalid_arguments` or `invalid_configuration` |
| 3 | `chain_initialization_failed` or `transition_failed` |
| 4 | `artifact_io_failed` or `invalid_bundle` |
| 5 | `incompatible_schema` |

Verify before using a bundle. Verification checks schema, configuration, digests, transition count, cadence, and the torus count identities.
It also checks that each interval with zero accepted proposals preserves the previous profile.
The verifier accepts horizontal JSON whitespace in measurement rows as long as each row fits within `160 + 21 * time_slices` bytes, including LF.
A killed process or failed write can leave partial files without `metadata.json`. Restart into a new directory. Existing directories are rejected.

## Test the release candidate

From the reviewed v0.2.0 release tree, use the pinned compiler and a shell with `tar`.

```bash
(
  set -euo pipefail
  repo=$PWD
  work=$(mktemp -d)
  version=0.2.0
  host=$(rustc -vV | awk '$1 == "host:" { print $2 }')
  cargo package --locked --target "$host" --target-dir "$work/package-build"
  archive="$work/package-build/package/causal-triangulations-$version.crate"
  tar -xzf "$archive" -C "$work"
  packed="$work/causal-triangulations-$version"
  cargo +1.100.0 build --locked --release --target "$host" --manifest-path "$packed/Cargo.toml" \
    --target-dir "$work/cli-build" --bin cdt-validation
  binary="$work/cli-build/$host/release/cdt-validation"
  case "$host" in
    *-windows-*) binary="$binary.exe" ;;
  esac
  cargo +1.100.0 new --vcs none --bin "$work/consumer"
  cp "$repo/tests/packaged-validation/smoke.rs" "$work/consumer/src/main.rs"
  cp "$packed/Cargo.lock" "$work/consumer/Cargo.lock"
  cat >> "$work/consumer/Cargo.toml" <<EOF
causal-triangulations = { path = "../causal-triangulations-$version" }
serde_json = { version = "1.0", features = ["float_roundtrip"] }
sha2 = "0.11"
EOF
  cd "$work/consumer"
  cargo +1.100.0 metadata --format-version 1 --filter-platform "$host" > /dev/null
  cargo +1.100.0 metadata --locked --format-version 1 --filter-platform "$host" > "$work/resolution.json"
  cargo +1.100.0 run --locked --release --target "$host" -- \
    "$binary" "$archive" "$work/smoke" "$work/resolution.json" > "$work/packaged-validation.json"
  printf '%s\n' "$work/packaged-validation.json"
)
```

Retain the report, Cargo and rustc identities, and exact source commit. Record the
`.crate` filename and checksum, contract and artifact schema versions, tested dependency resolution and build profile, and fixture result.
The clean consumer seeds its first resolution from the archived lock to preserve sampler dependency versions. It replays the flat fixture
using the same downstream build.
The standalone consumer checks the documented formats, exact-volume conditioning, outcomes, failure codes, and seeded replay at bounded
chain lengths. #238 owns the independent statistical evidence. #335 owns acceptance of the report for the reviewed release candidate and
later crates.io verification.
