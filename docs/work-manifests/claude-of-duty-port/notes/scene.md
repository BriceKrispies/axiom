# `scene` — the ten ported subsystems, wired into one running game

`apps/claude-of-duty/src/viewer.rs` (four weapon parts on a turntable) is no
longer the app's only rendering arm. `src/scene/` is the composition tier, and
`claude_of_duty_start` now boots the game: bare ground you can walk on, at real
scale, under the ported atmosphere, with the rifle on the road in front of you.

## What is new

| file | what it is |
|------|------------|
| `src/input.rs` | port of `src/core/input.js` — see `notes/input.md` |
| `src/physics/character.rs` | port of `src/physics/character.js` (490 lines) — the swept-capsule collide-and-slide controller |
| `src/physics/probe.rs` | port of `src/physics/index.js:411-678` — `raycast`/`capsuleCast`/`checkCapsule`/`groundHeight`/`queryAabb` |
| `src/scene/level.rs` | `world/index.js`'s `WorldSystem.init`, reduced to what is ported |
| `src/scene/sky_look.rs` | `sky/index.js`'s per-frame key-light + sky terms, from the CPU atmosphere model |
| `src/scene/game.rs` | `player/index.js`'s `PlayerSystem` + `core/engine.js`'s frame ordering |
| `src/scene/app.rs` | `main.js` — the browser bootstrap on Axiom's engine path |

One additive accessor landed on the ported BVH: `StaticWorld::triangle_of(tri)`,
the raw world-space vertices of a triangle. The JS reads `world.pos` directly for
this (`atlas.js:227-235`); `crate::fx::decals::DecalWorld` named it as the one
thing the BVH did not expose, and it is now exposed. `Movement::slide_side` was
made `pub` for the same reason — `player/index.js:360-363` reads it.

## Every seam that was named, and whether it is bound

| seam | named by | bound by | live in the running scene? |
|------|----------|----------|-----------------|
| `player::movement::CharacterController` | `movement.js` | `physics::character::Character` | **yes** |
| `player::mantle::LedgeCharacter` | `mantle.js` | `physics::character::Character` | **yes** |
| `player::mantle::WorldProbe` | `mantle.js`, `movement.js`'s lean probe | `physics::probe::PhysicsWorld` | **yes** — `Movement::step` is handed it every fixed step |
| `player::movement::PlayerInput` | `movement.js`'s `latchInput` | `input::Input` | **yes** |
| `ui::CameraBasis` / `ui::FramePull` | `ui/index.js`'s `ctx.peek` reaches | `Game::camera_basis` / `Game::hud_frame` | **yes** for the player half; `weapon: None` (see below) |
| `ui::markers::ScreenProjector` | `markers.js` | `ui::markers::FixedCamera`, from the camera's view-projection | type exists and is bound; **not driven** — there are no world markers to project (no objectives, no AI) |
| `audio::spatial::WorldProbe` | `spatial.js`'s occlusion ray | `physics::probe::PhysicsWorld` | **bound, not driven** — see below |
| `fx::decals::DecalWorld` | `atlas.js`'s decal clipper | `physics::probe::PhysicsWorld` | **bound, not driven** |
| `fx::world::FxWorld` | `impacts.js`, `fx/index.js` | `physics::probe::PhysicsWorld` | **bound, not driven** |
| `weapons::ballistics::RaycastWorld` | `ballistics.js` | **not bound** | its second method is `fire_bullet`, which needs `src/physics/penetration.js` — unported. Binding it would mean inventing the solver. |

`PhysicsWorld` is one type satisfying four of those traits, because in the source
they are all one object (`ctx.get('physics')`). No subsystem knows that; each
still speaks only to its own trait.

## What is honestly not connected, and why

* **Buildings, props, dressing.** `WorldSystem.init` calls `registerProps`,
  `registerDressingProps`, `buildBuilding` × `BUILDINGS`, `collapseRoof`,
  `buildGate`, `buildPerimeter`, `dressStreet`, `dressBuildings` and
  `scatterDebris`. None of those source files (`kit.js`, `props.js`,
  `dressing.js`, `buildings.js`, `gate.js`) is ported. The level is exactly what
  `ground.js` authors — terrain, road, kerbs, pavement slabs, alleys, sand
  drifts, one manhole. Bare ground with no building on it. `finalize`'s
  `instanced` and `lights` lists come back empty for the same reason.
* **The viewmodel rig.** `weapons/rig.js` and `viewmodel.js` are not ported, so
  there is no hand rig, no ADS pose and no sway. The rifle (`build_rifle()`,
  all 27 parts, merged into its material buckets) is placed as what it honestly
  is: an object lying on the road 3 m in front of the spawn, at real scale.
  Inventing a rig would be fabricating the exact thing that has not been done.
