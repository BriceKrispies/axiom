# Dog — notes and limitations

This file records what this app **could not** show, and why the answer was to
write it down rather than to hack the renderer. Adding a debug concept to
`axiom-mesh` / `axiom-mesh-ops` to make a picture easier would make the geometry
layers know what a debug view is, which is exactly the shape the No-Shortcuts
rule bans. All three views that *are* implemented live in `src/debug_view.rs` and
use nothing but the shipped mesh and material vocabulary.

The scene itself is concentric rings of walking dachshunds on generated terrain —
eight rings and 104 dogs at the panel's defaults. Every dog is the same 23
registered bone meshes at another transform, wearing one of 18 shared coats — see
`src/rings.rs` for the layout and `src/install.rs` for the one place that
registration happens.

Fifteen sliders drive it, generated from the dial table in `src/config.rs`.
Fourteen re-pose the running scene; one (`detail`) reloads. **Why that boundary
falls where it does, and which sliders the engine will not let this app have, is
sections 8 and 9 below.**

Two buttons above those sliders switch the *stage*: the field, or one still dog
close up. Both are presentations of the same bound geometry — section 10.

## What is implemented, and how

| View | Query | Mechanism |
|---|---|---|
| Shaded (default) | `?view=shaded` | Each operator's own normals, each object's authored colour. |
| Flat normals | `?view=flat` | `axiom_mesh::generate_flat_normals` per object — unwelds to three vertices per triangle. |
| Smooth normals | `?view=smooth` | `axiom_mesh::generate_normals` per object — area-weighted per vertex. |
| Normal chart | `?view=normals` | Each vertex's UV is replaced by its normal's `(x, y)` mapped into `0..1`, and every object shares one 64×64 app-authored RGBA chart registered with `RunningApp::add_texture_data`. The sampled colour is then a direct picture of the surface normal. |
| Detail switch | `detail` dial / `?detail=0\|1\|2` | `SceneVariant` — re-tessellates the whole scene from the same authoring. Reloads; see section 8. |
| The field (default) | `?stage=field` | Every ring walking on the terrain — the scene this app opens on. |
| One dog | `?stage=study` | `src/study.rs` — one pool slot, the terrain and the rest of the crowd retired with `Visible(false)`, the dog posed **without a tick** and translated to the origin, and the camera re-seeded to a 19-unit close-up. A *presentation* of the geometry already bound, not a second scene: see section 10. |
| Dial panel | the sliders under the canvas | `src/config.rs` owns every dial's meaning, range and clamp; `src/slider_input.rs` (wasm32 only) *builds* the panel from that table and writes one shared `SceneConfig`. Every dial round-trips through the query string, so a reload restores the scene. |
| Orbit camera | drag / pinch / wheel / right-drag | `src/orbit.rs` holds `target`/`yaw`/`pitch`/`distance` and re-authors the camera every frame through `RunningApp::set_camera`; `src/pointer_input.rs` (wasm32 only) measures the gestures. |
| Camera lock | the button under the stage switch / `?lock=on` | `src/camera_lock.rs` owns what the lock means; a locked `OrbitState` ignores every gesture path, **and** `src/pointer_input.rs` gives the canvas's `touch-action` and `preventDefault()` back so the page scrolls normally over the scene. Both halves are required: freezing the shot alone leaves the page just as stuck, with a still picture on it. |
| Dragging a dog | drag a dog while the camera is locked | `src/herd.rs` — the dog carries a **displacement** from where its ring puts it, and nothing else changes: its travel, its slot in the chain and its point in the trot are untouched, so when the displacement decays to zero it is bit-for-bit the dog the undisturbed field would have drawn. Dogs collide as oriented capsules sized from the gap the layout left them (`crowd_space` in `src/rings.rs`), so the crowd at rest is provably out of contact. `tests/herd.rs` compares a field that has been hauled about against one nobody touched, at the same tick, and demands they are equal. |

## Limitations found, and why nothing was hacked

### 1. No wireframe

The engine's render path is a single indexed **triangle-list** pipeline. There is
no line topology, no per-draw polygon mode, and no place in `Material` or
`FrameOutcome` to ask for one. A wireframe would therefore mean either a new
primitive topology through `axiom-render` / `axiom-webgpu` / the live backends, or
an app-side barycentric-edge shader hook that does not exist.

Rejected alternative: generating a separate "edge cage" mesh of thin boxes per
triangle in the app. At the base density that is ~31k triangles ×
3 edges — a six-figure box explosion whose only purpose is to make a picture.
It would have been geometry theatre, not a wireframe.

**Verdict:** genuinely a renderer capability, not an app one. Not built.

