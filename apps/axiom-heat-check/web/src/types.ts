/*
 * types.ts — the SDK-free vocabulary the gameplay core, the session state machine,
 * and the scene's read-only view all share. No behavior lives here; these are the
 * plain shapes that flow between the pure logic (`gameplay.ts` / `session.ts`) and
 * the one engine-facing file (`scene.ts`). Imports only `vec.ts`.
 */

import type { Vec3 } from "./vec.ts";

/** The round state machine. */
export type Phase = "ready" | "playing" | "shooting" | "scoredFeedback" | "gameOver";

/** How a shot resolved, in ascending quality. */
export type ShotResult = "miss" | "make" | "swish";

/** The dominant deficit that shapes a miss's ARC (make/swish are perfect/clean). */
export type ShotReason = "perfect" | "clean" | "contested" | "offBalance" | "early" | "late" | "forced";

/** The three separate pre-release readiness axes shown by the meter. */
export type SpaceTag = "smothered" | "contested" | "open" | "broken";
export type RhythmTag = "early" | "good" | "perfect" | "late";
export type BalanceTag = "moving" | "set" | "planted";

/** The three readiness tags + the raw quality (for the compact meter bar). */
export interface Readiness {
  readonly space: SpaceTag;
  readonly rhythm: RhythmTag;
  readonly balance: BalanceTag;
  readonly quality: number;
}

/**
 * The fully-explainable shot-quality breakdown: every component is 0..1 (higher =
 * better) EXCEPT `pressurePenalty` (higher = worse), and `quality` is their weighted
 * blend. Computed every frame while holding (for the readiness meter) and reused at
 * release — the same numbers the player reads are the numbers that decide the shot.
 */
export interface ShotBreakdown {
  readonly advantage: number;
  readonly separation: number;
  readonly timing: number;
  readonly stability: number;
  readonly shotSelection: number;
  readonly heatBonus: number;
  readonly pressurePenalty: number;
  readonly quality: number;
}

/** The class of a floating feedback event (the space labels + the heat cues). */
export type FeedbackKind = SpaceTag | "offBalance" | "forced" | "heatup" | "heatcheck" | "double";

/** One buffered feedback event, drained by the DOM HUD each frame. */
export interface Feedback {
  readonly kind: FeedbackKind;
  readonly text: string;
  /** Bigger, brighter treatment (swish / heat check). */
  readonly big: boolean;
}

/**
 * This tick's folded input under the floating dribble-stick model. The player never
 * touches the ball directly: `stickX` is a lateral *movement intent* (-1..1, already
 * in world sign) from the thumb's horizontal displacement off its anchor (or the
 * keyboard axis); `holding` is true while a control press is active (drives movement +
 * the rhythm meter); `released` is the release edge that *shoots* (gated by a minimum
 * hold so micro-taps don't fire); `shoot` is an explicit keyboard shot (Space) that
 * bypasses the hold gate; `reset` runs the round back from `gameOver`. Shot direction
 * is decided by game state + quality, never by finger aim.
 */
export interface Intent {
  readonly stickX: number;
  readonly holding: boolean;
  readonly released: boolean;
  readonly shoot: boolean;
  readonly reset: boolean;
}

/** A deterministic quadratic shot arc: sampled `start → control(apex) → end`. */
export interface ShotArc {
  readonly start: Vec3;
  readonly control: Vec3;
  readonly end: Vec3;
  readonly result: ShotResult;
}

/**
 * The read-only snapshot the session hands `scene.ts` each frame — everything the 3D
 * presentation needs, and nothing the presentation could use to mutate gameplay.
 */
export interface SceneView {
  readonly phase: Phase;
  readonly playerX: number;
  /** Lateral lean, roughly -1..1 from lateral velocity. */
  readonly playerLean: number;
  /** Shot-pose progress 0..1 (plant → rise → release → follow-through); 0 when dribbling. */
  readonly shotPose: number;
  readonly defenderX: number;
  readonly defenderBalance: number;
  /** Current defender contest-zone radius (world units) — shrinks when off balance. */
  readonly contestRadius: number;
  /** Live advantage 0..1 (the transient edge from beating the defender). */
  readonly advantage: number;
  /** True while a real advantage window is open (for the open-window scene cue). */
  readonly windowActive: boolean;
  /** Ball world position (dribbling near the player, or riding the arc in flight). */
  readonly ball: Vec3;
  readonly ballInFlight: boolean;
  readonly heat: number;
  /** Player heat-glow intensity 0..1 (0 below heat 2). */
  readonly glow: number;
  /** Crowd/court pulse 0..1 (swish flash decay + high-heat shimmer + final window). */
  readonly pulse: number;
  /** Rhythm meter phase 0..1 and whether it should be shown. */
  readonly rhythmPhase: number;
  readonly rhythmActive: boolean;
  /** Recent ball positions during flight (bounded), for the shot trail. */
  readonly trail: readonly Vec3[];
  /** Hoop make-flash intensity 0..1. */
  readonly scoreFlash: number;
  /** Small additive camera shake on a make. */
  readonly cameraShake: Vec3;
  readonly finalWindow: boolean;
}
