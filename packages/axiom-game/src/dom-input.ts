/*
 * The platform edge: DOM key/pointer events → the wasm input injection exports
 * (`inputKey` / `inputPointerEvent` / `inputPointerClear` / `inputSetSurface`).
 * This is @axiom/game's analogue of @axiom/client's `webtransport.ts` and the Rust
 * spine's `host`/`windowing` layers — it binds real browser event APIs, so the
 * branch ban and the unsafe/async rules are scoped off here (documented in
 * `.oxlintrc.json`) and it is coverage-exempt (browser-only, verified via the
 * Playwright path; see the `--test-coverage-exclude` in `package.json`). It is kept
 * MINIMAL: just the listener wiring. The deterministic input semantics — the
 * action-binding table, the per-tick edge resolution, the swipe gesture — all live
 * native-side (`apps/axiom-game-runtime/src/input.rs`); here we only forward raw
 * events and the surface size.
 */

/** The wasm input injection surface this edge feeds — a subset of the `WasmGame` exports. */
export interface DomInputTarget {
  readonly inputKey: (token: string, down: boolean) => void;
  readonly inputPointerEvent: (x: number, y: number, down: boolean) => void;
  readonly inputPointerClear: () => void;
  readonly inputSetSurface: (width: number, height: number) => void;
}

/*
 * Attach key listeners to `window` and pointer listeners to `canvas`, forwarding
 * each raw event to `target`. Reports the canvas size once up front so the native
 * swipe threshold scales to the real surface. A key crosses as its layout-stable
 * `KeyboardEvent.code`; a pointer as its canvas-relative offset plus whether any
 * button is held. Returns a stop function that removes every listener.
 */
export const driveDomInput = (target: DomInputTarget, canvas: HTMLCanvasElement): (() => void) => {
  target.inputSetSurface(canvas.width, canvas.height);
  const onKeyDown = (event: KeyboardEvent): void => {
    target.inputKey(event.code, true);
  };
  const onKeyUp = (event: KeyboardEvent): void => {
    target.inputKey(event.code, false);
  };
  const onPointer = (event: PointerEvent): void => {
    target.inputPointerEvent(event.offsetX, event.offsetY, event.buttons !== 0);
  };
  const onPointerLeave = (): void => {
    target.inputPointerClear();
  };
  // Each listener as a [node, type, handler] registration, so add and remove walk one list.
  const registrations: readonly [EventTarget, string, EventListener][] = [
    [globalThis, "keydown", onKeyDown as EventListener],
    [globalThis, "keyup", onKeyUp as EventListener],
    [canvas, "pointermove", onPointer as EventListener],
    [canvas, "pointerdown", onPointer as EventListener],
    [canvas, "pointerup", onPointer as EventListener],
    [canvas, "pointerleave", onPointerLeave as EventListener],
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
