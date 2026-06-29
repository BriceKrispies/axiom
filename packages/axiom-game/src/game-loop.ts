/*
 * `GameLoop`: binds a NativeBridge, a fixedHz, and a registry into the per-frame
 * driver. Each `advance(elapsedNanos)` asks the bridge for the integer step budget,
 * runs the registered fixed updates that many times and one render via the pure
 * `stepFrame`, and tracks the monotonic tick. This is the deterministic core the
 * platform edge drives from requestAnimationFrame and the tests drive with a fake
 * bridge — no wasm, no clock.
 *
 * The loop owns the single durable `TickPump` (the timer/tween/state-machine
 * callback registry) and drives it once per fixed tick by PREPENDING a pump
 * fixed-update ahead of the author's. Pump-first means a state machine's tick and a
 * timer registered during the author's update at tick T are scoped to T, and that
 * timer fires when the pump dispatches it at tick T+D. Every per-tick `Sim`
 * (`Sim.time`/`Sim.tweens`) registers into this same pump.
 *
 * The loop also DRIVES THE SCENE lifecycle (SPEC-14 §9). A `Scene` is attached with
 * `mount(scene)` (the loop starts with an inert base `Scene` whose default hooks
 * author nothing); the loop then runs the scene over its own `SimContext`
 * (`scene-runtime.ts`): `create` runs once before the first frame's fixed updates (a
 * one-shot drained on the first `advance`), and the scene's `update` runs as a
 * per-tick fixed update scheduled right after the pump and before the free
 * `onFixedUpdate` callbacks — so the scene authors first each tick, then the free
 * callbacks layer on.
 */

import { type FixedUpdate, stepFrame } from "./loop-core.ts";
import { type MountedScene, mountScene } from "./scene-runtime.ts";
import { type SimContext, makeFrame, makeSim } from "./sim.ts";
import type { GameRegistry } from "./registry.ts";
import type { NativeBridge } from "./native-bridge.ts";
import { Scene } from "./scene.ts";
import type { StepBudget } from "./step-budget.ts";
import { TickPump } from "./pump.ts";
import { each } from "./control-flow.ts";

const FIRST_TICK = 0;

/** Drives a NativeBridge's fixed steps through the registered callbacks and the mounted scene's lifecycle. */
export class GameLoop {
  #tick = FIRST_TICK;
  #mounted: MountedScene;
  readonly #bridge: NativeBridge;
  readonly #registry: GameRegistry;
  readonly #pump: TickPump;
  readonly #context: SimContext;
  // The scene `create` one-shot: drained on the first `advance` so `create` runs
  // Exactly once, before any fixed update — branchless (an `each` over a list that
  // Is then cleared), never an `if firstFrame`.
  readonly #pendingStart: (() => void)[] = [];

  public constructor(bridge: NativeBridge, fixedHz: number, registry: GameRegistry) {
    this.#bridge = bridge;
    this.#registry = registry;
    this.#pump = new TickPump(bridge, fixedHz);
    this.#context = { bridge, fixedHz, pump: this.#pump };
    this.#mounted = mountScene(new Scene(), this.#context);
    this.#armStart();
  }

  /** Drive `scene`'s lifecycle from this loop: `create` once at start, `update` each tick (SPEC-14 §9). */
  public mount(scene: Scene): this {
    this.#mounted = mountScene(scene, this.#context);
    this.#armStart();
    return this;
  }

  /** Bank `elapsedNanos`, run the scene `create` once + the resulting fixed updates + one render, return the budget. */
  public advance(elapsedNanos: number): StepBudget {
    const budget = this.#bridge.advance(elapsedNanos);
    // Run `create` once, before this frame's fixed updates, then drain the one-shot.
    each(this.#pendingStart, (run): void => {
      run();
    });
    this.#pendingStart.length = 0;
    const pumpUpdate: FixedUpdate = (sim): void => {
      this.#pump.pump(sim.tick);
    };
    this.#tick = stepFrame({
      budget,
      fixedUpdates: [pumpUpdate, this.#mounted.tick, ...this.#registry.fixedUpdates()],
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

  /** The asset keys the mounted scene's `preload` declared (empty until the first `advance`). */
  public assets(): readonly string[] {
    return this.#mounted.assets();
  }

  /** The durable simulation state as opaque bytes, from the native bridge. */
  public snapshot(): Uint8Array {
    return this.#bridge.snapshot();
  }

  /** Re-arm the scene `create` one-shot so the next `advance` runs the (re)mounted scene's `start`. */
  #armStart(): void {
    this.#pendingStart.length = 0;
    this.#pendingStart.push((): void => {
      this.#mounted.start();
    });
  }
}
