/*
 * The browser boot harness for the Three-Point Shootout — the host / platform
 * edge of a FULLY SELF-CONTAINED app: it boots the app's own pure-TypeScript
 * engine (`web/src/engine/` — WebGL2 renderer, fixed-step loop, input, WebAudio)
 * with no external SDK and no wasm. It wires the pieces together (renderer →
 * input → game → loop) and drives the DOM HUD: score / streak, rack + ball pips,
 * the shot meter with its ideal-window band, floating shot feedback, the
 * rack-transition banner, the pointer-lock / touch cue, and the results overlay
 * — all from the game's `readHud()`.
 *
 * The two dev-server couplings (the versioned hot-reload import and the
 * `/events` SSE channel) are the anchors the single-file packager rewrites for
 * the static gallery build — keep them verbatim.
 */

import { initRenderer, renderScene } from "./engine/renderer.ts";
import { startLoop } from "./engine/loop.ts";
import { InputState, attachDomInput } from "./engine/input.ts";
import { BALLS_PER_RACK, FIXED_HZ, RACK_COUNT, SHOT_TUNING } from "./constants.ts";

const MAX_STEPS_PER_FRAME = 8;
const CANVAS_ID = "axiom-canvas";

interface Feedback {
  readonly kind: string;
  readonly text: string;
  readonly big: boolean;
}

interface Results {
  readonly score: number;
  readonly makes: number;
  readonly bestStreak: number;
  readonly label: string;
}

interface ReticleView {
  readonly mode: "hidden" | "active" | "dim";
}

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly phase: string;
  readonly score: number;
  readonly streak: number;
  readonly rackIndex: number;
  readonly ballsLeft: number;
  readonly golden: boolean;
  readonly motion: number;
  readonly atTop: boolean;
  readonly reticle: ReticleView;
  readonly movingToLabel: string | undefined;
  readonly results: Results | undefined;
  readonly events: readonly Feedback[];
}

interface GameModule {
  readonly initGame: (input: InputState) => void;
  readonly updateGame: (input: InputState, tick: number) => void;
  readonly readHud: () => Hud;
  readonly configureViewport: (width: number, height: number) => void;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

/** A clamped 0..1 value as a CSS percentage (for meter fills). */
const pct = (v: number): string => `${Math.round(Math.max(0, Math.min(1, v)) * 100)}%`;

const boot_ = async (): Promise<void> => {
  const canvas = el(CANVAS_ID) as HTMLCanvasElement;
  const floaters = el("floaters");
  const reticle = el("reticle");
  const power = el("power");
  const powerFill = el("power-fill");
  const powerZone = el("power-zone");
  const moving = el("moving");
  const lockCue = el("lock-cue");
  const resultsEl = el("results");
  const pips = Array.from({ length: BALLS_PER_RACK }, (_, i) => el(`pip-${i}`));
  const fields = {
    rack: el("rack"),
    resLabel: el("res-label"),
    resMakes: el("res-makes"),
    resScore: el("res-score"),
    resStreak: el("res-streak"),
    score: el("score"),
    streak: el("streak"),
  };

  // The ideal-release band is static: position it once from the tuning constants.
  powerZone.style.left = pct(SHOT_TUNING.idealWindowStart);
  powerZone.style.width = pct(SHOT_TUNING.idealWindowEnd - SHOT_TUNING.idealWindowStart);

  let floaterSeq = 0;
  const spawnFloater = (fb: Feedback): void => {
    const node = document.createElement("div");
    node.className = `floater ${fb.kind}${fb.big ? " big" : ""}`;
    node.textContent = fb.text;
    const spread = ((floaterSeq % 5) - 2) * 44;
    floaterSeq += 1;
    node.style.marginLeft = `${spread}px`;
    floaters.append(node);
    globalThis.setTimeout((): void => node.remove(), 1200);
  };

  let scorePopTimer = 0;
  let lastScore = 0;

  // Desktop: the cue asks for pointer lock. Touch (no pointer lock exists): the
  // cue is a one-time instruction splash, dismissed by the first touch.
  const coarsePointer = globalThis.matchMedia?.("(pointer: coarse)").matches ?? false;
  let pointerLocked = false;
  let touched = false;
  document.addEventListener("pointerlockchange", (): void => {
    pointerLocked = document.pointerLockElement === canvas;
  });
  canvas.addEventListener("pointerdown", (): void => {
    touched = true;
  });

  const updateHud = (hud: Hud): void => {
    fields.score.textContent = String(hud.score);
    if (hud.score !== lastScore) {
      lastScore = hud.score;
      fields.score.classList.add("pop");
      globalThis.clearTimeout(scorePopTimer);
      scorePopTimer = globalThis.setTimeout((): void => fields.score.classList.remove("pop"), 260);
    }
    fields.streak.textContent = `STREAK ${hud.streak}`;
    fields.streak.classList.toggle("hot", hud.streak >= 3);

    fields.rack.textContent = `RACK ${hud.rackIndex + 1}/${RACK_COUNT}`;
    const spent = BALLS_PER_RACK - hud.ballsLeft;
    for (let i = 0; i < pips.length; i += 1) {
      pips[i]!.classList.toggle("spent", i < spent);
    }

    const inMotion = hud.motion >= 0;
    power.classList.toggle("on", inMotion);
    power.classList.toggle("maxed", hud.atTop);
    if (inMotion) {
      powerFill.style.width = pct(hud.motion);
    }
    // The reticle is a FIXED center crosshair on the player's own aim line —
    // the game only sets its visibility, never its position.
    reticle.classList.toggle("active", hud.reticle.mode === "active");
    reticle.classList.toggle("dim", hud.reticle.mode === "dim");
    reticle.classList.toggle("hidden", hud.reticle.mode === "hidden");

    moving.classList.toggle("on", hud.movingToLabel !== undefined);
    if (hud.movingToLabel !== undefined) {
      moving.textContent = `NEXT UP: ${hud.movingToLabel}`;
    }

    const over = hud.results !== undefined;
    resultsEl.classList.toggle("show", over);
    if (over) {
      const r = hud.results!;
      fields.resScore.textContent = String(r.score);
      fields.resMakes.textContent = `${r.makes}/15`;
      fields.resStreak.textContent = String(r.bestStreak);
      fields.resLabel.textContent = r.label;
    }

    lockCue.classList.toggle("on", !over && (coarsePointer ? !touched : !pointerLocked));

    for (const fb of hud.events) {
      spawnFloater(fb);
    }
  };

  initRenderer(canvas);

  let stopLoop: (() => void) | undefined;
  let detachInput: (() => void) | undefined;
  let applyViewport: () => void = () => {};
  globalThis.addEventListener("resize", (): void => applyViewport());

  const load = async (version: number): Promise<void> => {
    stopLoop?.();
    detachInput?.();
    const input = new InputState();
    detachInput = attachDomInput(input, canvas);
    const mod = (await import(`/dist/game.js?v=${version}`)) as GameModule;

    // Touch gestures project against the DISPLAYED canvas size (CSS px), which
    // shrinks on mobile — keep the game's viewport in sync.
    applyViewport = (): void => mod.configureViewport(canvas.clientWidth || canvas.width, canvas.clientHeight || canvas.height);
    applyViewport();

    mod.initGame(input);
    stopLoop = startLoop({
      fixedHz: FIXED_HZ,
      maxCatchUpSteps: MAX_STEPS_PER_FRAME,
      render: (): void => {
        renderScene();
        updateHud(mod.readHud());
      },
      update: (tick: number): void => {
        input.beginTick();
        mod.updateGame(input, tick);
      },
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
