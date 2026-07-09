/*
 * The browser boot harness for Swipe Basketball — the host / platform edge (NOT
 * engine spine), so it lives in the app `web/` dir, outside the branchless +
 * coverage gates, and uses ordinary control flow. A near-clone of the soccer
 * harness: `createGame()` mints the per-game registry the author module (`game.ts`)
 * registers its `onFixedUpdate` into as an import side effect; the SDK's `boot()`
 * installs the host channel, builds the deterministic loop over the wasm bridge,
 * wires DOM input, drives `requestAnimationFrame`, and — because we pass `present3d`
 * — binds the live surface and presents the authored 3D machine every frame. The
 * only thing this harness adds is the DOM HUD, updated each frame from the game's
 * exported `readHud()` via a registered `onRender`.
 *
 * The three dev-server couplings (the wasm init call, the versioned hot-reload
 * import, and the `/events` SSE channel) are the anchors the single-file packager
 * rewrites for the static gallery build — keep them verbatim.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 4096;
const CANVAS_ID = "axiom-canvas";

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly title: string;
  readonly instruction: string;
  readonly score: number;
  readonly shots: number;
  readonly scorePop: boolean;
}

interface SwipeModule {
  readonly readHud: () => Hud;
  readonly configureViewport: (width: number, height: number) => void;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

const boot_ = async (): Promise<void> => {
  const canvas = el("axiom-canvas") as HTMLCanvasElement;
  const fields = {
    instruction: el("instruction"),
    score: el("score"),
    shots: el("shots"),
  };

  await initWasm();

  const updateHud = (hud: Hud): void => {
    fields.score.textContent = String(hud.score);
    fields.shots.textContent = String(hud.shots);
    fields.instruction.textContent = hud.instruction;
    fields.score.classList.toggle("pop", hud.scorePop);
  };

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    const mod = (await import(`/dist/game.js?v=${version}`)) as SwipeModule;
    mod.configureViewport(canvas.width, canvas.height);

    onRender((): void => updateHud(mod.readHud()));

    app.start();
    // frameLocked: one sim tick per displayed frame, so the first frame builds the
    // whole scene (registering every material) BEFORE the 3D surface binds — the
    // engine snapshots the material bind-group set once at bind and never re-uploads
    // it, so late-registered materials would otherwise be silently skipped.
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, {
      canvas,
      frameLocked: true,
      present3d: { maxInstances: MAX_INSTANCES },
    });
  };

  await load(0);

  // Live hot-reload is a dev-server convenience only; skip it on a static host
  // (GitHub Pages / file://) where the /events SSE endpoint does not exist.
  const isDev = location.hostname === "localhost" || location.hostname === "127.0.0.1";
  if (isDev) {
    const events = new EventSource("/events");
    events.addEventListener("reload", (event: MessageEvent<string>): void => {
      void load(Number(event.data));
    });
  }
};

void boot_();
