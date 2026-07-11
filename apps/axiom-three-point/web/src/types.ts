/*
 * types.ts — the SDK-free shared vocabulary of the game. No behavior lives here;
 * it imports only `vec.ts`. `session.ts` produces these shapes, `scene.ts` and the
 * harness consume them.
 */

import type { Quat, Vec3 } from "./vec.ts";

/** The explicit shot state machine (spec-mandated states, no loose flags). */
export type Phase =
  | "ready"
  | "charging"
  | "releasing"
  | "ballInFlight"
  | "shotResolved"
  | "movingToNextRack"
  | "results";

/** A completed swipe shot (mobile): flick strength → release progress, sideways
 * flick → a bounded launch-yaw offset. The camera is never touched by it. */
export interface SwipeGesture {
  /** Where in the rise the flick releases (0 weak .. 1 full). */
  readonly progress: number;
  /** Launch-yaw offset from the current aim (rad, bounded). */
  readonly yawOffset: number;
}

/** One fixed tick of player input, folded from the SDK by `game.ts` (or a test script). */
export interface Intent {
  /** Space is held this tick. */
  readonly shootHeld: boolean;
  /** Space went down this tick. */
  readonly shootPressed: boolean;
  /** Space went up this tick. */
  readonly shootReleased: boolean;
  /** R went down this tick. */
  readonly restartPressed: boolean;
  /** Look delta this tick (raw pixels, +x right / +y down) — pointer-locked
   * mouse, or a touch look-drag scaled by `touchLookScale`. */
  readonly lookDx: number;
  readonly lookDy: number;
  /** A swipe shot completed this tick (touch lift-off), or null. */
  readonly swipe: SwipeGesture | null;
}

/** A resting, no-input tick (the agent/test scripts build on this). */
export const IDLE_INTENT: Intent = {
  lookDx: 0,
  lookDy: 0,
  restartPressed: false,
  shootHeld: false,
  shootPressed: false,
  shootReleased: false,
  swipe: null,
};

/** How a resolved shot ended. Exactly the HUD feedback vocabulary. */
export type ShotOutcome = "swish" | "made" | "rim" | "backboard" | "miss";

/** Which surface the ball hit (physics contact report + impact flash placement). */
export type ContactSurface = "rim" | "backboard" | "floor" | "pole";

/**
 * The unified feedback event stream: every gameplay moment the presentation
 * layer reacts to, emitted once by the session at the exact state transition
 * (never inferred by polling). Animation (`polish.ts`), audio (`game.ts`), and
 * the DOM HUD all key off these same events.
 */
export type GameEvent =
  | { readonly kind: "ballPickupStarted"; readonly slot: number; readonly golden: boolean }
  | { readonly kind: "chargeTick"; readonly level: number }
  | { readonly kind: "ballReleased"; readonly progress: number }
  | { readonly kind: "rimHit"; readonly speed: number; readonly position: Vec3 }
  | { readonly kind: "backboardHit"; readonly speed: number; readonly position: Vec3 }
  | { readonly kind: "floorHit"; readonly speed: number; readonly seq: number }
  | { readonly kind: "basketMade"; readonly points: number; readonly streak: number; readonly golden: boolean; readonly swish: boolean; readonly entryX: number; readonly entryZ: number }
  | { readonly kind: "swishMade" }
  | { readonly kind: "shotMissed"; readonly outcome: ShotOutcome }
  | { readonly kind: "streakIncreased"; readonly streak: number }
  | { readonly kind: "streakBroken"; readonly hadStreak: number }
  | { readonly kind: "rackCompleted"; readonly station: number }
  | { readonly kind: "stationTransitionStarted"; readonly label: string }
  | { readonly kind: "stationTransitionCompleted"; readonly station: number; readonly label: string; readonly final: boolean }
  | { readonly kind: "gameCompleted"; readonly results: Results }
  | { readonly kind: "gameRestarted" };

/** A floating HUD feedback line (word + points), drained per frame. */
export interface Feedback {
  readonly kind: ShotOutcome | "points";
  readonly text: string;
  readonly big: boolean;
}

/** The final-screen summary. */
export interface Results {
  readonly score: number;
  readonly makes: number;
  readonly bestStreak: number;
  readonly label: string;
}

/** The reticle: a FIXED center crosshair marking the player's own aim line
 * (launch yaw = camera yaw). It is never repositioned by the game — only the
 * player's look moves it, because it moves with the camera. */
export interface ReticleView {
  /** active (ball in hand / rising) · dim (ball away) · hidden (glide/results). */
  readonly mode: "hidden" | "active" | "dim";
}

