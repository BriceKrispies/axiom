/*
 * glyphs.ts — the brand lettering's STROKE font: a clean, uniform-weight
 * uppercase-plus-digits alphabet drawn in the Helvetica idiom (a grotesque sans
 * — straight even strokes, true diagonals, round bowls), authored as centerline
 * segments rather than a pixel grid.
 *
 * WHY a stroke font, not the engine's 5×7 bitmap: the scene builds lettering out
 * of welded box MESHES (there is no textured text quad in this render path), and
 * the engine's bitmap font only produces axis-aligned CELL runs — so every letter
 * came out blocky and pixelated, with staircased diagonals and no round bowls.
 * These letterforms are centerlines instead: each glyph is a handful of straight
 * SEGMENTS (verticals, horizontals, diagonals) plus arcs approximated by short
 * segments, so `label.ts` can weld one thin ROTATED box per stroke and the
 * lettering reads as a smooth proportional sans — as close to Helvetica as box
 * geometry gets. (True Helvetica outlines are proprietary and would need real
 * font tessellation the engine does not offer; this is the honest approximation.)
 *
 * Coordinate space per glyph: x ∈ [0, GLYPH_W], y ∈ [0, GLYPH_H], y UP (baseline
 * at y = 0, cap line at y = GLYPH_H). Advance is uniform (GLYPH_W + GLYPH_GAP),
 * matching the old bitmap metrics so `label.ts`'s block-fit math is unchanged.
 */

/** Glyph cell: uniform advance width and cap height, in abstract cell units. */
export const GLYPH_W = 5;
export const GLYPH_H = 7;
export const GLYPH_GAP = 1;
/** Uniform stroke weight (cell units) — ~0.14 of the cap, the grotesque ratio. */
export const STROKE_THICK = 0.95;

/** One stroke centerline segment in glyph-local space: [x1, y1, x2, y2]. */
type Seg = readonly [number, number, number, number];

const seg = (x1: number, y1: number, x2: number, y2: number): Seg => [x1, y1, x2, y2];

/** A connected polyline (points) expanded to its segments. */
const poly = (...pts: readonly (readonly [number, number])[]): readonly Seg[] =>
  pts.slice(1).map((p, i): Seg => {
    const from = pts[i] as readonly [number, number];
    return seg(from[0], from[1], p[0], p[1]);
  });

/** An elliptical arc (a0→a1 radians, 0 = +x / right, π/2 = +y / top) as a
 * polyline of `steps` short segments — how the round bowls are drawn. */
const arc = (cx: number, cy: number, rx: number, ry: number, a0: number, a1: number, steps = 10): readonly Seg[] =>
  Array.from({ length: steps }, (_, i): Seg => {
    const t0 = a0 + (a1 - a0) * (i / steps);
    const t1 = a0 + (a1 - a0) * ((i + 1) / steps);
    return seg(cx + rx * Math.cos(t0), cy + ry * Math.sin(t0), cx + rx * Math.cos(t1), cy + ry * Math.sin(t1));
  });

// Shared metrics: stroke centerlines are inset half a stroke off the cell edges
// so the drawn weight stays inside the [0,5]×[0,7] box.
const L = 0.65;
const R = 4.35;
const B = 0.65;
const T = 6.35;
const MX = 2.5;
const MY = 3.5;
const RX = 1.85;
const RY = 2.85;
const PI = Math.PI;

