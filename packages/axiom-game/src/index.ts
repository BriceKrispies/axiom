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
 * Wave 4 CORE projected the synchronous data facades over the `NativeBridge`
 * seam: `Sim.rng` (SPEC-01), `Sim.world` (SPEC-02), `Sim.input` (SPEC-05), math
 * (SPEC-03), and the host bridge (SPEC-12, emit-exactly-once).
 *
 * Wave 4 TAIL adds the remaining author-facing surfaces, all bridge-backed and
 * fully covered against a fake bridge (no wasm):
 *   - `Sim.time` (SPEC-07) — `after`/`every`/`cancel` timers + `createMachine`,
 *     driven by the per-tick callback-dispatch PUMP (`TickPump`) the `GameLoop`
 *     runs once per fixed tick: the native `TickApi` reports the due ids each tick
 *     and the pump dispatches the held author closures, so a timer set at tick T
 *     with delay D fires deterministically at T+D;
 *   - `Sim.tweens` (SPEC-09) — `add`/`cancel`, sampled per fixed tick by the same
 *     pump through the native `TweenApi`;
 *   - `Sim.add.*` (SPEC-14) — retained `GameObject` handles wrapping an ECS entity
 *     (`sprite`/`text`/`rectangle`/`image`) with bridge-backed mutators;
 *   - `Sim.physics` (SPEC-10) — `physics.add.*` bodies, `applyImpulse`/`applyForce`
 *     /`applyTorque`, velocity setters, and world `setConfig`;
 *   - audio (SPEC-08) — the `loadSound`/`playSound`/`playTone`/… free functions,
 *     presentation-side over the `HostBridge`.
 *
 * Deferred to a later wave (documented in the report) — the larger "tail" of the
 * surface: SPEC-06 grid/path, SPEC-11 3D (`createMesh`/`v3`/`mat4`/`quat`), and
 * SPEC-13 `NetSim`/`joinRoom` netcode. They are additive bridge projections that
 * land on the same seam; nothing here changes shape when they arrive. The pump
 * mechanism this wave introduces is what SPEC-06/11/13 (and a future Scene↔loop
 * binding) build their per-tick wiring on.
 */

export { createGame } from "./game.ts";
export type { Game, GameConfig, GameStatus } from "./game.ts";

export { GameLoop } from "./game-loop.ts";
export { onFixedUpdate, onRender, GameRegistry, defaultRegistry } from "./registry.ts";

export { stepFrame } from "./loop-core.ts";
export type { FixedUpdate, FrameStep, Render } from "./loop-core.ts";

export { makeFrame, makeSim } from "./sim.ts";
export type { Frame, Sim, SimContext } from "./sim.ts";

export { interpolationAlpha } from "./step-budget.ts";
export type { StepBudget } from "./step-budget.ts";

export type { BodyKind, NativeBridge, PointerSample, Swipe, TweenCurve } from "./native-bridge.ts";

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
export type {
  HostBridge,
  MusicOptions,
  Outcome,
  ScheduleOptions,
  SessionConfig,
  SoundOptions,
  ToneEnvelope,
  ToneLfo,
  ToneSpec,
} from "./host-binding.ts";

// Wave 4 TAIL — the pump-driven and retained-object surfaces.

export { makeTime } from "./time.ts";
export type { Time } from "./time.ts";

export { BridgeStateMachine } from "./state-machine.ts";
export type { MachineInit, StateMachine, StateNode, TickDriven } from "./state-machine.ts";

export { makeTweens, EASES } from "./tweens.ts";
export type { Ease, Tweens, TweenSpec } from "./tweens.ts";

export { TickPump } from "./pump.ts";

export { makeAdd, GameObject } from "./game-object.ts";
export type { Add, RectangleStyle } from "./game-object.ts";

export { makePhysics } from "./physics.ts";
export type { Body, Physics, PhysicsAdd, PhysicsConfig } from "./physics.ts";

export {
  loadSound,
  playMusic,
  playSound,
  playTone,
  scheduleSound,
  setMasterVolume,
  setMuted,
  stopVoice,
} from "./sound.ts";

export { orElse, whenPresent } from "./branchless.ts";

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
