# Build Plan — reaching the north-star `App` API

This is the roadmap from today's spine to
[`rotating_cubes.rs`](rotating_cubes.rs): promote proven app-glue, one slice at
a time, into the correct layer/module until the demo apps are pure scene
description — then delete the sketch (the [README](README.md) "deletion test").

Every code commit ships **with the tests that take it to 100% coverage** and
keeps `cargo xtask check-architecture` green (plus the dylint rulebook). There
is no "tests later." Enforced by a pre-commit gate (architecture + coverage +
dylint).

## Status (2026-06-09)

| Slice | State |
|---|---|
| 1 — schedule backbone | ✅ complete |
| 2 — ergonomics (→ umbrella) | ✅ complete (+ `Transform::looking_at` for camera aiming) |
| 3 — windowing **engine** module | ✅ complete — module + live `wgpu` arm + rAF driver, screenshot-verified |
| 4 — `App` frontend | ✅ complete — `App::run()` is the terminal web entry, owning the loop via windowing |
| 5 — collapse apps, retire sketch | 🟢 ~85% — browser app is pure scene description on `App::run` (screenshot-verified); sketch retired; headless demo deliberately kept as the introspection harness |

**Fidelity:** shape & altitude match, **not** the verbatim deletion test — the
sketch's symbol names are illustrative (per the README); today's honest idioms
(`.setup`, `fixed_timestep_nanos`, the `Window` builder) stay. Slice 5's
"deletion test" is therefore judgment, not literal compilation.

**The rotating-cube north-star is realized:** the browser app is pure scene
description on `App::new()…run()`, screenshot-verified, with the engine owning
the window/GPU/pipeline/loop. The one open item is folding the headless demo's
**introspection** capability into `App` so it, too, collapses onto the high-level
API — its own slice, since it adds an engine feature rather than moving code.

## Settled architectural decisions

These three were open; they are now decided and are load-bearing for the
commits below.

1. **The Schedule lives on `axiom-ecs` (Layer 05).** `World::advance` already
   runs registered `WorldSystem`s once per engine frame; the Schedule is just
   *labelled phases* (`Startup`/`Update`) ordering that existing primitive. No
   new crate.
