/*
 * game.ts — THE game, wired to the engine. Registering an `onFixedUpdate` as an
 * import side effect, it builds the scene on the first tick, folds this tick's
 * keyboard (+ optional touch pad) into a plain `Intent`, advances the deterministic
 * SDK-free `HomeRunSession`, mirrors the result into the 3D scene, and plays the
 * synthesized audio hooks for contact / home runs / misses. It exports `readHud()`
 * for the harness's DOM overlay and `configure()` for the harness's URL-driven
 * dev/screenshot affordances (seed, freeze-at-tick, scripted autoplay).
 *
 * Controls: A/D (or ←/→) shift the batter · SPACE swings (always full power; the
 * batter re-winds on his own between swings) · SPACE or ENTER restarts once the
 * round is over.
 */

import type { TickInput } from "@axiom/web-engine";
import type { InputState } from "@axiom/web-engine";
import { playTone } from "@axiom/web-engine";
import { SUN_NOON_MS, SUN_START_MS, type SceneHandles, applyFrame, applySun, buildScene } from "./scene.ts";
import type { Feedback, Intent, Outcome, Phase, PitchResult } from "./types.ts";
import { HomeRunSession } from "./session.ts";

/** The HUD snapshot the harness renders each frame. */
export interface Hud {
  readonly phase: Phase;
  readonly score: number;
  readonly pitchNumber: number;
  readonly pitchCount: number;
  readonly homers: number;
  readonly streak: number;
  readonly multiplier: number;
  readonly bestDistance: number;
  readonly lastMph: number;
  readonly lastPitchName: string;
  /** Rewind progress 0…1 — the ready meter (1 = wound and ready to swing). */
  readonly readiness: number;
  readonly ready: boolean;
  readonly results: readonly PitchResult[];
  /** Feedback events to present this frame (center text, flashes, audio). */
  readonly events: readonly Feedback[];
}

const PITCH_COUNT = 10;

let handles: SceneHandles | undefined;
let session = new HomeRunSession(1);

// Harness-provided configuration (URL params), applied before the first tick.
let pendingSeed = 1;
let freezeAtTick = Number.POSITIVE_INFINITY;
let autoStart = false;
let autoSwingAt = -1;
let ticks = 0;

// The optional on-screen touch pad pushes its state here (see harness.ts). A tap
// on the swing button queues one press edge, consumed by the next fixed tick.
let padMoveX = 0;
let padSwingQueued = false;

let prevReady = true;
let hudEvents: Feedback[] = [];

/** Bind the game's actions, build the scene, and start a fresh session. Called
 * once by the harness (and again on hot-reload) before the first `updateGame`. */
export const initGame = (input: InputState): void => {
  input.bindAction("left", ["ArrowLeft", "KeyA"]);
  input.bindAction("right", ["ArrowRight", "KeyD"]);
  input.bindAction("swing", ["Space"]);
  input.bindAction("restart", ["Enter"]);
  handles = buildScene();
  session = new HomeRunSession(pendingSeed);
  ticks = 0;
  prevReady = true;
  padMoveX = 0;
  padSwingQueued = false;
  hudEvents = [];
};

/** Harness affordances: seed + deterministic screenshot/autoplay hooks. */
export const configure = (opts: {
  readonly seed?: number;
  readonly freezeAt?: number;
  readonly autoStart?: boolean;
  readonly swingAt?: number;
}): void => {
  pendingSeed = opts.seed ?? pendingSeed;
  freezeAtTick = opts.freezeAt ?? freezeAtTick;
  autoStart = opts.autoStart ?? autoStart;
  autoSwingAt = opts.swingAt ?? autoSwingAt;
};

/** The harness's touch pad feeds its state here (screen-sign moveX; tap queues a swing). */
export const setPad = (moveX: number, swingTap: boolean): void => {
  padMoveX = moveX;
  padSwingQueued = padSwingQueued || swingTap;
};

/**
 * Fold this tick's keyboard + pad into the session `Intent`. The camera looks
 * downfield so world +X renders to screen-LEFT; we negate the keyboard axis so
 * pressing D/→ moves the batter right ON SCREEN.
 */
