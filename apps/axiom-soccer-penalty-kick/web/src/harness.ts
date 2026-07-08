/*
 * The browser boot harness for the TS-only soccer penalty app — the host /
 * platform edge (NOT engine spine), so it lives in the app `web/` dir, outside the
 * branchless + coverage gates, and uses ordinary control flow.
 *
 * It drives the real engine end to end: `createGame()` mints the per-game registry
 * the author module (`game.ts`) registers its `onFixedUpdate` into as an import
 * side effect; the SDK's `boot()` installs the host channel, builds the
 * deterministic `GameLoop` over the wasm `NativeBridge`, wires DOM input, drives
 * `requestAnimationFrame`, and — because we pass `present3d` — binds the live
 * WebGPU/WebGL2/Canvas2D surface and presents the authored 3D diorama every frame.
 * The only thing this harness adds is the DOM HUD, updated each frame from the
 * game's exported `readHud()` via a registered `onRender` (a 3D game draws nothing
 * through the render hook, so we reuse it for the HUD + reticle + banner).
 *
 * Hot reload (Mode B): each save recompiles with tsgo and pushes a `reload` SSE
 * event; we tear down the loop, mint a fresh `WasmGame` (same seed) + `createGame`,
 * re-import the author module, and re-run from tick 0. The wasm module stays loaded.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { type Frame, createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
// A plain number (not BigInt): the wasm constructor takes the step as f64 so the
// Binaryen wasm2js fallback (no i64 ABI) runs.
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 4096;
const CANVAS_ID = "axiom-canvas";

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly score: number;
  readonly roundCurrent: number;
  readonly roundTotal: number;
  readonly best: number;
  readonly powerFill: number;
  readonly powerLabel: string;
  readonly reticleX: number;
  readonly reticleY: number;
  readonly instruction: string;
  readonly result: string | null;
  readonly resultDetail: string | null;
  readonly award: string | null;
  readonly prompt: string | null;
  readonly banner: string | null;
  readonly sessionComplete: boolean;
}

interface SoccerModule {
  readonly readHud: () => Hud;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

const boot_ = async (): Promise<void> => {
  const canvas = el("axiom-canvas") as HTMLCanvasElement;
  const status = el("status");
  const stage = el("stage");
  const reticle = el("reticle");
  const banner = el("banner");
  const bannerText = el("banner-text");
  const bannerSub = el("banner-sub");
  const fields = {
    score: el("score"),
    round: el("round"),
    best: el("best"),
    powerLabel: el("power-label"),
    powerFill: el("power-fill"),
    instruction: el("instruction"),
    prompt: el("prompt"),
  };

  await initWasm();

  const updateHud = (hud: Hud, tick: number): void => {
    fields.score.textContent = String(hud.score);
    fields.round.textContent = `${hud.roundCurrent} / ${hud.roundTotal}`;
    fields.best.textContent = String(hud.best);
    fields.powerLabel.textContent = hud.powerLabel;
    fields.powerFill.style.width = `${Math.round(hud.powerFill * 100)}%`;
    fields.instruction.textContent = hud.instruction;
    fields.prompt.textContent = hud.prompt ?? "";

    // Reticle over the goal (fraction of the stage box).
    reticle.style.left = `${hud.reticleX * 100}%`;
    reticle.style.top = `${hud.reticleY * 100}%`;
    reticle.style.opacity = hud.result || hud.sessionComplete ? "0" : "0.9";

    // Result / session banner.
    const showBanner = hud.sessionComplete ? "FINAL SCORE" : hud.result;
    if (showBanner) {
      bannerText.textContent = showBanner;
      bannerSub.textContent = hud.sessionComplete ? String(hud.best) : [hud.resultDetail, hud.award].filter(Boolean).join("  ");
      banner.classList.add("show");
    } else {
      banner.classList.remove("show");
    }

    status.textContent = `live · tick ${tick}`;
    status.className = "ok";
  };
  // The stage sizes the reticle overlay to the canvas box.
  void stage;

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    const mod = (await import(`/dist/game.js?v=${version}`)) as SoccerModule;

    onRender((frame: Frame): void => updateHud(mod.readHud(), frame.tick));

    app.start();
    // The wasm-bindgen `WasmGame` satisfies every boot seam structurally; the cast
    // bridges a `string[]`-vs-`readonly string[]` nominal mismatch on the glue.
    //
    // frameLocked: one sim tick per displayed frame. This also guarantees the first
    // frame ADVANCES (building the whole scene, incl. every material) BEFORE the 3D
    // surface binds — the engine snapshots the material bind-group set once at bind
    // and (unlike meshes) never re-uploads it, so a scene whose materials are all
    // registered on tick 1 must have that tick run before the bind. Real-time pacing
    // can bind on a 0-advance first frame, leaving late-registered materials
    // unuploaded (their draws are silently skipped).
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, {
      canvas,
      frameLocked: true,
      present3d: { maxInstances: MAX_INSTANCES },
    });
  };

  await load(0);

  const events = new EventSource("/events");
  events.addEventListener("reload", (event: MessageEvent<string>): void => {
    void load(Number(event.data)).then((): void => {
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  });
};

void boot_();
