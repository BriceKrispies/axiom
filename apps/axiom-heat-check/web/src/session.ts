/*
 * session.ts — `HeatCheckSession`, the framework-free heart of the game. It owns one
 * explicit mutable state, advances it exactly one deterministic tick per
 * `advance(intent)`, and folds the pure `gameplay.ts` rules into the round state
 * machine (`ready → playing → shooting → scoredFeedback → gameOver`). It imports
 * NOTHING from `@axiom/game`, so the whole game is constructible and replayable in a
 * bare `node --test`; `scene.ts` reads its `view()` snapshot, and `game.ts` reads its
 * HUD accessors. No wall-clock, no RNG — variation is derived from the shot number.
 *
 * The hoop is fixed at lateral center (x = 0); the player creates space by moving.
 */

import { type Vec3, clamp, vec3 } from "./vec.ts";
import {
  applyScore,
  classifyShot,
  clampPlayerPosition,
  computeBalanceTag,
  computeBreakdown,
  computeRequiredQuality,
  computeRhythmTag,
  computeSpaceTag,
  createShotArc,
  describeShot,
  determineShotResult,
  gainAdvantage,
  gainFatigue,
  pushFeedback,
  rhythmPhaseAt,
  sampleArc,
  stepAdvantage,
  stepFatigue,
  updateDefenderBalance,
  updateHeat,
  updateStreakMultiplier,
} from "./gameplay.ts";
import type { Feedback, Intent, Readiness, SceneView, ShotArc, ShotBreakdown, ShotReason, ShotResult } from "./types.ts";
import * as C from "./constants.ts";

const HOOP_X = 0;
const TRAIL_MAX = 16;
const SCORE_FLASH_TICKS = 18;
const SHAKE_TICKS = 10;

/** The in-flight shot the session is resolving. */
interface ActiveShot {
  readonly arc: ShotArc;
  t: number;
  readonly result: ShotResult;
  readonly reason: ShotReason;
  readonly quality: number;
  readonly required: number;
  readonly deep: boolean;
  /** The two-part feedback, built at release from the same breakdown. */
  readonly feedback: Feedback;
}

const sign = (v: number): number => (v > 0 ? 1 : v < 0 ? -1 : 0);

const EMPTY_BREAKDOWN: ShotBreakdown = {
  advantage: 0,
  heatBonus: 0,
  pressurePenalty: 0,
  quality: 0,
  separation: 0,
  shotSelection: 0,
  stability: 1,
  timing: 0,
};

export class HeatCheckSession {
  // Round + clock.
  #phase: SceneView["phase"] = "ready";
  #tick = 0;
  #elapsed = 0;

  // Player.
  #playerX = 0;
  #playerVelX = 0;
  #prevStickX = 0;
  #plantTicks = 0;
  /** Consecutive ticks the control has been held this possession (for the shoot gate). */
  #holdTicks = 0;
  /** True while a control press is active (drives the rhythm meter). */
  #holding = false;
  /** Transient edge from beating the defender (0..1) — decays fast, drives the window. */
  #advantage = 0;
  /** Dribble fatigue (0..1) from wiggle spam — dulls crossovers + shakes the handle. */
  #fatigue = 0;
  /** Tick of the last stick reversal (to detect repeated spam within a window). */
  #lastReversalTick = -999;

  // Defender + its delayed-target ring.
  #defenderX = 0;
  #defenderVelX = 0;
  #defenderBalance = 1;
  readonly #history: number[] = new Array<number>(C.DEFENDER_REACTION_DELAY).fill(0);
  #hhead = 0;

  // Score + heat.
  #score = 0;
  #streak = 0;
  #multiplier = 1;
  #heat = 0;
  #best = 0;
  #shotCount = 0;

  // Feedback + transient visuals.
  #breakdown: ShotBreakdown = EMPTY_BREAKDOWN;
  #shot: ActiveShot | undefined;
  #feedbackHold = 0;
  #ticksSinceScore = 999;
  #lastScoreBig = false;
  #swishPulse = 0;
  #trail: Vec3[] = [];
  #events: Feedback[] = [];
  #ball: Vec3 = vec3(0, C.BALL_RADIUS, C.PLAYER_Z);
  #ballInFlight = false;

