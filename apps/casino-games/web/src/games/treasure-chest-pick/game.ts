/*
 * game.ts — the Treasure Chest Pick controller: the mount spec's mechanic,
 * per-tick step, and reveal timeline. Nine carved-wood chests in a 3×3 grid;
 * the choice-population adapter preassigns which chests hold prizes before
 * the player can possibly choose; the reveal follows the classic cadence —
 * focus, anticipation brace, LATCH FALLS FIRST, pause, lid pops with
 * overshoot, warm light, reward (or honest empty interior).
 *
 * Idle "chest dances" draw exclusively from the AMBIENT stream keyed by tick
 * window and grid slot — never from the population — so no wobble can hint
 * at contents. The dance test pins this.
 */

import type { InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { EngineVec3 } from "@axiom/web-engine";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import { tabletopCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import type { ChoiceCore } from "../choice-input.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";

export interface ChestSpec {
  /** Idle dance liveliness in [0, 1]. */
  readonly danceLiveliness: number;
}

export interface ChestExtra {
  readonly choice: ChoiceCore;
  /** Tick at which the reveal began (session tick space), for cue edges. */
  readonly revealStartTick: number | null;
}

export type ChestState = CasinoState<ChestExtra>;

export const CHEST_COLUMNS = 3;
export const CHEST_SPACING = 2.05;

/** Grid slot world position (3 columns, rows recede in −Z). */
export const chestPosition = (index: number, count: number): EngineVec3 => {
  const columns = CHEST_COLUMNS;
  const rows = Math.ceil(count / columns);
  const col = index % columns;
  const row = Math.floor(index / columns);
  return v3((col - (columns - 1) / 2) * CHEST_SPACING, 0, (row - (rows - 1) / 2) * CHEST_SPACING * 0.92);
};

export const chestCamera = (count: number): ReturnType<typeof tabletopCamera> =>
  tabletopCamera(v3(0, 0.42, -0.1), 3.6 + Math.ceil(count / CHEST_COLUMNS) * 0.55);

export const chestTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: chestPosition(index, count),
    index,
    radiusPx: 78,
  }));

// ── presentation timing (ONE central config — no scattered magic numbers) ──────

/**
 * Every duration, easing magnitude, and staging constant of the chest's
 * presentation ritual, gathered here so the sequence is tuned in one place
 * rather than sprinkled through the view. Durations are in ticks (speed-scaled
 * where used); magnitudes are world-space unless noted. All of it is purely
 * cosmetic — nothing here can reach the outcome.
 */
export const CHEST_TIMING = {
  // Idle — a gentle, per-chest-desynced breathing plus an occasional gold gleam.
  idleBobPeriod: 150, // ticks per idle bob cycle
  idleBobAmp: 0.014, // world-units of vertical idle bob
  idleTwistAmp: 0.035, // radians of idle sway
  gleamPeriod: 300, // ticks between gold-trim gleam sweeps
  gleamDuty: 0.14, // fraction of the period a chest's trim gleams
  // Selection staging — the chosen chest lifts, tilts, and the others recede.
  liftInTicks: 12, // ease-up time when a chest is committed
  lift: 0.17, // world-units the chosen chest rises (~10 px at this camera)
  tilt: 0.15, // radians tilted toward the camera
  selectScale: 1.07, // slight enlarge of the chosen chest
  othersDim: 0.78, // brightness multiplier for the eight others (~22% dim)
  pushIn: 0.72, // reveal camera closeness (0..1) — the chosen chest fills the frame
  // Reveal ritual durations (ticks, speed-scaled at build time).
  brace: 22,
  latch: 16,
  pause: 12,
  lid: 14,
  rise: 34,
  hold: 12,
  burst: 10, // the light-burst flash window, right after the lid opens
  // Reveal magnitudes.
  shakeMag: 0.05, // anticipation shake amplitude
  latchDrop: 1.55, // radians the latch swings open
  latchRecoil: 0.22, // extra kick on the latch's release snap
  lidOpen: 1.9, // radians the lid swings open
  burstParticles: 12, // bounded upward light-burst motes
  riseHeight: 1.2, // world-units the prize climbs to hover clear above the chest
} as const;

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface RevealTimeline {
  readonly braceEnd: number;
  readonly latchStart: number;
  readonly latchEnd: number;
  /** Warm seam light begins leaking here (as the latch lands). */
  readonly seamStart: number;
  readonly pauseEnd: number;
  /** The lid begins to swing (= pauseEnd). */
  readonly lidStart: number;
  readonly lidEnd: number;
  /** The upward light burst peaks here (= lidEnd). */
  readonly burstAt: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const revealTimeline = (presentationSpeed: number, reducedMotion: boolean): RevealTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const braceEnd = t(CHEST_TIMING.brace);
  const latchStart = braceEnd;
  const latchEnd = latchStart + t(CHEST_TIMING.latch);
  const seamStart = latchEnd;
  const pauseEnd = latchEnd + t(CHEST_TIMING.pause);
  const lidStart = pauseEnd;
  const lidEnd = pauseEnd + t(CHEST_TIMING.lid);
  const burstAt = lidEnd;
  const riseEnd = lidEnd + t(CHEST_TIMING.rise);
  return {
    braceEnd,
    burstAt,
    latchEnd,
    latchStart,
    lidEnd,
    lidStart,
    pauseEnd,
    riseEnd,
    seamStart,
    total: riseEnd + t(CHEST_TIMING.hold),
  };
};

