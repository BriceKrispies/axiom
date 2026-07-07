import assert from "node:assert/strict";
import { test } from "node:test";

import { FakeBridge } from "./fake-bridge.testkit.ts";
import { component } from "./manifest.ts";
import { migrateComponents } from "./reconcile.ts";

const doubleFirstByte = (bytes: Uint8Array): Uint8Array => Uint8Array.of((bytes[0] ?? 0) * 2);
const identityBytes = (bytes: Uint8Array): Uint8Array => bytes;

test("migrateComponents rewrites the raw bytes of every carrier of a migrated component", () => {
  const bridge = new FakeBridge();
  // Two entities carry `health`; seed prior-layout bytes via the raw column.
  bridge.worldRawSet(1, "health", Uint8Array.of(10));
  bridge.worldRawSet(2, "health", Uint8Array.of(20));

  migrateComponents(bridge, [component("health", { migrate: doubleFirstByte, version: 2 })]);

  assert.deepEqual([...bridge.worldRawGet(1, "health")], [20]);
  assert.deepEqual([...bridge.worldRawGet(2, "health")], [40]);
});

test("migrateComponents skips a component with no migrator (branch-off case)", () => {
  const bridge = new FakeBridge();
  bridge.worldRawSet(1, "mana", Uint8Array.of(7));

  migrateComponents(bridge, [component("mana", { version: 2 })]);

  // No migrator ⇒ the bytes are untouched.
  assert.deepEqual([...bridge.worldRawGet(1, "mana")], [7]);
});

test("migrateComponents is a no-op for a component with no carriers", () => {
  const bridge = new FakeBridge();
  // No entity carries `absent`; the query returns empty, the inner each never runs.
  migrateComponents(bridge, [component("absent", { migrate: identityBytes, version: 2 })]);
  assert.deepEqual(bridge.worldQuery(["absent"]), []);
});
