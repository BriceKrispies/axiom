/*
 * `bootHotApp` — the ONE-CALL browser boot for a hot-reloadable @axiom/game app, so a
 * new app is `defineApp(...)` + a ~5-line harness instead of ~50-110 lines of copied
 * wiring. It is the platform edge (like `boot.ts`): listed in `test-exempt.json`, its
 * `.oxlintrc.json` override scopes off the branch ban + async/unsafe rules, and it
 * carries no unit test (browser-only, proven via the Playwright path).
 *
 * It absorbs everything every hot-reloadable app repeated by hand:
 *   - load the wasm MODULE once, construct the `WasmGame` ONCE (never per reload), and
 *     publish `globalThis.__wasmGameConstructCount` / `__game` for the browser proof;
 *   - for a 2D app: bind the live 2D surface, bake + upload the monospace font atlas,
 *     and drain the game's registered sprite/atlas textures each frame ("fetch in the
 *     app", SPEC-04) — all generic, none app-specific;
 *   - build the long-lived `HotRuntime` over the manifest (2D `present2d` or 3D
 *     `present3d`), defaulting `onEngineRestart` to a page reload;
 *   - drive an optional per-frame `onFrame(game)` hook (the app's DOM HUD / status).
 *
 * The ONE thing it cannot hide is the `import.meta.hot.accept("./game.ts", …)` line:
 * Vite resolves that dep specifier relative to the ACCEPTING module, so it must live in
 * the app harness. The harness passes the returned runtime's `apply` there — one line.
 */

import { type HotRuntime, type HotRuntimeOptions, createHotRuntime } from "./hot-runtime.ts";
import type { AppManifest } from "./manifest.ts";
import type { BootGame } from "./boot.ts";

/** Nanoseconds per second — the numerator of the fixed step `1s / fixedHz`. */
const NANOS_PER_SECOND = 1_000_000_000;
/** Default fixed simulation rate. */
const DEFAULT_FIXED_HZ = 60;
/** Default max catch-up fixed steps per frame. */
const DEFAULT_MAX_STEPS = 8;

// The baked monospace atlas grid — MUST match apps/axiom-game-runtime/src/font.rs.
const ATLAS_COLS = 16;
const ATLAS_ROWS = 6;
const CELL_W = 8;
const CELL_H = 16;
const FONT_BASELINE_INSET = 3;
const CODE_FIRST = 32;
const CODE_LAST = 126;
/** The reserved font-atlas texture id — MUST match `font.rs` FONT_ATLAS_TEXTURE. */
const FONT_ATLAS_TEXTURE = 0x00_F0_00_00;

/** The 2D presentation methods on the wasm game the shared boot drives (beyond the `BootGame` seams). */
export interface Hot2dGame {
  readonly bind2dSurface: (canvasId: string) => void;
  readonly upload2dTexture: (id: number, width: number, height: number, pixels: Uint8Array) => void;
  readonly textureIds: () => readonly number[];
  readonly textureUrl: (id: number) => string;
}

/** The live wasm game `bootHotApp` drives — the boot seams plus the 2D presentation surface. */
export type HotGame = BootGame & Hot2dGame;

/** The one config a hot-reloadable app hands `bootHotApp`. */
export interface HotAppConfig {
  /** The `<canvas>` id the engine presents to (must match `manifest.config.surface`). */
  readonly canvasId: string;
  /** The author's `defineApp` manifest (its default export). */
  readonly manifest: AppManifest;
  /** The wasm-bindgen module initializer (`initWasm` default export from `/pkg`). */
  readonly initWasm: () => Promise<unknown>;
  /** The wasm `WasmGame` class from `/pkg`. */
  readonly WasmGame: new (fixedStepNanos: number, maxSteps: number) => HotGame;
  /** `"2d"` drives the draw2d presenter + font/texture upload; `"3d"` presents the retained scene. */
  readonly present: "2d" | "3d";
  /** Fixed simulation rate (default 60). */
  readonly fixedHz?: number;
  /** Max catch-up steps per frame (default 8). */
  readonly maxSteps?: number;
  /** 3D per-frame instance-buffer cap. */
  readonly maxInstances?: number;
  /** Pace the sim one tick per rendered frame (3D parity default off). */
  readonly frameLocked?: boolean;
  /** Optional per-frame hook for the app's DOM HUD / status read-out. */
  readonly onFrame?: (game: HotGame) => void;
}

/** Decoded RGBA8 pixels plus their dimensions — the upload shape the engine takes. */
interface Rgba8 {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array;
}

/** Read an `OffscreenCanvas`'s pixels as a tight RGBA8 buffer (top-left origin). */
const canvasRgba = (canvas: OffscreenCanvas): Rgba8 => {
  const ctx = canvas.getContext("2d") as OffscreenCanvasRenderingContext2D;
  const image = ctx.getImageData(0, 0, canvas.width, canvas.height);
  return { height: canvas.height, pixels: new Uint8Array(image.data), width: canvas.width };
};

