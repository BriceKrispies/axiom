/*
 * scene.ts — Claw Grab presentation, seen from INSIDE the cabinet. The
 * machine-interior camera mounts near the upper-left corner; the housing frames
 * the view; a bed of half-buried plush prizes (stacked spheres/boxes, distinct
 * pastel colors by index) sits below an overhead gantry carrying a three-finger
 * claw on a thin cable, with a target ring on the bed tracking the claw. The
 * front-left chute swallows a won prize. Glass frames the view edges. Pure view.
 */

import type { EngineVec3, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { GLASS_MATERIALS, glassPane } from "../../presentation/glass/panels.ts";
import { REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack } from "../../presentation/stage/easing.ts";
import { machineHousing, SKY_CLEAR, STAGE_MATERIALS, stageLights } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { ClawGrabSpec, ClawState } from "./game.ts";
import {
  BED_Y,
  CHUTE_AT,
  CLAW_X_LIMIT,
  CLAW_Z_LIMIT,
  clawCamera,
  clawTimeline,
  clawTip,
  fingerCloseOf,
  focusIndexOf,
  GANTRY_Y,
  MACHINE_VOLUME,
  PRIZE_RADIUS,
  prizePosition,
  prizeWorldPosition,
  targetInReach,
  targetedPrizeIndexOf,
} from "./game.ts";

// ── plush palette + prize forms (distinct shape + color by index) ────────────────

const PLUSH_COLORS: readonly string[] = [
  "PlushCoral",
  "PlushMint",
  "PlushSky",
  "PlushLemon",
  "PlushLilac",
  "PlushRose",
  "PlushPeach",
  "PlushAqua",
];

const plushMaterials = (): Readonly<Record<string, MaterialSpec>> => ({
  PlushAqua: { baseColor: [0.5, 0.92, 0.9, 1], emissive: [0.08, 0.18, 0.17, 1] },
  PlushCoral: { baseColor: [1, 0.58, 0.52, 1], emissive: [0.2, 0.08, 0.07, 1] },
  PlushLemon: { baseColor: [1, 0.9, 0.5, 1], emissive: [0.2, 0.17, 0.06, 1] },
  PlushLilac: { baseColor: [0.78, 0.66, 1, 1], emissive: [0.14, 0.1, 0.22, 1] },
  PlushMint: { baseColor: [0.62, 0.98, 0.76, 1], emissive: [0.09, 0.2, 0.12, 1] },
  PlushPeach: { baseColor: [1, 0.76, 0.6, 1], emissive: [0.2, 0.12, 0.08, 1] },
  PlushRose: { baseColor: [1, 0.68, 0.82, 1], emissive: [0.2, 0.1, 0.14, 1] },
  PlushSky: { baseColor: [0.6, 0.82, 1, 1], emissive: [0.09, 0.15, 0.22, 1] },
});

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  ...GLASS_MATERIALS,
  ...plushMaterials(),
  Bed: { baseColor: [0.55, 0.78, 0.72, 1], emissive: [0.06, 0.1, 0.09, 1] },
  Cable: { baseColor: [0.3, 0.32, 0.36, 1] },
  ClawMetal: { baseColor: [0.82, 0.84, 0.9, 1], emissive: [0.12, 0.13, 0.16, 1] },
  ClawPlate: { baseColor: [1, 0.8, 0.32, 1], emissive: [0.3, 0.2, 0.05, 1] },
  ChuteWall: { baseColor: [0.72, 0.5, 0.9, 1], emissive: [0.14, 0.09, 0.2, 1] },
  RingHot: { baseColor: [1, 0.85, 0.4, 1], emissive: [0.9, 0.7, 0.25, 1], opacity: 0.7 },
  RingIdle: { baseColor: [0.7, 0.85, 1, 1], emissive: [0.3, 0.4, 0.6, 1], opacity: 0.4 },
};

export const CLAW_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

