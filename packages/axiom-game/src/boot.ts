/*
 * The platform-edge BOOT aggregator: the single entry a browser page calls to
 * bring a built `WasmGame` to life. It is @axiom/game's analogue of @axiom/client's
 * `build-transport.ts` — it binds the live wasm object plus browser timer/event
 * APIs, so the branch ban and the unsafe/async rules are scoped off here
 * (documented in `.oxlintrc.json`) and it is coverage-exempt (browser-only,
 * verified via the Playwright path; see the `--test-coverage-exclude` in
 * `package.json`). Every deterministic piece it touches lives behind the bridges
 * it installs — the `GameLoop` core, the `NativeBridge`/`HostBridge` adapters, the
 * DOM-input edge — each covered (or coverage-exempt) on its own; this file only
 * wires them together.
 *
 * It performs the four boot steps and returns a teardown:
 *   1. install the native host channel  — `bindNative(hostFromWasm(game))`;
 *   2. build the deterministic loop      — `new GameLoop(bridgeFromWasm(game), …)`
 *      and drive it from `requestAnimationFrame` (`driveRaf`), gated on the game's
 *      running status (pause/stop freeze the accumulator);
 *   3. wire DOM input                     — `driveDomInput`;
 *   4. preload declared sound assets      — `loadSound` per URL (the app owns
 *      fetch/decode; this only registers them through the host channel).
 */

import { type DomInputTarget, driveDomInput } from "./dom-input.ts";
import { type WasmGameExport, bridgeFromWasm } from "./wasm-bridge.ts";
import { type WasmHostExport, hostFromWasm } from "./wasm-host.ts";
import type { Game } from "./game.ts";
import { GameLoop } from "./game-loop.ts";
import { bindNative } from "./host-binding.ts";
import { defaultRegistry } from "./registry.ts";
import { driveRaf } from "./raf-loop.ts";
import { loadSound } from "./sound.ts";

/** The live wasm game the boot path drives — it satisfies all three wasm seams at once. */
export type BootGame = WasmGameExport & WasmHostExport & DomInputTarget;

/** The browser surface to read pointer events from, plus optional sound URLs to preload. */
export interface BootOptions {
  readonly canvas: HTMLCanvasElement;
  readonly sounds?: readonly string[];
}

/**
 * Boot `game` (the wasm object) against the author's `app` lifecycle and the
 * browser `options`. Returns a stop function that halts the RAF chain and removes
 * the DOM listeners.
 */
export const boot = (game: BootGame, app: Game, options: BootOptions): (() => void) => {
  bindNative(hostFromWasm(game));
  const loop = new GameLoop(bridgeFromWasm(game), app.config.fixedHz, defaultRegistry);
  const stopInput = driveDomInput(game, options.canvas);
  const stopRaf = driveRaf(loop, (): boolean => app.status === "running");
  // Preload declared sound assets (a plain loop — the branch ban is off at this edge).
  for (const url of options.sounds ?? []) {
    loadSound(url);
  }
  return (): void => {
    stopRaf();
    stopInput();
  };
};
