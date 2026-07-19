---
name: godot-port
description: Migrate an Axiom pure-TS app (the heat-check/home-run shape — an SDK-free deterministic core + one declarative view.ts that returns keyed box/sphere/cylinder instances) into an idiomatic Godot 4 / GDScript port under ports/godot-<app>/. Produces a typed-GDScript sim, an authored scene tree with MultiMesh static geometry + pooled actor views, a hand-rolled canvas2d software "3D attempt" renderer (Axiom's canvas2d analogue), and a Web/WASM build with LAN/phone serving + hot reload. Reference implementation: ports/godot-home-run. Invoke with the app name (e.g. "/godot-port home-run" or "apps/axiom-home-run").
---

# godot-port

Turn an Axiom pure-TypeScript app (`apps/axiom-<name>/web/`) into an **idiomatic
Godot 4.6 / GDScript project** at `ports/godot-<name>/`. Not a faithful
reconciler-on-Godot — a project built the way Godot wants: authored scene tree,
`MultiMeshInstance3D` static geometry, persistent pooled actor nodes, `InputMap`
actions, sim in `_physics_process`, typed GDScript classes, signals — plus a
hand-written **canvas2d software renderer** (the direct analogue of Axiom's
`canvas2d` backend) and a Web/WASM export you can play on a phone.

**`ports/godot-home-run/` is the reference implementation and the source of
truth.** It was built this way and verified running on desktop and on a phone
over the LAN. MIRROR ITS STRUCTURE file-for-file; if it has fixes newer than this
skill, follow the port, not the skill.

Placement: `ports/` is outside the Cargo workspace and the engine graph (no
`Cargo.toml`, no `layer.toml`/`module.toml`/`app.toml`), so the architecture
checker, coverage gate, dylint, and ts-gate all ignore it. It commits to `main`
like everything else. `.gitignore` it: `.godot/`, `/dist/`, `/.certs/`.

## Why the port is clean (and when it isn't)

The heat-check convention already splits the app into **pure logic + declarative
rendering**, which is exactly what ports cleanly:

- The SDK-free core (`vec`/`hash`, `constants`, `types`, gameplay modules,
  `session`) is engine-free arithmetic driven by a deterministic integer hash →
  ports to typed GDScript almost line-for-line.
- `view.ts` describes the whole scene every frame as a flat list of keyed
  `box`/`sphere`/`cylinder` instances + named materials + a camera → maps **1:1**
  onto Godot primitives.

**Gate (STOP and tell the user if any fail):** the app must render only
box/sphere/cylinder primitives with flat/emissive materials (no custom shaders,
no imported meshes, no GPU postcards), and its sim must be pure logic (no
`sim.world`/ECS/`sim.physics` the port can't own app-side). A `draw2d`/2D app is
a different, simpler job — do not improvise it here.

## Inputs

- **The target app** — a name (`home-run`, `heat-check`) or a path
  (`apps/axiom-home-run`). Read its `web/src/*.ts` in full first: `view.ts`
  (the scene builder + material palette + camera + lights), `constants.ts`,
  the sim modules, `session.ts`, `game.ts` (materials/input/audio), `harness.ts`.

## Step 0 — Read the whole app, then the reference port

1. Read every `web/src/*.ts` of the target. Note: the mesh/material conventions,
   the camera (pos/target/fov), the lights (and any wall-clock sun), the
   `hash01` implementation, the session's public surface (`advance`/`view`/HUD
   accessors), the audio cue table, the input→intent mapping.
2. Read `ports/godot-home-run/` end to end. Every decision below is embodied
   there; you are re-applying its shape to a new app.

## Step 1 — Scaffold `ports/godot-<app>/`

Copy the shape from `ports/godot-home-run/`:

- `project.godot` — `renderer/rendering_method="gl_compatibility"` (WebGL2 on
  web, portable), viewport size, `msaa_3d=1`. `config/features=("4.6")`.
- `main.tscn` — the authored scene tree (Step 3).
- `scripts/` — the ports (Steps 2–4).
- `icon.svg`, `.gitignore`, `README.md`.

## Step 2 — Port the pure sim to TYPED GDScript classes

One class per file, `extends RefCounted`, typed fields in **snake_case**, no
`Dictionary` records. Vectors are `Vector3`, rotations `Quaternion`.

**The critical gotcha — use `preload()` consts, NOT `class_name`.** `class_name`
registers a global that only exists after Godot has *imported* the project (the
`.godot/global_script_class_cache.cfg`). Launching `godot --path .` on a fresh,
never-opened checkout parses the main scene *before* that cache exists, so every
`class_name` reference fails ("Could not find type …"). Instead, every file
references its dependencies — and ITS OWN type, for self-returning methods — via
`const Swing = preload("res://scripts/swing.gd")` (self-preload is allowed and
resolves at parse time). This compiles and runs on the very first launch with no
editor/import pass. Do not reintroduce `class_name`.

- **`hash01` must be bit-exact.** JS `Math.imul` → `(a & 0xFFFFFFFF) * (b &
  0xFFFFFFFF) & 0xFFFFFFFF`; `>>>` (unsigned shift) → keep the value masked to
  32 bits (non-negative) and use `>>`. Same seed must reproduce the same round
  as the TS original — verify it (Step 6).
- **`quatFromEulerXyz`** → port the component-wise formula verbatim into
  `Quaternion(x, y, z, w)`; do NOT trust `Basis.from_euler`'s ordering to match.
- **Records → classes:** e.g. `Swing`, `Contact`, `PitchSpec`, `BallFlight`,
  `Fielder`, `SwingOutcome`, `Cinematic`, `SceneView`, `Feedback`, `PitchResult`.
  Static factories (`Swing.ready_swing()`), immutable-step methods returning new
  instances (`func step(...) -> Swing`), in-place mutation only where the TS
  mutated in place (flight, fielder roster, trail).
- **`undefined` → `null`.** A typed object field is nullable (`var _spec:
  PitchSpec = null`); test `!= null`, not `.is_empty()`. Config blobs (the
  cinematic tuning, the pitch-profile table) may stay as plain const
  Dictionaries — those are config, not flowing records.
- Drop the web-only `session.clone()` (Godot advances state in place).
- `session.view()` returns a typed `SceneView` the presentation reads.

## Step 3 — Idiomatic presentation

**`main.tscn` (authored, not code-spawned):** `WorldEnvironment` (+ an
`Environment` sub-resource carrying the grade), `Camera3D`, two
`DirectionalLight3D` (Sun, Fill, `shadow_enabled = false`), `Field` → `Static`
+ `Actors`, and a `HUD` `CanvasLayer` → a full-rect `Control` (`anchors_preset
= 15`, `anchor_right/bottom = 1.0`) running `hud.gd`.

- **Static geometry → `MultiMeshInstance3D`** (`stadium.gd`): collect the fixed
  instances (ground/stands/panels/patrol) into one MultiMesh per
  `mesh|material` group, built once. `material_override` provides the color;
  `cast_shadow = OFF`.
- **Dynamic actors → persistent pooled `Node3D` views** (`batter_view.gd`,
  `machine_view.gd`, `ball_view.gd`, `fielder_view.gd`): each builds its
  `MeshInstance3D` parts once and `pose()`s them each tick — no per-frame
  spawn/despawn, no per-frame allocation. Instance the reusable actor view (the
  fielder ×N). Toggle visibility for parts that come and go (ball, trail).
  `parts.gd` holds the shared `make`/`pose_box`/`pose_orb`/`pose_shadow`/
  `ground_y_at` helpers.
- **Mesh conventions** (match the SDK's, so the app's numbers work unchanged):
  `BoxMesh` size `(1,1,1)` (unit cube, scale = full extents); `SphereMesh`
  radius `0.5` height `1.0` (unit diameter, scale = 2r); `CylinderMesh`
  top/bottom `0.5` height `1.0`.
- **Transforms:** `mi.position/quaternion/scale` (Node3D composes T·R·S with
  local scale) is the reliable path; for MultiMesh, `Transform3D(Basis(quat) *
  Basis.from_scale(scale), pos)`.
- **Materials** (`materials.gd`): `StandardMaterial3D` with `roughness = 1`,
  `metallic = 0`, `specular_mode = DISABLED`, `diffuse_mode = LAMBERT` to
  approximate the flat toy look; `emission_enabled` for glow markings.
- **Sun** (`sun.gd`): a `SunState` from wall-clock ms → the directional light's
  colour/direction/energy AND the ground-shadow projection (dir + stretch) the
  actor shadow ellipses use.
- **Input:** register `InputMap` actions at runtime in `_ready`
  (`InputMap.add_action` + `InputEventKey` with `physical_keycode`), read via
  `Input.is_action_just_pressed` / `Input.get_axis`. Movement world-sign: the
  camera looks downfield, so negate the axis (world +X → screen-left).
- **Loop:** advance the sim in `_physics_process` (the project's 60 Hz physics
  tick) — NOT a hand-rolled accumulator in `_process`.
- **HUD as a scene node** (`hud.gd`): build the labels/bars in `_ready` under
  the authored `Control`, `update(session, view)` each frame from the session's
  accessors; feed one-shot outcome text via a `signal feedback(kind, text,
  big)` the HUD connects to. Add a small `Engine.get_frames_per_second()`
  readout — you WILL want it for the canvas2d perf work.
- **Audio** (`audio.gd`, a `Node`): an `AudioStreamPlayer` pool fed runtime
  `AudioStreamWAV` tones synthesized from the app's cue table (square/triangle/
  sine/sawtooth + a short attack/release envelope). A 2-arg method can't bind to
  a 3-arg signal — call `_audio.play(kind, big)` directly in the loop.