### 2. No per-object UV checker *alongside* the normal chart

`Texture::Checker` and `Texture::UvGrid` exist and would visualise the
operator-authored UV parameterization directly — and this app could switch
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

The app spawns 1 + 162 × 23 = 3727 instances (the whole pool — see section 8) and
asks for 4096, with a `const` assertion in `src/live.rs` tying the two together so
the relationship cannot rot into a stale comment. That is an app choosing its own
budget, which is right — but the *silence* is a sharp edge for the next app that
grows past its number: this app has already been bitten once, when the crowd grew
from 19 dogs to 120 and the old 2048 would have stopped drawing at dog 89 without
a word.

**Verdict:** the app sizes its own buffer, and `src/live.rs` says out loud what
overflow looks like. Reporting truncation is the backend's design to make.

### 5. A per-instance colour needs a per-instance material

`FrameDrawItem` carries a per-instance `color[4]` and the packed instance stream
is 40 floats wide, so the *wire format* already varies colour per instance. The
value, however, has exactly one source: `axiom-render-pipeline` looks a draw's
colour up in its `MaterialSlot` table by the renderable's **material id**, and
`axiom-gpu-backend`'s `frame_packet_adapter` then keys each batch on the
`(mesh_id, material_id)` pair. `axiom_scene::Renderable` has no tint field and
there is no per-entity colour component anywhere on the path, so two instances
can differ in colour only by differing in material — and two instances that
differ in material cannot share a batch.

For this app that is the difference between 2392 single-instance draw calls (one
material per dog) and 415 (a bounded 18-coat palette shared by the whole field).
The palette is the right app-tier answer and it is what shipped.

The engine-tier fix, if it is ever wanted, is a small and well-shaped one: an
optional per-renderable tint carried on `Renderable`, threaded through
`RenderReport`'s per-draw tuple into `DrawData::color`, and *excluded* from the
batch key. That is a change to `axiom-scene`, `axiom-render-pipeline` and
`axiom` — three modules, a data contract and a coverage burden — and it is not an
app's errand. It is recorded here rather than made.

**Verdict:** a real engine limitation with a real engine-shaped fix, worked
around in the app the honest way (fewer distinct colours), not by reaching into a
module.

### 6. `renderable_count()` reads zero

The app installs every object through the runtime path
(`add_mesh_data` / `add_material` / `spawn`) rather than through `App::setup`,
because `setup` only hands out the built-in `Mesh` catalogue enum and cannot
register author geometry. `RunningApp::renderable_count()` reports the *authoring*
count, so it stays at zero; the live arm passes a fixed instance capacity instead,
exactly as `apps/burnt-rubber` does. Draw count is asserted from the frame
outcome, which is the number that actually matters.

**Verdict:** a known shape of the umbrella's two authoring paths, not a defect
this app should paper over.

### 7. No live surface resize — the page reloads instead

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
re-runs `dog_start` against the new viewport, and because it is a reload
rather than a navigation the `?detail=`/`?view=` selection survives untouched.
The debounce keeps a desktop window-drag from reloading on every intermediate
pixel; the size comparison keeps a URL-bar or soft-keyboard reflow — which moves
`innerHeight` without moving the canvas box we chose — from reloading at all.

**Verdict:** a missing engine capability, named here rather than hacked around.
The app degrades to a reload; nothing was added to a module to avoid one.

### 8. Geometry is uploaded once at bind, so one dial reloads and the crowd is pooled

`WindowingApi::run_web_multi` takes the driver **by value** and consumes it into
the animation-frame loop. From that moment the only things a frame may change are
what the per-frame closure returns: the clear colour, the lights, the camera and
the *instance* stream. `GpuBackendApi::replace_geometry` and
`WindowingApi::update_present_meshes` both exist — but the second is a method on a
`WindowingApi` the app no longer owns, and the first is behind the module boundary
entirely. There is no seam by which an app driving `run_web_multi` can put a new
vertex on the GPU.

Two consequences, and both are shaped by that one fact rather than worked around
it:

* **The `detail` dial reloads.** Re-tessellating is a geometry change, so it
  cannot be a re-pose. The panel therefore writes the whole configuration into the
  address bar on *every* dial move (`history.replaceState`), and the detail dial
  calls `location.reload()` — which comes back to exactly the scene the user had
  built. The same mechanism is what makes the pre-existing resize reload
  (section 7) non-destructive now that there is state worth keeping.
