import assert from "node:assert/strict";
import { test } from "node:test";

import { mat4, quat, v3 } from "../src/math3d.ts";
import { bindNative } from "../src/host-binding.ts";
import { FakeHost } from "./fake-host.ts";

// Every v3/mat4/quat op routes to the native MathApi (the fake bridge here). These
// assert the projection FORWARDS a vector of sample inputs to the bridge and returns
// the bridge's result verbatim — there is no TS-side math twin to drift from.

test("v3 forwards every vector op to the native MathApi", () => {
  bindNative(new FakeHost());
  const lhs = { x: 1, y: 2, z: 3 };
  const rhs = { x: 4, y: 5, z: 6 };
  assert.deepEqual(v3.add(lhs, rhs), { x: 5, y: 7, z: 9 });
  assert.deepEqual(v3.sub(rhs, lhs), { x: 3, y: 3, z: 3 });
  assert.deepEqual(v3.scale(lhs, 2), { x: 2, y: 4, z: 6 });
  assert.equal(v3.dot(lhs, rhs), 32);
  assert.deepEqual(v3.cross(lhs, rhs), { x: -3, y: 6, z: -3 });
  assert.equal(v3.len({ x: 3, y: 4, z: 0 }), 5);
  assert.deepEqual(v3.normalize({ x: 0, y: 0, z: 2 }), { x: 0, y: 0, z: 1 });
  assert.equal(v3.dist({ x: 0, y: 0, z: 0 }, { x: 3, y: 4, z: 0 }), 5);
  assert.deepEqual(v3.lerp(lhs, rhs, 0.5), { x: 2.5, y: 3.5, z: 4.5 });
});

test("mat4 forwards every matrix op to the native MathApi", () => {
  bindNative(new FakeHost());
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  assert.deepEqual(mat4.identity(), identity);
  // The fake's multiply is an elementwise sum — a deterministic function of BOTH args.
  assert.deepEqual(
    mat4.multiply(identity, identity),
    [2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2],
  );
  assert.deepEqual(
    mat4.perspective({ aspect: 2, far: 4, fovY: 1, near: 3 }),
    [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  );
  assert.deepEqual(
    mat4.lookAt({ x: 1, y: 2, z: 3 }, { x: 4, y: 5, z: 6 }, { x: 0, y: 1, z: 0 }),
    [1, 2, 3, 4, 5, 6, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
  );
  assert.deepEqual(mat4.invert(identity), [-1, -0, -0, -0, -0, -1, -0, -0, -0, -0, -1, -0, -0, -0, -0, -1]);
  assert.deepEqual(
    mat4.fromTRS({ x: 1, y: 2, z: 3 }, [0, 0, 0, 1], { x: 4, y: 5, z: 6 }),
    [1, 2, 3, 0, 0, 0, 1, 4, 5, 6, 0, 0, 0, 0, 0, 0],
  );
});

test("quat forwards every quaternion op to the native MathApi", () => {
  bindNative(new FakeHost());
  assert.deepEqual(quat.identity(), [0, 0, 0, 1]);
  assert.deepEqual(quat.fromEuler(1, 2, 3), [1, 2, 3, 0]);
  assert.deepEqual(quat.multiply([1, 2, 3, 4], [5, 6, 7, 8]), [5, 12, 21, 32]);
  assert.deepEqual(quat.normalize([1, 2, 3, 4]), [1, 2, 3, 4]);
  assert.deepEqual(quat.toMat4([1, 2, 3, 4]), [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
});