2. **Windowing is an *engine* module over plain data** —
   `modules/axiom-windowing`, `kind = "engine-module"`, `allowed_modules = []`,
   building only on the kernel and the `host` layer's presentation boundary,
   with a rule-9 platform-API allowlist exception (`PLATFORM_FACING_MODULES`).
   - **Correction (was: "feature module composing `webgpu`").** Module contract
     types are not nameable across modules: `axiom-webgpu` exposes only its
     `WebGpuApi` facade, and `GpuSubmission` is `pub` inside a *private* module,
     so another crate can thread it as an inferred local but can never name it in
     a signature. A presentation backend therefore cannot take a `&GpuSubmission`,
     so composing `webgpu` buys nothing. What actually reaches the GPU is plain,
     nameable data (per-draw `mvp:[f32;16]` + `color:[f32;4]` + clear colour,
     extracted from the render pipeline's report). Windowing operates on that +
     host presentation types, needs no module dependency, and is an isolated
     engine module. The "one path" unifying artifact is the single
     `RenderPipelineApi.submit` → extracted plain draws → present seam, **not**
     `GpuSubmission` (so `axiom-webgpu` is untouched).
   - **No portable trait.** With one real backend (the wasm `wgpu` arm) and a
     native headless no-op, a `PresentationBackend` trait would be dead
     abstraction; `cfg`-dispatch on `present_frame`/`binding_is_ready` is the
     honest seam. Readiness is the presence of the optional live binding.
   - A platform **layer** was considered and rejected: the spine is strictly
     linear, each layer must adapt the one directly beneath it, and the
     browser-free engine-data layers (frame/ecs/introspect) may never use a
     platform layer — so no linear position exists for it. Rule #9 anticipates an
     allowlist extension, not a new layer.
3. **The prelude umbrella is just a feature module — no new tier, no law
   amendment.** Module Law #8 is enforced by counting `lib.rs` lines starting
   with `pub ` (`class_check.rs::check_module_facades_export_one`). A single
   `pub mod prelude;` is exactly one such line, so it satisfies #8. The `axiom`
   crate is therefore an ordinary **feature module**: `lib.rs` = `pub mod
   prelude;` (its one facade), `allowed_modules = [scene, resources,
   render-pipeline, windowing]`, depended on by apps (apps may depend on
   modules). A `prelude` is the idiomatic single entry point — honest, not
   gaming the rule. **Original commit 12 (sanction an umbrella tier) dissolves**;
   the only remaining law change in the effort is the windowing platform-API
   allowlist (commit 9).
4. **There is one canonical world, and it is `axiom-scene`'s.** Godot
   (`SceneTree`), Unity (the active scene), and Unreal (`UWorld`) all collapse
   "scene" and "world" into a single tree that engine systems and user scripts
   share. Axiom already does this: `axiom-scene`'s `World<SceneStorage>` *is*
   the entity/component world (nodes are entities, facts are columns). So the
   App frontend does **not** create a second world — it drives and extends
   scene's. User-defined components/systems plug into that world through the
   ecs `DynamicComponents` primitive when needed. Consequence: the original
   commits 3 (scene "registerable systems") and 4 (headless demo "through the
   schedule") dissolve — scene already registers `SpinSystem` +
   `TransformPropagation` and exposes `advance`; manufacturing a new scene API
   for them would be the speculative abstraction the engine forbids. The
   Startup/Update schedule from commit 2 is instead consumed and proven by the
   App frontend (slice 4).

## The commit series

`(law)` edits the mechanically enforced rules in `crates/xtask/src/`;
`(doc)` is prose only.

### Slice 1 — schedule backbone
1. **(doc)** add the north-star sketch, README, and this build plan. ✅
2. `axiom-ecs`: add labelled `Schedule` phases (`Startup`/`Update`) over
   `WorldSystem`; adapter-over-frame relationship preserved. ✅
3. ~~`axiom-scene`: expose spin + transform propagation as registerable
   systems~~ — **dissolved** (decision 4): scene already registers
   `SpinSystem` + `TransformPropagation` and exposes `advance`.
4. ~~headless demo: drive its tick through the ecs Schedule~~ — **dissolved**
   (decision 4): the schedule is consumed/proven by the App frontend (slice 4),
   not the demo. No second world to drive it through.

### Slice 2 — ergonomics — RELOCATED to the umbrella crate (Module Law #8)
Module Law #8 (one public facade per module) forbids a module from exporting
free-standing types like `Assets<T>`, `Mesh`, `Material`, or component bundles —
they would be a second top-level `pub`. `ResourcesApi`/`SceneApi` already expose
everything needed, so the modules get **no change**. The ergonomic *types* are
an umbrella-crate concern and move into slice 4:
- ~~5–6 `axiom-resources`: `Assets<T>` / `Mesh::cube` / `Material::lit`~~ →
  umbrella `Mesh`/`Material`/`Assets<T>` value types wrapping `ResourcesApi`.
- ~~7 `axiom-scene`: bundle spawn + component bundles~~ → umbrella `SceneCommands`
  + `Renderable`/`Camera`/`Spin`/`DirectionalLight` bundles wrapping `SceneApi`.
- ~~8 headless demo shrink~~ → folds into slice 5 (collapse onto `App`).

Consequence: the umbrella-tier law change (was commit 12) moves onto the
critical path early, since it gates both the prelude *and* every relocated
ergonomic type.

### Slice 3 — windowing engine module — 🟡 ~90%
9. **(law)** `xtask`: add windowing to the rule-9 browser/GPU-API allowlist
   (`PLATFORM_FACING_MODULES`). ✅ `ac79483`
10. `axiom-windowing`: engine module owning the presentation-request assembly +
    the live GPU binding promoted out of the browser app (the real `wgpu` arm,
    wasm32-only, behind the deterministic core; `cfg`-dispatch, not a trait);
    boundary documented in its `ARCHITECTURE.md`. ✅ `a881bcb`/`e35759a`/`abdd66c`
    — verified by browser screenshot (cubes render + spin).
11. `axiom-windowing`: run-loop driver (rAF on web, native fallback). 🟡 the
    deterministic loop **counters** (`step`/`next_tick`/`frames_driven`) + the
    present seam live in windowing; the **rAF driver itself still lives in the
    browser app** (`render_loop.rs`). It moves into windowing with `App::run`
    (slice 4 commit 14) — the umbrella supplies a per-frame closure producing
    plain draw data, windowing owns the loop.

### Slice 4 — `App` frontend — 🟡 ~80%
12. ~~**(law)** sanction an umbrella tier~~ — **dissolved** (decision 3): the
    `axiom` umbrella is an ordinary feature module.
13. `axiom` (the umbrella feature module): lifecycle + composition of scene +
    resources + render-pipeline. ✅ (pre-existing; windowing is added in 14)
14. fixed-step run loop + `.window` / `.fixed_timestep` config. 🟡 the
    **headless** core (`App::build` → `RunningApp::tick`) + `.window` /
    `.fixed_timestep_nanos` exist; **`App::run()` is not yet terminal** (it does
    not own the loop or compose windowing). This is the next step: `App::run`
    builds the world, then hands windowing a per-frame closure (plain draw data)
    and the canvas id; windowing drives the rAF loop (web) / a headless drive
    (native). Adds `axiom-windowing` to the umbrella's `allowed_modules`.
15. `DefaultPlugins`. ✅ (pre-existing)
16. `axiom` prelude. ✅ (pre-existing)

### Slice 5 — collapse apps, retire sketch — 🟢 ~85%
17. headless demo: rewrite onto `App`. **Deferred — deliberately.** The headless
    demo (`axiom-demo-rotating-cube`) is not just the slice; it is the
    **introspection / agent-interrogability** harness (`IntrospectApi`,
    `describe_frame`, `component_schemas`) + the per-boundary determinism
    inspector, neither of which `App` exposes today. Force-rewriting it onto
    `App` would *lose* that capability demo — a regression. Collapsing it cleanly
    needs introspection integrated into `App` first (a real feature, its own
    slice). Until then it keeps a distinct role.
18. browser demo: rewrite onto `App`. ✅ `44a2530` — now one `lib.rs` of scene
    description; deleted `browser_api`/`cube_slice`/`scene_content`/`render_loop`/
    `browser_bootstrap` (and earlier `live_gpu_binding`/`browser_surface_registry`).
    Depends only on the `axiom` umbrella. **Screenshot-verified.**
19. retire `rotating_cubes.rs`. ✅ — realized at shape & altitude by the browser
    app; sketch deleted, README marks the slice done.

## Invariants (do not violate)

- **One facade per module.** Ergonomics in slices 2 & 4 land *behind*
  `SceneApi`/`ResourcesApi`/the engine facade — never new top-level `pub use`.
- **100% coverage per layer/module commit.** Apps are outside the gate but still
  ship with their slice tests.
- **Determinism.** Schedule, windowing trait, and run loop carry the
  fixed-step/replay invariants; `Spin` stays tick-driven, never wall-clock.
- **`App` sequences, never accretes.** The day the engine module holds behavior
  instead of composing modules, it has become a junk drawer.
