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

/** A sound the session asks the platform edge to play (drained per frame). */
export type AudioCue =
  | { readonly kind: "charge"; readonly level: number }
  | { readonly kind: "release" }
  | { readonly kind: "contact"; readonly surface: ContactSurface; readonly speed: number }
  | { readonly kind: "score"; readonly swish: boolean; readonly streak: number }
  | { readonly kind: "miss" }
  | { readonly kind: "transition" }
  | { readonly kind: "results" };

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
}

/** Everything the 3D scene needs to pose this frame — pure data, no SDK types. */
export interface SceneView {
  readonly cameraPosition: Vec3;
  readonly cameraTarget: Vec3;
  /** True for each of the 15 rack slots still holding its ball (rack·5 + slot). */
  readonly rackFilled: readonly boolean[];
  /** The ball currently in the shooter's hands (picking up / settled / rising). */
  readonly heldBall: BallView | null;
  /** Every launched ball still on the court — several can fly at once. */
  readonly flying: readonly BallView[];
  /** Golden-ball trail positions, oldest → newest (empty unless golden in flight). */
  readonly trail: readonly Vec3[];
  /** Net gather pulse 0..1 (made shot). */
  readonly netPulse: number;
  /** Rim/backboard impact flash, or null. */
  readonly impact: ImpactFlash | null;
  /** Predicted trajectory dots (DEBUG_TRAJECTORY only, else empty). */
  readonly preview: readonly Vec3[];
}
