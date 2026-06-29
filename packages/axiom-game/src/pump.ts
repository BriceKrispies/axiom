/*
 * The per-tick callback-dispatch pump — the KEY new mechanism Wave-4-TAIL adds.
 * The deterministic native core (`TickApi`/`TweenApi`) owns every schedule: which
 * timer ids come due on a tick, which tweens to sample and their eased value, and
 * each state machine's current index + entry tick. The pump holds the AUTHOR
 * closures those native ids map to, and once per fixed tick — driven by the
 * `GameLoop` through the loop's fixed-step path — it asks the bridge what is due
 * and dispatches the matching closures.
 *
 * Dispatch is branchless: each phase is a `each` (map) over the bridge-returned
 * id list, and a single optional callback is invoked through `whenPresent` (a
 * `filter`+`map` over the 0-or-1 present singleton) — never an `if id has a
 * callback`. Because the native core decides what is due and the pump only
 * forwards, the firing order is a pure function of the tick, so a timer/tween
 * fires identically on replay.
 *
 * The pump is owned by the `GameLoop` (one durable registry across ticks) and
 * handed to each per-tick `Sim` so `Sim.time`/`Sim.tweens` register into the same
 * registry the loop drives.
 */

import { BridgeStateMachine, type StateMachine, type StateNode, type TickDriven } from "./state-machine.ts";
import { EASES, type TweenSpec } from "./tweens.ts";
import type { Handle, Ticks } from "./vocabulary.ts";
import { each, orElse, whenPresent } from "./branchless.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** A no-op completion sink — the default when a tween declares no `onComplete`. */
const NO_COMPLETE = (): void => {
  // Nothing to do when a tween completes without an author sink.
};

/** The held closures for one active tween. */
interface TweenSinks {
  readonly onUpdate: (value: number) => void;
  readonly onComplete: () => void;
}

/** The dense index of `initial` among `states` (declaration order). */
const initialIndex = <State extends string>(
  states: readonly StateNode<State>[],
  initial: State,
): number => states.map((node): State => node.name).indexOf(initial);

/** The loop-owned registry of author timer/tween/machine closures, pumped per tick. */
export class TickPump {
  readonly #bridge: NativeBridge;
  readonly #fixedHz: number;
  readonly #timers = new Map<Handle, () => void>();
  readonly #tweens = new Map<Handle, TweenSinks>();
  readonly #machines: TickDriven[] = [];

  public constructor(bridge: NativeBridge, fixedHz: number) {
    this.#bridge = bridge;
    this.#fixedHz = fixedHz;
  }

  /** Schedule a one-shot timer due `delay` ticks after `tick`; hold its callback. */
  public scheduleAfter(tick: Ticks, delay: Ticks, callback: () => void): Handle {
    const id = this.#bridge.timerAfter(tick, delay);
    this.#timers.set(id, callback);
    return id;
  }

  /** Schedule a repeating timer firing every `interval` ticks; hold its callback. */
  public scheduleEvery(tick: Ticks, interval: Ticks, callback: () => void): Handle {
    const id = this.#bridge.timerEvery(tick, interval);
    this.#timers.set(id, callback);
    return id;
  }

  /** Cancel a timer in both the native schedule and the held registry. */
  public cancelTimer(id: Handle): void {
    this.#bridge.timerCancel(id);
    this.#timers.delete(id);
  }

  /** Register a tween (seconds → whole ticks against `fixedHz`); hold its sinks. */
  public addTween(tick: Ticks, spec: TweenSpec): Handle {
    const durationTicks = Math.round(spec.duration * this.#fixedHz);
    const easeIndex = EASES.indexOf(orElse(spec.ease, "linear"));
    const id = this.#bridge.tweenAdd(tick, {
      durationTicks,
      easeIndex,
      from: spec.from,
      to: spec.to,
    });
    this.#tweens.set(id, { onComplete: orElse(spec.onComplete, NO_COMPLETE), onUpdate: spec.onUpdate });
    return id;
  }

  /** Cancel a tween in both the native schedule and the held registry. */
  public cancelTween(id: Handle): void {
    this.#bridge.tweenCancel(id);
    this.#tweens.delete(id);
  }

  /** Mint a tick-driven state machine, fire its initial `onEnter`, and register it. */
  public createMachine<State extends string>(
    tick: Ticks,
    states: readonly StateNode<State>[],
    initial: State,
  ): StateMachine<State> {
    const id = this.#bridge.machineCreate(tick, states.length, initialIndex(states, initial));
    const machine = new BridgeStateMachine<State>({ bridge: this.#bridge, id, nodes: states, tick });
    this.#machines.push(machine);
    machine.enterInitial();
    return machine;
  }

  /** Dispatch everything due on `tick`: timers, tween samples/completions, machines. */
  public pump(tick: Ticks): void {
    each(this.#bridge.timersDue(tick), (id: Handle): void => {
      this.#fireTimer(id);
    });
    each(this.#bridge.tweenActive(tick), (id: Handle): void => {
      this.#sampleTween(id, tick);
    });
    each(this.#bridge.tweenCompleted(tick), (id: Handle): void => {
      this.#completeTween(id);
    });
    each(this.#machines, (machine: TickDriven): void => {
      machine.advance(tick);
    });
  }

  #fireTimer(id: Handle): void {
    whenPresent(this.#timers.get(id), (callback): void => {
      callback();
    });
  }

  #sampleTween(id: Handle, tick: Ticks): void {
    const value = this.#bridge.tweenValue(id, tick);
    whenPresent(this.#tweens.get(id), (sinks): void => {
      sinks.onUpdate(value);
    });
  }

  #completeTween(id: Handle): void {
    whenPresent(this.#tweens.get(id), (sinks): void => {
      sinks.onComplete();
    });
    this.#tweens.delete(id);
  }
}
