import assert from "node:assert/strict";
import { test } from "node:test";

import { IDENTITY_MAT4, IDENTITY_QUAT, ORIGIN_CELL, ZERO_VEC3 } from "./host-descriptors.ts";

// host-descriptors.ts is the neutral parameter records plus the inert default
// values an unbound read returns. The interfaces are erased at runtime; the four
// exported constants are the whole runtime surface — assert their exact shapes.

test("ZERO_VEC3 is the origin vector an inert v3 read returns", () => {
  assert.deepEqual(ZERO_VEC3, { x: 0, y: 0, z: 0 });
});

test("IDENTITY_MAT4 is the 4x4 identity an inert mat4 read returns", () => {
  assert.deepEqual(IDENTITY_MAT4, [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
});

test("IDENTITY_QUAT is the identity quaternion an inert quat read returns", () => {
  assert.deepEqual(IDENTITY_QUAT, [0, 0, 0, 1]);
});

test("ORIGIN_CELL is the origin cell an inert grid read returns", () => {
  assert.deepEqual(ORIGIN_CELL, { x: 0, y: 0 });
});
