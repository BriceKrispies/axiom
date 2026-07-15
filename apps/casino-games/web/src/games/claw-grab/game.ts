/*
 * game.ts — Claw Grab controller: a claw-machine cabinet seen from INSIDE.
 * Single-reveal mechanic. The player steers the claw over a bed of plush
 * prizes; on drop the prize actually under the claw is committed as the
 * targeted index (echoed into `plan.manifestation.focusIndex`). The reveal
 * descends over THAT prize — never a different, distant one — and the
 * configured `targetWinRate` alone decides whether the grip holds. A win
 * carries the prize to the front-left chute; a loss plays one warm, brief
 * flavor (slip / grip-and-release / clean miss) from the trajectory stream.
 *
 * All kinematics are pure functions of (spec, session, extra), so the scene and
 * the tests read the same claw and prize positions.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import type { MachineVolume } from "../../presentation/cameras/presets.ts";
import { machineInteriorCamera } from "../../presentation/cameras/presets.ts";
import { bob, clamp01, easeOutCubic, lerp, smoothstep } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

export interface ClawGrabSpec {
  /** Number of plush prizes on the bed. */
  readonly prizeCount: number;
  /** Horizontal steer speed (world units per tick). */
  readonly steerSpeed: number;
}

export interface ClawExtra {
  /** Claw x over the bed (frozen at drop). */
  readonly clawX: number;
  /** Claw z over the bed (frozen at drop). */
  readonly clawZ: number;
}

export type ClawState = CasinoState<ClawExtra>;

// ── machine interior geometry ──────────────────────────────────────────────────

export const MACHINE_VOLUME: MachineVolume = { center: v3(0, 1.45, 0), size: v3(5, 3.1, 3.4) };
export const BED_Y = 0.32;
export const GANTRY_Y = 2.62;
export const CLAW_HOVER_Y = 2.15;
export const CHUTE_AT: EngineVec3 = v3(-1.9, 0.5, 1.05);
export const CLAW_X_LIMIT = 1.85;
export const CLAW_Z_LIMIT = 0.9;
export const PRIZE_RADIUS = 0.62;

export const clawCamera = (): ReturnType<typeof machineInteriorCamera> => machineInteriorCamera(MACHINE_VOLUME);

const PRIZE_COLUMNS = 4;

/** Resting position of prize `index` on the bed (2-row grid, half-buried). */
export const prizePosition = (index: number, count: number): EngineVec3 => {
  const columns = Math.min(PRIZE_COLUMNS, count);
  const rows = Math.ceil(count / columns);
  const col = index % columns;
  const row = Math.floor(index / columns);
  const x = (col - (columns - 1) / 2) * (2 * CLAW_X_LIMIT / Math.max(1, columns));
  const z = (row - (rows - 1) / 2) * (2 * CLAW_Z_LIMIT / Math.max(1, rows)) * 0.85;
  return v3(x, BED_Y, z);
};

/** The prize nearest the claw — ALWAYS some index, so a drop always has a
 * target. The ring highlight (scene) additionally requires the prize be within
 * `PRIZE_RADIUS`; targeting itself never returns null. */
export const targetedPrizeIndexOf = (count: number, clawX: number, clawZ: number): number => {
  let best = 0;
  let bestDist = Number.POSITIVE_INFINITY;
  for (let i = 0; i < count; i += 1) {
    const at = prizePosition(i, count);
    const dist = Math.hypot(at.x - clawX, at.z - clawZ);
    if (dist < bestDist) {
      best = i;
      bestDist = dist;
    }
  }
  return best;
};

/** True when a prize is close enough under the claw to highlight its ring. */
export const targetInReach = (count: number, clawX: number, clawZ: number): boolean => {
  const at = prizePosition(targetedPrizeIndexOf(count, clawX, clawZ), count);
  return Math.hypot(at.x - clawX, at.z - clawZ) <= PRIZE_RADIUS;
};

/** The committed focus prize (the one the claw targeted at drop). */
export const focusIndexOf = (session: SessionState): number => {
  const m = session.committed?.manifestation;
  return m !== undefined && m.kind === "single" ? m.focusIndex : 0;
};

// ── loss flavors ─────────────────────────────────────────────────────────────────

export type LossFlavor = "slip" | "grip-release" | "clean-miss";
const LOSS_FLAVORS: readonly LossFlavor[] = ["slip", "grip-release", "clean-miss"];

export const lossFlavorOf = (presentationSeed: number): LossFlavor =>
  LOSS_FLAVORS[sampleInt(LOSS_FLAVORS.length, presentationSeed, "trajectory", 202)] as LossFlavor;

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface ClawTimeline {
  readonly descendEnd: number;
  readonly gripEnd: number;
  readonly liftEnd: number;
  readonly carryEnd: number;
  readonly releaseEnd: number;
  readonly total: number;
}

