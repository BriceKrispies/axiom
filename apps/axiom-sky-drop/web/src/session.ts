/*
 * session.ts — THE game core. SDK-free and deterministic: given the same sequence of
 * `Intent`s it produces the same state every run, so every behaviour is unit-testable
 * in bare Node. It owns the rack of balls, the grab-and-carry, the fixed-step fall
 * under wind, the landings, and it drives the pure round state machine (`round.ts`).
 * Scene + HUD only read it.
 *
 * ## A rack, thrown as fast as you like
 *
 * The throw is the arcade cabinet's: you PICK A BALL UP and move it. While held, a ball
 * is driven kinematically across a horizontal plane at the stand's altitude, following
 * the finger with a little weight and bounded by `DRAG_REACH`. On release it keeps the
 * speed it was already travelling (`motion.ts`) — the game reads the throw off the
 * ball's own motion rather than interpreting a gesture into one.
 *
 * The moment a ball leaves your hand the next one is waiting at the stand. Balls are
 * therefore INDEPENDENT and CONCURRENT: several are falling while you are still
 * throwing, each carrying its own velocity, all in the same wind. That is why the ball
 * is an array here and not a single field, and why nothing in the update loop is
 * allowed to assume there is only one.
 *
 * The camera never moves during a round (see `viewpoint.ts`), so the projection
 * matrices are solved once per round rather than per tick.
 */

import { type Vec2, type Vec3, add, length, lerp, sub, vec2, vec3 } from "./vec.ts";
import { type AimBasis, type Viewpoint, aimBasis, standView } from "./viewpoint.ts";
import { type RoundConditions, roundConditions } from "./conditions.ts";
import { horizontalDistance, stepBall } from "./physics.ts";
import { type Mat4, invert, rayPlaneY, unprojectRay, viewProjection } from "./projection.ts";
import { pointerGrabsBall } from "./selection.ts";
import { BallMotion } from "./motion.ts";
import {
  type RoundPhase,
  type RoundState,
  type ThrowResult,
  ballsLeft,
  bestLanding,
  bullseyeCount,
  hasBallInHand,
  newRound,
  onTargetCount,
  recordLanding,
  recordThrow,
  settle,
  totalScore,
} from "./round.ts";
import {
  BALLS_PER_ROUND,
  BALL_RADIUS,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  DEFAULT_VIEWPORT,
  DRAG_REACH,
  DRAG_SMOOTHING,
  DT,
  GROUND_Y,
  MAX_RELEASE_SPEED,
  REST_SPEED,
  REST_TICKS,
  SEED,
} from "./constants.ts";

/** The target sits at the world origin, on the ground. */
const AIM_POINT: Vec3 = vec3(0, GROUND_Y, 0);

/**
 * What a ball is doing.
 *
 * `"racked"` and `"ready"` are deliberately distinct. A racked ball is still in the
 * bag: it has no position worth drawing and must not be rendered. A ready ball is
 * sitting at the stand waiting to be picked up, and MUST be rendered — collapsing the
 * two once made the ball in your hand invisible, which is only survivable if, like the
 * tests, you aim at its coordinates rather than at what you can see.
 */
export type BallState = "racked" | "ready" | "held" | "flying" | "down";

interface Ball {
  pos: Vec3;
  vel: Vec3;
  state: BallState;
  restTicks: number;
  /** Where it first touched the ground, or `null` while it is still up. */
  landedAt: Vec3 | null;
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
  readonly state: BallState;
}

/** The deterministic Sky Drop session. */
export class SkyDropSession {
  /** The held ball's recent positions — the throw is read off these. */
  readonly #motion = new BallMotion();
  #round: RoundState = newRound(0);
  #conditions: RoundConditions = roundConditions(SEED, 0);
  #roundIndex = 0;
  #balls: Ball[] = [];
  /** Index of the ball in hand or waiting at the stand; `-1` once the rack is empty. */
  #ready = -1;
  #holding = false;
  /** The swinging hand: smoothed, UNBOUNDED, and what the throw is measured from. */
  #carry: Vec3 = vec3(0, 0, 0);
  #tick = 0;
  #viewport: Vec2 = vec2(DEFAULT_VIEWPORT.x, DEFAULT_VIEWPORT.y);
  #camera: Viewpoint = { position: vec3(0, 0, 0), target: vec3(0, 0, 0) };
  #viewProj: Mat4 = [];
  #invViewProj: Mat4 = [];

  public constructor() {
    this.#beginRound(0);
  }

  // ── public accessors (scene + HUD read these) ──────────────────────────────

  public get phase(): RoundPhase {
    return this.#round.phase;
  }