  /** Advance exactly one fixed tick. */
  public advance(intent: Intent): void {
    this.#tick += 1;

    switch (this.#phase) {
      case "ready":
        // A control press (or an explicit keyboard shot) starts the round.
        this.#holding = false;
        if (intent.holding || intent.shoot) {
          this.#phase = "playing";
        }
        this.#idleDribble();
        return;
      case "playing":
        this.#stepPlaying(intent);
        return;
      case "shooting":
        // Ball is in the air — input is ignored (no move, no shoot, no restart).
        this.#holding = false;
        this.#stepClock();
        this.#stepDefender();
        this.#advanceShot();
        return;
      case "scoredFeedback":
        this.#holding = false;
        this.#stepClock();
        this.#stepDefender();
        this.#decayScoredFeedback();
        return;
      case "gameOver":
        // A discrete tap (release edge), a keyboard shot, or R runs it back — a held
        // drag does NOT, so the player can't blow past the game-over screen while
        // still touching.
        this.#holding = false;
        if (intent.reset || intent.released || intent.shoot) {
          this.reset();
        }
        this.#idleDribble();
        return;
      default:
        return;
    }
  }

  /** One live tick of play: move by the stick, chase, dribble, and gate the shot. */
  #stepPlaying(intent: Intent): void {
    this.#holding = intent.holding;
    this.#holdTicks = intent.holding ? this.#holdTicks + 1 : this.#holdTicks;
    this.#stepClock();
    this.#stepMovement(intent);
    this.#stepDefender();
    this.#dribbleBall();
    // Recompute the full shot breakdown every tick so the readiness meter is live and
    // a release uses exactly the numbers the player is looking at.
    this.#breakdown = this.#liveBreakdown();

