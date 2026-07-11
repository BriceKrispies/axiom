/*
 * session.ts — `HomeRunSession`, the framework-free heart of the game. It owns one
 * explicit mutable state, advances it exactly one deterministic tick per
 * `advance(intent)`, and folds the pure modules (`swing.ts`, `pitch.ts`,
 * `fielders.ts`, `ball.ts`) into the round state machine
 * (`ready → windup → pitch → flight → result → … → over`). It imports NOTHING
 * from `@axiom/game`, so the whole game is constructible and replayable in a bare
 * `node --test`; `scene.ts` reads its `view()` snapshot and `game.ts` its HUD
 * accessors. All variation derives from the constructor seed via `hash01`.
 */

import { type Vec3, add, clamp, clamp01, lerp, scale, vec3 } from "./vec.ts";
import { type Swing, type Feedback, type Intent, type Outcome, type Phase, type PitchResult, type PitchSpec, type SceneView, type FielderState } from "./types.ts";
import { newSwing, stepSwing, sweptContact } from "./swing.ts";
import { pitchGapTicks, selectPitch, solvePitch } from "./pitch.ts";
import { catchingFielder, newFielders, projectLanding, stepFielders } from "./fielders.ts";
import { type BallFlight, classifyCaught, classifyFlight, newFlight, scoreFor, stepFlight } from "./ball.ts";
import * as C from "./constants.ts";

const TRAIL_MAX = 14;
const HIDDEN_BALL: Vec3 = vec3(0, -100, 0);

