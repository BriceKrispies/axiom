/*
 * The `Sim` handed to every fixed update and the `Frame` handed to every render
 * (SPEC-00 §4.2). `Sim` exposes no wall-clock accessor — elapsed simulated time
 * is `tick * dt`, constant per game. Its `rng`/`input`/`world` members are the
 * real subsystem projections (SPEC-01 rng, SPEC-05 input, SPEC-02 world) built
 * over the `NativeBridge`: `rng` is the game's root stream, `input` is bound to
 * the running tick's snapshot, and `world` is the retained ECS surface.
 */

import { type Input, makeInput } from "./input.ts";
import { type Rng, makeRng } from "./rng.ts";
import { type World, makeWorld } from "./world.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** The deterministic simulation view handed to a fixed update. */
export interface Sim {
  /** The monotonic fixed-tick index this update runs at. */
  readonly tick: number;
  /** The constant fixed timestep in seconds (`1 / fixedHz`). */
  readonly dt: number;
  /** The game's root deterministic RNG stream (SPEC-01). */
  readonly rng: Rng;
  /** Input over this tick's snapshot (SPEC-05). */
  readonly input: Input;
  /** The retained ECS world (SPEC-02). */
  readonly world: World;
}

/** The presentation view handed to a render — interpolated with `alpha`. */
export interface Frame {
  /** The latest completed fixed tick this frame presents. */
  readonly tick: number;
}

/** One second expressed in seconds — the numerator of `dt = 1 second / fixedHz`. */
const ONE_SECOND_IN_SECONDS = 1;

/** Build the deterministic `Sim` for `tick` at a `fixedHz` cadence over `bridge`. */
export const makeSim = (bridge: NativeBridge, fixedHz: number, tick: number): Sim => ({
  dt: ONE_SECOND_IN_SECONDS / fixedHz,
  input: makeInput(bridge, tick),
  rng: makeRng(bridge),
  tick,
  world: makeWorld(bridge),
});

/** Build the presentation `Frame` for the latest completed `tick`. */
export const makeFrame = (tick: number): Frame => ({ tick });