- **On-screen touch controls** (mobile/web): a `CanvasLayer` (above the HUD)
  with move `<`/`>` buttons and a big **SWING** button, shown only when
  `DisplayServer.is_touchscreen_available() or OS.has_feature("web")`.
  `focus_mode = NONE` so they don't steal keyboard focus; `button_down`/`up`
  for held movement, `button_down` → a one-tick edge flag for swing (which also
  starts/restarts). Fold these flags into the same intent the keyboard produces.

## Step 4 — The canvas2d software "3D attempt"

`canvas2d_renderer.gd` — a `Node2D` that reads the SAME 3D nodes the GPU
pipeline draws (`MeshInstance3D` + `MultiMeshInstance3D` under `Field`) and
rasterizes them to 2D itself. This is Axiom's `canvas2d` backend re-done in
Godot's 2D layer: the 3D *math* is software (GDScript), only the final fills are
accelerated. Toggle with **B**, a `?backend=canvas2d` URL param
(`JavaScriptBridge.eval("location.search")` on web), or a `-- canvas2d` arg; the
host blanks the real 3D via `Camera3D.cull_mask = 0` and shows the overlay. Godot
has NO built-in software-3D backend — this is a parallel renderer, not a flag.

Projection & clipping:

- `proj = camera.get_camera_projection()`; `view =
  camera.get_camera_transform().affine_inverse()`; per vertex `clip = proj *
  Vector4((view * world).xyz, 1)`; screen from NDC.