/** The stacked-body instances of one plush prize at `at` (2–3 shapes by index). */
const plushInstances = (key: string, index: number, at: EngineVec3, dim: boolean): readonly SceneInstance[] => {
  const mat = dim ? "Bed" : (PLUSH_COLORS[index % PLUSH_COLORS.length] as string);
  const kind = index % 3;
  const body: SceneInstance = {
    key: `${key}:body`,
    material: mat,
    mesh: kind === 1 ? "box" : "sphere",
    transform: { position: v3(at.x, at.y, at.z), rotation: quatYaw(index), scale: v3(0.42, 0.36, 0.42) },
  };
  const head: SceneInstance = {
    key: `${key}:head`,
    material: mat,
    mesh: "sphere",
    transform: { position: v3(at.x, at.y + 0.32, at.z), rotation: QUAT_IDENTITY, scale: v3(0.28, 0.28, 0.28) },
  };
  const ear: SceneInstance = {
    key: `${key}:ear`,
    material: mat,
    mesh: kind === 2 ? "box" : "sphere",
    transform: { position: v3(at.x + 0.16, at.y + 0.46, at.z), rotation: QUAT_IDENTITY, scale: v3(0.12, 0.16, 0.12) },
  };
  return kind === 0 ? [body, head] : [body, head, ear];
};

/** The three-finger claw at `tip`, opened by `1 - close`. */
const clawInstances = (tip: EngineVec3, close: number): readonly SceneInstance[] => {
  const cableTop = GANTRY_Y;
  const cableLen = Math.max(0.05, cableTop - (tip.y + 0.3));
  const cable: SceneInstance = {
    key: "claw:cable",
    material: "Cable",
    mesh: "cylinder",
    transform: { position: v3(tip.x, tip.y + 0.3 + cableLen / 2, tip.z), rotation: QUAT_IDENTITY, scale: v3(0.05, cableLen, 0.05) },
  };
  const hub: SceneInstance = {
    key: "claw:hub",
    material: "ClawPlate",
    mesh: "cylinder",
    transform: { position: v3(tip.x, tip.y + 0.28, tip.z), rotation: quatPitch(Math.PI / 2), scale: v3(0.34, 0.14, 0.34) },
  };
  const spread = 0.24 - close * 0.16;
  const fingers = Array.from({ length: 3 }, (_, i) => {
    const a = (i / 3) * Math.PI * 2;
    const fx = Math.cos(a) * spread;
    const fz = Math.sin(a) * spread;
    return {
      key: `claw:finger${i}`,
      material: "ClawMetal",
      mesh: "box",
      transform: { position: v3(tip.x + fx, tip.y + 0.02, tip.z + fz), rotation: quatYaw(a), scale: v3(0.09, 0.34, 0.14) },
    };
  });
  return [cable, hub, ...fingers];
};