export const clawTimeline = (presentationSpeed: number, reducedMotion: boolean): ClawTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const descendEnd = t(40);
  const gripEnd = descendEnd + t(18);
  const liftEnd = gripEnd + t(34);
  const carryEnd = liftEnd + t(46);
  const releaseEnd = carryEnd + t(26);
  return { carryEnd, descendEnd, gripEnd, liftEnd, releaseEnd, total: releaseEnd + t(10) };
};

// ── kinematics (all pure) ────────────────────────────────────────────────────────

const revealAgeOf = (session: SessionState): number => {
  const settled = session.phase === "celebrating" || session.phase === "complete";
  return session.phase === "revealing" ? phaseAge(session) : settled ? Number.MAX_SAFE_INTEGER : -1;
};

/** The claw tip world position in any phase. */
export const clawTip = (spec: ClawGrabSpec, state: ClawState, reducedMotion: boolean): EngineVec3 => {
  const session = state.session;
  const dropX = state.extra.clawX;
  const dropZ = state.extra.clawZ;
  const age = revealAgeOf(session);
  if (age < 0) {
    return v3(dropX, CLAW_HOVER_Y + bob(session.tick, 90) * 0.03, dropZ);
  }
  const timeline = clawTimeline(session.config.presentationSpeed, reducedMotion);
  const a = Math.min(age, timeline.total);
  const focus = prizePosition(focusIndexOf(session), spec.prizeCount);
  const win = session.committed?.win ?? false;
  const grabTop = BED_Y + 0.34;

  // Descend: ease horizontally onto the focus prize while lowering.
  const descendT = smoothstep(clamp01(a / timeline.descendEnd));
  const x0 = lerp(dropX, focus.x, descendT);
  const z0 = lerp(dropZ, focus.z, descendT);
  const y0 = lerp(CLAW_HOVER_Y, grabTop, descendT);
  if (a <= timeline.descendEnd) {
    return v3(x0, y0, z0);
  }
  if (a <= timeline.gripEnd) {
    return v3(focus.x, grabTop, focus.z);
  }
  // Lift back up.
  const liftT = smoothstep(clamp01((a - timeline.gripEnd) / (timeline.liftEnd - timeline.gripEnd)));
  const yLift = lerp(grabTop, CLAW_HOVER_Y, liftT);
  if (a <= timeline.liftEnd || !win) {
    return v3(focus.x, yLift, focus.z);
  }
  // Carry (win only): traverse to the chute.
  const carryT = smoothstep(clamp01((a - timeline.liftEnd) / (timeline.carryEnd - timeline.liftEnd)));
  return v3(lerp(focus.x, CHUTE_AT.x, carryT), CLAW_HOVER_Y, lerp(focus.z, CHUTE_AT.z, carryT));
};

/** Finger close amount in [0,1] (0 open, 1 gripped). */
export const fingerCloseOf = (session: SessionState, reducedMotion: boolean): number => {
  const age = revealAgeOf(session);
  if (age < 0) {
    return 0;
  }
  const timeline = clawTimeline(session.config.presentationSpeed, reducedMotion);
  const a = Math.min(age, timeline.total);
  const win = session.committed?.win ?? false;
  const flavor = lossFlavorOf(session.committed?.presentationSeed ?? session.seed);
  const closeT = clamp01((a - timeline.descendEnd) / (timeline.gripEnd - timeline.descendEnd));
  const closed = easeOutCubic(closeT);
  // A clean miss closes only partway; a slip closes then springs open on lift.
  const missCap = !win && flavor === "clean-miss" ? 0.55 : 1;
  const grip = Math.min(closed, missCap);
  const slipRelease = !win && flavor === "slip" ? clamp01((a - timeline.gripEnd) / 14) : 0;
  const carryRelease = win ? clamp01((a - timeline.carryEnd) / (timeline.releaseEnd - timeline.carryEnd)) : 0;
  const gripReleaseFlavor =
    !win && flavor === "grip-release" ? clamp01((a - (timeline.gripEnd + timeline.liftEnd) / 2) / 14) : 0;
  return grip * (1 - Math.max(slipRelease, carryRelease, gripReleaseFlavor));
};

/** The index of the prize the claw is actually carrying, or null. Always the
 * committed focus prize — never a substitute. */
