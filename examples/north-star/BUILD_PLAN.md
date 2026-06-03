# Build Plan — reaching the north-star `App` API

This is the roadmap from today's spine to
[`rotating_cubes.rs`](rotating_cubes.rs): promote proven app-glue, one slice at
a time, into the correct layer/module until the demo apps are pure scene
description — then delete the sketch (the [README](README.md) "deletion test").

Every code commit ships **with the tests that take it to 100% coverage** and
keeps `cargo xtask check-architecture` green. There is no "tests later."

## Settled architectural decisions

These three were open; they are now decided and are load-bearing for the
commits below.

1. **The Schedule lives on `axiom-ecs` (Layer 05).** `World::advance` already
   runs registered `WorldSystem`s once per engine frame; the Schedule is just
   *labelled phases* (`Startup`/`Update`) ordering that existing primitive. No
   new crate.
2. **Windowing is a feature module that composes `webgpu`** —
   `modules/axiom-windowing`, `kind = "feature-module"`,
   `allowed_modules = ["webgpu"]`, with a rule-9 platform-API allowlist
   exception. Because it may consume `axiom-webgpu`'s `GpuSubmission`, live and
   deterministic presentation become **one path**, not the raw-wgpu bypass the
   browser app uses today.
   - A platform **layer** was considered and rejected: the spine is strictly
     linear, each layer must adapt the one directly beneath it, and the
     browser-free engine-data layers (frame/ecs/introspect) may never use a
     platform layer — so no linear position exists for it. Layers also may not
     import modules, which would permanently bar it from `GpuSubmission`. Rule #9
     already anticipates an allowlist extension, not a new layer.
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

### Slice 3 — windowing feature module
9. **(law)** `xtask`: add windowing to the rule-9 browser/GPU-API allowlist.
10. `axiom-windowing`: feature module composing `webgpu`; owns the live GPU
    binding + surface lifecycle promoted out of the browser app. Native stub +
    wasm impl behind one portable trait; boundary documented in its
    `ARCHITECTURE.md`.
11. `axiom-windowing`: run-loop driver (rAF on web, native fallback).

### Slice 4 — `App` frontend
12. **(law)** `xtask`: sanction the `axiom` umbrella/prelude tier.
13. `axiom-engine`: feature module (lifecycle + plugin registry), composing
    scene + resources + render-pipeline + windowing. Composition only.
14. `axiom-engine`: fixed-step run loop + `.window` / `.fixed_timestep` config.
15. `axiom-engine`: `DefaultPlugins`.
16. `axiom`: the prelude umbrella crate.

### Slice 5 — collapse apps, delete sketch
17. headless demo: rewrite onto `App`/`DefaultPlugins`.
18. browser demo: rewrite onto `App`; delete `browser_*` + `live_gpu_binding`.
19. delete `rotating_cubes.rs` once it compiles verbatim against `axiom::prelude`.

## Invariants (do not violate)

- **One facade per module.** Ergonomics in slices 2 & 4 land *behind*
  `SceneApi`/`ResourcesApi`/the engine facade — never new top-level `pub use`.
- **100% coverage per layer/module commit.** Apps are outside the gate but still
  ship with their slice tests.
- **Determinism.** Schedule, windowing trait, and run loop carry the
  fixed-step/replay invariants; `Spin` stays tick-driven, never wall-clock.
- **`App` sequences, never accretes.** The day the engine module holds behavior
  instead of composing modules, it has become a junk drawer.
