/*
 * brand.ts — the white-label brand a casino game is dressed in: a display NAME
 * and a small color scheme (primary accent, signboard ink, and the lettering
 * color that sits on the primary). It is plain JSON-serializable config data
 * (it lives in a game's `gameSpecific` block), so a brand travels through the
 * same import/export/validation path as every other config field.
 *
 * This module owns the brand VALUE and the materials derived from it; `glyphs.ts`
 * owns the letterforms and `label.ts` welds lettering onto surfaces. Nothing here
 * touches the engine beyond the neutral `MaterialSpec`/`Rgba` shapes.
 */

import type { MaterialSpec, Rgba } from "@axiom/web-engine";

/** An sRGB triple, 0..1 — the same convention as `ThemeOverrides.accent`. */
export type Rgb = readonly [number, number, number];

/**
 * A brand: the name stamped across the scene, and the three colors that dress
 * it. `primary` is the accent (banners, flags, and the chest lettering);
 * `onPrimary` is the lettering that reads on a primary-colored banner; `ink` is
 * the dark body of the signboards and the floor mat (whose lettering is
 * `primary`).
 */
export interface BrandSpec {
  readonly name: string;
  readonly primary: Rgb;
  readonly onPrimary: Rgb;
  readonly ink: Rgb;
}

/** The ex-works default: the "ACME" red house brand shown in the reference. */
export const DEFAULT_BRAND: BrandSpec = {
  ink: [0.09, 0.1, 0.12],
  name: "ACME",
  onPrimary: [0.97, 0.95, 0.9],
  primary: [0.82, 0.16, 0.13],
};

const clamp01 = (x: number): number => Math.min(1, Math.max(0, x));
const scaleRgb = (c: Rgb, k: number): Rgb => [clamp01(c[0] * k), clamp01(c[1] * k), clamp01(c[2] * k)];
const mixRgb = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const rgba = (c: Rgb, alpha = 1): Rgba => [c[0], c[1], c[2], alpha];

/** The named materials every branded prop and label draws from, derived from one
 * brand. Prefixed `Brand*` so they never collide with a scene's own palette. */
export const brandMaterials = (brand: BrandSpec): Readonly<Record<string, MaterialSpec>> => ({
  // Banner / flag / pennant body, and a dimmer step for its shaded back faces.
  BrandPrimary: { baseColor: rgba(brand.primary) },
  BrandPrimaryDim: { baseColor: rgba(scaleRgb(brand.primary, 0.68)) },
  // Signboard / mat body (dark), with a lighter step for their raised borders.
  BrandInk: { baseColor: rgba(brand.ink) },
  BrandInkEdge: { baseColor: rgba(mixRgb(brand.ink, [1, 1, 1], 0.16)) },
  // Lettering. `BrandLetter` (primary) reads on wood and on the ink signs;
  // `BrandLetterOnPrimary` reads on a primary banner. Both carry a whisper of
  // emissive so small strokes stay legible under the raking rig on canvas2d.
  BrandLetter: { baseColor: rgba(brand.primary), emissive: rgba(scaleRgb(brand.primary, 0.14)) },
  BrandLetterDim: { baseColor: rgba(scaleRgb(brand.primary, 0.5)) },
  BrandLetterOnPrimary: { baseColor: rgba(brand.onPrimary), emissive: rgba(scaleRgb(brand.onPrimary, 0.06)) },
  // Sign / banner posts: a neutral dark wood, brand-independent.
  BrandPost: { baseColor: [0.2, 0.15, 0.1, 1] },
});

// ── validation ──────────────────────────────────────────────────────────────

const isRgb = (value: unknown): value is Rgb =>
  Array.isArray(value) && value.length === 3 && value.every((c) => typeof c === "number" && Number.isFinite(c) && c >= 0 && c <= 1);

/** Config issues for a brand block at `path` (empty when valid). Returns the
 * plain `{ message, path }` shape a game's `validateSpec` already emits. */
export const brandIssues = (brand: unknown, path: string): readonly { readonly message: string; readonly path: string }[] => {
  if (typeof brand !== "object" || brand === null) {
    return [{ message: "brand must be an object", path }];
  }
  const b = brand as Record<string, unknown>;
  const issues: { readonly message: string; readonly path: string }[] = [];
  if (typeof b.name !== "string" || b.name.trim().length === 0) {
    issues.push({ message: "brand.name must be a non-empty string", path: `${path}.name` });
  }
  for (const key of ["primary", "onPrimary", "ink"] as const) {
    if (!isRgb(b[key])) {
      issues.push({ message: `brand.${key} must be an [r, g, b] triple in [0, 1]`, path: `${path}.${key}` });
    }
  }
  return issues;
};

/** Read a `BrandSpec` out of an unknown `gameSpecific` block, or null when it
 * carries no valid brand — the guard the workbench uses to decide whether to
 * offer the brand controls. */
export const readBrand = (gameSpecific: unknown): BrandSpec | null => {
  if (typeof gameSpecific !== "object" || gameSpecific === null) {
    return null;
  }
  const brand = (gameSpecific as Record<string, unknown>).brand;
  return brandIssues(brand, "brand").length === 0 ? (brand as BrandSpec) : null;
};

// ── color <-> hex, for the setup UI's <input type="color"> controls ───────────

const byteHex = (channel: number): string =>
  Math.round(clamp01(channel) * 255)
    .toString(16)
    .padStart(2, "0");

/** `[r,g,b]` (0..1) → `#rrggbb`. */
export const rgbToHex = (c: Rgb): string => `#${byteHex(c[0])}${byteHex(c[1])}${byteHex(c[2])}`;

/** `#rrggbb` → `[r,g,b]` (0..1), or null when the string is not a 6-digit hex. */
export const hexToRgb = (hex: string): Rgb | null => {
  const match = /^#?([0-9a-fA-F]{6})$/.exec(hex.trim());
  if (match === null) {
    return null;
  }
  const digits = match[1] as string;
  return [parseInt(digits.slice(0, 2), 16) / 255, parseInt(digits.slice(2, 4), 16) / 255, parseInt(digits.slice(4, 6), 16) / 255];
};
