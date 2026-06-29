/*
 * The `Sim` handed to every fixed update and the `Frame` handed to every render
 * (SPEC-00 §4.2). `Sim` exposes no wall-clock accessor — elapsed simulated time
 * is `tick * dt`, constant per game. Its members are the real subsystem
 * projections built over the `NativeBridge` / the loop's `TickPump`:
 *   - `rng` (SPEC-01) is the game's root stream;
 *   - `input` (SPEC-05) is bound to the running tick's snapshot;
 *   - `world` (SPEC-02) is the retained ECS surface;
 *   - `add` (SPEC-14) spawns retained game objects;
 *   - `physics` (SPEC-10) attaches bodies and configures the world;
 *   - `time` (SPEC-07) schedules tick-driven timers + state machines;
 *   - `tweens` (SPEC-09) registers tick-sampled tweens.
 *
 * `time`/`tweens` register into the loop-owned `TickPump` so the callbacks they
 * schedule fire/sample on the fixed tick the loop pumps. The durable per-game
 * inputs — the bridge, the fixed rate, and that pump — are grouped into a
 * `SimContext` so `makeSim(context, tick)` separates "what is constant for the
 * game" from "which tick is running".
 */

import { type Add, makeAdd } from "./game-object.ts";
import { type Input, makeInput } from "./input.ts";
import { type Physics, makePhysics } from "./physics.ts";
import { type Rng, makeRng } from "./rng.ts";
import { type Time, makeTime } from "./time.ts";
import { type Tweens, makeTweens } from "./tweens.ts";
import { type World, makeWorld } from "./world.ts";
import type { NativeBridge } from "./native-bridge.ts";
import type { TickPump } from "./pump.ts";

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
  /** The retained game-object factory (SPEC-14). */
  readonly add: Add;
  /** Physics bodies + world config (SPEC-10). */
  readonly physics: Physics;
  /** Tick-driven timers + state machines (SPEC-07). */
  readonly time: Time;
  /** Tick-sampled tweens (SPEC-09). */
  readonly tweens: Tweens;
}

/** The presentation view handed to a render — interpolated with `alpha`. */
export interface Frame {
  /** The latest completed fixed tick this frame presents. */
  readonly tick: number;
}

/** One second expressed in seconds — the numerator of `dt = 1 second / fixedHz`. */
const ONE_SECOND_IN_SECONDS = 1;

/** The durable per-game inputs every per-tick `Sim` is built from. */
export interface SimContext {
  /** The native fixed-step runtime (RNG / ECS / input / bodies). */
  readonly bridge: NativeBridge;
  /** The fixed simulation rate, so `dt = 1 / fixedHz`. */
  readonly fixedHz: number;
  /** The loop-owned per-tick pump backing `time` / `tweens`. */
  readonly pump: TickPump;
}

/** Build the deterministic `Sim` for `tick` from the game's `context`. */
export const makeSim = (context: SimContext, tick: number): Sim => ({
  add: makeAdd(context.bridge),
  dt: ONE_SECOND_IN_SECONDS / context.fixedHz,
  input: makeInput(context.bridge, tick),
  physics: makePhysics(context.bridge),
  rng: makeRng(context.bridge),
  tick,
  time: makeTime(context.pump, tick),
  tweens: makeTweens(context.pump, tick),
  world: makeWorld(context.bridge),
});

/** Build the presentation `Frame` for the latest completed `tick`. */
export const makeFrame = (tick: number): Frame => ({ tick });
