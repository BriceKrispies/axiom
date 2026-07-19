/*
 * types.ts — the SDK-free vocabulary the gameplay core, the session state machine,
 * and the scene's read-only view all share. No behavior lives here; these are the
 * plain shapes that flow between the pure logic (`swing.ts` / `pitch.ts` /
 * `fielders.ts` / `ball.ts` / `session.ts`) and the one engine-facing file
 * (`view.ts`). Imports only `vec.ts`.
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
  /** Cumulative planar distance walked — drives the procedural walk gait's phase. */
  traveled: number;
  /** Heading (yaw) the fielder is walking, held while nearly stopped. */
  facing: number;
  /** This tick's planar speed (u/s), from actual displacement. */
  speed: number;
  /** When set (during a force play), the base the fielder covers — it walks onto
   * this point to take the throw, overriding its normal spot/chase target. */
  cover?: { readonly x: number; readonly z: number };
}

/** One buffered feedback event, drained each tick by `game.ts` (HUD text + audio). */
export interface Feedback {
  readonly kind:
    | "windup"
    | "release"
    | "contact"
    | "caught"
    | "fielded"
    | "cinematicAnticipation"
    | "crowdErupt"
    | "baseHit"
    | "runScored"
    | "out"
    | "doublePlay"
    | Outcome;
  readonly text: string;
  readonly big: boolean;
}

/** A base runner as the scene sees it: where the figure stands/runs, which way it
 * faces, how far it has run (drives the running gait), and whether it is moving
 * (a standing runner on base uses an idle pose, not the run cycle). */
export interface RunnerView {
  readonly x: number;
  readonly z: number;
  readonly facing: number;
  readonly traveled: number;
  readonly moving: boolean;
}

/** A pitch's live flight state (position, velocity, and its own gravity-per-tick) —
 * the input `evaluateSwingOutcome` forward-simulates against. Structurally what
 * `session.ts` already tracks each tick (`#ballPos`/`#ballVel`/`#pitchGravity`). */
export interface PitchFlightState {
  readonly pos: Vec3;
  readonly vel: Vec3;
  readonly gravityPerTick: number;
}

/** The batter's lateral position at the instant a swing commits — frozen for the
 * duration of the swing so prediction and the real resolved contact agree exactly. */
export interface BatterPosition {
  readonly x: number;
  readonly z: number;
}

/** Why a swing's contact is (or isn't) ruled a home run. */
export type HomeRunReason = "no-contact" | "not-fair" | "below-wall-height" | "does-not-clear-wall" | "clears-wall-fair";

/**
 * The full deterministic prediction of one swing against one pitch — the single
 * source of truth `evaluateSwingOutcome` returns. Both the REAL launched ball and
 * the home-run cinematic consume this exact record; nothing recomputes it
 * separately. Always a total, fully-populated value — when `contactOccurs` is
 * false every contact/flight field is a zeroed placeholder.
 */
export interface SwingOutcome {
  readonly contactOccurs: boolean;
  /** Ticks from swing-commit until contact (or until the pitch passes uncontacted). */
  readonly contactTick: number;
  readonly contactPoint: Vec3;
  readonly contactNormal: Vec3;
  readonly batVelocityAtContact: Vec3;
  readonly pitchVelocityAtContact: Vec3;
  readonly exitVelocity: Vec3;
  /** `length(exitVelocity)` in u/s — carried alongside the vector because `ball.ts`'s
   * outcome classification (weak/grounder/popup/clean thresholds) is speed-keyed. */
  readonly exitSpeed: number;
  /** The contact's horizontal spray angle (see `Contact.spray`) — feeds the SAME
   * fair/foul + outcome classification the real hit already used. */
  readonly spray: number;
  /** The contact's blended quality 0…1 (see `Contact.quality`) — drives hit-stop/
   * shake/"big" feedback exactly as an ordinary contact already does. */
  readonly contactQuality: number;
  readonly launchDirection: Vec3;
  readonly launchAngle: number;
  readonly projectedApex: Vec3;
  readonly projectedLanding: Vec3;
  readonly projectedDistance: number;
  readonly isFair: boolean;
  readonly isHomeRun: boolean;
  readonly homeRunReason: HomeRunReason;
}

