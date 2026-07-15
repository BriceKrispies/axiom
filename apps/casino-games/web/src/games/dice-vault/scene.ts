/*
 * scene.ts — Dice Vault presentation: a felt table pad with 1–3 chunky toy
 * dice (box body + white pip spheres laid out per face), and behind them a
 * cheerful pastel vault with a big spoked wheel handle and gold trim. The
 * tumble follows the analytic timeline in game.ts; on a win the wheel spins,
 * the door swings on its hinge, and golden light + the reward prop spill out;
 * on a loss the vault gives a friendly wobble. Pure view.
 */

import type { EngineQuat, EngineVec3, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import { revealFocusCamera, tabletopCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, easeOutCubic } from "../../presentation/stage/easing.ts";
import { contactShadow, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  hingedTransform,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatRoll,
  quatYaw,
  rotateByQuat,
  v3,
} from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { DiceSpec, DiceState } from "./game.ts";
import {
  DICE_Z,
  DIE_SIZE,
  dieHeight,
  diePosition,
  dieRotationAt,
  dieSquash,
  diceTimeline,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  DieCoral: { baseColor: [1, 0.52, 0.45, 1] },
  DieMint: { baseColor: [0.5, 0.9, 0.72, 1] },
  DieSky: { baseColor: [0.5, 0.76, 1, 1] },
  Felt: { baseColor: [0.36, 0.72, 0.62, 1] },
  PipWhite: { baseColor: [0.98, 0.98, 0.95, 1], emissive: [0.1, 0.1, 0.1, 1] },
  VaultBody: { baseColor: [0.72, 0.85, 0.96, 1] },
  VaultDark: { baseColor: [0.5, 0.62, 0.78, 1] },
  VaultDoor: { baseColor: [0.85, 0.78, 0.97, 1] },
  VaultGlow: { baseColor: [1, 0.88, 0.55, 1], emissive: [1, 0.8, 0.4, 1], opacity: 0.8 },
  VaultInterior: { baseColor: [0.28, 0.24, 0.36, 1] },
};

export const DICE_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── one posed die (body + pips per face) ────────────────────────────────────────

const DIE_MATERIALS = ["DieCoral", "DieMint", "DieSky"];

/** Pip grid coordinates per face value (3×3 grid positions in {−1,0,1}²). */
const PIP_LAYOUTS: readonly (readonly (readonly [number, number])[])[] = [
  [[0, 0]],
  [[-1, -1], [1, 1]],
  [[-1, -1], [0, 0], [1, 1]],
  [[-1, -1], [-1, 1], [1, -1], [1, 1]],
  [[-1, -1], [-1, 1], [0, 0], [1, -1], [1, 1]],
  [[-1, -1], [-1, 0], [-1, 1], [1, -1], [1, 0], [1, 1]],
];

/** Local frames per face, matching DIE_FACE_NORMALS in game.ts. */
const FACE_FRAMES: readonly { readonly n: EngineVec3; readonly t1: EngineVec3; readonly t2: EngineVec3; readonly value: number }[] = [
  { n: v3(0, 1, 0), t1: v3(1, 0, 0), t2: v3(0, 0, 1), value: 1 },
  { n: v3(0, -1, 0), t1: v3(1, 0, 0), t2: v3(0, 0, -1), value: 6 },
  { n: v3(1, 0, 0), t1: v3(0, 0, -1), t2: v3(0, 1, 0), value: 2 },
  { n: v3(-1, 0, 0), t1: v3(0, 0, 1), t2: v3(0, 1, 0), value: 5 },
  { n: v3(0, 0, 1), t1: v3(1, 0, 0), t2: v3(0, 1, 0), value: 3 },
  { n: v3(0, 0, -1), t1: v3(-1, 0, 0), t2: v3(0, 1, 0), value: 4 },
];

const dieInstances = (index: number, center: EngineVec3, rotation: EngineQuat, squash: number): readonly SceneInstance[] => {
  const material = DIE_MATERIALS[index % DIE_MATERIALS.length] as string;
  const body: SceneInstance = {
    key: `die${index}:body`,
    material,
    mesh: "box",
    transform: {
      position: center,
      rotation,
      scale: v3(DIE_SIZE * (1 + squash * 0.5), DIE_SIZE * (1 - squash), DIE_SIZE * (1 + squash * 0.5)),
    },
  };
  const pips: SceneInstance[] = [];
  const half = DIE_SIZE / 2;
  const spread = DIE_SIZE * 0.27;
  for (const face of FACE_FRAMES) {
    const layout = PIP_LAYOUTS[face.value - 1] as readonly (readonly [number, number])[];
    layout.forEach(([g1, g2], pip) => {
      const local = v3(
        face.n.x * (half + 0.012) + face.t1.x * g1 * spread + face.t2.x * g2 * spread,
        face.n.y * (half + 0.012) + face.t1.y * g1 * spread + face.t2.y * g2 * spread,
        face.n.z * (half + 0.012) + face.t1.z * g1 * spread + face.t2.z * g2 * spread,
      );
      pips.push({
        key: `die${index}:f${face.value}p${pip}`,
        material: "PipWhite",
        mesh: "sphere",
        transform: {
          position: addV3(center, rotateByQuat(local, rotation)),
          rotation,
          scale: v3(DIE_SIZE * 0.17, DIE_SIZE * 0.17, DIE_SIZE * 0.17),
        },
      });
    });
  }
  return [body, ...pips];
};

// ── the vault ───────────────────────────────────────────────────────────────────

const VAULT_CENTER = v3(0, 1.0, -1.9);
const VAULT_SIZE = v3(2.4, 1.9, 1.1);
const DOOR_SIZE = v3(1.9, 1.5, 0.16);

interface VaultPose {
  /** Body yaw wobble (loss response). */
  readonly wobble: number;
  /** Door swing angle in radians (0 = closed). */
  readonly doorAngle: number;
  /** Wheel handle spin angle. */
  readonly wheelSpin: number;
  /** Interior glow strength [0, 1]. */
  readonly glow: number;
}

const vaultInstances = (pose: VaultPose): readonly SceneInstance[] => {
  const bodyQ = quatYaw(pose.wobble);
  const part = (key: string, local: EngineVec3, scale: EngineVec3, material: string): SceneInstance => ({
    key: `vault:${key}`,
    material,
    mesh: "box",
    transform: { position: addV3(VAULT_CENTER, rotateByQuat(local, bodyQ)), rotation: bodyQ, scale },
  });

  // Door on its left hinge, swinging toward the camera.
  const doorFace = VAULT_SIZE.z / 2;
  const hinge = addV3(VAULT_CENTER, rotateByQuat(v3(-DOOR_SIZE.x / 2, 0, doorFace), bodyQ));
  const doorQ = quatMul(bodyQ, quatYaw(pose.doorAngle));
  const doorOffset = v3(DOOR_SIZE.x / 2, 0, DOOR_SIZE.z / 2);
  const door: SceneInstance = {
    key: "vault:door",
    material: "VaultDoor",
    mesh: "box",
    transform: hingedTransform(hinge, doorOffset, doorQ, DOOR_SIZE),
  };
  const doorTrim: SceneInstance = {
    key: "vault:doortrim",
    material: "StageGold",
    mesh: "box",
    transform: hingedTransform(hinge, v3(DOOR_SIZE.x / 2, 0, DOOR_SIZE.z + 0.005), doorQ, v3(DOOR_SIZE.x - 0.18, DOOR_SIZE.y - 0.18, 0.03)),
  };
  // Wheel handle mounted on the door: rim + hub + four spinning spokes.
  const wheelLocal = v3(DOOR_SIZE.x / 2, 0, DOOR_SIZE.z + 0.12);
  const wheelPos = addV3(hinge, rotateByQuat(wheelLocal, doorQ));
  const wheel: SceneInstance[] = [
    {
      key: "vault:wheelrim",
      material: "StageGold",
      mesh: "cylinder",
      transform: { position: wheelPos, rotation: quatMul(doorQ, quatPitch(Math.PI / 2)), scale: v3(0.62, 0.09, 0.62) },
    },
    {
      key: "vault:wheelhub",
      material: "VaultDark",
      mesh: "sphere",
      transform: { position: wheelPos, rotation: doorQ, scale: v3(0.16, 0.16, 0.16) },
    },
    ...Array.from({ length: 4 }, (_, k): SceneInstance => ({
      key: `vault:spoke${k}`,
      material: "StageGold",
      mesh: "box",
      transform: {
        position: wheelPos,
        rotation: quatMul(doorQ, quatRoll(pose.wheelSpin + (k * Math.PI) / 2)),
        scale: v3(0.56, 0.06, 0.06),
      },
    })),
  ];

  const glow: SceneInstance[] =
    pose.glow > 0
      ? [
          part("glow", v3(0, 0, doorFace - 0.2), v3(DOOR_SIZE.x * 0.9 * pose.glow, DOOR_SIZE.y * 0.9 * pose.glow, 0.1), "VaultGlow"),
          part("shelf", v3(0, -0.35, doorFace - 0.35), v3(DOOR_SIZE.x * 0.8, 0.08, 0.5), "StageGold"),
        ]
      : [];

  return [
    part("body", v3(0, 0, 0), VAULT_SIZE, "VaultBody"),
    part("interior", v3(0, 0, doorFace - 0.28), v3(DOOR_SIZE.x - 0.1, DOOR_SIZE.y - 0.1, 0.4), "VaultInterior"),
    part("trim-top", v3(0, VAULT_SIZE.y / 2 + 0.04, 0), v3(VAULT_SIZE.x + 0.12, 0.1, VAULT_SIZE.z + 0.12), "StageGold"),
    part("trim-bottom", v3(0, -VAULT_SIZE.y / 2 - 0.04, 0), v3(VAULT_SIZE.x + 0.16, 0.12, VAULT_SIZE.z + 0.16), "VaultDark"),
    part("foot-l", v3(-VAULT_SIZE.x * 0.38, -VAULT_SIZE.y / 2 - 0.14, 0), v3(0.24, 0.12, VAULT_SIZE.z * 0.9), "VaultDark"),
    part("foot-r", v3(VAULT_SIZE.x * 0.38, -VAULT_SIZE.y / 2 - 0.14, 0), v3(0.24, 0.12, VAULT_SIZE.z * 0.9), "VaultDark"),
    ...glow,
    door,
    doorTrim,
    ...wheel,
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const diceScene = (runtime: GameRuntime<DiceSpec>, state: DiceState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const count = spec.diceCount;
  const tick = session.tick;
  const plan = session.committed;
  const timeline = diceTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? timeline.pauseEnd + timeline.vaultTicks
        : -1;
  const seed = plan?.presentationSeed ?? session.seed;
  const combination = plan !== null && plan.manifestation.kind === "combination" ? plan.manifestation.combination : null;

  // Dice: idle jiggle at rest, or the committed tumble.
  const dice: SceneInstance[] = [];
  const shadows: SceneInstance[] = [];
  for (let index = 0; index < count; index += 1) {
    const rest = diePosition(index, count);
    let center = rest;
    let rotation: EngineQuat;
    let squash = 0;
    if (revealAge >= 0 && combination !== null) {
      const height = dieHeight(revealAge, timeline, seed, index);
      squash = dieSquash(revealAge, timeline);
      center = v3(rest.x, rest.y + height, rest.z);
      rotation = dieRotationAt(revealAge, timeline, seed, index, combination[index] ?? 0);
    } else {
      // Micro ambient jiggle while waiting (AMBIENT stream phase, tick clock).
      const jigglePhase = sample01(session.seed, "ambient", index, 0) * Math.PI * 2;
      const jiggle = session.phase === "ready" ? Math.sin(tick * 0.07 + jigglePhase) * 0.02 : 0;
      rotation = quatYaw(jiggle + (sample01(session.seed, "ambient", index, 1) - 0.5) * 0.5);
    }
    dice.push(...dieInstances(index, center, rotation, squash));
    shadows.push(contactShadow(`die${index}:shadow`, rest, DIE_SIZE * 0.72));
  }

  // Vault reaction after the settle pause.
  const win = plan?.win ?? false;
  const reactAge = revealAge >= 0 ? revealAge - timeline.pauseEnd : -1;
  const wheelT = win && reactAge >= 0 ? clamp01(reactAge / (timeline.vaultTicks * 0.3)) : 0;
  const doorT = win && reactAge >= 0 ? clamp01((reactAge - timeline.vaultTicks * 0.2) / (timeline.vaultTicks * 0.6)) : 0;
  const wobble =
    !win && reactAge >= 0 && reactAge < timeline.wobbleTicks
      ? Math.sin(reactAge * 0.55) * 0.05 * (1 - reactAge / timeline.wobbleTicks)
      : 0;
  const vault = vaultInstances({
    doorAngle: -easeOutBack(doorT) * 1.7,
    glow: easeOutCubic(doorT),
    wheelSpin: easeOutCubic(wheelT) * Math.PI * 2.5,
    wobble,
  });

  // Reward inside the open vault.
  const rewardInstances: SceneInstance[] = [];
  const rarity = outcomeRarity(session);
  if (win && doorT > 0.35 && plan !== null && rarity !== "loss") {
    const at = addV3(VAULT_CENTER, v3(0, -0.25, VAULT_SIZE.z / 2 - 0.3));
    rewardInstances.push(...rewardProp("reward", rarity, at, clamp01((doorT - 0.35) / 0.65), tick, 0.9));
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    if (plan.win) {
      const at = addV3(VAULT_CENTER, v3(0, 0.4, VAULT_SIZE.z / 2 + 0.3));
      celebration.push(...confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    } else {
      celebration.push(...sparkleRing("cheer", v3(0, 0.6, DICE_Z), profile.particles, plan.presentationSeed, phaseAge(session)));
    }
  }

  // Camera: table framing, easing toward the vault as it opens on a win.
  const base = tabletopCamera(v3(0, 0.55, -0.4), 2.9);
  const camera =
    doorT > 0
      ? revealFocusCamera(base, addV3(VAULT_CENTER, v3(0, 0, VAULT_SIZE.z / 2)), doorT, runtime.settings.reducedMotion ? 0.18 : 0.38)
      : base;

  // Warm light spilling from the vault once the door opens.
  const lights: SceneLight[] = [...stageLights(v3(0, 0.3, DICE_Z), 0.55)];
  if (doorT > 0.1) {
    lights.push({
      key: "light:vault",
      light: {
        color: [1, 0.82, 0.45, 1],
        intensity: 1.5 * doorT,
        kind: "point",
        position: addV3(VAULT_CENTER, v3(0, 0.3, VAULT_SIZE.z / 2 + 0.5)),
      },
    });
  }

  const pad: SceneInstance = {
    key: "table:pad",
    material: "Felt",
    mesh: "box",
    transform: { position: v3(0, -0.045, DICE_Z), rotation: QUAT_IDENTITY, scale: v3(3.4, 0.09, 1.9) },
  };

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(15), pad, ...shadows, ...dice, ...vault, ...rewardInstances, ...celebration],
    lights,
  };
};
