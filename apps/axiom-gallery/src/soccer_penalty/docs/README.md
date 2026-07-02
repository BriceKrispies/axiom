# axiom-soccer-penalty

A deterministic, fixed-camera, **retro 32-bit-style soccer penalty-kick static diorama**.
This is the first stage of a penalty-kick game: it establishes the game
*visually* before any gameplay exists.

## What this is

`axiom-soccer-penalty` builds a single, frozen scene from fixed constants:

- a kicker (low-poly primitive puppet) standing behind the ball,
- the ball on the penalty spot,
- a goalie standing in the goal in a static ready stance,
- goal posts, a crossbar, and a rear/front net grid,
- a green field with a penalty box, goal area, goal line, and penalty spot,
- a stadium wall with a fake crowd and ad boards (one reads **AXIOM**),
- a chunky arcade HUD: score, round, best score, a shot-power meter, and an
  aim reticle over the goal.

Nothing moves. The app's entire job is to produce three deterministic
artifacts: an ordered **object list**, a backend-neutral **render plan**
(draw list + camera + lighting), and a static **HUD model**.

Entry point:

```rust
use axiom_soccer_penalty::SoccerPenaltyApp;
let stage1 = SoccerPenaltyApp::build_stage1(); // objects + render_plan + hud
```

Runnable dump:

```sh
cargo run -p axiom-soccer-penalty --example print_stage1
```

## Why this is an app, not an engine layer or module

This crate is a **composition leaf** (an Axiom *app*). It invents no reusable
engine capability — it only arranges fixed data (positions, sizes, colors, HUD
values) into a diorama descriptor and an ordered draw list. Soccer, goalie,
shot, score, and HUD are *gameplay/authoring* concepts, and per Axiom's
architecture they must never leak into the kernel, runtime, math, scene, or
render layers. They live here, and only here.

Its single engine dependency is the **math** layer's public `Vec3` facade, used
for every spatial quantity. It depends on no engine module.

Because the primitive/mesh-part vocabulary, palette, HUD model, and draw list
have no home in an existing engine module yet, they are defined **app-locally**
as the smallest deterministic data shapes needed to build the diorama. This is
temporary app glue: a later stage translates it through the real `axiom-scene`,
`axiom-render`, and `axiom-webgpu` facades (see `STAGE_1.md`).

## What Stage 1 contains

- Fixed constants for the camera, field dimensions, goal dimensions, and every
  object placement.
- A deterministic object builder that emits ~89 flat-shaded primitives with
  stable, sequential ids.
- A render plan with a total, reproducible draw order. **Pass 2** makes this an
  explicit 14-layer ordering model with a `(layer, coarse depth bucket, stable
  object ordinal)` sort key and a rear/front net split for retro 32-bit-style fake depth
  — see `PASS_2_DEPTH_ORDERING.md`.
- **Pass 3** adds a deterministic visual style: a fixed flat-shading light model
  with quantized brightness bands, a named material palette, faked per-actor
  blob shadows, an unlit HUD, and a retro 32-bit style descriptor (low internal
  resolution, pixel snapping, nearest filtering; no PBR, no dynamic shadows) —
  see `PASS_3_LIGHTING_AND_STYLE.md`.
- **Pass 4** adds deterministic aim + shot-power interaction: an app-local
  `PenaltyInputIntent`, a fixed-tick `Aiming`/`Charging`/`LockedPreview` state
  machine that moves the aim reticle in the goal mouth and charges the power
  meter (freezing a `PenaltyShotPreview` on release), and a HUD derived from
  that state — see `PASS_4_AIM_AND_POWER.md`.
- **Pass 5** adds a deterministic ball trajectory: on release the ball launches
  and follows a fixed parametric arc from the penalty spot to the mapped
  goal-plane target (`BallInFlight` → `ArrivedAtGoalPlane`), with a tracking
  blob shadow and a trail — see `PASS_5_BALL_TRAJECTORY.md`.
- **Pass 6** adds deterministic static goalie save volumes
  (`LeftHand`/`RightHand`/`Torso`/`Body`, in that priority order) and
  ball-sphere contact detection during flight, producing neutral
  `Hand`/`Torso`/`Body`/`None` contact facts and a `ContactDetected` state, with
  off-by-default debug visualization — see `PASS_6_GOALIE_SAVE_VOLUMES.md`.
- **Pass 7** makes the goalie a 16-part articulated primitive puppet that plays
  one of five deterministic dive pose clips (chosen from the locked shot target),
  with the Pass 6 save volumes attached to the animated hands/torso/pelvis so
  contact detection runs against the *posed* keeper — see
  `PASS_7_GOALIE_POSE_CLIPS.md`.
- **Pass 8** resolves each shot into a final deterministic `Goal`/`Save`/`Miss`/
  `Post` (priority: goalie contact → post/crossbar → goal mouth → miss), adds a
  `Resolved` state, and shows the result on the HUD — see
  `PASS_8_SHOT_RESULT_RESOLUTION.md`.
- **Pass 9** completes the playable loop: deterministic scoring (base + power/
  placement/streak bonuses), a 5-round session with continue/reset, round
  history, an app-local best score, and a `SessionComplete` summary, all shown
  on the HUD — see `PASS_9_SCORING_AND_LOOP.md`.
- **Pass 10** adds deterministic impact polish: net wobble (Goal), post/crossbar
  shake (Post), a save impact flash + fake ball deflection, a miss drift, crowd
  reaction, additive camera juice, and an animated result banner + score popup —
  all tick-driven descriptors that never change the Pass 8 result or Pass 9
  score. **No physics, no particle engine, no persistence** — see
  `PASS_10_IMPACT_POLISH.md`.
- A static HUD model (`SCORE 1250`, `ROUND 3 / 5`, `BEST 2520`, a 10-segment
  power meter, and an aim reticle over the goal).
- A single directional light + flat ambient, described as data.
- Blob shadows scaffolded as flat translucent quads under kicker, ball, goalie.

## What Stage 1 deliberately does NOT contain

- No gameplay: no shooting, no goalie dives, no save/goal/miss logic, no
  scoring logic, no round progression.
- No animation: no skeletal rig, no pose clips, no tweening — the puppets are
  static primitive boxes.
- No physics: no ball arc, no collision volumes, no net reaction.
- No real assets: no glTF import, no texture/mesh loading, no asset pipeline.
- No ECS / world / game-framework primitives.
- No GPU/WebGPU/WebGL/browser code, and no binding to `axiom-scene` /
  `axiom-render` / `axiom-webgpu` yet — the render plan is backend-neutral data.
- No camera controls; the camera is fixed.
- No randomness and no wall-clock time anywhere.
