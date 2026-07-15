/*
 * The platform-edge DOM binding for the pure `InputState` core in `input.ts`.
 * This is the impure boundary: the ONLY place that touches the DOM, wiring
 * window/canvas listeners into an `InputState` and returning a detach function.
 * Keyboard events feed `keyEvent`; clicking the canvas requests pointer lock
 * (rejection caught — the browser may deny it); pointer-locked mouse movement
 * feeds `lookEvent`; canvas pointer events feed `pointerEvent`/`pointerClear`;
 * focus/lock loss releases everything. Like the engine's other browser
 * boundaries it sits OUTSIDE the branchless / 100%-coverage spine laws (see the
 * `.oxlintrc.json` override and `test-exempt.json`); its correctness is proven by
 * the live browser path. Because the branch ban is off here, this file uses
 * ordinary control flow.
 */

import type { InputState } from "./input.ts";

/*
 * The pointer-lock arm: clicking the canvas captures the pointer; while locked,
 * raw mouse movement accumulates as the relative look, and losing focus or the
 * lock releases every held key (so a held charge resolves as a clean release
 * edge, and regaining focus fabricates nothing). Kept as a helper so
 * `attachDomInput` stays within its statement/size budget.
 */
const pointerLockRegistrations = (
  input: InputState,
  canvas: HTMLCanvasElement,
): readonly [EventTarget, string, EventListener][] => {
  const onClick = (): void => {
    /* Newer engines return a Promise (rejected if the browser denies the lock);
       older typings say void — catch either way without assuming. */
    const result = canvas.requestPointerLock() as unknown;
    if (result instanceof Promise) {
      result.catch((): void => {
        // A denied pointer-lock request is a non-issue; mouse look stays inactive.
      });
    }
  };
  const onMouseMove = (event: MouseEvent): void => {
    if (document.pointerLockElement === canvas) {
      input.lookEvent(event.movementX, event.movementY);
    }
  };
  const onLockChange = (): void => {
    if (document.pointerLockElement !== canvas) {
      input.releaseAllKeys();
    }
  };
  return [
    [canvas, "click", onClick as EventListener],
    [globalThis, "mousemove", onMouseMove as EventListener],
    [document, "pointerlockchange", onLockChange as EventListener],
  ];
};

/** Whether `attachDomInput` captures the pointer. A mouse-look game wants the
 * default (click the canvas → pointer lock → relative look); a cursor-driven
 * game (pickers, menus, clickable objects) sets `pointerLock: false` so the
 * cursor stays visible and clicks stay clicks. */
export interface DomInputOptions {
  readonly pointerLock?: boolean;
}

/**
 * The browser edge: wire window/canvas listeners into `input` and return a detach
 * function that removes every listener it added. Keyboard events feed `keyEvent`;
 * canvas pointer events feed `pointerEvent`/`pointerClear`; the pointer-lock arm
 * (mouse look + lock-loss release) is added from `pointerLockRegistrations`
 * unless `opts.pointerLock` is `false`. Focus loss always releases held keys.
 */
export const attachDomInput = (input: InputState, canvas: HTMLCanvasElement, opts: DomInputOptions = {}): (() => void) => {
  const onKeyDown = (event: KeyboardEvent): void => {
    input.keyEvent(event.code, true);
  };
  const onKeyUp = (event: KeyboardEvent): void => {
    input.keyEvent(event.code, false);
  };
  const onPointer = (event: PointerEvent): void => {
    input.pointerEvent(event.offsetX, event.offsetY, event.buttons !== 0);
  };
  const onPointerLeave = (): void => {
    input.pointerClear();
  };
  const onBlur = (): void => {
    input.releaseAllKeys();
    input.pointerClear();
  };
  /* Each listener is a [node, type, handler] registration (key + pointer + focus
     set, then the optional pointer-lock arm), added and removed by one loop each. */
  const registrations: readonly [EventTarget, string, EventListener][] = [
    [globalThis, "keydown", onKeyDown as EventListener],
    [globalThis, "keyup", onKeyUp as EventListener],
    [canvas, "pointerdown", onPointer as EventListener],
    [canvas, "pointermove", onPointer as EventListener],
    [canvas, "pointerup", onPointer as EventListener],
    [canvas, "pointerleave", onPointerLeave as EventListener],
    [globalThis, "blur", onBlur as EventListener],
    ...(opts.pointerLock === false ? [] : pointerLockRegistrations(input, canvas)),
  ];
  for (const [node, type, handler] of registrations) {
    node.addEventListener(type, handler);
  }
  return (): void => {
    for (const [node, type, handler] of registrations) {
      node.removeEventListener(type, handler);
    }
  };
};
