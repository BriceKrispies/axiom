/*
 * Tessellation: how a caller's requested facet BUDGET becomes the concrete
 * geometry a primitive is built at.
 *
 * This is policy, not storage. `store.ts` owns the primitive cache and the
 * handles; this module owns the arithmetic that decides what to put in it —
 * the default budget, the backend's detail scale, the floor below which a ring
 * stops being a ring, and the sphere's latitude:longitude proportion. It is
 * pure (no singleton, no backend, no handles), so every rule here is testable
 * on its own terms.
 *
 * The rule it exists to express: tessellation is a property of how big a
 * primitive is ON SCREEN, not of its kind. A lagoon spanning a third of the
 * frame and a rivet are both `cylinder`, and forcing them to share one facet
 * count means either a faceted lagoon or a needlessly expensive rivet.
 */

import type { MeshData, MeshKind } from "./api.ts";
import { unitBox, unitCylinderY, unitSphere } from "./meshes.ts";
import { orElse } from "./branchless.ts";

/**
 * The default RADIAL facet budget of a round primitive, at full detail — what a
 * caller gets when it does not say how big the thing is. A caller that knows its
 * primitive is large on screen passes its own budget instead; the geometry is
 * then cached per-budget, so the big one is smooth WITHOUT dragging every small
 * cylinder in the scene up with it.
 */
const DEFAULT_RADIAL_SEGMENTS = 24;

/** The unit sphere is authored at 16 latitude rings to 24 longitude rings, so
 * one radial budget scales both and the sphere keeps its authored proportion at
 * any size. Derived from the two authored counts rather than written as a bare
 * ratio, so the relationship stays legible if either is ever re-authored. */
const SPHERE_LAT_SEGMENTS = 16;
const SPHERE_LAT_RATIO = SPHERE_LAT_SEGMENTS / DEFAULT_RADIAL_SEGMENTS;

/** The GPU path draws a primitive at exactly the requested budget. */
export const FULL_DETAIL_SCALE = 1;

/** The software rasterizer pays per triangle, so it draws every primitive at
 * HALF the facet budget of the GPU path. Expressed as a scale rather than a
 * second table of constants: it is the same halving the old fixed low-detail
 * counts encoded (24→12 cylinder, 16/24→8/12 sphere), now applied to whatever
 * budget the caller asked for — so the backend LOD survives a caller's request
 * instead of being silently defeated by it. */
export const SOFTWARE_DETAIL_SCALE = 0.5;

/** No ring closes with fewer facets than a triangle. */
const MIN_SEGMENTS = 3;

/** Round to a whole number of facets, never below the triangle floor. */
const clampFacets = (facets: number): number => Math.max(MIN_SEGMENTS, Math.round(facets));

/**
 * Resolve a caller's requested budget (or none) against the active backend's
 * detail scale, yielding the facet count the geometry is actually built at.
 *
 * `detailScale` is the backend's own baseline (`FULL_DETAIL_SCALE` /
 * `SOFTWARE_DETAIL_SCALE`) already multiplied by the quality's `curveDetail`, so
 * the backend's LOD and the player's curve-detail setting compose into one
 * number instead of fighting over the budget.
 *
 * Omitting `segments` at a backend's baseline scale reproduces the engine's
 * historical fixed counts exactly (24 / 12 cylinder), so a call site that never
 * asked for a budget renders byte-identically to before per-mesh tessellation
 * existed.
 */
export const resolveFacets = (segments: number | undefined, detailScale: number): number =>
  clampFacets(clampFacets(orElse(segments, DEFAULT_RADIAL_SEGMENTS)) * detailScale);

/** Primitive builders, each taking the resolved radial facet count. `box` is
 * flat-faceted by definition and ignores it. */
const KIND_BUILDERS: Readonly<Record<MeshKind, (facets: number) => MeshData>> = {
  box: unitBox,
  cylinder: (facets): MeshData => unitCylinderY(facets),
  sphere: (facets): MeshData => unitSphere(clampFacets(facets * SPHERE_LAT_RATIO), facets),
};

/** Build the unit geometry for `kind` at a resolved facet count. */
export const buildPrimitive = (kind: MeshKind, facets: number): MeshData => KIND_BUILDERS[kind](facets);

/** The primitive cache key: kind AND facets, never kind alone — two nodes of the
 * same kind at different budgets are legitimately different geometry. */
export const primitiveCacheKey = (kind: MeshKind, facets: number): string => `${kind}:${facets}`;
