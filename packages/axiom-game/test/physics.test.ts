import assert from "node:assert/strict";
import { test } from "node:test";

import { makeAdd } from "../src/game-object.ts";
import { makePhysics } from "../src/physics.ts";
import { FakeBridge } from "./fake-bridge.ts";

test("setConfig forwards gravity and damping to the native world", () => {
  const fake = new FakeBridge();
  makePhysics(fake).setConfig({
    angularDamping: 0.1,
    gravity: { x: 0, y: -9.8, z: 0 },
    linearDamping: 0.2,
  });
  assert.deepEqual(fake.config, [0, -9.8, 0, 0.2, 0.1]);
});

test("physics.add.{dynamic,static,kinematic} attach a body to the object's entity", () => {
  const fake = new FakeBridge();
  const add = makeAdd(fake);
  const physics = makePhysics(fake);
  const dynamicBody = physics.add.dynamic(add.sprite("a", 0, 0));
  const staticBody = physics.add.static(add.sprite("b", 0, 0));
  const kinematicBody = physics.add.kinematic(add.sprite("c", 0, 0));
  assert.deepEqual(
    fake.bodies.map(([, kind]) => kind),
    ["dynamic", "static", "kinematic"],
  );
  // Each body carries a distinct native handle.
  assert.equal(dynamicBody.handle, 1);
  assert.equal(staticBody.handle, 2);
  assert.equal(kinematicBody.handle, 3);
});

test("a body forwards impulse / force / torque and velocity setters", () => {
  const fake = new FakeBridge();
  const object = makeAdd(fake).sprite("a", 0, 0);
  const body = makePhysics(fake).add.dynamic(object);
  body.applyImpulse({ x: 1, y: 2, z: 3 });
  body.applyForce({ x: 4, y: 5, z: 6 });
  body.applyTorque({ x: 7, y: 8, z: 9 });
  body.setVelocity({ x: 10, y: 11, z: 12 });
  body.setAngularVelocity({ x: 13, y: 14, z: 15 });
  assert.deepEqual(fake.impulses, [[body.handle, 1, 2, 3]]);
  assert.deepEqual(fake.forces, [[body.handle, 4, 5, 6]]);
  assert.deepEqual(fake.torques, [[body.handle, 7, 8, 9]]);
  assert.deepEqual(fake.velocities, [[body.handle, 10, 11, 12]]);
  assert.deepEqual(fake.angularVelocities, [[body.handle, 13, 14, 15]]);
});
