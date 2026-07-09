/*
 * session.ts — THE game core. SDK-free and deterministic: given the same sequence
 * of `Intent`s (pointer position, press/release edges, reset) it produces the same
 * state every run, so every behaviour is unit-testable in bare Node. It owns the
 * balls, the held-ball drag, the swipe-release throw, the fixed-step physics, the
 * one-way scoring, the score/shot tallies, and ball recycling — everything except
 * how it looks (scene.ts) and how input is gathered (game.ts).
 *
 * A ball is in one of three modes:
 *   - `rack`   — at rest in the foreground rack, selectable;
 *   - `held`   — grabbed, driven kinematically toward the pointer on a drag plane;
 *   - `flight` — released, simulated purely by physics until it settles or leaves,
 *                then recycled back to its rack slot so play never stalls.
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
  HOOP_Y,
  RACK_SPREAD,
  RACK_Y,
  RACK_Z,
  REST_SPEED,
  REST_TICKS,
} from "./constants.ts";

/** How the player is currently interacting, per ball. */
export type BallMode = "rack" | "held" | "flight";

/** One basketball's live state. */
export interface Ball {
  pos: Vec3;
  vel: Vec3;
  mode: BallMode;
  /** Ticks the ball has been slow-and-low (for recycle timing). */
  restTicks: number;
  /** Whether this shot has already been counted (one score per throw). */
  scored: boolean;
  /** The ball's home rack slot index. */
  readonly slot: number;
}

/** The per-tick input the session consumes. All fields are plain data (testable). */
export interface Intent {
  /** Pointer position in canvas pixels, or `null` when there is no contact. */
  readonly pointer: Vec2 | null;
  /** Pointer went down THIS tick (down-edge). */
  readonly pressed: boolean;
  /** Pointer came up THIS tick (up-edge). */
  readonly released: boolean;
  /** Reset requested this tick (R key). */
  readonly reset: boolean;
  /** The canvas backing size in pixels (for projection); optional, defaults kept. */
  readonly viewport?: Vec2;
}

/** A read-only view of one ball for the renderer. */
export interface BallView {
  readonly pos: Vec3;
  readonly mode: BallMode;
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
  readonly #colliders: Colliders = buildColliders();
  readonly #history = new PointerHistory();
  #balls: Ball[] = [];
  #heldIndex = -1;
  #score = 0;
  #shots = 0;
  #tick = 0;
  #lastScoreTick = -1000;
  #lastContact: Contact | null = null;
  #viewport: Vec2 = vec2(DEFAULT_VIEWPORT.x, DEFAULT_VIEWPORT.y);
  #viewProj: Mat4;
  #invViewProj: Mat4;

  public constructor() {
    this.#viewProj = viewProjection(CAMERA, this.#viewport.x / this.#viewport.y);
    this.#invViewProj = invert(this.#viewProj);
    this.reset();
  }

  // ── public accessors (scene + HUD read these) ──────────────────────────────

  public get score(): number {
    return this.#score;
  }

  public get shots(): number {
    return this.#shots;
  }

  public get tick(): number {
    return this.#tick;
  }

  public get holding(): boolean {
    return this.#heldIndex >= 0;
  }

  /** Ticks since the last made basket (for the score pop); large when none recently. */
  public get ticksSinceScore(): number {
    return this.#tick - this.#lastScoreTick;
  }

  /** The strongest contact recorded on the latest tick, or `null`. */
  public get lastContact(): Contact | null {
    return this.#lastContact;
  }

  /** A read-only snapshot of every ball for the renderer. */
  public ballViews(): readonly BallView[] {
    return this.#balls.map((b): BallView => ({ mode: b.mode, pos: b.pos }));
  }

  /** Restore the machine to its start state: full rack, zeroed score/shots, nothing held. */
  public reset(): void {
    this.#balls = [];
    for (let i = 0; i < BALL_COUNT; i += 1) {
      this.#balls.push({ mode: "rack", pos: rackSlot(i), restTicks: 0, scored: false, slot: i, vel: vec3(0, 0, 0) });
    }
    this.#heldIndex = -1;
    this.#score = 0;
    this.#shots = 0;
    this.#lastScoreTick = -1000;
    this.#lastContact = null;
    this.#history.clear();
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
    this.#handlePointer(intent);
    this.#stepFlightBalls();
  }

  // ── internals ──────────────────────────────────────────────────────────────

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

    // Grab a ball on press.
    if (this.#heldIndex < 0 && intent.pressed && pointer !== null) {
      const selectables: Selectable[] = this.#balls.map((b): Selectable => ({ pos: b.pos, selectable: b.mode === "rack" }));
      const idx = pickBall(pointer, selectables, this.#viewProj, this.#viewport);
      if (idx >= 0) {
        this.#heldIndex = idx;
        this.#balls[idx]!.mode = "held";
        this.#balls[idx]!.vel = vec3(0, 0, 0);
        this.#history.clear();
        this.#history.push(pointer.x, pointer.y, this.#tick);
      }
    }

    // Release into flight.
    if (this.#heldIndex >= 0 && intent.released) {
      this.#release();
      return;
    }

    // Drag the held ball toward the pointer on the near interaction plane.
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
    const throwVel = swipeToThrow(this.#history.releaseVelocity());
    ball.vel = throwVel;
    ball.mode = "flight";
    ball.scored = false;
    ball.restTicks = 0;
    this.#shots += 1;
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
      }
      if (!ball.scored && scoredThroughHoop(prev, ball.pos, ball.vel)) {
        ball.scored = true;
        this.#score += 1;
        this.#lastScoreTick = this.#tick;
      }
      this.#recycleIfDone(ball);
    }
  }

  #recycleIfDone(ball: Ball): void {
    // A ball that has been slow for a while ANYWHERE is done — a heavy ball that
    // trickles to a stop (on the ramp, in a corner, or behind the backboard where
    // it can slip under the tall board) must still return to the rack, not creep
    // forever. The forward speed of a live shot keeps it above REST_SPEED even at
    // its apex, so this never recycles a ball mid-flight.
    const slow = length(ball.vel) < REST_SPEED;
    ball.restTicks = slow ? ball.restTicks + 1 : 0;
    const settled = ball.restTicks >= REST_TICKS;
    const outOfBounds =
      ball.pos.y < -0.6 || Math.abs(ball.pos.x) > 3 || ball.pos.z > 3 || ball.pos.z < CABINET_FAR_Z - 0.3;
    if (settled || outOfBounds) {
      ball.pos = rackSlot(ball.slot);
      ball.vel = vec3(0, 0, 0);
      ball.mode = "rack";
      ball.restTicks = 0;
      ball.scored = false;
    }
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
