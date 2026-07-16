/*
 * session.ts — `HomeRunSession`, the framework-free heart of the game. It owns one
 * explicit mutable state, advances it exactly one deterministic tick per
 * `advance(intent)`, and folds the pure modules (`swing.ts`, `pitch.ts`,
 * `fielders.ts`, `ball.ts`) into the round state machine
 * (`ready → windup → pitch → flight → result → … → over`). It imports NOTHING
 * from the engine, so the whole game is constructible and replayable in a bare
 * `node --test`; the pure `view.ts` reads its `view()` snapshot and `game.ts` its
 * HUD accessors. A session is the immutable STATE the pure `game.update` advances:
 * `update` calls `clone()` and advances the clone, never the original. All variation
 * derives from the constructor seed via `hash01`.
 */

import { type Vec3, add, clamp, clamp01, lerp, scale, vec3 } from "./vec.ts";
import {
  type BatterPosition,
  type CinematicState,
  type Feedback,
  type FielderState,
  type Intent,
  type Outcome,
  type Phase,
  type PitchFlightState,
  type PitchResult,
  type PitchSpec,
  type SceneView,
  type Swing,
  type SwingOutcome,
} from "./types.ts";
import { newSwing, stepSwing } from "./swing.ts";
import { isStrike, pitchGapTicks, selectPitch, solvePitch } from "./pitch.ts";
import { catchingFielder, newFielders, projectLanding, stepFielders } from "./fielders.ts";
import { type BallFlight, beyondWall, classifyCaught, classifyFlight, newFlight, scoreFor, stepFlight } from "./ball.ts";
import { evaluateSwingOutcome } from "./swing-outcome.ts";
import { HOME_RUN_CINEMATIC_TUNING } from "./cinematic-constants.ts";
import { enterCinematicPhase, newCinematic, stepCinematic } from "./cinematic.ts";
import { cinematicFovY, contactCameraPose, groundTrackingCameraPose, groundTrackingZoomTarget } from "./cinematic-camera.ts";
import * as C from "./constants.ts";

const TRAIL_MAX = 14;
const HIDDEN_BALL: Vec3 = vec3(0, -100, 0);

const OUTCOME_TEXT: Record<Outcome, string> = {
  ball: "BALL",
  clean: "CLEAN HIT",
  foul: "FOUL",
  grounder: "GROUNDER",
  homer: "HOME RUN!",
  miss: "MISS",
  popup: "POP UP",
  weak: "WEAK HIT",
};

export class HomeRunSession {
  readonly #seed: number;

  // Round + clock.
  #phase: Phase = "ready";
  #tick = 0;
  #phaseTicks = 0;
  #pitchIndex = 0;
  #results: PitchResult[] = [];

  // Score.
  #score = 0;
  #homers = 0;
  #streak = 0;
  #bestDist = 0;

  // Batter + bat.
  #batterX = C.BATTER_START_X;
  #swing: Swing = newSwing();
  #swungThisPitch = false;

  // The live pitch (machine → plate).
  #spec: PitchSpec | undefined;
  #gap = 0;
  #ballPos: Vec3 = HIDDEN_BALL;
  #ballVel: Vec3 = vec3(0, 0, 0);
  #pitchGravity = 0;
  #ballLive = false;
  /** Where this pitch crossed the plate plane (z = 0), for the ball/strike call. */
  #plateCross: { readonly x: number; readonly y: number } | undefined;

  // The ball in play (post-contact).
  #flight: BallFlight | undefined;
  #trail: Vec3[] = [];

  // Fielders.
  #fielders: FielderState[];

  // Feel + camera animation state (all deterministic).
  #hitStop = 0;
  #muzzleFlash = 0;
  #punchTicks = 0;
  #shakeTicks = 0;
  #shakeTotal = 1;
  #shakeMag = 0;
  #followBlend = 0;
  #impactFlash = 0;
  #resultDuration = C.RESULT_TICKS;

  #events: Feedback[] = [];
  /** Only the feedback emitted during the CURRENT `advance` — reset each tick, so a
   * pure caller can read exactly this tick's cues (audio + HUD) without draining. */
  #tickEvents: Feedback[] = [];
  #lastMph = 0;
  #lastPitchName = "";

