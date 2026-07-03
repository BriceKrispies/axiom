# axiom-animation — Architecture

`axiom-animation` is an **isolated engine module** that owns reusable skeletal-
animation *mechanics*. It is one of the engine's core mechanism modules
(alongside `scene`, `resources`, `render`, `physics`, `input`, `audio`).

## What this module owns

- **Skeletons** — a parented set of bones (`Skeleton`, `Bone`, `BoneId`). Bones
  are stored in parent-before-child insertion order, so every bone's parent has
  a strictly smaller id.
- **Poses** — one *local* transform per bone (`Pose`), and their resolution to
  absolute *model* space (`ModelPose`) by a single forward pass over the
  parent-ordered bones.
- **Clips** — per-bone keyframe tracks (`AnimationClip`, `Keyframe`) sampled at a
  deterministic `axiom_kernel::Tick`. Sampling holds the first/last key outside
  the clip's range and interpolates between keys inside it.
- **Blending** — a deterministic per-bone interpolation of two equal-length
  poses at an `axiom_kernel::Ratio`.
- **Joint limits** — anatomical per-bone `JointLimit`s (per-axis Euler `min`/
  `max`) that clamp a pose's bone rotations back into a valid range (a knee that
  only hinges, an elbow that cannot hyperextend), plus an `is_pose_legal` query.
- **Clip events & phases** — timed markers on a clip carrying an **opaque**
  `u32` code the *game* assigns and interprets (a strike, a footstep, a wind-up
  span). The mechanism carries and reports codes; it never names their meaning.
- **Forward kinematics** — model-space resolution (`resolve_model`) composing a
  pose down the parent chain, exposing per-bone world transforms and joint
  positions.

Rotation interpolation is delegated to the math layer's shortest-arc
`axiom_math::Quat::nlerp`; translation and scale interpolate componentwise.
Joint-limit clamping decomposes/recomposes rotations through the math layer's
`Quat::to_euler_xyz` / `Quat::from_euler_xyz`.

Everything is reached through the single facade **`AnimationApi`** plus its
identity vocabulary **`SkeletonId` / `ClipId` / `BoneId`**. No other type is
exported at the crate root.

## What this module must **not** know

- **No other engine subsystem.** It does not import or name `scene`,
  `resources`, `render`, `physics`, `input`, or `audio`. It knows nothing about
  scene nodes, meshes, materials, GPU resources, rigid bodies, input actions, or
  audio.
- **No platform.** No `web_sys`/`js_sys`/`wasm_bindgen`, no WebGPU/WebGL, no
  canvas/DOM/browser globals.
- **No nondeterminism.** No wall-clock time, no randomness, no global mutable
  state, no hash-ordered collections — only insertion-ordered `Vec` storage and
  the explicit `Tick` a caller supplies.
- **No meaning.** No character names, no humanoid assumptions, no gameplay
  vocabulary (`kicker`, `goalie`, `soccer`, `enemy`, …). *Engine owns mechanism;
  games own meaning.* A game decides which clip a character plays; this module
  only samples and blends whatever skeleton/clip it is handed.

## Allowed layers

`allowed_layers = ["kernel", "math"]`, `allowed_modules = []`.

- **kernel** — stable ids, `Tick` (sampling time), `Ratio` (blend factor), and
  the deterministic error/result vocabulary the module's `AnimationError` builds
  on.
- **math** — `Transform`, `Quat`, `Vec3`, and `Quat::nlerp` for all pose maths.

Only these two are declared because they are the only layers **genuinely used**
(the `engine_genuine_dependency` dylint rejects a declared-but-unused
dependency). `runtime` and `frame` are deliberately *not* dependencies: a clip
is sampled against a caller-supplied `Tick`, so the module needs neither the
runtime stepping vocabulary nor a full engine frame.

## Why it is a module, not a layer

A layer is a broad, ordered rung of the engine spine that many things build on.
Animation is a **self-contained capability** with a narrow facade — it does not
sit *underneath* other engine code, and nothing in the layer DAG needs to depend
on it. That is exactly the shape of an isolated engine module: composed by apps
and games, never by the spine.

## Why it imports no other modules

Two engine modules can never share a Rust type they each name — that is the
Module Law's isolation rule, and it is what keeps each module a re-composable
black box. Animation therefore takes and returns only **layer** value types
(`Transform`, `Tick`, `Ratio`) plus its own ids. An app is the place that reads
a `ModelPose` out of `AnimationApi` and writes the resulting `Transform`s into
`axiom-scene` — the animation module never reaches across to the scene itself.

## How apps translate into and out of the facade

1. **Author** a skeleton: `create_skeleton()`, then `add_root_bone` /
   `add_child_bone` with each bone's rest local `Transform`.
2. **Author** clips: `create_clip()`, then `add_track(clip, bone, keyframes)`
   where each keyframe is a `(Tick, Transform)`.
3. **Per frame**, `sample(skeleton, clip, tick)` → a `Pose`; optionally `blend`
   two sampled poses; then `resolve_model(skeleton, &pose)` → a `ModelPose`.
4. **Translate out**: read `ModelPose::transform(bone)` (a `Transform`) and write
   it into the app's scene as a node transform. This translation is app glue —
   it lives in the app, never in this module.

## Content lives in the app, not here

The reusable *mechanism* stops at skeletons/poses/clips/limits/events/blending.
The **meaning** — a specific humanoid rig, an authored soccer kick, named kick
phases, a "kick contact" event — lives in an app that authors it through this
facade. `apps/axiom-animation-lab` is the reference consumer: it builds an
18-bone humanoid and a `kick_right` clip entirely through `AnimationApi`, maps
opaque event/phase codes back to named concepts, and renders the kick as SVG.
Nothing in this module knows the word "kick".

## Deliberately deferred (not implemented here)

This module is the deterministic *contract*, not a full animation engine. The
following are intentionally **out of scope** and noted so a future agent does
not assume they exist:

- **`Animator`** — a stateful play-head that advances clip time frame to frame.
- **`AnimationGraph` / `AnimationStateId`** — a blend/state machine over clips.

Both are higher constructs that would build *on top of* this sampling/blending
contract (in a feature module or an app), not inside this isolated mechanism.
