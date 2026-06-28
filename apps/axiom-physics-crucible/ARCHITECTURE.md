# Axiom Physics Crucible — Architecture

The crucible is an **app** (`apps/axiom-physics-crucible`), the only kind of crate
that is a composition *leaf*: nothing in the engine depends on it, and it is the
single place allowed to translate between two isolated modules' contracts. It
composes exactly two modules — `engine` (the `axiom` umbrella) and `physics`
(`axiom-physics`) — plus the `kernel` and `runtime` layers for the explicit
`RuntimeStep` / handle vocabulary the physics facade consumes.

```
app.toml:
  allowed_layers  = ["kernel", "runtime"]
  allowed_modules = ["engine", "physics"]
```

## The two-module boundary the app owns

The Module Law forbids one module importing another; an **app** is the sanctioned
place to bridge them. The crucible bridges physics → renderer, and that bridge
lives entirely here:

```
PhysicsApi (axiom-physics)                     axiom umbrella (engine)
  │  step() / snapshot() / latest_contacts()       ▲  spawn(Transform, Renderable)
  │  raycast() / overlap_sphere()                  │  Camera / Light / Material / Mesh
  ▼                                                │
CrucibleWorld ──► BodyState / ContactInfo ──► physics_to_render ──► RenderInstance ──┘
 (the only        StepCounts (app-owned        debug_geometry        (neutral render data)
  PhysicsApi       projections)                 debug_overlay
  chokepoint)
```

- **Physics never imports a renderer type**, and **the renderer never imports a
  physics type.** They meet only as app-owned value types.
- The translation functions (`physics_to_render::render_instances`,
  `debug_geometry::debug_instances`, `debug_overlay::status_markers`) are the glue
  the law keeps in the app, never inside a module.

## The single-facade consequence

`axiom-physics` exposes exactly one public facade (`PhysicsApi`) plus its handle
vocabulary (`PhysicsBodyHandle`, `PhysicsColliderHandle`). Its
snapshot / record / contact / material / shape types are **not exported**, so an
app **cannot name them**. The crucible respects this rather than fighting it:

- `CrucibleWorld` is the *only* code that touches `PhysicsApi`. It reads the
  unnameable snapshot/record/contact values as **inferred locals** and immediately
  projects them into app-owned value types — `BodyState`, `ContactInfo`,
  `StepCounts` (in `crucible_report.rs`) — which the rest of the app *can* name,
  compare, hash, and print.
- Body *kind* is likewise unnameable (the physics body-kind enum is private), so
  the app records its own `KindTag` at spawn time instead of reading it back. This
  is why translation colours bodies from the app registry, never from a physics
  query.

This is the "app glue inlines un-nameable plumbing" pattern: factor out only the
nameable translation, inline the value plumbing at one chokepoint.

## Determinism: two worlds

`Crucible` owns two `CrucibleWorld`s — `visible` (rendered) and `replay` (hidden).
Both are populated by the same stations and stepped with the same explicit
`RuntimeStep` sequence, so their projected `BodyState` vectors stay equal.
`replay_matches()` compares them; `perturb_replay_at()` injects one impulse into
the replay world so the Replay Bay can prove divergence is *detected*. The
guarantee being demonstrated is the one physics actually makes:
**same-binary replay**, not cross-platform byte-determinism.

## Source layout

| File | Responsibility |
|------|----------------|
| `crucible_station.rs` | The six-station enum + the 3×2 floor-grid layout (`origin()`). |
| `crucible_scenario.rs` | The scripted-scenario vocabulary (`BodySpec`, `CrucibleShape`, `MaterialSpec`, `CrucibleKind`) and the `Station` trait. |
| `physics_crucible_app.rs` | `CrucibleWorld` (the sole `PhysicsApi` chokepoint), the `Crucible` two-world harness, and `build_physics_crucible` (the rendered entry). |
| `crucible_report.rs` | The app-owned projections (`BodyState`/`ContactInfo`/`StepCounts`) and the `CrucibleReport`. |
| `physics_to_render.rs` | Body state → `RenderInstance` translation (app glue). |
| `debug_geometry.rs` | The neutral render vocabulary (`RenderInstance`, `CrucibleMesh`, `DebugShape`, palette) and shape expansion. |
| `debug_overlay.rs` | Report → status geometry (replay beacon + contact tally) and text lines. |
| `crucible_camera.rs` | Deterministic overview / per-station camera placement. |
| `body_bay.rs` … `replay_bay.rs` | The six `Station` implementations. |
| `lib.rs` / `main.rs` | Crate root (`all_stations`, `run_report`) and the headless binary. |

## What stays out of this app

No physics math, no collision logic, no rendering pipeline — those belong to the
modules. The app only *scripts*, *projects*, *translates*, and *reports*. It adds
no backdoor API to `axiom-physics`; everything flows through the existing public
facade.
