# `axiom-wasm-runner-proof`

Proof that a Rust test can be **executed** on `wasm32` — not merely compiled —
and yield the same result it does natively. Today CI only `cargo build`s the
wasm target and never runs it; this crate plus `scripts/wasm-test.*` is the
runner that closes that gap, so a later workstream can run a
physics-determinism golden on wasm and diff it against native.

It is repo **tooling**: a deliberately separate single-crate workspace (like
`tools/lints`), absent from the root `cargo metadata`, so it never enters the
engine dependency graph, the Module Law classifier, or the coverage gate.

### Run it

```sh
scripts/wasm-test.ps1          # Windows / PowerShell (this repo's primary shell)
bash scripts/wasm-test.sh      # Linux / CI
```

Both run the suite **natively** and then on **wasm32-unknown-unknown** via
`wasm-bindgen-test-runner` (which executes the wasm test binary under node).
Pass a `Cargo.toml` path to point the runner at any other crate whose tests use
`wasm-bindgen-test`:

```sh
scripts/wasm-test.ps1 path/to/Cargo.toml
```

### Why this path

The runner needs `node`, the `wasm32-unknown-unknown` target, and
`wasm-bindgen-test-runner` — all already present on this repo's dev box, making
it the lightest *workable* option. The crate's `wasm-bindgen-test` dependency is
pinned (`=0.2.122`) to the installed runner; the two versions must agree.

The test bodies use only the cross-target-portable float subset
`{+, -, *, /, sqrt}` — the same subset the `engine_no_unportable_float` dylint
enforces in the step path — so they assert *exact* results (`to_bits`) and pass
identically on both targets.

### Future: wasip1 + wasmtime

`wasm32-wasip1` + `wasmtime` would let the runner execute **unmodified**
`#[test]` functions (no `#[wasm_bindgen_test]` annotation), which is preferable
once the real engine crates' determinism tests run on wasm. That needs
`rustup target add wasm32-wasip1` and a `wasmtime` binary on PATH (neither is
installed here yet); the script can then set
`CARGO_TARGET_WASM32_WASIP1_RUNNER=wasmtime` and target `wasm32-wasip1`.
