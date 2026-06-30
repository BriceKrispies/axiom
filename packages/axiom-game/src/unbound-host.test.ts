import assert from "node:assert/strict";
import { test } from "node:test";

import { UNBOUND_HOST_BASE } from "./unbound-host.ts";

// The inert non-2D host base used before `bindNative`: every read returns a
// neutral value and every signal is a no-op. These DIRECT tests call each method on
// the base object so every projection (and the `absent` Result helper raycast uses)
// is exercised — the base is the total fallback that keeps the free surface silent,
// not crashing, before the app binds a host.

const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const zero = { x: 0, y: 0, z: 0 };

const zero2 = { x: 0, y: 0 };

test("the numeric math reads pass through or return the neutral scalar", () => {
  assert.equal(UNBOUND_HOST_BASE.clamp(5), 5); // identity passthrough
  assert.equal(UNBOUND_HOST_BASE.lerp(5), 5); // identity passthrough (inert start)
  assert.equal(UNBOUND_HOST_BASE.normalizeAngle(7), 7); // identity passthrough
  assert.equal(UNBOUND_HOST_BASE.v2Dot(), 0);
  assert.equal(UNBOUND_HOST_BASE.v2Len(), 0);
  assert.equal(UNBOUND_HOST_BASE.v2Dist(), 0);
  assert.equal(UNBOUND_HOST_BASE.v3Dot(), 0);
  assert.equal(UNBOUND_HOST_BASE.v3Len(), 0);
  assert.equal(UNBOUND_HOST_BASE.v3Dist(), 0);
});

test("the vector reads return the zero vector", () => {
  assert.deepEqual(UNBOUND_HOST_BASE.v2Add(), zero2);
  assert.deepEqual(UNBOUND_HOST_BASE.v2Sub(), zero2);
  assert.deepEqual(UNBOUND_HOST_BASE.v2Scale(), zero2);
  assert.deepEqual(UNBOUND_HOST_BASE.v2Normalize(), zero2);
  assert.deepEqual(UNBOUND_HOST_BASE.v2Lerp(), zero2);
  assert.deepEqual(UNBOUND_HOST_BASE.v3Add(), zero);
  assert.deepEqual(UNBOUND_HOST_BASE.v3Sub(), zero);
  assert.deepEqual(UNBOUND_HOST_BASE.v3Scale(), zero);
  assert.deepEqual(UNBOUND_HOST_BASE.v3Cross(), zero);
  assert.deepEqual(UNBOUND_HOST_BASE.v3Normalize(), zero);
  assert.deepEqual(UNBOUND_HOST_BASE.v3Lerp(), zero);
});

test("the pure predicates return false until a host is bound", () => {
  assert.equal(UNBOUND_HOST_BASE.aabbOverlap(), false);
  assert.equal(UNBOUND_HOST_BASE.pointInRect(), false);
  assert.equal(UNBOUND_HOST_BASE.circleOverlap(), false);
});

test("the matrix reads return the identity matrix", () => {
  assert.deepEqual(UNBOUND_HOST_BASE.mat4Identity(), identity);
  assert.deepEqual(UNBOUND_HOST_BASE.mat4Multiply(), identity);
  assert.deepEqual(UNBOUND_HOST_BASE.mat4Perspective(), identity);
  assert.deepEqual(UNBOUND_HOST_BASE.mat4LookAt(), identity);
  assert.deepEqual(UNBOUND_HOST_BASE.mat4Invert(), identity);
  assert.deepEqual(UNBOUND_HOST_BASE.mat4FromTRS(), identity);
  assert.deepEqual(UNBOUND_HOST_BASE.quatToMat4(), identity);
});

test("the quaternion reads return the identity quaternion", () => {
  assert.deepEqual(UNBOUND_HOST_BASE.quatIdentity(), [0, 0, 0, 1]);
  assert.deepEqual(UNBOUND_HOST_BASE.quatFromEuler(), [0, 0, 0, 1]);
  assert.deepEqual(UNBOUND_HOST_BASE.quatMultiply(), [0, 0, 0, 1]);
  assert.deepEqual(UNBOUND_HOST_BASE.quatNormalize(), [0, 0, 0, 1]);
});

test("the scene-query reads return empty collections / the origin cell / a null handle", () => {
  assert.deepEqual(UNBOUND_HOST_BASE.overlapCircle(), []);
  assert.deepEqual(UNBOUND_HOST_BASE.overlapBox(), []);
  assert.equal(UNBOUND_HOST_BASE.raycast(), undefined); // covers the `absent` Result helper
  assert.deepEqual(UNBOUND_HOST_BASE.gridDistanceField(), []);
  assert.deepEqual(UNBOUND_HOST_BASE.gridPath(), []);
  assert.equal(UNBOUND_HOST_BASE.gridReachable(), false);
  assert.deepEqual(UNBOUND_HOST_BASE.gridStepToward(), { x: 0, y: 0 });
});

test("the handle-minting authoring reads return a null handle", () => {
  assert.equal(UNBOUND_HOST_BASE.createMesh(), 0);
  assert.equal(UNBOUND_HOST_BASE.createMaterial(), 0);
  assert.equal(UNBOUND_HOST_BASE.addLight(), 0);
  assert.equal(UNBOUND_HOST_BASE.spawnRenderable(), 0);
  assert.equal(UNBOUND_HOST_BASE.createController(), 0);
  assert.equal(UNBOUND_HOST_BASE.loadSound(), 0);
  assert.equal(UNBOUND_HOST_BASE.loadTexture(), 0);
  assert.equal(UNBOUND_HOST_BASE.playSound(), 0);
  assert.equal(UNBOUND_HOST_BASE.playMusic(), 0);
  assert.equal(UNBOUND_HOST_BASE.playTone(), 0);
  assert.equal(UNBOUND_HOST_BASE.scheduleSound(), 0);
});

test("loadFont returns the built-in monospace font until a host is bound", () => {
  assert.deepEqual(UNBOUND_HOST_BASE.loadFont(), { family: "monospace", size: 16 });
});

test("getSessionConfig returns the neutral seed-zero config", () => {
  assert.deepEqual(UNBOUND_HOST_BASE.getSessionConfig(), { params: {}, seed: 0n });
});

test("every signal is a silent no-op until a host is bound", () => {
  assert.doesNotThrow(() => {
    UNBOUND_HOST_BASE.bindAction();
    UNBOUND_HOST_BASE.notifyReady();
    UNBOUND_HOST_BASE.reportOutcome();
    UNBOUND_HOST_BASE.reportOutcomes();
    UNBOUND_HOST_BASE.setCamera3D();
    UNBOUND_HOST_BASE.setNodeTransform();
    UNBOUND_HOST_BASE.setNodeBounds();
    UNBOUND_HOST_BASE.clearScene();
    UNBOUND_HOST_BASE.controlFirstPerson();
    UNBOUND_HOST_BASE.stopVoice();
    UNBOUND_HOST_BASE.setMasterVolume();
    UNBOUND_HOST_BASE.setMuted();
  });
});
