/*
 * The browser boot harness for the 2D @axiom/game hot-reload demo — now the turnkey
 * shape. The shared SDK helper `bootHotApp` does ALL the wiring (load wasm once,
 * construct the engine once, bind the 2D surface, bake + upload the font atlas, stream
 * sprite textures, build the long-lived `HotRuntime`), so this harness is just:
 *   1. call `bootHotApp({ present: "2d", … })`;
 *   2. accept `./game.ts` HMR updates into the running engine (one line — the accept
 *      MUST live here because Vite resolves its dep path relative to this module).
 * A DOM status line rides the optional `onFrame` hook. Host/platform edge — app tier,
 * outside the engine gates.
 */

import { type AppManifest, type HotGame, bootHotApp } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import manifest from "./game.ts";

type WasmGameCtor = new (fixedStepNanos: number, maxSteps: number) => HotGame;

const status = document.getElementById("status") as HTMLSpanElement;

const runtime = await bootHotApp({
  WasmGame: WasmGame as unknown as WasmGameCtor,
  canvasId: "c",
  initWasm,
  manifest,
  onFrame: (game): void => {
    status.textContent = `live · engine alive · tick ${(game as unknown as { current_tick: number }).current_tick}`;
  },
  present: "2d",
});

if (import.meta.hot) {
  import.meta.hot.accept("./game.ts", (updated): void => {
    const mod = updated as { readonly default: AppManifest } | undefined;
    if (mod) {
      runtime.apply(mod.default);
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    }
  });
}
