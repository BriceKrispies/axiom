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
3. **The prelude is a sanctioned umbrella crate.** A single `axiom` frontend
   crate re-exports the curated barrel (`axiom::prelude::*`). Module Law #8
   (one facade per module) stays intact; the umbrella is the one named exception.

## The commit series

`(law)` edits the mechanically enforced rules in `crates/xtask/src/`;
`(doc)` is prose only.

### Slice 1 — schedule backbone
1. **(doc)** add the north-star sketch, README, and this build plan.
2. `axiom-ecs`: add labelled `Schedule` phases (`Startup`/`Update`) over
   `WorldSystem`; adapter-over-frame relationship preserved.
3. `axiom-scene`: expose spin + transform propagation as registerable systems,
   behind `SceneApi`.
4. headless demo: drive its tick through the ecs Schedule (first app shrink).

### Slice 2 — module facade ergonomics (no new crates)
5. `axiom-resources`: handle-based `Assets<T>` (`add`/`get`) behind `ResourcesApi`.
6. `axiom-resources`: `Mesh::cube` / `Material::lit` constructors.
7. `axiom-scene`: bundle spawn (`spawn`/`with_child`) + `Renderable`/`Camera`/
   `DirectionalLight`/`Spin`/`Transform` bundles, behind `SceneApi`.
8. headless demo: rewrite onto `Assets` + bundle spawn (second shrink).
   **← lowest-risk checkpoint: payoff to both apps, zero new crates, zero law changes.**

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