- **Near-plane clip (Sutherland–Hodgman against `w >= W_EPS`)** — without it,
  the huge ground/sky boxes that straddle the camera project to garbage
  coordinates. Interpolate crossing edges with `Vector4.lerp`.

The five things that make it correct AND fast (it's CPU rasterization in
GDScript, on phones too — each one mattered):

1. **Emission as a per-channel FLOOR, not additive.** `color = max(albedo *
   shade, emission)`. Additive emission blows the sky/glow materials out to
   white in this LDR fill.
2. **Painter's sort by the FARTHEST vertex (max clip-`w`), not the average.**
   The ground/deck/sky span from in-front-of-camera to the far outfield; an
   average depth makes them sort "near" and paint over the stands + actors (and
   HIDE the batter/machine). Max-`w` draws the big floor/backdrop first.
3. **Backface culling** (`world_normal.dot(face_center - cam_pos) >= 0` → skip).
   Halves the faces AND removes intra-convex-object overlap, so you never need a
   per-face sort within a box/cylinder.
4. **Static cache.** The stadium is fixed geometry — project it once into cached
   packed arrays and reuse until `view`/`proj` actually change. (Camera dolly/
   shake/cinematic invalidate it; that's fine, those are brief.)
5. **Batch into ONE draw call.** Triangulate every face into a single
   `PackedVector2Array` + `PackedColorArray` and draw with
   `RenderingServer.canvas_item_add_triangle_array(get_canvas_item(), idx, pts,
   cols)` using an **exact-size sequential index buffer** (build `idx` sized to
   `pts.size()`; count defaults to -1 = all). Per-face `draw_colored_polygon` is
   thousands of draw calls = single-digit FPS. Getting the index buffer wrong
   renders NOTHING (blank but "fast") — size it exactly.

Two more that bite:

- **Degenerate polys** (thin decals seen edge-on) make `draw_colored_polygon`'s
  triangulator spam "triangulation failed". Skip any face whose 2D signed area
  is `< ~1 px²`.
- **Coplanar markings** (foul lines, bases) sit a hair above the field; per-
  object painter's sort lets the grass cover them → they "don't render". Tag
  those materials (`mat.resource_name = "mark"` in `materials.gd`) and draw the
  static markings as a SEPARATE top layer after the field. (Harmless on the GPU
  path, which has a real depth buffer.)

**Honest limitation to state up front:** this is a per-object painter's sort, not
the per-pixel **z-buffer** Axiom's `canvas2d` uses. It cannot generally resolve
arbitrary overlapping surfaces; the marking-layer split is a targeted patch that
works because you know the scene's layers. A real software z-buffer is the robust
fix, at real perf cost — offer it, don't ship it silently.

## Step 5 — Web / WASM export + LAN / phone serving

- **The mono/.NET Godot build CANNOT export web** — its export-template `.tpz`
  ships every platform except web. Use **standard (non-mono) Godot 4.6** + its
  standard export templates (the `.tpz` contains `web_*.zip`). Install the web
  templates into `%APPDATA%/Godot/export_templates/<version>/` (extract just
  `web_*.zip` + `version.txt`). A pure-GDScript project opens/exports fine in
  standard Godot.
- `export_presets.cfg` (Web preset). Export: `godot --headless --path . --export-release "Web" dist/index.html`.
- **`variant/thread_support = false`.** The threaded build needs a *secure
  context* (`SharedArrayBuffer`/cross-origin isolation); the no-threads build
  doesn't — required for LAN/phone over plain conditions.
- **Godot's web build refuses to run outside a secure context** (localhost or
  HTTPS). Over plain HTTP to a LAN IP it dies with "Secure Context - use HTTPS".
  So for a phone: **serve over HTTPS** with a self-signed cert whose SAN includes
  the LAN IP. Generate with `MSYS_NO_PATHCONV=1 openssl req -x509 ... -subj
  "/CN=<ip>" -addext "subjectAltName=IP:<ip>,DNS:localhost"` (Git Bash mangles a
  leading-`/` subj without `MSYS_NO_PATHCONV=1`). Accept the one-time warning on
  the phone.
