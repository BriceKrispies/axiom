/*
 * scene.ts — Ball Machine presentation, seen from INSIDE the machine. The
 * machine-interior camera mounts near the upper-left corner; the housing frames
 * the view; a clear globe (glass panes + a gilded rim) holds ~14 pastel capsule
 * balls resting in a bowl; a big button and a pickup chute sit to the lower
 * right. During the reveal the chamber agitates, one ball rolls down the chute,
 * and its capsule splits open to show the committed tier glow (win) or a small
 * try-again token (loss). Pure view: `ballScene(runtime, state)` returns a Scene.
 */

import type { MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { GLASS_MATERIALS, glassPane } from "../../presentation/glass/panels.ts";
import { REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, easeOutCubic, pulse } from "../../presentation/stage/easing.ts";
import { machineHousing, SKY_CLEAR, STAGE_MATERIALS, stageLights } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { BallMachineSpec, BallState } from "./game.ts";
import {
  BALL_DIAMETER,
  BOWL_CENTER,
  ballCamera,
  ballTimeline,
  ballWorldPosition,
  BUTTON_AT,
  DOOR_AT,
  dispensedIndexOf,
  GLOBE_CENTER,
  GLOBE_RADIUS,
  MACHINE_VOLUME,
} from "./game.ts";

// ── pastel ball palette (fixed by index) ────────────────────────────────────────

const BALL_COLORS: readonly string[] = [
  "BallCoral",
  "BallLemon",
  "BallMint",
  "BallSky",
  "BallLilac",
  "BallPeach",
  "BallRose",
];

const ballColorMaterials = (): Readonly<Record<string, MaterialSpec>> => ({
  BallCoral: { baseColor: [1, 0.6, 0.55, 1], emissive: [0.2, 0.08, 0.07, 1] },
  BallLemon: { baseColor: [1, 0.92, 0.55, 1], emissive: [0.2, 0.18, 0.07, 1] },
  BallLilac: { baseColor: [0.78, 0.68, 1, 1], emissive: [0.14, 0.1, 0.22, 1] },
  BallMint: { baseColor: [0.62, 0.98, 0.78, 1], emissive: [0.09, 0.2, 0.13, 1] },
  BallPeach: { baseColor: [1, 0.78, 0.62, 1], emissive: [0.2, 0.13, 0.08, 1] },
  BallRose: { baseColor: [1, 0.7, 0.85, 1], emissive: [0.2, 0.1, 0.15, 1] },
  BallSky: { baseColor: [0.62, 0.84, 1, 1], emissive: [0.09, 0.16, 0.22, 1] },
});

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  ...GLASS_MATERIALS,
  ...ballColorMaterials(),
  Bowl: { baseColor: [0.9, 0.55, 0.4, 1], emissive: [0.12, 0.06, 0.04, 1] },
  Button: { baseColor: [1, 0.35, 0.35, 1], emissive: [0.35, 0.06, 0.06, 1] },
  ButtonRim: { baseColor: [1, 0.82, 0.32, 1], emissive: [0.3, 0.2, 0.05, 1] },
  ChuteMetal: { baseColor: [0.72, 0.76, 0.82, 1], emissive: [0.1, 0.11, 0.13, 1] },
  GlobeRim: { baseColor: [1, 0.8, 0.32, 1], emissive: [0.3, 0.2, 0.05, 1] },
  Post: { baseColor: [0.85, 0.5, 0.42, 1] },
};

export const BALL_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

const ballMaterial = (index: number): string => BALL_COLORS[index % BALL_COLORS.length] as string;

// ── the scene ────────────────────────────────────────────────────────────────────

