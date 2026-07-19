/*
 * figure.test.ts — the ported rigged player figure: the quat/transform math, the
 * rig that bakes a pose to world boxes, and the stateless running gait's anti-skate
 * (a planted foot stays fixed in the world while the body travels over it). Run with
 * `node --test`, like the rest of the game core.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { IDENTITY_QUAT, vec3 } from "./vec.ts";
import { combine, quatMul, quatRotate, transform, transformPoint } from "./figure-math.ts";
import { HEAD, L_FOOT, PART_COUNT, R_FOOT, bodyTransform, neutralPose, posedParts } from "./figure.ts";
import { runningPose } from "./figure-pose.ts";

// ── math primitives ──────────────────────────────────────────────────────────

test("quatMul by identity is a no-op; rotating a vector by identity is a no-op", () => {
  const q = [0.1, 0.2, 0.3, Math.sqrt(1 - 0.14)] as const;
  assert.deepEqual(quatMul(IDENTITY_QUAT, q), q);
  const v = vec3(1, 2, 3);
  const r = quatRotate(IDENTITY_QUAT, v);
  assert.ok(Math.hypot(r.x - 1, r.y - 2, r.z - 3) < 1e-9);
});

test("combine(parent, child) matches applying the transforms in sequence", () => {
  const parent = transform(vec3(1, 2, 3), [0, Math.SQRT1_2, 0, Math.SQRT1_2], vec3(2, 2, 2));
  const child = transform(vec3(0.5, 0, 0), [0, 0, 0, 1], vec3(1, 1, 1));
  const p = vec3(0.3, 0.4, 0.1);
  const viaCombine = transformPoint(combine(parent, child), p);
  const viaSequence = transformPoint(parent, transformPoint(child, p));
  assert.ok(Math.hypot(viaCombine.x - viaSequence.x, viaCombine.y - viaSequence.y, viaCombine.z - viaSequence.z) < 1e-6);
});

// ── rig ──────────────────────────────────────────────────────────────────────

test("the neutral figure stands on the ground with a raised head", () => {
  const parts = posedParts(neutralPose(), bodyTransform(vec3(0, 0, 0), 0, neutralPose(), 0));
  assert.equal(parts.length, PART_COUNT, "17 parts (head + cap crown + brim, no pads/helmet/facemask)");
  // A foot's sole (box center minus half its height) sits ~on the field.
  const foot = parts[L_FOOT]!.transform;
  const sole = foot.position.y - foot.scale.y / 2;
  assert.ok(Math.abs(sole) < 0.1, `foot sole near ground, got ${sole}`);
  // The head is a rounded sphere up around head height, at the ~25%-smaller scale.
  const head = parts[HEAD]!;
  assert.equal(head.mesh, "sphere", "the head is rounded");
  assert.ok(head.transform.position.y > 1.2, `head up high, got ${head.transform.position.y}`);
  assert.ok(head.transform.position.y < 1.7, `head lowered by the figure scale, got ${head.transform.position.y}`);
});

// ── anti-skate ─────────────────────────────────────────────────────────────────

test("the running gait plants feet: a foot is nearly stationary at some ticks and fast at others", () => {
  // A figure running straight down +Z at a steady clip. Track the LEFT foot's
  // per-tick world displacement: a planted foot barely moves (anti-skate), the
  // swinging foot moves faster than the body — the signature of foot-planting
  // rather than a rigidly body-locked leg.
  const dt = 1 / 60;
  const speed = 7.5;
  const bodyStep = speed * dt;
  let traveled = 0;
  let prev = null as null | { x: number; z: number };
  let minMove = Infinity;
  let maxMove = 0;
  for (let t = 0; t < 200; t += 1) {
    const ground = vec3(0, 0, traveled);
    const parts = posedParts(runningPose(ground, 0, speed, traveled), bodyTransform(ground, 0, runningPose(ground, 0, speed, traveled), 0));
    const f = parts[L_FOOT]!.transform.position;
    if (prev !== null && t > 20) {
      const move = Math.hypot(f.x - prev.x, f.z - prev.z);
      minMove = Math.min(minMove, move);
      maxMove = Math.max(maxMove, move);
    }
    prev = { x: f.x, z: f.z };
    traveled += bodyStep;
  }
  assert.ok(minMove < bodyStep * 0.5, `a planted foot barely moves (min ${minMove.toFixed(4)} << body step ${bodyStep.toFixed(4)})`);
  assert.ok(maxMove > bodyStep, `the swinging foot outpaces the body (max ${maxMove.toFixed(4)} > body step ${bodyStep.toFixed(4)})`);
});

test("the running gait is deterministic (identical inputs → identical pose)", () => {
  const a = runningPose(vec3(1, 0, 2), 0.4, 6, 3.3);
  const b = runningPose(vec3(1, 0, 2), 0.4, 6, 3.3);
  assert.deepEqual(a, b);
  // Feet resolve to a valid (finite) pose for both feet.
  for (const q of [...a.joints]) {
    assert.ok(q.every((c) => Number.isFinite(c)));
  }
  assert.ok(R_FOOT < PART_COUNT);
});
