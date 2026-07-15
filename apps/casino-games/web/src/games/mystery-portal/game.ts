/*
 * game.ts — the Mystery Portal controller: mechanic, portal layout, idle-pose
 * math, reveal timeline, and per-tick step. Three floating portals hover in a
 * row; the player steps through one and a brief white-out swaps in a small
 * reward vignette behind it. The choice-population adapter preassigned every
 * portal's contents at session start.
 *
 * The idle pose (bob / breathe / shimmer phase) is a pure function of
 * (index, tick, root seed, liveliness) drawing only from the AMBIENT stream —
 * it takes no population input at all, so no hover or shimmer can hint at
 * what waits behind a portal. The idle-independence test pins this.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { clamp01 } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import type { ChoiceCore } from "../choice-input.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";

export interface PortalSpec {
  /** Idle ring-shimmer liveliness in [0, 1]. */
  readonly shimmerLiveliness: number;
}

export interface PortalExtra {
  readonly choice: ChoiceCore;
}

export type PortalState = CasinoState<PortalExtra>;

export const PORTAL_MIN_CHOICES = 3;
export const PORTAL_MAX_CHOICES = 6;
export const PORTAL_DEFAULT_CHOICES = 3;

/** Clamp a configured choice count into this game's supported range. */
export const portalChoiceCountOf = (raw: number | undefined): number =>
  Math.min(PORTAL_MAX_CHOICES, Math.max(PORTAL_MIN_CHOICES, Math.round(raw ?? PORTAL_DEFAULT_CHOICES)));

export const portalChoiceCount = (session: SessionState): number => portalChoiceCountOf(session.config.choiceCount);

/** Each slot's DISTINCT visual identity — ring bead density and edge shape —
 * fixed tables by index (they describe the portal, never its contents). */
export const PORTAL_BEADS: readonly number[] = [12, 9, 15, 10, 13, 8];
export const PORTAL_EDGES: readonly ("bead" | "diamond")[] = ["bead", "diamond", "bead", "diamond", "bead", "diamond"];

/** Portal centers: a gentle arc, floating above the stage floor. */
export const portalPosition = (index: number, count: number): EngineVec3 => {
  const offset = index - (count - 1) / 2;
  return v3(offset * 2.35, 1.6, -Math.abs(offset) * 0.35);
};

export const portalCamera = (count: number): ReturnType<typeof showcaseCamera> =>
  showcaseCamera(v3(0, 1.55, 0), 4.6 + count * 0.5, 0.4, 0.85);

export const portalTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: portalPosition(index, count),
    index,
    radiusPx: 88,
  }));

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface PortalRevealTimeline {
  /** The chosen portal has expanded and its shimmer has settled. */
  readonly approachEnd: number;
  /** The white-out disc reaches full cover (the vignette swaps in behind it). */
  readonly whiteoutPeak: number;
  /** The white-out has receded, unveiling the vignette. */
  readonly whiteoutEnd: number;
  /** The reward vignette has fully risen. */
  readonly vignetteEnd: number;
  readonly total: number;
}

export const portalTimeline = (presentationSpeed: number, reducedMotion: boolean): PortalRevealTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const approachEnd = t(26);
  const whiteoutPeak = approachEnd + t(12);
  const whiteoutEnd = whiteoutPeak + t(12);
  const vignetteEnd = whiteoutEnd + t(34);
  return { approachEnd, total: vignetteEnd + t(8), vignetteEnd, whiteoutEnd, whiteoutPeak };
};

// ── idle pose (AMBIENT stream only — takes no population input at all) ────────

export interface PortalIdlePose {
  /** Vertical bob offset in world units. */
  readonly bob: number;
  /** Ring/disc size-breathing multiplier around 1. */
  readonly breathe: number;
  /** Shimmer phase offset the beads twinkle around. */
  readonly shimmerPhase: number;
}

/** The idle pose of portal `index` at `tick`: pure in (index, tick, seed,
 * liveliness). Winner data is not an input, so idle motion cannot leak it. */
export const portalIdlePose = (index: number, tick: number, seed: number, liveliness: number): PortalIdlePose => {
  const window = Math.floor(tick / 110);
  const amp = sample01(seed, "ambient", 40 + index, window);
  return {
    bob: Math.sin((tick / 78) * Math.PI * 2 + index * 1.9) * (0.05 + 0.05 * amp) * (0.35 + 0.65 * liveliness),
    breathe: 1 + Math.sin((tick / 64) * Math.PI * 2 + index * 1.3) * 0.035 * (0.4 + 0.6 * liveliness),
    shimmerPhase: amp * Math.PI * 2,
  };
};

/** The reveal-focus camera pull strength: exactly 0 until a selection exists
 * and the reveal has begun, then easing in over the approach. */
export const portalFocusT = (session: SessionState, selected: number | null, reducedMotion: boolean): number => {
  if (selected === null) {
    return 0;
  }
  if (session.phase === "revealing") {
    const timeline = portalTimeline(session.config.presentationSpeed, reducedMotion);
    return clamp01(phaseAge(session) / timeline.approachEnd);
  }
  return session.phase === "celebrating" || session.phase === "complete" ? 1 : 0;
};

// ── the controller ──────────────────────────────────────────────────────────────

export const initialPortalExtra = (_session: SessionState): PortalExtra => ({ choice: initialChoice(1) });

/** Per-tick controller: selection commits; the reveal advances on the shared
 * timeline and hands off to "celebrating" when the vignette settles. */
export const stepPortal = (
  runtime: GameRuntime<PortalSpec>,
  state: PortalState,
  input: InputFrame,
  _ctx: TickContext,
): PortalState => {
  const session = state.session;
  const count = portalChoiceCount(session);

  if (session.phase === "ready") {
    // A single row of portals: the grid is one row wide, so left/right walk it.
    const result = stepChoice(state.extra.choice, input, portalCamera(count), portalTargets(count), count);
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
    const timeline = portalTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Portal-mechanism cues: a shimmer as the ring stabilizes and another at the
 * white-out peak (the win/loss fanfare is played centrally by the harness). */
export const portalCues = (
  prev: PortalState,
  next: PortalState,
  reducedMotion: boolean,
  shimmer: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = portalTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(timeline.approachEnd) ? shimmer(seed, 20) : []),
    ...(crossed(timeline.whiteoutPeak) ? shimmer(seed, 21) : []),
  ];
};
