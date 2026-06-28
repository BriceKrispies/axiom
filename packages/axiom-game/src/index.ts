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
 */

export { createGame } from "./game.ts";
export type { Game, GameConfig, GameStatus } from "./game.ts";

export { GameLoop } from "./game-loop.ts";
export { onFixedUpdate, onRender, GameRegistry, defaultRegistry } from "./registry.ts";

export { stepFrame } from "./loop-core.ts";
export type { FixedUpdate, FrameStep, Render } from "./loop-core.ts";

export { makeFrame, makeSim } from "./sim.ts";
export type { Frame, InputStub, RngStub, Sim, WorldStub } from "./sim.ts";

export { interpolationAlpha } from "./step-budget.ts";
export type { StepBudget } from "./step-budget.ts";

export type { NativeBridge } from "./native-bridge.ts";

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