* **Materials.** The 19 procedural surface generators bake CPU textures, and
  there is no texture-upload path for them yet. Every level batch is flat-lit
  from its palette entry's authored `tint` hex, de-gamma'd (Three r180 decodes
  every hex literal as sRGB, so using the tint raw would render washed out). The
  level therefore reads with the right *palette* and no texture.
* **FX.** The seam is closed but `FxSystem`'s output is particle and decal
  *geometry* — instanced, additively-blended, atlas-sampled quads — which needs
  the unported render frame graph. Driving it would produce state nothing can
  draw.
* **Audio.** Same shape: `AudioCore::set_world_probe` takes the same
  `PhysicsWorld`, but realising the graph needs `web_audio`'s bridge on a
  user-gesture-unlocked `AudioContext` and a per-frame flush. That is a separate
  arm, not a missing binding.
* **AI.** Not ported at all.
* **The HUD's DOM views.** `Hud::late_update` is driven every frame with the
  real movement state (so its damped channels are correct whenever a view is
  mounted), but the twelve per-widget `view` modules are not mounted. Mounting
  them is a `wasm32`-only composition step over an API that already exists.
* **The sky dome.** `crate::sky` is a CPU reference for GLSL the port has no
  emission path for. What *is* reachable is used: the ephemeris gives the sun
  direction, `transmittance_to_space` gives its colour, and `raymarch_sky`
  against the real (CPU-baked) transmittance + multiscatter LUTs gives the clear
  colour and the hemisphere ambient. Those are real integrals, not constants.
  The one invented step is the display transform — a plain Reinhard standing in
  for the unported HDR exposure + ACES tone-map, labelled as invented at the
  site.

## The one engine gap this hit

`App::run` cannot drive an input-driven game. Its per-frame closure is
`|tick| running.tick(tick)`; there is no seam at which an app reads this frame's
input, steps its simulation, and writes this frame's camera before the engine
renders. The app is authored entirely on the normal path
(`App::new().window().add_plugins().setup().install()` → `build()`), but
`scene::app::claude_of_duty_start` then replicates `App::run`'s body — same
surface configuration, same ambient/fog/surface/material-program carry-over, same
seven-tuple frame closure — and adds three lines ahead of `tick`. Every
input-driven Rust app in this repository hits the same wall
(`apps/burnt-rubber/src/web.rs`, `apps/dog/src/live.rs`).

**The right fix is an engine one**: a per-frame app hook on `App` —
`App::each_frame(FnMut(&mut RunningApp, u64))` — called by `run` and by
`RunningApp::tick`, so it is natively testable and coverable. That lands in
`modules/axiom`, which this port slice is not permitted to touch. Until it does,
the loop's *owner* differs from the engine default; nothing about the authoring
path does.

## Traps hit

* **Euler order.** `camera.rotation.set(pitch, yaw, roll)` with r180's default
  `'XYZ'` order is `qx * qy * qz`. `Quat::from_euler_xyz` composes `qz * qy * qx`
  — a different rotation. `scene::app::write_camera` spells the composition out.
* **Triangle winding decides de-penetration.** `StaticWorld::overlap_capsule`
  falls back to the stored face normal when the closest-point direction
  disagrees with it, so a test floor wound clockwise-from-above pushes the
  capsule *down* through itself. Three of the new physics tests were wrong for
  exactly this reason before the fixtures were re-wound. `sweep_capsule` orients
  its normal toward the capsule and hides the problem, which is what made it
  confusing.
* **`landingSpeed` is post-clip.** `_slide` clips the caller's velocity into the
  contact plane *before* `move()` reads `-min(0, velocity.y)`, so the
  controller's own landing speed is 0 on a clean landing. That is the source's
  behaviour and exactly why `movement.js`'s `_postMove` maxes it against its own
  pre-move `prevVy`. Pinned by name.
* **Crouch is a toggle, not a hold.** `update_stance` reads `cmd.crouchPressed`,
  the press *edge*. Releasing `C` does not stand you up; pressing it again does.

## Verified

* `cargo test -p axiom-claude-of-duty` — 762 tests across 22 binaries, all green
  (356 in the lib, up from 306).
* `cargo xtask check-architecture` — OK.
* In a real browser (WebGPU backend, console error-free): the street runs away
  from the camera with its kerbs and pavements, the sky is the resolved daylight
  blue, the eye rides at 1.66 m over the road, `W` walks you forward past the
  manhole, and the M4 is lying on the asphalt three metres ahead at real scale.