  /** Balls still to throw, including the one in hand. */
  public get ballsLeft(): number {
    return ballsLeft(this.#round);
  }

  public get ballsTotal(): number {
    return BALLS_PER_ROUND;
  }

  /** Which ball of the rack is in hand (1-based, for the HUD). */
  public get ballNumber(): number {
    return Math.min(this.#round.thrown + 1, BALLS_PER_ROUND);
  }

  /**
   * The round's score. Meaningful only in the `"results"` phase — the HUD must not
   * display it before then (see `round.ts`).
   */
  public get score(): number {
    return totalScore(this.#round);
  }

  public get best(): number {
    return this.#round.best;
  }

  public get bullseyes(): number {
    return bullseyeCount(this.#round);
  }

  public get onTarget(): number {
    return onTargetCount(this.#round);
  }

  /** The tightest landing of the round (m), or `null` if nothing has landed. */
  public get tightest(): number | null {
    return bestLanding(this.#round);
  }

  /** Every landing of the round, in the order the balls came down. */
  public get results(): readonly ThrowResult[] {
    return this.#round.results;
  }

  /** The wind for this round — drift speed (m/s) and bearing (radians). */
  public get windSpeed(): number {
    return this.#conditions.windSpeed;
  }

  public get windBearing(): number {
    return this.#conditions.windBearing;
  }

  public get windVector(): Vec3 {
    return this.#conditions.wind;
  }

  /** How far the stand is from the target (m) — the throw you have to make. */
  public get standDistance(): number {
    return this.#conditions.standDistance;
  }

  public get tick(): number {
    return this.#tick;
  }

  public get holding(): boolean {
    return this.#holding;
  }

  /** How many balls are in the air right now. */
  public get inFlight(): number {
    return this.#balls.filter((ball) => ball.state === "flying").length;
  }

  /** The speed (m/s) the held ball is being swung at — the scene's grab glow. */
  public get heldSpeed(): number {
    return this.#holding ? length(this.#motion.releaseVelocity(DT)) : 0;
  }

  /** The fixed camera for this round. */
  public get camera(): Viewpoint {
    return this.#camera;
  }

  /** The camera's horizontal basis — used by the HUD to orient the wind arrow. */
  public get basis(): AimBasis {
    return aimBasis(this.#conditions.stand, AIM_POINT);
  }

  /** Every ball the renderer should draw. */
  public ballViews(): readonly BallView[] {
    return this.#balls
      .filter((ball) => ball.state !== "racked")
      .map((ball): BallView => ({ pos: ball.pos, state: ball.state }));
  }

  /** The ball currently in hand or waiting at the stand, if any. */
  public readyBall(): BallView | null {
    const ball = this.#balls[this.#ready];
    return ball === undefined ? null : { pos: ball.pos, state: ball.state };
  }

  /** Start a completely fresh round (best score preserved). */
  public reset(): void {
    this.#beginRound(this.#roundIndex + 1);
  }

  // ── the fixed-step update ──────────────────────────────────────────────────

  /** Advance one deterministic fixed tick from `intent`. */
  public advance(intent: Intent): void {
    this.#tick += 1;
    this.#applyViewport(intent.viewport);

    if (intent.reset) {
      this.reset();
      return;
    }

    if (this.#round.phase === "results") {
      // A tap (or R, handled above) throws a fresh rack.
      if (intent.pressed) {
        this.reset();
      }
      return;
    }

    this.#handlePointer(intent);
    this.#stepBalls();
    settle(this.#round);
  }

  // ── internals ──────────────────────────────────────────────────────────────

  /** Rack up a fresh round: new stand, new wind, new camera, `BALLS_PER_ROUND` balls. */
  #beginRound(index: number): void {
    this.#roundIndex = index;
    this.#round = newRound(this.#round.best);
    this.#conditions = roundConditions(SEED, index);
    this.#balls = Array.from({ length: BALLS_PER_ROUND }, (): Ball => ({
      landedAt: null,
      pos: this.#conditions.stand,
      restTicks: 0,
      state: "racked",
      vel: vec3(0, 0, 0),
    }));
    this.#holding = false;
    this.#carry = this.#conditions.stand;
    this.#motion.clear();

    // The camera is fixed for the whole round, so solve it once here rather than
    // re-deriving an unchanging pose every tick.
    this.#camera = standView(this.#conditions.stand, AIM_POINT);
    this.#refreshMatrices();
    this.#offerNextBall();
  }

  /** Put the next racked ball at the stand, ready to pick up — or empty the hand. */
  #offerNextBall(): void {
    const next = this.#balls.findIndex((ball) => ball.state === "racked");
    this.#ready = hasBallInHand(this.#round) ? next : -1;
    const ball = this.#balls[this.#ready];
    if (ball !== undefined) {
      ball.pos = this.#conditions.stand;
      ball.state = "ready";
    }
    this.#carry = this.#conditions.stand;
    this.#motion.clear();
  }

  #applyViewport(viewport: Vec2 | undefined): void {
    const changed = viewport !== undefined && (viewport.x !== this.#viewport.x || viewport.y !== this.#viewport.y);
    this.#viewport = viewport ?? this.#viewport;
    if (changed) {
      this.#refreshMatrices();
    }
  }

  /** Rebuild the projection matrices for the round's fixed camera. */
  #refreshMatrices(): void {
    this.#viewProj = viewProjection(
      {
        far: CAMERA_FAR,
        fovY: CAMERA_FOV_Y,
        near: CAMERA_NEAR,
        position: this.#camera.position,
        target: this.#camera.target,
        up: vec3(0, 1, 0),
      },
      this.#viewport.x / Math.max(this.#viewport.y, 1),
    );
    this.#invViewProj = invert(this.#viewProj);
  }

  #handlePointer(intent: Intent): void {
    const ball = this.#balls[this.#ready];
    if (ball === undefined) {
      return;
    }

    // Picking a ball up: the press has to actually land on it.
    if (!this.#holding && intent.pressed && intent.pointer !== null) {
      const grabbed = pointerGrabsBall(intent.pointer, ball.pos, this.#viewProj, this.#viewport);
      this.#holding = grabbed;
      ball.state = grabbed ? "held" : "ready";
      this.#carry = ball.pos;
      this.#motion.clear();
      this.#motion.push(this.#carry, this.#tick);
      return;
    }

    if (this.#holding && intent.released) {
      this.#release(ball);
      return;
    }

    if (this.#holding && intent.pointer !== null) {
      this.#carryBall(ball, intent.pointer);
    }
  }

  /**
   * Carry the held ball to wherever the finger is, across the horizontal plane at the
   * stand's altitude.
   *
   * Two positions are tracked, and the distinction matters. `#carry` is the swinging
   * hand: it chases the finger with `DRAG_SMOOTHING` — never snapping, which is what
   * gives the throw its weight — and is UNBOUNDED. The visible ball is that point
   * clamped to `DRAG_REACH` of the stand.
   *
   * The throw is read off `#carry`, not off the clamped ball, because reading the
   * clamped position makes the reach limit silently eat throws: swing hard, hit the
   * rim, and the last samples before release are frozen, so the weighted average
   * collapses and a hard throw launches soft. Within reach the two are identical; only
   * at the rim does the ball strain against your reach while your hand keeps its speed.
   */
  #carryBall(ball: Ball, pointer: Vec2): void {
    const ray = unprojectRay(pointer.x, pointer.y, this.#viewport, this.#invViewProj);
    const hit = rayPlaneY(ray, this.#conditions.stand.y);
    if (hit === null) {
      return;
    }

    this.#carry = lerp(this.#carry, hit, DRAG_SMOOTHING);
    this.#motion.push(this.#carry, this.#tick);

    const fromStand = sub(this.#carry, this.#conditions.stand);
    const distance = Math.hypot(fromStand.x, fromStand.z);
    const scaled = distance > DRAG_REACH ? DRAG_REACH / distance : 1;
    ball.pos = vec3(
      this.#conditions.stand.x + fromStand.x * scaled,
      this.#conditions.stand.y,
      this.#conditions.stand.z + fromStand.z * scaled,
    );
  }

  /** Let go: the ball keeps its velocity, and the next one is immediately in reach. */
  #release(ball: Ball): void {
    const measured = this.#motion.releaseVelocity(DT);
    const speed = length(measured);
    // Purely a guard against a pointer teleport; a real throw never reaches it.
    const capped = speed > MAX_RELEASE_SPEED ? MAX_RELEASE_SPEED / speed : 1;
    // Horizontal only — the carry plane is level, so this just strips float noise.
    ball.vel = vec3(measured.x * capped, 0, measured.z * capped);
    ball.state = "flying";
    this.#holding = false;

    recordThrow(this.#round);
    this.#offerNextBall();
  }

  /** Integrate every airborne ball, and score each one's FIRST ground contact. */
  #stepBalls(): void {
    this.#balls.forEach((ball, index) => {
      if (ball.state !== "flying" && ball.state !== "down") {
        return;
      }
      const result = stepBall(ball.pos, ball.vel, BALL_RADIUS, this.#conditions.wind, DT);
      ball.pos = result.pos;
      ball.vel = result.vel;

      if (result.contact !== null && ball.landedAt === null) {
        ball.landedAt = result.contact.point;
        ball.state = "down";
        // Recorded silently: no verdict is shown until the whole rack is down.
        recordLanding(this.#round, index, horizontalDistance(result.contact.point));
      }

      // Freeze a settled ball so the grouping stays readable on the ground.
      ball.restTicks = length(ball.vel) < REST_SPEED ? ball.restTicks + 1 : 0;
      ball.vel = ball.restTicks >= REST_TICKS ? vec3(0, 0, 0) : ball.vel;
    });
  }
}
