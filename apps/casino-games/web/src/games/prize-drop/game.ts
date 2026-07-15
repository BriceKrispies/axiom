/*
 * game.ts — Prize Drop controller. A pachinko board: the reward slots at the
 * bottom ARE the destination slots, their drawn widths are exactly the compiled
 * per-slot probabilities (the board never lies about the odds), and the winning
 * slot is committed at drop time. The reveal is an analytic, deterministic fall:
 * the token descends through the staggered peg field, deflecting at each row by
 * a bounded amount drawn from the trajectory stream, while its per-row target x
 * is interpolated from the release column (row 0) to the committed slot center
 * (final row). The deflection amplitude shrinks to zero at the last row, so the
 * token arrives EXACTLY at the committed slot via a continuous path — never a
 * final-frame snap (the continuity test pins this).
 */

import type { InputFrame, TickContext } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { DestinationSlot } from "../../chance-engine/probability/destination.ts";
import { destinationProbabilities } from "../../chance-engine/probability/destination.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { CANVAS_WIDTH } from "../../presentation/cameras/picking.ts";
import { clamp01, lerp, smoothstep } from "../../presentation/stage/easing.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** One authored reward slot at the foot of the board. Its drawn width is NOT
 * authored — it is compiled from (targetWinRate, mass) so the board matches the
 * odds. `tierId` null marks a non-winning slot. */
export interface DropSlot {
  readonly label: string;
  readonly tierId: string | null;
  readonly mass: number;
}

export interface DropSpec {
  readonly slots: readonly DropSlot[];
}

export interface DropExtra {
  /** Release column in [0, 1] (left→right), aimed during "ready". */
  readonly dropPosition: number;
}

export type DropState = CasinoState<DropExtra>;

// ── board geometry (world units) ──────────────────────────────────────────────

/** Half-width of the reward-slot strip (slots span [−HALF, +HALF]). */
export const BOARD_HALF = 3;
/** The drop column is constrained inside the board so the token starts on-board. */
export const DROP_HALF = 2.5;
export const TOP_Y = 4.4;
export const SLOT_Y = 0.42;
/** Number of staggered peg rows the token falls through. */
export const PEG_ROWS = 5;
/** Peg columns per row (odd rows are offset by half a spacing). */
export const PEG_COLS = 6;
/** Maximum per-row deflection at row 0 (shrinks linearly to 0 at the last row). */
const MAX_DEFLECT = 0.66;
/** Keyboard nudge per tick while aiming. */
const AIM_STEP = 0.018;

export const destinationSlotsOf = (spec: DropSpec): readonly DestinationSlot[] =>
  spec.slots.map((slot, index) => ({ id: `${index}:${slot.label}`, mass: slot.mass, tierId: slot.tierId }));

export interface SlotRange {
  readonly start: number;
  readonly end: number;
  readonly center: number;
}

/** Slot x-ranges across the board, widths proportional to compiled probability. */
export const dropSlotRanges = (spec: DropSpec, targetWinRate: number): readonly SlotRange[] => {
  const probabilities = destinationProbabilities(destinationSlotsOf(spec), targetWinRate);
  let acc = -BOARD_HALF;
  const width = BOARD_HALF * 2;
  return probabilities.map((p) => {
    const start = acc;
    acc += Math.max(0.02, p) * width;
    return { center: (start + acc) / 2, end: acc, start };
  });
};

/** Normalize the slot ranges so the last slot ends exactly at +BOARD_HALF even
 * when the floor width (0.02) padding nudged the running total. */
export const committedSlotRanges = (spec: DropSpec, targetWinRate: number): readonly SlotRange[] => {
  const raw = dropSlotRanges(spec, targetWinRate);
  const span = raw[raw.length - 1]?.end ?? BOARD_HALF;
  const scale = span > -BOARD_HALF ? (BOARD_HALF * 2) / (span + BOARD_HALF) : 1;
  return raw.map((r) => ({
    center: -BOARD_HALF + (r.center + BOARD_HALF) * scale,
    end: -BOARD_HALF + (r.end + BOARD_HALF) * scale,
    start: -BOARD_HALF + (r.start + BOARD_HALF) * scale,
  }));
};

