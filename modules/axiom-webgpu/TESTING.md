# Axiom WebGPU — Testing Discipline

## What is tested

- `BackendKind` — variant exists, is `Copy`.
- `GpuCommand` — kind code stability, variant-to-code mapping, payload
  equality.
- `GpuSubmission` — new is empty, push records in order, target
  dimensions round-trip.
- `GpuSubmissionReport` — per-kind counter computation, target
  dimensions round-trip, equality for identical input.
- `WebGpuApi` — every facade method:
  - `new_submission`, every `submission_*` push method,
  - `submit` produces a deterministic record,
  - `report_*` accessors return correct counts and kinds,
  - identical submissions produce equal reports.

## Determinism

`submit(sub)` is a pure function of `sub`. The test
`submit_is_deterministic_for_identical_input` proves byte-equal
reports across two builds. When a `Live` backend lands, the same
test will continue to apply to the `Recording` backend; the live
arm will need its own non-deterministic-by-nature test in a future
GPU-aware integration suite.

## Architecture / boundary

`tests/architecture.rs` enforces:

- `module.toml` exists.
- `lib.rs` publicly exports exactly `pub use webgpu_api::WebGpuApi;`.
- No `axiom_scene`, `axiom_resources`, `axiom_render` imports.
- No real GPU/JS bindings (`wgpu::`, `web_sys::`, `js_sys::`,
  `wasm_bindgen::`) yet — see `ARCHITECTURE.md` for the blocker.
- No `println!` / placeholder macros.
- No junk-drawer modules.