export const carriedPrizeIndex = (session: SessionState, reducedMotion: boolean): number | null => {
  const age = revealAgeOf(session);
  if (age < 0 || session.committed === null) {
    return null;
  }
  const timeline = clawTimeline(session.config.presentationSpeed, reducedMotion);
  const a = Math.min(age, timeline.total);
  const win = session.committed.win;
  const flavor = lossFlavorOf(session.committed.presentationSeed);
  if (a < timeline.gripEnd) {
    return null;
  }
  if (win) {
    return a < timeline.releaseEnd ? focusIndexOf(session) : null;
  }
  // Losing flavors: only grip-release actually lifts the prize (briefly).
  return flavor === "grip-release" && a < timeline.liftEnd ? focusIndexOf(session) : null;
};

/** World position of prize `index` (rest, or riding the claw when carried). */
export const prizeWorldPosition = (
  spec: ClawGrabSpec,
  state: ClawState,
  reducedMotion: boolean,
  index: number,
): EngineVec3 => {
  const session = state.session;
  const rest = prizePosition(index, spec.prizeCount);
  const carried = carriedPrizeIndex(session, reducedMotion);
  if (carried !== index) {
    // A grip-release prize settles back down after its brief lift (eased).
    const settleBack = settleBackOf(spec, state, reducedMotion, index);
    return settleBack ?? rest;
  }
  const tip = clawTip(spec, state, reducedMotion);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const sway = (sample01(seed, "trajectory", index, 5) - 0.5) * 0.14;
  const age = revealAgeOf(session);
  const dangle = Math.sin(age * 0.22) * sway;
  return v3(tip.x + dangle, tip.y - 0.42, tip.z);
};

/** For the grip-release loss flavor, the prize's eased descent back to its bed
 * spot after the claw lets go (null when not applicable). */
const settleBackOf = (
  spec: ClawGrabSpec,
  state: ClawState,
  reducedMotion: boolean,
  index: number,
): EngineVec3 | null => {
  const session = state.session;
  if (session.committed === null || session.committed.win || index !== focusIndexOf(session)) {
    return null;
  }
  if (lossFlavorOf(session.committed.presentationSeed) !== "grip-release") {
    return null;
  }
  const timeline = clawTimeline(session.config.presentationSpeed, reducedMotion);
  const a = Math.min(Math.max(revealAgeOf(session), 0), timeline.total);
  if (a < timeline.liftEnd) {
    return null;
  }
  const rest = prizePosition(index, spec.prizeCount);
  const dropT = smoothstep(clamp01((a - timeline.liftEnd) / (timeline.carryEnd - timeline.liftEnd)));
  const liftedY = CLAW_HOVER_Y - 0.42;
  return v3(rest.x, lerp(liftedY, rest.y, dropT), rest.z);
};

// ── controller ─────────────────────────────────────────────────────────────────

export const initialClawExtra = (_session: SessionState): ClawExtra => ({ clawX: 0, clawZ: 0 });

export const stepClawGrab = (
  runtime: GameRuntime<ClawGrabSpec>,
  state: ClawState,
  input: InputFrame,
  _ctx: TickContext,
): ClawState => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;

  if (session.phase === "ready") {
    return { ...state, session: transition(session, "interacting") };
  }

  if (session.phase === "interacting") {
    const speed = spec.steerSpeed;
    const dx = (input.down.has("right") ? 1 : 0) - (input.down.has("left") ? 1 : 0);
    const dz = (input.down.has("down") ? 1 : 0) - (input.down.has("up") ? 1 : 0);
    const clawX = Math.min(Math.max(state.extra.clawX + dx * speed, -CLAW_X_LIMIT), CLAW_X_LIMIT);
    const clawZ = Math.min(Math.max(state.extra.clawZ + dz * speed, -CLAW_Z_LIMIT), CLAW_Z_LIMIT);
    const moved: ClawState = { ...state, extra: { clawX, clawZ } };
    if (input.pressed.has("primary")) {
      const targetedPrizeIndex = targetedPrizeIndexOf(spec.prizeCount, clawX, clawZ);
      return { ...moved, pendingContext: { targetedPrizeIndex }, session: transition(session, "committing") };
    }
    return moved;
  }

  if (session.phase === "revealing") {
    const timeline = clawTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Mechanism cues: gantry motor ticks while descending/carrying, and a thunk
 * when the claw closes on the prize. */
export const clawCues = (runtime: GameRuntime<ClawGrabSpec>, prev: ClawState, next: ClawState): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = clawTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  const motorPeriod = 11;
  const motoring = after <= timeline.carryEnd && Math.floor(after / motorPeriod) > Math.floor(before / motorPeriod);
  return [
    ...(motoring ? tickCue(seed, Math.floor(after / motorPeriod)) : []),
    ...(crossed(timeline.gripEnd) ? thumpCue(seed, 1) : []),
  ];
};