    // Release-to-shoot: only a hold long enough to not be a micro-tap fires; an
    // explicit keyboard shot bypasses the hold gate.
    const heldLongEnough = this.#holdTicks >= C.MIN_SHOOT_HOLD_TICKS;
    const shoots = intent.shoot || (intent.released && heldLongEnough);
    if (shoots) {
      this.#holdTicks = 0;
      this.#beginShot();
      return;
    }
    // A released-but-too-short tap (or letting go without shooting) clears the hold.
    this.#holdTicks = intent.holding ? this.#holdTicks : 0;
  }

  /** Restart the round, preserving the session-best score. */
  public reset(): void {
    this.#best = Math.max(this.#best, this.#score);
    this.#phase = "ready";
    this.#elapsed = 0;
    this.#playerX = 0;
    this.#playerVelX = 0;
    this.#prevStickX = 0;
    this.#plantTicks = 0;
    this.#holdTicks = 0;
    this.#holding = false;
    this.#advantage = 0;
    this.#fatigue = 0;
    this.#lastReversalTick = -999;
    this.#defenderX = 0;
    this.#defenderVelX = 0;
    this.#defenderBalance = 1;
    this.#history.fill(0);
    this.#hhead = 0;
    this.#score = 0;
    this.#streak = 0;
    this.#multiplier = 1;
    this.#heat = 0;
    this.#shotCount = 0;
    this.#breakdown = EMPTY_BREAKDOWN;
    this.#shot = undefined;
    this.#feedbackHold = 0;
    this.#ticksSinceScore = 999;
    this.#lastScoreBig = false;
    this.#swishPulse = 0;
    this.#trail = [];
    this.#events = [];
    this.#ball = vec3(0, C.BALL_RADIUS, C.PLAYER_Z);
    this.#ballInFlight = false;
  }

  // ── round clock ───────────────────────────────────────────────────────────
  #stepClock(): void {
    this.#elapsed += 1;
    this.#ticksSinceScore += 1;
    this.#swishPulse *= 0.92;
    const wasFinal = this.#finalWindowAt(this.#elapsed - 1);
    if (!wasFinal && this.#finalWindow) {
      this.#emit({ big: true, kind: "double", text: "DOUBLE POINTS" });
    }
    if (this.#elapsed >= C.ROUND_TICKS) {
      this.#best = Math.max(this.#best, this.#score);
      this.#phase = "gameOver";
    }
  }

  // ── player movement (floating dribble-stick velocity model) ─────────────────
  #stepMovement(intent: Intent): void {
    // `stickX` is a velocity INTENT (-1..1), not a position. Remap past the deadzone,
    // apply an expo curve (fine control near center, full tilt at the edge), then TRACK
    // the target velocity tightly — ease toward it fast, and bleed to rest fast on
    // recenter — so the player follows the stick crisply instead of coasting past it.
    const raw = intent.stickX;
    const mag = Math.abs(raw);
    const past = mag < C.STICK_DEADZONE ? 0 : (mag - C.STICK_DEADZONE) / (1 - C.STICK_DEADZONE);
    const curved = sign(raw) * past * past;
    const targetVel = curved * C.PLAYER_MOVE_SPEED;
    const rate = curved !== 0 ? C.PLAYER_ACCELERATION : C.PLAYER_FRICTION;
    this.#playerVelX += (targetVel - this.#playerVelX) * rate;
    this.#playerVelX = clamp(this.#playerVelX, -C.PLAYER_MOVE_SPEED, C.PLAYER_MOVE_SPEED);
    const newX = clampPlayerPosition(this.#playerX + this.#playerVelX);
    // Kill velocity into a wall so the player doesn't "stick" to the sideline.
    this.#playerVelX = newX === this.#playerX ? 0 : this.#playerVelX;
    this.#playerX = newX;

    this.#plantTicks = Math.abs(this.#playerVelX) < 0.03 ? Math.min(this.#plantTicks + 1, 24) : 0;

    // ── crossover / advantage / fatigue ──
    // A stick reversal past the threshold is a direction change. If it comes too soon
    // after the last one, it's wiggle spam → fatigue (which dulls the next crossover
    // and shakes the handle). A reversal that ALSO catches the defender committed the
    // wrong way BEATS them: their balance buckles and you earn an advantage window.
    const reversal = Math.abs(raw - this.#prevStickX);
    // A genuine reversal is a hard flip between two real directions — NOT the first
    // push off from rest (which would poison the repeat/fatigue tracking).
    const isReversal =
      reversal >= C.CROSSOVER_REVERSAL_THRESHOLD &&
      sign(raw) !== 0 &&
      sign(this.#prevStickX) !== 0 &&
      sign(raw) !== sign(this.#prevStickX);
    const isRepeat = this.#tick - this.#lastReversalTick < C.REPEATED_REVERSAL_WINDOW;
    if (isReversal) {
      this.#fatigue = isRepeat ? gainFatigue(this.#fatigue) : this.#fatigue;
      this.#lastReversalTick = this.#tick;
    }
    const sharpCrossover =
      isReversal &&
      Math.abs(this.#playerVelX) >= C.CROSSOVER_SPEED_THRESHOLD * 0.5 &&
      sign(this.#defenderVelX) === sign(this.#prevStickX);
    this.#defenderBalance = updateDefenderBalance(this.#defenderBalance, sharpCrossover);

    // Advantage decays every tick (faster as the defender recovers), and jumps on a
    // clean crossover (dulled by fatigue + a repeat-spam penalty). Fatigue bleeds off
    // while committing to a direction (no reversal this tick).
    this.#advantage = stepAdvantage(this.#advantage, this.#defenderBalance);
    this.#advantage = sharpCrossover ? gainAdvantage(this.#advantage, this.#fatigue, isRepeat) : this.#advantage;
    this.#fatigue = isReversal ? this.#fatigue : stepFatigue(this.#fatigue);
    this.#prevStickX = raw;
  }

  // ── defender AI ───────────────────────────────────────────────────────────
  #stepDefender(): void {
    const delayed = this.#history[this.#hhead] ?? 0;
    this.#history[this.#hhead] = this.#playerX;
    this.#hhead = (this.#hhead + 1) % this.#history.length;

    const speed =
      (C.DEFENDER_BASE_SPEED + this.#heat * C.DEFENDER_HEAT_SPEED_BONUS) * (0.45 + 0.55 * this.#defenderBalance);
    const dx = clamp(delayed - this.#defenderX, -speed, speed);
    const newX = clampPlayerPosition(this.#defenderX + dx);
    this.#defenderVelX = newX - this.#defenderX;
    this.#defenderX = newX;
    // Balance also recovers slowly while chasing (crossover handled in movement).
  }

  // ── shooting ──────────────────────────────────────────────────────────────

  /** The live, fully-explainable shot breakdown from the current state. */
  #liveBreakdown(): ShotBreakdown {
    return computeBreakdown({
      advantage: this.#advantage,
      defenderBalance: this.#defenderBalance,
      defenderX: this.#defenderX,
      fatigue: this.#fatigue,
      finalWindow: this.#finalWindow,
      heat: this.#heat,
      plantTicks: this.#plantTicks,
      playerVelX: this.#playerVelX,
      playerX: this.#playerX,
      rhythmPhase: this.#rhythmPhase,
    });
  }

  #beginShot(): void {
    // Use the SAME breakdown the readiness meter is showing this tick — quality alone,
    // deterministically, decides the outcome, and the dominant reason shapes the arc.
    const b = this.#breakdown;
    const required = computeRequiredQuality(this.#heat, this.#score);
    const result = determineShotResult(b.quality, required, C.SHOT_SWISH_QUALITY);
    const reason = classifyShot(result, b, this.#rhythmPhase, required);
    const gap = Math.abs(this.#playerX - this.#defenderX);
    // A deep bonus requires real separation AND a clean (stable) release.
    const deep = gap >= C.DEEP_SHOT_SEPARATION && b.stability >= C.STABILITY_REQUIRED_FOR_CLEAN_SHOT;

    this.#shotCount += 1;
    this.#shot = {
      arc: createShotArc(reason, this.#playerX, HOOP_X, this.#playerVelX),
      deep,
      feedback: describeShot(result, reason, b, this.#rhythmPhase, this.#advantage),
      quality: b.quality,
      reason,
      required,
      result,
      t: 0,
    };
    this.#trail = [];
    this.#ballInFlight = true;
    this.#phase = "shooting";
  }

  #advanceShot(): void {
    const shot = this.#shot;
    if (shot === undefined) {
      this.#phase = "playing";
      return;
    }
    shot.t = Math.min(1, shot.t + 1 / C.SHOT_ARC_DURATION);
    this.#ball = sampleArc(shot.arc, shot.t);
    this.#trail.push(this.#ball);
    if (this.#trail.length > TRAIL_MAX) {
      this.#trail = this.#trail.slice(this.#trail.length - TRAIL_MAX);
    }
    if (shot.t >= 1) {
      this.#resolveShot(shot);
    }
  }

  #resolveShot(shot: ActiveShot): void {
    const heatBefore = this.#heat;
    this.#score += applyScore({
      deep: shot.deep,
      doublePoints: this.#finalWindow,
      multiplier: this.#multiplier,
      result: shot.result,
    });
    this.#heat = updateHeat(this.#heat, shot.result, shot.quality, shot.required);
    const sm = updateStreakMultiplier(this.#streak, shot.result);
    this.#streak = sm.streak;
    this.#multiplier = sm.multiplier;
    this.#best = Math.max(this.#best, this.#score);

    this.#emitResult(shot);
    this.#emitHeat(heatBefore, this.#heat);

    if (shot.result !== "miss") {
      this.#ticksSinceScore = 0;
      this.#lastScoreBig = shot.result === "swish" || shot.deep;
      this.#swishPulse = Math.min(1, this.#swishPulse + (shot.result === "swish" ? 1 : 0.5));
    }
    this.#ballInFlight = false;
    this.#phase = "scoredFeedback";
    this.#feedbackHold = C.SCORED_FEEDBACK_TICKS;
  }

  #decayScoredFeedback(): void {
    this.#feedbackHold -= 1;
    if (this.#feedbackHold <= 0) {
      this.#shot = undefined;
      this.#trail = [];
      this.#phase = "playing";
    }
  }

  // ── feedback ──────────────────────────────────────────────────────────────
  /** The two-part label (SPACE / RHYTHM) built at release. */
  #emitResult(shot: ActiveShot): void {
    this.#emit(shot.feedback);
  }

  #emitHeat(before: number, after: number): void {
    if (after > before && after === C.HEAT_MAX && before < C.HEAT_MAX) {
      this.#emit({ big: true, kind: "heatcheck", text: "HEAT CHECK" });
      return;
    }
    if (Math.floor(after) > Math.floor(before)) {
      this.#emit({ big: false, kind: "heatup", text: "HEAT UP" });
    }
  }

  #emit(event: Feedback): void {
    this.#events = pushFeedback(this.#events, event);
  }

  /** Drain and clear the buffered feedback events (the DOM HUD floats them). */
  public drainEvents(): readonly Feedback[] {
    const out = this.#events;
    this.#events = [];
    return out;
  }

  // ── ball dribble ──────────────────────────────────────────────────────────
  #dribbleBall(): void {
    const phase = (this.#tick % C.DRIBBLE_PERIOD_TICKS) / C.DRIBBLE_PERIOD_TICKS;
    const y = C.BALL_DRIBBLE_HEIGHT * Math.abs(Math.sin(Math.PI * phase));
    this.#ball = vec3(this.#playerX + 0.34, y + C.BALL_RADIUS, C.PLAYER_Z + 0.18);
  }

  #idleDribble(): void {
    // A gentle idle bounce on the ready / game-over screens.
    const phase = (this.#tick % C.DRIBBLE_PERIOD_TICKS) / C.DRIBBLE_PERIOD_TICKS;
    const y = 0.5 * C.BALL_DRIBBLE_HEIGHT * Math.abs(Math.sin(Math.PI * phase));
    this.#ball = vec3(0.34, y + C.BALL_RADIUS, C.PLAYER_Z + 0.18);
  }

  // ── derived ───────────────────────────────────────────────────────────────
  get #rhythmPhase(): number {
    return rhythmPhaseAt(this.#tick);
  }

  #finalWindowAt(elapsed: number): boolean {
    return C.ROUND_TICKS - elapsed <= C.FINAL_SECONDS_DOUBLE_POINTS * C.FIXED_HZ;
  }

  get #finalWindow(): boolean {
    const live = this.#phase === "playing" || this.#phase === "shooting" || this.#phase === "scoredFeedback";
    return live && this.#finalWindowAt(this.#elapsed);
  }

  #glow(): number {
    return this.#heat < 2 ? 0 : 0.35 + 0.65 * clamp((this.#heat - 2) / 3, 0, 1);
  }

  #pulse(): number {
    const shimmer = 0.15 * this.#glow() * (0.5 + 0.5 * Math.sin(this.#tick * 0.3));
    const finalShimmer = this.#finalWindow ? 0.12 * (0.5 + 0.5 * Math.sin(this.#tick * 0.6)) : 0;
    return clamp(this.#swishPulse + shimmer + finalShimmer, 0, 1);
  }

  #scoreFlash(): number {
    return this.#ticksSinceScore < SCORE_FLASH_TICKS ? 1 - this.#ticksSinceScore / SCORE_FLASH_TICKS : 0;
  }

  #cameraShake(): Vec3 {
    if (this.#ticksSinceScore >= SHAKE_TICKS) {
      return vec3(0, 0, 0);
    }
    const mag = C.CAMERA_SHAKE * (1 - this.#ticksSinceScore / SHAKE_TICKS) * (this.#lastScoreBig ? 2 : 1);
    return vec3(Math.sin(this.#tick * 2.7) * mag, Math.cos(this.#tick * 2.1) * mag * 0.5, 0);
  }

  #shotPose(): number {
    return this.#shot !== undefined ? this.#shot.t : 0;
  }

  /** The defender's contest-zone radius, shrinking when they're off balance. */
  #contestRadius(): number {
    return C.CONTEST_RADIUS * (0.35 + 0.65 * clamp(this.#defenderBalance, 0, 1));
  }

  // ── read-only snapshots ───────────────────────────────────────────────────

  /** The full scene snapshot for `scene.ts` (presentation only — cannot mutate play). */
  public view(): SceneView {
    return {
      advantage: this.#advantage,
      ball: this.#ball,
      ballInFlight: this.#ballInFlight,
      cameraShake: this.#cameraShake(),
      contestRadius: this.#contestRadius(),
      defenderBalance: this.#defenderBalance,
      defenderX: this.#defenderX,
      finalWindow: this.#finalWindow,
      windowActive: this.#phase === "playing" && this.#advantage >= C.ADVANTAGE_WINDOW_WEAK_THRESHOLD,
      glow: this.#glow(),
      heat: this.#heat,
      phase: this.#phase,
      playerLean: clamp(this.#playerVelX / C.PLAYER_MOVE_SPEED, -1, 1),
      playerX: this.#playerX,
      pulse: this.#pulse(),
      rhythmActive: this.#phase === "playing" && this.#holding,
      rhythmPhase: this.#rhythmPhase,
      scoreFlash: this.#scoreFlash(),
      shotPose: this.#shotPose(),
      trail: this.#trail,
    };
  }

  // HUD accessors (read each frame by game.ts → the DOM overlay).
  get phase(): SceneView["phase"] {
    return this.#phase;
  }
  get score(): number {
    return this.#score;
  }
  get best(): number {
    return Math.max(this.#best, this.#score);
  }
  get streak(): number {
    return this.#streak;
  }
  get multiplier(): number {
    return this.#multiplier;
  }
  get heat(): number {
    return this.#heat;
  }
  get finalWindow(): boolean {
    return this.#finalWindow;
  }
  get scorePop(): boolean {
    return this.#ticksSinceScore < SCORE_FLASH_TICKS;
  }
  get playerX(): number {
    return this.#playerX;
  }
  /**
   * The three live readiness tags (SPACE / RHYTHM / BALANCE) + quality for the meter —
   * present only while actively holding in play. Three separate axes, so the player
   * reads *why* a shot is good rather than waiting for one "guaranteed make" label.
   */
  readiness(): Readiness | undefined {
    if (this.#phase !== "playing" || !this.#holding) {
      return undefined;
    }
    const b = this.#breakdown;
    return {
      balance: computeBalanceTag(b.stability, this.#plantTicks),
      quality: b.quality,
      rhythm: computeRhythmTag(b.timing, this.#rhythmPhase),
      space: computeSpaceTag(this.#advantage, b.pressurePenalty),
    };
  }
  get timeRemaining(): number {
    const live = this.#phase !== "ready";
    return live ? Math.max(0, (C.ROUND_TICKS - this.#elapsed) / C.FIXED_HZ) : C.ROUND_SECONDS;
  }

  /** A cheap bounded digest of the observable state (replay-equality tests). */
  public hash(): number {
    const fields = [
      this.#tick,
      Math.round(this.#playerX * 100),
      Math.round(this.#defenderX * 100),
      Math.round(this.#defenderBalance * 1000),
      this.#score,
      this.#streak,
      this.#multiplier,
      this.#heat,
      this.#shotCount,
      this.#phase === "gameOver" ? 4 : this.#phase === "scoredFeedback" ? 3 : this.#phase === "shooting" ? 2 : this.#phase === "playing" ? 1 : 0,
    ];
    return fields.reduce((h, f) => (h * 1_000_003 + (f | 0)) % 2_147_483_647, 2_166_136_261);
  }
}
