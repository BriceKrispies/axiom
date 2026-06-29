/*
 * `GameLoop`: binds a NativeBridge, a fixedHz, and a registry into the per-frame
 * driver. Each `advance(elapsedNanos)` asks the bridge for the integer step
 * budget, runs the registered fixed updates that many times and one render via
 * the pure `stepFrame`, and tracks the monotonic tick. This is the deterministic
 * core the platform edge drives from requestAnimationFrame and the tests drive
 * with a fake bridge — no wasm, no clock.
 *
 * The loop owns the single durable `TickPump` (the timer/tween/state-machine
 * callback registry) and drives it once per fixed tick by PREPENDING a pump
 * fixed-update ahead of the author's. Pump-first means a state machine's tick and
 * a timer registered during the author's update at tick T are scoped to T, and
 * that timer fires when the pump dispatches it at tick T+D. Every per-tick `Sim`
 * (`Sim.time`/`Sim.tweens`) registers into this same pump, so callbacks scheduled
 * in `onFixedUpdate` fire/sample deterministically on the fixed tick.
 */

import { type FixedUpdate, stepFrame } from "./loop-core.ts";
import { type SimContext, makeFrame, makeSim } from "./sim.ts";
import type { GameRegistry } from "./registry.ts";
import type { NativeBridge } from "./native-bridge.ts";
import type { StepBudget } from "./step-budget.ts";
import { TickPump } from "./pump.ts";

const FIRST_TICK = 0;

/** Drives a NativeBridge's fixed steps through the registered callbacks. */
export class GameLoop {
  #tick = FIRST_TICK;
  readonly #bridge: NativeBridge;
  readonly #registry: GameRegistry;
  readonly #pump: TickPump;
  readonly #context: SimContext;

  public constructor(bridge: NativeBridge, fixedHz: number, registry: GameRegistry) {
    this.#bridge = bridge;
    this.#registry = registry;
    this.#pump = new TickPump(bridge, fixedHz);
    this.#context = { bridge, fixedHz, pump: this.#pump };
  }

  /** Bank `elapsedNanos`, run the resulting fixed updates + one render, return the budget. */
  public advance(elapsedNanos: number): StepBudget {
    const budget = this.#bridge.advance(elapsedNanos);
    const pumpUpdate: FixedUpdate = (sim): void => {
      this.#pump.pump(sim.tick);
    };
    this.#tick = stepFrame({
      budget,
      fixedUpdates: [pumpUpdate, ...this.#registry.fixedUpdates()],
      makeFrame,
      makeSim: (tick): ReturnType<typeof makeSim> => makeSim(this.#context, tick),
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
