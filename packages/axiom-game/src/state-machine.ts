/*
 * The tick-driven finite state machine (SPEC-07 §4.2). An author declares an
 * ordered list of states (each a `StateNode` carrying optional `onEnter`/
 * `onUpdate`/`onExit` closures) and an initial state; `Sim.time.createMachine`
 * mints one. The native `TickApi` owns the dense current-state index and the
 * entry tick (so `current`/`ticksInState` are deterministic native reads); the
 * closures stay in the TS layer and are re-bound on replay, exactly like timer
 * callbacks and handles (SPEC-00 §9).
 *
 * The states are an ORDERED `StateNode[]` rather than the spec's
 * `Record<State, StateDef>` so the name list is genuinely typed `State[]`
 * (`states.map(node => node.name)`), with no `Object.keys(...) as State[]`
 * downcast — the unsafe assertion the SDK's lint law forbids. Declaration order is
 * the dense index order the native core uses, so the mapping stays deterministic.
 *
 * `#runHandler` dispatches an optional lifecycle closure through `whenPresent`
 * (a `filter`+`map` over the 0-or-1 present singleton), so there is no `if
 * handler` branch — the Branchless Law holds across the whole machine.
 */

import type { Handle, Ticks } from "./vocabulary.ts";
import { pick, whenPresent } from "./control-flow.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** The author-facing state machine surface (SPEC-07 §4.2). */
export interface StateMachine<State extends string> {
  /** The current state name. */
  readonly current: State;
  /** How many ticks the machine has been in its current state. */
  readonly ticksInState: Ticks;
  /** Move to state `to`, firing the current state's `onExit` then `to`'s `onEnter`. */
  readonly transition: (to: State) => void;
}

/** One declared state: its name plus optional lifecycle closures (SPEC-07 §4.2). */
export interface StateNode<State extends string> {
  /** The state's name — the dense index is its position in the declared list. */
  readonly name: State;
  /** Run once when the machine enters this state (creation or transition). */
  readonly onEnter?: (machine: StateMachine<State>) => void;
  /** Run each tick while this state is active. */
  readonly onUpdate?: (machine: StateMachine<State>) => void;
  /** Run once when the machine leaves this state. */
  readonly onExit?: (machine: StateMachine<State>) => void;
}

/** The pump-facing capability of anything advanced once per fixed tick. */
export interface TickDriven {
  /** Advance to `tick`: the machine refreshes its tick and runs `onUpdate`. */
  readonly advance: (tick: Ticks) => void;
}

/** The construction inputs for a {@link BridgeStateMachine}. */
export interface MachineInit<State extends string> {
  readonly bridge: NativeBridge;
  readonly id: Handle;
  readonly nodes: readonly StateNode<State>[];
  readonly tick: Ticks;
}

/** Selects one optional lifecycle closure from a {@link StateNode}. */
type HandlerSelector<State extends string> = (
  node: StateNode<State>,
) => ((machine: StateMachine<State>) => void) | undefined;

/** The `StateMachine` projection bound to one native machine id and its nodes. */
export class BridgeStateMachine<State extends string>
  implements StateMachine<State>, TickDriven
{
  readonly #bridge: NativeBridge;
  readonly #id: Handle;
  readonly #names: readonly State[];
  readonly #nodes: ReadonlyMap<State, StateNode<State>>;
  #tick: Ticks;

  public constructor(init: MachineInit<State>) {
    this.#bridge = init.bridge;
    this.#id = init.id;
    this.#names = init.nodes.map((node): State => node.name);
    this.#nodes = new Map(
      init.nodes.map((node): readonly [State, StateNode<State>] => [node.name, node]),
    );
    this.#tick = init.tick;
  }

  public get current(): State {
    return pick(this.#names, this.#bridge.machineCurrent(this.#id));
  }

  public get ticksInState(): Ticks {
    return this.#bridge.machineTicksInState(this.#id, this.#tick);
  }

  public transition(to: State): void {
    this.#runHandler(this.current, (node): HandlerResult<State> => node.onExit);
    this.#bridge.machineTransition(this.#id, this.#tick, this.#names.indexOf(to));
    this.#runHandler(to, (node): HandlerResult<State> => node.onEnter);
  }

  public advance(tick: Ticks): void {
    this.#tick = tick;
    this.#runHandler(this.current, (node): HandlerResult<State> => node.onUpdate);
  }

  /** Fire the initial state's `onEnter` once, at creation. */
  public enterInitial(): void {
    this.#runHandler(this.current, (node): HandlerResult<State> => node.onEnter);
  }

  #runHandler(state: State, select: HandlerSelector<State>): void {
    whenPresent(this.#nodes.get(state), (node): void => {
      whenPresent(select(node), (handler): void => {
        handler(this);
      });
    });
  }
}

/** The result of a {@link HandlerSelector} — an optional lifecycle closure. */
type HandlerResult<State extends string> = ReturnType<HandlerSelector<State>>;
