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
 *   - the author draws through the real `frame.rect/circle/line/…` 2D surface,
 *     which records into the native `draw2d` builder.
 *
 * The one piece the engine does not yet do itself is PRESENT the 2D surface to a
 * browser canvas. `frame.finish()` now returns a self-describing geometry stream
 * (see apps/axiom-game-runtime/src/draw2d.rs `draw2d_finish`); the `present`
 * function below is the canvas2d interpreter for it. It is registered as the LAST
 * `onRender`, so it runs after the author's draws each frame, drains the builder,
 * and rasterizes.
 *
 * Hot reload is deterministic re-run (Mode B): on each save the dev server pushes
 * a `reload` event; we tear down the loop, mint a FRESH `WasmGame` (same seed) +
 * `createGame`, re-import the author module, and re-run from tick 0 with the new
 * logic. The wasm MODULE stays loaded; only a fresh game instance is made — so a
 * deterministic engine re-derives the same run under the edited rules.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { type Frame, createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
const MAX_STEPS_PER_FRAME = 8;
const TAU = Math.PI * 2;

// --- the canvas2d interpreter for the draw2d command stream ---------------------
// Kinds + payload layout mirror apps/axiom-game-runtime/src/draw2d.rs `draw2d_finish`.
const KIND_RECT = 1;
const KIND_CIRCLE = 2;
const KIND_ELLIPSE = 3;
const KIND_LINE = 4;
const KIND_PARTICLE = 8;

/** A packed `0xRRGGBBAA` value as a CSS `rgba()` string. */
const cssColor = (packed: number): string => {
  const v = packed >>> 0;
  return `rgba(${(v >>> 24) & 0xff},${(v >>> 16) & 0xff},${(v >>> 8) & 0xff},${(v & 0xff) / 255})`;
};

/** Trace `path`, then fill (if the fill is not fully transparent) and stroke (if width > 0). */
const fillStroke = (
  ctx: CanvasRenderingContext2D,
  path: () => void,
  fillRGBA: number,
  strokeRGBA: number,
  strokeWidth: number,
): void => {
  ctx.beginPath();
  path();
  if (fillRGBA !== 0) {
    ctx.fillStyle = cssColor(fillRGBA);
    ctx.fill();
  }
  if (strokeWidth > 0) {
    ctx.strokeStyle = cssColor(strokeRGBA);
    ctx.lineWidth = strokeWidth;
    ctx.stroke();
  }
};

/** Rasterize one finished draw2d command stream onto `ctx`. */
const present = (list: readonly number[], ctx: CanvasRenderingContext2D, width: number, height: number): void => {
  ctx.globalAlpha = 1;
  ctx.fillStyle = "#07090e";
  ctx.fillRect(0, 0, width, height);
  let i = 0;
  while (i < list.length) {
    const kind = list[i];
    const len = list[i + 3];
    const p = list.slice(i + 4, i + 4 + len);
    if (kind === KIND_RECT) {
      ctx.globalAlpha = p[7];
      fillStroke(ctx, () => ctx.rect(p[0], p[1], p[2], p[3]), p[4], p[5], p[6]);
    } else if (kind === KIND_CIRCLE) {
      ctx.globalAlpha = p[6];
      fillStroke(ctx, () => ctx.arc(p[0], p[1], p[2], 0, TAU), p[3], p[4], p[5]);
    } else if (kind === KIND_ELLIPSE) {
      ctx.globalAlpha = p[8];
      fillStroke(ctx, () => ctx.ellipse(p[0], p[1], p[2], p[3], p[4], 0, TAU), p[5], p[6], p[7]);
    } else if (kind === KIND_LINE) {
      ctx.globalAlpha = p[6];
      ctx.strokeStyle = cssColor(p[4]);
      ctx.lineWidth = p[5] || 1;
      ctx.beginPath();
      ctx.moveTo(p[0], p[1]);
      ctx.lineTo(p[2], p[3]);
      ctx.stroke();
    } else if (kind === KIND_PARTICLE) {
      ctx.globalAlpha = p[4];
      ctx.fillStyle = cssColor(p[3]);
      ctx.fillRect(p[0] - p[2] / 2, p[1] - p[2] / 2, p[2], p[2]);
    }
    i += 4 + len;
  }
  ctx.globalAlpha = 1;
};

const boot_ = async (): Promise<void> => {
  const canvas = document.getElementById("c") as HTMLCanvasElement;
  const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
  const status = document.getElementById("status") as HTMLSpanElement;

  // Load the wasm engine MODULE once; each (re)load mints a fresh game instance.
  await initWasm();
  const fixedStepNanos = BigInt(Math.round(NANOS_PER_SECOND / FIXED_HZ));

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(fixedStepNanos, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: "c" });
    // The author module registers its onFixedUpdate / onRender into the active
    // (this game's) registry as an import side effect.
    await import(`/dist/game.js?v=${version}`);
    // Register the presenter LAST so it runs after the author's draws, drains the
    // draw2d builder via `finish()`, and rasterizes the result.
    onRender((frame: Frame): void => {
      present(frame.finish(), ctx, canvas.width, canvas.height);
      status.textContent = `live · deterministic re-run · loop tick ${frame.tick}`;
    });
    app.start();
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, { canvas });
  };
  await load(0);

  // Each save → fresh deterministic run with the new author logic.
  const events = new EventSource("/events");
  events.addEventListener("reload", (event: MessageEvent<string>): void => {
    void load(Number(event.data)).then((): void => {
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  });
};

void boot_();
