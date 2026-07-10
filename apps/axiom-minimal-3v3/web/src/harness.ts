/*
 * The browser boot harness for Minimal 3v3 Basketball — the host / platform edge
 * (NOT engine spine), so it lives in the app `web/` dir, outside the branchless +
 * coverage gates, and uses ordinary control flow. `createGame()` mints the per-game
 * registry the author module (`game.ts`) registers its `onFixedUpdate` into; the
 * SDK's `boot()` builds the deterministic loop over the wasm bridge, wires DOM
 * keyboard input, and presents the authored 3D half-court every frame. This harness
 * adds the DOM HUD: possession line, the big result banner, the release-timing tag,
 * and the running make count — all driven from the game's `readHud()`.
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
  readonly phase: "playing" | "shooting" | "shotResult" | "turnoverResult";
  readonly possession: string;
  readonly result: "made" | "miss" | "stolen" | "intercepted" | undefined;
  readonly timing: "early" | "good" | "perfect" | "late" | undefined;
  readonly makes: number;
  readonly attempts: number;
}

interface GameModule {
  readonly readHud: () => Hud;
}

const RESULT_TEXT: Record<NonNullable<Hud["result"]>, string> = {
  intercepted: "INTERCEPTED",
  made: "BUCKET!",
  miss: "MISS",
  stolen: "STOLEN",
};

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

// The Canvas2D software backend logs verbose stats + profile lines EVERY frame
// (~60/sec each) — pure canvas2d-only overhead the browser retains. Drop just
// those per-frame lines here at the platform edge; one-time engine logs still show.
const passthroughLog = console.log.bind(console);
console.log = (...args: unknown[]): void => {
  const first = args[0];
  if (typeof first === "string" && first.startsWith("axiom-canvas2d")) {
    return;
  }
  passthroughLog(...args);
};

const boot_ = async (): Promise<void> => {
  const canvas = el(CANVAS_ID) as HTMLCanvasElement;
  const possession = el("possession");
  const result = el("result");
  const timing = el("timing");
  const score = el("score");

  await initWasm();

  const updateHud = (hud: Hud): void => {
    possession.textContent = hud.possession;
    score.textContent = `MAKES ${hud.makes} / ${hud.attempts}`;

    const showResult = hud.result !== undefined;
    result.classList.toggle("show", showResult);
    if (showResult) {
      result.textContent = RESULT_TEXT[hud.result!];
      result.dataset["kind"] = hud.result!;
    }

    const showTiming = hud.timing !== undefined && (hud.phase === "shotResult" || hud.phase === "shooting");
    timing.classList.toggle("show", showTiming);
    if (hud.timing !== undefined) {
      timing.textContent = hud.timing.toUpperCase();
      timing.dataset["tag"] = hud.timing;
    }
  };

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    const mod = (await import(`/dist/game.js?v=${version}`)) as GameModule;

    onRender((): void => updateHud(mod.readHud()));

    app.start();
    // frameLocked: one sim tick per displayed frame, so the first frame builds the
    // whole scene (registering every material) BEFORE the 3D surface binds.
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, {
      canvas,
      frameLocked: true,
      present3d: { maxInstances: MAX_INSTANCES },
    });
  };

  await load(0);

  const isDev = location.hostname === "localhost" || location.hostname === "127.0.0.1";
  if (isDev) {
    const events = new EventSource("/events");
    events.addEventListener("reload", (event: MessageEvent<string>): void => {
      void load(Number(event.data));
    });
  }
};

void boot_();
