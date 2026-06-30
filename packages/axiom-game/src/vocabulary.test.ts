import assert from "node:assert/strict";
import { test } from "node:test";

import type {
  Cell,
  Circle,
  Component,
  ComponentKind,
  Entity,
  Handle,
  Mat4,
  PlayerId,
  Quat,
  RayHit,
  Rect,
  Result,
  Rgba,
  Seconds,
  Ticks,
  Transform,
  Vec2,
  Vec3,
} from "./vocabulary.ts";

// vocabulary.ts is TYPE-ONLY (every export is `export type`/`export interface`),
// erased at runtime — it carries no instrumented code and so does NOT appear in
// the coverage report. These checks are compile-time contract assertions (the
// literals must match each shape) that also run trivially: if a shape drifts, the
// typecheck — not this run — fails.

test("the opaque numeric handles are plain numbers", () => {
  const entity: Entity = 1;
  const ticks: Ticks = 60;
  const seconds: Seconds = 0.5;
  const handle: Handle = 7;
  const player: PlayerId = 3;
  assert.equal(entity, 1);
  assert.equal(ticks, 60);
  assert.equal(seconds, 0.5);
  assert.equal(handle, 7);
  assert.equal(player, 3);
});

test("the geometric records expose their declared fields", () => {
  const vec2: Vec2 = { x: 1, y: 2 };
  const vec3: Vec3 = { x: 1, y: 2, z: 3 };
  const cell: Cell = { x: 4, y: 5 };
  const rect: Rect = { height: 40, width: 30, x: 10, y: 20 };
  const circle: Circle = { center: { x: 1, y: 2 }, radius: 3 };
  assert.deepEqual(vec2, { x: 1, y: 2 });
  assert.equal(vec3.z, 3);
  assert.deepEqual(cell, { x: 4, y: 5 });
  assert.equal(rect.width, 30);
  assert.equal(rect.height, 40);
  assert.deepEqual(circle.center, { x: 1, y: 2 });
  assert.equal(circle.radius, 3);
});

test("the positional colour and math tuples carry plain numbers", () => {
  const rgba: Rgba = [1, 0.5, 0.25, 1];
  const quat: Quat = [0, 0, 0, 1];
  const mat4: Mat4 = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  assert.equal(rgba[3], 1);
  assert.equal(quat[3], 1);
  assert.equal(mat4.length, 16);
});

test("a Transform composes a position, a quaternion rotation, and a scale", () => {
  const transform: Transform = {
    position: { x: 1, y: 2, z: 3 },
    rotation: [0, 0, 0, 1],
    scale: { x: 1, y: 1, z: 1 },
  };
  assert.equal(transform.position.z, 3);
  assert.equal(transform.rotation[3], 1);
  assert.equal(transform.scale.x, 1);
});

test("a RayHit names the struck entity, entry point, and distance", () => {
  const hit: RayHit = { distance: 9, entity: 42, point: { x: 1, y: 0, z: 0 } };
  assert.equal(hit.entity, 42);
  assert.equal(hit.point.x, 1);
  assert.equal(hit.distance, 9);
});

test("Result is present-or-empty and Component carries its kind discriminant", () => {
  const present: Result<number> = 5;
  const empty: Result<number> = undefined;
  const kind: ComponentKind = "health";
  const component: Component = { kind };
  assert.equal(present, 5);
  assert.equal(empty, undefined);
  assert.equal(component.kind, "health");
});
