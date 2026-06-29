#!/usr/bin/env bash
# Run a Rust test suite on wasm32 AND natively, proving the SAME test executes
# and passes on both targets. Today CI only `cargo build`s wasm and never runs
# it; this is the runner that closes that gap, so a later workstream can execute
# a physics-determinism golden on wasm and diff it against native.
#
# Path: wasm32-unknown-unknown + `wasm-bindgen-test-runner` (executes the wasm
# test binary under node). It is the lightest runner already installed on this
# repo's dev box. The wasip1 + wasmtime alternative is noted in
# tools/wasm-runner/README.md for when running unmodified `#[test]`s matters.
#
# Usage:
#   scripts/wasm-test.sh                       # the bundled proof crate
#   scripts/wasm-test.sh path/to/Cargo.toml    # any crate with wasm-bindgen-test
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST="${1:-$SCRIPT_DIR/../tools/wasm-runner/Cargo.toml}"

echo "==> ensuring the wasm32-unknown-unknown target is installed"
rustup target add wasm32-unknown-unknown >/dev/null

echo "==> native run: cargo test --manifest-path $MANIFEST"
cargo test --manifest-path "$MANIFEST"

echo "==> wasm32 run: cargo test --target wasm32-unknown-unknown (wasm-bindgen-test-runner / node)"
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
    cargo test --manifest-path "$MANIFEST" --target wasm32-unknown-unknown

echo "==> OK: the test suite passed natively AND on wasm32"
