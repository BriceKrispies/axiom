/*
 * The timer + state-machine projection (SPEC-07 §4.2). `Sim.time` is the author
 * surface over the native `TickApi`: one-shot (`after`) and repeating (`every`)
 * timers whose callbacks fire on the fixed tick they come due, plus
 * `createMachine` for a tick-driven finite state machine.
 *
 * The KEY mechanism is the per-tick callback-dispatch pump (`TickPump`). The
 * native core owns the schedule and reports the due timer ids each tick; this
 * projection only registers the author's closures with the loop-owned pump and
 * scopes each scheduling call to the running tick. `Sim.time` is therefore a thin
 * per-tick view — the durable callback registry lives in the pump the `GameLoop`
 * owns and drives once per fixed tick, so a timer registered in `onFixedUpdate`
 * fires deterministically (a timer set at tick T with delay D fires at T+D).
 */

import type { Handle, Ticks } from "./vocabulary.ts";
import type { StateMachine, StateNode } from "./state-machine.ts";
import type { TickPump } from "./pump.ts";

/** The timer + state-machine factory on `Sim.time` (SPEC-07 §4.2). */
export interface Time {
  /** Fire `callback` once, `delay` ticks from now; return the timer id. */
  readonly after: (delay: Ticks, callback: () => void) => Handle;
  /** Fire `callback` every `interval` ticks; return the timer id. */
  readonly every: (interval: Ticks, callback: () => void) => Handle;
  /** Cancel a timer so it never fires again (a stale id is a clean no-op). */
  readonly cancel: (id: Handle) => void;
  /** Create a tick-driven state machine from its ordered states and initial state. */
  readonly createMachine: <State extends string>(
    states: readonly StateNode<State>[],
    initial: State,
  ) => StateMachine<State>;
}

/** Build the `Time` projection bound to `pump` and the running `tick`. */
export const makeTime = (pump: TickPump, tick: Ticks): Time => ({
  after: (delay: Ticks, callback: () => void): Handle => pump.scheduleAfter(tick, delay, callback),
  cancel: (id: Handle): void => {
    pump.cancelTimer(id);
  },
  createMachine: <State extends string>(
    states: readonly StateNode<State>[],
    initial: State,
  ): StateMachine<State> => pump.createMachine(tick, states, initial),
  every: (interval: Ticks, callback: () => void): Handle =>
    pump.scheduleEvery(tick, interval, callback),
});
