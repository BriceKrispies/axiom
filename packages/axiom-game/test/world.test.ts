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
