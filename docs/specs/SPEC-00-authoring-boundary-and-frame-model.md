# SPEC-00 — Authoring boundary & frame model

> Status: Draft
> Contract: §0–§2   Vocabulary: Variable-dt loop, Fixed-step tick, Game-flow state machine, the embed seam   Determinism: boundary

## 1. Summary

This is the keystone. It defines **how an author writes a whole game in
TypeScript and the deterministic native core runs it** — the loop, the callback
registration, the `Sim` handed to fixed updates, and the wasm↔TS boundary every
other spec projects through. Without it, every native facade in SPEC-01..13 has
no author-facing surface (today: 0 of the ~79 contract entry points exist in TS;
`packages/axiom-client` is a netcode client only).

All 11 games need a loop and an outcome seam. None of them should write it.

## 2. Current state (verified)

- **Native fixed-step core exists.** `axiom-runtime` (`RuntimeStep`, validated
  `FixedStep` from `fixed_step_nanos`) advances exactly one deterministic step;
  `axiom-frame` owns the per-frame envelope (`FrameContext`, `FrameCommandQueue`,
  `frame_timing`). This is the real engine loop.
- **No author loop, no `createGame`, no `Sim`, no `onFixedUpdate`/`onRender`.**
  The loop is driven today only by app `main`s and `axiom-windowing`'s live
  binding. There is no registration API and no presentation/sim callback split.
- **No TS authoring package.** `packages/axiom-client` exports `AxiomClient`,
  transports, and the wire codec — nothing game-authoring.
- **Variable-dt accumulator: missing.** The core is strict fixed-step; the
  contract's "0..N fixed updates then one render with `alpha`" accumulator is not
  implemented anywhere.

## 3. Architectural placement

Two new pieces plus one extension:

1. **TS authoring SDK — new package `packages/axiom-game` (`@axiom/game`).**
   The stable boundary. Exposes `createGame`, `onFixedUpdate`, `onRender`, the
   `Sim`/`Frame` interfaces, and re-exports every subsystem projection
   (SPEC-01..13). Held to the TS spine laws (tsgo, Oxlint, branch ban, 100%
   coverage) per `packages/axiom-client/STATIC_ANALYSIS.md`. It is **not** folded
   into `axiom-client`: that package is the netcode transport; this is the
   authoring surface. `@axiom/game` may depend on `@axiom/client` (SPEC-13), not
   the reverse.

2. **Native wasm boundary — new app `apps/axiom-game-runtime`.** The
   wasm-bindgen export layer that drives the fixed-step accumulator, owns the
   handle tables (entities, resources), and marshals each subsystem facade to JS.
   This is an **app**, not a module: it composes many modules and owns platform
   bootstrap (`requestAnimationFrame`, the wasm glue) — exactly the leaf
   composition + platform-edge role apps exist for, and the only legal home for
   cross-module wiring (Module Law). It is outside the coverage gate (apps are)
   but ships with slice tests.

3. **Accumulator — extend `axiom-frame`.** Add a deterministic fixed-step
   **accumulator** that, given a real elapsed presentation interval, yields the
   integer number of fixed steps to run and the residual `alpha` fraction. This
   is pure arithmetic over the existing `FrameTiming` — a spine primitive, the
   lowest correct layer for "how many ticks fit in this frame" (No-Shortcuts: the
   accumulator is not invented in the app). `sim`-class, branchless, fully
   covered.

The split is the determinism boundary made physical: the **accumulator decides
*how many* deterministic steps** (pure, in `axiom-frame`); the **app drives the
clock and the render** (impure, presentation); the **TS SDK is the author's
words**. No wall-clock value crosses from the app into a fixed update.

## 4. API surface

### 4.1 Native

`axiom-frame` (**landed**, sim-class):

```rust
// Pure accumulator: fold real elapsed time into whole fixed steps + a banked
// sub-step remainder. Reads no clock; elapsed time enters as explicit data.
impl FrameAccumulator {
    pub fn new(fixed_step_nanos: u64) -> FrameResult<FrameAccumulator>; // rejects a zero step
    pub fn advance(&mut self, elapsed_nanos: u64, max_steps: u32) -> StepBudget;
    pub const fn fixed_step_nanos(&self) -> u64;
    pub const fn banked_nanos(&self) -> u64;   // remainder + any clamped-away whole steps
}

// StepBudget is INTEGER-ONLY: { steps: u32, remainder_nanos: u64, fixed_step_nanos: u64 }.
```