  // Home-run cinematic: the authoritative prediction, the cinematic's own phase
  // machine, the gameplay-tick clock it's measured against, and the fractional
  // accumulator that dilates gameplay-mutating ticks (never the presentation ones).
  #swingOutcome: SwingOutcome | undefined;
  #swingCommitSimTick = 0;
  /** Increments once per GATED gameplay tick (never per real tick) — the clock
   * `#swingOutcome.contactTick` is measured against, so slow motion never shifts
   * WHEN the precomputed contact lands relative to the swing that predicted it. */
  #simTick = 0;
  /** Starts at 1 so the very first real tick always runs a gameplay step. */
  #simAccum = 1;
  #cinematic: CinematicState = newCinematic();
  #cinematicCamPos: Vec3 = vec3(0, 0, 0);
  #cinematicCamTarget: Vec3 = vec3(0, 0, 0);
  /** The ground-tracking camera's own zoom-in blend (0…1, eased) — separate from
   * `#cinematic.zoom` (the contact zoom-in/out) so the two can overlap: zoomed in
   * for contact, back out as the ball climbs, in again a bit as it falls. */
  #groundCameraZoom = 0;

  public constructor(seed = 1) {
    this.#seed = seed | 0;
    this.#fielders = newFielders(this.#seed);
  }

