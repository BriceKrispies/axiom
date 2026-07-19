# Home Run! — GDScript / Godot 4

An idiomatic **Godot 4.6 / GDScript** version of Axiom's pure-TypeScript
**Home Run!** app (`apps/axiom-home-run`). Same toy-tabletop diamond, same
always-armed swing, same deterministic seeded pitch sequence, same home-run
cinematic — built the way a Godot project wants to be built.

This is an experiment in retargeting an Axiom app onto a different engine. It is
**not** part of the Axiom engine graph (not a layer/module/app/tool, not a Cargo
package) — it lives under `ports/` and is ignored by the architecture checker and
the coverage gate.

![the opening frame](docs/screenshot.png)

## Idiomatic Godot, not an engine-on-Godot

The port started as a faithful reconciler (rebuild a flat instance list every
frame, diff it into nodes — the web engine's shape). This version is rebuilt to
Godot conventions:

- **Authored scene tree** (`main.tscn`): `Camera3D`, two `DirectionalLight3D`s,
  a `WorldEnvironment` (+ an `Environment` sub-resource carrying the grade), the
  `Field` roots, and the `HUD` `CanvasLayer` — real nodes, not code-spawned.
- **Static geometry as `MultiMeshInstance3D`**: the whole grandstand + field is
  baked once into one MultiMesh per mesh×material group and never touched again.
- **Persistent, pooled actor nodes**: the batter, machine, ball (+ trail), and
  ten fielders are reusable actor views built once and re-posed each tick — no
  per-frame spawn/despawn, no per-frame allocation.
- **InputMap actions** (`move_left`/`move_right`/`swing`/`restart`) with
  `Input.is_action_just_pressed`, not polled keycodes.
- **Fixed-step sim in `_physics_process`** (the engine's 60 Hz tick), not a
  hand-rolled accumulator.
- **Typed GDScript throughout**: every sim record is a class (`Swing`, `Contact`,
  `PitchSpec`, `BallFlight`, `Fielder`, `SwingOutcome`, `Cinematic`, `SceneView`,
  …) with typed fields and methods — no `Dictionary` records.
- **Signals**: gameplay events are re-emitted as a `feedback` signal the HUD and
  audio connect to (event-driven), separate from the polled per-frame state.

Cross-script references use `preload()` consts (also used as types) rather than
`class_name`, so the project compiles and runs on the **very first launch** with
no prior editor/import pass. `hash01` is reproduced bit-for-bit, so **a given seed
produces the same round as the TypeScript original** (seed 1 opens with a
44 MPH slow ball).

## Structure

```
project.godot            # Godot 4.6 project (gl_compatibility / WebGL2)
main.tscn                # authored scene: camera, lights, environment, field, HUD
scripts/
  constants.gd           # every tuning number
  math_util.gd           # hash01, euler->quaternion, scalar helpers
  cinematic_constants.gd # the home-run cinematic tuning
  # --- typed sim (engine-agnostic) ---
  swing.gd  contact.gd  pitch.gd  ball.gd  fielders.gd
  swing_outcome.gd  cinematic.gd  cinematic_camera.gd
  feedback.gd  pitch_result.gd  scene_view.gd
  session.gd             # HomeRunSession — the round state machine
  # --- presentation (Godot) ---
  materials.gd           # the StandardMaterial3D palette
  sun.gd                 # wall-clock sun (light + shadow projection)
  stadium.gd             # static geometry -> MultiMeshInstance3D
  parts.gd               # shared part-creation / posing helpers
  batter_view.gd  machine_view.gd  ball_view.gd  fielder_view.gd
  hud.gd                 # the on-screen overlay
  audio.gd               # runtime tone-synth cue player
  canvas2d_renderer.gd   # software "3D attempt" backend (toggle with B)
  game.gd                # Main: loop, input, camera, lights, signals, backend switch
serve_web.py             # static server with COOP/COEP headers
dev_web.py               # hot-reload dev server (watch -> re-export -> SSE reload)
export_presets.cfg       # Web export preset
```

## Controls

- **A / D** (or **←/→**) — shift the batter in the box
- **SPACE** — swing (also starts the round); the bat re-winds on its own (cooldown)
- **ENTER** — restart once the round is over
- **B** — toggle the software **canvas2d "3D attempt"** renderer (see below)

## Two render backends

Besides Godot's real GPU render (WebGL2 on the web), the port ships a hand-rolled
**software rasterizer** — `scripts/canvas2d_renderer.gd`, the direct analogue of
Axiom's `canvas2d` backend. It reads the *same* 3D scene nodes, projects every
box/cylinder/sphere to screen itself (view+projection math in GDScript, with
near-plane clipping), painter-sorts the faces, and fills them as flat-shaded 2D
polygons/circles via Godot's `CanvasItem` API. The 3D *math* is software; only the
final 2D fills are accelerated — exactly like the original's canvas2d path, which
bypasses WebGL rather than routing through it. Toggle it live with **B** (or export
arg `-- canvas2d`); the host blanks the real 3D (`Camera3D.cull_mask = 0`) and shows
the `Node2D` overlay instead. Godot has no built-in software-3D backend, so this is
a parallel renderer, not a flag on Godot's pipeline.

## Run

Open `project.godot` in Godot 4.6+, or from the CLI:

```sh
godot --path .
```

### Web / WebAssembly build

Export needs the **standard (non-mono) Godot 4.6** build + its web export
templates (the `.NET`/mono build does not ship web templates). Then:

```sh
godot --headless --path . --export-release "Web" dist/index.html
python serve_web.py 8060 dist          # COOP/COEP headers for threaded wasm
# open http://localhost:8060/
```

`serve_web.py` sends the cross-origin-isolation headers Godot's threaded web
build needs (a plain `python -m http.server` won't).

**Hot reload:** `dev_web.py` is the `axiom-serve` loop for this port — it watches
`scripts/*.gd` + the scene/config, re-exports the Web build on save (standard
Godot), and live-reloads the browser over SSE. The mono Godot can't export web, so
point it at a standard binary:

```sh
GODOT_WEB_BIN=/path/to/standard/godot python dev_web.py 8060 dist
# edit any script under scripts/ -> the page reloads itself
```

### Deterministic screenshot

Pass args after `--` to capture one frame and quit
(`shot <frame> [out.png] [seed] [swingAt]`):

```sh
godot --path . --rendering-driver opengl3 --resolution 1024x700 \
  -- shot 70 shot.png 1 -1
```

## Fidelity notes

- Renderer is `gl_compatibility` (WebGL2 on the web). Lighting is the data-driven
  moving sun + a fill light; the game bakes its own translucent ground-shadow
  ellipses, so Godot's real-time shadows stay off, exactly as in the original.
- Godot gamma-brightens midtones vs the original's flatter Lambert, so a small
  ambient reduction + brightness/contrast trim in the `Environment` brings the
  toy palette back toward the original's dusk mood.
