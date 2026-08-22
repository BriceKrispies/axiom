# Wiring wave — the port draws four things; make it draw the rest

Every subsystem in `apps/shmup` was ported. Almost none of it reaches a pixel.
This wave closes that, and only that: **no new porting, no new engine
capability.** If you find yourself transcribing from `C:/dev/Claude-of-Duty`,
you are in the wrong wave — the Rust already exists; find it and connect it.

## The finding this wave exists to fix

`apps/shmup/src/scene/app.rs` puts exactly four things into the engine scene:

```
world.spawn(key_light)   // the sun
install_level(...)       // the street
install_practicals(...)  // the world's lights
install_rifle(...)       // the gun
```

Per frame it moves the camera, moves the rifle, and ticks a HUD model nothing
draws. That is the entire renderer-facing surface of a 116,000-line port.

A sweep of every public symbol in `apps/shmup/src`, counting references from
outside its own file, found **876 with zero consumers**:

| subsystem | unwired | subsystem | unwired |
|---|---|---|---|
| weapons | 140 | audio | 61 |
| ui | 128 | physics | 60 |
| ai | 125 | materials | 45 |
| sky | 84 | fx | 41 |
| player | 71 | world | 38 |

Every visual readback the wiring layer computes is consumed by nobody:
`particle_points`, `soldier_points`, `decal_points`, `tracer_points`,
`shell_points`. The simulation runs, produces them, and the frame ends.

## The shape of the defect — read this before your slice

The port wave built each subsystem faithfully as
`apps/shmup/src/<sys>/system.rs`, then `apps/shmup/src/scene/wiring/*.rs`
hand-rolled a **thinner substitute** that drives the simulation and discards its
output. That is why `add_lights` looked unported when it was called all along,
and why "everything was ported" was true and useless simultaneously.

Five hand-inlined duplicates have been found; four still stand:

| the real port | the thinner copy that runs instead | status |
|---|---|---|
| `world::system::WorldSystem` | `scene::level` inline build | **fixed** — `level.rs` delegates |
| `ui::system::UiCore` | `ui::Hud` (model only, no view) | open — slice 1 |
| `player::system::PlayerCore` | fields on `scene::game::Game` | open |
| `weapons::system::WeaponCore` | `scene::wiring::weapons` | open |

**Prefer deleting the copy over feeding it.** A slice that wires the real port
and leaves the substitute standing has added a fifth duplicate, not removed one.

## Verified engine capability — do not re-derive these

Checked against the engine during this wave, because three earlier claims in
this port's notes were stale:

- **Skinning EXISTS.** `RunningApp::submit_skinned_draw(mesh, material,
  transform, &[Mat4])` (`modules/axiom/src/app/authoring.rs:88`) is per-frame
  immediate mode, and `MeshData::new_skinned` authors the joint/weight streams.
  The soldiers are wireable today.
- **Alpha blending EXISTS.** The scene pipeline blends every draw. A material
  blends when its own alpha is `< 1` (`Material::with_opacity`); an *opaque*
  material now deliberately ignores its albedo map's alpha, because the bake
  packs the height field there.
- **`Visible` is a `Component`** and `despawn` exists, so a node can be hidden
  and shown per frame with `app.set(entity, Visible(..))`.
- **`Transform` is the only other per-frame-settable component.** `PointLight`
  is a `Bundle`, not a `Component` — a spawned light's colour and intensity are
  fixed at spawn time.
- **There is no billboard primitive and no per-frame mesh geometry update.** A
  camera-facing quad is a pooled node whose `Transform` rotation you compute
  CPU-side each frame. That is the shape every particle/decal/tracer uses.
- **The UI is DOM.** `apps/shmup/src/ui/*` is already `web_sys`. It needs no
  engine capability at all — only mounting.

## Rules — this wave is wide, so these are hard

1. **Do not build, check, test, clippy, fmt, or run any gate.** Builds serialise
   on one target directory and are the thing that limits how many of you run at
   once. The orchestrator compiles and runs everything in one integration pass
   afterwards. Your code will not compile before you finish; the discipline
   below is what stands between that and a slice that looks done and is wrong.
2. **No mutating git command** (`add`, `commit`, `reset`, `checkout`, `stash`,
   `clean`, `pull`, `merge`, `rebase`). Read-only git is fine.
3. **Do not touch shared files** — specifically `scene/app.rs`, `scene/game.rs`,
   `scene/mod.rs`, `lib.rs`, `Cargo.toml`, `app.toml`. Every slice needs them and
   you will collide. Instead **end your report with the exact lines to add**,
   file by file, ready to paste.
4. **Write only**: your new file(s) under `apps/shmup/src/scene/wiring/`, files
   inside your own subsystem directory, and
   `docs/work-manifests/shmup-port/notes/<slice>.md`.
5. **Use `scripts/ax`** for every search and symbol lookup — `ax def <sym>`,
   `ax refs <sym>`, `ax impact <sym>`. A zero-result search is data: it is
   recorded, and it is how this wave's own finding was made.
6. **Wire what exists; do not reimplement it.** Before writing a function, run
   `ax def` on the name you were about to invent. This wave exists because
   people did not do that.
