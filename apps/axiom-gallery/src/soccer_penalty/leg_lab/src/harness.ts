/*
 * The browser boot harness for the TypeScript leg lab — the host / platform edge.
 * Modeled on `apps/axiom-retro-fps-ts-browser/web/src/harness.ts` but trimmed: no
 * hot-reload, no input, just boot the 3D present loop and mirror the gait's debug
 * read-out into the DOM HUD.
 *
 * It drives the real engine end to end:
 *   - `createGame()` mints the per-game registry the author module (`game.ts`)
 *     registers its `onFixedUpdate` into as an import side effect;
 *   - the SDK's `boot()` installs the host channel, builds the deterministic
 *     `GameLoop` over the wasm `NativeBridge`, drives `requestAnimationFrame`, and
 *     — because we pass `present3d` — binds the live WebGPU/WebGL2/Canvas2D surface
 *     and presents the authored 3D scene every frame;
 *   - the only thing this harness adds is the DOM HUD, updated from the game's
 *     exported `readDebug()` via a registered `onRender` (a 3D game draws nothing
 *     through the render hook, so we reuse it for the HUD).
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { type Frame, createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
// A plain `number` (not BigInt): the wasm constructor takes the step as f64 so the
// Binaryen wasm2js fallback (no i64 ABI) runs.
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 64;
const CANVAS_ID = "axiom-leg-lab-canvas";
const CANVAS_W = 960;
const CANVAS_H = 600;

/** The debug snapshot the game module exposes each frame. */
interface Debug {
  readonly tick: number;
  readonly phase: string;
  readonly planted: boolean;
  readonly kneeDeg: number;
  readonly line: string;
}

interface LegLabModule {
  readonly readDebug: () => Debug;
}

const boot_ = async (): Promise<void> => {
  const canvas = document.getElementById(CANVAS_ID) as HTMLCanvasElement;
  const status = document.getElementById("status") as HTMLElement;
  const fields = {
    tick: document.getElementById("tick") as HTMLElement,
    phase: document.getElementById("phase") as HTMLElement,
    knee: document.getElementById("knee") as HTMLElement,
  };

  // Load the wasm engine MODULE once; each (re)load mints a fresh game instance.
  await initWasm();

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    // Reset the canvas backing store to full size before each (re)bind (a previous
    // 3D session shrinks it to the render resolution). The page's CSS `!important`
    // keeps the DISPLAY size fixed regardless of the inline size the backend sets.
    canvas.width = CANVAS_W;
    canvas.height = CANVAS_H;
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });

    // Importing the author module registers its onFixedUpdate into THIS game's
    // fresh registry (import side effect), so it must come after `createGame`. The
    // `?v=` cache-bust re-imports a fresh module each reload; serve.py version-stamps
    // its relative imports too, so a change to ANY leg-lab module takes effect.
    const mod = (await import(`/dist/game.js?v=${version}`)) as LegLabModule;

    onRender((frame: Frame): void => {
      const d = mod.readDebug();
      fields.tick.textContent = String(frame.tick);
      fields.phase.textContent = d.phase;
      fields.phase.className = d.planted ? "planted" : "swing";
      fields.knee.textContent = `${d.kneeDeg}deg`;
      status.textContent = `live · TypeScript on @axiom/game · tick ${frame.tick}`;
      status.className = "ok";
    });

    app.start();
    // The wasm-bindgen `WasmGame` satisfies every boot seam structurally; the cast
    // bridges a `string[]`-vs-`readonly string[]` nominal mismatch on the glue.
    // Real-time pacing (no frameLocked): the engine's 3D present re-uploads the mesh
    // set whenever it changes (a mesh-set generation, the peer of the 2D texture
    // generation), so the leg's meshes reach the GPU even though they are registered
    // on tick 1 — after the surface binds with the engine's demo scene.
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, {
      canvas,
      present3d: { maxInstances: MAX_INSTANCES },
    });
  };

  await load(0);

  // Hot reload: each save recompiles (serve.py) and pushes a `reload` event; tear
  // down the loop, mint a fresh game, re-import the author module, re-run from tick 0.
  const events = new EventSource("/events");
  events.addEventListener("reload", (event: MessageEvent<string>): void => {
    void load(Number(event.data)).then((): void => {
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  });
};

void boot_();
