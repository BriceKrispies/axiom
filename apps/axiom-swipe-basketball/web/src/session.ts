/*
 * session.ts — THE game core. SDK-free and deterministic: given the same sequence
 * of `Intent`s it produces the same state every run, so every behaviour is
 * unit-testable in bare Node. It owns the balls, the held-ball drag, the
 * swipe-release throw, the fixed-step physics + shot-quality tracking, the moving
 * hoop, the ball feed, and it drives the pure 60-second arcade state machine
 * (`arcade.ts`). Scene + HUD only read it.
 *
 * A ball is in one of three modes: `rack` (selectable), `held` (dragged), or
 * `flight` (physics-driven, tracking rim/backboard touches, then recycled to the
 * rack — the arcade ball-return).
 */

import { type Vec2, type Vec3, clamp, lerp, length, vec2, vec3 } from "./vec.ts";
import { type Camera, type Mat4, invert, rayPlaneZ, unprojectRay, viewProjection } from "./projection.ts";
import { type Selectable, pickBall } from "./selection.ts";
import { type Colliders, buildColliders } from "./colliders.ts";
import { type Contact, stepFreeBall } from "./physics.ts";
import { PointerHistory } from "./pointer.ts";
import { swipeToThrow } from "./throw.ts";
import { scoredThroughHoop } from "./scoring.ts";
import {
  type ArcadeEvent,
  type ArcadeState,
  type RoundPhase,
  classifyShot,
  drainEvents,
  inFinalWindow,
  isGoldenSpawn,
  newRound,
  registerMake,
  registerMiss,
  registerShot,
  startIfReady,
  tick as arcadeTick,
} from "./arcade.ts";
import {
  BALL_COUNT,
  BALL_RADIUS,
  CABINET_FAR_Z,
  CABINET_HALF_WIDTH,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  CAMERA_POS,
  CAMERA_TARGET,
  DEFAULT_VIEWPORT,
  DRAG_PLANE_Z,
  DRAG_SMOOTHING,
  DT,
  FIXED_HZ,
  HOOP_Y,
  RACK_SPREAD,
  RACK_Y,
  RACK_Z,
  REST_SPEED,
  REST_TICKS,
  SHAKE_BIG,
  SHAKE_DECAY,
  SHAKE_SCORE,
} from "./constants.ts";

/** How the player is currently interacting, per ball. */
export type BallMode = "rack" | "held" | "flight";

/** One basketball's live state. */
interface Ball {
  pos: Vec3;
  vel: Vec3;
  mode: BallMode;
  restTicks: number;
  /** Whether this shot has already been counted (one score per throw). */
  scored: boolean;
  /** Whether this ball was thrown (so a non-scoring settle counts as a miss). */
  thrown: boolean;
  /** Did the ball touch the rim during this flight? */
  touchedRim: boolean;
  /** Did the ball touch the backboard during this flight? */
  touchedBackboard: boolean;
  /** Is this a golden (5-point) ball? */
  golden: boolean;
  /** The ball's home rack slot index. */
  readonly slot: number;
}

/** The per-tick input the session consumes. All fields are plain data (testable). */
export interface Intent {
  readonly pointer: Vec2 | null;
  readonly pressed: boolean;
  readonly released: boolean;
  readonly reset: boolean;
  readonly viewport?: Vec2;
}

/** A read-only view of one ball for the renderer. */
export interface BallView {
  readonly pos: Vec3;
  readonly mode: BallMode;
  readonly golden: boolean;
}

const CAMERA: Camera = {
  far: CAMERA_FAR,
  fovY: CAMERA_FOV_Y,
  near: CAMERA_NEAR,
  position: CAMERA_POS,
  target: CAMERA_TARGET,
  up: vec3(0, 1, 0),
};

/** The rack slot position for ball `i`, spread across the front of the shaft. */
const rackSlot = (i: number): Vec3 => {
  const spread = 2 * CABINET_HALF_WIDTH * RACK_SPREAD;
  const x = -spread / 2 + (spread * (i + 0.5)) / BALL_COUNT;
  return vec3(x, RACK_Y, RACK_Z);
};

/** The deterministic Swipe Basketball session. */
export class SwipeBasketballSession {
  readonly #history = new PointerHistory();
  #arcade: ArcadeState = newRound(0);
  #colliders: Colliders = buildColliders(0);
  #hoopOffsetX = 0;
  #spawnCounter = 0;
  #balls: Ball[] = [];
  #heldIndex = -1;
  #tick = 0;
  #shake = 0;
  #lastScoreTick = -1000;
  #lastScoreBig = false;
  #lastContact: Contact | null = null;
  #viewport: Vec2 = vec2(DEFAULT_VIEWPORT.x, DEFAULT_VIEWPORT.y);
  #viewProj: Mat4;
  #invViewProj: Mat4;

  public constructor() {
    this.#viewProj = viewProjection(CAMERA, this.#viewport.x / this.#viewport.y);
    this.#invViewProj = invert(this.#viewProj);
    this.#startRound(0);
  }

