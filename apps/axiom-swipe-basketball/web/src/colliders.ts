/*
 * colliders.ts — the machine's static collision geometry, derived once from
 * `constants.ts`. Pure data, SDK-free: the deterministic ball physics
 * (`physics.ts`) resolves a sphere against these, and `scene.ts` draws matching
 * renderables so what you see is what you hit.
 *
 * Two collider shapes cover the whole cabinet:
 *   - **AABB boxes** — backboard, two side rails, the front lip, and the ring of
 *     small boxes that stand in for the rim.
 *   - **Planes** — the sloped return ramp (an inclined half-space that also serves
 *     as the floor of the play shaft).
 *
 * Each carries a `material` tag so a contact can drive hit feedback (a rim/backboard
 * flash) without the physics knowing anything about presentation.
 */

import { type Vec3, normalize, vec3 } from "./vec.ts";
import {
  BACKBOARD_HALF_D,
  BACKBOARD_HALF_H,
  BACKBOARD_HALF_W,
  BACKBOARD_Y,
  BACKBOARD_Z,
  CABINET_HALF_WIDTH,
  CABINET_FAR_Z,
  CABINET_NEAR_Z,
  FRONT_LIP_Y,
  HOOP_X,
  HOOP_Y,
  HOOP_Z,
  RAMP_FAR_Y,
  RAMP_FAR_Z,
  RAMP_NEAR_Y,
  RAMP_NEAR_Z,
  RIM_RADIUS,
  RIM_SEGMENTS,
  RIM_TUBE,
  TRIGGER_CENTER_Y,
  TRIGGER_HALF_D,
  TRIGGER_HALF_H,
  TRIGGER_HALF_W,
} from "./constants.ts";

/** A surface material tag, used only for contact feedback (juice), not physics. */
export type ContactMaterial = "rim" | "backboard" | "rail" | "ramp" | "lip";

/** An axis-aligned box collider. */
export interface BoxCollider {
  readonly center: Vec3;
  readonly half: Vec3;
  readonly material: ContactMaterial;
}

/** An infinite plane half-space collider (the ball lives on the `+normal` side). */
export interface PlaneCollider {
  readonly point: Vec3;
  readonly normal: Vec3;
  readonly material: ContactMaterial;
}

/** The full static collision set of the machine. */
export interface Colliders {
  readonly boxes: readonly BoxCollider[];
  readonly planes: readonly PlaneCollider[];
}

/** The rim ring's small collider boxes, evenly spaced around the (shifted) hoop opening. */
export const rimBoxes = (offsetX = 0): BoxCollider[] => {
  const boxes: BoxCollider[] = [];
  const ringRadius = RIM_RADIUS + RIM_TUBE;
  for (let i = 0; i < RIM_SEGMENTS; i += 1) {
    const angle = (2 * Math.PI * i) / RIM_SEGMENTS;
    boxes.push({
      center: vec3(HOOP_X + offsetX + Math.cos(angle) * ringRadius, HOOP_Y, HOOP_Z + Math.sin(angle) * ringRadius),
      half: vec3(RIM_TUBE, RIM_TUBE, RIM_TUBE),
      material: "rim",
    });
  }
  return boxes;
};

/** The backboard box, shifted laterally with the moving hoop target. */
export const backboardBox = (offsetX = 0): BoxCollider => ({
  center: vec3(HOOP_X + offsetX, BACKBOARD_Y, BACKBOARD_Z),
  half: vec3(BACKBOARD_HALF_W, BACKBOARD_HALF_H, BACKBOARD_HALF_D),
  material: "backboard",
});

/** The sloped return-ramp plane (also the floor of the shaft), derived from its endpoints. */
export const rampPlane = (): PlaneCollider => {
  const dz = RAMP_NEAR_Z - RAMP_FAR_Z;
  const dy = RAMP_NEAR_Y - RAMP_FAR_Y;
  // Up-normal perpendicular to the (z,y) slope tangent, leaning toward the near end.
  const normal = normalize(vec3(0, dz, -dy));
  return { material: "ramp", normal, point: vec3(0, RAMP_NEAR_Y, RAMP_NEAR_Z) };
};

/** The always-fixed cabinet colliders (side rails + front lip). */
const staticBoxes = (): BoxCollider[] => {
  const railHalfW = 0.05;
  const railHalfH = 1.3;
  const railMidY = railHalfH;
  const railMidZ = (CABINET_NEAR_Z + CABINET_FAR_Z) / 2;
  const railHalfD = (CABINET_NEAR_Z - CABINET_FAR_Z) / 2;
  return [
    { center: vec3(-(CABINET_HALF_WIDTH + railHalfW), railMidY, railMidZ), half: vec3(railHalfW, railHalfH, railHalfD), material: "rail" },
    { center: vec3(CABINET_HALF_WIDTH + railHalfW, railMidY, railMidZ), half: vec3(railHalfW, railHalfH, railHalfD), material: "rail" },
    { center: vec3(0, FRONT_LIP_Y / 2, CABINET_NEAR_Z), half: vec3(CABINET_HALF_WIDTH, FRONT_LIP_Y / 2, 0.05), material: "lip" },
  ];
};

/**
 * Build every collider in the cabinet for the current hoop offset. The rim ring and
 * backboard follow the moving target (`offsetX`); the rails, lip, and ramp are
 * fixed. Rebuilt only when the hoop shifts (every few makes), not per frame.
 */
export const buildColliders = (offsetX = 0): Colliders => ({
  boxes: [backboardBox(offsetX), ...staticBoxes(), ...rimBoxes(offsetX)],
  planes: [rampPlane()],
});

/** The scoring trigger volume, a box just below the rim opening. */
export const triggerBox = (): BoxCollider => ({
  center: vec3(HOOP_X, TRIGGER_CENTER_Y, HOOP_Z),
  half: vec3(TRIGGER_HALF_W, TRIGGER_HALF_H, TRIGGER_HALF_D),
  material: "rim",
});
