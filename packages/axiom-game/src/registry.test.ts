import assert from "node:assert/strict";
import { test } from "node:test";

import { GameRegistry, activeRegistry, onFixedUpdate, onRender, useRegistry } from "./registry.ts";
import type { FixedUpdate, Render } from "./loop-core.ts";
import { system } from "./manifest.ts";

const noopFixed: FixedUpdate = () => {
  // a fixed-update callback that records nothing
};
const otherFixed: FixedUpdate = () => {
  // a second, distinct fixed-update callback
};
const noopRender: Render = () => {
  // a render callback that records nothing
};
const swapped: FixedUpdate = () => {
  // the replacement body for a hot patch
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

test("upsert replaces a system body in place under the same id (the hot-patch primitive)", () => {
  const registry = new GameRegistry();
  registry.upsert(system("orb.spin", { phase: "fixedUpdate", run: noopFixed }));
  registry.upsert(system("orb.draw", { phase: "render", run: noopRender }));
  registry.upsert(system("orb.spin", { phase: "fixedUpdate", run: swapped }));
  // Same id → replaced, not appended; still one fixed update, now the swapped body.
  assert.deepEqual(registry.fixedUpdates(), [swapped]);
  assert.deepEqual(registry.renders(), [noopRender]);
});

test("remove drops a system by id; a stale id is a no-op", () => {
  const registry = new GameRegistry();
  registry.upsert(system("a", { phase: "fixedUpdate", run: noopFixed }));
  registry.remove("a");
  registry.remove("never"); // stale id: clean no-op
  assert.deepEqual(registry.fixedUpdates(), []);
});

test("get returns the mounted def or the empty value", () => {
  const registry = new GameRegistry();
  const def = system("a", { phase: "fixedUpdate", run: noopFixed });
  registry.upsert(def);
  assert.equal(registry.get("a"), def);
  assert.equal(registry.get("missing"), undefined);
});

test("the order key sorts systems ahead of later registrations", () => {
  const registry = new GameRegistry();
  registry.upsert(system("late", { order: 10, phase: "fixedUpdate", run: noopFixed }));
  registry.upsert(system("early", { order: -5, phase: "fixedUpdate", run: otherFixed }));
  // Despite `late` registering first, `early` (lower order) runs first.
  assert.deepEqual(registry.fixedUpdates(), [otherFixed, noopFixed]);
});
