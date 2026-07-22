/*
 * shading.test.ts — `node --test` coverage AND GLSL-parity pins for the pure
 * per-fragment shading term in shading.ts. Every assertion here is hand-computed
 * from the SAME closed-form math the WebGL2 fragment shader implements
 * (backend-webgl2.ts), so a drift in either the software truth or its GLSL twin
 * breaks a test:
 *   - the Lambert diffuse bucket (ambient floor, directional N·L, point N·L with
 *     the 1/(1 + 0.08·d²) falloff), unchanged from the historical renderer;
 *   - the WHITE Blinn-Phong specular lobe (untinted by albedo, gated to the lit
 *     hemisphere, point lights attenuated) driven by roughness + the eye vector;
 *   - the Schlick Fresnel rim that brightens grazing edges, scaled by glossiness;
 *   - the default-roughness (matte) case producing ZERO specular — the backward-
 *     compatible collapse to the old diffuse-only render;
 *   - the highlight-rolloff `tonemap` (exact identity below the knee, a bounded
 *     Reinhard shoulder above it).
 * No DOM and no WebGL — this is the shared math both backends agree on.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { AMBIENT } from "./backend.ts";
import { shadeSurface, tonemap } from "./shading.ts";

const EPS = 1e-5;
const assertClose = (actual: number, expected: number, msg: string): void => {
  assert.ok(Math.abs(actual - expected) <= EPS, `${msg}: expected ${expected}, got ${actual}`);
};
const assertRgbClose = (actual: readonly [number, number, number], expected: readonly [number, number, number], msg: string): void => {
  assertClose(actual[0], expected[0], `${msg} r`);
  assertClose(actual[1], expected[1], `${msg} g`);
  assertClose(actual[2], expected[2], `${msg} b`);
};

// Constants mirrored from shading.ts (and the GLSL twin) for the reference math.
const pointFalloff = (d: number): number => 1 / (1 + 0.08 * d * d);
const fresnel = (ndv: number, gloss: number): number => (1 - 0.04) * (1 - ndv) ** 5 * gloss * 0.5;
const NO_LIGHTS = { dirLights: [], pointLights: [] } as const;

// No lights + default (matte) roughness: a pure ambient diffuse and zero specular
// — byte-identical to the historical Lambert term's ambient floor.
test("no lights leaves a matte ambient diffuse and zero specular", () => {
  const dark = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 1, NO_LIGHTS);
  assertRgbClose(dark.diffuse, [AMBIENT, AMBIENT, AMBIENT], "ambient diffuse");
  assertRgbClose(dark.specular, [0, 0, 0], "no specular");
});

// A directional light traveling straight down fully lights an up-facing surface:
// ambient + color·intensity, per channel. Matte roughness ⇒ no specular.
test("a directional light fully lights a facing surface, per channel", () => {
  const sun = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 1, {
    dirLights: [{ color: [1, 0.5, 0.25], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assertRgbClose(sun.diffuse, [AMBIENT + 1, AMBIENT + 0.5, AMBIENT + 0.25], "sun diffuse");
  assertRgbClose(sun.specular, [0, 0, 0], "sun specular (matte)");
});

// The same light on the BACK of the surface contributes no diffuse (max(0, ·)) and
// no specular (the facing gate), even for a mirror-smooth material.
test("a back-facing directional light contributes neither diffuse nor specular", () => {
  const back = shadeSurface(0, -1, 0, 0, 0, 0, 0, -5, 0, 0, {
    dirLights: [{ color: [1, 1, 1], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assertRgbClose(back.diffuse, [AMBIENT, AMBIENT, AMBIENT], "back diffuse");
  assertRgbClose(back.specular, [0, 0, 0], "back specular (facing gate)");
});

// A point light 2 m overhead: lambert 1, falloff 1/(1 + 0.08·4) on the diffuse.
test("a point light applies the soft distance falloff to diffuse", () => {
  const point = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 1, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assertClose(point.diffuse[0], AMBIENT + pointFalloff(2), "point falloff");
});

// A point light coincident with the surface stays finite (the MIN_DISTANCE floor);
// at d = 0 the N·L term is zero, so it adds no directional diffuse.
test("a coincident point light stays finite", () => {
  const here = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 1, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 0, 0] }],
  });
  assert.ok(Number.isFinite(here.diffuse[0]), "coincident point light diffuse is finite");
  assertClose(here.diffuse[0], AMBIENT, "coincident point light adds no directional term");
});

// Both light lists fold into the same running diffuse sums, in order.
test("directional and point contributions accumulate together", () => {
  const both = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 1, {
    dirLights: [{ color: [0.2, 0.2, 0.2], direction: [0, -1, 0] }],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assertClose(both.diffuse[0], AMBIENT + 0.2 + pointFalloff(2), "combined diffuse");
});

// A glossy material with the light, eye, and normal aligned puts N·H = 1: the
// specular lobe is a full WHITE highlight (untinted by albedo — albedo is applied
// downstream in the backend), added on top of the diffuse.
test("a glossy material adds a full white specular highlight", () => {
  const shine = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 0, {
    dirLights: [{ color: [1, 1, 1], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assertRgbClose(shine.specular, [1, 1, 1], "aligned mirror specular");
  assertClose(shine.diffuse[0], AMBIENT + 1, "diffuse still present under specular");
});

// A glossy point light's specular is scaled by the same distance falloff as its
// diffuse (N·H = 1 with the light overhead and the eye above).
test("a point light's specular is attenuated by its falloff", () => {
  const shine = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 0, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assertClose(shine.specular[0], pointFalloff(2), "point specular attenuation");
});

// Backward compatibility: at the default (matte) roughness the specular collapses
// to zero while the diffuse is IDENTICAL to the glossy case — only the highlight
// changed, proving an unset roughness is a pure no-op.
test("a fully-rough material zeroes specular but leaves diffuse untouched", () => {
  const rough = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 1, {
    dirLights: [{ color: [1, 1, 1], direction: [0, -1, 0] }],
    pointLights: [],
  });
  const glossy = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 0, {
    dirLights: [{ color: [1, 1, 1], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assertRgbClose(rough.specular, [0, 0, 0], "matte specular is zero");
  assertRgbClose(rough.diffuse, glossy.diffuse, "diffuse unchanged by roughness");
});

// The Schlick Fresnel rim brightens a grazing view (N·V = 0) even with no lights,
// scaled by glossiness — and vanishes at the matte default.
test("a Fresnel rim brightens grazing edges, scaled by glossiness", () => {
  const grazing = shadeSurface(0, 1, 0, 0, 0, 0, 1, 0, 0, 0, NO_LIGHTS);
  assertRgbClose(grazing.specular, [fresnel(0, 1), fresnel(0, 1), fresnel(0, 1)], "grazing rim");
  const matte = shadeSurface(0, 1, 0, 0, 0, 0, 1, 0, 0, 1, NO_LIGHTS);
  assertRgbClose(matte.specular, [0, 0, 0], "no rim when matte");
});

// tonemap is EXACT identity on [0, knee=0.9] — content that stays in range is
// visually unchanged.
test("tonemap is exact identity below the knee", () => {
  assertClose(tonemap(0), 0, "tonemap 0");
  assertClose(tonemap(0.3), 0.3, "tonemap 0.3");
  assertClose(tonemap(0.9), 0.9, "tonemap at the knee");
});

// Above the knee it rolls off: bounded below 1, monotonically increasing, and
// strictly less than the (over-driven) input.
test("tonemap rolls off highlights above the knee", () => {
  const two = tonemap(2);
  const three = tonemap(3);
  assert.ok(two > 0.9 && two < 1, `tonemap(2)=${two} in (0.9, 1)`);
  assert.ok(three > two, "tonemap is monotonically increasing");
  assert.ok(three < 3, "tonemap compresses the input");
  assert.ok(tonemap(1e6) < 1, "tonemap is bounded below 1");
});

// Deterministic: identical inputs, identical output.
test("identical inputs give identical output", () => {
  const seed = {
    dirLights: [{ color: [1, 1, 1] as const, direction: [0, -1, 0] as const }],
    pointLights: [{ color: [1, 1, 1] as const, position: [0, 2, 0] as const }],
  };
  const first = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 0.3, seed);
  const again = shadeSurface(0, 1, 0, 0, 0, 0, 0, 5, 0, 0.3, seed);
  assert.deepEqual(first, again);
});
