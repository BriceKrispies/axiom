# Axiom Game-API Specs

This directory turns the two north-star documents —
[`../axiom-engine-vocabulary.md`](../axiom-engine-vocabulary.md) (the
capability inventory mapped from 11 real games) and
[`../game-api-contract.md`](../game-api-contract.md) (the stable authoring
surface) — into **implementable specifications**, one per subsystem.

The contract names the verbs and fixes their signatures. These specs add the
two things the contract deliberately omits: **exact architectural placement**
(which layer/module under the Layer Law and Module Law) and **a definition of
done** (the proof obligations a change must ship with). A future agent should be
able to open one spec and implement it without re-deriving where the code goes
or what "finished" means.

Each spec is the source of truth for its subsystem. When code and spec disagree,
that is a defect in one of them — fix it, don't let them drift.

## The two contracts (load-bearing determinism split)

Every spec declares a **determinism class**, and the class dictates placement and
proof obligations. This split is the same one the contract draws in §0.1; it is
not advisory.

- **`sim`** — deterministic and authoritative (contract §2–§9, §16). Runs on the
  fixed tick. The only clock is the tick counter; the only randomness is the
  seeded stream; the only input is the per-tick intent snapshot. Identical
  `(seed, config, input stream)` **must** produce byte-identical state every run
  and across machines (contract §17). Sim-class code lives in the engine spine
  (layers/modules), is branchless, and is 100% covered.
- **`presentation`** — client-side, non-authoritative (contract §10–§14). May read
  real time, interpolate, drop frames. **No value produced here may ever be read
  back into a `sim`-class API.** Presentation-class native code is still spine
  (branchless, covered); its *platform binding* (Web Audio, canvas, etc.) is the
  one exception — see "Platform arms" below.
- **`boundary`** — the host/transport seam (contract §15, parts of §16). Carries
  values in and out of the engine; obeys whichever side it touches.

A capability that needs to **affect gameplay** is `sim`. A capability that only
needs to be **seen or heard** is `presentation`. When unsure, it is `presentation`
— promoting it to `sim` later is cheap; demoting it after gameplay depends on it
is a determinism break.

## Architectural placement rules (recap of the Laws these specs obey)

- **Layers** (`crates/<name>/` + `layer.toml`) form a DAG; a layer imports only
  the layers it declares in `depends_on` and genuinely uses. New broadly-shared *primitives*
  go in the kernel, not a new ceremonial layer.
- **Engine modules** (`modules/<name>/` + `module.toml`, `allowed_modules = []`)
  are isolated capabilities exposing **one** facade. They never import another
  module.
- **Feature modules** (`kind = "feature-module"`) compose the modules they list.
- **Apps** (`apps/`) are the only place two module contracts are translated into
  each other. Glue lives here, never in a module.
