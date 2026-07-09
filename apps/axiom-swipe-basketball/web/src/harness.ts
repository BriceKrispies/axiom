/*
 * The browser boot harness for Swipe Basketball — the host / platform edge (NOT
 * engine spine), so it lives in the app `web/` dir, outside the branchless +
 * coverage gates, and uses ordinary control flow. `createGame()` mints the per-game
 * registry the author module (`game.ts`) registers its `onFixedUpdate` into; the
 * SDK's `boot()` builds the deterministic loop over the wasm bridge, wires DOM
 * input, and presents the authored 3D machine every frame. This harness adds the
 * DOM HUD: the round clock, score / best / streak, the game-over overlay, and the
 * floating arcade feedback text — all driven from the game's `readHud()`.
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

interface Feedback {
  readonly kind: string;
  readonly text: string;
  readonly big: boolean;
}

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly phase: "ready" | "playing" | "gameover";
  readonly score: number;
  readonly best: number;
  readonly time: number;
  readonly streak: number;
  readonly multiplier: number;
  readonly finalWindow: boolean;
  readonly scorePop: boolean;
  readonly events: readonly Feedback[];
}

interface SwipeModule {
  readonly readHud: () => Hud;
  readonly configureViewport: (width: number, height: number) => void;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

const boot_ = async (): Promise<void> => {
  const canvas = el("axiom-canvas") as HTMLCanvasElement;
  const flash = el("flash");
  const floaters = el("floaters");
  const gameover = el("gameover");
  const fields = {
    best: el("best"),
    goBest: el("go-best"),
    goScore: el("go-score"),
    mult: el("mult"),
    score: el("score"),
    time: el("time"),
  };

  await initWasm();

  let flashTimer = 0;
  const popFlash = (big: boolean): void => {
    flash.classList.add("on");
    flash.classList.toggle("big", big);
    globalThis.clearTimeout(flashTimer);
    flashTimer = globalThis.setTimeout((): void => flash.classList.remove("on", "big"), big ? 200 : 130);
  };

  let floaterSeq = 0;
  const spawnFloater = (fb: Feedback): void => {
    const node = document.createElement("div");
    node.className = `floater ${fb.kind}${fb.big ? " big" : ""}`;
    node.textContent = fb.text;
    // Fan successive floaters out horizontally so they don't overlap.
    const spread = ((floaterSeq % 5) - 2) * 46;
    floaterSeq += 1;
    node.style.marginLeft = `${spread}px`;
    floaters.append(node);
    globalThis.setTimeout((): void => node.remove(), 1200);
  };

  const updateHud = (hud: Hud): void => {
    fields.time.textContent = hud.time.toFixed(1);
    fields.time.classList.toggle("final", hud.finalWindow);
    fields.score.textContent = String(hud.score);
    fields.score.classList.toggle("pop", hud.scorePop);
    fields.best.textContent = String(hud.best);
    fields.mult.innerHTML = `${hud.multiplier}&times;`;
    fields.mult.classList.toggle("up", hud.multiplier > 1);

    const over = hud.phase === "gameover";
    gameover.classList.toggle("show", over);
    if (over) {
      fields.goScore.textContent = String(hud.score);
      fields.goBest.textContent = String(hud.best);
      fields.goBest.classList.toggle("record", hud.best > 0 && hud.best === hud.score);
    }

    for (const fb of hud.events) {
      spawnFloater(fb);
      if (fb.big) {
        popFlash(true);
      } else if (fb.kind !== "miss") {
        popFlash(false);
      }
    }
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
