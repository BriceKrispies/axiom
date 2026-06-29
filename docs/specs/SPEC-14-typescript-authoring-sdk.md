# SPEC-14 — TypeScript authoring SDK (`@axiom/game`)

> Status: Landed
> Landed (2026-06-28): `@axiom/game` ships the `Scene` shell + the seven factory namespaces, now wired to the real subsystem projections (`this.add`/`physics`/`input`/`tweens`/`sound`/`time`/`cameras`) backed by `apps/axiom-game-runtime` (`WasmGame`). Gate green: tsgo + Oxlint (every category an error + the branch ban) + 100% `node:test`. The §2 stubs are now filled.
> Contract: §1–§4   Vocabulary: Phaser-style `Scene`, `createGame`, factory namespaces, the retained-ECS game object   Determinism: boundary (projects through SPEC-00)

## 1. Summary

SPEC-00 defines the *frame model* — the variable-dt loop, the fixed-step tick,
and the wasm↔TS seam. **This spec defines the author's words**: the Phaser-style
`Scene` class and `createGame`/`onFixedUpdate`/`onRender` surface a game is
actually written in, and the mapping from each later subsystem spec (SPEC-01..13)
into a `Scene` factory namespace (`this.add`, `this.physics`, `this.input`,
`this.tweens`, `this.sound`, `this.time`, `this.cameras`).

It is the projection layer: SPEC-00 is the loop, SPEC-14 is the vocabulary that
rides on it. The two ship together as `@axiom/game`. An author who knows Phaser
should be able to write an Axiom game with a familiar shape, while the
deterministic native core (the SPEC-00 accumulator + `apps/axiom-game-runtime`)
runs underneath.

## 2. Current state (verified)

- **The loop landed (SPEC-00).** `@axiom/game` exposes `createGame`,
  `onFixedUpdate`, `onRender`, the pure branchless `stepFrame`/`GameLoop` core
  (driven by a `NativeBridge`, tested against a fake bridge), and the
  `Sim`/`Frame`/`StepBudget` contracts. The native boundary
  (`apps/axiom-game-runtime`, `WasmGame`) and the `axiom-frame` `FrameAccumulator`
  are real.
- **The `Scene` shell exists, the factories are stubs.** `Scene` (subclassable;
  `preload`/`create`/`update`) and its seven factory namespaces are present as
  typed, discriminated placeholders. None of them spawn/animate/route input yet —
  each is a named seam a later spec fills.
- **No subsystem projections yet.** `Sim.rng`/`Sim.input`/`Sim.world` and every
  `this.*` factory are M0 stubs (`{ subsystem: "…" }`). SPEC-01/02/03/05/… replace
  them with the real surface.

## 3. Architectural placement

This spec adds **no new package or app** — it lands inside the two pieces SPEC-00
created:

1. **`packages/axiom-game` (`@axiom/game`)** — the authoring vocabulary lives
   here, beside the loop. Held to the TS spine laws (tsgo, Oxlint every category
   an error + the branch ban, 100% `node:test` coverage), per
   `packages/axiom-client/STATIC_ANALYSIS.md`. The `Scene` class and factory
   types are pure, branchless, fully-covered core; the `requestAnimationFrame`
   driver is the single coverage-/branch-exempt platform edge (`raf-loop.ts`).
2. **`apps/axiom-game-runtime`** — as each subsystem spec lands, the runtime app
   grows the `#[wasm_bindgen]` marshalling that backs that subsystem's factory
   (e.g. an entity handle table behind `this.add`). It stays an **app**: a leaf
   composition + platform edge, the only legal home for cross-module wiring.

The retained-ECS rule is the spine of the model: **a game object is a handle
wrapping an entity**. `this.add.sprite(...)` (SPEC-02) returns a handle; the
handle is the noun every subsystem speaks. Handles are opaque and never
serialized into sim state — a replay re-binds them (SPEC-00 §5).

## 4. API surface

### 4.1 Native (`apps/axiom-game-runtime`, wasm-bindgen)

The boundary the SDK binds. `WasmGame` owns the runtime; subsystem specs extend
its exports (handle tables, per-subsystem marshalling). Not a reusable facade.

```rust
#[wasm_bindgen]
impl WasmGame {
    pub fn new(fixed_step_nanos: u64, max_steps: u32) -> WasmGame;
    pub fn advance(&mut self, elapsed_nanos: u64) -> StepReport; // { steps, remainder_nanos, fixed_step_nanos }
    pub fn current_tick(&self) -> u64;
    pub fn snapshot(&self) -> Vec<u8>;
}
```

### 4.2 TypeScript authoring projection

```ts
// The loop surface (SPEC-00 §4.2): createGame / onFixedUpdate / onRender / Sim / Frame.

// The Phaser-style scene shell (this spec). An author subclasses Scene, overrides
// the lifecycle hooks, and reaches the engine through the factory namespaces.
class Scene {
  // Factory namespaces — each is the projection target of one later spec:
  readonly add: AddFactory;        // SPEC-02 world  — this.add.*      -> spawn entity, return handle
  readonly physics: PhysicsFactory;// SPEC-03 physics— this.physics.add.* -> body on a handle
  readonly input: InputFactory;    // SPEC-05 input  — this.input.keyboard / pointer
  readonly tweens: TweensFactory;  // SPEC-08 anim   — this.tweens.add(...)
  readonly sound: SoundFactory;    // SPEC-10 audio  — this.sound.play(...)
  readonly time: TimeFactory;      // SPEC-00 frame  — this.time.delayedCall(...)
  readonly cameras: CamerasFactory;// SPEC-04 2D     — this.cameras.main

  preload(): readonly string[];                 // declare asset keys to load
  create(): readonly number[];                  // author initial entities -> handles
  update(tick: number, dt: number): readonly number[]; // advance -> mutated handles
}
```

