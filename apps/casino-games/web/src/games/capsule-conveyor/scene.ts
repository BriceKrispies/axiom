/*
 * scene.ts — Capsule Conveyor presentation, seen from INSIDE the machine. The
 * machine-interior camera mounts near the upper-left corner; the housing frames
 * the view; an elliptical belt (slat boxes + two roller cylinders) carries the
 * capsules past a bank of cycling indicator lights, with the opening station on
 * the RIGHT. On arrival the station capsule twists open (two hemisphere halves
 * rotate apart) to a tier glow (win) or a cheerful empty wobble (loss). Glass
 * frames the view edges. Pure view.
 */

import type { MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { GLASS_MATERIALS, glassPane } from "../../presentation/glass/panels.ts";
import { RARITY_COLORS, REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, easeOutCubic } from "../../presentation/stage/easing.ts";
import { machineHousing, SKY_CLEAR, STAGE_MATERIALS, stageLights } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, quatRoll, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { CapsuleConveyorSpec, ConveyorState } from "./game.ts";
import {
  BELT_CENTER,
  BELT_RX,
  BELT_RY,
  CAPSULE_DIAMETER,
  beltProgress,
  capsuleWorldPosition,
  conveyorCamera,
  conveyorTimeline,
  destinationIndexOf,
  MACHINE_VOLUME,
  openingCapsuleIndex,
} from "./game.ts";

// ── capsule shell colors by index (pastel) ──────────────────────────────────────

const SHELL_COLORS: readonly string[] = ["ShellCoral", "ShellMint", "ShellSky", "ShellLemon", "ShellLilac", "ShellPeach"];

const shellMaterials = (): Readonly<Record<string, MaterialSpec>> => ({
  ShellCoral: { baseColor: [1, 0.6, 0.55, 1], emissive: [0.2, 0.08, 0.07, 1] },
  ShellLemon: { baseColor: [1, 0.9, 0.52, 1], emissive: [0.2, 0.17, 0.06, 1] },
  ShellLilac: { baseColor: [0.78, 0.66, 1, 1], emissive: [0.14, 0.1, 0.22, 1] },
  ShellMint: { baseColor: [0.62, 0.98, 0.76, 1], emissive: [0.09, 0.2, 0.12, 1] },
  ShellPeach: { baseColor: [1, 0.76, 0.6, 1], emissive: [0.2, 0.12, 0.08, 1] },
  ShellSky: { baseColor: [0.6, 0.82, 1, 1], emissive: [0.09, 0.15, 0.22, 1] },
});

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  ...GLASS_MATERIALS,
  ...shellMaterials(),
  Belt: { baseColor: [0.28, 0.3, 0.36, 1] },
  BeltSlat: { baseColor: [0.4, 0.43, 0.5, 1], emissive: [0.05, 0.06, 0.08, 1] },
  Button: { baseColor: [1, 0.35, 0.35, 1], emissive: [0.35, 0.06, 0.06, 1] },
  ButtonRim: { baseColor: [1, 0.82, 0.32, 1], emissive: [0.3, 0.2, 0.05, 1] },
  CapsuleClear: { baseColor: [0.9, 0.95, 1, 1], emissive: [0.2, 0.24, 0.3, 1], opacity: 0.6 },
  LampLit: { baseColor: [1, 0.92, 0.55, 1], emissive: [0.95, 0.82, 0.4, 1] },
  LampOff: { baseColor: [0.5, 0.48, 0.4, 1], emissive: [0.06, 0.05, 0.03, 1] },
  Roller: { baseColor: [0.8, 0.82, 0.88, 1], emissive: [0.12, 0.13, 0.16, 1] },
  Station: { baseColor: [1, 0.8, 0.32, 1], emissive: [0.3, 0.2, 0.05, 1] },
};

export const CONVEYOR_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

const shellMaterial = (index: number): string => SHELL_COLORS[index % SHELL_COLORS.length] as string;

