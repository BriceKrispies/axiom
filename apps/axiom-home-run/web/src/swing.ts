/*
 * swing.ts — the always-armed bat: an explicit state machine
 * (ready → swing → follow → rewind → ready) plus the swept bat-vs-ball contact
 * resolution. The batter STARTS wound at full power: one press fires the
 * max-power forward swing instantly, the bat overshoots into follow-through,
 * then re-winds ON ITS OWN back to the ready stance — that rewind is the swing
 * cooldown (pressing during it does nothing). `readiness` (0…1, 1 = ready)
 * drives the HUD's ready indicator. Pure and deterministic — identical inputs
 * produce identical poses. SDK-free.
 */

import { type Vec3, clamp, clamp01, mix, vec3 } from "./vec.ts";
import type { Contact, Swing } from "./types.ts";
import * as C from "./constants.ts";

export const newSwing = (): Swing => ({
  omega: 0,
  readiness: 1,
  state: "ready",
  stateTicks: 0,
  theta: C.THETA_READY,
});

/** Rewind progress (0 just after follow-through … 1 back at the ready stance). */
const rewindReadiness = (theta: number): number =>
  clamp01((C.THETA_FOLLOW_END - theta) / (C.THETA_FOLLOW_END - C.THETA_READY));

/** The effective ω this tick, including the strike's snap ramp-up. */
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

/** Advance the swing one tick. `swingPressed` is the press EDGE from the intent. */
export const stepSwing = (s: Swing, swingPressed: boolean): Swing => {
  const t = s.stateTicks + 1;
  switch (s.state) {
    case "ready": {
      if (swingPressed) {
        // The committed full-power swing — fires the instant it is pressed.
        return { omega: C.OMEGA_SWING, readiness: 0, state: "swing", stateTicks: 0, theta: s.theta };
      }
      return { ...s, stateTicks: t };
    }
    case "swing": {
      const w = effectiveOmega({ ...s, stateTicks: t - 1 });
      const theta = s.theta + w;
      if (theta >= C.THETA_FOLLOW_START) {
        return { omega: s.omega, readiness: 0, state: "follow", stateTicks: 0, theta };
      }
      return { ...s, stateTicks: t, theta };
    }
    case "follow": {
      const omega = s.omega * C.FOLLOW_DRAG;
      const theta = Math.min(C.THETA_FOLLOW_END, s.theta + omega);
      if (omega < C.FOLLOW_MIN_OMEGA || theta >= C.THETA_FOLLOW_END) {
        return { omega: 0, readiness: rewindReadiness(theta), state: "rewind", stateTicks: 0, theta };
      }
      return { omega, readiness: 0, state: "follow", stateTicks: t, theta };
    }
    case "rewind": {
      // The cooldown: the batter re-winds the bat on his own — a press does nothing.
      const theta = s.theta + (C.THETA_READY - s.theta) * C.REWIND_RATE;
      if (Math.abs(theta - C.THETA_READY) < C.REWIND_EPSILON) {
        return { omega: 0, readiness: 1, state: "ready", stateTicks: 0, theta: C.THETA_READY };
      }
      return { omega: 0, readiness: rewindReadiness(theta), state: "rewind", stateTicks: t, theta };
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
