/*
 * cinematic-constants.ts — the ONE tuning object for the home-run cinematic,
 * `HOME_RUN_CINEMATIC_TUNING`, plus the `CinematicPhase` gate values it drives.
 * Nothing outside this file hardcodes a cinematic number; `swing-outcome.ts`,
 * `cinematic.ts`, and `cinematic-camera.ts` all read from here. Field geometry
 * (foul angle, wall distance/height, gravity) is NOT duplicated — it's read
 * straight from the shared `constants.ts` so the trajectory projection and the
 * real ball physics can never drift apart.
 */

import * as C from "./constants.ts";

export const HOME_RUN_CINEMATIC_TUNING = {
  // ── field geometry the trajectory projection classifies against (shared, not duplicated) ──
  /** Fair-territory half-angle (radians) — beyond this off center is foul. */
  foulLineHalfAngle: C.FOUL_ANGLE,
  outfieldWallDistance: C.WALL_LINE,
  outfieldWallHeight: C.WALL_HEIGHT,
  gravity: C.GRAVITY,

  // ── trajectory prediction (evaluateSwingOutcome) ──
  /** Sim ticks per post-contact projection step (1 = the real per-tick `stepFlight` rate). */
  trajectoryPredictionStepTicks: 1,
  /** Hard cap on post-contact projection steps — bounds the worst-case prediction cost. */
  maxPredictionSteps: 300,
  /** Hard cap on the pre-contact swing/pitch forward-simulation search. */
  swingContactSearchMaxTicks: 240,

  // ── cinematic timeline (all in ticks at the base 60 Hz sim rate) ──
  /** Target perceived anticipation lead — a pacing goal for the slow-motion ramp, not a
   * hard delay: anticipation always starts the instant a home-run swing commits, however
   * many real ticks that leaves before the (authoritative, already-fixed) contact tick. */
  preContactCinematicLeadTicks: 40,
  // Camera/letterbox transitions ease over roughly half a second of REAL ticks
  // (not gameplay ticks) — slow and readable, never a snap.
  cinematicCameraBlendDurationTicks: 30,
  contactSlowMotionScale: 0.2,
  contactSlowMotionDurationTicks: 40,
  postContactSlowMotionScale: 0.55,
  timeScaleRecoveryDurationTicks: 55,
  impactHoldDurationTicks: 5,
  letterboxEntranceDurationTicks: 32,
  letterboxExitDurationTicks: 38,
  /** Each bar's max screen-height share at full scrunch (10–14% per side). */
  letterboxScreenFraction: 0.12,
  /** Cinematic zoom, 0…1 — blended into a narrower `cameraFovY`. */
  cinematicZoomAmount: 0.22,

  // ── low-angle contact camera (offsets from the batter's transform) ──
  // `C.CAMERA_NEAR` is a generous 3.5 units (the canvas2d backend's depth-fog cue
  // keys on NDC z, so it can't be shrunk without fogging the whole scene — see
  // constants.ts). A close-up low-angle camera MUST still clear that near plane
  // or its own subject (the batter/bat/ball) is clipped into invisibility — these
  // offsets keep the camera-to-target distance comfortably beyond it.
  lowCameraLateralOffset: 3.0,
  lowCameraHeight: 0.5,
  lowCameraBackwardOffset: 2.6,
  lowCameraLookAtHeight: 1.1,

  // ── ground-tracking camera (offsets from the batter's transform) ──
  // Planted on the ground behind the batter — it does NOT chase the ball, it
  // pivots in place to keep pointing at it. Comfortably beyond `C.CAMERA_NEAR`
  // (3.5) from the earliest ball-follow tick onward (the ball has already
  // separated from the bat by the time this camera takes over from contact).
  groundCameraLateralOffset: 1.2,
  groundCameraHeight: 1.8,
  groundCameraBackwardOffset: 4.5,
  /** How far (0…1, added to the contact zoom, both feeding `cinematicFovY`) the
   * shot zooms in once the ball starts falling — "a bit," not the full amount. */
  groundCameraDescentZoomAmount: 0.5,

  // The camera never moves for "landing"/"celebration" — it just holds wherever
  // the ground-tracking camera was already frozen. This is a pure pacing
  // duration (how long "landing" holds before "celebration" starts), not a
  // camera parameter.
  landingCameraDurationTicks: 55,

  // ── bounded effects ──
  /** Matches `harness.ts`'s existing DOM confetti burst size — not re-created here. */
  confettiMaxCount: 36,
  impactParticleMaxCount: 10,
  impactFlashDurationTicks: 10,
  cameraShakeStrength: 0.16,
  cameraShakeDurationTicks: 18,
} as const;

export type HomeRunCinematicTuning = typeof HOME_RUN_CINEMATIC_TUNING;
