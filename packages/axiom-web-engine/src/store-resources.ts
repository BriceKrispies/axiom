/*
 * store-resources.ts — the RESOURCE half of the retained-scene store: turning
 * authored specs into handles the backend can draw with.
 *
 * Split out of `store.ts` because the two halves answer different questions and
 * change for different reasons. This file is about REGISTRATION — geometry
 * uploaded once, materials resolved once, a primitive cache so two nodes of the
 * same kind and facet budget share one upload. Nothing here runs per frame.
 * `store.ts` keeps the per-frame half: the scene's nodes, lights and camera, and
 * the frame it hands the backend.
 *
 * Both halves share the one singleton, which lives in `store.ts` and is reached
 * here through `requireState` / `allocHandle`. The dependency is strictly
 * one-directional — this file imports from `store.ts` and `store.ts` imports
 * nothing back — so there is no cycle.
 *
 * Branchless, fully-covered spine: it builds no WebGL/Canvas context itself, so
 * every path is exercisable with a fake backend.
 */

import type { Handle, MaterialSpec, MeshData, MeshKind, Rgba } from "./api.ts";
import { allocHandle, requireState } from "./store.ts";
import { assert, orCompute, orElse, presentOf } from "./branchless.ts";
import { buildPrimitive, primitiveCacheKey, resolveFacets } from "./tessellation.ts";

const DEFAULT_EMISSIVE: Rgba = [0, 0, 0, 1];
const DEFAULT_OPACITY = 1;
// Absent roughness is fully MATTE: glossiness 0 -> zero specular, so a material
// that never sets roughness renders byte-identically to the old Lambert-only path.
const DEFAULT_ROUGHNESS = 1;

/** Register custom triangle-list geometry and return its handle. */
export const createMeshData = (data: MeshData): Handle => {
  const st = requireState();
  assert(
    data.positions.length === data.normals.length,
    `store: createMeshData positions (${data.positions.length}) and normals (${data.normals.length}) differ`,
  );
  // An `ao` array, when present, must carry one scalar per vertex; absent -> the
  // backends default it to 1.0, so this only fires on a genuine length mismatch.
  const aoLength = orElse(
    presentOf(data.ao).map((ao): number => ao.length)[0],
    data.positions.length,
  );
  assert(
    aoLength === data.positions.length,
    `store: createMeshData ao (${aoLength}) must match positions (${data.positions.length})`,
  );
  const handle = allocHandle();
  st.backend.uploadMesh(handle, data);
  return handle;
};

/**
 * Get (or lazily build + cache) the shared geometry for a primitive kind at a
 * radial facet budget.
 *
 * `segments` is the budget at FULL detail; `tessellation.ts` resolves it against
 * the active backend's `detailScale` (the software path halves it), and the
 * result keys the cache — so asking for the same budget twice reuses one upload,
 * and asking for a bigger one adds geometry beside the default rather than
 * replacing it. Omitting `segments` reproduces the previous fixed counts exactly
 * (24/12 cylinder, 16×24 / 8×12 sphere).
 */
export const createMesh = (kind: MeshKind, segments?: number): Handle => {
  const st = requireState();
  const facets = resolveFacets(segments, st.backend.detailScale);
  const cacheKey = primitiveCacheKey(kind, facets);
  return orCompute(st.primitiveCache.get(cacheKey), (): Handle => {
    const handle = createMeshData(buildPrimitive(kind, facets));
    st.primitiveCache.set(cacheKey, handle);
    return handle;
  });
};

/** Register a material (diffuse base + additive emissive + opacity + roughness). */
export const createMaterial = (spec: MaterialSpec): Handle => {
  const st = requireState();
  const [br, bg, bb, ba] = spec.baseColor;
  const [er, eg, eb] = orElse(spec.emissive, DEFAULT_EMISSIVE);
  const handle = allocHandle();
  st.materials.set(handle, {
    baseColor: [br, bg, bb, ba],
    emissive: [er, eg, eb],
    opacity: orElse(spec.opacity, DEFAULT_OPACITY),
    roughness: orElse(spec.roughness, DEFAULT_ROUGHNESS),
  });
  return handle;
};
