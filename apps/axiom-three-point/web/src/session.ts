/*
 * session.ts — the ONE mutable state machine of the shootout, entirely SDK-free
 * (importing only vec / constants / types / gameplay / physics), so a bare
 * `node --test` process can play complete games. `advance(intent)` runs exactly one
 * deterministic 60 Hz tick of the explicit phase machine
 * (ready → charging → releasing → ballInFlight → shotResolved → movingToNextRack →
 * results); `view()` exposes a read-only scene snapshot, `hud()` the HUD snapshot,
 * `drainAudio()` the sound cues, and `hash()` a replay-equality fingerprint.
 *
 * THE SHOT IS ONE CONTINUOUS MOTION, AND THE LOOP NEVER WAITS. The moment a ball
 * is released the NEXT ball is dealt off its actual rack slot into the hands (the
 * pickup animation plays through the follow-through), while every launched ball
 * keeps flying — several can be airborne at once, each with its own basket
 * detector. "ready" means a ball is in (or moving into) the hands; holding Space
 * runs chest settle → shot rise (progress p 0→1 over the release curves in
 * `gameplay.ts`); releasing launches at that exact instant. The reticle exists
 * only while a ball is at the chest or rising — it is the true launch calculation.
 *
 * THE CAMERA IS EXCLUSIVELY MOUSE-DRIVEN. The game never rotates, nudges, eases,
 * or drifts the view — no pickup glance, no rise tilt, no follow-through motion,
 * no aim reset. The only thing the game moves is the player's POSITION during the
 * rack-to-rack glide (the spec fixes the shooting spots); orientation stays under
 * the mouse the whole way, inside soft bounds that block-not-snap at the edges.
 *
 * Scoring resolves per ball as it lands but is APPLIED in launch order, so the
 * streak formula reads exactly like sequential shots: a make awards
 * `3 + 3·streakBefore` and THEN increments the streak; a miss resets it. The
 * golden fifth ball follows the same formula. After a rack's fifth launch the
 * session waits in "ballInFlight" for every ball to resolve, takes a
 * "shotResolved" beat, then glides ("movingToNextRack") or shows "results".
 */

import { type Vec3, clamp, lerp, smoothstep, vec3 } from "./vec.ts";
import {
  type ShootingStation,
  BALLS_PER_RACK,
  DEBUG_TRAJECTORY,
  EYE_HEIGHT,
  FEEDBACK_TICKS,
  GOLDEN_BALL_INDEX,
  MOVE_TICKS,
  PREVIEW_POINTS,
  PREVIEW_STRIDE_TICKS,
  RACK_COUNT,
  SHOT_TUNING,
  STATIONS,
  TRAIL_POOL,
  TRAIL_SAMPLE_TICKS,
  aimDirection,
  rackSlotPosition,
} from "./constants.ts";
import {
  AUTO_RELEASE_TICKS,
  chestAnchor,
  classifyOutcome,
  INITIAL_DETECTION,
  launchVelocity,
  motionProgress,
  outcomeText,
  outOfBounds,
  performanceLabel,
  pointsForMake,
  risePosition,
  softBoundedTurn,
  stepDetection,
} from "./gameplay.ts";
import { type BallState, makeBall, predictTrajectory, stepBall } from "./physics.ts";
import type { AudioCue, BallView, Feedback, Hud, Intent, Phase, Results, ReticleView, SceneView, ShotOutcome } from "./types.ts";

const FEEDBACK_MAX = 8;
const CONTACT_CUE_GAP_TICKS = 6;
const IMPACT_DECAY = 0.08;
const NET_PULSE_DECAY = 0.03;

/** One launched ball still on the court. */
interface LiveBall {
  readonly seq: number;
  readonly slot: number;
  ball: BallState;
  detection: { enteredFromAbove: boolean; scored: boolean };
  touchedRim: boolean;
  touchedBackboard: boolean;
  flightTicks: number;
  outcome: ShotOutcome | undefined;
  applied: boolean;
  /** Visual linger after resolution before the ball is cleared away. */
  fadeTicks: number;
}

export class ThreePointSession {
  #phase: Phase = "ready";
  #tick = 0;
  #phaseTicks = 0;