const OUTCOME_TEXT: Record<Outcome, string> = {
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
  #lastMph = 0;
  #lastPitchName = "";

  public constructor(seed = 1) {
    this.#seed = seed | 0;
    this.#fielders = newFielders(this.#seed);
  }

  /** Advance exactly one fixed tick. */
  public advance(intent: Intent): void {
    this.#tick += 1;
    this.#decayFeel();

    // Hit-stop: the impact freeze — ball, bat and fielders hold for a few ticks.
    if (this.#hitStop > 0) {
      this.#hitStop -= 1;
      return;
    }
    this.#phaseTicks += 1;

    // Batter repositioning (only while a pitch can still be met).
    if (this.#phase === "ready" || this.#phase === "windup" || this.#phase === "pitch") {
      this.#batterX = clamp(this.#batterX + intent.moveX * C.BATTER_STEP_SPEED, C.BATTER_MIN_X, C.BATTER_MAX_X);
    }

    // The swing machine runs in every live phase (practice swings included).
    const prevTheta = this.#swing.theta;
    if (this.#phase !== "over") {
      this.#swing = stepSwing(this.#swing, intent.swing);
      if (this.#swing.state === "swing" && (this.#phase === "pitch" || this.#phase === "windup")) {
        this.#swungThisPitch = true;
      }
    }

    switch (this.#phase) {
      case "ready":
        stepFielders(this.#fielders, this.#seed, this.#tick, null);
        if (intent.start || intent.swing) {
          this.#beginPitch();
        }
        return;
      case "windup":
        stepFielders(this.#fielders, this.#seed, this.#tick, null);
        if (this.#phaseTicks >= this.#gap + C.WINDUP_TICKS) {
          this.#releasePitch();
        }
        return;
      case "pitch":
        stepFielders(this.#fielders, this.#seed, this.#tick, null);
        this.#stepPitch(prevTheta);
        return;
      case "flight":
        this.#stepFlight();
        return;
      case "result":
        stepFielders(this.#fielders, this.#seed, this.#tick, null);
        if (this.#phaseTicks >= this.#resultDuration) {
          this.#nextPitchOrOver();
        }
        return;
      case "over":
        stepFielders(this.#fielders, this.#seed, this.#tick, null);
        if (intent.start) {
          this.reset();
        }
        return;
      default:
        return;
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
    this.#trail = [];
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

  #stepPitch(prevTheta: number): void {
    const prevBall = this.#ballPos;
    this.#ballVel = vec3(this.#ballVel.x, this.#ballVel.y - this.#pitchGravity, this.#ballVel.z);
    this.#ballPos = add(this.#ballPos, this.#ballVel);

    // The swept bat-vs-ball test — only a committed forward swing can strike.
    if (this.#swing.state === "swing") {
      const contact = sweptContact(
        prevTheta,
        this.#swing.theta,
        this.#swing.omega,
        this.#batterX,
        prevBall,
        this.#ballPos,
        this.#ballVel.z,
      );
      if (contact !== null) {
        this.#beginFlight(contact.point, contact.exitVel, contact.exitSpeed, contact.loft, contact.spray, contact.quality);
        return;
      }
    }
    // Past the plate untouched → a miss (swung) or a take (watched it go by).
    if (this.#ballPos.z <= C.CATCHER_Z) {
      this.#ballLive = false;
      this.#ballPos = HIDDEN_BALL;
      this.#resolve(this.#swungThisPitch ? "MISS" : "STRIKE", "miss", 0, false);
    }
  }

  #beginFlight(point: Vec3, vel: Vec3, exitSpeed: number, loft: number, spray: number, quality: number): void {
    this.#flight = newFlight(point, vel, exitSpeed, loft, spray);
    this.#ballPos = point;
    this.#ballLive = true;
    this.#phase = "flight";
    this.#phaseTicks = 0;
    this.#trail = [];
    this.#emit({ big: quality > 0.8, kind: "contact", text: "" });
    this.#impactFlash = Math.round(6 + 10 * quality);
    if (quality >= C.HIT_STOP_QUALITY) {
      this.#hitStop = C.HIT_STOP_BASE_TICKS + Math.round(C.HIT_STOP_MAX_EXTRA * clamp01((quality - C.HIT_STOP_QUALITY) / (1 - C.HIT_STOP_QUALITY)));
      this.#shake(C.SHAKE_CONTACT * (0.5 + quality), C.SHAKE_TICKS);
    }
  }

  #stepFlight(): void {
    const b = this.#flight!;
    const wasHomer = b.homer;
    const landing = projectLanding(b.pos, b.vel, C.GRAVITY / (C.FIXED_HZ * C.FIXED_HZ));
    stepFielders(this.#fielders, this.#seed, this.#tick, b.foul ? null : landing);

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
    // Ball-follow camera for genuinely long hits.
    const long = !b.foul && b.exitSpeed > 20 && b.pos.z > 12;
    this.#followBlend = clamp01(this.#followBlend + (long ? C.CAMERA_FOLLOW_RATE : -C.CAMERA_FOLLOW_RATE));
    if (this.#followBlend > C.CAMERA_FOLLOW_MAX) {
      this.#followBlend = C.CAMERA_FOLLOW_MAX;
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
    if (outcome !== "miss" && outcome !== "foul" && !caught) {
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
    return vec3(Math.sin(this.#tick * 2.9) * m, Math.cos(this.#tick * 2.3) * m * 0.6, 0);
  }

  #windupProgress(): number {
    if (this.#phase !== "windup" || this.#phaseTicks < this.#gap) {
      return 0;
    }
    const w = clamp01((this.#phaseTicks - this.#gap) / C.WINDUP_TICKS);
    return w * w * (3 - 2 * w);
  }

  // ── read-only snapshots ─────────────────────────────────────────────────────

  /** The full scene snapshot for `scene.ts` (presentation only — cannot mutate play). */
  public view(): SceneView {
    const windup = this.#windupProgress();
    const dolly = windup * C.CAMERA_WINDUP_DOLLY + (this.#punchTicks / C.CAMERA_PUNCH_TICKS) * C.CAMERA_RELEASE_PUNCH;
    const shake = this.#shakeOffset();
    const cameraPos = add(add(C.CAMERA_POS, vec3(0, 0, dolly)), shake);
    const followTarget =
      this.#followBlend > 0 && this.#ballLive ? lerp(C.CAMERA_TARGET, this.#ballPos, this.#followBlend) : C.CAMERA_TARGET;
    const cameraTarget = add(followTarget, scale(shake, 0.5));
    return {
      ball: this.#ballPos,
      ballInPlay: this.#flight !== undefined,
      ballVisible: this.#ballLive,
      batterX: this.#batterX,
      cameraPos,
      cameraTarget,
      fielders: this.#fielders.map((f) => ({ chasing: f.chasing, x: f.x, z: f.z })),
      hitStop: this.#hitStop > 0,
      impactFlash: clamp01(this.#impactFlash / 12),
      muzzleFlash: clamp01(this.#muzzleFlash / C.FLASH_TICKS),
      phase: this.#phase,
      swing: this.#swing,
      tick: this.#tick,
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
    if (this.#events.length > 8) {
      this.#events.shift();
    }
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