  // ── public accessors (scene + HUD read these) ──────────────────────────────

  public get score(): number {
    return this.#arcade.score;
  }

  public get best(): number {
    return this.#arcade.best;
  }

  public get shots(): number {
    return this.#arcade.shots;
  }

  public get streak(): number {
    return this.#arcade.consecutiveMakes;
  }

  public get multiplier(): number {
    return this.#arcade.multiplier;
  }

  public get phase(): RoundPhase {
    return this.#arcade.phase;
  }

  /** Seconds left in the round (for the HUD clock). */
  public get timeRemaining(): number {
    return this.#arcade.timeRemaining / FIXED_HZ;
  }

  /** Whether the round is in its final double-points window. */
  public get finalWindow(): boolean {
    return inFinalWindow(this.#arcade);
  }

  public get tick(): number {
    return this.#tick;
  }

  public get holding(): boolean {
    return this.#heldIndex >= 0;
  }

  /** The current lateral hoop offset (metres), for the scene to move the hoop group. */
  public get hoopOffsetX(): number {
    return this.#hoopOffsetX;
  }

  /** Ticks since the last made basket (for the score pop). */
  public get ticksSinceScore(): number {
    return this.#tick - this.#lastScoreTick;
  }

  /** Whether the last score was a big one (golden / streak-up), for a stronger flash. */
  public get lastScoreBig(): boolean {
    return this.#lastScoreBig;
  }

  /** The strongest contact recorded on the latest tick, or `null`. */
  public get lastContact(): Contact | null {
    return this.#lastContact;
  }

  /** A small deterministic camera-shake offset (decays after a score). */
  public cameraShakeOffset(): Vec3 {
    return vec3(this.#shake * Math.sin(this.#tick * 12.9), this.#shake * Math.cos(this.#tick * 9.7), 0);
  }

  /** Drain the pending feedback events (the HUD floats these each frame). */
  public drainEvents(): readonly ArcadeEvent[] {
    return drainEvents(this.#arcade);
  }

  /** A read-only snapshot of every ball for the renderer. */
  public ballViews(): readonly BallView[] {
    return this.#balls.map((b): BallView => ({ golden: b.golden, mode: b.mode, pos: b.pos }));
  }

  /** Restore the machine to a fresh round (best score preserved). */
  public reset(): void {
    this.#startRound(this.#arcade.best);
  }

  // ── the fixed-step update ──────────────────────────────────────────────────

  /** Advance one deterministic fixed tick from `intent`. */
  public advance(intent: Intent): void {
    this.#tick += 1;
    this.#lastContact = null;

    if (intent.reset) {
      this.reset();
      return;
    }
    this.#updateViewport(intent.viewport);

    const phase = this.#arcade.phase;
    if (phase === "gameover") {
      // Round over: a tap (or R, handled above) restarts. Balls freeze.
      if (intent.pressed) {
        this.reset();
      }
      this.#decayShake();
      return;
    }
    if (phase === "playing") {
      arcadeTick(this.#arcade);
    }
    // The clock may have just run out this tick — freeze if so.
    if (this.#arcade.phase === "gameover") {
      this.#decayShake();
      return;
    }

    this.#handlePointer(intent);
    this.#stepFlightBalls();
    this.#decayShake();
  }

  // ── internals ──────────────────────────────────────────────────────────────

  #startRound(best: number): void {
    this.#arcade = newRound(best);
    this.#hoopOffsetX = 0;
    this.#colliders = buildColliders(0);
    this.#spawnCounter = 0;
    this.#balls = [];
    for (let i = 0; i < BALL_COUNT; i += 1) {
      this.#balls.push(this.#freshBall(i));
    }
    this.#heldIndex = -1;
    this.#shake = 0;
    this.#lastScoreTick = -1000;
    this.#lastScoreBig = false;
    this.#lastContact = null;
    this.#history.clear();
  }

  /** Mint a rack ball for slot `i`, assigning golden by the running spawn counter. */
  #freshBall(i: number): Ball {
    this.#spawnCounter += 1;
    return {
      golden: isGoldenSpawn(this.#spawnCounter),
      mode: "rack",
      pos: rackSlot(i),
      restTicks: 0,
      scored: false,
      slot: i,
      thrown: false,
      touchedBackboard: false,
      touchedRim: false,
      vel: vec3(0, 0, 0),
    };
  }

  #decayShake(): void {
    this.#shake = this.#shake < 0.001 ? 0 : this.#shake * SHAKE_DECAY;
  }

  #updateViewport(viewport: Vec2 | undefined): void {
    if (viewport === undefined || (viewport.x === this.#viewport.x && viewport.y === this.#viewport.y)) {
      return;
    }
    this.#viewport = viewport;
    this.#viewProj = viewProjection(CAMERA, viewport.x / viewport.y);
    this.#invViewProj = invert(this.#viewProj);
  }

  #handlePointer(intent: Intent): void {
    const pointer = intent.pointer;
    if (pointer !== null) {
      this.#history.push(pointer.x, pointer.y, this.#tick);
    }

    // Grab a ball on press — the first grab starts the round clock.
    if (this.#heldIndex < 0 && intent.pressed && pointer !== null) {
      const selectables: Selectable[] = this.#balls.map((b): Selectable => ({ pos: b.pos, selectable: b.mode === "rack" }));
      const idx = pickBall(pointer, selectables, this.#viewProj, this.#viewport);
      if (idx >= 0) {
        startIfReady(this.#arcade);
        this.#heldIndex = idx;
        this.#balls[idx]!.mode = "held";
        this.#balls[idx]!.vel = vec3(0, 0, 0);
        this.#history.clear();
        this.#history.push(pointer.x, pointer.y, this.#tick);
      }
    }

    if (this.#heldIndex >= 0 && intent.released) {
      this.#release();
      return;
    }

    if (this.#heldIndex >= 0 && pointer !== null) {
      this.#dragHeld(pointer);
    }
  }

  #dragHeld(pointer: Vec2): void {
    const ray = unprojectRay(pointer.x, pointer.y, this.#viewport, this.#invViewProj);
    const hit = rayPlaneZ(ray, DRAG_PLANE_Z);
    if (hit === null) {
      return;
    }
    const target = vec3(
      clamp(hit.x, -CABINET_HALF_WIDTH + BALL_RADIUS, CABINET_HALF_WIDTH - BALL_RADIUS),
      clamp(hit.y, BALL_RADIUS + 0.1, HOOP_Y + 0.4),
      DRAG_PLANE_Z,
    );
    const ball = this.#balls[this.#heldIndex]!;
    ball.pos = lerp(ball.pos, target, DRAG_SMOOTHING);
  }

  #release(): void {
    const ball = this.#balls[this.#heldIndex]!;
    ball.vel = swipeToThrow(this.#history.releaseVelocity());
    ball.mode = "flight";
    ball.scored = false;
    ball.thrown = true;
    ball.touchedRim = false;
    ball.touchedBackboard = false;
    ball.restTicks = 0;
    registerShot(this.#arcade);
    this.#heldIndex = -1;
    this.#history.clear();
  }

  #stepFlightBalls(): void {
    for (const ball of this.#balls) {
      if (ball.mode !== "flight") {
        continue;
      }
      const prev = ball.pos;
      const result = stepFreeBall(prev, ball.vel, BALL_RADIUS, this.#colliders, DT);
      ball.pos = result.pos;
      ball.vel = result.vel;
      if (result.contact !== null) {
        this.#lastContact = result.contact;
        ball.touchedRim = ball.touchedRim || result.contact.material === "rim";
        ball.touchedBackboard = ball.touchedBackboard || result.contact.material === "backboard";
      }
      if (!ball.scored && scoredThroughHoop(prev, ball.pos, ball.vel, this.#hoopOffsetX)) {
        this.#onScored(ball);
      }
      this.#recycleIfDone(ball);
    }
  }

  /** Award a made basket: classify it, drive the arcade state, kick the camera, shift the hoop. */
  #onScored(ball: Ball): void {
    ball.scored = true;
    const quality = classifyShot(ball.touchedRim, ball.touchedBackboard);
    const prevMultiplier = this.#arcade.multiplier;
    registerMake(this.#arcade, quality, ball.golden);
    const big = ball.golden || this.#arcade.multiplier > prevMultiplier;
    this.#lastScoreTick = this.#tick;
    this.#lastScoreBig = big;
    this.#shake = Math.max(this.#shake, big ? SHAKE_BIG : SHAKE_SCORE);
  }

  #recycleIfDone(ball: Ball): void {
    const slow = length(ball.vel) < REST_SPEED;
    ball.restTicks = slow ? ball.restTicks + 1 : 0;
    const settled = ball.restTicks >= REST_TICKS;
    const outOfBounds =
      ball.pos.y < -0.6 || Math.abs(ball.pos.x) > 3 || ball.pos.z > 3 || ball.pos.z < CABINET_FAR_Z - 0.3;
    if (!settled && !outOfBounds) {
      return;
    }
    // A thrown ball that never scored is a miss — break the streak.
    if (ball.thrown && !ball.scored) {
      registerMiss(this.#arcade);
    }
    // Recycle to the rack as a freshly-spawned ball (re-rolls the golden feed).
    const fresh = this.#freshBall(ball.slot);
    ball.pos = fresh.pos;
    ball.vel = fresh.vel;
    ball.mode = "rack";
    ball.restTicks = 0;
    ball.scored = false;
    ball.thrown = false;
    ball.touchedRim = false;
    ball.touchedBackboard = false;
    ball.golden = fresh.golden;
  }
}

/** The rack slot positions (exported for the scene's rack geometry). */
export const rackPositions = (): Vec3[] => {
  const out: Vec3[] = [];
  for (let i = 0; i < BALL_COUNT; i += 1) {
    out.push(rackSlot(i));
  }
  return out;
};

export { RACK_Z };
