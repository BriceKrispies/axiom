/*
 * The platform-edge HEADLESS driver: the non-browser analogue of `boot.ts`. Where
 * `boot` drives the loop from `requestAnimationFrame` and feeds it DOM input, this
 * binds the same live `WasmGame` and returns a hand-cranked handle — a caller
 * (an agent harness, an integration test) steps the deterministic loop itself and
 * injects input programmatically, with no browser, no clock, and no canvas.
 *
 * Like `boot.ts` it touches the live wasm object directly, so the branch ban and
 * the unsafe/async rules are scoped off here (documented in `.oxlintrc.json`) and
 * it is coverage-exempt (no wasm under node:test; exercised via the integration /
 * Playwright path — see the exempt list in `test-exempt.json`). Every
 * deterministic piece it touches lives behind the bridges it installs — the
 * `GameLoop` core, the `NativeBridge`/`HostBridge` adapters, the native input
 * injection — each covered (or coverage-exempt) on its own; this file only wires
 * them together and forwards.
 *
 * The injection methods (`key` / `pointer` / `clearPointer` / `setSurface`) are
 * pure pass-throughs to the same `WasmGame` exports the DOM edge feeds
 * (`inputKey` / `inputPointerEvent` / `inputPointerClear` / `inputSetSurface`), so
 * the deterministic input semantics (action table, per-tick edges, swipe) stay
 * native-side and identical to the browser path.
 */

import { type WasmGameExport, bridgeFromWasm } from "./wasm-bridge.ts";
import { type WasmHostExport, hostFromWasm } from "./wasm-host.ts";
import type { DomInputTarget } from "./dom-input.ts";
import type { Game } from "./game.ts";
import { GameLoop } from "./game-loop.ts";
import type { StepBudget } from "./step-budget.ts";
import { bindNative } from "./host-binding.ts";

const NANOS_PER_SECOND = 1_000_000_000;

/** The live wasm game the headless path drives — it satisfies all three wasm seams at once (identical to `BootGame`). */
export type HeadlessGame = WasmGameExport & WasmHostExport & DomInputTarget;

/** A hand-cranked driver over a live `WasmGame`: step the deterministic loop, inject input, read snapshots. */
export interface HeadlessHandle {
  /** Bank `elapsedNanos` and run the resulting fixed steps + one render; returns the integer step budget. */
  readonly step: (elapsedNanos: number) => StepBudget;
  /** Step exactly `ticks` whole fixed steps (`ticks * fixedStepNanos`), the deterministic agent cadence. */
  readonly stepTicks: (ticks: number) => StepBudget;
  /** Inject a key edge by its layout-stable `KeyboardEvent.code` (native action table resolves it). */
  readonly key: (token: string, down: boolean) => void;
  /** Inject a pointer sample at canvas-relative `(x, y)` with whether any button is held. */
  readonly pointer: (x: number, y: number, down: boolean) => void;
  /** Clear the pointer (pointer-left-surface), matching the DOM edge's `pointerleave`. */
  readonly clearPointer: () => void;
  /** Report the surface size so the native swipe threshold scales to it. */
  readonly setSurface: (width: number, height: number) => void;
  /** The durable simulation state as opaque bytes, from the native bridge. */
  readonly snapshot: () => Uint8Array;
  /** The monotonic count of fixed ticks driven so far. */
  readonly currentTick: number;
}

/**
 * Wire `game` (the live wasm object) to `app`'s registry and return a headless
 * driver. Mirrors `boot`'s native+loop binding, but drives nothing on its own —
 * the caller cranks `step` / `stepTicks` and injects input. The caller owns the
 * lifecycle (`app.start()`); `headless` only installs the seams.
 */
export const headless = (game: HeadlessGame, app: Game): HeadlessHandle => {
  bindNative(hostFromWasm(game));
  const loop = new GameLoop(bridgeFromWasm(game), app.config.fixedHz, app.registry);
  const fixedStepNanos = NANOS_PER_SECOND / app.config.fixedHz;
  return {
    clearPointer: (): void => {
      game.inputPointerClear();
    },
    get currentTick(): number {
      return loop.tick;
    },
    key: (token: string, down: boolean): void => {
      game.inputKey(token, down);
    },
    pointer: (x: number, y: number, down: boolean): void => {
      game.inputPointerEvent(x, y, down);
    },
    setSurface: (width: number, height: number): void => {
      game.inputSetSurface(width, height);
    },
    snapshot: (): Uint8Array => loop.snapshot(),
    step: (elapsedNanos: number): StepBudget => loop.advance(elapsedNanos),
    stepTicks: (ticks: number): StepBudget => loop.advance(ticks * fixedStepNanos),
  };
};
