/*
 * types.ts — the SDK-free vocabulary the gameplay core, the session state machine,
 * and the scene's read-only view all share. No behavior lives here; these are the
 * plain shapes that flow between the pure logic (`swing.ts` / `pitch.ts` /
 * `fielders.ts` / `ball.ts` / `session.ts`) and the one engine-facing file
 * (`scene.ts`). Imports only `vec.ts`.
 */

import type { Vec3 } from "./vec.ts";

/** The round state machine. */
export type Phase = "ready" | "windup" | "pitch" | "flight" | "result" | "over";

/** The arcade outcome of one pitch. A taken pitch outside the zone is a `ball`. */
export type Outcome = "miss" | "ball" | "foul" | "weak" | "grounder" | "popup" | "clean" | "homer";

/** The bat's always-armed swing state machine (rewind = the swing cooldown). */
export type SwingState = "ready" | "swing" | "follow" | "rewind";

/** One tick's folded input. `moveX` is in WORLD sign (+X = batter's side / screen-left). */
export interface Intent {
  /** Lateral step intent: -1, 0, +1 (world sign). */
  readonly moveX: number;
  /** The swing press EDGE — fires the full-power swing when the batter is ready. */
  readonly swing: boolean;
  /** A discrete press this tick (starts the round / restarts from `over`). */
  readonly start: boolean;
}

/** The bat's live pose + cooldown state (one immutable snapshot per tick). */
export interface Swing {
  readonly state: SwingState;
  /** Bat sweep angle θ (see constants.ts for the frame). */
  readonly theta: number;
  /** Current angular velocity (rad/tick); nonzero only in swing/follow. */
  readonly omega: number;
  /** Rewind progress 0…1 — 1 means the batter is wound and ready to swing. */
  readonly readiness: number;
  /** Ticks spent in the current state. */
  readonly stateTicks: number;
}

/** Everything a bat-ball contact resolves to. */
export interface Contact {
  /** Contact point radius along the bat (grip→tip, world units from the pivot). */
  readonly r: number;
  /** Normalized position along the hittable segment, 0 handle … 1 tip. */
  readonly u: number;
  /** Sweet-spot / timing / vertical sub-qualities and their blend (0…1). */
  readonly sweetQ: number;
  readonly timingQ: number;
  readonly vertQ: number;
  readonly quality: number;
  /** Exit velocity, world units per TICK. */
  readonly exitVel: Vec3;
  /** Exit speed, u/s (pre-loft, horizontal reference for classification). */
  readonly exitSpeed: number;
  /** Horizontal spray angle: 0 = dead center, + = pull side (+X), |>45°| = foul. */
  readonly spray: number;
  /** Launch loft in radians. */
  readonly loft: number;
  /** World point of contact. */
  readonly point: Vec3;
}

/** One selected, jittered pitch. */
export interface PitchSpec {
  readonly profileId: string;
  readonly name: string;
  /** Ball speed toward the plate, u/s (after jitter). */
  readonly speed: number;
  readonly gravity: number;
  readonly targetX: number;
  readonly targetY: number;
  /** Display speed. */
  readonly mph: number;
}

/** A fielder's live state (positions are world XZ). */
export interface FielderState {
  x: number;
  z: number;
  /** True while reacting toward a projected landing point. */
  chasing: boolean;
}

/** One buffered feedback event, drained each tick by `game.ts` (HUD text + audio). */
export interface Feedback {
  readonly kind:
    | "windup"
    | "release"
    | "contact"
    | "caught"
    | "fielded"
    | Outcome;
  readonly text: string;
  readonly big: boolean;
}

/** The per-pitch log entry (drives the results HUD and the replay tests). */
export interface PitchResult {
  readonly outcome: Outcome;
  readonly points: number;
  readonly distance: number;
  readonly mph: number;
  readonly caught: boolean;
}

/**
 * The read-only snapshot the session hands `scene.ts` each frame — everything the
 * 3D presentation needs, and nothing the presentation could use to mutate gameplay.
 */
export interface SceneView {
  readonly phase: Phase;
  readonly tick: number;
  /** Batter lateral position and the bat pose. */
  readonly batterX: number;
  readonly swing: Swing;
  /** Ball world position; hidden between pitches. */
  readonly ball: Vec3;
  readonly ballVisible: boolean;
  readonly ballInPlay: boolean;
  /** Recent ball positions while in play (bounded), for the hit trail. */
  readonly trail: readonly Vec3[];
  /** Machine wind-up compression 0…1 and muzzle flash 0…1. */
  readonly windup: number;
  readonly muzzleFlash: number;
  /** Fielders in FIELDER_SPOTS order. */
  readonly fielders: readonly { readonly x: number; readonly z: number; readonly chasing: boolean }[];
  /** The animated camera (already composed: base + dolly + punch + follow + shake). */
  readonly cameraPos: Vec3;
  readonly cameraTarget: Vec3;
  /** Result flash intensity 0…1 (scene pulse on strong contact). */
  readonly impactFlash: number;
  /** True during frozen hit-stop ticks (scene may pop the ball slightly). */
  readonly hitStop: boolean;
}