export const ballScene = (runtime: GameRuntime<BallMachineSpec>, state: BallState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const reduced = runtime.settings.reducedMotion;
  const tick = session.tick;
  const plan = session.committed;
  const timeline = ballTimeline(spec, session.config.presentationSpeed, reduced);
  const settled = session.phase === "celebrating" || session.phase === "complete";
  const revealAge = session.phase === "revealing" ? phaseAge(session) : settled ? timeline.total : -1;
  const dispensed = plan === null ? -1 : dispensedIndexOf(spec.ballCount, plan.presentationSeed);
  const opened = revealAge >= timeline.chuteEnd;
  const openT = opened ? clamp01((revealAge - timeline.chuteEnd) / (timeline.openEnd - timeline.chuteEnd)) : 0;

  // The balls (the dispensed one is hidden once its capsule split shows the result).
  const balls: SceneInstance[] = Array.from({ length: spec.ballCount }, (_, index) => {
    const at = ballWorldPosition(spec, session, reduced, index);
    const isDispensed = index === dispensed;
    const hide = isDispensed && openT > 0.05;
    return hide
      ? null
      : ({
          key: `ball${index}`,
          material: ballMaterial(index),
          mesh: "sphere",
          transform: { position: at, rotation: quatYaw(tick * 0.02 + index), scale: v3(BALL_DIAMETER, BALL_DIAMETER, BALL_DIAMETER) },
        } as SceneInstance);
  }).filter((instance): instance is SceneInstance => instance !== null);

  // The clear globe: gilded rim ring, top knob, and glass panes at the view edges.
  const globe: SceneInstance[] = [
    {
      key: "globe:rim",
      material: "GlobeRim",
      mesh: "cylinder",
      transform: { position: v3(GLOBE_CENTER.x, BOWL_CENTER.y + 0.06, GLOBE_CENTER.z), rotation: QUAT_IDENTITY, scale: v3(GLOBE_RADIUS * 2.1, 0.12, GLOBE_RADIUS * 2.1) },
    },
    {
      key: "globe:knob",
      material: "GlobeRim",
      mesh: "sphere",
      transform: { position: v3(GLOBE_CENTER.x, GLOBE_CENTER.y + GLOBE_RADIUS + 0.05, GLOBE_CENTER.z), rotation: QUAT_IDENTITY, scale: v3(0.24, 0.24, 0.24) },
    },
    {
      key: "bowl",
      material: "Bowl",
      mesh: "cylinder",
      transform: { position: v3(BOWL_CENTER.x, BOWL_CENTER.y - 0.14, BOWL_CENTER.z), rotation: QUAT_IDENTITY, scale: v3(GLOBE_RADIUS * 1.7, 0.28, GLOBE_RADIUS * 1.7) },
    },
    {
      key: "post",
      material: "Post",
      mesh: "box",
      transform: { position: v3(GLOBE_CENTER.x, 0.1, GLOBE_CENTER.z), rotation: QUAT_IDENTITY, scale: v3(0.9, 0.5, 0.9) },
    },
  ];

  // The button and pickup chute at the lower right.
  const pressT = session.phase === "committing" ? clamp01(phaseAge(session) / 8) : 0;
  const controls: SceneInstance[] = [
    {
      key: "button:rim",
      material: "ButtonRim",
      mesh: "cylinder",
      transform: { position: v3(BUTTON_AT.x, BUTTON_AT.y - 0.02, BUTTON_AT.z), rotation: quatPitch(Math.PI / 2), scale: v3(0.62, 0.14, 0.62) },
    },
    {
      key: "button:cap",
      material: "Button",
      mesh: "cylinder",
      transform: {
        position: v3(BUTTON_AT.x, BUTTON_AT.y - pressT * 0.06 + (session.phase === "ready" ? pulse((tick % 60) / 60) * 0.01 : 0), BUTTON_AT.z + 0.08),
        rotation: quatPitch(Math.PI / 2),
        scale: v3(0.46, 0.16, 0.46),
      },
    },
    {
      key: "chute:mouth",
      material: "ChuteMetal",
      mesh: "box",
      transform: { position: v3(DOOR_AT.x, DOOR_AT.y + 0.02, DOOR_AT.z), rotation: QUAT_IDENTITY, scale: v3(0.6, 0.5, 0.6) },
    },
    {
      key: "chute:lip",
      material: "GlobeRim",
      mesh: "box",
      transform: { position: v3(DOOR_AT.x, DOOR_AT.y - 0.28, DOOR_AT.z + 0.28), rotation: QUAT_IDENTITY, scale: v3(0.66, 0.06, 0.14) },
    },
  ];

  // The opened capsule at the door: two hemispheres split apart + interior reveal.
  const capsule: SceneInstance[] = [];
  const rewardInstances: SceneInstance[] = [];
  if (opened && plan !== null) {
    const at = addV3(DOOR_AT, v3(0, 0.02, 0.05));
    const spread = easeOutBack(openT) * 0.22;
    const dispMat = ballMaterial(dispensed);
    capsule.push(
      {
        key: "cap:top",
        material: dispMat,
        mesh: "sphere",
        transform: { position: v3(at.x, at.y + spread, at.z), rotation: QUAT_IDENTITY, scale: v3(BALL_DIAMETER, BALL_DIAMETER * 0.62, BALL_DIAMETER) },
      },
      {
        key: "cap:bottom",
        material: dispMat,
        mesh: "sphere",
        transform: { position: v3(at.x, at.y - spread, at.z), rotation: QUAT_IDENTITY, scale: v3(BALL_DIAMETER, BALL_DIAMETER * 0.62, BALL_DIAMETER) },
      },
    );
    const riseT = clamp01((revealAge - timeline.openEnd) / (timeline.riseEnd - timeline.openEnd));
    const rarity = outcomeRarity(session);
    const rewardAt = addV3(at, v3(0, 0.28, 0));
    if (plan.win && rarity !== "loss") {
      rewardInstances.push(...rewardProp("reward", rarity, rewardAt, riseT, tick, 0.85));
    } else {
      capsule.push({
        key: "cap:token",
        material: "TryAgain",
        mesh: "box",
        transform: {
          position: v3(rewardAt.x, rewardAt.y - 0.1 + easeOutCubic(riseT) * 0.12, rewardAt.z),
          rotation: quatYaw(tick * 0.04),
          scale: v3(0.16, 0.16, 0.05),
        },
      });
    }
  }

  // Celebration above the door.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(DOOR_AT, v3(0, 0.6, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Glass at the view edges (front-right pane + corner streaks), never over the bowl.
  const glass: SceneInstance[] = [
    ...glassPane("glass:front", v3(GLOBE_CENTER.x + 1.5, 1.5, GLOBE_CENTER.z + MACHINE_VOLUME.size.z / 2 - 0.1), 1.4, 2.4, 1),
    ...glassPane("glass:left", v3(MACHINE_VOLUME.center.x - MACHINE_VOLUME.size.x / 2 + 0.2, MACHINE_VOLUME.center.y + 0.5, GLOBE_CENTER.z + 1.2), 0.7, 1.5, 1),
  ];

  // Camera: stable interior mount; a subtle pull toward the door during the reveal.
  const base = ballCamera();
  const focusT = revealAge >= timeline.agitationEnd ? clamp01((revealAge - timeline.agitationEnd) / (timeline.chuteEnd - timeline.agitationEnd)) : 0;
  const camera = focusT > 0 ? revealFocusCamera(base, addV3(DOOR_AT, v3(0, 0.2, 0)), focusT, reduced ? 0.12 : 0.24) : base;

  const lights: SceneLight[] = [...stageLights(GLOBE_CENTER, 0.7)];
  if (opened) {
    lights.push({
      key: "light:door",
      light: { color: [1, 0.85, 0.5, 1], intensity: 1.5, kind: "point", position: addV3(DOOR_AT, v3(0, 0.5, 0.4)) },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [
      ...machineHousing("housing", MACHINE_VOLUME.center, MACHINE_VOLUME.size),
      ...globe,
      ...balls,
      ...controls,
      ...capsule,
      ...rewardInstances,
      ...celebration,
      ...glass,
    ],
    lights,
  };
};