/** The HUD snapshot the harness renders each frame. */
export interface Hud {
  readonly phase: Phase;
  readonly score: number;
  readonly streak: number;
  /** 0-based current rack. */
  readonly rackIndex: number;
  /** Balls not yet shot at the current rack (5..0). */
  readonly ballsLeft: number;
  /** True while the ball in hand (or next up) is the golden fifth ball. */
  readonly golden: boolean;
  /** Shot-motion progress (0..1) while holding, else −1 (meter hidden). */
  readonly motion: number;
  /** True once the motion is held at the top of the rise (late territory). */
  readonly atTop: boolean;
  readonly reticle: ReticleView;
  /** Which rack the transition is gliding toward (HUD line), else undefined. */
  readonly movingToLabel: string | undefined;
  /** Arrival label ("CENTER RACK" / "RIGHT RACK"), held briefly, else null. */
  readonly stationLabel: string | null;
  /** The points just awarded (+3/+6/…); `seq` retriggers the pooled element. */
  readonly award: { readonly points: number; readonly seq: number } | null;
  /** Streak presentation level: 0 (0–1), 1 (=2), 2 (=3), 3 (≥4). */
  readonly streakLevel: number;
  /** Screen-edge warmth 0..1 (smoothed; only nonzero at streakLevel 3). */
  readonly glow: number;
  /** Increment counters that retrigger the streak grow / collapse animations. */
  readonly streakPulseSeq: number;
  readonly streakBrokenSeq: number;
  readonly results: Results | undefined;
  /** Feedback floaters to spawn this frame (drained). */
  readonly events: readonly Feedback[];
}

/** A brief emissive impact flash at a contact point. */
export interface ImpactFlash {
  readonly position: Vec3;
  /** 1 → 0 decay. */
  readonly strength: number;
}

/** One visible basketball (held or airborne). */
export interface BallView {
  readonly position: Vec3;
  readonly orientation: Quat;
  readonly golden: boolean;
  /** Vertical squash multiplier (1 = spherical; < 1 for a frame after a hard
   * floor hit). Visual only — the collider never deforms. */
  readonly squash: number;
  /** Recent positions, oldest → newest, hard-capped by POLISH_TUNING (golden
   * balls carry a longer trail; ordinary balls only when moving fast). */
  readonly trail: readonly Vec3[];
  /** True during a golden-ball glint window. */
  readonly glint: boolean;
}

/** The net's deterministic reaction state (drives the strand/ring pose). */
export interface NetView {
  /** Downward stretch 0..1 (sharp on a swish, softer on a rimmed make). */
  readonly drop: number;
  /** Outward flare 0..1. */
  readonly flare: number;
  /** Sideways displacement (m) near the crossed section. */
  readonly lateralX: number;
  readonly lateralZ: number;
}

/** Everything the 3D scene needs to pose this frame — pure data, no SDK types.
 * All reaction fields come from the session's `polish.ts` state (event-driven,
 * deterministic); the tick is included only so IDLE presentation cycles (crowd
 * bob, banner sway, glints) can loop without polling game state. */
export interface SceneView {
  readonly tick: number;
  readonly cameraPosition: Vec3;
  readonly cameraTarget: Vec3;
  /** True for each of the 15 rack slots still holding its ball (rack·5 + slot). */
  readonly rackFilled: readonly boolean[];
  /** The ball currently in the shooter's hands (picking up / settled / rising). */
  readonly heldBall: BallView | null;
  /** Every launched ball still on the court — several can fly at once. */
  readonly flying: readonly BallView[];
  /** Visual displacement of the rim (damped vibration; collider unaffected). */
  readonly rimOffset: Vec3;
  /** Visual displacement of the backboard (short shake; collider unaffected). */
  readonly boardOffset: Vec3;
  /** The net's reaction pose. */
  readonly net: NetView;
  /** Rim/backboard impact flash, or null. */
  readonly impact: ImpactFlash | null;
  /** Current station's rack-frame dip (m, downward) and per-slot settle offsets. */
  readonly rackDip: number;
  readonly slotSettle: readonly number[];
  /** Crowd reaction pulse 0..1 (made baskets lift it; idles decay it). */
  readonly crowdPulse: number;
  /** Live scoreboard values (mirrors the HUD; drawn as 7-segment digits). */
  readonly score: number;
  readonly streak: number;
  /** Predicted trajectory dots (DEBUG_TRAJECTORY only, else empty). */
  readonly preview: readonly Vec3[];
}