  /** Advance exactly one fixed tick. */
  public advance(intent: Intent): void {
    this.#tick += 1;
    this.#tickEvents = [];

    // The cinematic DIRECTOR's own clock always runs — camera/letterbox/zoom
    // timing must never depend on how fast the underlying simulation is
    // advancing (a slowed gameplay tick must not stall the shot itself). The
    // camera pose is recomputed here too, EVERY real tick, not just on the
    // (much rarer, during heavy slow motion) gated gameplay ticks below —
    // otherwise the blend progress eases smoothly while its target snaps
    // forward only once every few real ticks, reading as stutter exactly when
    // the shot should be at its smoothest. Everything else — presentation
    // decay (shake/flash/punch), fielders, the bat, the ball — genuinely
    // slows down with the rest of the game; see the gated block below.
    if (this.#cinematic.phase !== "none") {
      this.#cinematic = stepCinematic(this.#cinematic, HOME_RUN_CINEMATIC_TUNING);
      this.#updateCinematicCamera();
    }

    // Hit-stop: the impact freeze — ball, bat and fielders hold for a few ticks.
    if (this.#hitStop > 0) {
      this.#hitStop -= 1;
      return;
    }
    this.#phaseTicks += 1;

    // Time dilation: EVERYTHING below — presentation decay, fielders, the bat,
    // the ball — only fires once this fractional accumulator crosses 1,
    // advancing by the cinematic's current time scale each real tick. Outside
    // a cinematic, timeScale is always exactly 1, so this fires every real
    // tick — zero behavior change for ordinary play. No physics constant
    // (gravity, etc.) is ever altered to produce slow motion; only how often a
    // real tick is allowed to run the game forward.
    this.#simAccum += this.#cinematic.timeScale;
    if (this.#simAccum < 1) {
      return;
    }
    this.#simAccum -= 1;
    this.#simTick += 1;
    this.#decayFeel();

    // Batter repositioning (only while a pitch can still be met, and never mid-swing
    // — once committed, the swing and its precomputed outcome can't be nudged).
    if ((this.#phase === "ready" || this.#phase === "windup" || this.#phase === "pitch") && this.#swing.state !== "swing") {
      this.#batterX = clamp(this.#batterX + intent.moveX * C.BATTER_STEP_SPEED, C.BATTER_MIN_X, C.BATTER_MAX_X);
    }

    // The swing machine runs in every live phase (practice swings included).
    const prevSwingState = this.#swing.state;
    if (this.#phase !== "over") {
      this.#swing = stepSwing(this.#swing, intent.swing);
      if (this.#swing.state === "swing" && (this.#phase === "pitch" || this.#phase === "windup")) {
        this.#swungThisPitch = true;
      }
    }
    // The instant the swing commits, lock in the authoritative outcome for the
    // whole swing — the real hit AND the cinematic both consume this exact record;
    // nothing about the actual hit is ever recomputed separately later.
    if (prevSwingState === "ready" && this.#swing.state === "swing") {
      this.#commitSwing();
    }

    switch (this.#phase) {
      case "ready":
        stepFielders(this.#fielders, this.#seed, this.#simTick, null);
        if (intent.start || intent.swing) {
          this.#beginPitch();
        }
        break;
      case "windup":
        stepFielders(this.#fielders, this.#seed, this.#simTick, null);
        if (this.#phaseTicks >= this.#gap + C.WINDUP_TICKS) {
          this.#releasePitch();
        }
        break;
      case "pitch":
        stepFielders(this.#fielders, this.#seed, this.#simTick, null);
        this.#stepPitch();
        break;
      case "flight":
        this.#stepFlight();
        break;
      case "result":
        stepFielders(this.#fielders, this.#seed, this.#simTick, null);
        if (this.#cinematic.phase === "landing" && this.#cinematic.phaseTicks >= HOME_RUN_CINEMATIC_TUNING.landingCameraDurationTicks) {
          this.#cinematic = enterCinematicPhase(this.#cinematic, "celebration");
        }
        if (this.#phaseTicks >= this.#resultDuration) {
          this.#nextPitchOrOver();
        }
        break;
      case "over":
        stepFielders(this.#fielders, this.#seed, this.#simTick, null);
        if (intent.start) {
          this.reset();
        }
        break;
      default:
        break;
    }
  }

  /** The instant a swing commits (ready → swing), lock in the authoritative
   * `SwingOutcome` for this whole swing — a one-shot deterministic prediction the
   * real per-tick resolution below simply applies at the predicted tick. */
  #commitSwing(): void {
    const pitchState: PitchFlightState = { gravityPerTick: this.#pitchGravity, pos: this.#ballPos, vel: this.#ballVel };
    const batterState: BatterPosition = { x: this.#batterX, z: C.BATTER_Z };
    this.#swingOutcome = evaluateSwingOutcome(this.#swing, pitchState, batterState, HOME_RUN_CINEMATIC_TUNING);
    this.#swingCommitSimTick = this.#simTick;
    if (this.#swingOutcome.isHomeRun) {
      this.#cinematic = enterCinematicPhase(newCinematic(), "anticipation");
      this.#emit({ big: false, kind: "cinematicAnticipation", text: "" });
    }
  }

  /** Restart the round from scratch (same seed → the same replayable round). */
  public reset(): void {
    this.#phase = "ready";
    this.#phaseTicks = 0;
    this.#pitchIndex = 0;
    this.#results = [];
    this.#score = 0;
    this.#homers = 0;
    this.#streak = 0;
    this.#bestDist = 0;
    this.#batterX = C.BATTER_START_X;
    this.#swing = newSwing();
    this.#swungThisPitch = false;
    this.#spec = undefined;
    this.#gap = 0;
    this.#ballPos = HIDDEN_BALL;
    this.#ballVel = vec3(0, 0, 0);
    this.#pitchGravity = 0;
    this.#ballLive = false;
    this.#plateCross = undefined;
    this.#flight = undefined;
    this.#trail = [];
    this.#fielders = newFielders(this.#seed);
    this.#hitStop = 0;
    this.#muzzleFlash = 0;
    this.#punchTicks = 0;
    this.#shakeTicks = 0;
    this.#shakeMag = 0;
    this.#followBlend = 0;
    this.#impactFlash = 0;
    this.#events = [];
    this.#lastMph = 0;
    this.#lastPitchName = "";
    // The cinematic can never survive a restart — clears mid-anticipation, mid-
    // slow-motion, mid-letterbox, or after a lost focus/pointer-lock alike.
    this.#swingOutcome = undefined;
    this.#swingCommitSimTick = 0;
    this.#simTick = 0;
    this.#simAccum = 1;
    this.#cinematic = newCinematic();
    this.#cinematicCamPos = vec3(0, 0, 0);
    this.#cinematicCamTarget = vec3(0, 0, 0);
    this.#groundCameraZoom = 0;
  }

  // ── pitch lifecycle ─────────────────────────────────────────────────────────

  #beginPitch(): void {
    this.#phase = "windup";
    this.#phaseTicks = 0;
    this.#swungThisPitch = false;
    this.#spec = selectPitch(this.#seed, this.#pitchIndex);
    this.#gap = pitchGapTicks(this.#seed, this.#pitchIndex);
    this.#ballPos = HIDDEN_BALL;
    this.#ballLive = false;
    this.#plateCross = undefined;
    this.#trail = [];
    // A new pitch always starts with a clean cinematic slate — belt-and-suspenders
    // against any prior pitch's cinematic ever bleeding into this one.
    this.#swingOutcome = undefined;
    this.#cinematic = newCinematic();
    this.#simAccum = 1;
    this.#groundCameraZoom = 0;
    this.#emit({ big: false, kind: "windup", text: "" });
  }

  #releasePitch(): void {
    const spec = this.#spec!;
    const solved = solvePitch(spec);
    this.#ballPos = C.PITCH_RELEASE;
    this.#ballVel = solved.vel;
    this.#pitchGravity = solved.gravityPerTick;
    this.#ballLive = true;
    this.#lastMph = spec.mph;
    this.#lastPitchName = spec.name;
    this.#muzzleFlash = C.FLASH_TICKS;
    this.#punchTicks = C.CAMERA_PUNCH_TICKS;
    this.#phase = "pitch";
    this.#phaseTicks = 0;
    this.#emit({ big: false, kind: "release", text: `${spec.mph} MPH` });
  }

  #stepPitch(): void {
    const prevBall = this.#ballPos;
    this.#ballVel = vec3(this.#ballVel.x, this.#ballVel.y - this.#pitchGravity, this.#ballVel.z);
    this.#ballPos = add(this.#ballPos, this.#ballVel);

    // Record the plate crossing (interpolated at z = 0) for the ball/strike call.
    if (this.#plateCross === undefined && prevBall.z > 0 && this.#ballPos.z <= 0) {
      const f = prevBall.z / (prevBall.z - this.#ballPos.z);
      this.#plateCross = {
        x: prevBall.x + (this.#ballPos.x - prevBall.x) * f,
        y: prevBall.y + (this.#ballPos.y - prevBall.y) * f,
      };
    }

    // Apply the swing's PRECOMPUTED contact at its predicted tick — resolved once,
    // deterministically, the instant the swing committed (see `#commitSwing`), not
    // re-probed reactively here.
    const outcome = this.#swingOutcome;
    if (this.#swing.state === "swing" && outcome !== undefined && outcome.contactOccurs && this.#simTick - this.#swingCommitSimTick === outcome.contactTick) {
      this.#beginFlight(outcome);
      return;
    }
    // Past the plate untouched. Swinging at anything is a MISS; a take is
    // umpired at the plate crossing — in the zone it's a STRIKE, off the
    // plate it's a BALL.
    if (this.#ballPos.z <= C.CATCHER_Z) {
      this.#ballLive = false;
      this.#ballPos = HIDDEN_BALL;
      const cross = this.#plateCross;
      const took = !this.#swungThisPitch;
      const wasBall = took && (cross === undefined || !isStrike(cross.x, cross.y));
      if (wasBall) {
        this.#resolve("BALL", "ball", 0, false);
        return;
      }
      this.#resolve(took ? "STRIKE" : "MISS", "miss", 0, false);
    }
  }

  #beginFlight(outcome: SwingOutcome): void {
    this.#flight = newFlight(outcome.contactPoint, outcome.exitVelocity, outcome.exitSpeed, outcome.launchAngle, outcome.spray);
    this.#ballPos = outcome.contactPoint;
    this.#ballLive = true;
    this.#phase = "flight";
    this.#phaseTicks = 0;
    this.#trail = [];
    const quality = outcome.contactQuality;
    this.#emit({ big: quality > 0.8 || outcome.isHomeRun, kind: "contact", text: "" });
    this.#impactFlash = Math.round(6 + 10 * quality);
    if (outcome.isHomeRun) {
      // The cinematic's own brief authored hold replaces the quality-based
      // hit-stop formula — a short, tunable beat, never long enough to feel
      // unresponsive (see `HOME_RUN_CINEMATIC_TUNING.impactHoldDurationTicks`).
      this.#hitStop = HOME_RUN_CINEMATIC_TUNING.impactHoldDurationTicks;
      // A tiny camera impulse at contact — NOT the bigger wall-crossing
      // `SHAKE_HOMER` crescendo below, which still fires unchanged in `#stepFlight`.
      this.#shake(HOME_RUN_CINEMATIC_TUNING.cameraShakeStrength, HOME_RUN_CINEMATIC_TUNING.cameraShakeDurationTicks);
      this.#cinematic = enterCinematicPhase(this.#cinematic, "contact");
    } else if (quality >= C.HIT_STOP_QUALITY) {
      this.#hitStop = C.HIT_STOP_BASE_TICKS + Math.round(C.HIT_STOP_MAX_EXTRA * clamp01((quality - C.HIT_STOP_QUALITY) / (1 - C.HIT_STOP_QUALITY)));
      this.#shake(C.SHAKE_CONTACT * (0.5 + quality), C.SHAKE_TICKS);
    }
  }

  #stepFlight(): void {
    const b = this.#flight!;
    const wasHomer = b.homer;
    const landing = projectLanding(b.pos, b.vel, C.GRAVITY / (C.FIXED_HZ * C.FIXED_HZ));
    stepFielders(this.#fielders, this.#seed, this.#simTick, b.foul ? null : landing);

    const done = stepFlight(b);
    this.#ballPos = b.pos;
    this.#trail.push(b.pos);
    if (this.#trail.length > TRAIL_MAX) {
      this.#trail.shift();
    }

    // The home-run moment: the instant the ball clears the wall.
    if (!wasHomer && b.homer) {
      this.#shake(C.SHAKE_HOMER, C.SHAKE_TICKS_HOMER);
    }
    // Ball-follow camera for genuinely long hits — the ORDINARY partial follow.
    // A cinematic home run uses its own full ball-follow camera instead (below).
    const long = !b.foul && b.exitSpeed > 20 && b.pos.z > 12;
    this.#followBlend = clamp01(this.#followBlend + (long ? C.CAMERA_FOLLOW_RATE : -C.CAMERA_FOLLOW_RATE));
    if (this.#followBlend > C.CAMERA_FOLLOW_MAX) {
      this.#followBlend = C.CAMERA_FOLLOW_MAX;
    }

    // Once the ball has clearly separated from the bat, hand the cinematic off
    // from the low contact camera to the ground-tracking shot.
    if (this.#cinematic.phase === "contact" && this.#cinematic.phaseTicks >= HOME_RUN_CINEMATIC_TUNING.contactSlowMotionDurationTicks) {
      this.#cinematic = enterCinematicPhase(this.#cinematic, "ballFollow");
      this.#emit({ big: false, kind: "crowdErupt", text: "" });
    }

    // Fielders: catch in the air, or field a grounded ball (never a homer).
    if (!b.homer) {
      const who = catchingFielder(this.#fielders, b.pos);
      if (who >= 0) {
        const outcome = classifyCaught(b);
        const dist = Math.hypot(b.pos.x, b.pos.z);
        const caughtAir = b.bounces === 0 && !b.foul;
        this.#resolve(caughtAir ? "CAUGHT!" : "FIELDED", outcome, dist, true);
        return;
      }
    }

    if (done) {
      const outcome = classifyFlight(b);
      const dist = outcome === "homer" ? Math.hypot(b.pos.x, b.pos.z) : b.firstLandDist > 0 ? Math.max(b.firstLandDist, Math.hypot(b.pos.x, b.pos.z)) : Math.hypot(b.pos.x, b.pos.z);
      if (this.#cinematic.phase !== "none") {
        // No new camera move here — "landing"/"celebration" just HOLD wherever
        // the ground-tracking camera was already frozen (the moment the ball
        // left the park), through the "HOME RUN!" hold and the confetti, until
        // `camBlend` eases back to the ordinary gameplay camera in celebration.
        this.#cinematic = enterCinematicPhase(this.#cinematic, "landing");
      }
      this.#resolve(OUTCOME_TEXT[outcome], outcome, dist, false);
    }
  }

  #resolve(text: string, outcome: Outcome, distance: number, caught: boolean): void {
    const dist = Math.round(distance);
    this.#streak = outcome === "homer" ? this.#streak + 1 : 0;
    const points = scoreFor(outcome, dist, this.#streak);
    this.#score += points;
    if (outcome === "homer") {
      this.#homers += 1;
    }
    if (outcome !== "miss" && outcome !== "ball" && outcome !== "foul" && !caught) {
      this.#bestDist = Math.max(this.#bestDist, dist);
    }
    this.#results.push({ caught, distance: dist, mph: this.#lastMph, outcome, points });

    const suffix = points > 0 ? ` +${points}` : "";
    const streakTag = outcome === "homer" && this.#streak > 1 ? ` ×${Math.min(this.#streak, C.STREAK_MULT_CAP)}` : "";
    this.#emit({ big: outcome === "homer", kind: outcome, text: `${text}${suffix}${streakTag}` });

    this.#flight = undefined;
    this.#resultDuration = outcome === "homer" ? C.HOMER_RESULT_TICKS : C.RESULT_TICKS;
    this.#phase = "result";
    this.#phaseTicks = 0;
  }

  #nextPitchOrOver(): void {
    this.#pitchIndex += 1;
    this.#ballPos = HIDDEN_BALL;
    this.#ballLive = false;
    this.#trail = [];
    if (this.#pitchIndex >= C.PITCHES_PER_ROUND) {
      this.#phase = "over";
      this.#phaseTicks = 0;
      return;
    }
    this.#beginPitch();
  }

  // ── feel + camera ───────────────────────────────────────────────────────────

  #shake(mag: number, ticks: number): void {
    this.#shakeMag = Math.max(this.#shakeMag, mag);
    this.#shakeTicks = Math.max(this.#shakeTicks, ticks);
    this.#shakeTotal = Math.max(this.#shakeTicks, 1);
  }

  #decayFeel(): void {
    this.#muzzleFlash = Math.max(0, this.#muzzleFlash - 1);
    this.#punchTicks = Math.max(0, this.#punchTicks - 1);
    this.#impactFlash = Math.max(0, this.#impactFlash - 1);
    if (this.#shakeTicks > 0) {
      this.#shakeTicks -= 1;
      if (this.#shakeTicks === 0) {
        this.#shakeMag = 0;
      }
    }
    if (this.#phase !== "flight") {
      this.#followBlend = clamp01(this.#followBlend - C.CAMERA_FOLLOW_RATE);
    }
  }

  #shakeOffset(): Vec3 {
    if (this.#shakeTicks <= 0) {
      return vec3(0, 0, 0);
    }
    const decay = this.#shakeTicks / this.#shakeTotal;
    const m = this.#shakeMag * decay;
    return vec3(Math.sin(this.#simTick * 2.9) * m, Math.cos(this.#simTick * 2.3) * m * 0.6, 0);
  }

  #windupProgress(): number {
    if (this.#phase !== "windup" || this.#phaseTicks < this.#gap) {
      return 0;
    }
    const w = clamp01((this.#phaseTicks - this.#gap) / C.WINDUP_TICKS);
    return w * w * (3 - 2 * w);
  }

  /** Recompute the cinematic director's own camera pose for THIS tick (fresh
   * batter/ball transforms) — a mutable field, not something `view()` computes,
   * so a "stop tracking" moment can genuinely STOP updating (freeze in place)
   * regardless of how many times `view()` itself is called between ticks. */
  #updateCinematicCamera(): void {
    const tuning = HOME_RUN_CINEMATIC_TUNING;
    const batter: BatterPosition = { x: this.#batterX, z: C.BATTER_Z };

    // "landing"/"celebration" never move the camera at all — they simply hold
    // wherever the ground-tracking camera was already frozen when the ball
    // left the park (below), rather than cutting to a different shot.
    if (this.#cinematic.phase === "landing" || this.#cinematic.phase === "celebration") {
      return;
    }

    if (this.#cinematic.phase === "ballFollow") {
      // Stop tracking the instant the ball leaves the ballpark: hold whatever
      // pose and zoom were last computed rather than continuing to chase it.
      if (beyondWall(this.#ballPos.x, this.#ballPos.z)) {
        return;
      }
      const pose = groundTrackingCameraPose(batter, this.#ballPos, tuning);
      this.#cinematicCamPos = pose.position;
      this.#cinematicCamTarget = pose.target;
      const zoomTarget = groundTrackingZoomTarget(this.#ballVel, tuning);
      const zoomRate = tuning.cinematicCameraBlendDurationTicks > 0 ? 1 / tuning.cinematicCameraBlendDurationTicks : 1;
      this.#groundCameraZoom =
        zoomTarget > this.#groundCameraZoom
          ? Math.min(zoomTarget, this.#groundCameraZoom + zoomRate)
          : Math.max(zoomTarget, this.#groundCameraZoom - zoomRate);
      return;
    }

    // anticipation / contact — the low camera, never zoomed by the ground tracker.
    const pose = contactCameraPose(batter, tuning);
    this.#cinematicCamPos = pose.position;
    this.#cinematicCamTarget = pose.target;
    this.#groundCameraZoom = 0;
  }

  // ── read-only snapshots ─────────────────────────────────────────────────────

  /** The full scene snapshot for `scene.ts` (presentation only — cannot mutate play). */
  public view(): SceneView {
    const windup = this.#windupProgress();
    const dolly = windup * C.CAMERA_WINDUP_DOLLY + (this.#punchTicks / C.CAMERA_PUNCH_TICKS) * C.CAMERA_RELEASE_PUNCH;
    const shake = this.#shakeOffset();
    const gameplayCameraPos = add(add(C.CAMERA_POS, vec3(0, 0, dolly)), shake);
    const followTarget =
      this.#followBlend > 0 && this.#ballLive ? lerp(C.CAMERA_TARGET, this.#ballPos, this.#followBlend) : C.CAMERA_TARGET;
    const gameplayCameraTarget = add(followTarget, scale(shake, 0.5));

    // Blend from the ordinary gameplay camera toward the cinematic director's
    // pose — 0 for every ordinary pitch/swing, so this is a pure no-op then.
    const camBlend = this.#cinematic.phase === "none" ? 0 : this.#cinematic.camBlend;
    const cameraPos = camBlend > 0 ? lerp(gameplayCameraPos, this.#cinematicCamPos, camBlend) : gameplayCameraPos;
    const cameraTarget = camBlend > 0 ? lerp(gameplayCameraTarget, this.#cinematicCamTarget, camBlend) : gameplayCameraTarget;

    return {
      ball: this.#ballPos,
      ballInPlay: this.#flight !== undefined,
      ballVisible: this.#ballLive,
      batterX: this.#batterX,
      // Contact's zoom-in and the ground-tracking camera's descent zoom-in are
      // independent blends that can overlap; combine and cap at the same
      // `cinematicZoomAmount` ceiling either would use alone.
      cameraFovY: cinematicFovY(clamp01(this.#cinematic.zoom + this.#groundCameraZoom), HOME_RUN_CINEMATIC_TUNING),
      cameraPos,
      cameraTarget,
      cinematicPhase: this.#cinematic.phase,
      debugCounters: { impactParticles: this.#cinematic.impactParticles, trailSegments: this.#trail.length },
      fielders: this.#fielders.map((f) => ({ chasing: f.chasing, x: f.x, z: f.z })),
      hitStop: this.#hitStop > 0,
      hudVisible: this.#cinematic.letterbox < 0.5,
      impactFlash: clamp01(this.#impactFlash / 12),
      letterboxProgress: this.#cinematic.letterbox,
      muzzleFlash: clamp01(this.#muzzleFlash / C.FLASH_TICKS),
      phase: this.#phase,
      swing: this.#swing,
      // The GATED tick, not the real one — every tick-driven presentation
      // oscillation `view.ts` builds from this (fielder bob, machine blink)
      // slows down along with the rest of gameplay during a cinematic.
      tick: this.#simTick,
      trail: this.#trail,
      windup,
    };
  }

  /** Drain and clear the buffered feedback events (HUD text + audio cues). */
  public drainEvents(): readonly Feedback[] {
    const out = this.#events;
    this.#events = [];
    return out;
  }

  #emit(event: Feedback): void {
    this.#events.push(event);
    this.#tickEvents.push(event);
    if (this.#events.length > 8) {
      this.#events.shift();
    }
  }

  /** The feedback emitted during the most recent `advance` (this tick's cues). */
  public get tickEvents(): readonly Feedback[] {
    return this.#tickEvents;
  }

  /**
   * A deep-enough copy for pure stepping: a fresh session that shares nothing
   * MUTABLE with this one, so `clone().advance(intent)` never disturbs the
   * original. Only the in-place-mutated containers (results, trail, events,
   * fielders, the live flight) need fresh copies; every other field is either a
   * primitive or an immutable value the sim REPLACES rather than mutates, so a
   * shared reference is safe. This is what lets the game's `update` be a pure
   * function over an immutable `HomeRunSession` state.
   */
  public clone(): HomeRunSession {
    const c = new HomeRunSession(this.#seed);
    c.#phase = this.#phase;
    c.#tick = this.#tick;
    c.#phaseTicks = this.#phaseTicks;
    c.#pitchIndex = this.#pitchIndex;
    c.#results = [...this.#results];
    c.#score = this.#score;
    c.#homers = this.#homers;
    c.#streak = this.#streak;
    c.#bestDist = this.#bestDist;
    c.#batterX = this.#batterX;
    c.#swing = this.#swing;
    c.#swungThisPitch = this.#swungThisPitch;
    c.#spec = this.#spec;
    c.#gap = this.#gap;
    c.#ballPos = this.#ballPos;
    c.#ballVel = this.#ballVel;
    c.#pitchGravity = this.#pitchGravity;
    c.#ballLive = this.#ballLive;
    c.#plateCross = this.#plateCross;
    c.#flight = this.#flight === undefined ? undefined : { ...this.#flight };
    c.#trail = [...this.#trail];
    c.#fielders = this.#fielders.map((f) => ({ ...f }));
    c.#hitStop = this.#hitStop;
    c.#muzzleFlash = this.#muzzleFlash;
    c.#punchTicks = this.#punchTicks;
    c.#shakeTicks = this.#shakeTicks;
    c.#shakeTotal = this.#shakeTotal;
    c.#shakeMag = this.#shakeMag;
    c.#followBlend = this.#followBlend;
    c.#impactFlash = this.#impactFlash;
    c.#resultDuration = this.#resultDuration;
    c.#events = [...this.#events];
    c.#tickEvents = [...this.#tickEvents];
    // Cinematic fields are always REPLACED wholesale (never mutated in place — see
    // `#commitSwing`/`stepCinematic`/`#updateCinematicCamera`), so a shared
    // reference is exactly as safe here as it is for `#swing`/`#spec` above.
    c.#swingOutcome = this.#swingOutcome;
    c.#swingCommitSimTick = this.#swingCommitSimTick;
    c.#simTick = this.#simTick;
    c.#simAccum = this.#simAccum;
    c.#cinematic = this.#cinematic;
    c.#cinematicCamPos = this.#cinematicCamPos;
    c.#cinematicCamTarget = this.#cinematicCamTarget;
    c.#groundCameraZoom = this.#groundCameraZoom;
    c.#lastMph = this.#lastMph;
    c.#lastPitchName = this.#lastPitchName;
    return c;
  }

  // HUD accessors (read each frame by game.ts → the DOM overlay).
  public get phase(): Phase {
    return this.#phase;
  }
  public get score(): number {
    return this.#score;
  }
  public get homers(): number {
    return this.#homers;
  }
  public get streak(): number {
    return this.#streak;
  }
  public get streakMultiplier(): number {
    return Math.min(Math.max(1, this.#streak), C.STREAK_MULT_CAP);
  }
  public get bestDistance(): number {
    return this.#bestDist;
  }
  /** 1-based pitch number, clamped to the round length for display. */
  public get pitchNumber(): number {
    return Math.min(this.#pitchIndex + 1, C.PITCHES_PER_ROUND);
  }
  public get lastMph(): number {
    return this.#lastMph;
  }
  public get lastPitchName(): string {
    return this.#lastPitchName;
  }
  public get batterX(): number {
    return this.#batterX;
  }
  public get swing(): Swing {
    return this.#swing;
  }
  public get results(): readonly PitchResult[] {
    return this.#results;
  }

  /** A cheap bounded digest of the observable state (replay-equality tests). */
  public hash(): number {
    const phaseIndex = ["ready", "windup", "pitch", "flight", "result", "over"].indexOf(this.#phase);
    const fields = [
      this.#tick,
      Math.round(this.#batterX * 1000),
      Math.round(this.#swing.theta * 1000),
      Math.round(this.#ballPos.x * 100),
      Math.round(this.#ballPos.y * 100),
      Math.round(this.#ballPos.z * 100),
      this.#score,
      this.#homers,
      this.#streak,
      this.#pitchIndex,
      phaseIndex,
    ];
    return fields.reduce((h, f) => (Math.imul(h, 1_000_003) + (f | 0)) % 2_147_483_647, 2_166_136_261);
  }
}
