/*
 * `SignalRunnerGame` — the top-level, framework-free game object. It owns the live
 * `State`, advances it one deterministic tick per `step(intent)`, exposes the pure
 * `hud()` model for the renderer, and folds win/lose confirmation into a clean
 * restart. It imports nothing from `@axiom/game`, so the whole game is constructible
 * and replayable in a bare Node test; the app manifest (app.ts) is the only place
 * the SDK's live `Sim`/`Frame` meet this object.
 */

import { type Hud, buildHud } from "./hud.ts";
import type { Intent, State } from "./types.ts";
import { generateLevel } from "./level.ts";
import { initialState } from "./state.ts";
import { stepSim } from "./sim.ts";

/** A cheap, bounded deterministic digest of the whole observable state. */
export const hashState = (state: State): number => {
  const fields = [
    state.tick,
    Math.round(state.runner.dist * 100),
    Math.round(state.runner.lateral * 100),
    Math.round(state.runner.speed * 100),
    Math.round(state.runner.latVel * 100),
    Math.round(state.runner.charge * 1000),
    state.runner.crashes,
    state.shardsCollected,
    state.platesActivated,
    Math.round(state.storm.dist * 100),
    state.phase === "win" ? 2 : state.phase === "lose" ? 1 : 0,
  ];
  return fields.reduce((h, f) => (h * 1_000_003 + (f | 0)) % 2_147_483_647, 2_166_136_261);
};

/** The playable game: a seed in, deterministic ticks out. */
export class SignalRunnerGame {
  #state: State;
  readonly #seed: number;

  public constructor(seed: number) {
    this.#seed = seed;
    this.#state = initialState(generateLevel(seed));
  }

  /** The live state (read-only view for the renderer). */
  public get state(): State {
    return this.#state;
  }

  /** Advance one fixed tick. On a finished run, `confirm` restarts from tick 0. */
  public step(intent: Intent): void {
    if (this.#state.phase === "run") {
      stepSim(this.#state, intent);
      return;
    }
    if (intent.confirm) {
      this.restart();
    }
  }

  /** Reset to the exact opening state for this seed (provably == a fresh boot). */
  public restart(): void {
    this.#state = initialState(generateLevel(this.#seed));
  }

  /** The pure UI model for the current frame. */
  public hud(): Hud {
    return buildHud(this.#state);
  }

  /** The deterministic state digest (replay/equality tests). */
  public hash(): number {
    return hashState(this.#state);
  }
}