// ── formalized presentation phases (readable names for the reveal ritual) ──────

/** The named visual phases the chest presentation moves through. The legal
 * ordering is guaranteed upstream by the session phase machine (which also
 * hard-locks input during the protected phases), so this is a pure read of
 * where the ritual is — never a place a stray click can jump. */
export type ChestPresentation =
  | "idle"
  | "committed"
  | "anticipation"
  | "latch"
  | "seam"
  | "lid"
  | "burst"
  | "prize"
  | "result"
  | "reset";

export const presentationPhase = (session: SessionState, timeline: RevealTimeline): ChestPresentation => {
  const phase = session.phase;
  if (phase === "intro" || phase === "ready") {
    return "idle";
  }
  if (phase === "committing") {
    return "committed";
  }
  if (phase === "resetting") {
    return "reset";
  }
  if (phase === "celebrating" || phase === "complete") {
    return "result";
  }
  const age = phaseAge(session);
  if (age < timeline.braceEnd) {
    return "anticipation";
  }
  if (age < timeline.latchEnd) {
    return "latch";
  }
  if (age < timeline.pauseEnd) {
    return "seam";
  }
  if (age < timeline.lidEnd) {
    return "lid";
  }
  if (age < timeline.burstAt + timeline.lidEnd - timeline.lidStart) {
    return "burst";
  }
  return "prize";
};

// ── idle cosmetics (deterministic, per-chest, outcome-independent) ─────────────

/** A per-chest idle phase (radians), spaced by the golden angle so the nine
 * chests never bob in unison. Pure in the slot index — no seed — so it cannot
 * correlate with which chest wins. */
export const idlePhase = (index: number): number => (index * 2.399963 + 0.4) % (Math.PI * 2);

/** Gold-trim gleam strength in [0,1] for chest `index` at `tick`: a brief sweep
 * on a slow, per-chest-offset cycle so at most a couple of chests gleam at once.
 * Cosmetic and deterministic; never a function of the outcome or a live clock. */
export const goldGleam = (index: number, tick: number): number => {
  const period = CHEST_TIMING.gleamPeriod;
  const local = ((((tick + index * 71) % period) + period) % period) / period;
  const duty = CHEST_TIMING.gleamDuty;
  return local < duty ? Math.sin((local / duty) * Math.PI) : 0;
};

/**
 * The idle dance pose for chest `index` at `tick` — AMBIENT stream only.
 * Time is cut into windows; each window elects one dancer (and, rarely, a
 * second) and gives it a small scoot + twist + squash figure.
 */
export interface DancePose {
  readonly scootX: number;
  readonly twist: number;
  readonly squash: number;
}

