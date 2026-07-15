/*
 * scene.ts — Prize Elevator presentation: a vertical prize tower. A glass-fronted
 * car rides a shaft past labeled reward floors; indicator lamps up the side light
 * in order as the car climbs; at the committed floor the car settles and its two
 * doors slide apart onto a vignette — a pedestal + reward prop for a win, or a
 * friendly supply closet (broom + bucket) for a loss. The camera target tracks
 * the car height with restrained smoothing. Pure view: returns a Scene value.
 */

import type { GameResources, MaterialSpec, Rgba, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { GLASS_MATERIALS, glassPane } from "../../presentation/glass/panels.ts";
import { RARITY_COLORS, REWARD_MATERIALS, rewardBeam, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutElastic, lerp, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { QUAT_IDENTITY, quatPitch, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { ElevatorSpec, ElevatorState } from "./game.ts";
import {
  carHeight,
  committedFloorIndex,
  doorOpen,
  FLOOR_BASE,
  FLOOR_SPACING,
  floorHeight,
  floorLit,
  rideTimeline,
} from "./game.ts";

const SHAFT_HALF = 0.95;
const CAR_HALF = 0.82;

const rarityColorOf = (runtime: GameRuntime<ElevatorSpec>, tierId: string | null): Rgba => {
  const tier = runtime.config.rewardTiers.find((t) => t.id === tierId);
  return tier === undefined ? [0.7, 0.75, 0.85, 1] : RARITY_COLORS[tier.rarity];
};

/** Declared once per mount: primitives + a per-floor lamp/label material. */
export const elevatorResources = (runtime: GameRuntime<ElevatorSpec>): GameResources => {
  const spec = runtime.config.gameSpecific;
  const materials: Record<string, MaterialSpec> = {
    ...STAGE_MATERIALS,
    ...REWARD_MATERIALS,
    ...CONFETTI_MATERIALS,
    ...GLASS_MATERIALS,
    Broom: { baseColor: [0.6, 0.42, 0.24, 1] },
    BroomHead: { baseColor: [0.85, 0.7, 0.35, 1] },
    Bucket: { baseColor: [0.5, 0.72, 0.85, 1], emissive: [0.1, 0.16, 0.2, 1] },
    ButtonIdle: { baseColor: [1, 0.75, 0.35, 1], emissive: [0.5, 0.32, 0.1, 1] },
    ButtonLit: { baseColor: [1, 0.9, 0.5, 1], emissive: [1, 0.7, 0.28, 1] },
    Car: { baseColor: [0.95, 0.5, 0.42, 1], emissive: [0.2, 0.08, 0.06, 1] },
    CarTrim: { baseColor: [1, 0.8, 0.32, 1], emissive: [0.35, 0.24, 0.05, 1] },
    Door: { baseColor: [0.86, 0.88, 0.94, 1], emissive: [0.16, 0.18, 0.22, 1] },
    LampOff: { baseColor: [0.4, 0.42, 0.5, 1], emissive: [0.05, 0.05, 0.06, 1] },
    Shaft: { baseColor: [0.24, 0.28, 0.4, 1], emissive: [0.04, 0.05, 0.08, 1] },
  };
  spec.floors.forEach((floor, i) => {
    const color = floor.tierId === null ? ([0.72, 0.78, 0.88, 1] as Rgba) : rarityColorOf(runtime, floor.tierId);
    materials[`lamp${i}`] = { baseColor: color, emissive: [color[0] * 0.9, color[1] * 0.9, color[2] * 0.9, 1] };
  });
  return {
    materials,
    meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
  };
};

/** The car's world height for the active phase (rests at the ground before launch). */
const carY = (state: ElevatorState, targetIndex: number): number => {
  const session = state.session;
  const timeline = rideTimeline(targetIndex, session.config.presentationSpeed, false);
  if (session.phase === "revealing") {
    const age = phaseAge(session);
    const settleAge = age - (timeline.cruise + timeline.decel);
    const bounce = settleAge >= 0 ? (1 - easeOutElastic(clamp01(settleAge / timeline.settle))) * 0.12 : 0;
    return carHeight(age, targetIndex, timeline) + bounce;
  }
  return session.phase === "celebrating" || session.phase === "complete" ? floorHeight(targetIndex) : FLOOR_BASE;
};

export const elevatorScene = (runtime: GameRuntime<ElevatorSpec>, state: ElevatorState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const floorCount = spec.floors.length;
  const targetIndex = committedFloorIndex(session);
  const timeline = rideTimeline(targetIndex, session.config.presentationSpeed, false);
  const topY = floorHeight(floorCount - 1);
  const y = carY(state, targetIndex);
  const age = session.phase === "revealing" ? phaseAge(session) : session.phase === "celebrating" || session.phase === "complete" ? timeline.total : -1;

  // Shaft: back wall + two rails spanning the tower.
  const shaftMidY = (FLOOR_BASE + topY) / 2;
  const shaftHeight = topY - FLOOR_BASE + FLOOR_SPACING;
  const chrome: SceneInstance[] = [
    { key: "shaft:back", material: "Shaft", mesh: "box", transform: { position: v3(0, shaftMidY, -0.5), rotation: QUAT_IDENTITY, scale: v3(SHAFT_HALF * 2 + 0.3, shaftHeight, 0.2) } },
    { key: "shaft:railL", material: "StageHousingDark", mesh: "box", transform: { position: v3(-SHAFT_HALF - 0.15, shaftMidY, 0.1), rotation: QUAT_IDENTITY, scale: v3(0.2, shaftHeight, 1) } },
    { key: "shaft:railR", material: "StageHousingDark", mesh: "box", transform: { position: v3(SHAFT_HALF + 0.15, shaftMidY, 0.1), rotation: QUAT_IDENTITY, scale: v3(0.2, shaftHeight, 1) } },
  ];

  // Floor indicator lamps + label ledges up the right rail.
  const floors: SceneInstance[] = [];
  spec.floors.forEach((_floor, i) => {
    const lit = age >= 0 && floorLit(i, age, targetIndex, timeline);
    floors.push({
      key: `lamp${i}`,
      material: lit ? `lamp${i}` : "LampOff",
      mesh: "sphere",
      transform: { position: v3(SHAFT_HALF + 0.15, floorHeight(i), 0.62), rotation: QUAT_IDENTITY, scale: v3(0.18, 0.18, 0.18) },
    });
    floors.push({
      key: `ledge${i}`,
      material: "CarTrim",
      mesh: "box",
      transform: { position: v3(-SHAFT_HALF - 0.15, floorHeight(i) - 0.42, 0.35), rotation: QUAT_IDENTITY, scale: v3(0.5, 0.06, 0.7) },
    });
  });

  // The launch button (glowing while ready, latched while riding).
  const idle = session.phase === "ready";
  const button: SceneInstance = {
    key: "button",
    material: idle && pulse((session.tick % 44) / 44) > 0.5 ? "ButtonLit" : idle ? "ButtonIdle" : "ButtonLit",
    mesh: "cylinder",
    transform: { position: v3(0, FLOOR_BASE - 0.5, 1.1), rotation: quatPitch(Math.PI / 2), scale: v3(0.5, 0.24, 0.5) },
  };

  // The car body + gold trim + glass front + parting doors.
  const car: SceneInstance[] = [
    { key: "car:body", material: "Car", mesh: "box", transform: { position: v3(0, y, 0), rotation: QUAT_IDENTITY, scale: v3(CAR_HALF * 2, FLOOR_SPACING - 0.1, CAR_HALF * 1.4) } },
    { key: "car:trim-top", material: "CarTrim", mesh: "box", transform: { position: v3(0, y + (FLOOR_SPACING - 0.1) / 2, 0), rotation: QUAT_IDENTITY, scale: v3(CAR_HALF * 2 + 0.1, 0.1, CAR_HALF * 1.4 + 0.1) } },
    { key: "car:trim-bot", material: "CarTrim", mesh: "box", transform: { position: v3(0, y - (FLOOR_SPACING - 0.1) / 2, 0), rotation: QUAT_IDENTITY, scale: v3(CAR_HALF * 2 + 0.1, 0.1, CAR_HALF * 1.4 + 0.1) } },
  ];
  const open = age >= 0 ? doorOpen(age, timeline) : 0;
  const doorShift = open * (CAR_HALF - 0.06);
  car.push(
    { key: "door:left", material: "Door", mesh: "box", transform: { position: v3(-CAR_HALF / 2 - doorShift, y, CAR_HALF * 0.72), rotation: QUAT_IDENTITY, scale: v3(CAR_HALF - 0.04, FLOOR_SPACING - 0.24, 0.08) } },
    { key: "door:right", material: "Door", mesh: "box", transform: { position: v3(CAR_HALF / 2 + doorShift, y, CAR_HALF * 0.72), rotation: QUAT_IDENTITY, scale: v3(CAR_HALF - 0.04, FLOOR_SPACING - 0.24, 0.08) } },
    ...glassPane("car:glass", v3(0, y + 0.35, CAR_HALF * 0.76), CAR_HALF * 1.5, 0.7, 1),
  );

  // Floor vignette revealed behind the doors at the committed floor.
  const vignette: SceneInstance[] = [];
  const plan = session.committed;
  if (plan !== null && open > 0.15) {
    const rarity = outcomeRarity(session);
    const at = v3(0, floorHeight(targetIndex) - 0.35, -0.1);
    if (plan.win && rarity !== "loss") {
      vignette.push(
        { key: "vig:pedestal", material: "StagePedestal", mesh: "cylinder", transform: { position: v3(0, floorHeight(targetIndex) - 0.55, -0.1), rotation: QUAT_IDENTITY, scale: v3(0.5, 0.28, 0.5) } },
        ...rewardProp("reward", rarity, v3(at.x, at.y + 0.1, at.z), open, session.tick, 0.9),
      );
    } else {
      vignette.push(
        { key: "vig:broom", material: "Broom", mesh: "cylinder", transform: { position: v3(-0.28, floorHeight(targetIndex) - 0.25, -0.1), rotation: QUAT_IDENTITY, scale: v3(0.08, 0.85, 0.08) } },
        { key: "vig:broomhead", material: "BroomHead", mesh: "box", transform: { position: v3(-0.28, floorHeight(targetIndex) - 0.62, -0.1), rotation: QUAT_IDENTITY, scale: v3(0.24, 0.16, 0.14) } },
        { key: "vig:bucket", material: "Bucket", mesh: "cylinder", transform: { position: v3(0.26, floorHeight(targetIndex) - 0.62, 0.05), rotation: QUAT_IDENTITY, scale: v3(0.34, 0.32, 0.34) } },
        ...sparkleRing("vig:dust", v3(0, floorHeight(targetIndex) - 0.2, 0.1), 6, plan.presentationSeed, age - (timeline.cruise + timeline.decel + timeline.settle), 44),
      );
    }
  }

  // Celebration + reward beam at the car.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = v3(0, floorHeight(targetIndex) + 0.5, 0.4);
    if (profile.beam) {
      celebration.push(rewardBeam("beam", v3(0, floorHeight(targetIndex) - 0.5, -0.1), clamp01(phaseAge(session) / 20)));
    }
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera target tracks the car with restrained smoothing.
  const follow = runtime.settings.reducedMotion ? 0.3 : 0.72;
  const camY = lerp(shaftMidY, y, follow);
  const lights: SceneLight[] = [...stageLights(v3(0, camY, 1), 0.6)];
  if (age >= 0 && open > 0.1) {
    lights.push({ key: "light:floor", light: { color: [1, 0.86, 0.55, 1], intensity: 1.2, kind: "point", position: v3(0, floorHeight(targetIndex), 0.6) } });
  }

  return {
    camera: showcaseCamera(v3(0, camY, 0), 6.6, 0.5, 0.9),
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(18), ...chrome, ...floors, button, ...car, ...vignette, ...celebration],
    lights,
  };
};
