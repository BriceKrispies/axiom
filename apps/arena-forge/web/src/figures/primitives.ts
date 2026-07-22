/*
 * primitives.ts — resolves a grammar `PrimitiveType` to a cached engine mesh
 * handle. Built-ins (box/sphere/cylinder) use `createMesh`; the rest are generated
 * once via `meshgen.ts` and registered with `createMeshData`. Every primitive is
 * generated at a CANONICAL unit size (roughly fitting a ±0.5 box), so a part's
 * non-uniform `extents` are applied purely as the node's scale (`compose.ts`) and
 * many parts that differ only in size SHARE one mesh handle. The cache key is
 * therefore just `primitive:quality` — the segment count is the only geometry
 * variable, driven by the mobile quality tier. `resetMeshCache()` mirrors the
 * engine's own cache invalidation on `clearScene`.
 */

import { createMesh, createMeshData } from "@axiom/web-engine";
import type { Handle } from "@axiom/web-engine";
import type { PrimitiveType, QualityTier } from "./parts.ts";
import { billboard, capsule, cone, plate, ringTorus, roundedBox, segmentedAppendage, wedge } from "./meshgen.ts";

interface SegBudget {
  readonly radial: number;
  readonly cap: number;
  readonly ring: number;
  readonly tube: number;
}

const SEGMENTS: Readonly<Record<QualityTier, SegBudget>> = {
  low: { radial: 8, cap: 2, ring: 12, tube: 6 },
  med: { radial: 12, cap: 3, ring: 18, tube: 8 },
  high: { radial: 16, cap: 4, ring: 24, tube: 10 },
};

const cache = new Map<string, Handle>();

const generate = (primitive: PrimitiveType, s: SegBudget): Handle => {
  switch (primitive) {
    case "rounded_box":
      // A real chamfered box now (was a sharp-box fallback); softens every plate /
      // armor / torso edge tribe-wide. `roundedBox` also carries a per-vertex `ao`
      // array, which flows through to the engine's `MeshData.ao` here so crevices /
      // undersides darken.
      return createMeshData(roundedBox(1, 1, 1, 0.18));
    case "capsule":
      return createMeshData(capsule(0.26, 0.48, s.radial, s.cap));
    case "cone":
      return createMeshData(cone(0.5, 1, s.radial));
    case "wedge":
      return createMeshData(wedge(1, 1, 1));
    case "plate":
      return createMeshData(plate(1, 1, 1, 0.12));
    case "ring":
      return createMeshData(ringTorus(0.42, 0.09, s.ring, s.tube));
    case "segmented":
      return createMeshData(segmentedAppendage(0.16, 1, 5, 0.8, s.radial >> 1));
    case "billboard":
      return createMeshData(billboard(1, 1));
    default:
      // Unreachable for the generated set; the built-ins (box/sphere/cylinder) take
      // the fast path in `meshFor` and never call `generate`.
      return createMesh("box");
  }
};

/** The cached mesh handle for a primitive at a quality tier. */
export const meshFor = (primitive: PrimitiveType, quality: QualityTier): Handle => {
  const builtIn = primitive === "box" || primitive === "sphere" || primitive === "cylinder";
  const key = builtIn ? primitive : `${primitive}:${quality}`;
  const hit = cache.get(key);
  if (hit !== undefined) {
    return hit;
  }
  const handle = builtIn ? createMesh(primitive) : generate(primitive, SEGMENTS[quality]);
  cache.set(key, handle);
  return handle;
};

/** Invalidate the mesh cache (call alongside the engine's `clearScene`). */
export const resetMeshCache = (): void => cache.clear();
