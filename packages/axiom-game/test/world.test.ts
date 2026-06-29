import assert from "node:assert/strict";
import { test } from "node:test";

import { makeWorld } from "../src/world.ts";
import type { Component } from "../src/vocabulary.ts";
import { FakeBridge } from "./fake-bridge.ts";

interface Health extends Component {
  readonly kind: "health";
  readonly hp: number;
}

const health = (hp: number): Health => ({ hp, kind: "health" });

test("spawn returns a fresh handle and forwards the components", () => {
  const fake = new FakeBridge();
  const world = makeWorld(fake);
  const entity = world.spawn(health(10), { kind: "tag" });
  assert.equal(entity, 1);
  assert.deepEqual(fake.lastSpawn, [health(10), { kind: "tag" }]);
});

test("get reads a set component and returns the empty value on a miss", () => {
  const world = makeWorld(new FakeBridge());
  const entity = world.spawn(health(7));
  assert.deepEqual(world.get(entity, "health"), health(7));
  assert.equal(world.get(entity, "absent"), undefined);
});

test("set adds or replaces a component on a live entity", () => {
  const world = makeWorld(new FakeBridge());
  const entity = world.spawn(health(1));
  world.set(entity, health(42));
  assert.deepEqual(world.get(entity, "health"), health(42));
});

test("a read or set on a dead entity is a clean miss / no-op", () => {
  const world = makeWorld(new FakeBridge());
  const entity = world.spawn(health(5));
  world.despawn(entity);
  assert.equal(world.get(entity, "health"), undefined);
  world.set(entity, health(9));
  assert.equal(world.get(entity, "health"), undefined);
});

test("query returns the entities holding every kind, in stable order", () => {
  const world = makeWorld(new FakeBridge());
  const first = world.spawn(health(1), { kind: "enemy" });
  world.spawn(health(2));
  const third = world.spawn(health(3), { kind: "enemy" });
  assert.deepEqual(world.query("health", "enemy"), [first, third]);
});

test("childrenOf returns the direct children in stable order", () => {
  const fake = new FakeBridge();
  const world = makeWorld(fake);
  const parent = world.spawn({ kind: "node" });
  const childA = world.spawn({ kind: "node" });
  const childB = world.spawn({ kind: "node" });
  fake.link(childA, parent);
  fake.link(childB, parent);
  assert.deepEqual(world.childrenOf(parent), [childA, childB]);
});

test("despawnSubtree removes the entity and its whole subtree", () => {
  const fake = new FakeBridge();
  const world = makeWorld(fake);
  const root = world.spawn({ kind: "node" });
  const child = world.spawn({ kind: "node" });
  const grandchild = world.spawn({ kind: "node" });
  const sibling = world.spawn({ kind: "node" });
  fake.link(child, root);
  fake.link(grandchild, child);
  world.despawnSubtree(root);
  assert.deepEqual(world.query("node"), [sibling]);
});

test("alive is true for a live entity and false for a despawned / stale handle", () => {
  const world = makeWorld(new FakeBridge());
  const entity = world.spawn(health(1));
  assert.equal(world.alive(entity), true);
  assert.equal(world.alive(9999), false);
  world.despawn(entity);
  assert.equal(world.alive(entity), false);
});

test("has reports component presence and remove clears it", () => {
  const world = makeWorld(new FakeBridge());
  const entity = world.spawn(health(3), { kind: "enemy" });
  assert.equal(world.has(entity, "health"), true);
  assert.equal(world.has(entity, "absent"), false);
  world.remove(entity, "health");
  assert.equal(world.has(entity, "health"), false);
  assert.equal(world.get(entity, "health"), undefined);
  // Still carries the kind that was not removed.
  assert.equal(world.has(entity, "enemy"), true);
});

test("setParent links a child and parentOf reads it back (root is the empty value)", () => {
  const world = makeWorld(new FakeBridge());
  const parent = world.spawn({ kind: "node" });
  const child = world.spawn({ kind: "node" });
  assert.equal(world.parentOf(child), undefined);
  world.setParent(child, parent);
  assert.equal(world.parentOf(child), parent);
  assert.deepEqual(world.childrenOf(parent), [child]);
});

test("worldTransform reads the resolved transform for a live node, empty for a stale one", () => {
  const fake = new FakeBridge();
  const world = makeWorld(fake);
  const entity = world.spawn({ kind: "node" });
  // A live node with no scripted override reads the identity pose the bridge returns.
  assert.deepEqual(world.worldTransform(entity), {
    position: { x: 0, y: 0, z: 0 },
    rotation: [0, 0, 0, 1],
    scale: { x: 1, y: 1, z: 1 },
  });
  // A scripted bridge transform is forwarded verbatim (the projection adds no logic).
  const composed = {
    position: { x: 1, y: 2, z: 3 },
    rotation: [0, 0, 0, 1] as const,
    scale: { x: 1, y: 1, z: 1 },
  };
  fake.transforms.set(entity, composed);
  assert.deepEqual(world.worldTransform(entity), composed);
  // A stale handle is the empty value, never a throw.
  assert.equal(world.worldTransform(9999), undefined);
});
