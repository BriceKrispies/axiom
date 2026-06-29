/*
 * The spatial-query free functions (SPEC-03 §4.2 / §5): `overlapCircle` /
 * `overlapBox` / `raycast`. Each is a scene query over the *committed, propagated
 * world transforms for the current tick* (SPEC-03 §6), projected through the
 * installed `HostBridge` (`host-binding.ts`) onto the native `axiom-scene`
 * Entity-addressed query surface. The native core owns the bounds tests and the
 * nearest-hit tie-break (ascending node id); none of that query math is
 * re-implemented in TS — these only name the query and forward.
 *
 * They sit beside `math.ts` (the pure scalar `clamp`/`lerp`/`normalizeAngle`)
 * rather than inside it: the scalar helpers are stateless arithmetic, while these
 * read live scene state, so they are their own module — the same split the spec
 * draws between §5 "pure helpers" and §5 "scene queries".
 */

import type { Entity, RayHit, Result, Vec2, Vec3 } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";

/** Entities whose committed transform overlaps the circle, in stable order (SPEC-03 §4.2). */
export const overlapCircle = (center: Vec2, radius: number): readonly Entity[] =>
  boundHost().overlapCircle(center.x, center.y, radius);

/** Entities whose committed bounds overlap the query box, in stable order (SPEC-03 §4.2). */
export const overlapBox = (center: Vec3, halfExtents: Vec3): readonly Entity[] =>
  boundHost().overlapBox(center, halfExtents);

/** The nearest bounded ray hit, or the empty value on a miss (SPEC-03 §4.2). */
export const raycast = (origin: Vec3, direction: Vec3, maxDistance: number): Result<RayHit> =>
  boundHost().raycast(origin, direction, maxDistance);
