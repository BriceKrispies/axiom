/*
 * game.ts — THE game, wired to the engine. Registering an `onFixedUpdate` as an
 * import side effect, it builds the scene on the first tick, folds this tick's pointer
 * + keyboard into a plain `Intent`, advances the deterministic SDK-free
 * `HeatCheckSession`, and mirrors the result into the 3D scene. It exports `readHud()`
 * for the harness's DOM overlay (score / time / streak / multiplier / heat + floating
 * feedback) and `configureViewport()` for pointer projection.
 *
 * Controls: drag left/right to create space, release to shoot · A/D or ←/→ steer,
 * SPACE to shoot · R (or tap on game-over) to run the 60-second round back.
 */

import { type Sim, bindAction, onFixedUpdate } from "@axiom/game";
import { type SceneHandles, applyFrame, buildScene } from "./scene.ts";
import type { Feedback, Intent, Phase, Readiness } from "./types.ts";
import { HeatCheckSession } from "./session.ts";

/** The HUD snapshot the harness renders each frame. */
export interface Hud {
  readonly phase: Phase;
  readonly score: number;
  readonly best: number;
  /** Seconds left in the round. */
  readonly time: number;
  /** Consecutive makes. */
  readonly streak: number;
  /** Current streak multiplier (1…4). */
  readonly multiplier: number;
  /** Current heat level (0…5). */
  readonly heat: number;
  /** True in the final-seconds double-points window. */
  readonly finalWindow: boolean;
  /** True briefly after a made basket (score pop). */
  readonly scorePop: boolean;
  /** The live pre-release readiness tags (SPACE / RHYTHM / BALANCE), while holding. */
  readonly readiness: Readiness | undefined;
  /** Feedback events to float as text this frame. */
  readonly events: readonly Feedback[];
}

let handles: SceneHandles | undefined;
let session = new HeatCheckSession();

// The on-screen joystick (rendered by the harness BELOW the game) pushes its state
// here whenever it changes; the fixed-update loop reads the latest value each tick.
let padStickX = 0;
let padHolding = false;
let prevPadHolding = false;

const bindKeys = (): void => {
  bindAction("left", ["ArrowLeft", "KeyA"]);
  bindAction("right", ["ArrowRight", "KeyD"]);
  bindAction("shoot", ["Space"]);
  bindAction("reset", ["KeyR"]);
};

/** The harness's joystick feeds its state here (screen-sign stickX in [-1, 1]). */
export const setStick = (x: number, holding: boolean): void => {
  padStickX = x;
  padHolding = holding;
};

/**
 * Fold this tick's joystick + keyboard into the session `Intent`. The joystick gives a
 * movement INTENT (`stickX`, never an absolute position); lifting it is the release
 * edge that shoots. The camera looks downcourt so world +X renders to screen-LEFT, so
 * we negate: pushing the stick / pressing right moves the player right ON SCREEN.
 */
const readIntent = (sim: Sim): Intent => {
  const kbAxis = sim.input.axis("left", "right");
  const screenStick = padHolding ? padStickX : kbAxis;
  const holding = padHolding || kbAxis !== 0;
  // The shoot edge comes from the joystick release only (keyboard shoots with Space).
  const released = !padHolding && prevPadHolding;
  prevPadHolding = padHolding;
  const shoot = sim.input.pressed("shoot");
  const reset = sim.input.pressed("reset");
  return { holding, released, reset, shoot, stickX: -screenStick };
};

onFixedUpdate((sim: Sim): void => {
  if (handles === undefined) {
    bindKeys();
    handles = buildScene();
    session = new HeatCheckSession();
  }
  session.advance(readIntent(sim));
  applyFrame(handles, session.view());
});

/** The HUD the harness reads each frame (draining feedback events). */
export const readHud = (): Hud => ({
  best: session.best,
  events: session.drainEvents(),
  finalWindow: session.finalWindow,
  heat: session.heat,
  multiplier: session.multiplier,
  phase: session.phase,
  readiness: session.readiness(),
  score: session.score,
  scorePop: session.scorePop,
  streak: session.streak,
  time: session.timeRemaining,
});
