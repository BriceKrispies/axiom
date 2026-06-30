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
import { driveRaf } from "./raf-loop.ts";
import { loadSound } from "./sound.ts";

/**
 * The wasm methods a 3D game's boot path drives to present its authored scene
 * (SPEC-11): bind the canvas once the scene exists, then render it each frame. A
 * 2D game never calls these (it presents through its own `onRender`).
 */
export interface Present3dGame {
  readonly bindSurface: (canvasId: string, maxInstances: number) => void;
  readonly renderScene: () => void;
  /** Whether to step the sim this frame — false while the frame-scrubber overlay is scrubbing or after focus loss (Escape / blur). */
  readonly isInteractive: () => boolean;
}

/** The live wasm game the boot path drives — it satisfies every wasm seam at once. */
export type BootGame = WasmGameExport & WasmHostExport & DomInputTarget & Present3dGame;

/** The default per-frame instance-buffer cap a 3D surface binds with when the app names none. */
const DEFAULT_MAX_INSTANCES = 4096;

/** Nanoseconds per second — the numerator of the frame-locked fixed step `1s / fixedHz`. */
const NANOS_PER_SECOND = 1_000_000_000;

/** The per-frame fixed step for a frame-locked game (`1s / fixedHz`), or `0` for the real-time default. */
const frameLockStep = (options: BootOptions, fixedHz: number): number => {
  let nanos = 0;
  if (options.frameLocked === true) {
    nanos = NANOS_PER_SECOND / fixedHz;
  }
  return nanos;
};

/** The browser surface to read pointer events from, plus optional sound URLs and 3D presentation. */
export interface BootOptions {
  readonly canvas: HTMLCanvasElement;
  readonly sounds?: readonly string[];
  /**
   * Present an authored 3D scene each frame (SPEC-11). Set it for a 3D game (the
   * engine renders the retained scene to the canvas); omit it for a 2D game (which
   * draws through `onRender`). `maxInstances` caps the per-frame instance buffer.
   */
  readonly present3d?: { readonly maxInstances?: number };
  /**
   * Pace the sim by the display's refresh rate — exactly one fixed tick per
   * rendered frame — instead of by wall-clock time. Off by default (real-time:
   * the sim runs at `fixedHz` regardless of frame rate). Set it to match an engine
   * loop that "steps the sim once per frame"; a port wanting byte-for-byte parity
   * with such a loop (e.g. the Rust DOOM, which ticks once per frame) turns it on,
   * accepting that — like that loop — the game's wall-clock speed then scales with
   * the frame rate.
   */
  readonly frameLocked?: boolean;
}

/**
 * Build the after-advance presenter. For a 3D game (`present3d` set) it binds the
 * surface lazily on the first frame — so the scene the author built during that
 * first advance is already populated when its meshes/materials upload — then
 * renders the scene every frame. For a 2D game it is a no-op (the game presents
 * through its own `onRender`), so the boot path can always install one presenter.
 */
const make3dPresenter = (game: Present3dGame, options: BootOptions): (() => void) => {
  const config = options.present3d;
  if (!config) {
    return (): void => {
      // 2D game: nothing to present after advance; it draws through `onRender`.
    };
  }
  const maxInstances = config.maxInstances ?? DEFAULT_MAX_INSTANCES;
  let bound = false;
  return (): void => {
    if (!bound) {
      game.bindSurface(options.canvas.id, maxInstances);
      bound = true;
    }
    game.renderScene();
  };
};

/**
 * Boot `game` (the wasm object) against the author's `app` lifecycle and the
 * browser `options`. Returns a stop function that halts the RAF chain and removes
 * the DOM listeners.
 */
export const boot = (game: BootGame, app: Game, options: BootOptions): (() => void) => {
  bindNative(hostFromWasm(game));
  const loop = new GameLoop(bridgeFromWasm(game), app.config.fixedHz, app.registry);
  const stopInput = driveDomInput(game, options.canvas);
  /*
   * Present a 3D game's retained scene every frame (a no-op for a 2D game), but
   * only STEP the sim while running AND the engine's frame-scrubber overlay is
   * interactive (not scrubbing, not blurred). The presenter paints every frame
   * regardless, so a frozen or scrubbed-to frame stays on screen.
   */
  const present = make3dPresenter(game, options);
  const running = (): boolean => app.status === "running" && game.isInteractive();
  const stopRaf = driveRaf(loop, {
    frameLockNanos: frameLockStep(options, app.config.fixedHz),
    isRunning: running,
    present,
  });
  // Preload declared sound assets (a plain loop — the branch ban is off at this edge).
  for (const url of options.sounds ?? []) {
    loadSound(url);
  }
  return (): void => {
    stopRaf();
    stopInput();
  };
};
