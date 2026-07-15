/*
 * game.ts — Lucky Lanterns controller. Destination mechanic: the tier color
 * BANDS in the twilight sky are the destination slots; one gameplay draw
 * commits which band the released lantern belongs to, before it leaves the
 * platform. The lantern then rises on a continuous height curve that ENDS
 * inside the committed band's height range (never a snap), while a procedural
 * wind sway — drawn from the TRAJECTORY stream only — drifts it sideways. The
 * sway cannot touch the outcome: the band is already committed.
 */

import type { InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { DestinationSlot } from "../../chance-engine/probability/destination.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, tickCue } from "../../presentation/audio/cues.ts";
import { easeOutCubic, lerp } from "../../presentation/stage/easing.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** One sky color band. `tierId` null is the peaceful "drift away" band. */
export interface LanternBand {
  readonly label: string;
  readonly tierId: string | null;
  /** Relative mass within its winning/losing group. Must be > 0. */
  readonly mass: number;
}

export interface LanternSpec {
  readonly bands: readonly LanternBand[];
}

/** The default sky: a very rare gold jackpot, a rare rose, a mint, a common
 * sky band, and a pale band where the lantern simply drifts away. */
export const DEFAULT_LANTERN_BANDS: readonly LanternBand[] = [
  { label: "drift away", mass: 3, tierId: null },
  { label: "evening sky", mass: 3, tierId: "common" },
  { label: "mint glow", mass: 2, tierId: "uncommon" },
  { label: "rose light", mass: 1, tierId: "rare" },
  { label: "golden crown", mass: 0.4, tierId: "jackpot" },
];

export const destinationSlotsOf = (spec: LanternSpec): readonly DestinationSlot[] =>
  spec.bands.map((band, index) => ({ id: `${index}:${band.label}`, mass: band.mass, tierId: band.tierId }));

// ── sky geography ────────────────────────────────────────────────────────────────

export const PLATFORM_Y = 1.1;
export const RISE_BASE = 3.4;
export const RISE_TOP = 11.4;

export interface BandRange {
  readonly low: number;
  readonly high: number;
  readonly center: number;
}

/** The vertical slice band `index` occupies (stacked bottom → top). */
export const bandRange = (count: number, index: number): BandRange => {
  const slice = (RISE_TOP - RISE_BASE) / Math.max(1, count);
  const low = RISE_BASE + index * slice;
  const high = low + slice;
  return { center: (low + high) / 2, high, low };
};

/** The committed band index (destination), or 0 before commitment. */
export const committedBandIndex = (session: SessionState): number => {
  const m = session.committed?.manifestation;
  return m !== undefined && m.kind === "destination" ? m.destinationIndex : 0;
};

// ── the reveal timeline ─────────────────────────────────────────────────────────

export interface RiseTimeline {
  readonly riseEnd: number;
  readonly brightenEnd: number;
  readonly blossomEnd: number;
  readonly settleEnd: number;
  readonly total: number;
}

export const riseTimeline = (presentationSpeed: number, reducedMotion: boolean): RiseTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const riseEnd = t(96);
  const brightenEnd = riseEnd + t(20);
  const blossomEnd = brightenEnd + t(26);
  const settleEnd = blossomEnd + t(18);
  return { blossomEnd, brightenEnd, riseEnd, settleEnd, total: settleEnd + t(6) };
};

/** Fraction of the rise completed at reveal age `age` (eased, clamped 0..1). */
export const riseFraction = (tl: RiseTimeline, age: number): number => easeOutCubic(age / tl.riseEnd);

/**
 * The lantern's height at reveal age `age`: a continuous eased climb from the
 * platform to the committed band's center, then a hold. Always ends inside the
 * band's [low, high] range (it ends AT the center).
 */
export const lanternHeightAt = (range: BandRange, tl: RiseTimeline, age: number): number =>
  lerp(PLATFORM_Y, range.center, riseFraction(tl, age));

/**
 * Procedural wind sway (horizontal drift) at reveal age `age`, drawn ONLY from
 * the TRAJECTORY stream keyed by the presentation seed. Two summed sines with
 * seed-varied phase/frequency read as a soft breeze; because it is trajectory,
 * not gameplay, it can never move the committed band.
 */
export const lanternSwayAt = (presentationSeed: number, age: number): number => {
  const phase0 = sample01(presentationSeed, "trajectory", 0) * Math.PI * 2;
  const phase1 = sample01(presentationSeed, "trajectory", 1) * Math.PI * 2;
  const amp = 0.35 + sample01(presentationSeed, "trajectory", 2) * 0.3;
  const ramp = Math.min(1, age / 40);
  return (Math.sin(age * 0.045 + phase0) * 0.7 + Math.sin(age * 0.021 + phase1) * 0.3) * amp * ramp;
};

// ── controller ──────────────────────────────────────────────────────────────────

export interface LanternExtra {
  /** Ticks the release breath has been held (0 = idle). */
  readonly breathTicks: number;
}

export const initialLanternExtra = (_session: SessionState): LanternExtra => ({ breathTicks: 0 });

export type LanternState = CasinoState<LanternExtra>;

export const stepLantern = (
  runtime: GameRuntime<LanternSpec>,
  state: LanternState,
  input: InputFrame,
  _ctx: TickContext,
): LanternState => {
  const session = state.session;

  if (session.phase === "ready") {
    const holding = input.down.has("primary") || (input.pointer?.down ?? false);
    const released = input.pressed.has("primary") || (state.extra.breathTicks > 0 && !holding);
    if (released) {
      return { ...state, pendingContext: {}, session: transition(session, "committing") };
    }
    if (holding) {
      return { ...state, extra: { breathTicks: state.extra.breathTicks + 1 } };
    }
    return state;
  }

  if (session.phase === "revealing") {
    const tl = riseTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= tl.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Reveal-mechanism cues: soft ticks as the lantern climbs past band edges, a
 * shimmer when it settles into its band (win/loss fanfare is played centrally). */
export const lanternCues = (
  reducedMotion: boolean,
  prev: LanternState,
  next: LanternState,
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const tl = riseTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const bandCount = session.mechanicPlan.kind === "destination" ? session.mechanicPlan.slots.length : 1;
  const stepMark = tl.riseEnd / Math.max(1, bandCount);
  const crossedStep = Math.floor(after / stepMark) > Math.floor(before / stepMark) && after <= tl.riseEnd;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossedStep ? tickCue(seed, Math.round(after)) : []),
    ...(crossed(tl.brightenEnd) ? shimmerCue(seed, 31) : []),
  ];
};
