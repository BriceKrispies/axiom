import assert from "node:assert/strict";
import { test } from "node:test";

import { makeAdd } from "./game-object.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

test("add.sprite spawns a Transform + Sprite entity at the given position", () => {
  const fake = new FakeBridge();
  const sprite = makeAdd(fake).sprite("hero", 5, 7);
  assert.equal(sprite.x, 5);
  assert.equal(sprite.y, 7);
  assert.equal(sprite.rotation, 0);
  assert.equal(sprite.scaleX, 1);
  assert.equal(sprite.scaleY, 1);
  assert.equal(sprite.vx, 0);
  assert.equal(sprite.vy, 0);
  assert.deepEqual(
    (fake.lastSpawn ?? []).map((component) => component.kind),
    ["Transform", "Sprite"],
  );
  // The handle wraps a real, queryable entity.
  assert.deepEqual(fake.worldQuery(["Sprite"]), [sprite.entity]);
});

test("add.text / add.rectangle / add.image spawn their render component at position", () => {
  const fake = new FakeBridge();
  const add = makeAdd(fake);
  const text = add.text("hi", 0, 0);
  const rect = add.rectangle(1, 2, { color: 255, height: 4, width: 3 });
  const image = add.image("bg", 9, 9);
  assert.deepEqual(fake.worldQuery(["Text"]), [text.entity]);
  assert.deepEqual(fake.worldQuery(["Rectangle"]), [rect.entity]);
  assert.deepEqual(fake.worldQuery(["Image"]), [image.entity]);
  assert.equal(rect.x, 1);
  assert.equal(rect.y, 2);
  assert.equal(image.x, 9);
  assert.equal(image.y, 9);
});

test("setPosition / setRotation / setScale write the Transform and chain", () => {
  const fake = new FakeBridge();
  const object = makeAdd(fake).sprite("hero", 0, 0);
  const returned = object.setPosition(3, 4).setRotation(1.5).setScale(2, 8);
  assert.equal(returned, object);
  assert.equal(object.x, 3);
  assert.equal(object.y, 4);
  assert.equal(object.rotation, 1.5);
  assert.equal(object.scaleX, 2);
  assert.equal(object.scaleY, 8);
  // The latest Transform is committed to the native store with every field.
  const transform = fake.worldGet(object.entity, "Transform");
  assert.deepEqual(transform, {
    kind: "Transform",
    rotation: 1.5,
    scaleX: 2,
    scaleY: 8,
    x: 3,
    y: 4,
  });
});

test("setVelocity writes a Velocity component and records its values", () => {
  const fake = new FakeBridge();
  const object = makeAdd(fake).sprite("hero", 0, 0);
  const returned = object.setVelocity(-2, 6);
  assert.equal(returned, object);
  assert.equal(object.vx, -2);
  assert.equal(object.vy, 6);
  assert.deepEqual(fake.worldGet(object.entity, "Velocity"), {
    kind: "Velocity",
    x: -2,
    y: 6,
  });
});

test("destroy despawns the wrapped entity", () => {
  const fake = new FakeBridge();
  const object = makeAdd(fake).sprite("hero", 0, 0);
  object.destroy();
  assert.equal(fake.worldGet(object.entity, "Transform"), undefined);
  assert.deepEqual(fake.worldQuery(["Sprite"]), []);
});