export const conveyorScene = (runtime: GameRuntime<CapsuleConveyorSpec>, state: ConveyorState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const reduced = runtime.settings.reducedMotion;
  const tick = session.tick;
  const plan = session.committed;
  const count = spec.capsuleCount;
  const timeline = conveyorTimeline(session.config.presentationSpeed, reduced);
  const settled = session.phase === "celebrating" || session.phase === "complete";
  const revealAge = session.phase === "revealing" ? phaseAge(session) : settled ? timeline.total : -1;
  const opening = openingCapsuleIndex(spec, state, reduced);
  const dest = plan === null ? -1 : destinationIndexOf(session);
  const openT = revealAge >= timeline.armEnd ? clamp01((revealAge - timeline.armEnd) / (timeline.openEnd - timeline.armEnd)) : 0;

  // The belt: elliptical ring of slats + two rollers at the ends.
  const s = beltProgress(spec, state, reduced);
  const slats: SceneInstance[] = Array.from({ length: 28 }, (_, i) => {
    const theta = (i / 28 + s) * Math.PI * 2;
    return {
      key: `slat${i}`,
      material: "BeltSlat",
      mesh: "box",
      transform: {
        position: v3(BELT_CENTER.x + Math.cos(theta) * BELT_RX, BELT_CENTER.y + Math.sin(theta) * BELT_RY, BELT_CENTER.z - 0.28),
        rotation: quatRoll(theta),
        scale: v3(0.16, 0.36, 0.1),
      },
    };
  });
  const rollers: SceneInstance[] = [-1, 1].map((side) => ({
    key: `roller${side}`,
    material: "Roller",
    mesh: "cylinder",
    transform: { position: v3(BELT_CENTER.x + side * BELT_RX, BELT_CENTER.y, BELT_CENTER.z - 0.28), rotation: quatPitch(Math.PI / 2), scale: v3(BELT_RY * 2, 0.4, BELT_RY * 2) },
  }));

  // Indicator lights along the top rail (cycling — ambient decoration).
  const lamps: SceneInstance[] = Array.from({ length: 9 }, (_, i) => {
    const lit = (i + Math.floor(tick / 10)) % 3 === 0;
    return {
      key: `lamp${i}`,
      material: lit ? "LampLit" : "LampOff",
      mesh: "sphere",
      transform: { position: v3(-BELT_RX + (i / 8) * BELT_RX * 2, BELT_CENTER.y + BELT_RY + 0.55, BELT_CENTER.z + 0.1), rotation: QUAT_IDENTITY, scale: v3(0.12, 0.12, 0.12) },
    };
  });

  // The capsules. The opening capsule at the station splits into two halves.
  const capsules: SceneInstance[] = Array.from({ length: count }, (_, index) => {
    const at = capsuleWorldPosition(spec, state, reduced, index);
    const isOpening = index === opening && index === dest && openT > 0;
    if (isOpening) {
      const spread = easeOutBack(openT) * 0.24;
      const mat = shellMaterial(index);
      return [
        { key: `cap${index}:top`, material: mat, mesh: "sphere", transform: { position: v3(at.x, at.y + spread, at.z), rotation: quatRoll(openT * 1.2), scale: v3(CAPSULE_DIAMETER, CAPSULE_DIAMETER * 0.6, CAPSULE_DIAMETER) } },
        { key: `cap${index}:bottom`, material: mat, mesh: "sphere", transform: { position: v3(at.x, at.y - spread, at.z), rotation: quatRoll(-openT * 1.2), scale: v3(CAPSULE_DIAMETER, CAPSULE_DIAMETER * 0.6, CAPSULE_DIAMETER) } },
      ];
    }
    return [
      {
        key: `cap${index}`,
        material: shellMaterial(index),
        mesh: "sphere",
        transform: { position: at, rotation: quatYaw(tick * 0.02 + index), scale: v3(CAPSULE_DIAMETER, CAPSULE_DIAMETER, CAPSULE_DIAMETER) },
      },
    ];
  }).flat();

  // The opening station on the right + the STOP button.
  const stationAt = v3(BELT_CENTER.x + BELT_RX, BELT_CENTER.y, BELT_CENTER.z);
  const running = session.phase === "interacting";
  const pressT = session.phase === "committing" ? clamp01(phaseAge(session) / 8) : 0;
  const controls: SceneInstance[] = [
    { key: "station:platform", material: "Station", mesh: "box", transform: { position: v3(stationAt.x + 0.15, stationAt.y - 0.35, stationAt.z), rotation: QUAT_IDENTITY, scale: v3(0.7, 0.14, 0.7) } },
    { key: "station:post", material: "Roller", mesh: "box", transform: { position: v3(stationAt.x + 0.5, stationAt.y + 0.1, stationAt.z), rotation: QUAT_IDENTITY, scale: v3(0.14, 1, 0.5) } },
    { key: "button:rim", material: "ButtonRim", mesh: "cylinder", transform: { position: v3(BELT_CENTER.x + 0.3, BELT_CENTER.y - BELT_RY - 0.75, BELT_CENTER.z + 0.5), rotation: quatPitch(Math.PI / 2), scale: v3(0.6, 0.14, 0.6) } },
    {
      key: "button:cap",
      material: "Button",
      mesh: "cylinder",
      transform: {
        position: v3(BELT_CENTER.x + 0.3, BELT_CENTER.y - BELT_RY - 0.75 - pressT * 0.05 + (running ? Math.sin(tick * 0.1) * 0.01 : 0), BELT_CENTER.z + 0.58),
        rotation: quatPitch(Math.PI / 2),
        scale: v3(0.44, 0.16, 0.44),
      },
    },
  ];

  // Reward / empty reveal above the opened capsule.
  const rewardInstances: SceneInstance[] = [];
  if (plan !== null && dest >= 0 && revealAge >= timeline.openEnd) {
    const at = addV3(capsuleWorldPosition(spec, state, reduced, dest), v3(0, 0.35, 0));
    const riseT = clamp01((revealAge - timeline.openEnd) / (timeline.riseEnd - timeline.openEnd));
    const rarity = outcomeRarity(session);
    if (plan.win && rarity !== "loss") {
      rewardInstances.push(...rewardProp("reward", rarity, at, riseT, tick, 0.8));
    } else {
      rewardInstances.push({
        key: "empty:token",
        material: "TryAgain",
        mesh: "sphere",
        transform: {
          position: v3(at.x, at.y - 0.15 + easeOutCubic(riseT) * 0.1, at.z),
          rotation: quatYaw(tick * 0.05),
          scale: v3(0.14 + Math.sin(tick * 0.2) * 0.01, 0.14, 0.14),
        },
      });
    }
  }

  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(stationAt, v3(0, 0.7, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Glass at the view edges (front-right + a left corner streak), clear of the belt.
  const glass: SceneInstance[] = [
    ...glassPane("glass:front", v3(MACHINE_VOLUME.center.x + 1.9, BELT_CENTER.y, MACHINE_VOLUME.center.z + MACHINE_VOLUME.size.z / 2 - 0.1), 1.1, 2.2, 1),
    ...glassPane("glass:corner", v3(MACHINE_VOLUME.center.x - MACHINE_VOLUME.size.x / 2 + 0.25, MACHINE_VOLUME.center.y + 0.6, MACHINE_VOLUME.center.z + 1.2), 0.6, 1.4, 1),
  ];

  // Camera: stable interior mount; subtle pull toward the station during the open.
  const base = conveyorCamera();
  const focusT = revealAge >= timeline.brakeEnd ? clamp01((revealAge - timeline.brakeEnd) / (timeline.openEnd - timeline.brakeEnd)) : 0;
  const camera = focusT > 0 ? revealFocusCamera(base, addV3(stationAt, v3(0, 0.2, 0)), focusT, reduced ? 0.12 : 0.24) : base;

  const lights: SceneLight[] = [...stageLights(stationAt, 0.7)];
  if (revealAge >= timeline.armEnd && dest >= 0) {
    const rarity = outcomeRarity(session);
    const color = plan !== null && plan.win && rarity !== "loss" ? RARITY_COLORS[rarity] : ([1, 0.85, 0.55, 1] as const);
    lights.push({ key: "light:station", light: { color, intensity: 1.5, kind: "point", position: addV3(stationAt, v3(0, 0.6, 0.4)) } });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [
      ...machineHousing("housing", MACHINE_VOLUME.center, MACHINE_VOLUME.size),
      { key: "belt:body", material: "Belt", mesh: "box", transform: { position: v3(BELT_CENTER.x, BELT_CENTER.y, BELT_CENTER.z - 0.4), rotation: QUAT_IDENTITY, scale: v3(BELT_RX * 2 + 0.6, BELT_RY * 2 + 0.6, 0.12) } },
      ...slats,
      ...rollers,
      ...lamps,
      ...capsules,
      ...controls,
      ...rewardInstances,
      ...celebration,
      ...glass,
    ],
    lights,
  };
};
