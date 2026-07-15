/*
 * game.ts — the Present Pop controller: the mount spec's mechanic, per-tick
 * step, the pop timeline, and the analytic burst. A grid of wrapped presents
 * on the showcase stage; the choice-population adapter preassigns each
 * present's contents before the player can possibly choose; the reveal follows
 * a classic cadence — the present shakes, squashes down, springs up, then
 * BURSTS: its lid and wall panels (plus a few ribbon shards) fly outward on
 * ballistic arcs drawn from the TRAJECTORY stream, and the reward rises from
 * the center (or a soft empty puff).
 *
 * Idle hops draw exclusively from the AMBIENT stream keyed by tick window and
 * grid slot — never from the population or the presentation seed. The focused
 * test pins that the burst is a pure function of the presentation seed and
 * that a different presentation seed reshuffles the debris WITHOUT changing the
 * committed outcome.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, thumpCue } from "../../presentation/audio/cues.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { clamp01, easeOutBack, easeOutCubic } from "../../presentation/stage/easing.ts";
import { addV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";
import type { ChoiceCore } from "../choice-input.ts";

export interface PresentPopSpec {
  /** Idle anticipatory-hop liveliness in [0, 1]. */
  readonly hopLiveliness: number;
}

export interface PresentPopExtra {
  readonly choice: ChoiceCore;
}

export type PresentPopState = CasinoState<PresentPopExtra>;

export const PRESENT_COLUMNS = 3;
const PRESENT_SPACING_X = 1.85;
const PRESENT_SPACING_Z = 1.85;

/** Bounded debris budgets (analytic burst, never per-frame allocation growth). */
export const BURST_PANELS = 6;
export const BURST_RIBBONS = 4;

/** Grid slot world position (3 columns, rows recede in −Z). */
export const presentPosition = (index: number, count: number): EngineVec3 => {
  const rows = Math.ceil(count / PRESENT_COLUMNS);
  const col = index % PRESENT_COLUMNS;
  const row = Math.floor(index / PRESENT_COLUMNS);
  return v3((col - (PRESENT_COLUMNS - 1) / 2) * PRESENT_SPACING_X, 0, (row - (rows - 1) / 2) * PRESENT_SPACING_Z);
};

export const presentCamera = (count: number): ReturnType<typeof showcaseCamera> =>
  showcaseCamera(v3(0, 0.7, 0), 4.6 + Math.ceil(count / PRESENT_COLUMNS) * 0.9, 2.1, 0.9);

export const presentTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: addV3(presentPosition(index, count), v3(0, 0.4, 0)),
    index,
    radiusPx: 72,
  }));

// ── the pop timeline (ticks from entering "revealing", speed-scaled) ────────────

export interface PopTimeline {
  readonly shakeEnd: number;
  readonly squashEnd: number;
  readonly burstStart: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const popTimeline = (presentationSpeed: number, reducedMotion: boolean): PopTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const shakeEnd = t(16);
  const squashEnd = shakeEnd + t(14);
  const burstStart = squashEnd;
  const riseEnd = burstStart + t(40);
  return { burstStart, riseEnd, shakeEnd, squashEnd, total: riseEnd + t(10) };
};

/** Reveal-phase age: −1 before any reveal exists, the phase age while
 * revealing, and the timeline total once celebrating/complete. */
export const revealAgeOf = (session: SessionState, timelineTotal: number): number => {
  if (session.phase === "revealing") {
    return phaseAge(session);
  }
  return session.phase === "celebrating" || session.phase === "complete" ? timelineTotal : -1;
};

// ── the pre-burst box pose (shake, then squash-and-spring) ──────────────────────

export interface BoxPose {
  /** Horizontal shake offset. */
  readonly shakeX: number;
  /** Vertical scale factor (squash < 1, spring > 1). */
  readonly squashY: number;
  /** Vertical lift as it springs up before bursting. */
  readonly lift: number;
  /** True once the box has burst (its solid form is gone). */
  readonly burst: boolean;
}

export const boxPose = (revealAge: number, timeline: PopTimeline, seed: number): BoxPose => {
  if (revealAge < 0) {
    return { burst: false, lift: 0, shakeX: 0, squashY: 1 };
  }
  if (revealAge >= timeline.burstStart) {
    return { burst: true, lift: 0, shakeX: 0, squashY: 1 };
  }
  const shakeT = clamp01(revealAge / timeline.shakeEnd);
  const squashT = clamp01((revealAge - timeline.shakeEnd) / (timeline.squashEnd - timeline.shakeEnd));
  const wobble = Math.sin(revealAge * 1.5 + sample01(seed, "ambient", 0) * 6) * 0.06 * (1 - shakeT + 0.3);
  // Squash down through the first half, then spring up past neutral.
  const squash = squashT < 0.5 ? 1 - squashT * 0.8 : 0.6 + easeOutBack(clamp01((squashT - 0.5) / 0.5)) * 0.5;
  return {
    burst: false,
    lift: squashT < 0.5 ? 0 : easeOutCubic(clamp01((squashT - 0.5) / 0.5)) * 0.3,
    shakeX: wobble,
    squashY: squash,
  };
};

