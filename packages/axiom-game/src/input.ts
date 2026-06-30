/*
 * The input projection (SPEC-05 §4.2). `Input` is the author surface over one
 * tick's input snapshot: action edges/holds, the analog `axis`, the pointer, and
 * gestures. The native core owns the snapshot and the action-binding table; this
 * projection binds the running tick so author calls read action names only —
 * physical keys appear solely in the free `bindAction` (SPEC-05 §4.2).
 *
 * `Sim.input` is an `Input` over the snapshot for the running tick. `axis`
 * collapses two action holds into `-1 | 0 | 1` by a table index (no branch);
 * the optional reads (`pointer`/`pointerPressed`/`swipe`/`pressedAtTick`)
 * forward the bridge's `Result` directly — the `null` is produced native-side,
 * never written in this branchless spine.
 */

import type { NativeBridge, PointerSample, Swipe } from "./native-bridge.ts";
import type { Result, Ticks, Vec2 } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";
import { pick } from "./control-flow.ts";

/** An action name — the only input vocabulary gameplay reads (SPEC-05 §4.2). */
export type Action = string;

/** The three analog `axis` outcomes (SPEC-05 §4.2), the negative/zero/positive steps. */
const AXIS_NEGATIVE = -1;
const AXIS_ZERO = 0;
const AXIS_POSITIVE = 1;

/** The axis steps indexed by `Number(pos) - Number(neg) + AXIS_BIAS`. */
const AXIS_STEPS: readonly [-1, 0, 1] = [AXIS_NEGATIVE, AXIS_ZERO, AXIS_POSITIVE];

/** The offset that maps the `[-1, 0, 1]` difference onto the `[0, 1, 2]` index. */
const AXIS_BIAS = 1;

/** The input surface over one tick's snapshot (SPEC-05 §4.2). */
export interface Input {
  /** Whether `action` is held this tick. */
  readonly isDown: (action: Action) => boolean;
  /** Whether `action` went down this tick (edge). */
  readonly pressed: (action: Action) => boolean;
  /** Whether `action` went up this tick (edge). */
  readonly released: (action: Action) => boolean;
  /** `-1`, `0`, or `+1` from a negative/positive action pair. */
  readonly axis: (neg: Action, pos: Action) => -1 | 0 | 1;
  /** This tick's relative look (mouse / pointer-lock) as a raw-pixel `(dx, dy)` delta — `(0, 0)` when none. A game scales it by its own sensitivity and applies it to its yaw/pitch. */
  readonly look: () => Vec2;
  /** The pointer sample this tick, or the empty value when there is no pointer. */
  readonly pointer: () => Result<PointerSample>;
  /** The position a pointer-press began at this tick, or the empty value. */
  readonly pointerPressed: () => Result<Vec2>;
  /** The flick gesture committed this tick, or the empty value. */
  readonly swipe: () => Result<Swipe>;
  /** The tick `action` was most recently pressed at, or the empty value if never. */
  readonly pressedAtTick: (action: Action) => Result<Ticks>;
}

/** The `Input` projection bound to one tick's snapshot. */
export class SnapshotInput implements Input {
  readonly #bridge: NativeBridge;
  readonly #tick: Ticks;

  public constructor(bridge: NativeBridge, tick: Ticks) {
    this.#bridge = bridge;
    this.#tick = tick;
  }

  public isDown(action: Action): boolean {
    return this.#bridge.inputIsDown(this.#tick, action);
  }

  public pressed(action: Action): boolean {
    return this.#bridge.inputPressed(this.#tick, action);
  }

  public released(action: Action): boolean {
    return this.#bridge.inputReleased(this.#tick, action);
  }

  public axis(neg: Action, pos: Action): -1 | 0 | 1 {
    const difference = Number(this.isDown(pos)) - Number(this.isDown(neg));
    return pick(AXIS_STEPS, difference + AXIS_BIAS);
  }

  public look(): Vec2 {
    return this.#bridge.inputLookDelta(this.#tick);
  }

  public pointer(): Result<PointerSample> {
    return this.#bridge.inputPointer(this.#tick);
  }

  public pointerPressed(): Result<Vec2> {
    return this.#bridge.inputPointerPressed(this.#tick);
  }

  public swipe(): Result<Swipe> {
    return this.#bridge.inputSwipe(this.#tick);
  }

  public pressedAtTick(action: Action): Result<Ticks> {
    return this.#bridge.inputPressedAtTick(this.#tick, action);
  }
}

/** Build the `Input` projection over `bridge` for `tick` (SPEC-05 §4.2). */
export const makeInput = (bridge: NativeBridge, tick: Ticks): Input => new SnapshotInput(bridge, tick);

/** Bind an action name to the physical `keys` that trigger it (SPEC-05 §4.2). */
export const bindAction = (action: Action, keys: readonly string[]): void => {
  boundHost().bindAction(action, keys);
};
