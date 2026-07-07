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
import type { NativeBridge } from "./native-bridge.ts";
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

/**
 * The wasm method a 2D game's boot path drives to present the frame the author's
 * render systems drew: it drains the native `draw2d` builder into the engine's
 * layer-sorted command list and rasterizes it (WebGPU → WebGL2 → Canvas 2D). This is
 * the platform-edge dual of `renderScene` — presentation is a boot concern, run AFTER
 * the frame's render systems, not a system the author registers.
 */
export interface Present2dGame {
  readonly present2d: () => void;
}

/** The live wasm game the boot path drives — it satisfies every wasm seam at once. */
export type BootGame = WasmGameExport & WasmHostExport & DomInputTarget & Present3dGame & Present2dGame;

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
   * Present a 2D game's drawn frame each frame. Set it for a 2D game (the engine
   * rasterizes the `draw2d` command list the author's render systems recorded);
   * omit it for a 3D game. `beforePresent` runs just before the present each frame —
   * the app uses it to upload any newly-referenced sprite/atlas textures (SPEC-04
   * "fetch in the app").
   */
  readonly present2d?: { readonly beforePresent?: () => void };
  /**
   * Pace the sim by the display's refresh rate — exactly one fixed tick per
   * rendered frame — instead of by wall-clock time. Off by default (real-time:
   * the sim runs at `fixedHz` regardless of frame rate). Set it to match an engine
   * loop that "steps the sim once per frame"; a port wanting byte-for-byte parity
   * with such a loop (e.g. the Rust retro FPS, which ticks once per frame) turns it on,
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
 * Build the after-advance 2D presenter (a no-op unless `present2d` is set). Each
 * frame it runs the app's `beforePresent` (texture upload) then drains + rasterizes
 * the `draw2d` command list the author's render systems recorded this frame.
 */
const make2dPresenter = (game: Present2dGame, options: BootOptions): (() => void) => {
  const config = options.present2d;
  if (!config) {
    return (): void => {
      // 3D (or headless) game: nothing to present through the 2D path.
    };
  }
  return (): void => {
    config.beforePresent?.();
    game.present2d();
  };
};

/** Compose the 2D and 3D after-advance presenters into the single presenter the RAF driver runs each frame. */
const makePresenter = (game: BootGame, options: BootOptions): (() => void) => {
  const present3d = make3dPresenter(game, options);
  const present2d = make2dPresenter(game, options);
  return (): void => {
    present3d();
    present2d();
  };
};

/** A booted session: the live deterministic `GameLoop`, the `NativeBridge` it drives (for the hot runtime's world reconciliation), plus the teardown. */
export interface BootSession {
  readonly loop: GameLoop;
  readonly bridge: NativeBridge;
  readonly teardown: () => void;
}

/**
 * The shared boot wiring both `boot` and `createHotRuntime` (`hot-runtime.ts`) use:
 * install the native host channel, build the deterministic `GameLoop` over the wasm
 * bridge, wire DOM input, drive `requestAnimationFrame`, and preload sounds. It
 * RETURNS the live loop (so the hot runtime can enqueue tick-barrier updates onto
 * it) alongside the teardown, whereas `boot` exposes only the teardown.
 */
export const bootSession = (game: BootGame, app: Game, options: BootOptions): BootSession => {
  bindNative(hostFromWasm(game));
  const bridge = bridgeFromWasm(game);
  const loop = new GameLoop(bridge, app.config.fixedHz, app.registry);
  const stopInput = driveDomInput(game, options.canvas);
  /*
   * Present a 3D game's retained scene every frame (a no-op for a 2D game), but
   * only STEP the sim while running AND the engine's frame-scrubber overlay is
   * interactive (not scrubbing, not blurred). The presenter paints every frame
   * regardless, so a frozen or scrubbed-to frame stays on screen.
   */
  const present = makePresenter(game, options);
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
  return {
    bridge,
    loop,
    teardown: (): void => {
      stopRaf();
      stopInput();
    },
  };
};

/**
 * Boot `game` (the wasm object) against the author's `app` lifecycle and the
 * browser `options`. Returns a stop function that halts the RAF chain and removes
 * the DOM listeners.
 */
export const boot = (game: BootGame, app: Game, options: BootOptions): (() => void) =>
  bootSession(game, app, options).teardown;
