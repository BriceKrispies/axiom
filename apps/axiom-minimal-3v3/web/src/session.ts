/*
 * session.ts — `Mini3v3Session`, the framework-free heart of Minimal 3v3
 * Basketball. It imports nothing from `@axiom/game`: one deterministic tick per
 * `advance(intent)`, no wall clock, no unseeded randomness (the only roll is the
 * seeded hash in gameplay.ts). The state machine:
 *
 *   playing ──Space──▶ shooting ──release──▶ shotResult ──0.8s──▶ (reset) playing
 *      │                   │
 *      │ pass intercepted  │ defender touch during gather (steal)
 *      └──────────────▶ turnoverResult ──0.8s──▶ (reset) playing
 *
 * The player always controls the current blue ball handler; a completed Q/E pass
 * transfers control to the receiver. `view()` exposes a read-only `SceneView` for
 * the renderer, and `hash()` a replay-equality digest for the tests.
 */

import { type Vec3, ZERO, add, clamp, distXZ, lerp, mix, normalizeXZ, rotY, scale, sub, vec3 } from "./vec.ts";
import type { BallState, FigureView, Intent, Phase, ResultKind, SceneView, ShotArc, TimingTag } from "./types.ts";
import {
  clampToBounds,
  computeShotChance,
  defenderJumpY,
  defenderTarget,
  jumpY,
  makeArc,
  missEndpoint,
  passIntercepted,
  passTargets,
  primaryDefenderIndex,
  rollShot,
  sampleArc,
  stealTouch,
  teammateTarget,
} from "./gameplay.ts";
import * as C from "./constants.ts";

interface BlueState {
  pos: Vec3;
  vel: Vec3;
  yaw: number;
}

interface DefenderState {
  pos: Vec3;
  vel: Vec3;
  yaw: number;
  /** Ticks since the contest jump started; -1 while grounded. */
  jumpTick: number;
  /** Ticks until the next contest jump is allowed. */
  cooldown: number;
}

interface PassFlight {
  readonly arc: ShotArc;
  tick: number;
  readonly target: 0 | 1 | 2;
}

interface ShotFlight {
  readonly arc: ShotArc;
  readonly flightTicks: number;
  tick: number;
  readonly made: boolean;
}

/** Each blue's off-ball home spot, by player index (0 = top of key, 1 = left, 2 = right). */
const HOME_SLOTS: readonly [Vec3, Vec3, Vec3] = [C.RESET_HANDLER, C.RESET_WING_LEFT, C.RESET_WING_RIGHT];

const yawToward = (from: Vec3, to: Vec3): number => Math.atan2(to.x - from.x, to.z - from.z);

export class Mini3v3Session {
  #tick = 0;
  #phase: Phase = "playing";
  #resultTicks = 0;
  #settleTick = 0;
  #justReset = false;

  #controlled: 0 | 1 | 2 = 0;
  #blues: [BlueState, BlueState, BlueState];
  #defenders: [DefenderState, DefenderState, DefenderState];

  #ballState: BallState = "held";
  #ballPos: Vec3 = ZERO;
  #pass: PassFlight | undefined;
  /** Tick the Space press started the gather; -1 outside a shot. */
  #gatherStart = -1;
  #shot: ShotFlight | undefined;

  #attempts = 0;
  #makes = 0;
  #lastResult: ResultKind | undefined;
  #lastTiming: TimingTag | undefined;

