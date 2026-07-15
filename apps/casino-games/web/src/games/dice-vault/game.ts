/*
 * game.ts — the Dice Vault controller. COMBINATION mechanic: 1–3 chunky toy
 * dice in front of a cheerful vault. The winning-total rules in `gameSpecific`
 * compile into a `CombinationSpace` (reels = dice, 6 symbols per reel, symbol
 * s = face s+1); the engine commits an EXACT combination, and the tumble
 * animation is an analytic profile whose residual spin decays to zero on top
 * of each die's committed face-up orientation — so the settled faces equal the
 * committed combination with no final-frame snap.
 *
 * All physical variation (spin axes, spin amount, settle twist) draws from the
 * TRAJECTORY stream keyed off the committed plan's presentation seed; nothing
 * here can re-roll or override the committed outcome.
 */

import type { EngineQuat, EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { CombinationSpace, WinningCombination } from "../../chance-engine/probability/combination.ts";
import { sample01, sampleRange } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { easeOutCubic, pulse } from "../../presentation/stage/easing.ts";
import { QUAT_IDENTITY, quatAxisAngle, quatMul, quatPitch, quatRoll, quatYaw, rotateByQuat, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

// ── the game-specific spec ──────────────────────────────────────────────────────

/** One winning dice-total rule: rolling exactly `total` grants `tierId`. */
export interface DiceTotalRule {
  readonly total: number;
  readonly tierId: string;
}

/** The winning-combination rules. Precedence when several rules match one
 * roll: all-max (every die showing 6) beats all-same (doubles/triples), which
 * beats a matching total. All-same rules apply only with 2+ dice. */
export interface DiceCombos {
  readonly totals: readonly DiceTotalRule[];
  /** Tier for doubles/triples of any face, or null for no such rule. */
  readonly allSameTierId: string | null;
  /** Tier for all dice showing 6 (double-6 / triple-6), or null. */
  readonly allMaxTierId: string | null;
}

export interface DiceSpec {
  /** Number of dice thrown, 1–3. */
  readonly diceCount: number;
  readonly combos: DiceCombos;
}

/** The default two-dice table: lucky 7, boxcars-adjacent 11, doubles, double-6. */
export const DEFAULT_DICE_SPEC: DiceSpec = {
  combos: {
    allMaxTierId: "jackpot",
    allSameTierId: "uncommon",
    totals: [
      { tierId: "common", total: 7 },
      { tierId: "rare", total: 11 },
    ],
  },
  diceCount: 2,
};

// ── the combination space ───────────────────────────────────────────────────────

const FACES_PER_DIE = 6;

/** The tier a rolled face list wins under the spec's rules, or null. */
const tierOfFaces = (spec: DiceSpec, faces: readonly number[]): string | null => {
  const allSame = faces.length >= 2 && faces.every((face) => face === faces[0]);
  if (allSame && faces[0] === FACES_PER_DIE && spec.combos.allMaxTierId !== null) {
    return spec.combos.allMaxTierId;
  }
  if (allSame && spec.combos.allSameTierId !== null) {
    return spec.combos.allSameTierId;
  }
  const total = faces.reduce((sum, face) => sum + face, 0);
  return spec.combos.totals.find((rule) => rule.total === total)?.tierId ?? null;
};

/** Compile the spec into the engine's combination space: every combination
 * whose faces win under the rules is enumerated with its tier (6^diceCount
 * combinations total — bounded and done once per mount). */
export const diceSpace = (spec: DiceSpec): CombinationSpace => {
  const reels = spec.diceCount;
  const total = FACES_PER_DIE ** reels;
  const winningCombos: WinningCombination[] = [];
  for (let index = 0; index < total; index += 1) {
    let rest = index;
    const combo = Array.from({ length: reels }, () => {
      const symbol = rest % FACES_PER_DIE;
      rest = Math.floor(rest / FACES_PER_DIE);
      return symbol;
    });
    const tierId = tierOfFaces(spec, combo.map((symbol) => symbol + 1));
    if (tierId !== null) {
      winningCombos.push({ combo, tierId });
    }
  }
  return { reels, symbolsPerReel: FACES_PER_DIE, winningCombos };
};

// ── die orientation vocabulary ──────────────────────────────────────────────────

/** Die face assignment in local space (opposite faces sum to 7):
 * +Y=1, −Y=6, +X=2, −X=5, +Z=3, −Z=4. */
export const DIE_FACE_NORMALS: readonly { readonly normal: EngineVec3; readonly value: number }[] = [
  { normal: v3(0, 1, 0), value: 1 },
  { normal: v3(0, -1, 0), value: 6 },
  { normal: v3(1, 0, 0), value: 2 },
  { normal: v3(-1, 0, 0), value: 5 },
  { normal: v3(0, 0, 1), value: 3 },
  { normal: v3(0, 0, -1), value: 4 },
];

/** Orientation placing face `value` up (index by value − 1). */
const FACE_UP_QUATS: readonly EngineQuat[] = [
  QUAT_IDENTITY,
  quatRoll(Math.PI / 2),
  quatPitch(-Math.PI / 2),
  quatPitch(Math.PI / 2),
  quatRoll(-Math.PI / 2),
  quatRoll(Math.PI),
];

/** Which face value a die with orientation `q` shows upward. */
export const upFaceOf = (q: EngineQuat): number => {
  let best = 1;
  let bestY = Number.NEGATIVE_INFINITY;
  for (const face of DIE_FACE_NORMALS) {
    const worldY = rotateByQuat(face.normal, q).y;
    if (worldY > bestY) {
      bestY = worldY;
      best = face.value;
    }
  }
  return best;
};

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface DiceTimeline {
  readonly throwEnd: number;
  readonly bounce1End: number;
  readonly bounce2End: number;
  readonly settleEnd: number;
  readonly pauseEnd: number;
  /** Vault-reaction length after the pause: win door swing vs loss wobble. */
  readonly vaultTicks: number;
  readonly wobbleTicks: number;
}

export const diceTimeline = (presentationSpeed: number, reducedMotion: boolean): DiceTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const throwEnd = t(26);
  const bounce1End = throwEnd + t(16);
  const bounce2End = bounce1End + t(12);
  const settleEnd = bounce2End + t(14);
  const pauseEnd = settleEnd + t(18);
  return { bounce1End, bounce2End, pauseEnd, settleEnd, throwEnd, vaultTicks: t(48), wobbleTicks: t(28) };
};

/** Total revealing length for the committed outcome (win opens the vault). */
export const diceRevealTotal = (timeline: DiceTimeline, win: boolean): number =>
  timeline.pauseEnd + (win ? timeline.vaultTicks : timeline.wobbleTicks);

// ── the tumble (analytic; exact at settle) ──────────────────────────────────────

export const DIE_SIZE = 0.5;
export const DICE_Z = 0.55;

/** Rest position of die `index` on the table pad. */
export const diePosition = (index: number, count: number): EngineVec3 =>
  v3((index - (count - 1) / 2) * 0.78, DIE_SIZE / 2, DICE_Z);

/** Height of die `index` above its rest during the tumble: throw arc, then two
 * decaying bounces, each an analytic sine arc. */
export const dieHeight = (revealAge: number, timeline: DiceTimeline, seed: number, index: number): number => {
  const jitter = 0.9 + sample01(seed, "trajectory", index, 8) * 0.2;
  const arc = (from: number, to: number, h: number): number =>
    revealAge >= from && revealAge < to ? h * jitter * Math.sin((Math.PI * (revealAge - from)) / (to - from)) : 0;
  return (
    arc(0, timeline.throwEnd, 1.35) +
    arc(timeline.throwEnd, timeline.bounce1End, 0.42) +
    arc(timeline.bounce1End, timeline.bounce2End, 0.14)
  );
};

/** Impact squash (vertical compression pulse) right after each landing. */
export const dieSquash = (revealAge: number, timeline: DiceTimeline): number => {
  const impacts: readonly (readonly [number, number])[] = [
    [timeline.throwEnd, 0.2],
    [timeline.bounce1End, 0.13],
    [timeline.bounce2End, 0.08],
  ];
  let squash = 0;
  for (const [mark, amplitude] of impacts) {
    if (revealAge >= mark && revealAge < mark + 7) {
      squash += amplitude * pulse((revealAge - mark) / 7);
    }
  }
  return squash;
};

/**
 * Die orientation during the tumble. The residual spin — a rotation about a
 * per-die TRAJECTORY axis — decays to zero as the settle completes, leaving
 * exactly the committed face-up orientation (plus a settle twist about +Y,
 * which never changes the up face). Continuous everywhere; exact at the end.
 */
export const dieRotationAt = (
  revealAge: number,
  timeline: DiceTimeline,
  seed: number,
  index: number,
  symbol: number,
): EngineQuat => {
  const twist = sampleRange(-0.9, 0.9, seed, "trajectory", index, 5);
  const finalQ = quatMul(quatYaw(twist), FACE_UP_QUATS[symbol] as EngineQuat);
  const axisRaw = v3(
    sampleRange(-1, 1, seed, "trajectory", index, 0),
    sampleRange(-1, 1, seed, "trajectory", index, 1),
    sampleRange(-1, 1, seed, "trajectory", index, 2),
  );
  const length = Math.sqrt(axisRaw.x ** 2 + axisRaw.y ** 2 + axisRaw.z ** 2);
  const axis = length > 0.001 ? v3(axisRaw.x / length, axisRaw.y / length, axisRaw.z / length) : v3(0, 1, 0);
  const totalSpin = sampleRange(2, 3.5, seed, "trajectory", index, 3) * Math.PI * 2;
  const progress = Math.min(1, Math.max(0, revealAge / timeline.settleEnd));
  const remaining = totalSpin * (1 - easeOutCubic(progress));
  return quatMul(quatAxisAngle(axis, remaining), finalQ);
};

// ── the controller ──────────────────────────────────────────────────────────────

export interface DiceExtra {
  /** Previous tick's pointer-button state, for click edges. */
  readonly pointerWasDown: boolean;
}

export type DiceState = CasinoState<DiceExtra>;

export const initialDiceExtra = (_session: SessionState): DiceExtra => ({ pointerWasDown: false });

/** Per-tick controller: primary (or a click) rolls; the reveal advances on the
 * tumble timeline and hands off to "celebrating" when the vault has reacted. */
export const stepDice = (
  runtime: GameRuntime<DiceSpec>,
  state: DiceState,
  input: InputFrame,
  _ctx: TickContext,
): DiceState => {
  const session = state.session;
  const pointerDown = input.pointer?.down ?? false;
  const clicked = pointerDown && !state.extra.pointerWasDown;
  const tracked: DiceState =
    pointerDown === state.extra.pointerWasDown ? state : { ...state, extra: { pointerWasDown: pointerDown } };

  if (session.phase === "ready" && (input.pressed.has("primary") || clicked)) {
    return { ...tracked, pendingContext: {}, session: transition(session, "committing") };
  }

  if (session.phase === "revealing" && session.committed !== null) {
    const timeline = diceTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= diceRevealTotal(timeline, session.committed.win)) {
      return { ...tracked, session: transition(session, "celebrating") };
    }
  }

  return tracked;
};

/** Tumble cues: a table thump at each landing, a shimmer as the pause breaks
 * into the vault's reaction (the win/loss fanfare is played by the harness). */
export const diceCues = (
  prev: DiceState,
  next: DiceState,
  thump: (seed: number, key: number) => readonly ToneSpec[],
  shimmer: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing" || session.committed === null) {
    return [];
  }
  const timeline = diceTimeline(session.config.presentationSpeed, false);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed.presentationSeed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(timeline.throwEnd) ? thump(seed, 1) : []),
    ...(crossed(timeline.bounce1End) ? thump(seed, 2) : []),
    ...(crossed(timeline.bounce2End) ? thump(seed, 3) : []),
    ...(crossed(timeline.pauseEnd) ? shimmer(seed, 4) : []),
  ];
};