export const dancePose = (index: number, count: number, tick: number, seed: number, liveliness: number): DancePose => {
  const window = Math.floor(tick / 96);
  const dancer = sampleInt(count, seed, "ambient", window, 0);
  const second = sampleInt(count, seed, "ambient", window, 1);
  const duet = sample01(seed, "ambient", window, 2) < 0.2;
  const isDancing = index === dancer || (duet && index === second);
  if (!isDancing || liveliness <= 0) {
    return { scootX: 0, squash: 0, twist: 0 };
  }
  const local = (tick % 96) / 96;
  const envelope = Math.sin(Math.PI * local);
  const figure = sample01(seed, "ambient", window, 3 + index);
  return {
    scootX: Math.sin(local * Math.PI * 4 + figure * 6) * 0.05 * liveliness * envelope,
    squash: Math.abs(Math.sin(local * Math.PI * 6)) * 0.045 * liveliness * envelope,
    twist: Math.sin(local * Math.PI * 2 + figure * 4) * 0.07 * liveliness * envelope,
  };
};

export const initialChestExtra = (_session: SessionState): ChestExtra => ({
  choice: initialChoice(4),
  revealStartTick: null,
});

/** Per-tick controller. Selection commits; the reveal advances on the shared
 * timeline and hands off to "celebrating" when it completes. */
export const stepChest = (
  runtime: GameRuntime<ChestSpec>,
  state: ChestState,
  input: InputFrame,
  _ctx: TickContext,
): ChestState => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;

  if (session.phase === "ready") {
    // Tap-to-confirm: on touch the first tap highlights a chest and the second
    // opens it; a desktop click still opens in one action (hover pre-arms it).
    const result = stepChoice(state.extra.choice, input, chestCamera(count), chestTargets(count), CHEST_COLUMNS, true);
    if (result.selectedNow !== null) {
      return {
        ...state,
        extra: { ...state.extra, choice: result.core },
        pendingContext: { selectedIndex: result.selectedNow },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra: { ...state.extra, choice: result.core } };
  }

  if (session.phase === "revealing") {
    const start = state.extra.revealStartTick ?? session.phaseStartTick;
    const timeline = revealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    const withStart: ChestState =
      state.extra.revealStartTick === null ? { ...state, extra: { ...state.extra, revealStartTick: start } } : state;
    if (phaseAge(session) >= timeline.total) {
      return { ...withStart, session: transition(session, "celebrating") };
    }
    return withStart;
  }

  return state;
};

/**
 * The chest's own reveal-ritual cues, phrased as marks crossed on the reveal
 * timeline: a light latch click, the weighty latch-land thump, the rising seam
 * shimmer, the heavy lid-open thump, and the burst shimmer as the lid settles —
 * plus soft count-up ticks over the first stretch of a winning celebration. The
 * win/loss fanfare itself is played centrally by the mount harness.
 */
export const chestCues = (prev: ChestState, next: ChestState): readonly ToneSpec[] => {
  const session = next.session;
  const seed = session.committed?.presentationSeed ?? session.seed;
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const crossed = (mark: number): boolean => before < mark && after >= mark;

  if (session.phase === "revealing" && prev.session.phase === "revealing") {
    const tl = revealTimeline(session.config.presentationSpeed, false);
    return [
      ...(crossed(tl.latchStart) ? tickCue(seed, 1) : []), // latch click as it releases
      ...(crossed(tl.latchEnd) ? thumpCue(seed, 2) : []), // latch lands / recoil snap
      ...(crossed(tl.seamStart) ? shimmerCue(seed, 3) : []), // warm seam light rising
      ...(crossed(tl.lidStart) ? thumpCue(seed, 4) : []), // weighty lid heave
      ...(crossed(tl.lidEnd) ? shimmerCue(seed, 5) : []), // light burst as the lid settles
    ];
  }

  // Count-up ticks accompanying the number climbing during a winning result.
  if (session.phase === "celebrating" && prev.session.phase === "celebrating" && (session.committed?.win ?? false)) {
    return [4, 8, 12, 16, 20, 24].filter((mark) => crossed(mark)).flatMap((_, i) => tickCue(seed, 30 + i));
  }
  return [];
};