- `serve_web.py` (static + COOP/COEP headers + wasm MIME; bind `0.0.0.0` for
  LAN) and `dev_web.py` (the axiom-serve loop: watch `scripts/*.gd` + scene/
  config → re-export with standard Godot → **SSE live-reload** the browser;
  serve HTTPS when `.certs/{cert,key}.pem` exist). A Windows Firewall inbound
  rule for the port needs admin. Launch the server **detached**
  (`Start-Process -WindowStyle Hidden`) so it survives your session.

## Step 6 — Verify (all of it, not just "it exported")

1. **Fresh-checkout parse** — `rm -rf .godot; godot --headless --path .
   --quit-after 40` (no import pass). This catches GDScript errors AND the
   `class_name`-vs-`preload` first-launch trap.
2. **Determinism** — a headless `--script` (a `SceneTree` with `_initialize`)
   that advances the session for a known seed and prints the outcome; confirm it
   matches the TS original (home-run seed 1 → first pitch "SLOW BALL", ~44 MPH,
   no-swing → STRIKE).
3. **Screenshot** — a `-- shot <frame> [out.png] [seed] [swingAt] [canvas2d]`
   affordance in `game.gd` (scripted input, then `await
   RenderingServer.frame_post_draw` → `get_viewport().get_texture().get_image()
   .save_png()` → quit). Render with `--rendering-driver opengl3` and compare to
   the app's reference render for BOTH backends.
4. **Browser** — serve + open; verify the B toggle flips WebGL↔canvas2d, the
   touch controls work, hot reload reloads on save, and the fps readout is
   healthy. Don't run a desktop Godot shot against the same project while the
   dev server is exporting (they collide on `.godot`); use a second plain-HTTP
   server on `localhost` (a secure context, no cert needed) for your own checks.

## GDScript landmines (these bit repeatedly — pre-empt them)

- **`:=` inference fails on any `Variant` RHS:** Dictionary member access
  (`dict.field`), loop vars over untyped arrays (`for side in [1,-1]`),
  ternaries with a Variant branch, and the global `min()/max()` (which return
  Variant). Fix: annotate (`var x: float = …`), type the loop
  (`for side: int in [1,-1]`), and use `minf/maxf/clampf/mini/maxi/clampi`.
- **Self-referential types need self-`preload`** (see Step 2) — `class_name` is
  banned here for the first-launch reason.
- **`git commit -F - <<EOF` inside a `git add … && …` compound can silently drop
  the commit.** Run `git commit` as its own command.
- **`.uid` files** (Godot 4.4+) are committed alongside their scripts.
- Commit only `ports/godot-<app>/` (exclude `dist/`, `.certs/`); the repo is
  direct-to-`main`. If unrelated WIP sits uncommitted in the primary worktree,
  stage only the port paths — it stays untouched.

## Divergence from the reference

If the target needs something `ports/godot-home-run/` lacks (a new primitive, a
z-buffered canvas renderer, a different cinematic), decide whether it's genuinely
app-specific or a shared improvement. Shared improvements belong back in the
reference port first (verified), then mirrored — the reference is the single
source of truth, and every port should read the same way.
