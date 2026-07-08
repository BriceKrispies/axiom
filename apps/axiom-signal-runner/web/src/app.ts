/*
 * The `defineApp` manifest — the bridge between the framework-free game core and the
 * live `@axiom/game` engine. Two stable-id systems run against the real SDK surfaces:
 *
 *   - `signal.step` (fixedUpdate) reads this tick's input snapshot (keyboard actions
 *     + pointer drag) into a pure `Intent` and advances the deterministic game one
 *     fixed tick;
 *   - `signal.draw` (render) paints the whole frame through the draw2d `Frame`.
 *
 * The game object lives at module scope so the two systems share it. This file is the
 * ONLY place the SDK's live `Sim`/`Frame`/input meet `SignalRunnerGame`; everything
 * gameplay- or pixel-shaped is delegated to the core + renderer. App tier — ordinary
 * control flow, outside the engine gates.
 */

import { type Sim, bindAction, defineApp, system } from "@axiom/game";
import type { Frame } from "@axiom/game";
import type { Intent } from "./types.ts";
import { SignalRunnerGame } from "./game.ts";
import { WIDTH } from "./constants.ts";
import { renderGame } from "./render.ts";

/** The gameplay seed (independent of the wasm RNG — the core has its own PRNG). */
const SEED = 20_260_708;

const game = new SignalRunnerGame(SEED);
let bound = false;

/** Install the action → key bindings once, after the host channel is live. */
const bindActions = (): void => {
  bindAction("steerLeft", ["KeyA", "ArrowLeft"]);
  bindAction("steerRight", ["KeyD", "ArrowRight"]);
  bindAction("brake", ["ShiftLeft", "ShiftRight"]);
  bindAction("boost", ["Digit1", "Space", "KeyW"]);
  bindAction("shield", ["Digit2"]);
  bindAction("pulse", ["Digit3"]);
  bindAction("drone", ["Digit4"]);
  bindAction("confirm", ["Enter"]);
  bound = true;
};

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v));

/** Fold this tick's input snapshot into a pure `Intent`. */
const readIntent = (sim: Sim): Intent => {
  const pointer = sim.input.pointer();
  const steerTo = pointer && pointer.down ? clamp((pointer.pos.x - WIDTH / 2) / (WIDTH / 2), -1, 1) : null;
  return {
    boost: sim.input.pressed("boost"),
    brake: sim.input.isDown("brake"),
    confirm: sim.input.pressed("confirm"),
    drone: sim.input.pressed("drone"),
    pulse: sim.input.pressed("pulse"),
    shield: sim.input.pressed("shield"),
    steer: sim.input.axis("steerLeft", "steerRight"),
    steerTo,
  };
};

const stepSystem = system("signal.step", {
  phase: "fixedUpdate",
  run: (sim: Sim): void => {
    if (!bound) {
      bindActions();
    }
    game.step(readIntent(sim));
  },
});

const drawSystem = system("signal.draw", {
  phase: "render",
  run: (frame: Frame): void => {
    renderGame(frame, game.state);
  },
});

export default defineApp({
  config: { fixedHz: 60, seed: 1n, surface: "c" },
  id: "signal-runner",
  systems: [stepSystem, drawSystem],
});
