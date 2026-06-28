/*
 * The shape of the native (wasm) runtime the loop drives — the deterministic
 * core's only contact with `apps/axiom-game-runtime`'s `WasmGame` exports. The
 * pure loop core depends on this interface, never on a live wasm module, so its
 * tests inject a FAKE bridge (no wasm needed) and stay fully covered — exactly as
 * @axiom/client tests its codec against a fake socket. The platform edge
 * (`raf-loop.ts`) adapts the real `WasmGame` to this interface.
 */

import type { StepBudget } from "./step-budget.ts";

/** The native fixed-step runtime, as the loop core sees it. */
export interface NativeBridge {
  /** Bank `elapsedNanos` of real time and report the resulting integer step budget. */
  readonly advance: (elapsedNanos: number) => StepBudget;
  /** The durable simulation state as opaque bytes (for checkpoint / determinism checks). */
  readonly snapshot: () => Uint8Array;
}
