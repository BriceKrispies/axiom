import { strict as assert } from "node:assert";
import { test } from "node:test";

import { type Geometry, billboard, capsule, cone, plate, ringTorus, segmentedAppendage, taperedPrism, wedge } from "./meshgen.ts";
import { length } from "./vec3.ts";

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
});

test("geometry is deterministic (same params ⇒ identical mesh)", () => {
  assert.deepEqual(capsule(0.3, 0.8, 12, 4), capsule(0.3, 0.8, 12, 4));
  assert.deepEqual(ringTorus(0.4, 0.08, 20, 8), ringTorus(0.4, 0.08, 20, 8));
});

test("segment counts change the vertex count (LOD is real)", () => {
  assert.ok(capsule(0.3, 0.8, 8, 2).positions.length < capsule(0.3, 0.8, 16, 6).positions.length);
});