- **Platform arms.** Browser/platform APIs are layer-`host`-only plus the
  `windowing` module allowlist (Module Law #9). Any spec whose presentation
  needs a Web API (audio output, DOM, localStorage) must split into a **neutral,
  deterministic, fully-covered core** plus a **platform binding** that is a
  deliberate, documented allowlist amendment compiled for `wasm32` — exactly how
  `axiom-windowing` wraps `wgpu`/`web-sys` behind its deterministic core. Adding
  an allowlist entry is an amendment in `crates/xtask/src/hygiene.rs`, never a
  default.
- **The authoring SDK is TypeScript.** Every contract entry is ultimately
  projected across the wasm boundary into the `@axiom/*` TS surface (see
  [`SPEC-00`](SPEC-00-authoring-boundary-and-frame-model.md)) and held to the TS
  spine's laws (`packages/axiom-client/STATIC_ANALYSIS.md`). A native facade with
  no TS projection is half-built.

## Spec template

Every spec file follows this shape. Keep it terse — house style is no-fluff.

```
# SPEC-NN — <Title>

> Status: Draft | In progress | Landed
> Contract: §X(.Y)   Vocabulary: <primitive names>   Determinism: sim | presentation | boundary

## 1. Summary           — the gap this closes; which games (of 11) demand it.
## 2. Current state      — what the tree has TODAY (crate + symbol), verified; what is missing.
## 3. Placement          — exact crate/module/layer (new or extended), allowed deps, why it is legal.
## 4. API surface        — 4.1 native Rust facade · 4.2 TS authoring projection (the contract signatures).
## 5. Data contracts      — neutral types that cross boundaries.
## 6. Determinism         — how it satisfies §17, or why it is presentation-excluded.
## 7. Acceptance / proof  — the tests that must ship: 100% coverage, branchless, replay/golden where sim.
## 8. Dependencies & order — what lands first; what depends on this.
## 9. Open questions
```

## The specs

Listed in the contract's dependency-respecting build order (contract §18). Status
is tracked here and is the single index of the program.

| Spec | Subsystem | Contract | Determinism | Placement (new \| extend) | Status |
|------|-----------|----------|-------------|---------------------------|--------|
| [00](SPEC-00-authoring-boundary-and-frame-model.md) | Authoring boundary & frame model | §0–§2 | boundary | **new** TS `@axiom/game` SDK + wasm boundary app | **Landed** |
| [01](SPEC-01-deterministic-randomness.md) | Deterministic randomness | §3, §17 | sim | extend `axiom-entropy` + projection | **Landed** |
| [02](SPEC-02-entities-components-queries.md) | Entities, components, queries, hierarchy | §4 | sim | extend `axiom-ecs` / `axiom-scene` + projection | **Landed** |
| [03](SPEC-03-math-and-spatial-queries.md) | Math & spatial queries | §5 | sim | extend `axiom-math` (scalar helpers) + projection | **Landed** |
| [04](SPEC-04-2d-surface.md) | 2D surface (shapes/text/sprites/particles) | §10 | presentation | **new** module `axiom-draw2d` + backend arms | **Landed** ¹ |
| [05](SPEC-05-input.md) | Input (keyboard, bindings, pointer, timing) | §8 | sim | extend `axiom-input` module | **Landed** |
| [06](SPEC-06-grid-pathfinding-tilespace.md) | Grid, pathfinding, tile space | §6–§7 | sim | **new** module `axiom-grid` | **Landed** |
| [07](SPEC-07-timers-and-state-machines.md) | Timers & state machines | §9 | sim | **new** module `axiom-tick` (+ kernel `TickSchedule`) | **Landed** |
| [08](SPEC-08-audio.md) | Audio (synthesis, playback, analysis) | §13 | presentation | **new** module `axiom-audio` + platform arm | **Landed** ² |
| [09](SPEC-09-ui-hud-and-tween.md) | UI/HUD overlay & tween/easing | §14, §12 | presentation | extend `axiom-interface` + **new** `axiom-tween` | **Landed** |
| [10](SPEC-10-physics-extensions.md) | Physics extensions (angular, friction) | impl §10 | sim | extend `axiom-physics` module | **Landed** ³ |
| [11](SPEC-11-3d-scene-surface.md) | 3D scene authoring surface | §11 | presentation | extend `axiom` / `axiom-render` / `axiom-scene` | **Landed** ⁴ |
| [12](SPEC-12-host-bridge-and-persistence.md) | Host bridge & persistence | §15 | boundary | extend `axiom-host` + platform arm + TS bridge | **Landed** |
| [13](SPEC-13-multiplayer-netcode-authoring.md) | Multiplayer & netcode authoring | §16 | sim | extend `axiom-net-protocol`/`-netcode`/`-client-core` + projection | **Landed** ⁵ |
| [14](SPEC-14-typescript-authoring-sdk.md) | TypeScript authoring SDK (`@axiom/game`) | §1–§4 | boundary | `@axiom/game` `Scene` + factories (in SPEC-00's pkg/app) | **Landed** |

Every spec's **native facade has landed on `main`**, and the sim spine (the
deterministic Rust cores) is real, branchless, and covered. The status column
distinguishes **Landed** (native facade + `@axiom/game` TypeScript projection +
the spec's §7 proofs all real) from **Partial** (native landed, but the TS
projection, the wasm boundary, or a promised proof is still missing). The wasm
runtime bridge and live browser presentation are browser-proven (the native
sandbox cannot run browser WebGPU / Web Audio).

A 2026-06-29 adversarial spec-vs-implementation audit catalogued the precise
gaps behind every then-**Partial** spec — see
[`../reports/SPEC_VS_IMPL_GAP_AUDIT.md`](../reports/SPEC_VS_IMPL_GAP_AUDIT.md)
(and its **2026-06-30 remediation note**). As of 2026-06-30 those gaps are
**closed**: SPEC-02 (full 12-method `World` + hierarchy/lifecycle proof),
SPEC-03 (`v2` namespace + pure predicates + `lerp` routed to native f32),
SPEC-05 (input carried across the wasm boundary + replay proofs), SPEC-09
(`Ui` overlay + `solveLayout` + button truth-table/presentation-leak proofs),
and SPEC-13 (the whole multiplayer TS authoring surface + cross-instance
determinism golden + authored netplay-server) are now **Landed**. SPEC-04 and
SPEC-11 are Landed with the residual deferrals noted below. Status notes:

- ¹ **SPEC-04** (Landed) — the neutral `Draw2dList` core, the full `@axiom/game`
  `Frame` 2D projection (shapes, sprite, particles, render targets, camera/
  transform, layer sort), and the §10.2 `sampleAnimation` flip-book sampler are
  all landed, plus the `measureText`/`loadFont`/`loadTexture` handle seams. The
  **software** backend rasterizes rect / circle / ellipse / line / particle /
  sprite (per-shape fill + stroke, src-over alpha); the **GPU** backend now
  rasterizes **rect + sprite (alpha, layer sort)** in its wgpu offscreen/`wasm32`
  platform arm, proven at parity with software on that subset (the §7
  both-backends alpha proof passes). Still deferred: GPU raster of
  circle/ellipse/line/particle (the software backend has them), and on **both**
  backends path / gradient / text glyph-run raster (recognised, not drawn).
- ² **SPEC-08** (Landed) — neutral core + Web Audio arm landed; in the wasm arm
  only `PlayTone` produces sound — `Load`/`PlaySample`/`PlayMusic`/`Stop` are
  currently no-ops. Live playback and the optional §13.1 analyser are
  browser-proven only.
- ³ **SPEC-10** (Landed) — `apply_torque` + friction + damping landed; determinism
  stays **same-binary only** (cross-platform f32 §17.6 unresolved), so SPEC-13
  must not predict physics. The TS `Sim.physics` adds bodies but **does not yet
  project colliders/materials/friction** (§4.2's "already projected on collider
  attach").
- ⁴ **SPEC-11** (Landed) — cylinder + emissive/roughness/opacity + hemisphere
  ambient landed; the §7 render-one-frame slice ("nova-roll") and the
  GPU↔canvas2d backend-parity proof (cylinder+emissive) are landed (driven via
  `axiom-shot`). Still deferred: **3D translucency blending** (`opacity` is
  carried but not yet blended — needs back-to-front ordering) and author-supplied
  `MeshData`.
- ⁵ **SPEC-13** (Landed) — the per-player Rust spine, the full TS authoring
  surface (`onSnapshot`/`onRestore`, the `Intent`-derived wire codec, the
  per-player `ClientIntentFor`/`ServerSnapshotFor` twin, `hostRoom`, `matchmake`),
  the cross-instance determinism golden, and the authored-callback netplay-server
  are all landed. Deferred by decision: **physics net-prediction is OFF**
  (authority/non-physics state only), and delta encoding, JWT verification, and
  unreliable transports are follow-ups.

## Cross-cutting law: determinism & replay (contract §17)

Binding on **every `sim`-class spec** (01, 02, 03, 05, 06, 07, 10, 13 and the
sim half of 00):

1. **Single clock** — the only time source is the fixed tick. No wall-clock or
   frame-delta reaches sim code.
2. **Single randomness source** — all sim randomness flows through the seeded
   `Rng` (SPEC-01).
3. **Input is a tick-indexed intent stream** — raw device events are sampled to a
   per-tick snapshot before the sim sees them (SPEC-05).
4. **Reproducibility** — identical `(seed, config, input stream)` ⇒ identical
   state and identical per-tick state-hash sequence on replay.
5. **Presentation is excluded** — rendering, audio, tween, particles (SPEC-04,
   08, 09, 11) may use real time; none of their outputs re-enters sim.
6. **Cross-instance determinism** — authoritative and predicted sims produce
   bit-identical state across machines (SPEC-13); requires deterministic
   arithmetic in sim code.

A spec that cannot meet its class's obligations is a design signal, not an
exception — reshape until it can (No-Shortcuts rule).
