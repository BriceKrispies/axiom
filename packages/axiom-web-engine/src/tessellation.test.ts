/*
 * tessellation.test.ts — `node --test` coverage for the facet-budget policy in
 * `tessellation.ts`: the default budget, the backend detail scale, the triangle
 * floor, the sphere's authored proportion, and the cache key. Pure arithmetic —
 * no store, no backend, no DOM.
 *
 * The load-bearing assertion here is BYTE-IDENTITY: omitting a budget must
 * reproduce the fixed counts the engine used before per-mesh tessellation
 * existed (24/12 cylinder, 16x24 / 8x12 sphere), or the change silently
 * re-tessellates every existing app.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { FULL_DETAIL_SCALE, SOFTWARE_DETAIL_SCALE, buildPrimitive, primitiveCacheKey, resolveFacets } from "./tessellation.ts";

test("an omitted budget reproduces the engine's historical fixed counts", () => {
  assert.equal(resolveFacets(undefined, FULL_DETAIL_SCALE), 24, "GPU path: the old 24-segment default");
  assert.equal(resolveFacets(undefined, SOFTWARE_DETAIL_SCALE), 12, "software path: the old halved 12");
});

test("a requested budget survives the backend's detail scale", () => {
  assert.equal(resolveFacets(96, FULL_DETAIL_SCALE), 96, "full detail honours the request exactly");
  assert.equal(resolveFacets(96, SOFTWARE_DETAIL_SCALE), 48, "software halves the REQUEST, not a fixed table");
  assert.ok(resolveFacets(96, SOFTWARE_DETAIL_SCALE) > resolveFacets(undefined, FULL_DETAIL_SCALE), "a large request outranks the default even when halved");
});

test("a quality's curve detail composes with the backend's baseline", () => {
  const curveDetail = 2;
  assert.equal(
    resolveFacets(undefined, SOFTWARE_DETAIL_SCALE * curveDetail),
    24,
    "doubling curve detail on the software path reaches the GPU path's default",
  );
  const identityDetail = 1;
  assert.equal(
    resolveFacets(undefined, SOFTWARE_DETAIL_SCALE * identityDetail),
    resolveFacets(undefined, SOFTWARE_DETAIL_SCALE),
    "curve detail 1 is the identity — the default re-tessellates nothing",
  );
  assert.ok(
    resolveFacets(undefined, SOFTWARE_DETAIL_SCALE * 0.25) < resolveFacets(undefined, SOFTWARE_DETAIL_SCALE),
    "the low rung genuinely sheds facets",
  );
});

test("no ring closes with fewer facets than a triangle", () => {
  assert.equal(resolveFacets(0, FULL_DETAIL_SCALE), 3, "a zero budget floors at a triangle");
  assert.equal(resolveFacets(-40, FULL_DETAIL_SCALE), 3, "a negative budget floors at a triangle");
  assert.equal(resolveFacets(4, SOFTWARE_DETAIL_SCALE), 3, "halving a small budget still floors at a triangle");
});

test("a fractional budget resolves to a whole number of facets", () => {
  assert.equal(resolveFacets(12.4, FULL_DETAIL_SCALE), 12);
  assert.equal(resolveFacets(12.6, FULL_DETAIL_SCALE), 13);
  assert.equal(resolveFacets(15, SOFTWARE_DETAIL_SCALE), 8, "15 * 0.5 = 7.5 rounds to 8");
});

test("the sphere keeps its authored 16:24 latitude proportion at any budget", () => {
  // 24 longitude rings x 16 latitude rings is what the unit sphere is authored
  // at; the ratio must hold when the budget moves, or spheres distort with size.
  const authored = buildPrimitive("sphere", 24);
  const doubled = buildPrimitive("sphere", 48);
  assert.ok(doubled.positions.length > authored.positions.length, "a bigger budget builds more geometry");
  // (lat+1) * (lon+1) vertices: 17*25 = 425 at the authored budget.
  assert.equal(authored.positions.length, 425, "16 lat x 24 lon, exactly as before");
  assert.equal(doubled.positions.length, 33 * 49, "32 lat x 48 lon — the proportion scaled, not drifted");
});

test("buildPrimitive dispatches each kind, and box ignores the facet count", () => {
  const coarse = buildPrimitive("box", 3);
  const fine = buildPrimitive("box", 96);
  assert.deepEqual(coarse.positions.length, fine.positions.length, "a box is flat-faceted by definition");
  assert.ok(buildPrimitive("cylinder", 96).positions.length > buildPrimitive("cylinder", 12).positions.length);
});

test("the cache key is kind AND facets, never kind alone", () => {
  assert.equal(primitiveCacheKey("cylinder", 96), "cylinder:96");
  assert.notEqual(
    primitiveCacheKey("cylinder", 96),
    primitiveCacheKey("cylinder", 12),
    "same kind at different budgets must not collide — that collision was the original bug",
  );
  assert.notEqual(primitiveCacheKey("sphere", 24), primitiveCacheKey("cylinder", 24));
});
