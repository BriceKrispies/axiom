/*
 * engine/input.ts — keyboard actions, pointer-lock mouse look, and canvas
 * pointer sampling, split (like `loop.ts`) into a pure node-testable core and a
 * thin DOM edge. `InputState` is the core: DOM events (or tests) feed it raw
 * key / look / pointer samples at any rate; the game calls `beginTick()` once
 * before each fixed update to snapshot that stream, after which
 * `pressed`/`released` are exact one-tick edges against the previous snapshot,
 * `look()` is the mouse delta accumulated since the previous snapshot, and
 * `isDown` reflects the snapshotted key state. Key auto-repeat cannot re-fire
 * `pressed` because a held code is already down in the previous snapshot.
 * `attachDomInput` is the browser edge — the ONLY place in this file that
 * touches the DOM — wiring window/canvas listeners into an `InputState` and
 * returning a detach function.
 */

import type { TickInput } from "./api.ts";

/** The pure input core: event feeds in, per-tick snapshots out. */
export class InputState implements TickInput {
  /** action → the KeyboardEvent.code tokens that drive it (any one suffices). */
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
  /** The latest canvas pointer sample, or undefined if cleared / never seen. */
  #pointerSample: { readonly pos: { readonly x: number; readonly y: number }; readonly down: boolean } | undefined =
    undefined;

  /** Map an action name to KeyboardEvent.code tokens; the action is down while
   * ANY bound code is down. Re-binding an action replaces its codes. */
  public bindAction(action: string, codes: readonly string[]): void {
    this.#bindings.set(action, [...codes]);
  }

  /** Feed a key transition (`KeyboardEvent.code`). Repeated `down` events for a
   * held key are idempotent (a Set), so auto-repeat never fabricates an edge. */
  public keyEvent(code: string, down: boolean): void {
    if (down) {
      this.#liveCodes.add(code);
    } else {
      this.#liveCodes.delete(code);
    }
  }

  /** Accumulate a pointer-locked mouse-look delta (raw px, +x right / +y down). */
  public lookEvent(dx: number, dy: number): void {
    this.#lookAccX += dx;
    this.#lookAccY += dy;
  }

  /** Feed the latest canvas pointer sample (CSS px, top-left origin). */
  public pointerEvent(x: number, y: number, down: boolean): void {
    this.#pointerSample = { pos: { x, y }, down };
  }

  /** Forget the pointer (it left the canvas); `pointer()` returns undefined. */
  public pointerClear(): void {
    this.#pointerSample = undefined;
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

  public pressed(action: string): boolean {
    return this.#anyBoundDown(action, this.#tickCodes) && !this.#anyBoundDown(action, this.#prevCodes);
  }

  public released(action: string): boolean {
    return !this.#anyBoundDown(action, this.#tickCodes) && this.#anyBoundDown(action, this.#prevCodes);
  }

  public look(): { readonly x: number; readonly y: number } {
    return { x: this.#lookTickX, y: this.#lookTickY };
  }

  public pointer(): { readonly pos: { readonly x: number; readonly y: number }; readonly down: boolean } | undefined {
    return this.#pointerSample;
  }

  #anyBoundDown(action: string, codes: ReadonlySet<string>): boolean {
    const bound = this.#bindings.get(action);
    return bound !== undefined && bound.some((code) => codes.has(code));
  }
}

/**
 * The browser edge: wire window/canvas listeners into `input`. Keyboard events
 * feed `keyEvent`; clicking the canvas requests pointer lock (rejection caught —
 * the browser may deny it); pointer-locked mouse movement feeds `lookEvent`;
 * canvas pointer events feed `pointerEvent`/`pointerClear`. Returns a detach
 * function that removes every listener it added.
 */
export function attachDomInput(input: InputState, canvas: HTMLCanvasElement): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    input.keyEvent(event.code, true);
  };
  const onKeyUp = (event: KeyboardEvent): void => {
    input.keyEvent(event.code, false);
  };
  const onClick = (): void => {
    // Newer engines return a Promise (rejected if the browser denies the lock);
    // older typings say void. Catch either way without assuming.
    const result = canvas.requestPointerLock() as unknown;
    if (result instanceof Promise) {
      result.catch(() => {
        /* pointer lock denied — mouse look simply stays inactive */
      });
    }
  };
  const onMouseMove = (event: MouseEvent): void => {
    if (document.pointerLockElement === canvas) {
      input.lookEvent(event.movementX, event.movementY);
    }
  };
  const onPointer = (event: PointerEvent): void => {
    input.pointerEvent(event.offsetX, event.offsetY, event.buttons !== 0);
  };
  const onPointerLeave = (): void => {
    input.pointerClear();
  };

  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  window.addEventListener("mousemove", onMouseMove);
  canvas.addEventListener("click", onClick);
  canvas.addEventListener("pointerdown", onPointer);
  canvas.addEventListener("pointermove", onPointer);
  canvas.addEventListener("pointerup", onPointer);
  canvas.addEventListener("pointerleave", onPointerLeave);

  return (): void => {
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("keyup", onKeyUp);
    window.removeEventListener("mousemove", onMouseMove);
    canvas.removeEventListener("click", onClick);
    canvas.removeEventListener("pointerdown", onPointer);
    canvas.removeEventListener("pointermove", onPointer);
    canvas.removeEventListener("pointerup", onPointer);
    canvas.removeEventListener("pointerleave", onPointerLeave);
  };
}
