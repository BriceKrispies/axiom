# Procedural Mesh Crucible — notes and limitations

This file records what the crucible **could not** show, and why the answer was to
write it down rather than to hack the renderer. Adding a debug concept to
`axiom-mesh` / `axiom-mesh-ops` to make a picture easier would make the geometry
layers know what a debug view is, which is exactly the shape the No-Shortcuts
rule bans. All three views that *are* implemented live in `src/debug_view.rs` and
use nothing but the shipped mesh and material vocabulary.

The scene itself is two counter-rotating rings of walking dogs on generated
terrain. Every dog is the same 23 registered bone meshes at another transform —
see `src/rings.rs` for the layout and `src/install.rs` for the one place that
registration happens.

## What is implemented, and how

| View | Query | Mechanism |
|---|---|---|
| Shaded (default) | `?view=shaded` | Each operator's own normals, each object's authored colour. |
| Flat normals | `?view=flat` | `axiom_mesh::generate_flat_normals` per object — unwelds to three vertices per triangle. |
| Smooth normals | `?view=smooth` | `axiom_mesh::generate_normals` per object — area-weighted per vertex. |
| Normal chart | `?view=normals` | Each vertex's UV is replaced by its normal's `(x, y)` mapped into `0..1`, and every object shares one 64×64 app-authored RGBA chart registered with `RunningApp::add_texture_data`. The sampled colour is then a direct picture of the surface normal. |
| Detail switch | `?detail=base\|dense\|coarse` | `CrucibleVariant` — re-tessellates the whole scene from the same authoring. |
| Orbit camera | drag / pinch / wheel / right-drag | `src/orbit.rs` holds `target`/`yaw`/`pitch`/`distance` and re-authors the camera every frame through `RunningApp::set_camera`; `src/pointer_input.rs` (wasm32 only) measures the gestures. |

## Limitations found, and why nothing was hacked

### 1. No wireframe

The engine's render path is a single indexed **triangle-list** pipeline. There is
no line topology, no per-draw polygon mode, and no place in `Material` or
`FrameOutcome` to ask for one. A wireframe would therefore mean either a new
primitive topology through `axiom-render` / `axiom-webgpu` / the live backends, or
an app-side barycentric-edge shader hook that does not exist.

Rejected alternative: generating a separate "edge cage" mesh of thin boxes per
triangle in the app. At the crucible's base density that is ~31k triangles ×
3 edges — a six-figure box explosion whose only purpose is to make a picture.
It would have been geometry theatre, not a wireframe.

**Verdict:** genuinely a renderer capability, not an app one. Not built.

### 2. No per-object UV checker *alongside* the normal chart

`Texture::Checker` and `Texture::UvGrid` exist and would visualise the
operator-authored UV parameterization directly — and the crucible could switch
every material to them in one line. What it cannot do is show the UV checker *and*
the normal chart at once, because both are driven by the **same single UV set**:
the chart view overwrites the operator's UVs with normal-derived ones. A mesh
carries one `uvs` stream, `MeshData` accepts one, and the renderer binds one.

**Verdict:** the two views are mutually exclusive by data model, not by
oversight. The chart view was chosen because normals are the thing a geometry
library most often gets wrong; the built-in UV textures remain one line away for
anyone who wants the other picture.

### 3. No per-vertex colour

`MeshData` carries positions, normals, UVs and indices — there is no colour
stream, and `axiom_mesh::Mesh`'s `colors` stream has no path into the umbrella.
This is why "colour by normal" had to go through a texture lookup instead of the
obvious `colors = normal * 0.5 + 0.5`.

**Verdict:** the chart is a faithful substitute. Its one honest inaccuracy is that
`+Z` and `-Z` normals with the same `(x, y)` land on the same chart texel; the
page legend says so.

### 4. One instance buffer for the whole scene

The live backend packs **every** batch's instances back-to-back into one buffer
sized by the `max_instances` argument, and `SceneRenderer::record` silently
`min`s each batch's count against the room left. A capacity below the scene's
instance count therefore does not error: it just stops drawing partway round the
ring, which looks like a scene bug rather than a budget one.

The crucible spawns 1 + 19 × 23 = 438 instances and asks for 2048. That is an app
choosing its own budget, which is right — but the *silence* is a sharp edge for
the next app that grows past its number.

**Verdict:** the app sizes its own buffer, and `src/live.rs` says out loud what
overflow looks like. Reporting truncation is the backend's design to make.

### 5. `renderable_count()` reads zero

The crucible installs every object through the runtime path
(`add_mesh_data` / `add_material` / `spawn`) rather than through `App::setup`,
because `setup` only hands out the built-in `Mesh` catalogue enum and cannot
register author geometry. `RunningApp::renderable_count()` reports the *authoring*
count, so it stays at zero; the live arm passes a fixed instance capacity instead,
exactly as `apps/axiom-gravix` does. Draw count is asserted from the frame
outcome, which is the number that actually matters.

**Verdict:** a known shape of the umbrella's two authoring paths, not a defect
this app should paper over.

### 6. No live surface resize — the page reloads instead

The app sizes its presentation surface to the device it landed on: it measures
the canvas's laid-out CSS box (falling back to `innerWidth` × the stylesheet's
`min(94vw, 1180px)` rule, and to the `WIDTH`/`HEIGHT` constants if the browser
answers neither), multiplies by `devicePixelRatio` **capped at 2**, and hands
that to both `WindowingApi::configure_surface` and `Window::new` so the render
target and the camera's aspect agree. The aspect itself is pinned at 16:9 — the
canvas's CSS `aspect-ratio` — so a phone and a desktop see the *same framing* at
different resolutions.

That happens exactly once. `WindowingApi` configures its surface *before*
`run_web_multi` consumes the driver into the animation-frame loop, and there is
no public way to reconfigure it afterwards: no `resize`, no
`reconfigure_surface`, nothing on the presenter. So when the viewport changes —
a phone rotating, a desktop window dragged — the surface cannot follow it.

Rejected alternative: adding a resize path to `axiom-windowing`. That is a real
design (who owns the new viewport, what happens to the in-flight frame, how the
GPU backend's swapchain and depth/shadow targets are recreated, what the
deterministic core does with a mid-run viewport change) with a real coverage
burden, and it is the *engine's* design to make — not something an app should
reach in and bolt on because it wants to look right in landscape.

What the app does instead is the one honest thing an app can do: on a `resize` or
`orientationchange` that **settles** (350 ms debounce) on a surface size
different from the one it started with, it calls `location.reload()`. The reload
re-runs `crucible_start` against the new viewport, and because it is a reload
rather than a navigation the `?detail=`/`?view=` selection survives untouched.
The debounce keeps a desktop window-drag from reloading on every intermediate
pixel; the size comparison keeps a URL-bar or soft-keyboard reflow — which moves
`innerHeight` without moving the canvas box we chose — from reloading at all.

**Verdict:** a missing engine capability, named here rather than hacked around.
The app degrades to a reload; nothing was added to a module to avoid one.
