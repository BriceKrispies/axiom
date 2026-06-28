/*
 * The platform edge: the requestAnimationFrame + performance.now() impure driver
 * and the real wasm bridge load. This is @axiom/game's analogue of @axiom/client's
 * webtransport.ts and the Rust spine's host/windowing layers — it binds browser
 * APIs and the live wasm module, so a documented subset of rules (the branch ban,
 * async/await, optional chaining, no-unsafe-*) is scoped off here and it is
 * coverage-exempt (browser-only; verified via the Playwright path) — see its
 * .oxlintrc.json override and the --test-coverage-exclude in package.json.
 *
 * Everything deterministic lives behind this edge: the GameLoop + stepFrame core
 * is pure and fully covered; here we only measure real elapsed time and load wasm.
 * Because the branch ban is off here, this file uses ordinary `if` control flow.
 */

import type { GameLoop } from "./game-loop.ts";
import type { NativeBridge } from "./native-bridge.ts";
import type { StepBudget } from "./step-budget.ts";

/** The raw `WasmGame` exports `apps/axiom-game-runtime` produces (snake_case getters). */
export interface WasmGameExport {
  readonly advance: (elapsedNanos: bigint) => {
    readonly fixed_step_nanos: bigint;
    readonly remainder_nanos: bigint;
    readonly steps: number;
  };
  readonly snapshot: () => Uint8Array;
}

/** Adapt the snake_case wasm `WasmGame` to the loop core's camelCase NativeBridge. */
export const bridgeFromWasm = (game: WasmGameExport): NativeBridge => ({
  advance(elapsedNanos: number): StepBudget {
    const report = game.advance(BigInt(Math.round(elapsedNanos)));
    return {
      fixedStepNanos: Number(report.fixed_step_nanos),
      remainderNanos: Number(report.remainder_nanos),
      steps: report.steps,
    };
  },
  snapshot(): Uint8Array {
    return game.snapshot();
  },
});

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
