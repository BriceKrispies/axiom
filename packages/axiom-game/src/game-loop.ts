/*
 * `GameLoop`: binds a NativeBridge, a fixedHz, and a registry into the per-frame
 * driver. Each `advance(elapsedNanos)` asks the bridge for the integer step
 * budget, runs the registered fixed updates that many times and one render via
 * the pure `stepFrame`, and tracks the monotonic tick. This is the deterministic
 * core the platform edge drives from requestAnimationFrame and the tests drive
 * with a fake bridge — no wasm, no clock.
 */

import { makeFrame, makeSim } from "./sim.ts";
import type { GameRegistry } from "./registry.ts";
import type { NativeBridge } from "./native-bridge.ts";
import type { StepBudget } from "./step-budget.ts";
import { stepFrame } from "./loop-core.ts";

const FIRST_TICK = 0;

/** Drives a NativeBridge's fixed steps through the registered callbacks. */
export class GameLoop {
  #tick = FIRST_TICK;
  readonly #bridge: NativeBridge;
  readonly #fixedHz: number;
  readonly #registry: GameRegistry;

  public constructor(bridge: NativeBridge, fixedHz: number, registry: GameRegistry) {
    this.#bridge = bridge;
    this.#fixedHz = fixedHz;
    this.#registry = registry;
  }

  /** Bank `elapsedNanos`, run the resulting fixed updates + one render, return the budget. */
  public advance(elapsedNanos: number): StepBudget {
    const budget = this.#bridge.advance(elapsedNanos);
    this.#tick = stepFrame({
      budget,
      fixedUpdates: this.#registry.fixedUpdates(),
      makeFrame,
      makeSim: (tick): ReturnType<typeof makeSim> => makeSim(this.#bridge, this.#fixedHz, tick),
      renders: this.#registry.renders(),
      startTick: this.#tick,
    });
    return budget;
  }

  /** The monotonic count of fixed ticks driven so far. */
  public get tick(): number {
    return this.#tick;
  }

  /** The durable simulation state as opaque bytes, from the native bridge. */
  public snapshot(): Uint8Array {
    return this.#bridge.snapshot();
  }
}
