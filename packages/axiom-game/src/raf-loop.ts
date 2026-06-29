/*
 * The platform edge: the requestAnimationFrame + performance.now() impure driver.
 * This is @axiom/game's analogue of @axiom/client's webtransport.ts and the Rust
 * spine's host/windowing layers — it binds browser timing, so a documented subset
 * of rules (the branch ban, the running-state gate) is scoped off here and it is
 * coverage-exempt (browser-only; verified via the Playwright path) — see its
 * .oxlintrc.json override and the --test-coverage-exclude in package.json.
 *
 * The wasm↔bridge marshalling that used to live here now lives in `wasm-bridge.ts`
 * (the adapter edge): `bridgeFromWasm` builds a `NativeBridge` from the raw
 * `WasmGame` exports. It is re-exported here so the host's existing
 * `raf-loop`-rooted import keeps working. Everything deterministic lives behind
 * these two edges: the GameLoop + stepFrame core is pure and fully covered; here
 * we only measure real elapsed time. Because the branch ban is off here, this file
 * uses ordinary `if` control flow.
 */

import type { GameLoop } from "./game-loop.ts";

export type { WasmGameExport } from "./wasm-bridge.ts";
export { bridgeFromWasm } from "./wasm-bridge.ts";

const NANOS_PER_MILLI = 1_000_000;

/*
 * Drive `loop.advance` from requestAnimationFrame, measuring each frame's elapsed
 * time with performance.now() and converting to nanoseconds. `isRunning` gates
 * whether a frame steps the sim (pause/stop freeze the accumulator). Returns a
 * stop function that halts the RAF chain.
 */
export const driveRaf = (loop: GameLoop, isRunning: () => boolean): (() => void) => {
  let last = performance.now();
  let active = true;
  const frame = (now: number): void => {
    const elapsedNanos = (now - last) * NANOS_PER_MILLI;
    last = now;
    if (isRunning()) {
      loop.advance(elapsedNanos);
    }
    if (active) {
      requestAnimationFrame(frame);
    }
  };
  requestAnimationFrame(frame);
  return (): void => {
    active = false;
  };
};
