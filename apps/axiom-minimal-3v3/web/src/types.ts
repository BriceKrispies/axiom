/*
 * types.ts — the shared SDK-free vocabulary: the per-tick input `Intent`, the
 * read-only `SceneView` the session hands the scene, and the small unions the state
 * machine speaks. `scene.ts` consumes these; `session.ts` produces them; neither
 * side imports the other's internals.
 */

import type { Vec3 } from "./vec.ts";

/** The game phases. `reset` is an action (see `Mini3v3Session.reset`), not a phase. */
export type Phase = "playing" | "shooting" | "shotResult" | "turnoverResult";

export type BallState = "held" | "pass" | "shot" | "dead";

export type TimingTag = "early" | "good" | "perfect" | "late";

export type ResultKind = "made" | "miss" | "stolen" | "intercepted";

/**
 * One tick of device-agnostic input. The SDK edge detection (pressed / released)
 * happens in `game.ts`; tests script plain `Intent` streams straight into the session.
 */
export interface Intent {
  /** World-x movement, -1..1 (already sign-corrected for the screen in game.ts). */
  readonly moveX: number;
  /** World-z movement, -1..1 (+1 = toward the hoop). */
  readonly moveZ: number;
  /** Space went down this tick — starts the shot gather + jump. */
  readonly gatherPressed: boolean;
  /** Space is currently held. */
  readonly gatherHeld: boolean;
  /** Space went up this tick — releases the shot. */
  readonly gatherReleased: boolean;
  /** Q — pass to the teammate on the player's left (screen left). */
  readonly passLeft: boolean;
  /** E — pass to the teammate on the player's right (screen right). */
  readonly passRight: boolean;
  /** R — manual reset. */
  readonly reset: boolean;
}

/** Everything the scene needs to pose one humanoid figure. */
export interface FigureView {
  readonly pos: Vec3;
  /** Facing, radians around +Y; 0 faces +z (toward the hoop). */
  readonly yaw: number;
  /** Forward lean in the facing frame, -1..1. */
  readonly leanF: number;
  /** Sideways lean in the facing frame, -1..1. */
  readonly leanS: number;
  /** Shot-gather crouch, 0..1. */
  readonly crouch: number;
  /** Shooting-arm raise, 0..1. */
  readonly armRaise: number;
  /** Jump height offset (already in meters). */
  readonly jumpY: number;
  /** Idle bob offset (meters). */
  readonly bobY: number;
}

/** The read-only per-tick snapshot the scene renders. */
export interface SceneView {
  readonly phase: Phase;
  readonly tick: number;
  readonly blues: readonly [FigureView, FigureView, FigureView];
  readonly defenders: readonly [FigureView, FigureView, FigureView];
  readonly controlledIndex: 0 | 1 | 2;
  readonly ball: Vec3;
  /** The controlled player's position — what the follow camera tracks. */
  readonly cameraAnchor: Vec3;
  /** Set during shotResult / turnoverResult. */
  readonly resultKind: ResultKind | undefined;
  /** True on the exact tick the possession was reset — the camera snap cue. */
  readonly justReset: boolean;
}

/** A quadratic bezier flight path (pass or shot). */
export interface ShotArc {
  readonly start: Vec3;
  readonly control: Vec3;
  readonly end: Vec3;
}