  constructor() {
    this.#blues = [
      { pos: C.RESET_HANDLER, vel: ZERO, yaw: 0 },
      { pos: C.RESET_WING_LEFT, vel: ZERO, yaw: 0 },
      { pos: C.RESET_WING_RIGHT, vel: ZERO, yaw: 0 },
    ];
    this.#defenders = [
      { cooldown: 0, jumpTick: -1, pos: ZERO, vel: ZERO, yaw: 0 },
      { cooldown: 0, jumpTick: -1, pos: ZERO, vel: ZERO, yaw: 0 },
      { cooldown: 0, jumpTick: -1, pos: ZERO, vel: ZERO, yaw: 0 },
    ];
    this.#reset();
    this.#justReset = false;
  }

  // ── public surface ──────────────────────────────────────────────────────────

  get phase(): Phase {
    return this.#phase;
  }

  get controlledIndex(): 0 | 1 | 2 {
    return this.#controlled;
  }

  get attempts(): number {
    return this.#attempts;
  }

  get makes(): number {
    return this.#makes;
  }

  /** The result being displayed, during shotResult / turnoverResult only. */
  get resultKind(): ResultKind | undefined {
    return this.#phase === "shotResult" || this.#phase === "turnoverResult" ? this.#lastResult : undefined;
  }

  /** The last release's timing tag (survives into the result display). */
  get timingTag(): TimingTag | undefined {
    return this.#lastTiming;
  }

  get possessionLabel(): string {
    if (this.#phase === "shooting") {
      return "SHOOTING";
    }
    if (this.#ballState === "pass") {
      return "PASS IN FLIGHT";
    }
    return "YOU HAVE THE BALL";
  }

  /** Advance exactly one deterministic 60 Hz tick. */
  advance(intent: Intent): void {
    this.#tick += 1;
    this.#justReset = false;
    if (intent.reset) {
      this.#reset();
      return;
    }
    switch (this.#phase) {
      case "playing":
        this.#stepPlaying(intent);
        break;
      case "shooting":
        this.#stepShooting(intent);
        break;
      case "shotResult":
      case "turnoverResult":
        this.#stepResult();
        break;
    }
  }

  /** The read-only snapshot the scene renders. */
  view(): SceneView {
    return {
      ball: this.#ballPos,
      blues: [this.#blueView(0), this.#blueView(1), this.#blueView(2)],
      cameraAnchor: this.#blues[this.#controlled].pos,
      controlledIndex: this.#controlled,
      defenders: [this.#defenderView(0), this.#defenderView(1), this.#defenderView(2)],
      justReset: this.#justReset,
      phase: this.#phase,
      resultKind: this.resultKind,
      tick: this.#tick,
    };
  }

  /** A replay-equality digest: identical Intent streams → identical hashes. */
  hash(): number {
    const MOD = 2147483647;
    let h = 7;
    const fold = (f: number): void => {
      h = (h * 1000003 + (f | 0)) % MOD;
    };
    const foldVec = (v: Vec3): void => {
      fold(Math.round(v.x * 100));
      fold(Math.round(v.y * 100));
      fold(Math.round(v.z * 100));
    };
    fold(this.#tick);
    fold(["playing", "shooting", "shotResult", "turnoverResult"].indexOf(this.#phase));
    fold(this.#controlled);
    fold(this.#attempts);
    fold(this.#makes);
    for (const b of this.#blues) {
      foldVec(b.pos);
    }
    for (const d of this.#defenders) {
      foldVec(d.pos);
      fold(d.jumpTick);
      fold(d.cooldown);
    }
    foldVec(this.#ballPos);
    return h;
  }

  // ── reset ───────────────────────────────────────────────────────────────────

  #reset(): void {
    this.#blues[0].pos = C.RESET_HANDLER;
    this.#blues[1].pos = C.RESET_WING_LEFT;
    this.#blues[2].pos = C.RESET_WING_RIGHT;
    this.#blues.forEach((b) => {
      b.vel = ZERO;
      b.yaw = 0;
    });
    this.#defenders.forEach((d, i) => {
      d.pos = defenderTarget(i === 0, HOME_SLOTS[i]!, C.RESET_HANDLER);
      d.vel = ZERO;
      d.yaw = Math.PI;
      d.jumpTick = -1;
      d.cooldown = C.DEF_JUMP_COOLDOWN_BASE + i * C.DEF_JUMP_COOLDOWN_STAGGER;
    });
    this.#controlled = 0;
    this.#ballState = "held";
    this.#pass = undefined;
    this.#gatherStart = -1;
    this.#shot = undefined;
    this.#resultTicks = 0;
    this.#settleTick = 0;
    this.#phase = "playing";
    this.#justReset = true;
    this.#attachBall();
  }

  // ── playing ─────────────────────────────────────────────────────────────────

  #stepPlaying(intent: Intent): void {
    const handler = this.#blues[this.#controlled];
    if (this.#ballState === "held") {
      const wantsPass = intent.passLeft || intent.passRight;
      if (wantsPass) {
        this.#launchPass(intent.passLeft ? "left" : "right");
      } else if (intent.gatherPressed) {
        this.#gatherStart = this.#tick;
        this.#lastTiming = undefined;
        handler.vel = ZERO;
        this.#phase = "shooting";
      } else {
        this.#moveHandler(intent, handler);
      }
    }
    this.#stepTeammates();
    this.#stepDefenders(false);
    this.#stepBall();
  }

  #moveHandler(intent: Intent, handler: BlueState): void {
    const mag = Math.hypot(intent.moveX, intent.moveZ);
    const target =
      mag > 1e-6
        ? scale(vec3(intent.moveX / mag, 0, intent.moveZ / mag), C.PLAYER_SPEED * Math.min(1, mag))
        : ZERO;
    handler.vel = lerp(handler.vel, target, C.PLAYER_ACCEL);
    handler.pos = clampToBounds(add(handler.pos, handler.vel));
    const speed = distXZ(ZERO, handler.vel);
    handler.yaw = speed > 0.01 ? yawToward(ZERO, handler.vel) : yawToward(handler.pos, C.HOOP_POS);
  }

  #launchPass(side: "left" | "right"): void {
    const targets = passTargets(
      this.#blues.map((b) => b.pos),
      this.#controlled,
    );
    const target = targets[side] as 0 | 1 | 2;
    const catchPos = add(this.#blues[target].pos, vec3(0, C.PASS_CATCH_Y, 0));
    this.#pass = { arc: makeArc(this.#ballPos, catchPos, C.PASS_ARC_HEIGHT), target, tick: 0 };
    this.#ballState = "pass";
  }

  // ── shooting ────────────────────────────────────────────────────────────────

  #stepShooting(intent: Intent): void {
    const g = this.#tick - this.#gatherStart;
    if (this.#ballState === "held") {
      if (stealTouch(this.#blues[this.#controlled].pos, this.#defenderPositions())) {
        this.#turnover("stolen");
        return;
      }
      if (intent.gatherReleased || g >= C.AUTO_RELEASE_TICK) {
        this.#release(g);
      }
    }
    this.#stepTeammates();
    this.#stepDefenders(this.#ballState === "held");
    if (this.#ballState === "held") {
      this.#attachBall();
    } else if (this.#shot !== undefined) {
      this.#stepShotFlight();
    }
  }

  #release(releaseTick: number): void {
    const shooter = this.#blues[this.#controlled];
    const threats = this.#defenders.map((d) => ({
      jumping: defenderJumpY(d.jumpTick) > C.DEF_JUMPING_MIN_Y,
      pos: d.pos,
    }));
    const { chance, signedErr, tag, distance } = computeShotChance(releaseTick, shooter.pos, threats);
    this.#attempts += 1;
    this.#lastTiming = tag;
    const made = rollShot(chance, this.#attempts, this.#controlled, signedErr, distance);
    const end = made
      ? vec3(C.HOOP_POS.x, C.HOOP_Y, C.HOOP_POS.z)
      : missEndpoint(signedErr, shooter.pos, this.#attempts, this.#controlled);
    const height = 1.2 + Math.max(0, C.HOOP_Y - this.#ballPos.y) * 0.5;
    this.#shot = {
      arc: makeArc(this.#ballPos, end, height),
      flightTicks: clamp(Math.round(distance * 5), C.SHOT_FLIGHT_MIN, C.SHOT_FLIGHT_MAX),
      made,
      tick: 0,
    };
    this.#ballState = "shot";
  }

  #stepShotFlight(): void {
    const shot = this.#shot!;
    shot.tick += 1;
    const t = shot.tick / shot.flightTicks;
    this.#ballPos = sampleArc(shot.arc, t);
    if (t >= 1) {
      this.#lastResult = shot.made ? "made" : "miss";
      this.#makes += shot.made ? 1 : 0;
      this.#ballState = "dead";
      this.#settleTick = 0;
      this.#phase = "shotResult";
      this.#resultTicks = C.RESULT_TICKS;
    }
  }

  // ── result freeze ───────────────────────────────────────────────────────────

  #stepResult(): void {
    this.#resultTicks -= 1;
    if (this.#shot !== undefined && this.#settleTick < C.RIM_SETTLE_TICKS) {
      this.#settleTick += 1;
      this.#ballPos = this.#settledBall(this.#shot, this.#settleTick);
    }
    if (this.#resultTicks <= 0) {
      this.#reset();
    }
  }

  /** The canned post-arrival ball: a made shot drops through the net, a miss caroms off. */
  #settledBall(shot: ShotFlight, st: number): Vec3 {
    const end = shot.arc.end;
    if (shot.made) {
      const t = st / C.RIM_SETTLE_TICKS;
      return vec3(end.x, mix(C.HOOP_Y, C.BALL_RADIUS, t * t), end.z);
    }
    const away = normalizeXZ(C.HOOP_POS, end);
    const horiz = add(end, scale(away, 0.06 * st));
    const y = Math.max(C.BALL_RADIUS, end.y + 0.05 * st - 0.008 * st * st);
    return vec3(horiz.x, y, horiz.z);
  }

  // ── turnovers ───────────────────────────────────────────────────────────────

  #turnover(kind: ResultKind): void {
    this.#lastResult = kind;
    this.#ballState = "dead";
    this.#pass = undefined;
    this.#shot = undefined;
    this.#phase = "turnoverResult";
    this.#resultTicks = C.RESULT_TICKS;
  }

  // ── ball ────────────────────────────────────────────────────────────────────

  #attachBall(): void {
    const handler = this.#blues[this.#controlled];
    if (this.#phase === "shooting") {
      const g = this.#tick - this.#gatherStart;
      const raise = clamp((g - 6) / 12, 0, 1);
      const offset = rotY(vec3(0.15, 1.4 + raise * 0.6, 0.25), handler.yaw);
      this.#ballPos = add(add(handler.pos, offset), vec3(0, jumpY(g), 0));
      return;
    }
    const moving = distXZ(ZERO, handler.vel) > 0.01;
    const phase = (this.#tick % C.DRIBBLE_PERIOD) / C.DRIBBLE_PERIOD;
    const bounce = C.DRIBBLE_HEIGHT * Math.abs(Math.sin(Math.PI * phase)) * (moving ? 1 : 0.5);
    const offset = rotY(vec3(0.34, 0, 0.18), handler.yaw);
    this.#ballPos = add(add(handler.pos, offset), vec3(0, C.BALL_RADIUS + bounce, 0));
  }

  #stepBall(): void {
    if (this.#ballState === "held") {
      this.#attachBall();
      return;
    }
    if (this.#ballState === "pass" && this.#pass !== undefined) {
      const pass = this.#pass;
      pass.tick += 1;
      const t = pass.tick / C.PASS_TICKS;
      this.#ballPos = sampleArc(pass.arc, t);
      if (passIntercepted(this.#ballPos, this.#defenderPositions())) {
        this.#turnover("intercepted");
        return;
      }
      if (t >= 1) {
        this.#controlled = pass.target;
        this.#pass = undefined;
        this.#ballState = "held";
        this.#attachBall();
      }
    }
  }

  // ── AI steps ────────────────────────────────────────────────────────────────

  #defenderPositions(): Vec3[] {
    return this.#defenders.map((d) => d.pos);
  }

  /** Off-ball blues drift near their home slots; everyone freezes while a pass flies. */
  #stepTeammates(): void {
    if (this.#ballState === "pass") {
      return;
    }
    this.#blues.forEach((b, i) => {
      if (i === this.#controlled) {
        return;
      }
      const home = HOME_SLOTS[i]!;
      let nearest: Vec3 | undefined;
      let nearestDist = Number.POSITIVE_INFINITY;
      for (const d of this.#defenders) {
        const dist = distXZ(home, d.pos);
        if (dist < nearestDist) {
          nearest = d.pos;
          nearestDist = dist;
        }
      }
      const target = teammateTarget(home, nearest);
      const delta = sub(target, b.pos);
      const dl = distXZ(ZERO, delta);
      const desired = dl > 1e-6 ? scale(vec3(delta.x / dl, 0, delta.z / dl), Math.min(C.TEAMMATE_SPEED, dl)) : ZERO;
      b.vel = lerp(b.vel, desired, C.TEAMMATE_ACCEL);
      b.pos = clampToBounds(add(b.pos, b.vel));
      b.yaw = yawToward(b.pos, this.#ballPos);
    });
  }

  #stepDefenders(gathering: boolean): void {
    const handlerPos = this.#blues[this.#controlled].pos;
    const primary = primaryDefenderIndex(handlerPos, this.#defenderPositions());
    this.#defenders.forEach((d, i) => {
      const near = distXZ(d.pos, handlerPos) < C.CONTEST_TRIGGER_RADIUS;
      if (d.jumpTick < 0) {
        d.cooldown -= 1 + (near ? 1 : 0) + (gathering ? 1 : 0);
        if (d.cooldown <= 0 && near) {
          d.jumpTick = 0;
        }
      } else {
        d.jumpTick += 1;
        if (d.jumpTick >= C.DEF_JUMP_TICKS) {
          d.jumpTick = -1;
          d.cooldown = C.DEF_JUMP_COOLDOWN_BASE + i * C.DEF_JUMP_COOLDOWN_STAGGER;
        }
      }
      const target = defenderTarget(i === primary, this.#blues[i].pos, handlerPos);
      const delta = sub(target, d.pos);
      const dl = distXZ(ZERO, delta);
      const desired = dl > 1e-6 ? scale(vec3(delta.x / dl, 0, delta.z / dl), Math.min(C.DEFENDER_SPEED, dl)) : ZERO;
      d.vel = lerp(d.vel, desired, C.DEFENDER_SMOOTHING);
      d.pos = add(d.pos, d.vel);
      if (d.jumpTick >= 0 && d.jumpTick <= C.DEF_JUMP_APEX) {
        d.pos = add(d.pos, scale(normalizeXZ(d.pos, handlerPos), C.DEF_LUNGE_SPEED));
      }
    });
    // Pairwise separation in fixed index order — deterministic, applied through
    // position nudges small enough to stay smooth, then the bounds clamp.
    for (let i = 0; i < 3; i += 1) {
      for (let j = i + 1; j < 3; j += 1) {
        const a = this.#defenders[i]!;
        const b = this.#defenders[j]!;
        if (distXZ(a.pos, b.pos) < C.DEFENDER_SEPARATION) {
          const dir = distXZ(a.pos, b.pos) < 1e-6 ? vec3(1, 0, 0) : normalizeXZ(a.pos, b.pos);
          a.pos = add(a.pos, scale(dir, -C.DEFENDER_SEPARATION_PUSH));
          b.pos = add(b.pos, scale(dir, C.DEFENDER_SEPARATION_PUSH));
        }
      }
    }
    this.#defenders.forEach((d) => {
      d.pos = clampToBounds(d.pos);
      d.yaw = yawToward(d.pos, this.#ballPos);
    });
  }

  // ── figure views ────────────────────────────────────────────────────────────

  #lean(yaw: number, vel: Vec3, maxSpeed: number): { leanF: number; leanS: number } {
    const facing = vec3(Math.sin(yaw), 0, Math.cos(yaw));
    const right = vec3(Math.cos(yaw), 0, -Math.sin(yaw));
    const f = (vel.x * facing.x + vel.z * facing.z) / maxSpeed;
    const s = (vel.x * right.x + vel.z * right.z) / maxSpeed;
    return { leanF: clamp(f, -1, 1), leanS: clamp(s, -1, 1) };
  }

  #idleBob(seed: number, moving: boolean): number {
    if (moving) {
      return 0;
    }
    return C.BOB_AMPL * Math.sin((2 * Math.PI * (this.#tick + seed * 30)) / C.BOB_PERIOD);
  }

  #blueView(i: 0 | 1 | 2): FigureView {
    const b = this.#blues[i];
    const shooting = i === this.#controlled && this.#gatherStart >= 0 && this.#ballState !== "pass";
    const g = shooting ? this.#tick - this.#gatherStart : 0;
    const crouch = shooting ? clamp(g / C.GATHER_CROUCH_TICKS, 0, 1) * clamp(1 - g / C.JUMP_APEX_TICK, 0, 1) : 0;
    const armRaise = shooting ? clamp((g - 6) / 12, 0, 1) : 0;
    const moving = distXZ(ZERO, b.vel) > 0.005;
    return {
      armRaise,
      bobY: this.#idleBob(i, moving || shooting),
      crouch,
      jumpY: shooting ? jumpY(g) : 0,
      pos: b.pos,
      yaw: b.yaw,
      ...this.#lean(b.yaw, b.vel, C.PLAYER_SPEED),
    };
  }

  #defenderView(i: 0 | 1 | 2): FigureView {
    const d = this.#defenders[i];
    const moving = distXZ(ZERO, d.vel) > 0.005;
    return {
      armRaise: d.jumpTick >= 0 ? 1 : 0,
      bobY: this.#idleBob(i + 3, moving || d.jumpTick >= 0),
      crouch: 0,
      jumpY: defenderJumpY(d.jumpTick),
      pos: d.pos,
      yaw: d.yaw,
      ...this.#lean(d.yaw, d.vel, C.DEFENDER_SPEED),
    };
  }
}
