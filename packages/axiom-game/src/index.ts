/*
 * Public entry point for the `@axiom/game` Phaser-style authoring SDK.
 *
 * Exposes the authoring surface (SPEC-00 §4.2 / SPEC-14): `createGame`, the free
 * `onFixedUpdate`/`onRender` registration functions, the deterministic loop core
 * (`GameLoop`, `stepFrame`) the platform edge drives, the `Sim`/`Frame`/`Scene`
 * authoring types, and the `NativeBridge`/`StepBudget` boundary contracts the
 * wasm runtime (`apps/axiom-game-runtime`) projects through. The browser RAF
 * driver lives in `raf-loop.ts` (the platform edge) and is bound by the host, not
 * re-exported here.
 *
 * Wave 4 projects the landed native facades over the `NativeBridge` seam:
 *   - `Sim.rng` (SPEC-01) — the deterministic RNG (`next`/`int`/`range`/`bool`/
 *     `pick`/`weighted`/`shuffle`/`stream`);
 *   - `Sim.world` (SPEC-02) — entities/components/queries/hierarchy;
 *   - `Sim.input` (SPEC-05) — the per-tick input snapshot, plus free `bindAction`;
 *   - math (SPEC-03) — `clamp`/`lerp`/`normalizeAngle`/`overlapCircle`;
 *   - the host bridge (SPEC-12) — `getSessionConfig`/`notifyReady`/`reportOutcome`/
 *     `reportOutcomes` (emit-exactly-once), installed by the runtime app via
 *     `bindNative`.
 *
 * Deferred to a later wave (documented in the report): `this.time` (SPEC-07
 * timers + state machines) and `this.tweens` (SPEC-09 tween/ease). Both require a
 * native -> TS callback-dispatch pump (a timer firing, a tween sampling) wired
 * into the per-tick Scene update flow — a distinct seam from this wave's
 * synchronous data projections — so they remain typed factory stubs on `Scene`
 * (`this.time`/`this.tweens`) until that pump lands.
 */

export { createGame } from "./game.ts";
export type { Game, GameConfig, GameStatus } from "./game.ts";

export { GameLoop } from "./game-loop.ts";
export { onFixedUpdate, onRender, GameRegistry, defaultRegistry } from "./registry.ts";

export { stepFrame } from "./loop-core.ts";
export type { FixedUpdate, FrameStep, Render } from "./loop-core.ts";

export { makeFrame, makeSim } from "./sim.ts";
export type { Frame, Sim } from "./sim.ts";

export { interpolationAlpha } from "./step-budget.ts";
export type { StepBudget } from "./step-budget.ts";

export type { NativeBridge, PointerSample, Swipe } from "./native-bridge.ts";

export { Scene } from "./scene.ts";
export type {
  AddFactory,
  CamerasFactory,
  InputFactory,
  PhysicsFactory,
  SoundFactory,
  TimeFactory,
  TweensFactory,
} from "./scene.ts";

// Wave 4 — the projected subsystem surfaces.

export { makeRng, StreamRng, ROOT_STREAM } from "./rng.ts";
export type { Rng } from "./rng.ts";

export { makeWorld, BridgeWorld } from "./world.ts";
export type { World } from "./world.ts";

export { makeInput, SnapshotInput, bindAction } from "./input.ts";
export type { Action, Input } from "./input.ts";

export { clamp, lerp, normalizeAngle, overlapCircle } from "./math.ts";

export { getSessionConfig, notifyReady, reportOutcome, reportOutcomes } from "./host.ts";

export { bindNative } from "./host-binding.ts";
export type { HostBridge, Outcome, SessionConfig } from "./host-binding.ts";

export type {
  Component,
  ComponentKind,
  Entity,
  Handle,
  PlayerId,
  Result,
  Seconds,
  Ticks,
  Vec2,
  Vec3,
} from "./vocabulary.ts";
