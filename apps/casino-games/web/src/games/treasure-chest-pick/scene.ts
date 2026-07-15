/*
 * scene.ts — Treasure Chest Pick presentation: nine carved-wood, gold-gilded
 * chests with hinged lids and falling latches, the idle dance, the focused
 * reveal (latch → pause → lid pop → warm light → reward), and the tier-scaled
 * celebration. Pure view: `chestScene(state, runtime)` returns a Scene value.
 */

import type { MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineVec3, GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import { clamp01, easeOutBack, easeOutCubic, pulse } from "../../presentation/stage/easing.ts";
import { contactShadow, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  hingedTransform,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatYaw,
  rotateByQuat,
  scaleV3,
  v3,
} from "../../presentation/stage/vectors.ts";
import type { ChestSpec, ChestState } from "./game.ts";
import { chestCamera, chestPosition, dancePose, revealTimeline } from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  ChestInterior: { baseColor: [0.16, 0.1, 0.06, 1] },
  Gild: { baseColor: [1, 0.8, 0.32, 1], emissive: [0.22, 0.15, 0.03, 1] },
  GildBright: { baseColor: [1, 0.88, 0.42, 1], emissive: [0.55, 0.4, 0.1, 1] },
  InnerGlow: { baseColor: [1, 0.85, 0.5, 1], emissive: [1, 0.78, 0.38, 1], opacity: 0.75 },
  WoodBrown: { baseColor: [0.58, 0.37, 0.2, 1] },
  WoodDark: { baseColor: [0.42, 0.26, 0.13, 1] },
  WoodDim: { baseColor: [0.34, 0.24, 0.16, 1] },
};

export const CHEST_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── chest proportions ───────────────────────────────────────────────────────────

const BODY = v3(1.3, 0.6, 0.92);
const LID = v3(1.34, 0.26, 0.96);
const LATCH = v3(0.2, 0.24, 0.05);

interface ChestPose {
  readonly origin: EngineVec3;
  readonly yaw: number;
  readonly squash: number;
  readonly lift: number;
  readonly lidAngle: number;
  readonly latchAngle: number;
  readonly dim: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
  readonly glow: number;
}

