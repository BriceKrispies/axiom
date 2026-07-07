/*
 * The browser boot harness for the TS-only retro FPS — HOT-RUNTIME edition.
 *
 * Host / platform edge (NOT engine spine): app `web/` dir, ordinary control flow.
 * It constructs the `WasmGame` ONCE and keeps it alive for the whole session, builds a
 * `HotRuntime` (`createHotRuntime`) over the author's `defineApp` manifest with 3D
 * presentation (`present3d`), and — on every Vite HMR update of `./game.ts` — calls
 * `runtime.apply(next)`, which diffs the new manifest against the live one and applies
 * it to the running engine: a system-body edit hot-patches on the next tick; a scene
 * version bump re-authors the level in place; a component version bump migrates the
 * live component bytes. The engine, canvas, world, and tick counter persist across
 * every edit — `globalThis.__wasmGameConstructCount` stays 1.
 *
 * The DOM HUD rides a separate rAF loop reading the author module's `readHud()`; the
 * `import.meta.hot.accept` handler swaps in the new module's `readHud` too.
 */

import { type AppManifest, type BootGame, createHotRuntime } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import manifest, { readHud } from "./game.ts";

const FIXED_HZ = 60;
const NANOS_PER_SECOND = 1_000_000_000;
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 4096;
const CANVAS_ID = "axiom-canvas";

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly health: number;
  readonly score: number;
  readonly ammo: number;
  readonly enemies: number;
  readonly wave: number;
}

/** The author module's shape: the default-exported manifest plus the live HUD read-out. */
interface RetroFpsModule {
  readonly default: AppManifest;
  readonly readHud: () => Hud;
}

const boot_ = async (): Promise<void> => {
  const canvas = document.getElementById(CANVAS_ID) as HTMLCanvasElement;
  const status = document.getElementById("status") as HTMLSpanElement;
  const fields = {
    ammo: document.getElementById("ammo") as HTMLElement,
    enemies: document.getElementById("enemies") as HTMLElement,
    hp: document.getElementById("hp") as HTMLElement,
    score: document.getElementById("score") as HTMLElement,
  };

  // Load the wasm engine MODULE once, then construct the engine instance ONCE.
  await initWasm();
  const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
  const globals = globalThis as unknown as {
    __wasmGameConstructCount: number;
    __game: WasmGame;
    __lastApply: string;
  };
  globals.__wasmGameConstructCount = 1;
  globals.__game = game;
  globals.__lastApply = "none";

  // The long-lived hot runtime: mount the author manifest, boot once, present the 3D scene.
  // `frameLocked` matches the Rust retro FPS "one sim tick per displayed frame".
  const runtime = createHotRuntime(game as unknown as BootGame, manifest, {
    canvas,
    frameLocked: true,
    onEngineRestart: (): void => {
      // A config change (fixedHz/seed/surface) needs a fresh engine — page reload fallback.
      globalThis.location.reload();
    },
    present3d: { maxInstances: MAX_INSTANCES },
  });

  // The DOM HUD rides its own rAF loop; `hud.read` is swapped on each HMR accept.
  const hud = { read: readHud };
  const paintHud = (): void => {
    const snapshot = hud.read();
    fields.hp.textContent = String(snapshot.health);
    fields.score.textContent = String(snapshot.score);
    fields.ammo.textContent = String(snapshot.ammo);
    fields.enemies.textContent = String(snapshot.enemies);
    status.textContent = `live · engine alive · tick ${game.current_tick} · wave ${snapshot.wave}`;
    status.className = "ok";
    globalThis.requestAnimationFrame(paintHud);
  };
  globalThis.requestAnimationFrame(paintHud);

  // Vite HMR: apply the new manifest into the LIVE engine + swap the HUD read-out.
  if (import.meta.hot) {
    import.meta.hot.accept("./game.ts", (updated): void => {
      const mod = updated as RetroFpsModule | undefined;
      if (!mod) {
        return;
      }
      hud.read = mod.readHud;
      const kind = runtime.apply(mod.default);
      globals.__lastApply = kind;
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
      status.textContent = `hot · ${kind} · tick ${game.current_tick} (engine #${globals.__wasmGameConstructCount})`;
    });
  }
};

void boot_();
