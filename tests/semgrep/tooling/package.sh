#!/usr/bin/env bash

# ok: causal-triangulations.release.clean-package-verification
cargo publish --locked --dry-run --all-features
# ok: causal-triangulations.release.clean-package-verification
cargo package --locked --all-features
# ruleid: causal-triangulations.release.clean-package-verification
cargo publish --locked --allow-dirty --dry-run
# ruleid: causal-triangulations.release.clean-package-verification
cargo package --locked --no-verify
# ruleid: causal-triangulations.release.clean-package-verification
cargo publish --dry-run --no-verify --locked
# ruleid: causal-triangulations.release.clean-package-verification
cargo package --allow-dirty --locked