**Refinement vs the original sketch (implemented 2026-06-28).** The accumulator
does *not* carry `alpha: Ratio`. The kernel `Ratio` constructor is fallible
(rejects non-finite), so an `alpha: Ratio` field would force either an
unreachable error arm (a dead region the Coverage Law forbids) or leaking a
`KernelResult` out of layer 4 — and a public `alpha: f32` would trip the
`engine_no_unitless_float_public_api` lint. The structurally honest fix keeps the
spine **integer-pure** (matching `FrameTiming`'s "explicit nanoseconds, no
floats" rule): `advance` returns `steps` + `remainder_nanos`, and the
presentation boundary computes the `0..1` fraction `remainder_nanos /
fixed_step_nanos` itself, where float math is unconstrained (§17.5). The
construction takes raw `fixed_step_nanos: u64` (as `FrameTiming` does), not a
kernel `FixedStep`, validating non-zero once so `advance` divides without a
guard.

`apps/axiom-game-runtime` (wasm-bindgen, boundary): owns the JS-facing
`Game`/registration object, the RAF loop, and the per-subsystem marshalling. Not
a reusable facade — its surface is the `#[wasm_bindgen]` exports the TS SDK binds.

### 4.2 TS authoring projection (the contract, §1–§2)

```ts
interface GameConfig { fixedHz: number; seed: bigint; surface: string }
interface Game { start(): void; pause(): void; resume(): void; stop(): void }
function createGame(config: GameConfig): Game;

function onFixedUpdate(cb: (sim: Sim) => void): void;   // 0..N per frame, constant dt, deterministic
function onRender(cb: (frame: Frame, alpha: number) => void): void;  // once per frame, presentation only

interface Sim {
  readonly tick: Ticks; readonly dt: Seconds;
  readonly rng: Rng;        // SPEC-01
  readonly input: Input;    // SPEC-05
  readonly world: World;    // SPEC-02
}
```

`Sim` exposes **no wall-clock accessor**; elapsed simulated time is `tick * dt`.

## 5. Data contracts

- **Core value types** (contract §0.2): `Entity`, `Ticks`, `Seconds`, `Handle`
  (all opaque numbers), `Vec2`/`Vec3`/`Rect`/`Rgba`, `Result<T> = T | null`.
  Handles and entities are **opaque and never serialized into sim state** — a
  replay re-binds them.
- **`StepBudget`** `{ steps, remainder_nanos, fixed_step_nanos }` — the only
  thing crossing accumulator → loop. `steps` drives the sim; `remainder_nanos /
  fixed_step_nanos` is the presentation-only interpolation fraction.

## 6. Determinism

- The fixed update is the **only** place sim state changes, runs at constant
  `dt = 1/fixedHz`, and never sees real time (§17.1).
- The accumulator is pure integer arithmetic; given the same elapsed-time
  sequence it yields the same step counts (and the same total regardless of how
  the elapsed time was chunked across frames — the invariant a replay relies on).
- `onRender` is presentation-excluded (§17.5): it may read `alpha` and real time
  and must not mutate the world.
- `null` is a normal outcome everywhere; the boundary does not throw for ordinary
  control flow (§0.2).

## 7. Acceptance / proof

- `axiom-frame` accumulator: 100% covered, branchless. Property test: for any
  partition of a total elapsed interval into frames, `Σ steps` is identical
  (chunk-invariance), and `steps ≤ max_steps` (spiral-of-death clamp) with the
  clamped remainder carried, not dropped.
- `@axiom/game`: tsgo + Oxlint (branch ban) + 100% TS coverage. A test game
  registers `onFixedUpdate`/`onRender`, runs N ticks headless, and asserts the
  tick count and a per-tick state-hash sequence reproduce on a second run.
- Slice test in `apps/axiom-game-runtime`: a trivial authored game (spawn one
  entity, advance it by `rng`) renders one frame and reports an outcome
  (SPEC-12/§15), proving the whole boundary end-to-end.

## 8. Dependencies & order

**Lands first; everything else projects through it.** Strictly needs only the
accumulator extension and the wasm boundary; the `Sim` sub-interfaces (`rng`,
`input`, `world`) can be stubbed and filled in by SPEC-01/02/05 as they land.
Build order after this: SPEC-01 → 02 → 03, then the 2D surface (SPEC-04), then
the rest (contract §18).

## 9. Open questions

- **Handle table ownership.** Do handle tables (entity↔native, resource↔native)
  live in the runtime app, or in a thin sim-class allocator a future native test
  app can also reuse? Lean app-side until a second consumer proves the primitive.
- **`surface` binding.** `GameConfig.surface` is a host concept — its resolution
  is owned by SPEC-12 (host bridge), not here.
- **Pause semantics.** Does `pause()` freeze the accumulator (no catch-up on
  resume) or bank elapsed time? Default: freeze — banked wall-clock is a
  determinism foot-gun.
