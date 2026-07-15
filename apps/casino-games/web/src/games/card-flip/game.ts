/*
 * game.ts — the Card Flip controller: the mount spec's mechanic, per-tick
 * step, and the flip timeline. A grid of thick face-down cards on the table;
 * the choice-population adapter preassigns every card's face before the
 * player can possibly choose; the reveal follows the classic cadence — the
 * card lifts, spins on its VERTICAL axis (showing its thin edge mid-flip),
 * lands face-up with an easeOutBack contact bounce, then the prize rises.
 *
 * Idle breathing draws exclusively from the AMBIENT stream keyed by tick
 * window and grid slot — never from the population — so no sway can hint at
 * a card's face. The focused test pins that the unselected faces the round
 * committed to are identical from deal to completion, and that the flip can
 * reach face-up only after the commitment exists.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { Rarity } from "../../chance-engine/configuration/schema.ts";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, tickCue } from "../../presentation/audio/cues.ts";
import { tabletopCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { clamp01, easeOutBack, easeOutCubic, smoothstep } from "../../presentation/stage/easing.ts";
import { addV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";
import type { ChoiceCore } from "../choice-input.ts";

export interface CardFlipSpec {
  /** Grid width in cards (validated to [2, 6]). */
  readonly columns: number;
}

export interface CardFlipExtra {
  readonly choice: ChoiceCore;
}

export type CardFlipState = CasinoState<CardFlipExtra>;

const CARD_SPACING_X = 1.5;
const CARD_SPACING_Z = 1.78;

/** Grid slot world position (rows recede in −Z, like the chest table). */
export const cardPosition = (index: number, count: number, columns: number): EngineVec3 => {
  const rows = Math.ceil(count / columns);
  const col = index % columns;
  const row = Math.floor(index / columns);
  return v3((col - (columns - 1) / 2) * CARD_SPACING_X, 0, (row - (rows - 1) / 2) * CARD_SPACING_Z);
};

export const cardCamera = (count: number, columns: number): ReturnType<typeof tabletopCamera> =>
  tabletopCamera(v3(0, 0.55, -0.05), 2.7 + Math.ceil(count / columns) * 0.85);

export const cardTargets = (count: number, columns: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: addV3(cardPosition(index, count, columns), v3(0, 0.65, 0)),
    index,
    radiusPx: 64,
  }));

// ── the flip timeline (ticks from entering "revealing", speed-scaled) ──────────

export interface CardTimeline {
  readonly liftEnd: number;
  readonly flipEnd: number;
  readonly settleEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const cardTimeline = (presentationSpeed: number, reducedMotion: boolean): CardTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const liftEnd = t(14);
  const flipEnd = liftEnd + t(20);
  const settleEnd = flipEnd + t(14);
  const riseEnd = settleEnd + t(26);
  return { flipEnd, liftEnd, riseEnd, settleEnd, total: riseEnd + t(8) };
};

/** Reveal-phase age: −1 before any reveal exists, the phase age while
 * revealing, and the timeline's end once celebrating/complete. */
export const revealAgeOf = (session: SessionState, timelineTotal: number): number => {
  if (session.phase === "revealing") {
    return phaseAge(session);
  }
  return session.phase === "celebrating" || session.phase === "complete" ? timelineTotal : -1;
};

/** Rarity of a tier id under this session's ladder, or null. */
export const tierRarityOf = (session: SessionState, tierId: string | null): Rarity | null =>
  tierId === null ? null : (session.config.rewardTiers.find((tier) => tier.id === tierId)?.rarity ?? null);

/** The committed choice population (all slots), or null before commitment. */
export const winnersOf = (session: SessionState): readonly (string | null)[] | null => {
  const plan = session.committed;
  return plan !== null && plan.manifestation.kind === "choice" ? plan.manifestation.winnersByIndex : null;
};

// ── the flip pose (a pure function of reveal age) ──────────────────────────────

export const FACE_UP_ANGLE = Math.PI;
const LIFT_HEIGHT = 0.55;

export interface CardFlipPose {
  /** Vertical lift above the rest pose. */
  readonly lift: number;
  /** Rotation about the card's own vertical axis: 0 = face-down, π = face-up. */
  readonly angle: number;
  /** Contact-bounce compression at landing (0 = none). */
  readonly squash: number;
}

/** The selected card's pose at `revealAge` ticks into the reveal. Face-up is
 * reachable only with `revealAge ≥ 0`, i.e. only inside phases that the
 * session layer seals behind a committed outcome. */
export const cardFlipPose = (revealAge: number, timeline: CardTimeline): CardFlipPose => {
  if (revealAge < 0) {
    return { angle: 0, lift: 0, squash: 0 };
  }
  const liftT = clamp01(revealAge / timeline.liftEnd);
  const flipT = clamp01((revealAge - timeline.liftEnd) / (timeline.flipEnd - timeline.liftEnd));
  const settleT = clamp01((revealAge - timeline.flipEnd) / (timeline.settleEnd - timeline.flipEnd));
  const descent = easeOutBack(settleT);
  return {
    angle: FACE_UP_ANGLE * smoothstep(flipT),
    lift: LIFT_HEIGHT * easeOutCubic(liftT) * (1 - Math.min(1, descent)),
    squash: Math.max(0, descent - 1) * 1.6,
  };
};

// ── idle breathing (AMBIENT stream only) ───────────────────────────────────────

export interface CardBreath {
  readonly bob: number;
  readonly tilt: number;
}

/** Gentle per-card breathing plus one elected "settler" per time window that
 * shuffles in place. Draws only from the AMBIENT stream, keyed by window and
 * slot, so it can never correlate with card contents. */
export const cardBreath = (index: number, count: number, tick: number, seed: number): CardBreath => {
  const window = Math.floor(tick / 120);
  const settler = sampleInt(count, seed, "ambient", window, 0);
  const phase = sample01(seed, "ambient", window, 10 + index) * Math.PI * 2;
  const local = (tick % 120) / 120;
  const envelope = Math.sin(Math.PI * local);
  const settling = index === settler;
  return {
    bob: Math.sin(tick * 0.045 + phase) * 0.012 + (settling ? Math.abs(Math.sin(local * Math.PI * 3)) * 0.03 * envelope : 0),
    tilt: Math.sin(tick * 0.03 + phase) * 0.02 + (settling ? Math.sin(local * Math.PI * 2) * 0.03 * envelope : 0),
  };
};

// ── controller ─────────────────────────────────────────────────────────────────

export const initialCardFlipExtra = (_session: SessionState): CardFlipExtra => ({ choice: initialChoice(0) });

/** Per-tick controller. Selection commits; the reveal advances on the flip
 * timeline and hands off to "celebrating" when it completes. */
export const stepCardFlip = (
  runtime: GameRuntime<CardFlipSpec>,
  state: CardFlipState,
  input: InputFrame,
  _ctx: TickContext,
): CardFlipState => {
  const session = state.session;
  const count = session.config.choiceCount ?? 8;
  const columns = runtime.config.gameSpecific.columns;

  if (session.phase === "ready") {
    const result = stepChoice(state.extra.choice, input, cardCamera(count, columns), cardTargets(count, columns), columns);
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
    const timeline = cardTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Reveal-mechanism cues: a soft tick as the card lifts free of the table, a
 * shimmer as the flip lands (win/loss fanfare is played by the harness). */
export const cardFlipCues = (prev: CardFlipState, next: CardFlipState, reducedMotion: boolean): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = cardTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(timeline.liftEnd) ? tickCue(seed, 1) : []),
    ...(crossed(timeline.flipEnd) ? shimmerCue(seed, 2) : []),
  ];
};
