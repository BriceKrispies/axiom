/*
 * The browser boot harness — the host / platform edge (NOT engine spine), so it lives
 * in the app `web/` dir, outside the branchless + coverage gates, and uses ordinary
 * control flow. The turnkey `bootHotApp({ present: "2d" })` does all the wiring: load
 * the wasm once, construct the engine, bind the live 2D surface, bake + upload the
 * monospace font atlas, and run the `defineApp` manifest's fixed-update + render
 * systems every frame through the real WebGPU → WebGL2 → Canvas2D presenter.
 *
 * The single static-build seam is `loadWasm`: the dev server fetches the wasm
 * normally, while the self-contained gallery packager rewrites this one line to feed
 * the embedded bytes (see scripts/package_signal_runner_singlefile.mjs).
 */

import { type HotGame, bootHotApp } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import manifest from "./app.ts";

type WasmGameCtor = new (fixedStepNanos: number, maxSteps: number) => HotGame;

/** Load + instantiate the wasm runtime (the packager rewrites this for static builds). */
const loadWasm = (): Promise<unknown> => initWasm();

await bootHotApp({
  WasmGame: WasmGame as unknown as WasmGameCtor,
  canvasId: "c",
  initWasm: loadWasm,
  manifest,
  present: "2d",
});
