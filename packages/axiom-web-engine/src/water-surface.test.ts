/*
 * water-surface.test.ts — the stylized water net's invariants: it produces a
 * sparse two-layer cellular pattern clipped to the disc, is fully deterministic,
 * honors its options (and their defaults), and adds an optional base fill.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { waterSurface } from "./water-surface.ts";

const EPS = 1e-9;
const OPAQUE_FALLBACK = 1;

test("the net is a two-layer honeycomb clipped around the disc", () => {
  const radius = 5;
  const cellSize = 1.6;
  const surface = waterSurface({ cellSize, radius });

  // A sparse net, not empty and not a dense grid.
  assert.ok(surface.instances.length > 6, "the net has lines");
  assert.ok(surface.instances.length < 500, "the net stays sparse");

  // Two feathering layers per edge: the halo and core materials both exist and
  // both appear among the instances, so every line reads as a softened stroke.
  assert.ok("waterCore" in surface.materials, "core material exists");
  assert.ok("waterHalo" in surface.materials, "halo material exists");
  assert.ok(surface.instances.some((i) => i.material === "waterCore"), "core strips are placed");
  assert.ok(surface.instances.some((i) => i.material === "waterHalo"), "halo strips are placed");
  // The halo is fainter than the core (the feather).
  const coreAlpha = surface.materials.waterCore.opacity ?? OPAQUE_FALLBACK;
  const haloAlpha = surface.materials.waterHalo.opacity ?? OPAQUE_FALLBACK;
  assert.ok(haloAlpha < coreAlpha, "the halo is fainter than the core");

  // Hexagon CENTERS are clipped to the disc, so an edge midpoint stays within a
  // hex size of the rim — the net never sprawls far past the water.
  for (const instance of surface.instances) {
    const { x, z } = instance.transform.position;
    assert.ok(Math.hypot(x, z) <= radius + cellSize + EPS, "the strip center hugs the disc");
    assert.equal(instance.mesh, "box", "strips are boxes");
  }
});

test("shared hexagon edges are deduplicated — no double-weight seams", () => {
  // Every interior edge is shared by two hexagons; the net must draw it once, so
  // no two core strips land on the same midpoint+angle.
  const surface = waterSurface({ cellSize: 1.5, radius: 6 });
  const cores = surface.instances.filter((i) => i.material === "waterCore");
  const signatures = cores.map((i) => {
    const { x, z } = i.transform.position;
    const [, ry] = i.transform.rotation;
    return `${Math.round(x * 100)},${Math.round(z * 100)},${Math.round(ry * 100)}`;
  });
  assert.equal(new Set(signatures).size, signatures.length, "no duplicate edge strips");
});

test("it is fully deterministic — no clock, no randomness", () => {
  assert.deepEqual(waterSurface({ radius: 4 }), waterSurface({ radius: 4 }));
  assert.deepEqual(waterSurface({ radius: 4, cellSize: 1.1, drift: 0.3 }), waterSurface({ radius: 4, cellSize: 1.1, drift: 0.3 }));
});

test("options override the defaults (color, spacing, prefix, center)", () => {
  const dense = waterSurface({ radius: 6, cellSize: 0.8 });
  const sparse = waterSurface({ radius: 6, cellSize: 2.4 });
  assert.ok(dense.instances.length > sparse.instances.length, "smaller cells make more lines");

  const custom = waterSurface({
    radius: 3,
    cellSize: 1.2,
    center: { x: 10, z: -4 },
    keyPrefix: "pool",
    lineColor: [1, 0, 0, 1],
    lineWidth: 0.2,
    opacity: 0.4,
    softness: 2,
    y: 0.5,
  });
  assert.ok("poolCore" in custom.materials && "poolHalo" in custom.materials, "the prefix renames the materials");
  assert.deepEqual(custom.materials.poolCore.baseColor, [1, 0, 0, 1], "the line color is applied");
  assert.equal(custom.materials.poolCore.opacity, 0.4, "the opacity is applied");
  // Centered off-origin: every strip follows the given center (within a cell).
  for (const instance of custom.instances) {
    assert.ok(Math.hypot(instance.transform.position.x - 10, instance.transform.position.z + 4) <= 3 + 1.2 + EPS, "strips follow the center");
    assert.equal(instance.transform.position.y, 0.5, "strips sit at the given height");
  }
});

test("a baseColor adds one base disc under the net; omitting it adds none", () => {
  const withBase = waterSurface({ radius: 5, baseColor: [0.1, 0.4, 0.5, 1] });
  const base = withBase.instances.filter((i) => i.material === "waterBase");
  assert.equal(base.length, 1, "exactly one base disc");
  assert.ok(base.every((disc) => disc.mesh === "cylinder"), "the base is a disc");
  assert.ok("waterBase" in withBase.materials, "the base material exists");
  assert.deepEqual(withBase.materials.waterBase.baseColor, [0.1, 0.4, 0.5, 1]);

  const withoutBase = waterSurface({ radius: 5 });
  assert.ok(withoutBase.instances.every((i) => i.material !== "waterBase"), "no base disc without a baseColor");
  assert.ok(!("waterBase" in withoutBase.materials), "no base material without a baseColor");
});

test("a degenerate tiny radius still yields a valid (possibly empty) net", () => {
  // radius < cellSize: the only in-disc offset is 0, so there is at least the
  // centerline of each family — never a crash, never NaN geometry.
  const tiny = waterSurface({ radius: 0.5, cellSize: 1.4 });
  for (const instance of tiny.instances) {
    assert.ok(Number.isFinite(instance.transform.scale.x), "strip length is finite");
    assert.ok(instance.transform.scale.x > 0, "strip has positive length");
  }
});
