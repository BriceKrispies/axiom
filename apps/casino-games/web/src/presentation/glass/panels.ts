/*
 * panels.ts — the shared, inexpensive GLASS presentation. One recipe, reused
 * by every machine game: a low-opacity cyan tint pane, brighter edge
 * highlights, and one or two broad diagonal reflection streaks. No refraction,
 * no full-screen blur — just alpha-blended Lambert quads the current renderer
 * (WebGL2 or the Canvas2D fallback) already supports. Panels frame the view;
 * they are placed so they never obstruct the interaction target.
 */

import type { MaterialSpec, SceneInstance } from "@axiom/web-engine";
import type { EngineVec3 } from "@axiom/web-engine";
import { quatRoll } from "../stage/vectors.ts";

/** Spread these into a game's declared materials to use `glassPane`. */
export const GLASS_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  GlassEdge: { baseColor: [0.85, 0.97, 1, 1], emissive: [0.5, 0.65, 0.72, 1], opacity: 0.85 },
  GlassStreak: { baseColor: [1, 1, 1, 1], emissive: [0.55, 0.62, 0.68, 1], opacity: 0.22 },
  GlassTint: { baseColor: [0.62, 0.86, 0.98, 1], emissive: [0.05, 0.09, 0.12, 1], opacity: 0.14 },
};

/**
 * One glass pane facing +Z: the tint sheet, four edge highlights, and up to
 * two diagonal streaks. `center` is the pane center; `width`/`height` its
 * extents; `keyPrefix` keeps instance keys unique per pane.
 */
export const glassPane = (
  keyPrefix: string,
  center: EngineVec3,
  width: number,
  height: number,
  streaks = 2,
): readonly SceneInstance[] => {
  const t = 0.015;
  const edge = 0.035;
  const rim = (key: string, x: number, y: number, sx: number, sy: number): SceneInstance => ({
    key: `${keyPrefix}:${key}`,
    material: "GlassEdge",
    mesh: "box",
    transform: {
      position: { x: center.x + x, y: center.y + y, z: center.z },
      rotation: [0, 0, 0, 1],
      scale: { x: sx, y: sy, z: t },
    },
  });
  const streak = (i: number): SceneInstance => ({
    key: `${keyPrefix}:streak${i}`,
    material: "GlassStreak",
    mesh: "box",
    transform: {
      position: {
        x: center.x + (i === 0 ? -width * 0.18 : width * 0.24),
        y: center.y + (i === 0 ? height * 0.12 : -height * 0.08),
        z: center.z + t,
      },
      rotation: quatRoll(-0.62),
      scale: { x: width * 0.09, y: height * 1.1, z: t / 2 },
    },
  });
  return [
    {
      key: `${keyPrefix}:tint`,
      material: "GlassTint",
      mesh: "box",
      transform: { position: center, rotation: [0, 0, 0, 1], scale: { x: width, y: height, z: t } },
    },
    rim("top", 0, height / 2, width + edge, edge),
    rim("bottom", 0, -height / 2, width + edge, edge),
    rim("left", -width / 2, 0, edge, height + edge),
    rim("right", width / 2, 0, edge, height + edge),
    ...Array.from({ length: Math.min(streaks, 2) }, (_, i) => streak(i)),
  ];
};
