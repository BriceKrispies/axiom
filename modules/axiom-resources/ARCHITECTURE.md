# Axiom Resources — Module Architecture

`axiom-resources` is **an isolated engine module**, not a layer. It
owns CPU-side resource descriptions — arbitrary meshes from neutral
geometry, basic-lit materials, solid-colour textures — and the
deterministic `ResolvedResources` snapshot the app hands to the renderer.

The module is **shape-agnostic**: `ResourcesApi::register_mesh` takes
neutral `(position, normal, uv, color)` vertex data plus a triangle
index list and knows no shapes of its own. The built-in unit cube
(`register_cube_mesh`) is a thin *generator* layered on that one path —
a primitive, not a special case baked into the resource table.

## What this module owns

- `ResourceId` — opaque monotonic ids (never reused, never `0`).
- `Vertex` — position / normal / uv / colour.
- `MeshData` — id + name + vertex list + index list (CPU-side only).
- `MaterialData` — id + name + base colour + optional texture id.
- `TextureData` — id + name + width / height + RGBA8 pixel bytes.
- `ResourceTable` — mutable `BTreeMap`-backed deterministic storage.
- `ResolvedResources` — value-typed snapshot the renderer consumes.
- `ResourcesApi` — the single public facade.

## What this module is not allowed to know

- The scene module's `Scene`, `SceneSnapshot`, `SceneNodeId`, transforms.
- The render module's `RenderInput`, `RenderCommandList`.
- The webgpu module's `GpuSubmission`.
- File I/O, image decoding, GLTF, asset bundles, network fetches.
- GPU resources of any kind (`wgpu`, `web-sys`, raw buffers).
- Browser / DOM / canvas / `requestAnimationFrame` / `performance.now`.
- Wall-clock time, randomness, global mutable state.

`tests/architecture.rs` scans the source tree for every one of these.

## How it consumes layers

- `axiom-math` — `Vec2`/`Vec3`/`Vec4` for vertex / colour data, and the
  `Mat4` shape every higher layer agrees on.
- `axiom-kernel` / `axiom-runtime` / `axiom-frame` — transitively
  available but not directly used by the vertical slice today.

## Why resources does not import scene

Scene attaches renderables by `MeshRef(u64)` / `MaterialRef(u64)`.
Those `u64`s mean nothing inside the scene module — they are
*producer-supplied* ids the app registers with this module. Keeping
the resources module ignorant of scene means a future tooling app,
a future test harness, and a future native build can each register
the same resource ids with their own scene without dragging the scene
crate into a CPU-only headless build.

## Public surface

`lib.rs` exposes **exactly one** facade: `ResourcesApi`. Every other
type is reached through facade methods that take or return
primitives (`u64`, `[f32; 3]`, `&[u32]`, `[u8; 4]`) so the boundary
stays small and stable.
