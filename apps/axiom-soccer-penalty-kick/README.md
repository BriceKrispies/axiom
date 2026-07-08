# Axiom — Soccer Penalty, authored purely in TypeScript

A retro 32-bit **soccer penalty shootout**, the **TypeScript-only successor to the
Rust `soccer_penalty` gallery demo**: the same fixed-camera diorama (kicker behind
the ball, diving keeper, posts + net, lined pitch, stadium wall, crowd, ad boards),
the same aim + power interaction, physics-arc ball flight, five authored goalie
dive lanes with save volumes, goal/save/miss/post resolution, a 5-round scoring
session with impact effects, and a run-up-and-strike kicker — but with **no Rust in
this app at all**. The whole game is the TypeScript under `web/src/`, driving the
*real* engine (the shared `axiom-game-runtime` wasm) through the `@axiom/game` SDK's
3D scene-authoring, input, and camera surfaces; the engine renders the retained 3D
scene to the canvas via its live WebGPU → WebGL2 → Canvas2D backend.

It is the sibling of `apps/axiom-retro-fps-ts-browser` — the answer to "can a full
game be authored on Axiom in TypeScript instead of Rust?" — here a physics-driven
sports game with an articulated keeper and kicker.

## Run it

From the repo root:

```sh
# one-time prerequisites
( cd packages/axiom-game && npm install && npm run build )            # build the SDK to dist/
cargo build -p axiom-game-runtime --target wasm32-unknown-unknown --release
wasm-bindgen --target web \
  --out-dir apps/axiom-game-runtime/web/pkg \
  target/wasm32-unknown-unknown/release/axiom_game_runtime.wasm       # the shared wasm

# the dev loop (serves THIS app; reuses the wasm above)
AXIOM_DEV_APP=axiom-soccer-penalty-kick node scripts/axiom_dev_server.mjs  # http://localhost:8080
```

Open `http://localhost:8080`. **←/→** or **A/D** aim horizontally · **↑/↓** or
**W/S** aim height · hold **Space/K** to charge power, release to shoot · **Enter**
continue between rounds · **R** reset. Edit any `web/src/*.ts` and save — the
browser re-runs live within a few hundred milliseconds.

## Layout

| File | Role |
|------|------|
| `web/index.html` | The page: a 960×600 canvas + reticle/banner overlay + an import map pointing `@axiom/game` at the SDK's `dist/` build; loads `dist/harness.js`. |
| `web/src/harness.ts` | Host edge: `createGame` + `boot({ present3d })` wiring, the hot-reload (SSE) client, and the DOM HUD updater. |
| `web/src/game.ts` | The top-level wiring: maps input → intent, advances the session, builds the per-frame scene snapshot, drives the camera. Exposes `readHud()`. |
| `web/src/session.ts` | The 5-round session: awards, best score, round history, BetweenRounds / SessionComplete. |
| `web/src/interaction.ts` | The per-tick shot state machine (Aiming → Charging → LockedPreview → BallInFlight → …→ Resolved). |
| `web/src/ball.ts` | The physics-arc ball flight (semi-implicit Euler integrator + two-probe launch calibration), ported faithfully. |
| `web/src/goalie.ts` | The 16-part keeper puppet, five dive clips, dive-lane selection, and the save volumes + contact test. |
| `web/src/result.ts` `scoring.ts` `effects.ts` | Result classification, the points formula, and the impact-effect descriptors. |
| `web/src/kicker.ts` | The run-up-and-strike kicker box-man (see note below). |
| `web/src/scene.ts` | Builds the ~660 static renderables + dynamic actor handles; drives the fixed broadcast camera + two-light rig each frame. |
| `web/src/engine.ts` | The reusable, **game-agnostic** engine toolkit every other module builds on: Vec3/Quat/Transform math, articulated-hierarchy FK, ball-vs-sphere/AABB contact, the projectile integrator + two-probe launch solve, animation-curve sampling, and the `@axiom/game` mesh-catalog/transform adapters. Nothing here knows about penalties — it is the explicit "engine" side of the engine-vs-game split. |
| `web/src/hud.ts` `input.ts` `palette.ts` `scene-constants.ts` | The HUD model, input contract, material palette, and shared world geometry — the "game" data the modules above specialize `engine.ts` with. |

There is no `Cargo.toml` and no `app.toml` — this app is not a Rust crate. It is
pure TypeScript over the shared engine, which is the whole point.

## Faithful vs. adapted

The **gameplay is a faithful port** of the Rust `soccer_penalty` modules: every
constant, the aim/power integer math, the physics-integrated ball trajectory, the
goalie save volumes + dive lanes, the goal/save/miss/post priority, the scoring
formula, and the session loop are transliterated 1-to-1.

Two things are **adapted**, both honestly:

1. **Solid-color materials.** The SDK's 3D materials are lit solid colors (no
   texture maps), so the Rust recipe-baked textures (crowd faces, the jersey "10",
   the "AXIOM"/"SPORTS" ad boards, the ball's pentagon rosette) become their flat
   base colors. Everything else — geometry, palette, lighting, camera — is exact.
2. **The kicker animation.** The Rust kick is evaluated by the engine's
   animation-authoring forward-kinematics over a binary `.figure` asset whose box
   dimensions do not live in source. `kicker.ts` instead authors a
   visually-equivalent box-man kick to the *same* 9-phase timing (run-up → plant →
   backswing → hip drive → strike at tick 55 → follow-through → recover), as the
   port allows.

## Engine note — `frameLocked` and material upload

The harness boots with `frameLocked: true`. Beyond the once-per-frame pacing, this
is **required for correctness today**: the engine snapshots the material
bind-group set **once when the 3D surface binds**, and — unlike the mesh set, which
has a generation-based re-upload — it never re-uploads materials. Real-time pacing
can bind the surface on a first frame that advanced *zero* sim ticks, i.e. before
this game registers its ~27 materials, leaving them unuploaded and their draws
silently skipped (only the handful of pre-existing demo materials render).
`frameLocked` makes the first frame advance one tick — building the whole scene and
every material — before the bind. The clean structural fix is a **material-set
generation re-upload** in the engine, mirroring `mesh_generation`
(`GameBridge` / `WindowingApi::update_present_meshes` / `*BackendApi::load_meshes`);
that is a spine follow-up, tracked here.
