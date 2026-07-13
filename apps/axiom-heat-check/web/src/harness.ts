/*
 * The browser boot harness for Heat Check — the host / platform edge (NOT engine
 * spine), so it lives in the app `web/` dir, outside the branchless + coverage gates,
 * and uses ordinary control flow. `createGame()` mints the per-game registry the
 * author module (`game.ts`) registers its `onFixedUpdate` into; the SDK's `boot()`
 * builds the deterministic loop over the wasm bridge, wires DOM input, and presents
 * the authored 3D half-court every frame. This harness adds the DOM HUD: the round
 * clock, score / best / streak, the segmented heat meter, the ready + game-over
 * overlays, and the floating feedback text — all driven from the game's `readHud()`.
 *
 * The three dev-server couplings (the wasm init call, the versioned hot-reload import,
 * and the `/events` SSE channel) are the anchors the single-file packager rewrites for
 * the static gallery build — keep them verbatim.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import { STICK_RADIUS } from "./constants.ts";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 4096;
const CANVAS_ID = "axiom-canvas";
const HEAT_PIPS = 5;

interface Feedback {
  readonly kind: string;
  readonly text: string;
  readonly big: boolean;
}

/** The three live readiness tags the meter renders while holding. */
interface Readiness {
  readonly space: "smothered" | "contested" | "open" | "broken";
  readonly rhythm: "early" | "good" | "perfect" | "late";
  readonly balance: "moving" | "set" | "planted";
  readonly quality: number;
}

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly phase: "ready" | "playing" | "shooting" | "scoredFeedback" | "gameOver";
  readonly score: number;
  readonly best: number;
  readonly time: number;
  readonly streak: number;
  readonly multiplier: number;
  readonly heat: number;
  readonly finalWindow: boolean;
  readonly scorePop: boolean;
  readonly readiness: Readiness | undefined;
  readonly events: readonly Feedback[];
}

interface HeatModule {
  readonly readHud: () => Hud;
  readonly setStick: (x: number, holding: boolean) => void;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

/** A clamped 0..1 value as a CSS percentage (for meter fills). */
const pct = (v: number): string => `${Math.round(Math.max(0, Math.min(1, v)) * 100)}%`;

// The Canvas2D software backend logs a verbose stats line EVERY frame (~60/sec). Over a
// 60-second round that's ~3600 lines the browser retains — pure canvas2d-only overhead
// that drags the page down as the round goes on (and is brutal with devtools open). Drop
// just that per-frame line here at the platform edge; one-time engine logs still show.
const passthroughLog = console.log.bind(console);
console.log = (...args: unknown[]): void => {
  const first = args[0];
  if (typeof first === "string" && first.startsWith("axiom-canvas2d:")) {
    return;
  }
  passthroughLog(...args);
};

const boot_ = async (): Promise<void> => {
  const canvas = el(CANVAS_ID) as HTMLCanvasElement;
  const flash = el("flash");
  const floaters = el("floaters");
  const ready = el("ready");
  const gameover = el("gameover");
  const heat = el("heat");
  const pips = Array.from({ length: HEAT_PIPS }, (_, i) => el(`pip-${i}`));
  const pad = el("pad");
  const stickEl = el("stick");
  const stickBase = el("stick-base");
  const stickKnob = el("stick-knob");
  const readinessEl = el("readiness");
  const rdFill = el("rd-fill");
  const rdTags = {
    balance: el("rd-balance"),
    rhythm: el("rd-rhythm"),
    space: el("rd-space"),
  };
  const SPACE_LABEL: Record<Readiness["space"], string> = {
    broken: "BROKEN",
    contested: "CONTESTED",
    open: "OPEN",
    smothered: "SMOTHERED",
  };
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
    flashTimer = globalThis.setTimeout((): void => flash.classList.remove("on", "big"), big ? 220 : 130);
  };

  let floaterSeq = 0;
  const spawnFloater = (fb: Feedback): void => {
    const node = document.createElement("div");
    node.className = `floater ${fb.kind}${fb.big ? " big" : ""}`;
    node.textContent = fb.text;
    const spread = ((floaterSeq % 5) - 2) * 48;
    floaterSeq += 1;
    node.style.marginLeft = `${spread}px`;
    floaters.append(node);
    globalThis.setTimeout((): void => node.remove(), 1200);
  };

