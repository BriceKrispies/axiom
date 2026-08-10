/*
 * target.ts — the ground target: its concentric rings, and the rule that turns a
 * landing distance into points. Pure and SDK-free, so every scoring boundary is
 * unit-testable, and the scene builds its geometry from the SAME radii the scorer
 * uses — a ring you can see is a ring you can land in, by construction.
 *
 * Scoring is on the horizontal distance from the centre at the moment of FIRST
 * ground contact. Not where the ball settles: a bullseye that bounces out is still a
 * bullseye, and rewarding the roll would make the score depend on restitution
 * tuning rather than on the throw.
 */

import {
  POINTS_BULLSEYE,
  POINTS_DEAD_CENTRE,
  POINTS_INNER,
  POINTS_MID,
  POINTS_OUTER,
  RING_BULLSEYE,
  RING_DEAD_CENTRE,
  RING_INNER,
  RING_MID,
  RING_OUTER,
} from "./constants.ts";

/** Which band of the target a landing fell in. `"off"` is a complete miss. */
export type Ring = "dead-centre" | "bullseye" | "inner" | "mid" | "outer" | "off";

/** One scoring band: everything at or inside `radius` that is not in a tighter band. */
export interface Band {
  readonly ring: Ring;
  readonly radius: number;
  readonly points: number;
  /** The shout the HUD floats when a landing hits this band. */
  readonly label: string;
}

/**
 * The bands, TIGHTEST FIRST — the order the scorer walks and the order the scene
 * draws them (largest painted first, so tighter rings sit on top).
 */
export const BANDS: readonly Band[] = [
  { label: "DEAD CENTRE", points: POINTS_DEAD_CENTRE, radius: RING_DEAD_CENTRE, ring: "dead-centre" },
  { label: "BULLSEYE", points: POINTS_BULLSEYE, radius: RING_BULLSEYE, ring: "bullseye" },
  { label: "INNER RING", points: POINTS_INNER, radius: RING_INNER, ring: "inner" },
  { label: "MID RING", points: POINTS_MID, radius: RING_MID, ring: "mid" },
  { label: "OUTER RING", points: POINTS_OUTER, radius: RING_OUTER, ring: "outer" },
];

/** The band a landing `distance` (m) from the centre falls in. */
export const bandFor = (distance: number): Band | null =>
  BANDS.find((band) => distance <= band.radius) ?? null;

/** The ring a landing `distance` (m) from the centre falls in. */
export const ringFor = (distance: number): Ring => bandFor(distance)?.ring ?? "off";

/** Base points for a landing `distance` (m) from the centre, before any multiplier. */
export const pointsFor = (distance: number): number => bandFor(distance)?.points ?? 0;

/** The HUD shout for a landing `distance` (m) from the centre. */
export const labelFor = (distance: number): string => bandFor(distance)?.label ?? "OFF TARGET";

/** Whether a landing scored at all (landed anywhere on the painted target). */
export const isOnTarget = (distance: number): boolean => distance <= RING_OUTER;

/** Whether a landing is good enough to be a headline moment (flash, shake, shout). */
export const isBigLanding = (distance: number): boolean => distance <= RING_BULLSEYE;
