import assert from "node:assert/strict";
import { test } from "node:test";

import { GameRegistry, activeRegistry, onFixedUpdate, onRender, useRegistry } from "./registry.ts";
import type { FixedUpdate, Render } from "./loop-core.ts";

const noopFixed: FixedUpdate = () => {
  // a fixed-update callback that records nothing
};
const otherFixed: FixedUpdate = () => {
  // a second, distinct fixed-update callback
};
const noopRender: Render = () => {
  // a render callback that records nothing
};

test("GameRegistry collects fixed updates in registration order", () => {
  const registry = new GameRegistry();
  assert.deepEqual(registry.fixedUpdates(), []);
  registry.onFixedUpdate(noopFixed);
  registry.onFixedUpdate(otherFixed);
  assert.deepEqual(registry.fixedUpdates(), [noopFixed, otherFixed]);
});

test("GameRegistry collects renders in registration order", () => {
  const registry = new GameRegistry();
  assert.deepEqual(registry.renders(), []);
  registry.onRender(noopRender);
  assert.deepEqual(registry.renders(), [noopRender]);
});

test("useRegistry installs the registry activeRegistry reads back", () => {
  const registry = new GameRegistry();
  useRegistry(registry);
  assert.equal(activeRegistry(), registry);
});

test("the free onFixedUpdate targets the active registry", () => {
  const registry = new GameRegistry();
  useRegistry(registry);
  onFixedUpdate(noopFixed);
  assert.equal(activeRegistry().fixedUpdates().length, 1);
  assert.equal(registry.fixedUpdates()[0], noopFixed);
});

test("the free onRender targets the active registry", () => {
  const registry = new GameRegistry();
  useRegistry(registry);
  onRender(noopRender);
  assert.equal(activeRegistry().renders().length, 1);
  assert.equal(registry.renders()[0], noopRender);
});

test("a freshly installed registry is independent of an earlier one", () => {
  const first = new GameRegistry();
  useRegistry(first);
  onFixedUpdate(noopFixed);
  const second = new GameRegistry();
  useRegistry(second);
  // The second starts empty; the first keeps its registration.
  assert.equal(second.fixedUpdates().length, 0);
  assert.equal(first.fixedUpdates().length, 1);
});