// ── the analytic burst (TRAJECTORY stream, position-by-age like confetti) ───────

export interface BurstPiece {
  readonly position: EngineVec3;
  readonly spin: number;
  readonly axis: EngineVec3;
  readonly fade: number;
}

/**
 * A burst piece `i`'s pose `ageTicks` into the burst — an analytic ballistic
 * arc (launch angle + speed + spin from the TRAJECTORY stream, keyed by the
 * presentation seed and piece index), exactly like `confettiBurst` but for the
 * gift's own lid/wall/ribbon shards. Pure in (origin, presentationSeed, i,
 * ageTicks); returns null after the piece's life. Count is caller-bounded.
 */
export const burstPiece = (
  origin: EngineVec3,
  presentationSeed: number,
  i: number,
  ageTicks: number,
  lifeTicks: number,
): BurstPiece | null => {
  if (ageTicks < 0 || ageTicks > lifeTicks) {
    return null;
  }
  const t = ageTicks / 60;
  const angle = sample01(presentationSeed, "trajectory", i, 0) * Math.PI * 2;
  const speed = 1.6 + sample01(presentationSeed, "trajectory", i, 1) * 2.2;
  const up = 2.6 + sample01(presentationSeed, "trajectory", i, 2) * 2.2;
  const axis = v3(
    sample01(presentationSeed, "trajectory", i, 3) - 0.5,
    sample01(presentationSeed, "trajectory", i, 4) - 0.5,
    sample01(presentationSeed, "trajectory", i, 5) - 0.5,
  );
  const axisLen = Math.sqrt(axis.x ** 2 + axis.y ** 2 + axis.z ** 2) || 1;
  return {
    axis: v3(axis.x / axisLen, axis.y / axisLen, axis.z / axisLen),
    fade: 1 - ageTicks / lifeTicks,
    position: v3(
      origin.x + Math.cos(angle) * speed * t,
      origin.y + (up * t - 4.4 * t * t),
      origin.z + Math.sin(angle) * speed * t,
    ),
    spin: ageTicks * (0.14 + sample01(presentationSeed, "trajectory", i, 6) * 0.24),
  };
};

// ── idle anticipatory hop (AMBIENT stream only) ─────────────────────────────────

export interface HopPose {
  readonly hop: number;
  readonly wiggle: number;
}

/**
 * Idle hop for present `index` at `tick`. Each window elects one (rarely two)
 * hoppers that do a small anticipatory jump + wiggle; everything else rests.
 * Every draw is from the AMBIENT stream keyed by (window, slot) — never from
 * the presentation seed — so an idle hop cannot correlate with contents.
 */
export const hopPose = (index: number, count: number, tick: number, seed: number, liveliness: number): HopPose => {
  if (liveliness <= 0) {
    return { hop: 0, wiggle: 0 };
  }
  const window = Math.floor(tick / 84);
  const hopper = sampleInt(count, seed, "ambient", window, 0);
  const second = sampleInt(count, seed, "ambient", window, 1);
  const duet = sample01(seed, "ambient", window, 2) < 0.25;
  const isHopping = index === hopper || (duet && index === second);
  if (!isHopping) {
    return { hop: 0, wiggle: 0 };
  }
  const local = (tick % 84) / 84;
  const envelope = Math.sin(Math.PI * local);
  const figure = sample01(seed, "ambient", window, 10 + index);
  return {
    hop: Math.abs(Math.sin(local * Math.PI * 3)) * 0.14 * liveliness * envelope,
    wiggle: Math.sin(local * Math.PI * 5 + figure * 6) * 0.09 * liveliness * envelope,
  };
};

// ── controller ─────────────────────────────────────────────────────────────────

export const initialPresentPopExtra = (_session: SessionState): PresentPopExtra => ({ choice: initialChoice(0) });

/** Per-tick controller. Selection commits; the reveal advances on the pop
 * timeline and hands off to "celebrating" when it completes. */
export const stepPresentPop = (
  runtime: GameRuntime<PresentPopSpec>,
  state: PresentPopState,
  input: InputFrame,
  _ctx: TickContext,
): PresentPopState => {
  const session = state.session;
  const count = session.config.choiceCount ?? 6;

  if (session.phase === "ready") {
    const result = stepChoice(state.extra.choice, input, presentCamera(count), presentTargets(count), PRESENT_COLUMNS);
    if (result.selectedNow !== null) {
      return {
        ...state,
        extra: { choice: result.core },
        pendingContext: { selectedIndex: result.selectedNow },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra: { choice: result.core } };
  }

  if (session.phase === "revealing") {
    const timeline = popTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Reveal-mechanism cues: a shake shimmer, a thump at the burst (the win/loss
 * fanfare is played centrally by the harness). */
export const presentPopCues = (prev: PresentPopState, next: PresentPopState, reducedMotion: boolean): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = popTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(timeline.shakeEnd) ? shimmerCue(seed, 1) : []),
    ...(crossed(timeline.burstStart) ? thumpCue(seed, 2) : []),
  ];
};
