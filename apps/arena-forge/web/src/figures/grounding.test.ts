import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../sim/content/bundle.ts";
import { figureForCard } from "./registry.ts";
import { expandFigure } from "./generator.ts";
import { type RootFrame, composeBuffers, composeWorld } from "./compose.ts";
import { vec3 } from "./vec3.ts";

const content = loadDefaultContent();
const IDENTITY_ROOT: RootFrame = { position: vec3(0, 0, 0), rotation: [0, 0, 0, 1], scale: 1 };

/*
 * Regression guard for the Emberkin bug: a rotated ROOT part rotates the whole
 * figure, sending it upside-down below the floor. Roots must never rotate (world
 * facing is applied by the RootFrame at pose time), and every composed figure must
 * stand above the ground plane.
 */

test("no figure has a rotated root part (a rotated root flips the whole body)", () => {
  for (const card of content.cards) {
    const fig = expandFigure(figureForCard(content, card.id), "high", false);
    fig.parts.forEach((p) => {
      if (p.compose.parentIndex < 0) {
        const r = p.compose.rest.rotationEuler;
        assert.ok(r.x === 0 && r.y === 0 && r.z === 0, `${card.id}: root part ${p.id} is rotated (${r.x},${r.y},${r.z})`);
      }
    });
  }
});

test("every figure composes upright and grounded (not sunk/flipped underground)", () => {
  for (const card of content.cards) {
    const fig = expandFigure(figureForCard(content, card.id), "high", false);
    const compose = fig.parts.map((p) => p.compose);
    const buf = composeBuffers(compose.length);
    composeWorld(compose, IDENTITY_ROOT, fig.parts.map(() => undefined), buf.frames, buf.out);
    let maxY = -Infinity;
    let minY = Infinity;
    for (const t of buf.out) {
      maxY = Math.max(maxY, t.position.y);
      minY = Math.min(minY, t.position.y);
    }
    assert.ok(maxY > 0.3, `${card.id}: figure has no height above ground (maxY=${maxY.toFixed(2)}) — likely flipped`);
    assert.ok(minY > -0.7, `${card.id}: figure sinks below the floor (minY=${minY.toFixed(2)}) — likely flipped`);
  }
});