  // Progression.
  #station = 0;
  #ballIndex = 0;
  #rackTaken: boolean[] = Array.from({ length: RACK_COUNT * BALLS_PER_RACK }, () => false);

  // Aim (the stored, player-owned orientation — presentation offsets never touch it).
  #yaw: number = STATIONS[0]!.baseYaw;
  #pitch: number = SHOT_TUNING.pitchNeutral;

  // The ball in hand.
  #handSlot = -1;
  #handTicks = 0;
  #motionTicks = 0;

  // Launched balls (several can fly at once) + in-order score application.
  #liveBalls: LiveBall[] = [];
  #launchSeq = 0;
  #applySeq = 0;
  #releaseProgress = 0;

  // Score.
  #score = 0;
  #streak = 0;
  #makes = 0;
  #bestStreak = 0;
  #lastOutcome: ShotOutcome | undefined;

  // Presentation side-channels.
  #events: Feedback[] = [];
  #audio: AudioCue[] = [];
  #netPulse = 0;
  #impact: { position: Vec3; strength: number } | null = null;
  #trail: Vec3[] = [];
  #lastContactCueTick = -1000;

  #results: Results | undefined;
  #hash = 0x811c9dc5;

  constructor() {
    this.#dealNextBall();
  }

  // ── public reads ────────────────────────────────────────────────────────────

  get phase(): Phase {
    return this.#phase;
  }

  get score(): number {
    return this.#score;
  }

  get streak(): number {
    return this.#streak;
  }

  get makes(): number {
    return this.#makes;
  }

  get bestStreak(): number {
    return this.#bestStreak;
  }

  get stationIndex(): number {
    return this.#station;
  }

  /** The rack slot of the ball currently in (or moving into) the hands. */
  get ballIndex(): number {
    return this.#ballIndex;
  }

  /** True once the dealt ball has reached the chest and Space will start a shot. */
  get ballInHand(): boolean {
    return this.#handSlot >= 0 && this.#handTicks >= SHOT_TUNING.pickupTicks;
  }

  /** Launched balls not yet resolved (several can be airborne at once). */
  get ballsInFlight(): number {
    return this.#liveBalls.filter((b) => b.outcome === undefined).length;
  }

  /** Motion progress of the shot being held (0 outside the rise). */
  get motion(): number {
    return this.#phase === "charging" ? motionProgress(this.#motionTicks) : 0;
  }

  /** The progress the last launched shot was released at. */
  get lastReleaseProgress(): number {
    return this.#releaseProgress;
  }

  /** Total shots launched so far (0..15). */
  get shotsTaken(): number {
    return this.#launchSeq;
  }

  /** The outcome of the last shot APPLIED to the score (launch order). */
  get lastOutcome(): ShotOutcome | undefined {
    return this.#lastOutcome;
  }

  get results(): Results | undefined {
    return this.#results;
  }

  get yaw(): number {
    return this.#yaw;
  }

  get pitch(): number {
    return this.#pitch;
  }

  /** Replay-equality fingerprint folded once per tick. */
  hash(): number {
    return this.#hash;
  }

  drainEvents(): readonly Feedback[] {
    const out = this.#events;
    this.#events = [];
    return out;
  }

  drainAudio(): readonly AudioCue[] {
    const out = this.#audio;
    this.#audio = [];
    return out;
  }

  // ── reset (initial construction and the R key share one code path) ──────────

  reset(): void {
    this.#phase = "ready";
    this.#tick = 0;
    this.#phaseTicks = 0;
    this.#station = 0;
    this.#ballIndex = 0;
    this.#rackTaken = this.#rackTaken.map(() => false);
    this.#yaw = STATIONS[0]!.baseYaw;
    this.#pitch = SHOT_TUNING.pitchNeutral;
    this.#handSlot = -1;
    this.#handTicks = 0;
    this.#motionTicks = 0;
    this.#liveBalls = [];
    this.#launchSeq = 0;
    this.#applySeq = 0;
    this.#releaseProgress = 0;
    this.#score = 0;
    this.#streak = 0;
    this.#makes = 0;
    this.#bestStreak = 0;
    this.#lastOutcome = undefined;
    this.#events = [];
    this.#audio = [];
    this.#netPulse = 0;
    this.#impact = null;
    this.#trail = [];
    this.#lastContactCueTick = -1000;
    this.#results = undefined;
    this.#hash = 0x811c9dc5;
    this.#dealNextBall();
  }

