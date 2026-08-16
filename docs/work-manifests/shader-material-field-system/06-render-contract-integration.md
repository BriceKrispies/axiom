# 06 — Render contract integration

## Objective

Carry a `Surface` from an app, through the four-tier material chain, to the
backend boundary — **without any backend knowing how to render it yet**. After
this manifest a surface has an identity that survives every seam, and the
existing `Material` still works unchanged.

## Architectural placement

Four **engine modules** (`axiom`, `axiom-resources`, `axiom-render`,
`axiom-render-pipeline`) and one **layer** (`axiom-host`). No new package.

Each module adds `surface` (and, where it constructs graphs, `field`) to its
`allowed_layers`. That is legal without qualification — a module may depend on
any layer it declares, and there is no ordering constraint on module→layer edges.

## Existing code involved

| Path | Role |
|---|---|
| `modules/axiom/src/material.rs:31` | `Material` — 7 fields, the app-facing vocabulary |
| `modules/axiom/src/app/authoring.rs:132` | `RunningApp::add_texture_data(w, h, Vec<u8>)` |
| `modules/axiom/src/app/resources.rs:94-105` | `interleave_vertices` — **hard-codes vertex colour to white** |
| `modules/axiom/src/mesh_data.rs:55-65` | `MeshData` — no colour stream, though `axiom_mesh::MeshStreams.colors` exists |
| `modules/axiom-resources/src/material_data.rs:39` | `MaterialData` |
| `modules/axiom-render/src/render_material.rs:29` | `RenderMaterial`, and the `ratio_lit!` macro |
| `modules/axiom-render/src/render_pipeline_kind.rs` | `BASIC_LIT = 1`, `UNLIT = 2` — emitted, then **dropped at the `FramePacket` boundary** |
| `modules/axiom-render-pipeline/src/render_pipeline_api.rs:61,157,391,413` | `MaterialAsset`/`MaterialSlot`; where opacity folds into alpha and roughness becomes `1.0 - roughness` |
| `crates/axiom-host/src/frame_packet.rs:151` | `FrameDrawItem` — 9 fields |
| `modules/axiom-gpu-backend/src/frame_packet_adapter.rs:23` | `INSTANCE_FLOATS = 40` — **zero free lanes** |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-host/src/frame_packet.rs` | modify — add `surface_program: u64` to `FrameDrawItem` |
| `crates/axiom-host/layer.toml` | modify only if a new capability is exported |
| `modules/axiom-resources/src/{material_data.rs, resources_api.rs, ids.rs}` | modify |
| `modules/axiom-render/src/{render_material.rs, render_input.rs, render_api.rs}` | modify |
| `modules/axiom-render-pipeline/src/render_pipeline_api.rs` | modify |
| `modules/axiom/src/{material.rs, app/authoring.rs}` | modify |
| the four `module.toml` files | modify — `allowed_layers` gains `surface` (+ `field`) |
| the four `Cargo.toml` files | modify |

## Dependencies on earlier manifests

**`04`.** Independent of `05`. May run **in parallel with `12`**.

## Public API / data contracts

### The identity that crosses every seam

```rust
// crates/axiom-host/src/frame_packet.rs
pub struct FrameDrawItem {
    /* ...existing 9 fields... */
    surface_program: u64,   // 0 = the built-in fixed material path
}
```

**One `u64`, and `0` means "the engine as it is today".** This is the single most
important compatibility decision in the manifest: every existing draw keeps
working because it carries `0`, and `FramePacket` stays *primitive-only*, which is
the property `frame_packet.rs:1-14` exists to protect.

The value is `Surface::digest().raw()` — a content hash, not a slot index — so
two identical surfaces authored independently collide *correctly* into one
program. That is the dedup the render path has never had: every GPU cache today
is keyed on a caller-assigned `u64` and there is no content hashing anywhere in
`modules/axiom-gpu-backend`.

### App-facing authoring

```rust
// modules/axiom/src/material.rs
impl Material {
    pub fn from_surface(surface: Surface) -> Self;
    pub fn with_metallic(self, metallic: Ratio) -> Self;
}
```

**`Material::lit(color)` must remain a one-liner.** `apps/axiom-rotating-cube`
authors a complete cube in ~13 lines, one of which is the material
(`Material::lit(color).with_texture(Texture::Checker)`). A design that makes
`Texture::Checker` cost a graph has failed, and `13` pins this as a regression
control. A `Material` with no surface carries `surface_program = 0` and takes
exactly today's path.

### Where the translation happens

`modules/axiom-render-pipeline` is a **feature module**
(`allowed_modules = ["scene", "render", "webgpu"]`) and is the sanctioned
composition tier — it already owns the `Material` → `MaterialAsset` → draw
translation at `render_pipeline_api.rs:391-413`. The surface digest is threaded
through there. No new feature module is needed and none may be created.

### Do not widen the instance stream

`INSTANCE_FLOATS = 40` (`mvp 16 + world 16 + colour 4 + emissive 3 +
specular 1`) has **zero free lanes** — the specular lane was taken from the last
pad float. The rigid pipeline also binds **16 of the 16** vertex attributes
WebGL2 guarantees, and the skinned pipeline is at that ceiling already (which is
why skinned draws silently drop emissive and specular).

**Therefore: `surface_program` does not ride the instance stream.** It is a
*batching key* consumed by `frame_packet_adapter`'s sort — draws already group by
`(mesh_id, material_id)`; they will group by `(mesh_id, material_id,
surface_program)`. Per-surface *parameters* need a separate uniform channel,
designed in `07`. Do not attempt either in this manifest.

## Explicitly excluded

* **No backend behaviour.** Both backends ignore `surface_program` after this
  manifest. `07` is where it starts mattering.
* **No WGSL, no pipelines, no bind groups.**
* **No removal of any existing `Material` field.** `texture`, `custom_texture`,
  `roughness`, `opacity`, `emissive` all keep working exactly as they do.
* **No TypeScript.** `packages/axiom-web-engine` has a parallel `MaterialSpec`
  with no textures at all and a different shading curve (Blinn-Phong with
  `shininess = 8 + gloss*120`, a Schlick rim, per-vertex AO, an in-shader
  Reinhard knee). Unifying it is separate work and is out of scope for this
  entire directory.
* **Vertex colours are out of scope by default.** `interleave_vertices` hard-codes
  white and `MeshData` has no colour stream, which is why every app hand-writes
  RGBA byte arrays. Fixing it is a genuine prerequisite for *baking a field into a
  mesh*, but not for the slice. **If it is taken on**, it is: add a colour stream
  to `MeshData`, plumb it through `interleave_vertices` to the existing
  `@location(3) vertex_color` the shader already multiplies by. Do it as a
  separate, clearly-scoped change, not smuggled in here.

## Determinism requirements

`surface_program` is a content digest, so it is stable across runs and targets.
Two draws with equal surfaces get equal ids; a structural change to a surface
changes it; **a parameter value change does not** (per `04`).

## Serialization requirements

`FrameDrawItem` is not serialized by the engine, **but burnt-rubber's golden
`agent_opening_render.bin` records every draw including its emissive and specular
lanes**, and those goldens are SHA-256-pinned in `apps/burnt-rubber/slice.toml`
and checked by `cargo run -p xtask -- check-slices`. Adding a field to
`FrameDrawItem` **will change all 15 committed burnt-rubber goldens**. That is
expected and correct; regold them in this manifest, with the slice checker's
hashes updated in the same commit, and say in the commit message that the delta
is the new `surface_program: 0` lane.

> **CORRECTION — the paragraph above is wrong on its central claim.** Adding a
> field to `FrameDrawItem` does **not** move any burnt-rubber golden. Verified
> empirically during implementation: with the whole engine change in place and
> the encoder untouched, `cargo test -p axiom-burnt-rubber --test agent_golden`
> passed against the committed baselines. `apps/burnt-rubber/tests/agent_golden.rs`
> encodes `FrameOutcome`/`DrawData` **field by explicit field** and never names
> `FramePacket` or `FrameDrawItem` at all.
>
> The goldens were therefore moved **deliberately**, by adding
> `push_u64(&mut out, d.surface_program())` to the render encoder — for the same
> reason emissive and specular are already encoded: it is a per-draw shading
> identity the colour lane cannot carry, and the artifact is documented as "the
> render boundary". **5 files moved, not 15**: each `*_render.bin` grew by
> exactly `8 × draw_count` bytes (grid 449, opening 744, esses 499, canyon 433,
> finish 220), and every decoded `surface_program` is `0` — so the goldens now
> *pin* that burnt-rubber renders exactly as it did before surfaces existed.
> `agent_*_state.bin` and `agent_*_resources.bin` are byte-identical to `HEAD`.

## Testing requirements (100%)

* `FrameDrawItem::new` defaults `surface_program` to `0`; `with_surface_program`
  sets it; both covered.
* A `Material` with no surface produces `surface_program == 0` end to end.
* A `Material::from_surface` produces the surface's digest end to end.
* Two materials from equal surfaces produce equal ids; from different surfaces,
  different ids.
* `MaterialData`/`RenderMaterial`/`MaterialAsset` round-trips carry the id
  unchanged — one test per seam, because a dropped field at a seam is exactly the
  defect `RenderPipelineKind::UNLIT` already demonstrates.
* Every touched module returns to 100% coverage.

## Architecture tests

* `cargo xtask check-architecture` — `ModuleDependsOnLayerNotAllowed` fires if a
  `module.toml` and its `Cargo.toml` disagree; `ModuleFacadeMustExportOne` fires
  if a module's `lib.rs` gains a second non-`ids` public line. If
  `axiom-resources` must expose surface-shaped vocabulary, it goes through the
  single `pub use ids::{…}` channel, the `axiom-figure` shape.
* `cargo run -p xtask -- check-slices` after regolding.

## Performance risks

* **Batch fragmentation.** The batcher already produces near-singleton groups on
  real content — `frame_packet_adapter.rs:32-49` records that on burnt-rubber's
  road *"almost every draw carries its own mesh"*, and that a per-frame `HashMap`
  here was ~10% of a throttled frame before it was replaced by a sort +
  `chunk_by`. Adding a third sort key can only fragment further. Because every
  existing draw carries `surface_program = 0`, **fragmentation is zero until a
  surface is actually used** — verify that with a before/after batch count on
  burnt-rubber, and record the number.
* `FrameDrawItem` grows by 8 bytes. It is copied per draw per frame; at
  burnt-rubber's draw counts this is noise, but measure rather than assume.

## Migration considerations

* 15 burnt-rubber goldens + `slice.toml` hashes.
* `apps/axiom-rotating-cube` has **no goldens at all** — its two `.bin` artifacts
  were deleted 2026-08-11 as drifted, and `tests/render_determinism.rs:92-102`
  now only asserts self-equality. Nothing in the repo currently pins what trivial
  rendering looks like. `13` fixes that; note it here so the gap is not
  rediscovered.

## Completion criteria

1. `FrameDrawItem` carries `surface_program: u64`, defaulting to `0`.
2. A `Surface` authored in an app reaches `FrameDrawItem` with its digest intact,
   proven by a test at every one of the four seams.
3. Every existing app renders byte-identically apart from the new zero lane.
4. `Material::lit(color)` is still a one-liner.
5. Goldens regolded; `check-slices` passes.
6. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint
   count rises.

## Validation commands

```sh
cargo test --workspace
cargo xtask check-architecture
cargo run -p xtask -- check-slices
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 6.** Parallel with `12`. Owns the four module crates and
`crates/axiom-host/src/frame_packet.rs`.