export const clawScene = (runtime: GameRuntime<ClawGrabSpec>, state: ClawState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const reduced = runtime.settings.reducedMotion;
  const tick = session.tick;
  const plan = session.committed;
  const steering = session.phase === "interacting";
  const tip = clawTip(spec, state, reduced);
  const close = fingerCloseOf(session, reduced);
  const target = targetedPrizeIndexOf(spec.prizeCount, state.extra.clawX, state.extra.clawZ);
  const inReach = targetInReach(spec.prizeCount, state.extra.clawX, state.extra.clawZ);
  const focus = plan === null ? -1 : focusIndexOf(session);

  // The prize bed.
  const bed: SceneInstance = {
    key: "bed",
    material: "Bed",
    mesh: "box",
    transform: { position: v3(0, BED_Y - 0.28, 0), rotation: QUAT_IDENTITY, scale: v3(CLAW_X_LIMIT * 2 + 0.8, 0.4, CLAW_Z_LIMIT * 2 + 0.8) },
  };

  const prizes: SceneInstance[] = Array.from({ length: spec.prizeCount }, (_, index) => {
    const at = prizeWorldPosition(spec, state, reduced, index);
    return plushInstances(`prize${index}`, index, at, false);
  }).flat();

  // Target ring under the claw (tracks x/z), hot when a prize is in reach.
  const ring: SceneInstance[] = steering
    ? [
        {
          key: "ring",
          material: inReach ? "RingHot" : "RingIdle",
          mesh: "cylinder",
          transform: {
            position: v3(state.extra.clawX, BED_Y - 0.05, state.extra.clawZ),
            rotation: QUAT_IDENTITY,
            scale: v3(PRIZE_RADIUS * 2, 0.02, PRIZE_RADIUS * 2),
          },
        },
      ]
    : [];

  // The gantry rail + the claw.
  const gantry: SceneInstance[] = [
    {
      key: "gantry:rail",
      material: "ClawPlate",
      mesh: "box",
      transform: { position: v3(tip.x, GANTRY_Y + 0.1, 0), rotation: QUAT_IDENTITY, scale: v3(0.2, 0.16, CLAW_Z_LIMIT * 2 + 0.6) },
    },
    {
      key: "gantry:beam",
      material: "ClawMetal",
      mesh: "box",
      transform: { position: v3(0, GANTRY_Y + 0.24, tip.z), rotation: QUAT_IDENTITY, scale: v3(CLAW_X_LIMIT * 2 + 0.6, 0.14, 0.2) },
    },
  ];

  // The front-left chute.
  const chute: SceneInstance[] = [
    {
      key: "chute:wall",
      material: "ChuteWall",
      mesh: "box",
      transform: { position: v3(CHUTE_AT.x, CHUTE_AT.y, CHUTE_AT.z), rotation: QUAT_IDENTITY, scale: v3(0.9, 0.9, 0.9) },
    },
    {
      key: "chute:lip",
      material: "ClawPlate",
      mesh: "box",
      transform: { position: v3(CHUTE_AT.x, CHUTE_AT.y + 0.46, CHUTE_AT.z), rotation: QUAT_IDENTITY, scale: v3(0.98, 0.06, 0.98) },
    },
  ];

  // Reward + celebration after a won prize drops into the chute.
  const timeline = clawTimeline(session.config.presentationSpeed, reduced);
  const settled = session.phase === "celebrating" || session.phase === "complete";
  const revealAge = session.phase === "revealing" ? phaseAge(session) : settled ? timeline.total : -1;
  const rewardInstances: SceneInstance[] = [];
  if (plan !== null && plan.win && revealAge >= timeline.releaseEnd) {
    const rarity = outcomeRarity(session);
    const riseT = clamp01((revealAge - timeline.releaseEnd) / Math.max(1, timeline.total - timeline.releaseEnd));
    if (rarity !== "loss") {
      rewardInstances.push(...rewardProp("reward", rarity, addV3(CHUTE_AT, v3(0, 0.6, 0)), riseT, tick, 0.85));
    }
  }
  // Loss token: a soft wobble token near the focus prize.
  if (plan !== null && !plan.win && revealAge >= timeline.liftEnd && focus >= 0) {
    const at = addV3(prizePosition(focus, spec.prizeCount), v3(0, 0.55, 0));
    const wob = easeOutBack(clamp01((revealAge - timeline.liftEnd) / 20));
    rewardInstances.push({
      key: "loss:token",
      material: "TryAgain",
      mesh: "box",
      transform: { position: v3(at.x, at.y + wob * 0.1, at.z), rotation: quatYaw(tick * 0.04), scale: v3(0.16, 0.16, 0.05) },
    });
  }

  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = plan.win ? addV3(CHUTE_AT, v3(0, 0.9, 0)) : addV3(prizePosition(Math.max(0, focus), spec.prizeCount), v3(0, 0.7, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Glass at the view edges — front-right pane + a corner streak pane — clear of the bed.
  const glass: SceneInstance[] = [
    ...glassPane("glass:front", v3(MACHINE_VOLUME.center.x + 1.7, 1.4, MACHINE_VOLUME.center.z + MACHINE_VOLUME.size.z / 2 - 0.1), 1.3, 2.3, 1),
    ...glassPane("glass:corner", v3(MACHINE_VOLUME.center.x - MACHINE_VOLUME.size.x / 2 + 0.25, MACHINE_VOLUME.center.y + 0.7, MACHINE_VOLUME.center.z + 1.3), 0.6, 1.4, 1),
  ];

  const lights: SceneLight[] = [...stageLights(v3(tip.x, BED_Y + 0.4, tip.z), 0.7)];

  return {
    camera: clawCamera(),
    clearColor: SKY_CLEAR,
    instances: [
      ...machineHousing("housing", MACHINE_VOLUME.center, MACHINE_VOLUME.size),
      bed,
      ...prizes,
      ...ring,
      ...gantry,
      ...clawInstances(tip, close),
      ...chute,
      ...rewardInstances,
      ...celebration,
      ...glass,
    ],
    lights,
  };
};
