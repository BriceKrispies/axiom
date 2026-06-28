/*
 * `createGame` and the `Game` lifecycle (SPEC-00 §4.2). `createGame` returns a
 * `Game` an author starts/pauses/resumes/stops. The lifecycle here is a pure
 * status state machine — the actual clock-driven loop is wired by the platform
 * edge (`raf-loop.ts`), which reads `game.status` to gate whether a frame runs.
 * Creating a game resets the default callback registry so each game starts with a
 * clean `onFixedUpdate`/`onRender` set.
 */

import { defaultRegistry } from "./registry.ts";

/** The presentation surface, fixed cadence, and seed an author configures. */
export interface GameConfig {
  /** Fixed simulation rate in Hz; `dt = 1 / fixedHz` seconds per tick. */
  readonly fixedHz: number;
  /** The deterministic seed (a 64-bit value as a bigint) for the sim RNG. */
  readonly seed: bigint;
  /** The host presentation target id (resolved by the host bridge, SPEC-12). */
  readonly surface: string;
}

/** The lifecycle state of a game. */
export type GameStatus = "idle" | "paused" | "running" | "stopped";

/** A created game: an author drives its lifecycle; the platform edge runs the loop. */
export interface Game {
  /** Begin running (the platform edge starts driving frames while running). */
  readonly start: () => void;
  /** Pause: freeze the accumulator (no catch-up on resume — see SPEC-14 §9). */
  readonly pause: () => void;
  /** Resume from paused. */
  readonly resume: () => void;
  /** Stop for good. */
  readonly stop: () => void;
  /** The current lifecycle state. */
  readonly status: GameStatus;
  /** The configuration this game was created with. */
  readonly config: GameConfig;
}

/** The concrete lifecycle state machine `createGame` returns. */
class GameImpl implements Game {
  #status: GameStatus = "idle";
  readonly #config: GameConfig;

  public constructor(config: GameConfig) {
    this.#config = config;
  }

  public start(): void {
    this.#status = "running";
  }

  public pause(): void {
    this.#status = "paused";
  }

  public resume(): void {
    this.#status = "running";
  }

  public stop(): void {
    this.#status = "stopped";
  }

  public get status(): GameStatus {
    return this.#status;
  }

  public get config(): GameConfig {
    return this.#config;
  }
}

/** Create a game from its config, resetting the default authoring registry. */
export const createGame = (config: GameConfig): Game => {
  defaultRegistry.reset();
  return new GameImpl(config);
};
