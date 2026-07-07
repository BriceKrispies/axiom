/*
 * The browser boot harness for the @axiom/game HOT-RUNTIME dev loop.
 *
 * This is the host / platform edge — NOT engine spine — so it lives in an app
 * `web/` dir, outside the branchless + coverage gates, and uses ordinary control
 * flow.
 *
 * ## The long-lived engine host (the point of HMR)
 * Unlike the old "deterministic re-run" harness, this constructs the `WasmGame`
 * exactly ONCE and keeps it alive for the whole session. It builds a `HotRuntime`
 * (the SDK's `createHotRuntime`) over that instance, mounts the author's `defineApp`
 * manifest, and — on every Vite HMR update of `./game.ts` — calls
 * `runtime.apply(next)`, which diffs the new manifest against the live one and
 * SWAPS the changed system's body on the next tick barrier. The wasm engine, the
 * ECS world, the canvas/backend binding, the uploaded textures, and the tick
 * counter all persist across the edit. `globalThis.__wasmGameConstructCount` stays 1.
 *
 * ## The engine presents 2D — no TypeScript interpreter
 * Presentation is the engine's job. The author's `orb.draw` render system records
 * into the native `draw2d` builder; the SDK's own `present2d` boot presenter (a
 * boot concern, run after the frame's render systems) drains it into the engine's
 * layer-sorted command list and hands it to `axiom-windowing` — the SAME live
 * presenter a 3D game uses (WebGPU → WebGL2 → Canvas 2D). The harness owns no
 * `getContext("2d")` and no per-shape drawing.
 *
 * ## Sprites + text (Tier-0) — fetch in the app, present in the engine
 * The `present2d` presenter runs `beforePresent` each frame; we hook the texture
 * loader there: it polls the wasm core for the handles the game registered
 * (`textureIds`), `fetch`+decodes each url (`textureUrl`) to RGBA8, and uploads it
 * under the same id (`upload2dTexture`). The monospace font atlas is baked once and
 * uploaded under the reserved font-atlas id, so the engine's text path samples it.
 */

import { type AppManifest, type BootGame, createHotRuntime } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import manifest from "./game.ts";

const FIXED_HZ = 60;
const NANOS_PER_SECOND = 1_000_000_000;
const MAX_STEPS_PER_FRAME = 8;

// The baked monospace atlas grid — MUST match apps/axiom-game-runtime/src/font.rs.
const ATLAS_COLS = 16;
const ATLAS_ROWS = 6;
const CELL_W = 8;
const CELL_H = 16;
// The reserved font-atlas texture id — MUST match `font.rs` FONT_ATLAS_TEXTURE.
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

/** The per-run texture loader: discover the handles the game registered and upload any not yet uploaded. */
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

  // Load the wasm engine MODULE once, then construct the engine instance ONCE.
  await initWasm();
  const atlas = bakeFontAtlas();
  const fixedStepNanos = Math.round(NANOS_PER_SECOND / FIXED_HZ);

  const game = new WasmGame(fixedStepNanos, MAX_STEPS_PER_FRAME);
  // The construct-count the browser proof asserts stays 1 across a system-body edit.
  const globals = globalThis as unknown as { __wasmGameConstructCount: number; __game: WasmGame };
  globals.__wasmGameConstructCount = 1;
  globals.__game = game;

  game.bind2dSurface("c");
  game.upload2dTexture(FONT_ATLAS_TEXTURE, atlas.width, atlas.height, atlas.pixels);
  const loadTextures = makeTextureLoader(game);

  // The long-lived hot runtime: mount the author manifest, boot once, present 2D.
  const runtime = createHotRuntime(game as unknown as BootGame, manifest, {
    canvas,
    onEngineRestart: (): void => {
      // A config change (fixedHz/seed/surface) needs a fresh engine — fall back to a page reload.
      globalThis.location.reload();
    },
    present2d: {
      beforePresent: (): void => {
        loadTextures();
        status.textContent = `live · engine alive · tick ${game.current_tick}`;
      },
    },
  });

  // Vite HMR: on a `./game.ts` edit, apply the new manifest into the LIVE engine.
  if (import.meta.hot) {
    import.meta.hot.accept("./game.ts", (mod: { readonly default: AppManifest } | undefined): void => {
      const next = mod?.default;
      if (!next) {
        return;
      }
      const kind = runtime.apply(next);
      status.classList.add("flash");
      status.textContent = `hot · ${kind} · tick ${game.current_tick} (engine #${globals.__wasmGameConstructCount})`;
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  }
};

void boot_();
