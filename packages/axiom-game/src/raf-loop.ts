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
/*
 * The host-channel counterpart of `bridgeFromWasm`: builds the installed
 * `HostBridge` (math3d / grid / audio) the free authoring surface projects
 * through. Re-exported beside the `NativeBridge` builder so the boot path has one
 * platform-edge import for both wasm adapters.
 */
export type { WasmHostExport } from "./wasm-host.ts";
export { hostFromWasm } from "./wasm-host.ts";

const NANOS_PER_MILLI = 1_000_000;

/*
 * The longest single frame the loop will bank. A stall — a slow first frame
 * (wasm + GPU-backend init), a backgrounded tab whose rAF is throttled, a long GC
 * pause — otherwise hands the fixed-step accumulator seconds of elapsed time at
 * once. The accumulator caps the STEPS it runs per frame but keeps the excess
 * banked, so the sim then sprints at its max step rate for many frames to "catch
 * up" — which reads as the whole game (movement, enemies) running at multiples of
 * real speed. Clamping the per-frame elapsed drops that excess instead: a frame
 * never advances more than this much wall time, so real-time pacing is preserved
 * and a stall costs a few dropped frames, not a fast-forward. 100 ms is ~6 ticks
 * at 60 Hz — enough to smooth an ordinary hiccup, far short of a runaway.
 */
const MAX_FRAME_MILLIS = 100;
const MAX_FRAME_NANOS = MAX_FRAME_MILLIS * NANOS_PER_MILLI;

/*
 * Drive `loop.advance` from requestAnimationFrame, measuring each frame's elapsed
 * time with performance.now() and converting to nanoseconds. `isRunning` gates
 * whether a frame steps the sim (pause/stop, or a scrubbing/blurred 3D overlay,
 * freeze the accumulator). The optional `present` runs EVERY frame, even when not
 * stepping — a 3D game passes a presenter that paints the authored scene (or the
 * frozen / scrubbed-to frame) to its bound surface (SPEC-11); a 2D game omits it
 * and presents through its own `onRender` instead.
 *
 * `frameLockNanos` selects the pacing model. `0` is the default real-time model:
 * the sim banks the *measured* elapsed time, so it runs at a fixed wall-clock rate
 * (e.g. 60 Hz) no matter the display's refresh rate. A positive value is the
 * frame-locked model: every displayed frame advances exactly that fixed step
 * (one tick per frame), so the sim is paced by the refresh rate — the model an
 * engine that "steps the sim exactly once per frame" uses, and the one a port
 * matches for byte-for-byte parity with such a loop.
 *
 * Returns a stop function that halts the RAF chain.
 */
/** The per-frame hooks + pacing the rAF driver needs (bundled to stay within the parameter budget). */
export interface RafConfig {
  /** Whether this frame steps the sim (pause/stop/scrub/blur gate it false). */
  readonly isRunning: () => boolean;
  /** Runs every frame, stepped or not — the 3D presenter, or a no-op for 2D. */
  readonly present: () => void;
  /** `0` = real-time (bank measured elapsed); `> 0` = frame-locked (bank this fixed step per frame). */
  readonly frameLockNanos: number;
}

export const driveRaf = (loop: GameLoop, config: RafConfig): (() => void) => {
  let last = performance.now();
  let active = true;
  const frame = (now: number): void => {
    // Clamp the per-frame measured elapsed so a stall cannot bank a fast-forward
    // (above). Frame-locked games ignore this and bank one fixed step per frame.
    const measuredNanos = Math.min((now - last) * NANOS_PER_MILLI, MAX_FRAME_NANOS);
    last = now;
    const elapsedNanos = config.frameLockNanos || measuredNanos;
    if (config.isRunning()) {
      loop.advance(elapsedNanos);
    }
    // Present every frame (a frozen or scrubbed-to frame stays up); no-op for 2D.
    config.present();
    if (active) {
      requestAnimationFrame(frame);
    }
  };
  requestAnimationFrame(frame);
  return (): void => {
    active = false;
  };
};