The lifecycle hooks return the **handles they author** — the retained-ECS framing
made explicit. At M0 the defaults return empty lists; each subsystem spec wires
the corresponding factory to produce real handles.

### Subsystem → factory mapping

| Factory namespace      | Backing spec | Projection                              |
|------------------------|--------------|-----------------------------------------|
| `this.add.*`           | SPEC-02      | spawn an entity, return its handle      |
| `this.physics.add.*`   | SPEC-03      | attach a body/collider to a handle      |
| `this.input.keyboard`  | SPEC-05      | per-tick input state on `Sim.input`     |
| `this.tweens.add`      | SPEC-08      | interpolate a handle's components       |
| `this.sound.*`         | SPEC-10      | presentation-only audio (render side)   |
| `this.time.*`          | SPEC-00      | tick-scheduled callbacks                |
| `this.cameras.main`    | SPEC-04      | the 2D view/projection                  |

Each spec's §4.2 lands as the concrete factory replacing that namespace's M0
stub; nothing else in `@axiom/game` changes shape.

## 5. Data contracts

- **`StepBudget`** `{ steps, remainderNanos, fixedStepNanos }` — the only value
  crossing accumulator → loop (SPEC-00 §5). `steps` drives the sim;
  `remainderNanos / fixedStepNanos` (via `interpolationAlpha`) is the
  presentation-only fraction.
- **`NativeBridge`** `{ advance(elapsedNanos), snapshot() }` — the loop core's
  only contact with the wasm runtime. The pure core depends on the interface; the
  platform edge (`raf-loop.ts` `bridgeFromWasm`) adapts the real `WasmGame`.
- **Handle** — an opaque number; a game object is a handle wrapping an entity.
  Never serialized into sim state; a replay re-binds handles.
- **`Sim`** `{ tick, dt, rng, input, world }` — no wall-clock accessor; elapsed
  simulated time is `tick * dt`. `rng`/`input`/`world` are SPEC-01/05/02 stubs.

## 6. Determinism

- The fixed update is the **only** place sim state changes; it runs at constant
  `dt = 1 / fixedHz` and never sees real time (SPEC-00 §6).
- `stepFrame`/`GameLoop` are pure given a `StepBudget` and the registered
  callbacks: the same budget sequence yields the same callback sequence and the
  same tick total — proven headless with a fake bridge.
- `onRender` is presentation-only: it reads `alpha` and may read real time; it
  must not mutate the world.
- The `Scene` lifecycle hooks return data, do not read a clock, and (once wired)
  mutate only through the deterministic world projection (SPEC-02).
- Handles are opaque and non-serialized, so a replay reconstructs identical sim
  state regardless of handle identity.

## 7. Acceptance / proof

- **`@axiom/game` gate green:** tsgo typecheck, Oxlint (every category an error +
  the branch ban), and `node:test` 100% lines/branches/functions — all pass. The
  RAF driver (`raf-loop.ts`) is the sole branch-/coverage-exempt platform edge.
- **Headless determinism (landed):** a test game registers
  `onFixedUpdate`/`onRender`, runs N ticks through a `GameLoop` over a fake
  bridge, and asserts the tick count, the render count, and a per-tick state-hash
  sequence reproduce on a second run (`test/loop.test.ts`).
- **Native boundary (landed):** `apps/axiom-game-runtime` slice tests prove the
  same elapsed sequence drives the same total tick count regardless of chunking,
  two runtimes fed identical elapsed sequences produce byte-identical
  `snapshot_sim`, and a per-tick state-hash sequence reproduces on replay.
- **Per-subsystem (future):** as each factory is wired, a `Scene` subclass using
  `this.<factory>.*` renders/behaves deterministically and replays byte-equal.

## 8. Dependencies & order

Projects through SPEC-00 (the loop must exist first — it does). The `Scene`
factories fill in as their backing specs land: SPEC-02 (`this.add`) and SPEC-05
(`this.input`) and SPEC-01 (`Sim.rng`) are the first projections, then SPEC-03
(`this.physics`), SPEC-04 (`this.cameras`), SPEC-08 (`this.tweens`), SPEC-10
(`this.sound`). `@axiom/game` may depend on `@axiom/client` (SPEC-13) for netplay;
never the reverse.

## 9. Open questions

- **Global vs per-game registry.** `onFixedUpdate`/`onRender` target a
  module-global default registry (Phaser-shaped), reset on `createGame`. Two live
  games would share it. Default M0 simplification; revisit when a second
  concurrent game is real (likely: bind the registry to the `Game` instance).
- **Scene ↔ loop wiring.** At M0 `createGame` owns lifecycle + config and the
  platform edge owns the `GameLoop`; the `Scene` lifecycle is not yet called from
  the loop. SPEC-02 decides how a `Scene`'s `create`/`update` bind to the fixed
  update (likely: `create` once at start, `update` as the registered fixed update).
- **Handle table ownership.** Entity↔native and resource↔native tables live
  app-side in `axiom-game-runtime` until a second consumer proves the primitive
  (SPEC-00 §9).
- **`pause()` semantics.** Freeze the accumulator (no resume catch-up); banked
  wall-clock is a determinism foot-gun (SPEC-00 §9).
