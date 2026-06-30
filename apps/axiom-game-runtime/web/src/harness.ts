/*
 * The browser boot harness for the @axiom/game hot-reload dev loop.
 *
 * This is the host / platform edge — NOT engine spine — so it lives in an app
 * `web/` dir, outside the branchless + coverage gates, and uses ordinary control
 * flow.
 *
 * It drives the REAL engine path, not a stand-in:
 *   - `createGame()` mints the per-game registry the author registers into;
 *   - `boot()` (the SDK's own platform-edge aggregator) installs the real host
 *     channel (`hostFromWasm`), builds the real `GameLoop` over the real
 *     `NativeBridge` (`bridgeFromWasm`), wires DOM input, and drives `rAF`;
 *   - the author draws through the real `frame.rect/circle/line/sprite/text/…` 2D
 *     surface, which records into the native `draw2d` builder.
 *
 * ## The engine presents 2D — no TypeScript interpreter
 * Presentation is the engine's job, not the harness's. After the author's draws,
 * the harness calls `game.present2d()`, which drains the native `draw2d` builder
 * into its layer-sorted command list and hands it to `axiom-windowing` — the SAME
 * live presenter a 3D game uses. The engine rasterizes it through the WebGPU →
 * WebGL2 → Canvas 2D fallback cascade (the GPU 2D pipeline or the software
 * rasterizer, with byte-for-byte parity). The harness owns no `getContext("2d")`
 * and no per-shape drawing.
 *
 * ## Sprites + text (Tier-0) — fetch in the app, present in the engine
 * The engine's 2D rasterizers sample sprite/atlas textures by id. The harness is
 * the app side of the SPEC-04 "fetch in the app" rule: it polls the wasm core for
 * the texture handles the game has registered (`game.textureIds`), `fetch`+decodes
 * each handle's url (`game.textureUrl`) to RGBA8 pixels, and uploads them under the
 * same id (`game.upload2dTexture`); the engine binds them to its backend. The
 * built-in monospace font's atlas is baked once here (the same fixed ASCII grid
 * `src/font.rs` lays out) and uploaded under the reserved font-atlas id, so the
 * engine's text path samples it exactly as a sprite.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { type Frame, createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
const MAX_STEPS_PER_FRAME = 8;

// The baked monospace atlas grid — MUST match apps/axiom-game-runtime/src/font.rs.
const ATLAS_COLS = 16;
const ATLAS_ROWS = 6;
const CELL_W = 8;
const CELL_H = 16;
// The reserved font-atlas texture id — MUST match `font.rs` FONT_ATLAS_TEXTURE
// (0x00F0_0000). The engine's text path samples the atlas the harness uploads here.
const FONT_ATLAS_TEXTURE = 0x00f0_0000;

/** Decoded RGBA8 pixels plus their dimensions — the upload shape the engine takes. */
type Rgba8 = { readonly width: number; readonly height: number; readonly pixels: Uint8Array };

/** Read an `OffscreenCanvas`'s pixels as a tight RGBA8 buffer (top-left origin). */
const canvasRgba = (canvas: OffscreenCanvas): Rgba8 => {
  const ctx = canvas.getContext("2d") as OffscreenCanvasRenderingContext2D;
  const image = ctx.getImageData(0, 0, canvas.width, canvas.height);
  return { height: canvas.height, pixels: new Uint8Array(image.data.buffer.slice(0)), width: canvas.width };
};

/** Bake the white monospace ASCII atlas (codepoints 32..126) on the `font.rs` grid. */
const bakeFontAtlas = (): Rgba8 => {
  const canvas = new OffscreenCanvas(ATLAS_COLS * CELL_W, ATLAS_ROWS * CELL_H);
  const ctx = canvas.getContext("2d") as OffscreenCanvasRenderingContext2D;
  ctx.fillStyle = "#fff";
  ctx.textBaseline = "top";
  ctx.font = `${CELL_H - 3}px monospace`;
  for (let code = 32; code <= 126; code += 1) {
    const index = code - 32;
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

/*
 * The per-run texture loader: discover the handles the game has registered
 * (`textureIds`), and fetch/decode/upload any not yet uploaded under their id. Each
 * (re)load mints a fresh loader so a new run re-uploads from the same urls.
 */
const makeTextureLoader = (game: WasmGame): (() => void) => {
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

const boot_ = async (): Promise<void> => {
  const canvas = document.getElementById("c") as HTMLCanvasElement;
  const status = document.getElementById("status") as HTMLSpanElement;

  // Load the wasm engine MODULE once; each (re)load mints a fresh game instance.
  await initWasm();
  // Bake the font atlas once (handle-independent pixels); re-uploaded per run.
  const atlas = bakeFontAtlas();
  // A plain `number` (not a BigInt): the wasm `WasmGame` constructor takes the
  // fixed step as f64 so the Binaryen `wasm2js` fallback (no i64/BigInt ABI) runs.
  const fixedStepNanos = Math.round(NANOS_PER_SECOND / FIXED_HZ);

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(fixedStepNanos, MAX_STEPS_PER_FRAME);
    // Bind the engine's live 2D presenter to the canvas (selects the backend:
    // WebGPU → WebGL2 → Canvas 2D), then upload the font atlas under its reserved id.
    game.bind2dSurface("c");
    game.upload2dTexture(FONT_ATLAS_TEXTURE, atlas.width, atlas.height, atlas.pixels);
    const loadTextures = makeTextureLoader(game);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: "c" });
    // The author module registers its onFixedUpdate / onRender into the active
    // (this game's) registry as an import side effect.
    await import(`/dist/game.js?v=${version}`);
    // Register the presenter LAST so it runs after the author's draws: pick up any
    // newly-registered textures, then hand the frame to the engine to present.
    onRender((frame: Frame): void => {
      loadTextures();
      game.present2d();
      status.textContent = `live · deterministic re-run · loop tick ${frame.tick}`;
    });
    app.start();
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, { canvas });
  };
  await load(0);

  // Each save → fresh deterministic run with the new author logic. This live hot-reload
  // is a DEV-SERVER feature (the SSE `/events` stream). A statically packaged bundle
  // (scripts/package_app.py) has no such server, so close the stream the moment a
  // connection fails to open instead of letting EventSource retry the missing endpoint
  // forever. In the dev loop the stream opens (the server replies `: connected`), so
  // `live` flips true and transient drops still auto-reconnect as before.
  const events = new EventSource("/events");
  let live = false;
  events.addEventListener("open", (): void => {
    live = true;
  });
  events.addEventListener("error", (): void => {
    if (!live) {
      events.close();
    }
  });
  events.addEventListener("reload", (event: MessageEvent<string>): void => {
    void load(Number(event.data)).then((): void => {
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  });
};

void boot_();
