import { strict as assert } from "node:assert";
import { test } from "node:test";

import { IDENTITY_QUAT, quatFromAxisAngle, quatFromEulerXyz, quatMul, rotateVec, vec3 } from "./vec3.ts";

const near = (a: number, b: number, eps = 1e-9): boolean => Math.abs(a - b) < eps;

test("quatMul with identity returns the other quaternion", () => {
  const q = quatFromEulerXyz(0.3, -0.7, 1.1);
  const r = quatMul(IDENTITY_QUAT, q);
  for (let i = 0; i < 4; i += 1) {
    assert.ok(near(r[i] as number, q[i] as number), `component ${i}`);
  }
});

test("rotateVec by 90° about +Y turns +X into -Z", () => {
  const q = quatFromAxisAngle(vec3(0, 1, 0), Math.PI / 2);
  const r = rotateVec(q, vec3(1, 0, 0));
  assert.ok(near(r.x, 0, 1e-9) && near(r.y, 0, 1e-9) && near(r.z, -1, 1e-9), JSON.stringify(r));
});

test("composing two rotations equals rotating twice", () => {
  const a = quatFromAxisAngle(vec3(0, 1, 0), 0.5);
  const b = quatFromAxisAngle(vec3(1, 0, 0), 0.3);
  const v = vec3(0.2, 0.9, -0.4);
  const combined = rotateVec(quatMul(a, b), v);
  const stepwise = rotateVec(a, rotateVec(b, v));
  assert.ok(near(combined.x, stepwise.x, 1e-9) && near(combined.y, stepwise.y, 1e-9) && near(combined.z, stepwise.z, 1e-9));
});

test("euler quaternion is unit length", () => {
  const q = quatFromEulerXyz(0.9, -0.2, 1.7);
  const len = Math.hypot(q[0], q[1], q[2], q[3]);
  assert.ok(near(len, 1, 1e-9));
});
