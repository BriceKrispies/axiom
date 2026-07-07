/*
 * The browser boot harness for the TS-only retro FPS — turnkey shape. The shared SDK
 * helper `bootHotApp` does all the wiring (load wasm once, construct the engine once,
 * present the 3D scene, build the long-lived `HotRuntime`); this harness only:
 *   1. calls `bootHotApp({ present: "3d", frameLocked: true, … })`;
 *   2. paints the DOM HUD each frame via `onFrame` (reading the author module's
 *      `readHud()`, swapped on each HMR update);
 *   3. accepts `./game.ts` HMR updates into the running engine (one line, here because
 *      Vite resolves the accept's dep path relative to this module).
 * Host/platform edge — app tier, outside the engine gates.
 */

import { type AppManifest, type HotGame, bootHotApp } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import manifest, { readHud } from "./game.ts";

type WasmGameCtor = new (fixedStepNanos: number, maxSteps: number) => HotGame;
type Hud = { readonly health: number; readonly score: number; readonly ammo: number; readonly enemies: number; readonly wave: number };
type RetroFpsModule = { readonly default: AppManifest; readonly readHud: () => Hud };

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;
const status = el("status");
// `hud.read` is swapped to the new module's `readHud` on each HMR accept.
const hud = { read: readHud };
const globals = globalThis as unknown as { __lastApply: string };
globals.__lastApply = "none";

const runtime = await bootHotApp({
  WasmGame: WasmGame as unknown as WasmGameCtor,
  canvasId: "axiom-canvas",
  frameLocked: true,
  initWasm,
  manifest,
  maxInstances: 4096,
  onFrame: (game): void => {
    const snapshot = hud.read();
    el("hp").textContent = String(snapshot.health);
    el("score").textContent = String(snapshot.score);
    el("ammo").textContent = String(snapshot.ammo);
    el("enemies").textContent = String(snapshot.enemies);
    status.textContent = `live · engine alive · tick ${(game as unknown as { current_tick: number }).current_tick} · wave ${snapshot.wave}`;
    status.className = "ok";
  },
  present: "3d",
});

if (import.meta.hot) {
  import.meta.hot.accept("./game.ts", (updated): void => {
    const mod = updated as RetroFpsModule | undefined;
    if (mod) {
      hud.read = mod.readHud;
      globals.__lastApply = runtime.apply(mod.default);
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    }
  });
}