  // ── the tick ────────────────────────────────────────────────────────────────

  advance(intent: Intent): void {
    this.#tick += 1;

    if (intent.restartPressed) {
      this.reset();
      return;
    }

    this.#applyAim(intent);
    this.#decayEffects();
    this.#stepLiveBalls();

    if (this.#phase === "ready") {
      this.#handTicks += 1;
      // A completed swipe (mobile) IS the whole shot: flick strength decides the
      // release progress, the sideways flick offsets the launch yaw — the
      // camera/aim are untouched. Space starts the desktop rise. Both wait for
      // the dealt ball to reach the chest.
      if (intent.swipe !== null && this.ballInHand) {
        this.#launchBall(intent.swipe.progress, intent.swipe.yawOffset);
      } else if (intent.shootPressed && this.ballInHand) {
        this.#phase = "charging";
        this.#phaseTicks = 0;
        this.#motionTicks = 0;
      }
    }
    if (this.#phase === "charging") {
      this.#tickCharging(intent);
    } else if (this.#phase === "releasing") {
      this.#tickFollowThrough();
    } else if (this.#phase === "ballInFlight") {
      // The rack is empty; waiting for every airborne ball to resolve.
      if (this.ballsInFlight === 0) {
        this.#phase = "shotResolved";
        this.#phaseTicks = 0;
      }
    } else if (this.#phase === "shotResolved") {
      this.#tickRackEndBeat();
    } else if (this.#phase === "movingToNextRack") {
      this.#tickMoving();
    }

    this.#foldHash();
  }

  // ── aim ─────────────────────────────────────────────────────────────────────

