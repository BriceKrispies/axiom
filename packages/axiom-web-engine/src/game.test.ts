/*
 * game.test.ts — `node --test` coverage for the pure reconciler in game.ts. It
 * proves every diff branch of `reconcile`: first-frame spawns, transform re-poses,
 * unchanged nodes (no op), resource changes (despawn + respawn), removals, and the
 * full light lifecycle (add / re-set / remove). Pure values in, plan + memory out —
 * no DOM, no store — so the whole reconciler is exercised here.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { emptyMemory, reconcile } from "./game.ts";
import type { Light, Scene, SceneInstance, SceneLight, Transform } from "./index.ts";

const xform = (x: number, y = 0, z = 0): Transform => ({
  position: { x, y, z },
  rotation: [0, 0, 0, 1],
  scale: { x: 1, y: 1, z: 1 },
});

const inst = (key: string, transform: Transform, mesh = "box", material = "red"): SceneInstance => ({
  key,
  material,
  mesh,
  transform,
});

const dirLight = (key: string, intensity: number): SceneLight => ({
  key,
  light: { color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity, kind: "directional" },
});

const scene = (instances: readonly SceneInstance[], lights: readonly SceneLight[] = []): Scene => ({
  camera: { far: 100, fovY: 1, near: 0.1, position: { x: 0, y: 0, z: 5 }, target: { x: 0, y: 0, z: 0 } },
  instances,
  lights,
});

test("reconcile: an empty first frame spawns every instance", () => {
  const { plan, memory } = reconcile(emptyMemory(), scene([inst("a", xform(0)), inst("b", xform(1))]));
  assert.deepEqual(
    plan.spawns.map((s) => s.key),
    ["a", "b"],
  );
  assert.equal(plan.reposes.length, 0);
  assert.equal(plan.despawns.length, 0);
  assert.equal(memory.instances.size, 2);
});

test("reconcile: an unchanged instance produces no op", () => {
  const first = reconcile(emptyMemory(), scene([inst("a", xform(0))]));
  const second = reconcile(first.memory, scene([inst("a", xform(0))]));
  assert.equal(second.plan.spawns.length, 0);
  assert.equal(second.plan.reposes.length, 0);
  assert.equal(second.plan.despawns.length, 0);
});

test("reconcile: a moved instance re-poses (not respawns)", () => {
  const first = reconcile(emptyMemory(), scene([inst("a", xform(0))]));
  const second = reconcile(first.memory, scene([inst("a", xform(3, 4, 5))]));
  assert.equal(second.plan.spawns.length, 0);
  assert.deepEqual(
    second.plan.reposes.map((r) => r.key),
    ["a"],
  );
  assert.deepEqual(second.plan.reposes[0]!.transform.position, { x: 3, y: 4, z: 5 });
});

test("reconcile: a rotated/scaled instance re-poses too", () => {
  const first = reconcile(emptyMemory(), scene([inst("a", xform(0))]));
  const rotated: Transform = { position: { x: 0, y: 0, z: 0 }, rotation: [0, 1, 0, 0], scale: { x: 2, y: 1, z: 1 } };
  const second = reconcile(first.memory, scene([inst("a", rotated)]));
  assert.deepEqual(
    second.plan.reposes.map((r) => r.key),
    ["a"],
  );
});

test("reconcile: a vanished instance is despawned", () => {
  const first = reconcile(emptyMemory(), scene([inst("a", xform(0)), inst("b", xform(1))]));
  const second = reconcile(first.memory, scene([inst("a", xform(0))]));
  assert.deepEqual(second.plan.despawns, ["b"]);
  assert.equal(second.memory.instances.size, 1);
});

test("reconcile: changing an instance's mesh despawns and respawns it", () => {
  const first = reconcile(emptyMemory(), scene([inst("a", xform(0), "box")]));
  const second = reconcile(first.memory, scene([inst("a", xform(0), "sphere")]));
  assert.deepEqual(second.plan.despawns, ["a"]);
  assert.deepEqual(
    second.plan.spawns.map((s) => s.key),
    ["a"],
  );
  assert.equal(second.plan.reposes.length, 0);
});

test("reconcile: changing an instance's material despawns and respawns it", () => {
  const first = reconcile(emptyMemory(), scene([inst("a", xform(0), "box", "red")]));
  const second = reconcile(first.memory, scene([inst("a", xform(0), "box", "blue")]));
  assert.deepEqual(second.plan.despawns, ["a"]);
  assert.deepEqual(
    second.plan.spawns.map((s) => s.key),
    ["a"],
  );
});

test("reconcile: new lights are added, surviving lights re-set, gone lights removed", () => {
  const first = reconcile(emptyMemory(), scene([], [dirLight("sun", 1), dirLight("fill", 0.5)]));
  assert.deepEqual(
    first.plan.addLights.map((l) => l.key),
    ["sun", "fill"],
  );
  assert.equal(first.plan.setLights.length, 0);
  assert.equal(first.plan.removeLights.length, 0);

  const second = reconcile(first.memory, scene([], [dirLight("sun", 0.8)]));
  assert.equal(second.plan.addLights.length, 0);
  assert.deepEqual(
    second.plan.setLights.map((l) => l.key),
    ["sun"],
  );
  assert.deepEqual(second.plan.removeLights, ["fill"]);
});

test("reconcile: a point light survives the diff by key", () => {
  const point: Light = { color: [1, 0, 0, 1], intensity: 2, kind: "point", position: { x: 1, y: 2, z: 3 } };
  const first = reconcile(emptyMemory(), scene([], [{ key: "lamp", light: point }]));
  assert.deepEqual(
    first.plan.addLights.map((l) => l.key),
    ["lamp"],
  );
  const second = reconcile(first.memory, scene([], [{ key: "lamp", light: point }]));
  assert.deepEqual(
    second.plan.setLights.map((l) => l.key),
    ["lamp"],
  );
  assert.equal(second.memory.lights.size, 1);
});
