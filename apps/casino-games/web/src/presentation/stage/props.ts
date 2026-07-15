/*
 * props.ts — the shared pavilion stagecraft: the pastel backdrop, the stage
 * floor, pedestals, machine housings, and the standard three-light rig. Games
 * arrange these instead of re-inventing the room, so the whole app reads as
 * one bright, toy-like pavilion.
 */

import type { MaterialSpec, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineVec3, Rgba } from "@axiom/web-engine";
import { QUAT_IDENTITY, v3 } from "./vectors.ts";

/** The shared pavilion palette (pastel sky, turquoise, coral, gold…). */
export const STAGE_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  StageBackdrop: { baseColor: [0.66, 0.84, 0.98, 1], emissive: [0.42, 0.55, 0.68, 1] },
  StageFloor: { baseColor: [0.94, 0.9, 0.82, 1] },
  StageFloorAccent: { baseColor: [0.32, 0.78, 0.76, 1] },
  StageGold: { baseColor: [1, 0.78, 0.28, 1], emissive: [0.3, 0.2, 0.04, 1] },
  StageHousing: { baseColor: [0.98, 0.55, 0.45, 1] },
  StageHousingDark: { baseColor: [0.72, 0.36, 0.3, 1] },
  StagePedestal: { baseColor: [0.85, 0.76, 0.98, 1] },
  StageShadow: { baseColor: [0.1, 0.14, 0.18, 1], opacity: 0.3 },
};

/** The default clear color: pastel sky blue. */
export const SKY_CLEAR: Rgba = [0.71, 0.87, 0.99, 1];

/** Backdrop sheet + floor slab centered under the playfield. */
export const stageRoom = (span = 20): readonly SceneInstance[] => [
  {
    key: "stage:floor",
    material: "StageFloor",
    mesh: "box",
    transform: { position: v3(0, -0.55, 0), rotation: QUAT_IDENTITY, scale: v3(span, 1, span) },
  },
  {
    key: "stage:floor-ring",
    material: "StageFloorAccent",
    mesh: "cylinder",
    transform: { position: v3(0, -0.049, 0), rotation: QUAT_IDENTITY, scale: v3(span * 0.5, 0.02, span * 0.5) },
  },
  {
    key: "stage:backdrop",
    material: "StageBackdrop",
    mesh: "box",
    transform: { position: v3(0, span * 0.28, -span * 0.55), rotation: QUAT_IDENTITY, scale: v3(span * 2.2, span, 0.4) },
  },
];

/** A rounded pedestal (cylinder + gold trim) whose top is at `topY`. */
export const pedestal = (key: string, at: EngineVec3, topY: number, radius = 0.8): readonly SceneInstance[] => [
  {
    key: `${key}:column`,
    material: "StagePedestal",
    mesh: "cylinder",
    transform: {
      position: v3(at.x, (topY + at.y) / 2, at.z),
      rotation: QUAT_IDENTITY,
      scale: v3(radius * 2, Math.max(0.05, topY - at.y), radius * 2),
    },
  },
  {
    key: `${key}:trim`,
    material: "StageGold",
    mesh: "cylinder",
    transform: { position: v3(at.x, topY, at.z), rotation: QUAT_IDENTITY, scale: v3(radius * 2.15, 0.06, radius * 2.15) },
  },
];

/** A soft round contact shadow under an object. */
export const contactShadow = (key: string, at: EngineVec3, radius: number): SceneInstance => ({
  key,
  material: "StageShadow",
  mesh: "cylinder",
  transform: { position: v3(at.x, 0.012, at.z), rotation: QUAT_IDENTITY, scale: v3(radius * 2, 0.01, radius * 2) },
});

/**
 * The standard rig: a warm key from the upper right, a cool sky fill, and a
 * focal point light that games may re-pose onto the reveal (`focus`).
 */
export const stageLights = (focus: EngineVec3, focusIntensity = 0.9): readonly SceneLight[] => [
  {
    key: "light:key",
    light: { color: [1, 0.96, 0.88, 1], direction: v3(-0.45, -0.78, -0.42), intensity: 0.95, kind: "directional" },
  },
  {
    key: "light:fill",
    light: { color: [0.75, 0.86, 1, 1], direction: v3(0.5, -0.4, 0.75), intensity: 0.35, kind: "directional" },
  },
  {
    key: "light:focus",
    light: { color: [1, 0.92, 0.7, 1], intensity: focusIntensity, kind: "point", position: v3(focus.x, focus.y + 1.6, focus.z + 0.6) },
  },
];

/** The interior housing of a machine: back wall, ceiling, side rails, floor —
 * everything the machine-interior camera sees around the edges of its view. */
export const machineHousing = (keyPrefix: string, center: EngineVec3, size: EngineVec3): readonly SceneInstance[] => {
  const wall = 0.12;
  const part = (key: string, position: EngineVec3, scale: EngineVec3, material = "StageHousing"): SceneInstance => ({
    key: `${keyPrefix}:${key}`,
    material,
    mesh: "box",
    transform: { position, rotation: QUAT_IDENTITY, scale },
  });
  return [
    part("back", v3(center.x, center.y, center.z - size.z / 2), v3(size.x, size.y, wall), "StageHousingDark"),
    part("floor", v3(center.x, center.y - size.y / 2, center.z), v3(size.x, wall, size.z)),
    part("ceiling", v3(center.x, center.y + size.y / 2, center.z), v3(size.x, wall, size.z)),
    part("left", v3(center.x - size.x / 2, center.y, center.z), v3(wall, size.y, size.z)),
    part("right", v3(center.x + size.x / 2, center.y, center.z), v3(wall, size.y, size.z)),
    part("trim-top", v3(center.x, center.y + size.y / 2 - wall, center.z + size.z / 2 - wall), v3(size.x, wall / 2, wall / 2), "StageGold"),
    part("trim-bottom", v3(center.x, center.y - size.y / 2 + wall, center.z + size.z / 2 - wall), v3(size.x, wall / 2, wall / 2), "StageGold"),
  ];
};