  /** The mouse owns the camera in every phase. Yaw lives inside a SOFT bound
   * around the current station's hoop-facing direction: movement deeper out of
   * range is blocked, never snapped — the game itself never turns the view. */
  #applyAim(intent: Intent): void {
    const base = this.#currentStation().baseYaw;
    this.#yaw = softBoundedTurn(this.#yaw, intent.lookDx * SHOT_TUNING.aimYawSensitivity, base, SHOT_TUNING.yawClampHalf);
    this.#pitch = clamp(this.#pitch - intent.lookDy * SHOT_TUNING.aimPitchSensitivity, SHOT_TUNING.minPitch, SHOT_TUNING.maxPitch);
  }

  // ── dealing + shooting ──────────────────────────────────────────────────────

  /** Pull the next ball (slot `#ballIndex`) off the rack into the hands. */
  #dealNextBall(): void {
    this.#rackTaken[this.#station * BALLS_PER_RACK + this.#ballIndex] = true;
    this.#handSlot = this.#ballIndex;
    this.#handTicks = 0;
  }

  #tickCharging(intent: Intent): void {
    this.#phaseTicks += 1;
    this.#motionTicks += 1;
    const p = motionProgress(this.#motionTicks);
    if (p > 0 && p < 1 && this.#motionTicks % 6 === 0) {
      this.#pushAudio({ kind: "charge", level: p });
    }
    // Release at the player's instant — or auto-release after the max hold.
    if (intent.shootReleased || this.#motionTicks >= AUTO_RELEASE_TICKS) {
      this.#launchBall(p);
    }
  }

  /** Let the ball go at motion progress `p`: launch its physics and DEAL THE
   * NEXT BALL IMMEDIATELY — its pickup animation plays through the
   * follow-through. The player's aim is untouched: a swipe's `yawOffset` only
   * steers this one launch, never the stored yaw. */
  #launchBall(p: number, yawOffset = 0): void {
    const station = this.#currentStation();
    const slot = this.#handSlot;
    const launchYaw = clamp(
      this.#yaw + yawOffset,
      station.baseYaw - SHOT_TUNING.yawClampHalf,
      station.baseYaw + SHOT_TUNING.yawClampHalf,
    );
    this.#releaseProgress = p;
    const from = risePosition(station, launchYaw, slot, p);
    const launch = launchVelocity(launchYaw, p);
    this.#liveBalls.push({
      applied: false,
      ball: makeBall(from, launch.velocity, launch.angularVelocity),
      detection: INITIAL_DETECTION,
      fadeTicks: FEEDBACK_TICKS,
      flightTicks: 0,
      outcome: undefined,
      seq: this.#launchSeq,
      slot,
      touchedRim: false,
      touchedBackboard: false,
    });
    this.#launchSeq += 1;
    if (slot === GOLDEN_BALL_INDEX) this.#trail = [];
    const nextSlot = slot + 1;
    if (nextSlot < BALLS_PER_RACK) {
      this.#ballIndex = nextSlot;
      this.#dealNextBall();
    } else {
      this.#handSlot = -1;
    }
    this.#phase = "releasing";
    this.#phaseTicks = 0;
    this.#pushAudio({ kind: "release" });
  }

  /** The follow-through: the launched ball is flying and the next one is already
   * on its way to the hands. A pacing beat only — the camera is not touched. */
  #tickFollowThrough(): void {
    this.#phaseTicks += 1;
    this.#handTicks += 1;
    if (this.#phaseTicks >= SHOT_TUNING.followThroughTicks) {
      if (this.#handSlot >= 0) {
        this.#phase = "ready";
      } else {
        this.#phase = "ballInFlight";
      }
      this.#phaseTicks = 0;
    }
  }

  #tickRackEndBeat(): void {
    this.#phaseTicks += 1;
    if (this.#phaseTicks < FEEDBACK_TICKS) return;
    if (this.#station < RACK_COUNT - 1) {
      this.#phase = "movingToNextRack";
      this.#phaseTicks = 0;
      this.#pushAudio({ kind: "transition" });
    } else {
      this.#results = {
        bestStreak: this.#bestStreak,
        label: performanceLabel(this.#makes),
        makes: this.#makes,
        score: this.#score,
      };
      this.#phase = "results";
      this.#phaseTicks = 0;
      this.#pushAudio({ kind: "results" });
    }
  }

  /** The glide moves the player's POSITION to the next station (locations are
   * game-fixed by design); the view stays wherever the mouse points it. */
  #tickMoving(): void {
    this.#phaseTicks += 1;
    if (this.#phaseTicks >= MOVE_TICKS) {
      this.#station += 1;
      this.#ballIndex = 0;
      this.#dealNextBall();
      this.#phase = "ready";
      this.#phaseTicks = 0;
    }
  }

  // ── the airborne balls ──────────────────────────────────────────────────────

  /** One physics tick for every launched ball: contacts, basket detection, shot
   * resolution, in-order score application, fade-out of settled balls. */
  #stepLiveBalls(): void {
    for (const live of this.#liveBalls) {
      if (live.outcome === undefined) {
        const step = stepBall(live.ball);
        live.flightTicks += 1;
        let hitFloor = false;
        for (const contact of step.contacts) {
          if (contact.surface === "rim") live.touchedRim = true;
          if (contact.surface === "backboard") live.touchedBackboard = true;
          if (contact.surface === "floor") hitFloor = true;
          this.#reportContact(contact.surface, contact.speed, contact.position);
        }
        let scoredNow = false;
        for (const sample of step.samples) {
          const result = stepDetection(live.detection, sample);
          live.detection = result.state;
          scoredNow = scoredNow || result.scoredNow;
        }
        if (scoredNow) {
          live.outcome = classifyOutcome(true, live.touchedRim, live.touchedBackboard);
        } else if (hitFloor || outOfBounds(live.ball.pos) || live.flightTicks > SHOT_TUNING.maxShotLifetimeTicks) {
          live.outcome = classifyOutcome(false, live.touchedRim, live.touchedBackboard);
        }
      } else {
        // Resolved: keep bouncing behind the action, then clear away.
        stepBall(live.ball);
        if (live.applied) live.fadeTicks -= 1;
      }
      if (live.slot === GOLDEN_BALL_INDEX && this.#tick % TRAIL_SAMPLE_TICKS === 0 && live.fadeTicks > FEEDBACK_TICKS / 2) {
        this.#trail.push(live.ball.pos);
        if (this.#trail.length > TRAIL_POOL) this.#trail.shift();
      }
    }
    this.#applyOutcomesInOrder();
    this.#liveBalls = this.#liveBalls.filter((b) => b.fadeTicks > 0);
    if (!this.#liveBalls.some((b) => b.slot === GOLDEN_BALL_INDEX)) {
      if (this.#trail.length > 0) this.#trail.shift();
    }
  }

  /** Apply resolved outcomes strictly in launch order, so streak scoring reads
   * exactly like sequential shots even with several balls airborne. */
  #applyOutcomesInOrder(): void {
    for (;;) {
      const next = this.#liveBalls.find((b) => b.seq === this.#applySeq);
      if (next === undefined || next.outcome === undefined) return;
      this.#applySeq += 1;
      next.applied = true;
      const outcome = next.outcome;
      this.#lastOutcome = outcome;
      const made = outcome === "swish" || outcome === "made";
      if (made) {
        const points = pointsForMake(this.#streak);
        this.#score += points;
        this.#streak += 1;
        this.#makes += 1;
        this.#bestStreak = Math.max(this.#bestStreak, this.#streak);
        this.#netPulse = 1;
        this.#pushEvent({ big: outcome === "swish" || this.#streak >= 3, kind: outcome, text: outcomeText(outcome) });
        this.#pushEvent({ big: false, kind: "points", text: `+${points}` });
        this.#pushAudio({ kind: "score", streak: this.#streak, swish: outcome === "swish" });
      } else {
        this.#streak = 0;
        this.#pushEvent({ big: false, kind: outcome, text: outcomeText(outcome) });
        this.#pushAudio({ kind: "miss" });
      }
    }
  }

  // ── presentation helpers ────────────────────────────────────────────────────

  #reportContact(surface: "rim" | "backboard" | "floor" | "pole", speed: number, position: Vec3): void {
    if (surface === "rim" || surface === "backboard") {
      this.#impact = { position, strength: 1 };
    }
    if (this.#tick - this.#lastContactCueTick >= CONTACT_CUE_GAP_TICKS && speed > 0.4) {
      this.#lastContactCueTick = this.#tick;
      this.#pushAudio({ kind: "contact", speed, surface });
    }
  }

  #decayEffects(): void {
    this.#netPulse = Math.max(0, this.#netPulse - NET_PULSE_DECAY);
    if (this.#impact !== null) {
      const strength = this.#impact.strength - IMPACT_DECAY;
      this.#impact = strength <= 0 ? null : { position: this.#impact.position, strength };
    }
  }

  #pushEvent(event: Feedback): void {
    if (this.#events.length < FEEDBACK_MAX) this.#events.push(event);
  }

  #pushAudio(cue: AudioCue): void {
    if (this.#audio.length < FEEDBACK_MAX) this.#audio.push(cue);
  }

  #currentStation(): ShootingStation {
    return STATIONS[this.#station]!;
  }

  #foldHash(): void {
    const fold = (v: number): void => {
      this.#hash = (Math.imul(this.#hash ^ (v | 0), 0x01000193) >>> 0);
    };
    const phaseIndex = ["ready", "charging", "releasing", "ballInFlight", "shotResolved", "movingToNextRack", "results"].indexOf(this.#phase);
    fold(this.#tick);
    fold(phaseIndex);
    fold(this.#score * 31 + this.#streak);
    fold(this.#station * 8 + this.#ballIndex);
    fold(Math.round(this.#yaw * 65536));
    for (const live of this.#liveBalls) {
      fold(live.seq);
      fold(Math.round(live.ball.pos.x * 4096));
      fold(Math.round(live.ball.pos.y * 4096));
      fold(Math.round(live.ball.pos.z * 4096));
    }
  }

  // ── camera + reticle + scene view ───────────────────────────────────────────

  /** The camera: orientation is ALWAYS exactly the player's stored aim (the game
   * never offsets it); position is the station spot, interpolated during the
   * rack-to-rack glide. */
  #cameraEye(): { position: Vec3; yaw: number; pitch: number } {
    const station = this.#currentStation();
    if (this.#phase === "movingToNextRack") {
      const next = STATIONS[this.#station + 1]!;
      const t = smoothstep(this.#phaseTicks / MOVE_TICKS);
      const pos = lerp(station.position, next.position, t);
      return { pitch: this.#pitch, position: vec3(pos.x, EYE_HEIGHT, pos.z), yaw: this.#yaw };
    }
    return { pitch: this.#pitch, position: vec3(station.position.x, EYE_HEIGHT, station.position.z), yaw: this.#yaw };
  }

  /** The reticle: a FIXED center crosshair on the player's own aim line — the
   * game only picks its visibility, never its position. Bright while a ball is
   * in hand or rising, faint while the ball is away, gone during glides/results. */
  #reticleView(): ReticleView {
    if ((this.#phase === "ready" && this.ballInHand) || this.#phase === "charging") return { mode: "active" };
    if (this.#phase === "movingToNextRack" || this.#phase === "results") return { mode: "hidden" };
    return { mode: "dim" };
  }

  #heldBallView(): BallView | null {
    if (this.#handSlot < 0) return null;
    const station = this.#currentStation();
    const golden = this.#handSlot === GOLDEN_BALL_INDEX;
    if (this.#phase === "charging") {
      return { golden, orientation: [0, 0, 0, 1], position: risePosition(station, this.#yaw, this.#handSlot, motionProgress(this.#motionTicks)) };
    }
    if (this.#phase === "ready" || this.#phase === "releasing") {
      const from = rackSlotPosition(station, this.#handSlot);
      const chest = chestAnchor(station, this.#yaw, this.#handSlot);
      const t = smoothstep(Math.min(this.#handTicks / SHOT_TUNING.pickupTicks, 1));
      const held = lerp(from, chest, t);
      const bob = t >= 1 ? Math.sin(this.#tick * 0.18) * 0.012 : 0;
      return { golden, orientation: [0, 0, 0, 1], position: vec3(held.x, held.y + bob, held.z) };
    }
    return null;
  }

  #previewPath(): readonly Vec3[] {
    if (!DEBUG_TRAJECTORY || this.#phase !== "charging") return [];
    const p = motionProgress(this.#motionTicks);
    const launch = launchVelocity(this.#yaw, p);
    const ghost = makeBall(risePosition(this.#currentStation(), this.#yaw, this.#handSlot, p), launch.velocity, launch.angularVelocity);
    return predictTrajectory(ghost, PREVIEW_POINTS, PREVIEW_STRIDE_TICKS);
  }

  view(): SceneView {
    const eye = this.#cameraEye();
    const dir = aimDirection(eye.yaw, eye.pitch);
    return {
      cameraPosition: eye.position,
      cameraTarget: vec3(eye.position.x + dir.x, eye.position.y + dir.y, eye.position.z + dir.z),
      flying: this.#liveBalls.map((live) => ({
        golden: live.slot === GOLDEN_BALL_INDEX,
        orientation: live.ball.orient,
        position: live.ball.pos,
      })),
      heldBall: this.#heldBallView(),
      impact: this.#impact,
      netPulse: this.#netPulse,
      preview: this.#previewPath(),
      rackFilled: this.#rackTaken.map((taken) => !taken),
      trail: this.#trail,
    };
  }

  hud(): Hud {
    const rackStart = this.#station * BALLS_PER_RACK;
    const ballsLeft = this.#rackTaken.slice(rackStart, rackStart + BALLS_PER_RACK).filter((taken) => !taken).length;
    return {
      atTop: this.#phase === "charging" && this.#motionTicks >= SHOT_TUNING.chestSettleTicks + SHOT_TUNING.shotRiseTicks,
      ballsLeft,
      events: this.drainEvents(),
      golden: this.#handSlot === GOLDEN_BALL_INDEX,
      motion: this.#phase === "charging" ? motionProgress(this.#motionTicks) : -1,
      movingToLabel: this.#phase === "movingToNextRack" ? STATIONS[this.#station + 1]!.label : undefined,
      phase: this.#phase,
      rackIndex: this.#station,
      results: this.#results,
      reticle: this.#reticleView(),
      score: this.#score,
      streak: this.#streak,
    };
  }
}
