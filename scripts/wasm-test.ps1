#!/usr/bin/env pwsh
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
#   scripts/wasm-test.ps1                       # the bundled proof crate
#   scripts/wasm-test.ps1 path/to/Cargo.toml    # any crate with wasm-bindgen-test
param(
    [string]$Manifest = "$PSScriptRoot/../tools/wasm-runner/Cargo.toml"
)
$ErrorActionPreference = "Stop"

Write-Host "==> ensuring the wasm32-unknown-unknown target is installed"
rustup target add wasm32-unknown-unknown | Out-Null

Write-Host "==> native run: cargo test --manifest-path $Manifest"
cargo test --manifest-path $Manifest
if ($LASTEXITCODE -ne 0) { throw "native test run failed ($LASTEXITCODE)" }

Write-Host "==> wasm32 run: cargo test --target wasm32-unknown-unknown (wasm-bindgen-test-runner / node)"
$env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = "wasm-bindgen-test-runner"
cargo test --manifest-path $Manifest --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw "wasm32 test run failed ($LASTEXITCODE)" }

Write-Host "==> OK: the test suite passed natively AND on wasm32"