* **The crowd is a pool, not a spawn.** The ring dials move the number of dogs on
  screen, and a dog that might be shown at frame 400 has to have been spawned at
  frame 0. `install_scene` therefore spawns `MAX_DOGS` dogs' worth of bone nodes
  up front and retires the unused ones with `Visible(false)` — the engine's own
  sanctioned pooling primitive, which drops a renderable at submission so it costs
  no projection, no shading and no draw. `tests/dials.rs` drives the ring dial
  through the real engine and asserts the *drawn* instance count follows it.

Rejected alternative: despawning and respawning the crowd on every ring-dial tick.
`RunningApp::spawn` propagates world transforms per call, so rebuilding ~2400
nodes is quadratic and turns a slider drag into a series of hitches. Pooling is
both cheaper and the thing the engine documents for exactly this case.

**Verdict:** a real engine boundary, honoured rather than reached through. The
live dials stop precisely where geometry begins.

### 9. There is no colour dial, and there cannot be one at the app tier

The panel has no saturation, value or hue-spread slider. Not an oversight — a
repaint is unreachable from an app:

* `Material` has no runtime mutation. `RunningApp::add_material` appends to a
  private store and hands back a `Handle`; nothing takes one back.
* `Renderable` is **not** a `Component`. `RunningApp::set::<T>` covers `Transform`,
  `Bounds` and `Visible`, so an installed instance cannot be re-pointed at a
  different material either.

So the only way to change an instance's colour is to give it a different
material, and the only way to give it a different material is to despawn and
respawn it — which is the quadratic scene rebuild section 8 rejects, for a slider
that would be dragged continuously.

This is section 5 hit from the other side, and it has the same engine-tier fix: an
optional per-renderable tint carried on `Renderable`, threaded through
`RenderReport`'s per-draw tuple into `DrawData::color`, and **excluded** from the
batch key. A cheaper half-measure would be to make `Renderable` a `Component` so
an app can at least re-point an instance at another registered material; that
alone would buy this panel a hue dial over a pre-registered bank.

The knock-on inside the app is recorded where it bites: because a pool slot's coat
is fixed at spawn, the layout may only ask for coats the pool already carries, so
a dog wears palette entry `crowd index % 18`. The parity-split hue comb this field
used to carry — which gave *adjacent rings* disjoint hue sets — is not a balanced
assignment and a fixed pool cannot honour it. What survives is the property that
mattered most and that `tests/rings.rs` still holds: no two dogs adjacent along a
ring share a coat, and the palette stays bounded at 18 whatever the crowd size.

**Verdict:** an engine limitation with a well-shaped engine fix, recorded rather
than made. The app does the honest thing an app can: it stops offering the dial.

### 10. The single-dog study is a presentation, not a second scene — and that is the same limitation from a third side

The stage switch under the canvas offers two views of the *same bound scene*: the
walking field, and one still dog close up. It could not offer anything else,
because sections 8 and 9 between them say what a live frame is allowed to change
— an instance transform and a visibility flag, and nothing more.

That constraint turned out to be the right shape rather than a cost, and it is
worth naming why. A study built the obvious way — its own registered geometry, so
it can be posed and lit independently — would be a **second dog**, free to drift
from the one the field is made of the first time either is touched. What the
engine allows instead is the honest thing: the study is one of the pool's own
slots, wearing the pool's own coat, drawn from the same 23 registered meshes,
posed by the same `Gait::pose` from the same rig and the same resolved dials. It
is the field's animal by construction, and `tests/stages.rs` holds the claim at
the frame: the study draws exactly `bone_count` instances and the field comes
back whole afterwards, out of a pool that was never rebuilt.

Three consequences the switch inherits rather than works around:

* **No ground under the study.** The terrain is one of the static instances, so
  retiring it is a visibility write. The dog is still *posed* against the real
  heightfield — the plants, the relief cap and the terrain pitch are all in play
  — and then translated by the ground point it was standing on. What is missing
  is the drawing of the ground, not the ground.
* **The study cannot be repainted.** It wears the coat pool slot 0 was spawned
  with, for exactly the reason section 9 gives. A "study in a neutral grey"
  would need the same per-renderable tint the colour dial needs.
* **It is still by construction, not by pausing.** `Study::pose` takes no tick.
  A paused clock would be a second kind of state to keep in step with the frame
  loop; an expression with no clock in it cannot fall out of step with anything.

The one thing the study does change about the gait is the swing *height*, which
it holds at zero — a still animal has no swing, and the dachshund's four contacts
are spread nearly a quarter-cycle apart, so no instant of its walk has all four
paws down. That is recorded in `src/study.rs` next to the scan that picks the
instant, not smuggled in as a magic travel constant.

**Verdict:** not a limitation this time — the engine's boundary named the right
design. Recorded here because the *reason* the study is shaped this way is the
same fact sections 8 and 9 are about.
