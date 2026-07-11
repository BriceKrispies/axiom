/*
 * swing.ts — the spring-loaded bat: an explicit state machine
 * (idle → loading → loaded → swing → follow → recover → idle) plus the swept
 * bat-vs-ball contact resolution. The swing NEVER fires on press: holding winds
 * the bat back (fast at first, resisting toward full load), releasing snaps it
 * forward with load-scaled angular velocity, it overshoots into follow-through,
 * and recovery back to idle is deliberately slower than the strike. Pure and
 * deterministic — identical inputs produce identical poses. SDK-free.
 */

import { type Vec3, clamp, clamp01, mix, vec3 } from "./vec.ts";
import type { Contact, Swing } from "./types.ts";
import * as C from "./constants.ts";

export const newSwing = (): Swing => ({
  load: 0,
  omega: 0,
  state: "idle",
  stateTicks: 0,
  theta: C.THETA_IDLE,
});

/** The effective ω this tick, including the release snap ramp-up. */
export const effectiveOmega = (s: Swing): number => {
  if (s.state === "swing") {
    const snap = s.stateTicks < C.SNAP_TICKS ? mix(C.SNAP_START, 1, (s.stateTicks + 1) / (C.SNAP_TICKS + 1)) : 1;
    return s.omega * snap;
  }
  if (s.state === "follow") {
    return s.omega;
  }
  return 0;
};

/** Advance the swing one tick. `holding`/`released` come straight from the intent. */
export const stepSwing = (s: Swing, holding: boolean, released: boolean): Swing => {
  const t = s.stateTicks + 1;
  switch (s.state) {
    case "idle": {
      if (holding) {
        return { load: 0, omega: 0, state: "loading", stateTicks: 0, theta: s.theta };
      }
      return { ...s, stateTicks: t };
    }
    case "loading":
    case "loaded": {
      if (released || !holding) {
        // The committed forward swing — fires on RELEASE only.
        const omega0 = mix(C.OMEGA_MIN, C.OMEGA_MAX, s.load);
        return { load: s.load, omega: omega0, state: "swing", stateTicks: 0, theta: s.theta };
      }
      // Winding: quick at first, then resisting as it approaches maximum load.
      const load = clamp01(s.load + (1 - s.load) * C.LOAD_RATE);
      const theta = mix(C.THETA_IDLE, C.THETA_LOADED, load);
      const state = load >= C.LOAD_FULL ? "loaded" : "loading";
      return { load, omega: 0, state, stateTicks: state === s.state ? t : 0, theta };
    }
    case "swing": {
      const w = effectiveOmega({ ...s, stateTicks: t - 1 });
      const theta = s.theta + w;
      if (theta >= C.THETA_FOLLOW_START) {
        return { load: s.load, omega: s.omega, state: "follow", stateTicks: 0, theta };
      }
      return { ...s, stateTicks: t, theta };
    }
    case "follow": {
      const omega = s.omega * C.FOLLOW_DRAG;
      const theta = Math.min(C.THETA_FOLLOW_END, s.theta + omega);
      if (omega < C.FOLLOW_MIN_OMEGA || theta >= C.THETA_FOLLOW_END) {
        return { load: 0, omega: 0, state: "recover", stateTicks: 0, theta };
      }
      return { load: s.load, omega, state: "follow", stateTicks: t, theta };
    }
    case "recover": {
      // The bat does NOT teleport back — it eases home slower than it struck.
      const theta = s.theta + (C.THETA_IDLE - s.theta) * C.RECOVER_RATE;
      if (Math.abs(theta - C.THETA_IDLE) < C.RECOVER_EPSILON) {
        return { load: 0, omega: 0, state: "idle", stateTicks: 0, theta: C.THETA_IDLE };
      }
      return { load: 0, omega: 0, state: "recover", stateTicks: t, theta };
    }
    default:
      return s;
  }
};

/** Bat direction in the XZ plane at sweep angle θ. */
export const batDir = (theta: number): Vec3 => vec3(-Math.sin(theta), 0, -Math.cos(theta));

/** The tangential (strike) direction — where contact at θ sends the ball. */
export const batTangent = (theta: number): Vec3 => vec3(-Math.cos(theta), 0, Math.sin(theta));

/** The bat plane's height at θ — a slight uppercut rising through the arc. */
export const batPlaneY = (theta: number): number =>
  C.BAT_PLANE_Y + clamp(C.BAT_UPPERCUT * (theta - C.THETA_SWEET), -C.BAT_UPPERCUT_CLAMP, C.BAT_UPPERCUT_CLAMP);