  // The on-screen joystick, mounted in the #pad BELOW the game. Pressing anywhere in the
  // pad drops a floating anchor; horizontal knob displacement (clamped to STICK_RADIUS)
  // is the movement intent, and lifting is the shoot edge. `control` forwards the value
  // into the game module (set once it loads); until then it's a no-op.
  let control: (x: number, holding: boolean) => void = () => {};
  let stickActive = false;
  let anchorX = 0;
  let anchorY = 0;
  const KNOB = 46;
  const placeStick = (kx: number, ky: number): void => {
    const d = 2 * STICK_RADIUS;
    stickBase.style.width = `${d}px`;
    stickBase.style.height = `${d}px`;
    stickBase.style.left = `${anchorX - STICK_RADIUS}px`;
    stickBase.style.top = `${anchorY - STICK_RADIUS}px`;
    stickKnob.style.left = `${kx - KNOB / 2}px`;
    stickKnob.style.top = `${ky - KNOB / 2}px`;
  };
  pad.addEventListener("pointerdown", (e: PointerEvent): void => {
    const r = pad.getBoundingClientRect();
    anchorX = e.clientX - r.left;
    anchorY = e.clientY - r.top;
    stickActive = true;
    pad.classList.add("active");
    stickEl.classList.add("on");
    placeStick(anchorX, anchorY);
    control(0, true);
    pad.setPointerCapture?.(e.pointerId);
  });
  pad.addEventListener("pointermove", (e: PointerEvent): void => {
    if (!stickActive) {
      return;
    }
    const r = pad.getBoundingClientRect();
    let dx = e.clientX - r.left - anchorX;
    let dy = e.clientY - r.top - anchorY;
    const d = Math.hypot(dx, dy);
    if (d > STICK_RADIUS) {
      dx = (dx / d) * STICK_RADIUS;
      dy = (dy / d) * STICK_RADIUS;
    }
    placeStick(anchorX + dx, anchorY + dy);
    control(Math.max(-1, Math.min(1, dx / STICK_RADIUS)), true); // horizontal drives play
  });
  const endStick = (): void => {
    stickActive = false;
    pad.classList.remove("active");
    stickEl.classList.remove("on");
    control(0, false);
  };
  pad.addEventListener("pointerup", endStick);
  pad.addEventListener("pointercancel", endStick);

  const updateHud = (hud: Hud): void => {
    fields.time.textContent = hud.time.toFixed(1);
    fields.time.classList.toggle("final", hud.finalWindow);
    fields.score.textContent = String(hud.score);
    fields.score.classList.toggle("pop", hud.scorePop);
    fields.best.textContent = String(hud.best);
    fields.mult.innerHTML = `${hud.multiplier}&times;`;
    fields.mult.classList.toggle("up", hud.multiplier > 1);

    heat.classList.toggle("hot", hud.heat >= 4);
    for (let i = 0; i < pips.length; i += 1) {
      pips[i]!.classList.toggle("on", i < hud.heat);
    }

    ready.classList.toggle("show", hud.phase === "ready");

    const over = hud.phase === "gameOver";
    gameover.classList.toggle("show", over);
    if (over) {
      fields.goScore.textContent = String(hud.score);
      fields.goBest.textContent = String(hud.best);
      fields.goBest.classList.toggle("record", hud.best > 0 && hud.best === hud.score);
    }

    // The pre-release readiness meter: three SEPARATE axes (SPACE / RHYTHM / BALANCE) so
    // the player reads WHY a shot is good — never one "guaranteed make" label. The
    // quality bar is tinted by the SPACE read, and only reads strong when truly beaten.
    const r = hud.readiness;
    readinessEl.classList.toggle("on", r !== undefined);
    if (r !== undefined) {
      rdTags.space.textContent = SPACE_LABEL[r.space];
      rdTags.rhythm.textContent = r.rhythm.toUpperCase();
      rdTags.balance.textContent = r.balance.toUpperCase();
      readinessEl.dataset.space = r.space;
      readinessEl.dataset.rhythm = r.rhythm;
      readinessEl.dataset.balance = r.balance;
      rdFill.style.width = pct(r.quality);
    }

    // One flash only on an EARNED make (OPEN / BROKEN) — never on a miss.
    for (const fb of hud.events) {
      spawnFloater(fb);
      if (fb.big) {
        popFlash(true);
      } else if (fb.kind === "open") {
        popFlash(false);
      }
    }
  };

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    const mod = (await import(`/dist/game.js?v=${version}`)) as HeatModule;
    control = mod.setStick;

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
