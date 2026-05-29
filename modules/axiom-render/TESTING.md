# Axiom Render — Testing Discipline

## What is tested

- `RenderCamera`, `RenderLight`, `RenderMesh`, `RenderMaterial`,
  `RenderObject` — accessor round-trip + equality.
- `RenderInput` — empty input shape, builder methods, equality on
  identical content, light list round-trip.
- `RenderCommand` — kind code stability + variant-to-code mapping.
- `RenderCommandList` — empty start, push order preserved, `default`
  matches `new`.
- `RenderApi` — every facade method:
  - `new_input` / `set_input_*` / `add_input_*`,
  - `build_command_list` produces 6 commands for a cube,
  - empty input produces 2 commands (no camera → no SetCamera, plus
    SetPipeline),
  - invisible objects are skipped,
  - out-of-range mesh/material indices are skipped,
  - inspection accessors return the correct payload per kind,
  - `build_command_list` is byte-equal across two builds.

## Determinism

`build_command_list(&input)` is a pure function of `&RenderInput`.
The test `build_command_list_is_deterministic` proves byte-equal
output for two identical inputs.

## Architecture / boundary

`tests/architecture.rs` enforces:

- `module.toml` exists.
- `lib.rs` publicly exports exactly `pub use render_api::RenderApi;`.
- No `axiom_scene`, `axiom_resources`, `axiom_webgpu` imports.
- No `axiom_host` import.
- No browser / DOM / WebGPU references.
- No `println!` / placeholder macros.
- No `std::fs`, `AssetLoader`, `Physics`, `Animator`, `Audio`,
  `InputState`, or `Scene` symbols.
- No junk-drawer modules.
