/*
 * physics.ts — the deterministic, fixed-step ball simulator, owned entirely by the
 * app (SDK-free). The engine's TS `Sim.physics` facade can't do restitution,
 * friction, static-geometry collision, or read a body's velocity back, so — like
 * the soccer app — the ball is genuinely simulated here: semi-implicit Euler under
 * gravity + air damping, then resolved against the machine's static colliders
 * (`colliders.ts`) with restitution and tangential friction. Same inputs → same
 * outputs every tick, so a shot is replayable and unit-testable.
 *
 * A ball is NOT hand-animated after release: `stepFreeBall` advances it purely by
 * physics. While a ball is *held*, it is driven kinematically (see session.ts) and
 * this stepper is not called for it.
 */

import { type Vec3, add, clampToBox, dot, length, normalize, scale, sub, vec3 } from "./vec.ts";
import type { Colliders, ContactMaterial } from "./colliders.ts";
import {
  GRAVITY,
  LINEAR_DAMPING,
  POST_COLLISION_DAMPING,
  RESTITUTION_BACKBOARD,
  RESTITUTION_DEFAULT,
  RESTITUTION_RIM,
  TANGENTIAL_FRICTION,
} from "./constants.ts";

/** The restitution for a contact material — rim is deadest, backboard liveliest. */
const restitutionFor = (material: ContactMaterial): number =>
  material === "rim" ? RESTITUTION_RIM : material === "backboard" ? RESTITUTION_BACKBOARD : RESTITUTION_DEFAULT;

/** A contact reported by a step, for hit feedback (juice). */
export interface Contact {
  readonly material: ContactMaterial;
  readonly point: Vec3;
  /** The closing speed along the contact normal before the bounce (≥ 0). */
  readonly impactSpeed: number;
}

/** The outcome of stepping one free ball. */
export interface StepResult {
  readonly pos: Vec3;
  readonly vel: Vec3;
  /** The strongest contact this step, or `null` if the ball touched nothing. */
  readonly contact: Contact | null;
}

/**
 * Reflect `vel` about `normal` with the material's restitution (normal component),
 * keep the tangential part scaled by friction, then bleed a MODERATE amount of
 * energy off the whole result (`POST_COLLISION_DAMPING`) so a heavy ball settles
 * instead of ping-ponging forever.
 */
const bounce = (vel: Vec3, normal: Vec3, restitution: number): Vec3 => {
  const vn = dot(vel, normal);
  const normalPart = scale(normal, vn);
  const tangentPart = sub(vel, normalPart);
  const reflected = scale(normal, -vn * restitution);
  const bounced = add(scale(tangentPart, TANGENTIAL_FRICTION), reflected);
  return scale(bounced, POST_COLLISION_DAMPING);
};

/** Resolve the ball against one plane half-space; returns the corrected pos/vel + optional contact. */
const resolvePlane = (
  pos: Vec3,
  vel: Vec3,
  radius: number,
  planePoint: Vec3,
  planeNormal: Vec3,
  material: ContactMaterial,
): { pos: Vec3; vel: Vec3; contact: Contact | null } => {
  const signedDist = dot(sub(pos, planePoint), planeNormal);
  const approaching = dot(vel, planeNormal) < 0;
  if (signedDist >= radius || !approaching) {
    return { contact: null, pos, vel };
  }
  const impactSpeed = -dot(vel, planeNormal);
  const correctedPos = add(pos, scale(planeNormal, radius - signedDist));
  const contactPoint = sub(correctedPos, scale(planeNormal, radius));
  return {
    contact: { impactSpeed, material, point: contactPoint },
    pos: correctedPos,
    vel: bounce(vel, planeNormal, restitutionFor(material)),
  };
};

/** Resolve the ball against one AABB box; returns the corrected pos/vel + optional contact. */
const resolveBox = (
  pos: Vec3,
  vel: Vec3,
  radius: number,
  center: Vec3,
  half: Vec3,
  material: ContactMaterial,
): { pos: Vec3; vel: Vec3; contact: Contact | null } => {
  const closest = clampToBox(pos, center, half);
  const diff = sub(pos, closest);
  const dist = length(diff);
  if (dist >= radius) {
    return { contact: null, pos, vel };
  }
  const normal = dist > 1e-6 ? scale(diff, 1 / dist) : vec3(0, 1, 0);
  const approaching = dot(vel, normal) < 0;
  if (!approaching) {
    // Overlapping but separating — nudge out of penetration, leave velocity.
    return { contact: null, pos: add(pos, scale(normal, radius - dist)), vel };
  }
  const impactSpeed = -dot(vel, normal);
  return {
    contact: { impactSpeed, material, point: closest },
    pos: add(pos, scale(normal, radius - dist)),
    vel: bounce(vel, normal, restitutionFor(material)),
  };
};

/** The stronger of two optional contacts (larger impact speed wins). */
const strongerContact = (a: Contact | null, b: Contact | null): Contact | null => {
  if (a === null) {
    return b;
  }
  if (b === null) {
    return a;
  }
  return b.impactSpeed > a.impactSpeed ? b : a;
};

/**
 * Advance one free (released) ball by a single fixed step: integrate under gravity
 * with air damping, then resolve every static collider once. Deterministic.
 */
export const stepFreeBall = (pos: Vec3, vel: Vec3, radius: number, colliders: Colliders, dt: number): StepResult => {
  // Semi-implicit Euler with exponential-ish air damping.
  const damped = scale(vel, Math.max(0, 1 - LINEAR_DAMPING * dt));
  const gravityStep = add(damped, vec3(0, GRAVITY * dt, 0));
  let curPos = add(pos, scale(gravityStep, dt));
  let curVel = gravityStep;
  let contact: Contact | null = null;

  for (const plane of colliders.planes) {
    const r = resolvePlane(curPos, curVel, radius, plane.point, plane.normal, plane.material);
    curPos = r.pos;
    curVel = r.vel;
    contact = strongerContact(contact, r.contact);
  }
  for (const box of colliders.boxes) {
    const r = resolveBox(curPos, curVel, radius, box.center, box.half, box.material);
    curPos = r.pos;
    curVel = r.vel;
    contact = strongerContact(contact, r.contact);
  }

  return { contact, pos: curPos, vel: curVel };
};

/** Convenience for tests / feedback: the ball's current speed. */
export const speedOf = (vel: Vec3): number => length(vel);

/** A unit direction toward a target (or `+Z` if coincident). */
export const dirTo = (from: Vec3, to: Vec3): Vec3 => {
  const d = sub(to, from);
  return length(d) < 1e-6 ? vec3(0, 0, 1) : normalize(d);
};