/** The alphabet. Each glyph is the set of centerline segments that draw it. */
const GLYPHS: Readonly<Record<string, readonly Seg[]>> = {
  " ": [],
  "-": [seg(1.2, MY, 3.8, MY)],
  A: [...poly([L, B], [MX, T], [R, B]), seg(1.45, 2.7, 3.55, 2.7)],
  B: [
    seg(L, B, L, T),
    ...poly([L, T], [3.1, T], [4.2, 5.9], [3.1, MY], [L, MY]),
    ...poly([L, MY], [3.35, MY], [4.35, 2.05], [3.1, B], [L, B]),
  ],
  C: arc(MX + 0.15, MY, RX, RY, 0.32 * PI, 1.68 * PI, 12),
  D: [seg(L, B, L, T), ...poly([L, T], [2.7, T], [4.35, 4.7], [4.35, 2.3], [2.7, B], [L, B])],
  E: [seg(L, B, L, T), seg(L, T, R, T), seg(L, MY, 3.9, MY), seg(L, B, R, B)],
  F: [seg(L, B, L, T), seg(L, T, R, T), seg(L, MY, 3.9, MY)],
  G: [...arc(MX + 0.15, MY, RX, RY, 0.32 * PI, 1.62 * PI, 12), ...poly([4.35, 1.55], [4.35, 3.0], [3.0, 3.0])],
  H: [seg(L, B, L, T), seg(R, B, R, T), seg(L, MY, R, MY)],
  I: [seg(MX, B, MX, T)],
  J: [...poly([R, T], [R, 1.9], [3.7, 0.75], [2.1, 0.7], [1.0, 1.5], [0.7, 2.5])],
  K: [seg(L, B, L, T), seg(L, 3.2, R, T), seg(1.7, 3.75, R, B)],
  L: [seg(L, B, L, T), seg(L, B, 4.15, B)],
  M: [seg(L, B, L, T), seg(R, B, R, T), ...poly([L, T], [MX, 2.7], [R, T])],
  N: [seg(L, B, L, T), seg(R, B, R, T), seg(L, T, R, B)],
  O: arc(MX, MY, RX, RY, 0, 2 * PI, 16),
  P: [seg(L, B, L, T), ...poly([L, T], [3.1, T], [4.3, 5.55], [3.1, 3.9], [L, 3.9])],
  Q: [...arc(MX, MY, RX, RY, 0, 2 * PI, 16), seg(2.9, 2.1, 4.55, 0.35)],
  R: [seg(L, B, L, T), ...poly([L, T], [3.1, T], [4.3, 5.55], [3.1, 3.9], [L, 3.9]), seg(2.6, 3.9, R, B)],
  S: poly([4.2, 5.7], [3.1, T], [1.4, 6.55], [0.75, 5.2], [1.5, 4.05], [3.0, 3.55], [4.25, 2.75], [3.9, 1.2], [2.2, 0.65], [0.75, 1.3]),
  T: [seg(L, T, R, T), seg(MX, B, MX, T)],
  U: poly([L, T], [L, 2.2], [1.55, 0.8], [3.45, 0.8], [R, 2.2], [R, T]),
  V: [...poly([L, T], [MX, B], [R, T])],
  W: poly([0.5, T], [1.65, B], [MX, 4.3], [3.35, B], [4.5, T]),
  X: [seg(L, B, R, T), seg(L, T, R, B)],
  Y: [seg(L, T, MX, 3.7), seg(R, T, MX, 3.7), seg(MX, B, MX, 3.7)],
  Z: [seg(L, T, R, T), seg(R, T, L, B), seg(L, B, R, B)],
  "0": [...arc(MX, MY, RX - 0.1, RY, 0, 2 * PI, 16), seg(1.5, 1.7, 3.5, 5.3)],
  "1": [seg(2.7, B, 2.7, T), seg(1.5, 5.4, 2.7, T), seg(1.4, B, 4.0, B)],
  "2": poly([0.8, 5.5], [1.7, T], [3.3, T], [4.25, 5.6], [3.9, 4.0], [0.8, B], [4.35, B]),
  "3": [
    ...poly([0.8, 5.7], [1.9, T], [3.5, T], [4.25, 5.6], [3.2, 3.75], [4.35, 2.4], [3.4, 0.65], [1.7, 0.65], [0.7, 1.5]),
    seg(3.2, 3.75, 2.3, 3.75),
  ],
  "4": [seg(3.4, B, 3.4, T), seg(3.4, T, L, 2.3), seg(L, 2.3, R, 2.3)],
  "5": poly([4.15, T], [1.0, T], [0.85, 3.7], [3.1, 4.0], [4.3, 2.6], [3.4, 0.7], [1.6, 0.65], [0.7, 1.4]),
  "6": [...arc(MX, 2.15, RX, 1.65, 0, 2 * PI, 12), ...poly([0.8, 3.0], [1.1, 5.1], [2.7, T], [4.0, 5.9])],
  "7": [seg(L, T, R, T), seg(R, T, 1.9, B)],
  "8": [...arc(MX, 5.0, 1.55, 1.4, 0, 2 * PI, 12), ...arc(MX, 2.0, 1.85, 1.55, 0, 2 * PI, 14)],
  "9": [...arc(MX, 4.85, RX, 1.65, 0, 2 * PI, 12), ...poly([4.2, 4.0], [3.9, 1.9], [2.3, 0.65], [1.0, 1.1])],
};

/** The visible fallback for an unmapped character: a filled box outline, so an
 * unknown glyph reads as a placeholder rather than silently vanishing. */
const FALLBACK: readonly Seg[] = [seg(L, B, R, B), seg(R, B, R, T), seg(R, T, L, T), seg(L, T, L, B)];

/** Total advance width of `text` in cell units (uniform metrics: one glyph slot
 * per code unit, gaps between). Matches the old bitmap metric so a caller's
 * block-fit math is unchanged. */
export const textColumns = (text: string): number =>
  text.length === 0 ? 0 : text.length * GLYPH_W + (text.length - 1) * GLYPH_GAP;

/** One positioned stroke of a laid-out string: its center in cell space
 * (`cx` across from the left edge, `cy` up from the baseline), the stroke length
 * (joints overlapped by one stroke width so corners meet cleanly), and its angle
 * in the surface plane. `label.ts` welds one rotated box per stroke. */
export interface GlyphStroke {
  readonly cx: number;
  readonly cy: number;
  readonly len: number;
  readonly angle: number;
}

/** Lay `text` (uppercased) out into positioned strokes across the whole string.
 * Each glyph sits at its own advance origin; a space contributes no strokes but
 * still advances, exactly like the old bitmap font. */
export const textStrokes = (text: string): readonly GlyphStroke[] =>
  Array.from(text.toUpperCase()).flatMap((ch, index): readonly GlyphStroke[] => {
    const originCol = index * (GLYPH_W + GLYPH_GAP);
    const segs = GLYPHS[ch] ?? FALLBACK;
    return segs.map(([x1, y1, x2, y2]): GlyphStroke => {
      const dx = x2 - x1;
      const dy = y2 - y1;
      return {
        cx: originCol + (x1 + x2) / 2,
        cy: (y1 + y2) / 2,
        len: Math.hypot(dx, dy) + STROKE_THICK,
        angle: Math.atan2(dy, dx),
      };
    });
  });
