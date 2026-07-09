/*
 * game.ts — THE game, wired to the engine. Registering an `onFixedUpdate` as an
 * import side effect (the soccer pattern), it builds the scene on the first tick,
 * reads pointer + keyboard into a plain `Intent`, advances the deterministic
 * SDK-free `SwipeBasketballSession`, and mirrors the result into the 3D scene. It
 * exports `readHud()` for the harness's DOM overlay and `configureViewport()` so
 * the harness can report the real canvas size for pointer projection.
 *
 * Controls: drag a ball, swipe up, release to shoot · R to reset.
 */

import { type Sim, bindAction, onFixedUpdate } from "@axiom/game";
import { type SceneHandles, applyFrame, buildScene } from "./scene.ts";
import { type Intent, SwipeBasketballSession } from "./session.ts";
import { type HudModel, readHudModel } from "./hud.ts";
import { type Vec2, vec2 } from "./vec.ts";
import { DEFAULT_VIEWPORT } from "./constants.ts";

let handles: SceneHandles | undefined;
let session = new SwipeBasketballSession();
let prevDown = false;
let viewport: Vec2 = vec2(DEFAULT_VIEWPORT.x, DEFAULT_VIEWPORT.y);

const bindKeys = (): void => {
  bindAction("reset", ["KeyR"]);
};

/** Fold this tick's pointer + keyboard into the session `Intent`. */
const readIntent = (sim: Sim): Intent => {
  const sample = sim.input.pointer();
  const down = sample !== undefined ? sample.down : false;
  const pressed = down && !prevDown;
  const released = !down && prevDown;
  prevDown = down;
  const pointer = sample !== undefined ? vec2(sample.pos.x, sample.pos.y) : null;
  return { pointer, pressed, released, reset: sim.input.pressed("reset"), viewport };
};

onFixedUpdate((sim: Sim): void => {
  if (handles === undefined) {
    bindKeys();
    handles = buildScene();
    session = new SwipeBasketballSession();
  }
  session.advance(readIntent(sim));
  applyFrame(handles, session);
});

/** The HUD the harness reads each frame to update the DOM overlay. */
export const readHud = (): HudModel => readHudModel(session);

/** Report the real canvas backing size (px) for pointer projection. */
export const configureViewport = (width: number, height: number): void => {
  viewport = vec2(width, height);
};
