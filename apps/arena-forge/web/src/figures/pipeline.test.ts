import { strict as assert } from "node:assert";
import { test } from "node:test";

import { vec3 } from "./vec3.ts";
import { REST_IDENTITY } from "./grammar.ts";
import type { FigureDefinition, FigurePartDefinition } from "./grammar.ts";
import { expandFigure, partCount } from "./generator.ts";
import { composeBuffers, composeWorld } from "./compose.ts";
import type { RootFrame } from "./compose.ts";

const part = (over: Partial<FigurePartDefinition> & { id: string }): FigurePartDefinition => ({
  parent: null, tag: "body", primitive: "box", rest: REST_IDENTITY, extents: vec3(0.3, 0.3, 0.3), material: "primary", ...over,
});

const fig = (parts: FigurePartDefinition[], forgedAugment: FigurePartDefinition[] = []): FigureDefinition => ({
  cardId: "test_card", language: "ironbound", silhouette: "grunt", tier: 3, parts, forgedAugment,
  animation: "ironbound_default", seedSalt: 7, groundY: 0, footprint: 0.5,
});

test("mirror expands a part into a twin with negated x position", () => {
  const f = expandFigure(fig([
    part({ id: "root", tag: "root" }),
    part({ id: "arm", parent: "root", rest: { ...REST_IDENTITY, position: vec3(0.4, 1, 0) }, mirror: { axis: "x", idSuffix: "_r" } }),
  ]), "high", false);
  const ids = f.parts.map((p) => p.id);
  assert.ok(ids.includes("arm") && ids.includes("arm_r"));
  const twin = f.parts.find((p) => p.id === "arm_r");
  assert.equal(twin?.compose.rest.position.x, -0.4);
});

test("bounded repeat emits `count` copies with cumulative step", () => {
  const f = expandFigure(fig([
    part({ id: "root", tag: "root" }),
    part({ id: "petal", parent: "root", repeat: { count: 4, mode: "ring", step: { ...REST_IDENTITY, rotationEuler: vec3(0, 1, 0) } } }),
  ]), "high", false);
  assert.equal(f.parts.filter((p) => p.id.startsWith("petal")).length, 4);
});

test("tier gating drops high-tier parts on a low-tier figure", () => {
  const base = [part({ id: "root", tag: "root" }), part({ id: "banner", parent: "root", tierMin: 5 })];
  const low = expandFigure({ ...fig(base), tier: 2 }, "high", false);
  const high = expandFigure({ ...fig(base), tier: 6 }, "high", false);
  assert.ok(!low.parts.some((p) => p.id === "banner"));
  assert.ok(high.parts.some((p) => p.id === "banner"));
});

test("forged augmentation only appears when forged", () => {
  const f = fig([part({ id: "root", tag: "root" })], [part({ id: "aura", parent: "root", forgedOnly: true })]);
  assert.equal(partCount(expandFigure(f, "high", false)), 1);
  assert.equal(partCount(expandFigure(f, "high", true)), 2);
});

test("expansion is deterministic (same inputs ⇒ identical parts)", () => {
  const f = fig([part({ id: "root", tag: "root" }), part({ id: "spike", parent: "root", repeat: { count: 5, min: 2, mode: "fan", step: REST_IDENTITY, countVariationKey: "spikes" } })]);
  assert.deepEqual(expandFigure(f, "med", false).parts, expandFigure(f, "med", false).parts);
});

test("composeWorld places a child in its parent's rotated frame", () => {
  const f = expandFigure(fig([
    part({ id: "root", tag: "root", rest: { position: vec3(2, 0, 0), rotationEuler: vec3(0, Math.PI / 2, 0), scale: 1 } }),
    part({ id: "tip", parent: "root", rest: { ...REST_IDENTITY, position: vec3(1, 0, 0) }, extents: vec3(0.1, 0.1, 0.1) }),
  ]), "high", false);
  const compose = f.parts.map((p) => p.compose);
  const buf = composeBuffers(compose.length);
  const root: RootFrame = { position: vec3(0, 0, 0), rotation: [0, 0, 0, 1], scale: 1 };
  composeWorld(compose, root, [undefined, undefined], buf.frames, buf.out);
  // root at (2,0,0) rotated 90° about Y; child local +X → world -Z from the root.
  const tip = buf.out[1]!.position;
  assert.ok(Math.abs(tip.x - 2) < 1e-6 && Math.abs(tip.z + 1) < 1e-6, JSON.stringify(tip));
});