/** Sweet-spot quality along the bat (gaussian around SWEET_SPOT_R). */
export const sweetQualityAt = (r: number): number => {
  const d = (r - C.SWEET_SPOT_R) / C.SWEET_SPOT_WIDTH;
  return Math.exp(-d * d);
};

/** Timing quality from the contact angle's distance to the sweet (square) angle. */
export const timingQualityAt = (theta: number): number => {
  const d = (theta - C.THETA_SWEET) / C.TIMING_WIDTH;
  return Math.exp(-d * d);
};

/** Resolve the exit ball from a contact at bat angle θ, radius r, vertical offset dy. */
export const resolveContact = (
  theta: number,
  omega: number,
  r: number,
  dy: number,
  point: Vec3,
  pitchVz: number,
): Contact => {
  const u = clamp01((r - C.BAT_GRIP_R) / (C.BAT_TIP_R - C.BAT_GRIP_R));
  const sweetQ = sweetQualityAt(r);
  const timingQ = timingQualityAt(theta);
  // Vertical mishit bleeds speed: clean inside VERT_CLEAN_DY, worst at the window edge.
  const vertMiss = clamp01((Math.abs(dy) - C.VERT_CLEAN_DY) / (C.CONTACT_HEIGHT - C.VERT_CLEAN_DY));
  const vertQ = 1 - vertMiss;
  const speedShare = mix(1, C.VERT_MISHIT_KEEP, vertMiss);

  // Exit speed: the bat's linear speed AT THE CONTACT POINT (tip beats handle),
  // shaped by the sweet spot AND how square the bat was (timing), plus a share of
  // the incoming pitch speed bounced back.
  const batPointSpeed = omega * r * C.FIXED_HZ;
  const squareness = mix(1 - C.TIMING_SPEED_SHARE, 1, timingQ);
  const exitSpeed =
    (batPointSpeed * C.HIT_POWER * (0.5 + 0.5 * sweetQ) + Math.abs(pitchVz) * C.FIXED_HZ * C.PITCH_BOUNCE_SHARE) *
    speedShare *
    squareness;

  // Direction: the bat's tangential direction at contact (early = pulled, late = pushed);
  // loft from the vertical contact offset (undercut lifts, topping drives down).
  const tangent = batTangent(theta);
  const spray = Math.atan2(tangent.x, tangent.z);
  const loft = clamp(C.LOFT_BASE + dy * C.LOFT_GAIN, C.LOFT_MIN, C.LOFT_MAX);
  const horizontal = (exitSpeed * Math.cos(loft)) / C.FIXED_HZ;
  const exitVel = vec3(tangent.x * horizontal, (exitSpeed * Math.sin(loft)) / C.FIXED_HZ, tangent.z * horizontal);

  const quality = 0.42 * sweetQ + 0.33 * timingQ + 0.25 * vertQ;
  return { exitSpeed, exitVel, loft, point, quality, r, spray, sweetQ, timingQ, u, vertQ };
};

/**
 * The swept bat-vs-ball contact test for one tick: both the bat sweep
 * (prevTheta → theta) and the ball segment (prevBall → ball) are subsampled
 * together, so neither the fast bat tip nor a fast pitch can tunnel through
 * the other. Returns the resolved contact at the FIRST touching substep, or null.
 */
export const sweptContact = (
  prevTheta: number,
  theta: number,
  omega: number,
  batterX: number,
  prevBall: Vec3,
  ball: Vec3,
  pitchVz: number,
): Contact | null => {
  for (let k = 1; k <= C.CONTACT_SUBSTEPS; k += 1) {
    const f = k / C.CONTACT_SUBSTEPS;
    const th = mix(prevTheta, theta, f);
    const bx = mix(prevBall.x, ball.x, f);
    const by = mix(prevBall.y, ball.y, f);
    const bz = mix(prevBall.z, ball.z, f);
    const d = batDir(th);
    // Project the ball onto the bat's XZ ray from the pivot.
    const relX = bx - batterX;
    const relZ = bz - C.BATTER_Z;
    const r = relX * d.x + relZ * d.z;
    if (r < C.BAT_GRIP_R || r > C.BAT_TIP_R) {
      continue;
    }
    const perpX = relX - r * d.x;
    const perpZ = relZ - r * d.z;
    const perp = Math.hypot(perpX, perpZ);
    const dy = by - batPlaneY(th);
    if (perp <= C.CONTACT_RADIUS && Math.abs(dy) <= C.CONTACT_HEIGHT) {
      return resolveContact(th, omega, r, dy, vec3(bx, by, bz), pitchVz);
    }
  }
  return null;
};