const readIntent = (input: TickInput): Intent => {
  // `axis(neg, pos)` shim: the engine's InputState exposes only isDown/pressed.
  const kbAxis = (input.isDown("right") ? 1 : 0) - (input.isDown("left") ? 1 : 0);
  const moveX = padMoveX !== 0 ? -padMoveX : -kbAxis;
  let swing = input.pressed("swing") || padSwingQueued;
  padSwingQueued = false;
  let start = input.pressed("swing") || input.pressed("restart");
  // Scripted autoplay for deterministic screenshots (?swingAt=N presses once).
  if (autoSwingAt >= 0 && ticks === autoSwingAt) {
    swing = true;
  }
  if (autoStart && ticks === 2) {
    start = true;
  }
  return { moveX, start, swing };
};

// ── audio hooks (synthesized; no assets) ──────────────────────────────────────

const toneFor = (kind: Feedback["kind"], big: boolean): void => {
  switch (kind) {
    case "release":
      playTone({ duration: 0.05, freq: 660, volume: 0.12, wave: "square" });
      return;
    case "contact":
      playTone({ duration: 0.07, freq: big ? 220 : 180, volume: 0.5, wave: "square" });
      playTone({ duration: 0.05, freq: big ? 1400 : 900, volume: 0.25, wave: "triangle" });
      return;
    case "homer": {
      // A rising major arpeggio (C–E–G–C), staggered via the engine's tone
      // `delay` so one event plays the whole triumphant flourish.
      const notes = [523, 659, 784, 1047];
      notes.forEach((f, i) => playTone({ delay: i * 0.05, duration: 0.16, freq: f, volume: 0.3, wave: "triangle" }));
      return;
    }
    case "clean":
      playTone({ duration: 0.12, freq: 587, volume: 0.22, wave: "triangle" });
      return;
    case "miss":
      playTone({ duration: 0.12, freq: 110, volume: 0.18, wave: "sawtooth" });
      return;
    case "ball":
      playTone({ duration: 0.1, freq: 300, volume: 0.12, wave: "sine" });
      return;
    case "foul":
      playTone({ duration: 0.08, freq: 240, volume: 0.18, wave: "square" });
      return;
    case "caught":
    case "fielded":
    case "weak":
    case "grounder":
    case "popup":
      playTone({ duration: 0.08, freq: 160, volume: 0.2, wave: "sine" });
      return;
    default:
      return;
  }
};

/** One fixed-step tick: fold input → intent, advance the session, drain audio
 * cues, and mirror the result into the scene. The harness calls this each step
 * after `input.beginTick()`. */
export const updateGame = (input: TickInput): void => {
  if (handles === undefined) {
    return;
  }
  ticks += 1;
  const intent = readIntent(input);
  if (ticks <= freezeAtTick) {
    session.advance(intent);
    for (const event of session.drainEvents()) {
      hudEvents.push(event);
      toneFor(event.kind, event.big);
    }
    // A soft click the instant the batter finishes re-winding (ready to swing).
    const ready = session.swing.state === "ready";
    if (ready && !prevReady) {
      playTone({ duration: 0.05, freq: 880, volume: 0.14, wave: "sine" });
    }
    prevReady = ready;
  }
  applyFrame(handles, session.view());
};

/** Once per RENDERED frame (not per fixed tick): wall-clock presentation work,
 * independent of the game loop — the sun's slow crawl across the sky. Starts at
 * mid-morning and pins to high noon under a ?shot freeze so screenshots stay
 * deterministic. */
export const frameGame = (nowMs: number): void => {
  if (handles === undefined) {
    return;
  }
  const pinned = freezeAtTick !== Number.POSITIVE_INFINITY;
  applySun(handles, pinned ? SUN_NOON_MS : SUN_START_MS + nowMs);
};

/** The HUD the harness reads each frame (draining buffered feedback events). */
export const readHud = (): Hud => {
  const events = hudEvents;
  hudEvents = [];
  return {
    bestDistance: session.bestDistance,
    events,
    homers: session.homers,
    lastMph: session.lastMph,
    lastPitchName: session.lastPitchName,
    multiplier: session.streakMultiplier,
    readiness: session.swing.readiness,
    ready: session.swing.state === "ready",
    phase: session.phase,
    pitchCount: PITCH_COUNT,
    pitchNumber: session.pitchNumber,
    results: session.results,
    score: session.score,
    streak: session.streak,
  };
};

/** Re-exported for the harness's outcome styling. */
export type { Outcome };
