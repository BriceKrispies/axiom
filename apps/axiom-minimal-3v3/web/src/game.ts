/*
 * game.ts — THE game, wired to the engine. Registering an `onFixedUpdate` as an
 * import side effect, it builds the scene on the first tick, folds this tick's
 * keyboard into a plain `Intent`, advances the deterministic SDK-free
 * `Mini3v3Session`, and mirrors the result into the 3D scene. It exports
 * `readHud()` for the harness's DOM overlay (possession / result / timing / score).
 *
 * Controls: WASD (or arrows) move · Q/E pass to the left/right teammate ·
 * hold SPACE to gather + jump, release near the apex to shoot · R to reset.
 */

import { type Sim, bindAction, onFixedUpdate } from "@axiom/game";
import { type SceneHandles, applyFrame, buildScene } from "./scene.ts";
import type { Intent, Phase, ResultKind, TimingTag } from "./types.ts";
import { Mini3v3Session } from "./session.ts";

/** The HUD snapshot the harness renders each frame. */
export interface Hud {
  readonly phase: Phase;
  readonly possession: string;
  readonly result: ResultKind | undefined;
  readonly timing: TimingTag | undefined;
  readonly makes: number;
  readonly attempts: number;
}

let handles: SceneHandles | undefined;
let session = new Mini3v3Session();

const bindKeys = (): void => {
  bindAction("left", ["KeyA", "ArrowLeft"]);
  bindAction("right", ["KeyD", "ArrowRight"]);
  bindAction("forward", ["KeyW", "ArrowUp"]);
  bindAction("back", ["KeyS", "ArrowDown"]);
  bindAction("gather", ["Space"]);
  bindAction("passLeft", ["KeyQ"]);
  bindAction("passRight", ["KeyE"]);
  bindAction("reset", ["KeyR"]);
};

/**
 * Fold this tick's keyboard into the session `Intent`. The camera looks downcourt
 * (+z), so world +X renders to screen-LEFT — the x axis is negated so pressing D
 * moves the player right ON SCREEN. W moves toward the hoop (+z).
 */
const readIntent = (sim: Sim): Intent => ({
  gatherHeld: sim.input.isDown("gather"),
  gatherPressed: sim.input.pressed("gather"),
  gatherReleased: sim.input.released("gather"),
  moveX: -sim.input.axis("left", "right"),
  moveZ: sim.input.axis("back", "forward"),
  passLeft: sim.input.pressed("passLeft"),
  passRight: sim.input.pressed("passRight"),
  reset: sim.input.pressed("reset"),
});

onFixedUpdate((sim: Sim): void => {
  if (handles === undefined) {
    bindKeys();
    handles = buildScene();
    session = new Mini3v3Session();
  }
  session.advance(readIntent(sim));
  applyFrame(handles, session.view());
});

/** The HUD the harness reads each frame. */
export const readHud = (): Hud => ({
  attempts: session.attempts,
  makes: session.makes,
  phase: session.phase,
  possession: session.possessionLabel,
  result: session.resultKind,
  timing: session.timingTag,
});