/** The committed slot ranges for this session (a stable per-mount projection). */
export const slotRangesOf = (runtime: GameRuntime<DropSpec>): readonly SlotRange[] =>
  committedSlotRanges(runtime.config.gameSpecific, runtime.config.targetWinRate);

/** The drop column (0..1) mapped to a world x inside the board. */
export const dropWorldX = (dropPosition: number): number => (clamp01(dropPosition) - 0.5) * 2 * DROP_HALF;

/** Committed destination slot index (0 before commitment). */
export const committedSlotIndex = (session: SessionState): number => {
  const plan = session.committed;
  return plan !== null && plan.manifestation.kind === "destination" ? plan.manifestation.destinationIndex : 0;
};

/** The x of control point `row` (0 = release column, PEG_ROWS = committed slot
 * center). Deflection amplitude shrinks to 0 at the final row, so the terminal
 * control point is EXACTLY the slot center. */
export const controlPointX = (
  dropX: number,
  slotCenter: number,
  row: number,
  seed: number,
  round: number,
): number => {
  const t = row / PEG_ROWS;
  const base = lerp(dropX, slotCenter, t);
  const amp = MAX_DEFLECT * (1 - t);
  const jitter = (sample01(seed, "trajectory", round, row) - 0.5) * 2 * amp;
  return base + jitter;
};

/** The token's world x at fall progress `p` in [0, 1] — a smoothstep blend
 * between adjacent control points, so motion is continuous and bounded. */
export const tokenPathX = (
  dropX: number,
  slotCenter: number,
  p: number,
  seed: number,
  round: number,
): number => {
  const rf = clamp01(p) * PEG_ROWS;
  const r0 = Math.min(PEG_ROWS - 1, Math.floor(rf));
  const frac = rf - r0;
  const a = controlPointX(dropX, slotCenter, r0, seed, round);
  const b = controlPointX(dropX, slotCenter, r0 + 1, seed, round);
  return lerp(a, b, smoothstep(frac));
};

/** The token's world y at fall progress `p` (gentle gravity acceleration). */
export const tokenPathY = (p: number): number => lerp(TOP_Y, SLOT_Y, clamp01(p) ** 1.4);

export interface DropTimeline {
  readonly fall: number;
  readonly settle: number;
  readonly total: number;
}

export const dropTimeline = (presentationSpeed: number, reducedMotion: boolean): DropTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const fall = speedTicks(Math.round(120 * scale), presentationSpeed);
  const settle = speedTicks(Math.round(26 * scale), presentationSpeed);
  return { fall, settle, total: fall + settle };
};

/** Fall progress in [0, 1] for a reveal age. */
export const fallProgress = (age: number, timeline: DropTimeline): number => clamp01(age / timeline.fall);

/** The squash factor from the most recent peg strike (decays after each hit). */
export const tokenSquash = (p: number): number => {
  const frac = (clamp01(p) * PEG_ROWS) % 1;
  return Math.max(0, 1 - frac / 0.22) * 0.28;
};

/** Whether the token crossed a peg row between two fall progresses. */
export const crossedPeg = (prevP: number, nextP: number): boolean =>
  Math.floor(nextP * PEG_ROWS) > Math.floor(prevP * PEG_ROWS) && nextP < 1;

export const initialDropExtra = (_session: SessionState): DropExtra => ({ dropPosition: 0.5 });

export const stepDrop = (
  _runtime: GameRuntime<DropSpec>,
  state: DropState,
  input: InputFrame,
  _ctx: TickContext,
): DropState => {
  const session = state.session;

  if (session.phase === "ready") {
    const pointerAim =
      input.pointer !== undefined ? clamp01(input.pointer.pos.x / CANVAS_WIDTH) : state.extra.dropPosition;
    const keyed = pointerAim + (input.down.has("right") ? AIM_STEP : 0) - (input.down.has("left") ? AIM_STEP : 0);
    const dropPosition = clamp01(keyed);
    const commit = input.pressed.has("primary") || (input.pointer?.down ?? false);
    if (commit) {
      return {
        ...state,
        extra: { dropPosition },
        pendingContext: { dropPosition },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra: { dropPosition } };
  }

  if (session.phase === "revealing") {
    const timeline = dropTimeline(session.config.presentationSpeed, _runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
    return state;
  }

  return state;
};