/** All instances of one posed chest (body, planks, gilding, latch, lid, shadow). */
const chestInstances = (key: string, pose: ChestPose): readonly SceneInstance[] => {
  const q = quatYaw(pose.yaw);
  const squashY = 1 - pose.squash;
  const squashXZ = 1 + pose.squash * 0.55;
  const origin = v3(pose.origin.x, pose.origin.y + pose.lift, pose.origin.z);
  const wood = pose.dim ? "WoodDim" : "WoodBrown";
  const gild = pose.hoverRing ? "GildBright" : "Gild";

  const part = (
    suffix: string,
    local: EngineVec3,
    scale: EngineVec3,
    material: string,
    extraQ = QUAT_IDENTITY,
  ): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh: "box",
    transform: {
      position: addV3(origin, rotateByQuat(v3(local.x * squashXZ, local.y * squashY, local.z * squashXZ), q)),
      rotation: quatMul(q, extraQ),
      scale: v3(scale.x * squashXZ, scale.y * squashY, scale.z * squashXZ),
    },
  });

  // Lid on its back hinge; latch hangs from the lid's front lip.
  const lidQ = quatMul(q, quatPitch(pose.lidAngle));
  const lidHingeLocal = v3(0, BODY.y, -BODY.z / 2);
  const lidHinge = addV3(origin, rotateByQuat(v3(lidHingeLocal.x, lidHingeLocal.y * squashY, lidHingeLocal.z * squashXZ), q));
  const lid: SceneInstance = {
    key: `${key}:lid`,
    material: wood,
    mesh: "box",
    transform: hingedTransform(lidHinge, v3(0, LID.y / 2, LID.z / 2), lidQ, LID),
  };
  const lidRim: SceneInstance = {
    key: `${key}:lidrim`,
    material: gild,
    mesh: "box",
    transform: hingedTransform(lidHinge, v3(0, LID.y / 2, LID.z - 0.02), lidQ, v3(LID.x + 0.02, LID.y + 0.03, 0.05)),
  };
  const latchQ = quatMul(lidQ, quatPitch(pose.latchAngle));
  const latchHinge = addV3(lidHinge, rotateByQuat(v3(0, 0.02, LID.z - 0.01), lidQ));
  const latch: SceneInstance = {
    key: `${key}:latch`,
    material: gild,
    mesh: "box",
    transform: hingedTransform(latchHinge, v3(0, -LATCH.y / 2, LATCH.z / 2), latchQ, LATCH),
  };

  const interior: SceneInstance = part("interior", v3(0, BODY.y - 0.03, 0), v3(BODY.x - 0.1, 0.05, BODY.z - 0.1), "ChestInterior");
  const glow: SceneInstance[] =
    pose.glow > 0
      ? [part("glow", v3(0, BODY.y + 0.1 + pose.glow * 0.15, 0), scaleV3(v3(BODY.x - 0.15, 0.3, BODY.z - 0.15), pose.glow), "InnerGlow")]
      : [];

  const rings: SceneInstance[] = [];
  if (pose.focusRing || pose.hoverRing) {
    rings.push({
      key: `${key}:ring`,
      material: pose.hoverRing ? "GildBright" : "Gild",
      mesh: "cylinder",
      transform: {
        position: v3(pose.origin.x, 0.02, pose.origin.z),
        rotation: QUAT_IDENTITY,
        scale: v3(BODY.x * 1.45, 0.02, BODY.x * 1.45),
      },
    });
  }

  return [
    part("body", v3(0, BODY.y / 2, 0), BODY, wood),
    part("plank1", v3(0, BODY.y * 0.36, 0), v3(BODY.x + 0.015, 0.03, BODY.z + 0.015), "WoodDark"),
    part("plank2", v3(0, BODY.y * 0.68, 0), v3(BODY.x + 0.015, 0.03, BODY.z + 0.015), "WoodDark"),
    part("strapL", v3(-BODY.x * 0.28, BODY.y / 2, 0), v3(0.07, BODY.y + 0.02, BODY.z + 0.03), gild),
    part("strapR", v3(BODY.x * 0.28, BODY.y / 2, 0), v3(0.07, BODY.y + 0.02, BODY.z + 0.03), gild),
    part("edgeL", v3(-BODY.x / 2, BODY.y / 2, BODY.z / 2), v3(0.05, BODY.y + 0.02, 0.05), gild),
    part("edgeR", v3(BODY.x / 2, BODY.y / 2, BODY.z / 2), v3(0.05, BODY.y + 0.02, 0.05), gild),
    interior,
    ...glow,
    lid,
    lidRim,
    latch,
    contactShadow(`${key}:shadow`, pose.origin, BODY.x * 0.72),
    ...rings,
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const chestScene = (runtime: GameRuntime<ChestSpec>, state: ChestState): Scene => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;
  const seed = session.seed;
  const tick = session.tick;
  const spec = runtime.config.gameSpecific;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const timeline = revealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge = session.phase === "revealing" ? phaseAge(session) : session.phase === "celebrating" || session.phase === "complete" ? timeline.total : -1;
  const liveliness = session.phase === "ready" || session.phase === "intro" ? spec.danceLiveliness : 0;

  const chests = Array.from({ length: count }, (_, index) => {
    const origin = chestPosition(index, count);
    const dance = dancePose(index, count, tick, seed, liveliness);
    const isSelected = selected === index;
    const beyondBrace = revealAge >= 0;

    // Anticipation brace: a tiny shiver before the latch moves.
    const bracing = isSelected && revealAge >= 0 && revealAge < timeline.braceEnd;
    const braceT = bracing ? revealAge / timeline.braceEnd : 0;
    const shiver = bracing ? Math.sin(revealAge * 1.4) * 0.02 * pulse(braceT) : 0;

    const latchT = isSelected ? clamp01((revealAge - timeline.latchStart) / (timeline.latchEnd - timeline.latchStart)) : 0;
    const lidT = isSelected ? clamp01((revealAge - timeline.pauseEnd) / (timeline.lidEnd - timeline.pauseEnd)) : 0;

    return chestInstances(`chest${index}`, {
      dim: beyondBrace && !isSelected,
      focusRing: session.phase === "ready" && state.extra.choice.focused === index,
      glow: isSelected ? easeOutCubic(lidT) : 0,
      hoverRing: session.phase === "ready" && state.extra.choice.hovered === index,
      latchAngle: easeOutCubic(latchT) * 1.55,
      lidAngle: -easeOutBack(lidT) * 1.85,
      lift: 0,
      origin,
      squash: dance.squash + (bracing ? pulse(braceT) * 0.05 : 0),
      yaw: dance.twist + shiver,
    });
  }).flat();

  // Reward / empty reveal above the selected, open chest.
  const rewardInstances: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.lidEnd) {
    const at = addV3(chestPosition(selected, count), v3(0, BODY_TOP, 0));
    const riseT = clamp01((revealAge - timeline.lidEnd) / (timeline.riseEnd - timeline.lidEnd));
    const rarity = outcomeRarity(session);
    if (plan.win && rarity !== "loss") {
      rewardInstances.push(...rewardProp(`reward`, rarity, at, riseT, tick));
    } else {
      rewardInstances.push(...sparkleRing("dust", at, 6, plan.presentationSeed, revealAge - timeline.lidEnd, 50));
    }
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(chestPosition(selected, count), v3(0, BODY_TOP + 0.4, 0));
    if (plan.win) {
      celebration.push(...confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    } else {
      celebration.push(...sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    }
  }

  // Camera: table framing, easing toward the selected chest during the reveal.
  const base = chestCamera(count);
  const focusT = revealAge >= 0 ? clamp01(revealAge / timeline.braceEnd) : 0;
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(base, addV3(chestPosition(selected, count), v3(0, 0.5, 0)), focusT, runtime.settings.reducedMotion ? 0.2 : 0.5)
      : base;

  // Warm escape light once the lid opens.
  const lights: SceneLight[] = [...stageLights(selected !== null ? chestPosition(selected, count) : v3(0, 0, 0), 0.5)];
  if (selected !== null && revealAge >= timeline.pauseEnd) {
    lights.push({
      key: "light:chest",
      light: {
        color: [1, 0.82, 0.45, 1],
        intensity: 1.4,
        kind: "point",
        position: addV3(chestPosition(selected, count), v3(0, 1.1, 0.3)),
      },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(16), ...chests, ...rewardInstances, ...celebration],
    lights,
  };
};

const BODY_TOP = 0.62;
