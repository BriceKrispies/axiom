/*
 * game.ts — Home Run! as a PURE `@axiom/web-engine` game. Everything here is a pure
 * function or plain data: the declared `resources` (named meshes + materials), the
 * key bindings, and `init` / `update` / `view` / `sound`. None of them touches the
 * engine, holds a handle, or mutates anything — the `runGame` shell (wired up in
 * `harness.ts`) owns the loop, the GPU, input and audio, and drives these.
 *
 * `update` folds one tick of input into the next `HomeRunSession` by cloning it and
 * advancing the clone (so the input state is never mutated — a pure step over an
 * immutable state). `view` hands the session's read-only snapshot to `sceneOf`,
 * which returns the whole stadium as a `Scene` value. `sound` reads exactly this
 * tick's feedback and returns the tones to play. `readHud` is the pure projection
 * the DOM overlay renders.
 */

import type { Game, InputFrame, MaterialSpec, Rgba, ToneSpec } from "@axiom/web-engine";
import { HomeRunSession } from "./session.ts";
import { sceneOf } from "./view.ts";
import type { CinematicPhase, Feedback, Intent, Outcome, Phase, PitchResult } from "./types.ts";
import { HOME_RUN_CINEMATIC_TUNING } from "./cinematic-constants.ts";

export const PITCH_COUNT = 10;

/** Dev-only bounded counters, surfaced for inspection (not gameplay UI). Every
 * pool here has a hard cap declared in `HOME_RUN_CINEMATIC_TUNING`; there is no
 * pre-contact replay buffer to count — predictive evaluation (`swing-outcome.ts`)
 * makes it unnecessary, so that counter is intentionally absent rather than faked. */
export interface CinematicDebugCounters {
  readonly trailSegments: number;
  readonly impactParticles: number;
  readonly confettiMaxCount: number;
  readonly audioCuesThisTick: number;
}

/** The HUD snapshot the harness renders (pure projection of the session). */
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
  readonly readiness: number;
  readonly ready: boolean;
  readonly results: readonly PitchResult[];
  readonly cinematicPhase: CinematicPhase;
  readonly letterboxProgress: number;
  readonly hudVisible: boolean;
  readonly debugCounters: CinematicDebugCounters;
}

// ── declared materials (the old scene.ts palette, as data) ───────────────────────

const flat = (baseColor: Rgba): MaterialSpec => ({ baseColor });
const glow = (baseColor: Rgba, emissive: Rgba): MaterialSpec => ({ baseColor, emissive });

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  BallWhite: flat([1, 1, 0.98, 1]),
  // Painted markings glow faintly so they read white at grazing angles.
  BaseWhite: glow([1, 1, 0.98, 1], [0.3, 0.3, 0.28, 1]),
  BatKnob: flat([0.55, 0.4, 0.16, 1]),
  BatterBlue: flat([0.22, 0.46, 1, 1]),
  BatterHelmet: flat([0.14, 0.3, 0.85, 1]),
  BatterPuck: flat([0.55, 0.85, 1, 1]),
  CornerBlue: flat([0.24, 0.3, 0.8, 1]),
  DeckBrown: flat([0.72, 0.5, 0.3, 1]),
  Dirt: flat([0.82, 0.58, 0.34, 1]),
  DirtLight: flat([0.95, 0.72, 0.44, 1]),
  DotBlue: flat([0.2, 0.35, 0.95, 1]),
  DotRed: flat([0.9, 0.15, 0.12, 1]),
  DotYellow: flat([0.95, 0.8, 0.15, 1]),
  FielderBase: flat([1, 0.6, 0.3, 1]),
  FielderCap: flat([1, 0.22, 0.18, 1]),
  FielderWhite: flat([1, 0.98, 0.95, 1]),
  GrassDark: flat([0.4, 0.82, 0.24, 1]),
  GrassLight: flat([0.55, 1, 0.34, 1]),
  GroundGreen: flat([0.38, 0.7, 0.24, 1]),
  Line: glow([1, 1, 0.96, 1], [0.3, 0.3, 0.28, 1]),
  MachineDark: flat([0.3, 0.3, 0.36, 1]),
  MachineOrange: flat([1, 0.6, 0.34, 1]),
  PanelNavy: flat([0.1, 0.13, 0.38, 1]),
  PatrolDirt: flat([0.68, 0.46, 0.26, 1]),
  PatrolGreen: flat([0.36, 0.72, 0.22, 1]),
  SeatBlue: flat([0.42, 0.54, 1, 1]),
  SeatBlueDark: flat([0.3, 0.39, 0.92, 1]),
  // The backdrop bowl reads as SKY — self-lit so the moving sun never darkens it.
  SkyBowl: glow([0.72, 0.76, 1, 1], [0.5, 0.56, 0.8, 1]),
  WallBlue: flat([0.32, 0.44, 1, 1]),
  WallTrim: flat([1, 0.68, 0.16, 1]),
  // Dynamic-actor materials.
  bat: glow([1, 0.88, 0.25, 1], [0.45, 0.36, 0.08, 1]),
  digit: glow([1, 0.3, 0.15, 1], [0.9, 0.2, 0.08, 1]),
  flash: glow([1, 0.95, 0.6, 1], [1, 0.85, 0.4, 1]),
  impact: { baseColor: [1, 0.9, 0.5, 1], emissive: [1, 0.8, 0.35, 1], opacity: 0.55 },
  shadow: { baseColor: [0.05, 0.12, 0.05, 1], opacity: 0.35 },
  trail: glow([1, 0.9, 0.6, 1], [1, 0.75, 0.35, 1]),
};