7. **Report honestly.** If your slice needs an engine capability that does not
   exist, say so and stop at the boundary — do not fake it in the app tier. A
   blocked slice reported accurately is worth more than one that renders
   something plausible and wrong.

## The slices

Each is independent: its own new wiring file, its own subsystem, its own note.
All five converge only on `scene/app.rs`, which the orchestrator edits from your
reports.

---

### Slice 1 — the HUD  *(largest visible gap; needs no engine capability)*

**Wire `ui::system::UiCore` and its DOM views into the page, and delete the
`ui::Hud` substitute.**

`UiCore` has **zero references**. `scene/game.rs` drives `ui::Hud`, a model-only
copy, and the comment above the call states the defect outright: *"The HUD model
is advanced every frame whether or not a view is mounted."* None is.

Ported and unmounted: `ui/ammo.rs`, `health.rs`, `crosshair.rs`, `compass.rs`,
`killfeed.rs`, `minimap.rs`, `hitmarkers.rs`, `damage.rs`, `markers.rs`,
`prompts.rs`, `menu.rs`, `style.rs` (+ `style.css.tpl`) — 8,540 lines, every
widget carrying its own `web_sys` view.

Produce `scene/wiring/hud.rs`: construct `UiCore`, mount the views under the page
(the canvas element id is `SURFACE_ID`; `input::dom::attach` shows the pattern),
inject `style.css.tpl`, and expose one `fn frame(&mut self, …)` the browser loop
calls. Report what `game.rs` must drop (`Hud`) and what `app.rs` must call.

Non-wasm builds must still compile — the native test path has no DOM. Gate the
view, not the model.

---

### Slice 2 — FX  *(8,886 lines, nothing drawn)*

**Draw the particles, decals, tracers, shells, explosions and muzzle flash the
fx system already computes.**

`scene/wiring/fx_audio.rs` computes `particle_points` and friends every frame and
nothing reads them. `fx/impacts.rs` (1,528 lines), `fx/explosions.rs`,
`fx/tracers.rs`, `fx/shells.rs`, `fx/decals.rs`, `fx/lights.rs` are all ported.

No billboard primitive exists. Use a **pool**: spawn a fixed budget of quad nodes
at install, then per frame set each one's `Transform` (position, camera-facing
rotation, scale) and its `Visible`. Translucent particles need
`Material::with_opacity(< 1)` or they render opaque.

Produce `scene/wiring/fx_draw.rs`. State your pool sizes, and say what happens
when the budget is exceeded — a silent drop is a finding, not an implementation
detail.

---

### Slice 3 — the soldiers  *(the enemies are invisible)*

**Draw the AI agents.** `ai/soldier.rs` (1,541), `ai/animator.rs` (1,385) and
`ai/textures.rs` (1,497 — the camo) are ported and unreferenced.
`scene/wiring/ai.rs` runs the simulation and draws nothing.

Skinning exists (above). Build the soldier mesh with `MeshData::new_skinned`,
drive the palette from `ai::animator`, and submit with `submit_skinned_draw` per
frame per visible agent.

Produce `scene/wiring/soldier_draw.rs`. If the ported animator does not produce a
joint palette in the form `submit_skinned_draw` wants, say exactly what shape it
does produce — do not invent a skeleton.

---

### Slice 4 — the sky

**Draw the dome, the clouds and the stars.** `sky/dome.rs` (429),
`sky/clouds.rs` (361), `sky/stars.rs` (197) and `sky/volumetrics.rs` (855) are
ported; 84 sky symbols are unreferenced. Today the sky is a flat clear colour.

`SkySystem` is already wired for *lighting* (`scene/wiring/look.rs` reads
`key_light`/`ambient`/`depth_fog`/`clear_color`). This slice adds the *visible*
sky. Check what `FrameSky` (`app.set_sky`) already accepts before building
geometry — if the engine's sky pass covers the dome, use it and say so.

Volumetrics likely needs a pass the engine does not have. Report the boundary
rather than approximating it.

Produce `scene/wiring/sky_draw.rs`.

---

### Slice 5 — the gun's materials and the hands

**Bind `weapons/materials.rs`** (1,678 lines; only its `ENV_OCCLUSION` constant
is used) **to the rifle, and draw `weapons/hands.rs`** (2,017 lines,
unreferenced).

`install_rifle` currently binds `Material::lit(bucket_color(bucket))` — a debug
palette. That is why the gun is untextured. `weapons::materials::material_keys()`
and `WeaponMaterial` are the real thing.

The level's material path in `install_level` is the working precedent: a
`runtime_material` surface plus a baked albedo bound with `with_custom_texture`.

Produce `scene/wiring/weapon_look.rs`.

---

## What "done" means for your slice

- The ported code is **called**, and the substitute (if any) is **deleted**.
- `ax refs <your subsystem's facade>` shows a consumer outside its own file.
- Your note in `notes/<slice>.md` records what you wired, what you deleted, what
  you found already broken, and anything you could not do and why.
- Your report ends with the exact shared-file lines the orchestrator must add.
