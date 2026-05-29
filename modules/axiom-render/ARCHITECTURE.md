# Axiom Render — Module Architecture

`axiom-render` is **an isolated engine module**. It takes a
scene-independent `RenderInput` and compiles it into a deterministic
`RenderCommandList`.

## What this module owns

- `RenderCamera` — view + projection matrices.
- `RenderLight` / `RenderLightKind` — directional and point lights with
  world-space direction or position, colour, intensity.
- `RenderMesh` — opaque id + positions / normals / uvs / indices.
- `RenderMaterial` — opaque id + base colour (vertical slice ships
  only basic-lit).
- `RenderObject` — world `Mat4` + mesh index + material index + visibility.
- `RenderInput` — the renderer's input contract; built incrementally
  through `RenderApi` methods.
- `RenderCommand` — backend-neutral commands:
  `ClearFrame`, `SetCamera`, `SetPipeline`, `SetMesh`, `SetMaterial`,
  `DrawIndexed`.
- `RenderCommandList` — ordered list, deterministic by construction.
- `RenderApi` — the single public facade.

## What this module is not allowed to know

- The scene module's `Scene` / `SceneSnapshot`. The renderer takes
  matrices and arrays, not scene-graph references.
- The resources module's `ResolvedResources` / `MeshData` /
  `MaterialData`. The renderer takes raw vertex arrays and base
  colours.
- The webgpu module's `GpuSubmission`. The renderer emits a
  *backend-neutral* command list; the app translates it.
- Any host / browser / GPU API.

## How `RenderInput` is built

The app calls `RenderApi`'s builder methods to add a camera, lights,
deduplicated meshes and materials, and one `RenderObject` per visible
scene renderable. Each `add_input_*` method returns the index the
caller embeds in `add_input_object` so the command list can refer to
the resource by index.

## How `RenderCommandList` is inspected

`RenderApi` exposes indexed inspection methods plus `KIND_*` `u32`
constants. The app reads the list one command at a time and switches
on the kind code — there is no public `RenderCommand` enum reachable
by name.

## Why render does not import scene

The renderer's job is to consume a *flat, matrix-shaped* description
of what to draw. Coupling it to scene would force every replay test,
every alternative frontend (a future tooling preview, a future
headless GPU farm), and every parallel renderer to depend on scene
mutation APIs. The boundary stays small by keeping every input as
primitive arrays + matrices.

## Public surface

`lib.rs` exposes **exactly one** facade: `RenderApi`.
