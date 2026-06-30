/*
 * The browser boot harness for the TS-only retro FPS app.
 *
 * This is the host / platform edge — NOT engine spine — so it lives in an app
 * `web/` dir, outside the branchless + coverage gates, and uses ordinary control
 * flow. It is deliberately THIN: unlike the 2D hot-reload harness, there is no
 * canvas2d interpreter here, because the game is 3D and the engine renders it.
 *
 * It drives the real engine path end to end:
 *   - `createGame()` mints the per-game registry the author (`game.ts`) registers
 *     its `onFixedUpdate` into;
 *   - the SDK's own `boot()` aggregator installs the real host channel, builds the
 *     real `GameLoop` over the real wasm `NativeBridge`, wires DOM input, drives
 *     `requestAnimationFrame`, and — because we pass `present3d` — binds the live
 *     wgpu/WebGL2/Canvas2D surface and presents the authored 3D scene every frame;
 *   - the only thing this harness adds is the DOM HUD, updated each frame from the
 *     game's exported `readHud()` via a registered `onRender` (the render callback
 *     is the per-frame hook; a 3D game draws nothing through it, so we reuse it for
 *     the HUD).
 *
 * ## Hot reload (deterministic re-run, Mode B)
 * Each save recompiles with tsgo and pushes a `reload` SSE event. We tear down the
 * loop, mint a fresh `WasmGame` (same seed) + `createGame`, re-import the author
 * module under the new logic, and re-run from tick 0. The wasm module stays loaded.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { type Frame, createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
// A plain `number` (not a BigInt): the wasm `WasmGame` constructor takes the
// fixed step as f64 so the Binaryen `wasm2js` fallback (no i64/BigInt ABI) runs.
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
}

/** The author module's shape: it registers its sim into the active registry as an import side effect and exposes the live HUD. */
interface RetroFpsModule {
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

  // Load the wasm engine MODULE once; each (re)load mints a fresh game instance.
  await initWasm();

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    // The author module registers its onFixedUpdate (sim step + scene authoring +
    // camera) into the active (this game's) registry as an import side effect.
    const mod = (await import(`/dist/game.js?v=${version}`)) as RetroFpsModule;
    // The HUD updater rides the render hook (a 3D game draws nothing through it).
    onRender((frame: Frame): void => {
      const hud = mod.readHud();
      fields.hp.textContent = String(hud.health);
      fields.score.textContent = String(hud.score);
      fields.ammo.textContent = String(hud.ammo);
      fields.enemies.textContent = String(hud.enemies);
      status.textContent = `live · TypeScript-only · tick ${frame.tick}`;
      status.className = "ok";
    });
    app.start();
    // The wasm-bindgen-generated `WasmGame` satisfies every boot seam structurally;
    // the cast bridges a `string[]`-vs-`readonly string[]` nominal mismatch on the
    // generated glue (the same bridge the 2D hot-reload harness uses).
    teardown = boot(
      game as unknown as Parameters<typeof boot>[0],
      app,
      // frameLocked: one sim tick per displayed frame — exact parity with the Rust
      // retro FPS's "step the sim once per frame" loop (no real-time accumulator).
      { canvas, frameLocked: true, present3d: { maxInstances: MAX_INSTANCES } },
    );
  };
  await load(0);

  // Each save → a fresh deterministic run with the new author logic.
  const events = new EventSource("/events");
  events.addEventListener("reload", (event: MessageEvent<string>): void => {
    void load(Number(event.data)).then((): void => {
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  });
};

void boot_();