/** Bake the white monospace ASCII atlas (codepoints 32..126) on the `font.rs` grid. */
const bakeFontAtlas = (): Rgba8 => {
  const canvas = new OffscreenCanvas(ATLAS_COLS * CELL_W, ATLAS_ROWS * CELL_H);
  const ctx = canvas.getContext("2d") as OffscreenCanvasRenderingContext2D;
  ctx.fillStyle = "#fff";
  ctx.textBaseline = "top";
  ctx.font = `${CELL_H - FONT_BASELINE_INSET}px monospace`;
  for (let code = CODE_FIRST; code <= CODE_LAST; code += 1) {
    const index = code - CODE_FIRST;
    ctx.fillText(String.fromCodePoint(code), (index % ATLAS_COLS) * CELL_W, Math.floor(index / ATLAS_COLS) * CELL_H);
  }
  return canvasRgba(canvas);
};

/** Decode an `ImageBitmap` to RGBA8 by drawing it onto an `OffscreenCanvas`. */
const bitmapToRgba = (bitmap: ImageBitmap): Rgba8 => {
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d") as OffscreenCanvasRenderingContext2D;
  ctx.drawImage(bitmap, 0, 0);
  return canvasRgba(canvas);
};

/** Build the per-run texture loader: fetch/decode/upload any handle the game registered but not yet uploaded. */
const makeTextureLoader = (game: HotGame): (() => void) => {
  const uploaded = new Set<number>();
  const pending = new Set<number>();
  return (): void => {
    for (const id of game.textureIds()) {
      if (uploaded.has(id) || pending.has(id)) {
        continue;
      }
      const url = game.textureUrl(id);
      if (url === "") {
        continue;
      }
      pending.add(id);
      void fetch(url)
        .then((response) => response.blob())
        .then((blob) => createImageBitmap(blob))
        .then((bitmap) => {
          const { width, height, pixels } = bitmapToRgba(bitmap);
          game.upload2dTexture(id, width, height, pixels);
          uploaded.add(id);
          pending.delete(id);
        })
        .catch(() => {
          pending.delete(id);
        });
    }
  };
};

/** Bind the 2D surface, upload the font atlas, and return the per-frame texture-drain the presenter runs. */
const prepare2d = (game: HotGame, canvasId: string): (() => void) => {
  const atlas = bakeFontAtlas();
  game.bind2dSurface(canvasId);
  game.upload2dTexture(FONT_ATLAS_TEXTURE, atlas.width, atlas.height, atlas.pixels);
  return makeTextureLoader(game);
};

/** Build the `createHotRuntime` options for the chosen presentation mode (no object spread — this edge keeps that rule on). */
const runtimeOptions = (game: HotGame, config: HotAppConfig, canvas: HTMLCanvasElement): HotRuntimeOptions => ({
  canvas,
  frameLocked: config.frameLocked,
  onEngineRestart: (): void => {
    globalThis.location.reload();
  },
  onFullPageReload: (): void => {
    globalThis.location.reload();
  },
  present2d: config.present === "2d" ? { beforePresent: prepare2d(game, config.canvasId) } : undefined,
  present3d: config.present === "3d" ? { maxInstances: config.maxInstances } : undefined,
});

/** Publish the live game on `globalThis` for the browser proof (construct-count stays 1 across reloads). */
const publishGameGlobals = (game: HotGame): void => {
  const globals = globalThis as unknown as { __wasmGameConstructCount: number; __game: HotGame };
  globals.__wasmGameConstructCount = 1;
  globals.__game = game;
};

/** Drive an optional per-frame HUD hook on its own rAF chain (independent of the engine loop). */
const driveFrameHook = (game: HotGame, onFrame?: (game: HotGame) => void): void => {
  if (!onFrame) {
    return;
  }
  const tick = (): void => {
    onFrame(game);
    globalThis.requestAnimationFrame(tick);
  };
  globalThis.requestAnimationFrame(tick);
};

/**
 * Boot a hot-reloadable @axiom/game app in one call and return its `HotRuntime`. The
 * harness then wires HMR with a single line:
 *   `if (import.meta.hot) import.meta.hot.accept("./game.ts", m => runtime.apply(m.default));`
 */
export const bootHotApp = async (config: HotAppConfig): Promise<HotRuntime> => {
  const canvas = document.querySelector(`#${config.canvasId}`) as HTMLCanvasElement;
  await config.initWasm();
  const fixedHz = config.fixedHz ?? DEFAULT_FIXED_HZ;
  const fixedStepNanos = Math.round(NANOS_PER_SECOND / fixedHz);
  const game = new config.WasmGame(fixedStepNanos, config.maxSteps ?? DEFAULT_MAX_STEPS);
  publishGameGlobals(game);
  const runtime = createHotRuntime(game, config.manifest, runtimeOptions(game, config, canvas));
  driveFrameHook(game, config.onFrame);
  return runtime;
};
