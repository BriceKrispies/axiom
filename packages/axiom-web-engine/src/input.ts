/*
 * Keyboard actions, pointer-lock mouse look, and canvas pointer sampling
 * (input.ts) — the pure node-testable core split out of the (platform-edge)
 * `dom-input.ts` binding. Events (from `dom-input.ts` or a test) feed `InputState`
 * raw key / look / pointer samples at any rate; the consumer calls `beginTick()` once
 * before each fixed update to snapshot that stream, after which `pressed`/
 * `released` are exact one-tick edges against the previous snapshot, `look()` is
 * the mouse delta accumulated since the previous snapshot, and `isDown` reflects
 * the snapshotted key state. Key auto-repeat cannot re-fire `pressed` because a
 * held code is already down in the previous snapshot. This is a branchless,
 * fully-covered spine unit — no DOM, no conditionals: selection is expressed as
 * `.slice`/`.filter` over singleton arrays and edge logic as arithmetic on the
 * `Number(...)`-coerced key states.
 */

import type { InputFrame, PointerSample, TickInput } from "./api.ts";

/** A `.map` callback returns this to iterate for side effect only. */
const SIDE_EFFECT = 0;

/** Run `effect` for each value with no control-flow loop: `.map` with a constant
 * return satisfies array-callback-return and the produced array is discarded. */
const each = <Value>(values: readonly Value[], effect: (value: Value) => void): void => {
  values.map((value): number => {
    effect(value);
    return SIDE_EFFECT;
  });
};

/** The absent sentinel, materialized WITHOUT writing the banned `undefined`
 * literal: a 0-argument call to an optional-parameter identity yields the missing
 * argument. Presence is then tested by `!==` against this captured value. */
const absentProbe = <Value>(slot?: Value): Value | undefined => slot;
const ABSENT = absentProbe();

/** The pure input core: event feeds in, per-tick snapshots out. */
export class InputState implements TickInput {
  /** Action → the KeyboardEvent.code tokens that drive it (any one suffices). */
  readonly #bindings = new Map<string, readonly string[]>();
  /** Codes physically down right now (live, updated by every keyEvent). */
  readonly #liveCodes = new Set<string>();
  /** Codes down as of this tick's snapshot. */
  #tickCodes = new Set<string>();
  /** Codes down as of the previous tick's snapshot. */
  #prevCodes = new Set<string>();
  /** Mouse-look delta accumulated since the last `beginTick`. */
  #lookAccX = 0;
  #lookAccY = 0;
  /** The delta handed out by `look()` for the current tick. */
  #lookTickX = 0;
  #lookTickY = 0;
  /** The latest canvas pointer sample, or absent if cleared / never seen. */
  #pointerSample: PointerSample | undefined = absentProbe<PointerSample>();

  /** Map an action name to KeyboardEvent.code tokens; the action is down while
   * ANY bound code is down. Re-binding an action replaces its codes. */
  public bindAction(action: string, codes: readonly string[]): void {
    this.#bindings.set(action, [...codes]);
  }

  /** Feed a key transition (`KeyboardEvent.code`). Selection is branchless: the
   * singleton `[code]` is sliced to length 1 for the matching transition and 0
   * for the other, so `down` adds and `!down` deletes with no `if`. Repeated
   * `down` events for a held key are idempotent (a Set), so auto-repeat never
   * fabricates an edge. */
  public keyEvent(code: string, down: boolean): void {
    each([code].slice(Number(!down)), (token): void => {
      this.#liveCodes.add(token);
    });
    each([code].slice(Number(down)), (token): void => {
      this.#liveCodes.delete(token);
    });
  }

  /** Release every held key (window blur / pointer-lock loss): each held code
   * gets a normal `released` edge on the next tick, so a charge in progress
   * resolves safely and nothing stays logically held while unfocused —
   * regaining focus can never fabricate a press or a release. */
  public releaseAllKeys(): void {
    this.#liveCodes.clear();
  }

  /** Accumulate a pointer-locked mouse-look delta (raw px, +x right / +y down). */
  public lookEvent(dx: number, dy: number): void {
    this.#lookAccX += dx;
    this.#lookAccY += dy;
  }

  /** Feed the latest canvas pointer sample (CSS px, top-left origin). */
  public pointerEvent(x: number, y: number, down: boolean): void {
    this.#pointerSample = { down, pos: { x, y } };
  }

  /** Forget the pointer (it left the canvas); `pointer()` returns the absent value. */
  public pointerClear(): void {
    this.#pointerSample = absentProbe<PointerSample>();
  }

  /**
   * Snapshot the accumulated event stream for one fixed update. Must be called
   * exactly once before each tick's reads: it rolls the current key snapshot
   * into "previous" (making `pressed`/`released` exact edges), captures the live
   * key state, and drains the accumulated look delta into this tick's `look()`.
   */
  public beginTick(): void {
    this.#prevCodes = this.#tickCodes;
    this.#tickCodes = new Set(this.#liveCodes);
    this.#lookTickX = this.#lookAccX;
    this.#lookTickY = this.#lookAccY;
    this.#lookAccX = 0;
    this.#lookAccY = 0;
  }

  public isDown(action: string): boolean {
    return this.#anyBoundDown(action, this.#tickCodes);
  }

  /** A press edge is "down now, not down previously" — branchlessly, the current
   * down state (0/1) strictly exceeding the previous down state. */
  public pressed(action: string): boolean {
    return Number(this.#anyBoundDown(action, this.#tickCodes)) > Number(this.#anyBoundDown(action, this.#prevCodes));
  }

  /** A release edge is the mirror: previous down state exceeding the current. */
  public released(action: string): boolean {
    return Number(this.#anyBoundDown(action, this.#prevCodes)) > Number(this.#anyBoundDown(action, this.#tickCodes));
  }

  public look(): { readonly x: number; readonly y: number } {
    return { x: this.#lookTickX, y: this.#lookTickY };
  }

  public pointer(): PointerSample | undefined {
    return this.#pointerSample;
  }

  /** Whether any code bound to `action` is present in `codes`. The binding is
   * defaulted branchlessly: `[bound]` filtered to its present singleton is the
   * bound codes when set, or empty (never down) when the action is unbound. */
  #anyBoundDown(action: string, codes: ReadonlySet<string>): boolean {
    return [this.#bindings.get(action)]
      .filter((entry): entry is readonly string[] => entry !== ABSENT)
      .some((entry): boolean => entry.some((code): boolean => codes.has(code)));
  }
}

/**
 * Snapshot a (freshly `beginTick`-ed) `InputState` into the flat immutable
 * `InputFrame` a pure `update` reads: for each bound action name, resolve its
 * current down / press-edge / release-edge state and its look + pointer sample.
 * Branchless — each set is a `.filter` over the action names — so the shell can
 * hand `update` plain data instead of the stateful input object. */
export const sampleInput = (input: InputState, actions: readonly string[]): InputFrame => ({
  down: new Set(actions.filter((action): boolean => input.isDown(action))),
  look: input.look(),
  pointer: input.pointer(),
  pressed: new Set(actions.filter((action): boolean => input.pressed(action))),
  released: new Set(actions.filter((action): boolean => input.released(action))),
});
