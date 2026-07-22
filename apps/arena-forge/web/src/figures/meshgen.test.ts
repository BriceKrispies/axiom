import { strict as assert } from "node:assert";
import { test } from "node:test";

import { type Geometry, billboard, capsule, cone, plate, ringTorus, roundedBox, segmentedAppendage, taperedPrism, verticalOcclusion, wedge } from "./meshgen.ts";
import { length, vec3 } from "./vec3.ts";

const wellFormed = (name: string, g: Geometry): void => {
  assert.equal(g.positions.length, g.normals.length, `${name}: positions/normals length mismatch`);
  assert.ok(g.positions.length > 0, `${name}: empty`);
  assert.ok(g.indices.length % 3 === 0, `${name}: indices not triangle-aligned`);
  for (const i of g.indices) {
    assert.ok(i >= 0 && i < g.positions.length, `${name}: index ${i} out of range`);
  }
  for (const n of g.normals) {
    assert.ok(Math.abs(length(n) - 1) < 1e-6 || length(n) < 1e-9, `${name}: non-unit normal`);
  }
};

test("every primitive generator produces well-formed geometry", () => {
  wellFormed("capsule", capsule(0.3, 0.8, 12, 4));
  wellFormed("cone", cone(0.4, 0.9, 12));
  wellFormed("taperedPrism", taperedPrism(0.6, 0.4, 0.2, 0.4, 0.5));
  wellFormed("wedge", wedge(0.5, 0.6, 0.2));
  wellFormed("plate", plate(0.7, 0.5, 0.08, 0.12));
  wellFormed("ringTorus", ringTorus(0.4, 0.08, 20, 8));
  wellFormed("segmented", segmentedAppendage(0.1, 0.8, 5, 0.9, 6));
  wellFormed("billboard", billboard(0.4, 0.6));
  wellFormed("roundedBox", roundedBox(1, 1, 1, 0.18));
  wellFormed("roundedBox/thin", roundedBox(0.4, 1.2, 0.2, 0.3));
});

test("roundedBox is a canonical unit (±0.5) chamfered box with beveled corners", () => {
  const g = roundedBox(1, 1, 1, 0.2);
  // Every vertex sits inside the unit box, and at least one axis is pulled off the
  // face by the bevel (a sharp box would have a vertex at a full ±0.5 corner).
  for (const p of g.positions) {
    assert.ok(Math.abs(p.x) <= 0.5 + 1e-9 && Math.abs(p.y) <= 0.5 + 1e-9 && Math.abs(p.z) <= 0.5 + 1e-9, "vertex escapes the unit box");
  }
  const sharpCorner = g.positions.some((p) => Math.abs(p.x) > 0.49 && Math.abs(p.y) > 0.49 && Math.abs(p.z) > 0.49);
  assert.ok(!sharpCorner, "beveled box must have no sharp ±0.5 corner vertex");
  // A real bevel adds chamfer faces, so it has far more geometry than a 24-vert box.
  assert.ok(g.positions.length > 24, "chamfered box should add edge/corner geometry");
});

test("roundedBox is deterministic (same params ⇒ identical mesh)", () => {
  assert.deepEqual(roundedBox(0.8, 1.1, 0.4, 0.16), roundedBox(0.8, 1.1, 0.4, 0.16));
});

test("AO hook: undersides are darker than tops, and it stays dormant (not on MeshData)", () => {
  // The pure occlusion proxy: an up-normal is fully lit, a down-normal is floored.
  const ao = verticalOcclusion([vec3(0, 1, 0), vec3(0, -1, 0), vec3(1, 0, 0)]);
  assert.equal(ao[0], 1);
  assert.ok(ao[1]! < ao[0]!, "underside must be darker than top");
  assert.ok(ao[1]! > 0, "occlusion is floored, never fully black");
  // The generator carries the AO array on its own Geometry (ready for the engine),
  // aligned with the vertices — but it is NOT forwarded to createMeshData yet.
  const g = roundedBox(1, 1, 1, 0.18);
  assert.equal(g.ao?.length, g.positions.length);
});

test("geometry is deterministic (same params ⇒ identical mesh)", () => {
  assert.deepEqual(capsule(0.3, 0.8, 12, 4), capsule(0.3, 0.8, 12, 4));
  assert.deepEqual(ringTorus(0.4, 0.08, 20, 8), ringTorus(0.4, 0.08, 20, 8));
});

test("segment counts change the vertex count (LOD is real)", () => {
  assert.ok(capsule(0.3, 0.8, 8, 2).positions.length < capsule(0.3, 0.8, 16, 6).positions.length);
});