/** The home-run cinematic's sub-phase. Live only while `phase === "flight"` (and,
 * for `"anticipation"`, the tail of `phase === "pitch"` right after a home-run
 * swing commits) — orthogonal to the round's `Phase`, which is never renamed. */
export type CinematicPhase = "none" | "anticipation" | "contact" | "ballFollow" | "landing" | "celebration";

/** The cinematic director's own live state (owned by the session, stepped once per
 * `advance()` alongside `#swing`/`#flight`). All fields are plain numbers/enums so
 * the whole thing resets by assignment (see `session.ts#reset`). */
export interface CinematicState {
  readonly phase: CinematicPhase;
  /** Ticks spent in the current cinematic phase. */
  readonly phaseTicks: number;
  /** Ticks since the cinematic began (anticipation start) — drives the global
   * slow-motion/letterbox schedule independent of phase transitions. */
  readonly elapsedTicks: number;
  /** Letterbox bar coverage, 0 (none) … 1 (max scrunch). */
  readonly letterbox: number;
  /** Current simulation time multiplier: 1 = full speed. Gameplay ticks (ball/bat/
   * fielders integration) are gated by this via the session's fractional accumulator —
   * gravity and other physics constants never change. */
  readonly timeScale: number;
  /** Camera zoom blend, 0 (normal FOV) … 1 (max cinematic zoom). */
  readonly zoom: number;
  /** Blend from the gameplay camera to the cinematic director's pose, 0…1. */
  readonly camBlend: number;
  /** A bounded, decaying count of the current impact-particle burst (contact flash). */
  readonly impactParticles: number;
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
 * The read-only snapshot the session hands the pure `view.ts` each frame — everything
 * the 3D presentation needs, and nothing the presentation could use to mutate gameplay.
 */
export interface SceneView {
  readonly phase: Phase;
  /** The GATED gameplay tick (not the real one) — every tick-driven presentation
   * oscillation `view.ts` builds from this genuinely slows during a cinematic,
   * instead of ticking at the normal rate while everything else eases down. */
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
  /** Fielders in FIELDER_SPOTS order (with the data their walk gait needs). */
  readonly fielders: readonly {
    readonly x: number;
    readonly z: number;
    readonly chasing: boolean;
    readonly facing: number;
    readonly traveled: number;
    readonly speed: number;
  }[];
  /** Base runners currently on the diamond (empty between hits with nobody on). */
  readonly runners: readonly RunnerView[];
  /** The thrown ball in flight during a defensive play (fielder → bag). */
  readonly throwBall: { readonly pos: Vec3; readonly visible: boolean };
  /** True once the batter has put a ball in play and become a base runner: the
   * plate figure + held bat are hidden (he let go of the bat and took off; the
   * lead runner IS him), and a dropped bat rests by home. */
  readonly batterRunning: boolean;
  /** The animated camera (already composed: base + dolly + punch + follow + shake). */
  readonly cameraPos: Vec3;
  readonly cameraTarget: Vec3;
  /** Result flash intensity 0…1 (scene pulse on strong contact). */
  readonly impactFlash: number;
  /** True during frozen hit-stop ticks (scene may pop the ball slightly). */
  readonly hitStop: boolean;
  /** The home-run cinematic's sub-phase (`"none"` for every ordinary pitch/swing). */
  readonly cinematicPhase: CinematicPhase;
  /** Letterbox bar coverage 0…1 — `view.ts`/the DOM edge read this directly. */
  readonly letterboxProgress: number;
  /** The camera's vertical FOV for this frame (cinematic zoom narrows it). */
  readonly cameraFovY: number;
  /** False while the HUD should hide/dim for the cinematic's wide framing. */
  readonly hudVisible: boolean;
  /** Dev-only bounded counters (trail segments, impact particles this frame). */
  readonly debugCounters: { readonly trailSegments: number; readonly impactParticles: number };
}
