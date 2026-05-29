# Axiom Resources — Testing Discipline

Resources is consumed by the demo app's vertical slice; deterministic
output is the contract. Every public concept reached through
`ResourcesApi` has a direct test.

## What is tested

- `ResourceId` — invalid sentinel, valid range, ordering, copy.
- `Vertex` — accessor round-trip, field equality.
- `MeshData` — accessor round-trip, equality.
- `MaterialData` — accessor round-trip, optional texture.
- `TextureData` — happy path, wrong-size pixel buffer rejection, equality.
- `cube_mesh::build_cube_mesh` — 24 vertices, 36 indices, each face
  has its expected outward normal, indices stay in vertex range, runs
  produce equal output.
- `basic_lit_material::build_basic_lit_material` — colour round-trip,
  deterministic.
- `solid_color_texture::build_solid_color_texture` — pixel buffer
  matches the requested RGBA, deterministic.
- `ResourceTable` — empty start, monotonic id assignment, insert /
  lookup, deterministic ascending-id iteration.
- `ResolvedResources` — empty snapshot, populated snapshot,
  determinism across two builds, id lookup.
- `ResourcesApi` — every facade method (`empty_table`,
  `register_cube_mesh`, `register_basic_lit_material`,
  `register_solid_color_texture`, `resolve`, every `resolved_*`
  inspection method).

## Determinism

- `resolve(&table)` produces byte-equal snapshots across two builds
  for identical operation sequences.
- `cube_mesh` and `basic_lit_material` produce byte-equal output
  given the same id.

## Architecture / boundary

`tests/architecture.rs` enforces:

- `module.toml` exists.
- `lib.rs` publicly exports exactly `pub use resources_api::ResourcesApi;`.
- No `axiom_scene`, `axiom_render`, `axiom_webgpu` imports.
- No `axiom_host` import (resources does not need a host).
- No browser / DOM / WebGPU references.
- No `println!` / `eprintln!` / `dbg!` / `todo!` / `unimplemented!`.
- No `std::fs`, `AssetLoader`, `Physics`, `Animator`, `Audio`,
  `InputState` symbols.
- No `utils`, `helpers`, `common`, or `misc` modules.