// ── input → intent (pure) ────────────────────────────────────────────────────────

/**
 * Fold this tick's resolved input into the session `Intent`. The camera looks
 * downfield so world +X renders to screen-LEFT; the keyboard axis is negated so
 * pressing D/→ moves the batter right ON SCREEN.
 */
const intentOf = (input: InputFrame): Intent => {
  const kbAxis = (input.down.has("right") ? 1 : 0) - (input.down.has("left") ? 1 : 0);
  return {
    moveX: -kbAxis,
    start: input.pressed.has("swing") || input.pressed.has("restart"),
    swing: input.pressed.has("swing"),
  };
};

// ── audio (pure: events → tones) ─────────────────────────────────────────────────

const toneFor = (kind: Feedback["kind"], big: boolean): readonly ToneSpec[] => {
  switch (kind) {
    case "release":
      return [{ duration: 0.05, freq: 660, volume: 0.12, wave: "square" }];
    case "contact":
      return [
        { duration: 0.07, freq: big ? 220 : 180, volume: 0.5, wave: "square" },
        { duration: 0.05, freq: big ? 1400 : 900, volume: 0.25, wave: "triangle" },
      ];
    case "homer":
      // A rising major arpeggio (C–E–G–C), staggered by the tone `delay`.
      return [523, 659, 784, 1047].map((freq, i) => ({ delay: i * 0.05, duration: 0.16, freq, volume: 0.3, wave: "triangle" }));
    case "clean":
      return [{ duration: 0.12, freq: 587, volume: 0.22, wave: "triangle" }];
    case "miss":
      return [{ duration: 0.12, freq: 110, volume: 0.18, wave: "sawtooth" }];
    case "ball":
      return [{ duration: 0.1, freq: 300, volume: 0.12, wave: "sine" }];
    case "foul":
      return [{ duration: 0.08, freq: 240, volume: 0.18, wave: "square" }];
    case "caught":
    case "fielded":
    case "weak":
    case "grounder":
    case "popup":
      return [{ duration: 0.08, freq: 160, volume: 0.2, wave: "sine" }];
    // Cinematic activation cue — a quiet rising sweep the instant anticipation begins.
    case "cinematicAnticipation":
      return [
        { duration: 0.1, freq: 200, volume: 0.14, wave: "sine" },
        { delay: 0.06, duration: 0.14, freq: 320, volume: 0.16, wave: "sine" },
      ];
    // Rising crowd reaction as the ball separates from the bat and heads for the wall.
    case "crowdErupt":
      return [
        { duration: 0.22, freq: 140, volume: 0.2, wave: "sawtooth" },
        { delay: 0.05, duration: 0.18, freq: 210, volume: 0.16, wave: "triangle" },
      ];
    default:
      return [];
  }
};

/** The pure Home Run! game the `runGame` shell drives. */
export const game: Game<HomeRunSession> = {
  actions: {
    left: ["ArrowLeft", "KeyA"],
    restart: ["Enter"],
    right: ["ArrowRight", "KeyD"],
    swing: ["Space"],
  },
  init: (seed: number): HomeRunSession => new HomeRunSession(seed),
  resources: {
    materials: MATERIALS,
    meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
  },
  sound: (previous: HomeRunSession, next: HomeRunSession): readonly ToneSpec[] => {
    const cues = next.tickEvents.flatMap((event) => toneFor(event.kind, event.big));
    // A soft click the instant the batter finishes re-winding (ready to swing).
    const becameReady = next.swing.state === "ready" && previous.swing.state !== "ready";
    return becameReady ? [...cues, { duration: 0.05, freq: 880, volume: 0.14, wave: "sine" }] : cues;
  },
  update: (state: HomeRunSession, input: InputFrame): HomeRunSession => {
    const next = state.clone();
    next.advance(intentOf(input));
    return next;
  },
  view: (state: HomeRunSession, ctx): ReturnType<typeof sceneOf> => sceneOf(state.view(), ctx.nowMs),
};

/** The pure HUD projection the DOM overlay renders each frame. */
export const readHud = (state: HomeRunSession): Hud => {
  const view = state.view();
  return {
    bestDistance: state.bestDistance,
    cinematicPhase: view.cinematicPhase,
    debugCounters: {
      audioCuesThisTick: state.tickEvents.flatMap((event) => toneFor(event.kind, event.big)).length,
      confettiMaxCount: HOME_RUN_CINEMATIC_TUNING.confettiMaxCount,
      impactParticles: view.debugCounters.impactParticles,
      trailSegments: view.debugCounters.trailSegments,
    },
    homers: state.homers,
    hudVisible: view.hudVisible,
    lastMph: state.lastMph,
    lastPitchName: state.lastPitchName,
    letterboxProgress: view.letterboxProgress,
    multiplier: state.streakMultiplier,
    phase: state.phase,
    pitchCount: PITCH_COUNT,
    pitchNumber: state.pitchNumber,
    ready: state.swing.state === "ready",
    readiness: state.swing.readiness,
    results: state.results,
    score: state.score,
    streak: state.streak,
  };
};

export type { Outcome };
