/*
 * label.ts — stamp brand lettering onto a surface as welded geometry. Given a
 * SURFACE FRAME (a world origin, an orientation whose local axes are
 * right/up/outward-normal, a local-space block center, and a per-axis basis
 * scale) it turns `glyphs.ts` cell runs into a `SceneInstance[]` of little
 * relief boxes — each one placed and scaled THROUGH that frame, so the lettering
 * rides whatever the frame rides.
 *
 * The `basis` scale is what makes the chest labels "stick": a chest label passes
 * the chest's own squash/grow as `basis` and the chest's quaternion as `orient`,
 * so the letters squash, grow, tilt, and spiral welded to the chest face — they
 * ARE part of the chest, not an overlay drawn near it. A flat sign passes a unit
 * basis and its own world rotation.
 *
 * Long names shrink to fit: the glyph cell is sized from the target height, then
 * scaled DOWN uniformly if the word would overrun `maxWidth`. Uniform (not
 * horizontal-only) so letters stay in proportion and legible, just smaller.
 */

import type { EngineQuat, EngineVec3, SceneInstance } from "@axiom/web-engine";
import { addV3, quatMul, quatRoll, rotateByQuat, v3 } from "../stage/vectors.ts";
import { GLYPH_H, STROKE_THICK, textColumns, textStrokes } from "./glyphs.ts";

/** Where a text block sits and how it is stretched. Local axes (before `basis`):
 * `+x` runs along the reading direction, `+y` up the surface, `+z` out of it. */
export interface SurfaceFrame {
  /** World anchor the local frame hangs off (e.g. the chest's `pose.at`). */
  readonly origin: EngineVec3;
  /** Local→world rotation: local `x`=right, `y`=up, `z`=outward normal. */
  readonly orient: EngineQuat;
  /** Local position of the text block's CENTER (before `basis`). */
  readonly center: EngineVec3;
  /** Per-local-axis scale applied to both offsets and box sizes — the chest's
   * `(squashXZ·grow, squashY·grow, squashXZ·grow)`, or `(1,1,1)` for a flat prop. */
  readonly basis: EngineVec3;
}

/** Fit + relief of the lettering, in LOCAL (pre-`basis`) units. */
export interface LabelStyle {
  readonly material: string;
  /** Target cap height. The word shrinks below this if it would overrun width. */
  readonly height: number;
  /** Max reading-direction width the word may occupy. */
  readonly maxWidth: number;
  /** How far the relief stands off the surface (local `+z`). */
  readonly lift: number;
  /** Box thickness along the normal. Defaults to `height · 0.14`. */
  readonly depth?: number;
}

const UNIT_BASIS: EngineVec3 = v3(1, 1, 1);

/**
 * The lettering of `text` as welded relief boxes. `text` is uppercased for the
 * font; empty / whitespace-only text yields nothing. One thin ROTATED box per
 * stroke centerline (see `glyphs.ts`), so a diagonal or a bowl is a real slanted
 * bar rather than a staircase of cubes — the Helvetica-idiom look.
 */
export const stampText = (keyPrefix: string, text: string, frame: SurfaceFrame, style: LabelStyle): readonly SceneInstance[] => {
  const upper = text.toUpperCase();
  const columns = textColumns(upper);
  const strokes = textStrokes(upper);
  if (columns === 0 || strokes.length === 0) {
    return [];
  }
  // Square cell from the target height, shrunk uniformly to honor maxWidth.
  const heightCell = style.height / GLYPH_H;
  const widthCell = style.maxWidth / columns;
  const cell = Math.min(heightCell, widthCell);
  const depth = style.depth ?? style.height * 0.14;
  const basis = frame.basis ?? UNIT_BASIS;
  const halfCols = columns / 2;

  return strokes.map((stroke, index): SceneInstance => {
    // Stroke center in cell space: `cx` across from the left edge (block-centered),
    // `cy` up from the baseline (block-centered on the cap-height midline).
    const cx = (stroke.cx - halfCols) * cell;
    const cy = (stroke.cy - GLYPH_H / 2) * cell;
    // Local offset of this box (block center + stroke offset, lifted off surface).
    const local = v3(frame.center.x + cx, frame.center.y + cy, frame.center.z + style.lift + depth / 2);
    const scaled = v3(local.x * basis.x, local.y * basis.y, local.z * basis.z);
    // The stroke runs along its own local +x, rotated by its angle IN the surface
    // plane (about the surface normal, local +z) then welded into the frame — so a
    // slanted stroke is a slanted box, welded to whatever the frame rides.
    const rotation = quatMul(frame.orient, quatRoll(stroke.angle));
    return {
      key: `${keyPrefix}:${index}`,
      material: style.material,
      mesh: "box",
      transform: {
        position: addV3(frame.origin, rotateByQuat(scaled, frame.orient)),
        rotation,
        scale: v3(stroke.len * cell * basis.x, STROKE_THICK * cell * basis.y, depth * basis.z),
      },
    };
  });
};
