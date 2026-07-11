/*
 * session.ts — the ONE mutable state machine of the shootout, entirely SDK-free
 * (importing only vec / constants / types / gameplay / physics), so a bare
 * `node --test` process can play complete games. `advance(intent)` runs exactly one
 * deterministic 60 Hz tick of the explicit phase machine
 * (ready → charging → releasing → ballInFlight → shotResolved → movingToNextRack →
 * results); `view()` exposes a read-only scene snapshot, `hud()` the HUD snapshot,
 * `drainGameEvents()` the unified feedback event stream (see `types.ts`
 * `GameEvent` — audio, HUD, and scene reactions all key off it), and `hash()` a
 * replay-equality fingerprint. Presentation reactions live in `polish.ts`
 * (`PolishState`), fed those same events and advanced once per tick — cosmetic
 * only, never folded into the hash, cleared exactly by restart.
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
  POLISH_TUNING,
  PREVIEW_POINTS,
  PREVIEW_STRIDE_TICKS,
  RACK_COUNT,
  RACK_LABELS,
  RIM_X,
  RIM_Z,
  SHOT_TUNING,
  STATIONS,
  aimDirection,
  rackSlotPosition,
  yawForward,
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
import { PolishState, glintOn, streakPresentationLevel } from "./polish.ts";
import type { BallView, ContactSurface, Feedback, GameEvent, Hud, Intent, Phase, Results, ReticleView, SceneView, ShotOutcome } from "./types.ts";

const FEEDBACK_MAX = 8;
const GAME_EVENT_MAX = 24;
const IMPACT_DECAY = 0.08;

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
  /** Bounded motion-trail samples (golden balls carry more; see POLISH_TUNING). */
  readonly trail: Vec3[];
  /** Horizontal rim-axis offset at the moment of scoring (net displacement). */
  entryX: number;
  entryZ: number;
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

  // Presentation side-channels (cosmetic only — never folded into the hash).
  #events: Feedback[] = [];
  #gameEvents: GameEvent[] = [];
  readonly #polish = new PolishState();
  #impact: { position: Vec3; strength: number } | null = null;
  /** Per-surface impact cooldown: last tick each surface reacted. */
  #lastImpact: Record<ContactSurface, number> = { backboard: -1000, floor: -1000, pole: -1000, rim: -1000 };
  /** Space pressed just before the ball reaches the chest is honored for a
   * short bounded window (never across transitions or restarts). */
  #bufferTicks = 0;

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

  /** Drain the unified feedback event stream (audio + HUD reactions key off it;
   * the internal `PolishState` has already consumed each event). */
  drainGameEvents(): readonly GameEvent[] {
    const out = this.#gameEvents;
    this.#gameEvents = [];
    return out;
  }

  /** Development counter: presentation reactions currently animating. */
  activeEffects(): number {
    return this.#polish.activeEffects();
  }

  /** Development counter: trail samples currently held across all live balls. */
  activeTrailSamples(): number {
    return this.#liveBalls.reduce((sum, b) => sum + b.trail.length, 0);
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
    this.#gameEvents = [];
    this.#polish.reset();
    this.#impact = null;
    this.#lastImpact = { backboard: -1000, floor: -1000, pole: -1000, rim: -1000 };
    this.#bufferTicks = 0;
    this.#results = undefined;
    this.#hash = 0x811c9dc5;
    this.#dealNextBall();
  }

  // ── the tick ────────────────────────────────────────────────────────────────

  advance(intent: Intent): void {
    this.#tick += 1;

    if (intent.restartPressed) {
      this.reset();
      // Queue for the platform layer only: `reset()` already restored the
      // polish state (including the fresh first-ball pickup), so feeding it
      // gameRestarted here would wipe that exact-initial presentation.
      this.#gameEvents.push({ kind: "gameRestarted" });
      return;
    }

    this.#applyAim(intent);
    this.#decayEffects();
    this.#stepLiveBalls();

    if (this.#phase === "ready") {
      this.#handTicks += 1;
      // A completed swipe (mobile) IS the whole shot: flick strength decides the
      // release progress, the sideways flick offsets the launch yaw — the
      // camera/aim are untouched. Space starts the desktop rise (a press that
      // lands just before the ball reaches the chest is buffered briefly). Both
      // wait for the dealt ball to reach the chest.
      if (intent.shootPressed && !this.ballInHand) {
        this.#bufferTicks = POLISH_TUNING.inputBufferTicks;
      } else if (this.#bufferTicks > 0) {
        this.#bufferTicks -= 1;
      }
      if (intent.swipe !== null && this.ballInHand) {
        this.#launchBall(intent.swipe.progress, intent.swipe.yawOffset);
      } else if ((intent.shootPressed || this.#bufferTicks > 0) && this.ballInHand) {
        this.#bufferTicks = 0;
        this.#phase = "charging";
        this.#phaseTicks = 0;
        this.#motionTicks = 0;
      }
    }
    if (this.#phase === "charging") {
      this.#tickCharging(intent);
    } else if (this.#phase === "releasing") {
      // An eager press during the follow-through buffers into the next ball.
      if (intent.shootPressed) this.#bufferTicks = POLISH_TUNING.inputBufferTicks;
      this.#tickFollowThrough();
    } else if (this.#phase === "ballInFlight") {
      // The rack is empty; waiting for every airborne ball to resolve. Buffered
      // input never crosses a rack boundary.
      this.#bufferTicks = 0;
      if (this.ballsInFlight === 0) {
        this.#phase = "shotResolved";
        this.#phaseTicks = 0;
      }
    } else if (this.#phase === "shotResolved") {
      this.#tickRackEndBeat();
    } else if (this.#phase === "movingToNextRack") {
      this.#bufferTicks = 0;
      this.#tickMoving();
    }

    this.#polish.advance();
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
    this.#pushGameEvent({ golden: this.#ballIndex === GOLDEN_BALL_INDEX, kind: "ballPickupStarted", slot: this.#ballIndex });
  }

  #tickCharging(intent: Intent): void {
    this.#phaseTicks += 1;
    this.#motionTicks += 1;
    const p = motionProgress(this.#motionTicks);
    if (p > 0 && p < 1 && this.#motionTicks % 6 === 0) {
      this.#pushGameEvent({ kind: "chargeTick", level: p });
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
      entryX: 0,
      entryZ: 0,
      fadeTicks: FEEDBACK_TICKS,
      flightTicks: 0,
      outcome: undefined,
      seq: this.#launchSeq,
      slot,
      touchedRim: false,
      touchedBackboard: false,
      trail: [],
    });
    this.#launchSeq += 1;
    const nextSlot = slot + 1;
    if (nextSlot < BALLS_PER_RACK) {
      this.#ballIndex = nextSlot;
      this.#dealNextBall();
    } else {
      this.#handSlot = -1;
    }
    this.#phase = "releasing";
    this.#phaseTicks = 0;
    this.#pushGameEvent({ kind: "ballReleased", progress: p });
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
        this.#pushGameEvent({ kind: "rackCompleted", station: this.#station });
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
      this.#pushGameEvent({ kind: "stationTransitionStarted", label: RACK_LABELS[this.#station + 1] ?? "" });
    } else {
      this.#results = {
        bestStreak: this.#bestStreak,
        label: performanceLabel(this.#makes),
        makes: this.#makes,
        score: this.#score,
      };
      this.#phase = "results";
      this.#phaseTicks = 0;
      this.#pushGameEvent({ kind: "gameCompleted", results: this.#results });
    }
  }

  /** The glide moves the player's POSITION to the next station (locations are
   * game-fixed by design); the view stays wherever the mouse points it. */
  #tickMoving(): void {
    this.#phaseTicks += 1;
    if (this.#phaseTicks >= MOVE_TICKS) {
      this.#station += 1;
      this.#ballIndex = 0;
      this.#pushGameEvent({
        final: this.#station === RACK_COUNT - 1,
        kind: "stationTransitionCompleted",
        label: RACK_LABELS[this.#station] ?? "",
        station: this.#station,
      });
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
          this.#reportContact(contact.surface, contact.speed, contact.position, live.seq);
        }
        let scoredNow = false;
        for (const sample of step.samples) {
          const result = stepDetection(live.detection, sample);
          live.detection = result.state;
          scoredNow = scoredNow || result.scoredNow;
        }
        if (scoredNow) {
          live.outcome = classifyOutcome(true, live.touchedRim, live.touchedBackboard);
          live.entryX = live.ball.pos.x - RIM_X;
          live.entryZ = live.ball.pos.z - RIM_Z;
        } else if (hitFloor || outOfBounds(live.ball.pos) || live.flightTicks > SHOT_TUNING.maxShotLifetimeTicks) {
          live.outcome = classifyOutcome(false, live.touchedRim, live.touchedBackboard);
        }
      } else {
        // Resolved: keep bouncing behind the action (still audible), then clear.
        const step = stepBall(live.ball);
        for (const contact of step.contacts) {
          this.#reportContact(contact.surface, contact.speed, contact.position, live.seq);
        }
        if (live.applied) live.fadeTicks -= 1;
      }
      this.#sampleTrail(live);
    }
    this.#applyOutcomesInOrder();
    this.#liveBalls = this.#liveBalls.filter((b) => b.fadeTicks > 0);
  }

  /** Bounded per-ball motion trail: golden balls always leave one (a longer
   * cap); ordinary balls only while moving fast. Resolved balls bleed their
   * trail out one sample per stride. All caps come from POLISH_TUNING. */
  #sampleTrail(live: LiveBall): void {
    if (this.#tick % POLISH_TUNING.trailSampleStrideTicks !== 0) return;
    const golden = live.slot === GOLDEN_BALL_INDEX;
    const v = live.ball.vel;
    const fast = v.x * v.x + v.y * v.y + v.z * v.z > POLISH_TUNING.trailSpeedSq;
    const cap = golden ? POLISH_TUNING.goldenTrailSamples : POLISH_TUNING.ballTrailSamples;
    if (live.outcome === undefined && (golden || fast)) {
      live.trail.push(live.ball.pos);
      while (live.trail.length > cap) live.trail.shift();
    } else if (live.trail.length > 0) {
      live.trail.shift();
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
        const swish = outcome === "swish";
        if (swish) this.#pushGameEvent({ kind: "swishMade" });
        this.#pushGameEvent({
          entryX: next.entryX,
          entryZ: next.entryZ,
          golden: next.slot === GOLDEN_BALL_INDEX,
          kind: "basketMade",
          points,
          streak: this.#streak,
          swish,
        });
        this.#pushGameEvent({ kind: "streakIncreased", streak: this.#streak });
        this.#pushEvent({ big: swish || this.#streak >= 3, kind: outcome, text: outcomeText(outcome) });
      } else {
        const hadStreak = this.#streak;
        this.#streak = 0;
        this.#pushGameEvent({ kind: "shotMissed", outcome });
        if (hadStreak > 0) this.#pushGameEvent({ hadStreak, kind: "streakBroken" });
        this.#pushEvent({ big: false, kind: outcome, text: outcomeText(outcome) });
      }
    }
  }

  // ── presentation helpers ────────────────────────────────────────────────────

  /** One contact reaction per surface per cooldown window (a rim rattle must
   * not machine-gun identical events); the ball's `seq` rides floor hits so the
   * squash lands on the right ball. Pole contacts read as rim (metal). */
  #reportContact(surface: ContactSurface, speed: number, position: Vec3, seq: number): void {
    if (speed <= 0.4) return;
    if (this.#tick - this.#lastImpact[surface] < POLISH_TUNING.impactCooldownTicks) return;
    this.#lastImpact[surface] = this.#tick;
    if (surface === "rim" || surface === "pole") {
      this.#impact = { position, strength: 1 };
      this.#pushGameEvent({ kind: "rimHit", position, speed });
    } else if (surface === "backboard") {
      this.#impact = { position, strength: 1 };
      this.#pushGameEvent({ kind: "backboardHit", position, speed });
    } else {
      this.#pushGameEvent({ kind: "floorHit", seq, speed });
    }
  }

  #decayEffects(): void {
    if (this.#impact !== null) {
      const strength = this.#impact.strength - IMPACT_DECAY;
      this.#impact = strength <= 0 ? null : { position: this.#impact.position, strength };
    }
  }

  #pushEvent(event: Feedback): void {
    if (this.#events.length < FEEDBACK_MAX) this.#events.push(event);
  }

  /** The single emission point: queue for the platform (audio/HUD) and feed the
   * internal presentation state the same event. */
  #pushGameEvent(event: GameEvent): void {
    if (this.#gameEvents.length < GAME_EVENT_MAX) this.#gameEvents.push(event);
    this.#polish.onEvent(event);
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
   * rack-to-rack glide, plus the release kick — a tiny, brief POSITION recoil
   * against the shot direction (visual only; it never touches yaw/pitch and
   * never alters a launch). */
  #cameraEye(): { position: Vec3; yaw: number; pitch: number } {
    const station = this.#currentStation();
    if (this.#phase === "movingToNextRack") {
      const next = STATIONS[this.#station + 1]!;
      const t = smoothstep(this.#phaseTicks / MOVE_TICKS);
      const pos = lerp(station.position, next.position, t);
      return { pitch: this.#pitch, position: vec3(pos.x, EYE_HEIGHT, pos.z), yaw: this.#yaw };
    }
    const recoil = this.#polish.kickRecoil();
    const fwd = yawForward(this.#yaw);
    return {
      pitch: this.#pitch,
      position: vec3(
        station.position.x - fwd.x * recoil,
        EYE_HEIGHT + recoil * 0.35,
        station.position.z - fwd.z * recoil,
      ),
      yaw: this.#yaw,
    };
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
    const glint = golden && glintOn(this.#tick);
    if (this.#phase === "charging") {
      const p = motionProgress(this.#motionTicks);
      const pos = risePosition(station, this.#yaw, this.#handSlot, p);
      // Subtle hold tension: a restrained tremor that grows with the rise —
      // presentation only (the launch reads the aim + progress, never this).
      const tension = Math.sin(this.#tick * 0.55) * 0.004 * p;
      return {
        glint,
        golden,
        orientation: [0, 0, 0, 1],
        position: vec3(pos.x, pos.y + tension, pos.z),
        squash: 1,
        trail: [],
      };
    }
    if (this.#phase === "ready" || this.#phase === "releasing") {
      const from = rackSlotPosition(station, this.#handSlot);
      const chest = chestAnchor(station, this.#yaw, this.#handSlot);
      // Anticipation: the ball lifts straight off its slot for a beat before it
      // flies to the chest (all inside the unchanged pickup window).
      const ant = POLISH_TUNING.pickupAnticipationTicks;
      const lift = vec3(from.x, from.y + 0.035, from.z);
      let held: Vec3;
      if (this.#handTicks <= ant) {
        held = lerp(from, lift, smoothstep(this.#handTicks / ant));
      } else {
        const t = smoothstep(Math.min((this.#handTicks - ant) / (SHOT_TUNING.pickupTicks - ant), 1));
        held = lerp(lift, chest, t);
      }
      const bob = this.#handTicks >= SHOT_TUNING.pickupTicks ? Math.sin(this.#tick * 0.18) * 0.012 : 0;
      return {
        glint,
        golden,
        orientation: [0, 0, 0, 1],
        position: vec3(held.x, held.y + bob, held.z),
        squash: 1,
        trail: [],
      };
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
      boardOffset: this.#polish.boardOffset(),
      crowdPulse: this.#polish.crowdPulse(),
      flying: this.#liveBalls.map((live) => ({
        glint: live.slot === GOLDEN_BALL_INDEX && glintOn(this.#tick + live.seq * 17),
        golden: live.slot === GOLDEN_BALL_INDEX,
        orientation: live.ball.orient,
        position: live.ball.pos,
        squash: this.#polish.squash(live.seq),
        trail: live.trail,
      })),
      heldBall: this.#heldBallView(),
      impact: this.#impact,
      net: this.#polish.net(),
      preview: this.#previewPath(),
      rackDip: this.#polish.rackDip(),
      rackFilled: this.#rackTaken.map((taken) => !taken),
      rimOffset: this.#polish.rimOffset(),
      score: this.#score,
      slotSettle: this.#polish.slotSettle(),
      streak: this.#streak,
      tick: this.#tick,
    };
  }

  hud(): Hud {
    const rackStart = this.#station * BALLS_PER_RACK;
    const ballsLeft = this.#rackTaken.slice(rackStart, rackStart + BALLS_PER_RACK).filter((taken) => !taken).length;
    return {
      atTop: this.#phase === "charging" && this.#motionTicks >= SHOT_TUNING.chestSettleTicks + SHOT_TUNING.shotRiseTicks,
      award: this.#polish.award(),
      ballsLeft,
      events: this.drainEvents(),
      glow: this.#polish.glow(),
      golden: this.#handSlot === GOLDEN_BALL_INDEX,
      motion: this.#phase === "charging" ? motionProgress(this.#motionTicks) : -1,
      movingToLabel: this.#phase === "movingToNextRack" ? (RACK_LABELS[this.#station + 1] ?? undefined) : undefined,
      phase: this.#phase,
      rackIndex: this.#station,
      results: this.#results,
      reticle: this.#reticleView(),
      score: this.#score,
      stationLabel: this.#polish.stationLabel(),
      streak: this.#streak,
      streakBrokenSeq: this.#polish.streakBrokenSeq(),
      streakLevel: streakPresentationLevel(this.#streak),
      streakPulseSeq: this.#polish.streakPulseSeq(),
    };
  }
}
