/*
 * The named material palette — the TS twin of `penalty_materials.rs`'s
 * `PENALTY_PALETTE`. Every renderable references a color by name; the scene
 * builder registers one `createMaterial({ baseColor })` per name and reuses the
 * handle. Colors are linear RGB 0–1 (the engine's lit-color materials), used
 * verbatim — the Rust flat pre-shade quantizes to a 1.0 multiplier at the top
 * face, so real engine lights do the shading.
 */

import type { Rgba } from "@axiom/game";

export type MaterialName =
  | "FieldGrass"
  | "DarkerGrassBand"
  | "WhiteFieldLines"
  | "GoalFrameWhite"
  | "NetOffWhite"
  | "GoalieJerseyYellow"
  | "GoalieShortsBlack"
  | "GoalieSkin"
  | "GoalieHair"
  | "GoalieGloves"
  | "GoalieSocks"
  | "GoalieShoes"
  | "KickerJerseyBlue"
  | "KickerShortsWhite"
  | "KickerSkin"
  | "KickerHair"
  | "KickerSocksBlue"
  | "KickerShoes"
  | "BallWhite"
  | "BallDarkPanels"
  | "CrowdMutedColors"
  | "CrowdMutedColorsAltA"
  | "CrowdMutedColorsAltB"
  | "StadiumWallDarkGray"
  | "AdBoardRed"
  | "BlobShadow"
  | "ImpactFlash";

export const PALETTE: Record<MaterialName, Rgba> = {
  FieldGrass: [0.4, 0.63, 0.31, 1],
  DarkerGrassBand: [0.27, 0.48, 0.21, 1],
  WhiteFieldLines: [0.95, 0.97, 0.95, 1],
  GoalFrameWhite: [0.99, 0.99, 1.0, 1],
  NetOffWhite: [0.7, 0.72, 0.76, 1],
  GoalieJerseyYellow: [0.96, 0.82, 0.14, 1],
  GoalieShortsBlack: [0.1, 0.11, 0.13, 1],
  GoalieSkin: [0.82, 0.62, 0.48, 1],
  GoalieHair: [0.12, 0.09, 0.07, 1],
  GoalieGloves: [0.18, 0.62, 0.86, 1],
  GoalieSocks: [0.1, 0.1, 0.12, 1],
  GoalieShoes: [0.06, 0.06, 0.08, 1],
  KickerJerseyBlue: [0.1, 0.18, 0.74, 1],
  KickerShortsWhite: [0.93, 0.94, 0.96, 1],
  KickerSkin: [0.86, 0.66, 0.52, 1],
  KickerHair: [0.09, 0.07, 0.06, 1],
  KickerSocksBlue: [0.14, 0.27, 0.7, 1],
  KickerShoes: [0.06, 0.06, 0.08, 1],
  BallWhite: [0.97, 0.97, 0.98, 1],
  BallDarkPanels: [0.1, 0.11, 0.13, 1],
  CrowdMutedColors: [0.74, 0.54, 0.52, 1],
  CrowdMutedColorsAltA: [0.76, 0.62, 0.46, 1],
  CrowdMutedColorsAltB: [0.82, 0.7, 0.5, 1],
  StadiumWallDarkGray: [0.7, 0.68, 0.63, 1],
  AdBoardRed: [0.8, 0.15, 0.19, 1],
  BlobShadow: [0.11, 0.27, 0.08, 1],
  ImpactFlash: [1.0, 0.95, 0.7, 1],
};
