/*
 * scene.ts — Safe Cracker presentation: a bright, pastel toy prize-vault seen
 * face-on. A rounded safe body with gold trim, a swinging door carrying three
 * rotating dials (cylinders facing the camera, six colored notch markers per
 * rim), a big spoked handle, and four chunky bolts along the door edge. On a
 * win the bolts retract one at a time, the handle spins, and the door swings
 * open on a glowing shelf with the reward; on a loss the handle wiggles and the
 * dials give a sympathetic wobble. Pure view.
 */

import type { EngineQuat, EngineVec3, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera, showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, easeOutCubic, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
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
import type { SafeSpec, SafeState } from "./game.ts";
import {
  boltRetractStart,
  DIAL_COUNT,
  dialDisplayAngle,
  NUM_BOLTS,
  safeTimeline,
  SAFE_SYMBOLS,
  stopsMade,
  symbolAngle,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const NOTCH_MATERIALS = ["NotchStar", "NotchCoral", "NotchMint", "NotchSky", "NotchLav", "NotchGold"];

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  BoltSteel: { baseColor: [0.86, 0.88, 0.94, 1], emissive: [0.12, 0.13, 0.16, 1] },
  DialFace: { baseColor: [0.95, 0.92, 0.98, 1] },
  DialRim: { baseColor: [1, 0.82, 0.34, 1], emissive: [0.28, 0.2, 0.05, 1] },
  NotchCoral: { baseColor: [1, 0.52, 0.45, 1], emissive: [0.3, 0.14, 0.12, 1] },
  NotchGold: { baseColor: [1, 0.82, 0.32, 1], emissive: [0.35, 0.26, 0.08, 1] },
  NotchLav: { baseColor: [0.78, 0.62, 1, 1], emissive: [0.24, 0.18, 0.4, 1] },
  NotchMint: { baseColor: [0.5, 0.92, 0.72, 1], emissive: [0.14, 0.34, 0.24, 1] },
  NotchSky: { baseColor: [0.5, 0.78, 1, 1], emissive: [0.14, 0.26, 0.4, 1] },
  NotchStar: { baseColor: [1, 0.94, 0.5, 1], emissive: [0.5, 0.44, 0.16, 1] },
  SafeBody: { baseColor: [0.74, 0.86, 0.97, 1] },
  SafeDark: { baseColor: [0.52, 0.64, 0.8, 1] },
  SafeDoor: { baseColor: [0.86, 0.79, 0.98, 1] },
  SafeGlow: { baseColor: [1, 0.88, 0.55, 1], emissive: [1, 0.8, 0.4, 1], opacity: 0.8 },
  SafeInterior: { baseColor: [0.26, 0.22, 0.34, 1] },
};

export const SAFE_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── proportions ─────────────────────────────────────────────────────────────────

const SAFE_CENTER = v3(0, 1.55, 0);
const SAFE_SIZE = v3(3.0, 3.0, 0.9);
const DOOR_SIZE = v3(2.5, 2.5, 0.24);
const DOOR_FACE_Z = SAFE_SIZE.z / 2;
const DIAL_RADIUS = 0.42;
const DIAL_Y_OFFSET = 0.55;

/** Face-camera orientation for a cylinder (its round face toward +Z). */
const faceCamera = quatPitch(Math.PI / 2);

/** The three dials sit across the upper door; the handle sits below. */
const dialLocalX = (k: number): number => (k - (DIAL_COUNT - 1) / 2) * 0.78;

const dialInstances = (
  key: string,
  center: EngineVec3,
  angle: number,
  doorQ: EngineQuat,
  symbols: number,
  wobble: number,
): readonly SceneInstance[] => {
  const spinQ = quatMul(doorQ, quatMul(faceCamera, quatRoll(angle + wobble)));
  const face: SceneInstance = {
    key: `${key}:face`,
    material: "DialFace",
    mesh: "cylinder",
    transform: { position: center, rotation: quatMul(doorQ, faceCamera), scale: v3(DIAL_RADIUS * 2, 0.14, DIAL_RADIUS * 2) },
  };
  const rim: SceneInstance = {
    key: `${key}:rim`,
    material: "DialRim",
    mesh: "cylinder",
    transform: { position: addV3(center, rotateByQuat(v3(0, -0.02, 0), quatMul(doorQ, faceCamera))), rotation: quatMul(doorQ, faceCamera), scale: v3(DIAL_RADIUS * 2.22, 0.1, DIAL_RADIUS * 2.22) },
  };
  // Six notch markers around the rim; the marker at the front is the reading.
  const notches: SceneInstance[] = Array.from({ length: symbols }, (_, s) => {
    const a = symbolAngle(s, symbols);
    const local = v3(Math.sin(a) * DIAL_RADIUS * 0.82, 0.09, Math.cos(a) * DIAL_RADIUS * 0.82);
    return {
      key: `${key}:notch${s}`,
      material: NOTCH_MATERIALS[s % NOTCH_MATERIALS.length] as string,
      mesh: "box",
      transform: {
        position: addV3(center, rotateByQuat(local, spinQ)),
        rotation: spinQ,
        scale: v3(0.12, 0.1, 0.12),
      },
    };
  });
  const pointer: SceneInstance = {
    key: `${key}:reader`,
    material: "StageGold",
    mesh: "box",
    transform: {
      position: addV3(center, rotateByQuat(v3(0, 0.1, DIAL_RADIUS * 1.15), quatMul(doorQ, faceCamera))),
      rotation: quatMul(doorQ, faceCamera),
      scale: v3(0.08, 0.05, 0.14),
    },
  };
  return [face, rim, ...notches, pointer];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const safeScene = (runtime: GameRuntime<SafeSpec>, state: SafeState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const symbols = spec.symbols ?? SAFE_SYMBOLS;
  const plan = session.committed;
  const tick = session.tick;
  const timeline = safeTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const win = plan?.win ?? false;
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? timeline.winTotal
        : -1;

  // Door swing + handle spin driven by the reveal timeline (win only).
  const doorT = win && revealAge >= timeline.doorStart ? clamp01((revealAge - timeline.doorStart) / (timeline.doorEnd - timeline.doorStart)) : 0;
  const handleT = win && revealAge >= 0 ? clamp01(revealAge / timeline.boltsEnd) : 0;
  const lossWiggle =
    !win && revealAge >= 0 && revealAge < timeline.lossTotal
      ? Math.sin(revealAge * 0.5) * 0.05 * (1 - revealAge / timeline.lossTotal)
      : 0;
  const dialWobble =
    !win && revealAge >= 0 ? Math.sin(revealAge * 0.6) * 0.12 * (1 - clamp01(revealAge / timeline.lossTotal)) : 0;

  const bodyQ = QUAT_IDENTITY;
  const doorHinge = addV3(SAFE_CENTER, v3(-DOOR_SIZE.x / 2, 0, DOOR_FACE_Z));
  const doorQ = quatMul(bodyQ, quatYaw(-easeOutBack(doorT) * 1.7));
  const doorOffset = v3(DOOR_SIZE.x / 2, 0, DOOR_SIZE.z / 2);

  const bodyPart = (key: string, local: EngineVec3, scale: EngineVec3, material: string): SceneInstance => ({
    key: `safe:${key}`,
    material,
    mesh: "box",
    transform: { position: addV3(SAFE_CENTER, local), rotation: bodyQ, scale },
  });

  const body: SceneInstance[] = [
    bodyPart("body", v3(0, 0, 0), SAFE_SIZE, "SafeBody"),
    bodyPart("interior", v3(0, 0, DOOR_FACE_Z - 0.22), v3(DOOR_SIZE.x - 0.1, DOOR_SIZE.y - 0.1, 0.34), "SafeInterior"),
    bodyPart("trim", v3(0, 0, DOOR_FACE_Z - 0.02), v3(SAFE_SIZE.x + 0.14, SAFE_SIZE.y + 0.14, 0.06), "StageGold"),
    bodyPart("foot-l", v3(-SAFE_SIZE.x * 0.34, -SAFE_SIZE.y / 2 - 0.16, 0), v3(0.32, 0.16, SAFE_SIZE.z * 0.9), "SafeDark"),
    bodyPart("foot-r", v3(SAFE_SIZE.x * 0.34, -SAFE_SIZE.y / 2 - 0.16, 0), v3(0.32, 0.16, SAFE_SIZE.z * 0.9), "SafeDark"),
  ];

  // Glowing shelf + reward once the door opens.
  const rarity = outcomeRarity(session);
  const opened: SceneInstance[] = [];
  if (win && doorT > 0) {
    opened.push(
      bodyPart("glow", v3(0, 0, DOOR_FACE_Z - 0.18), v3(DOOR_SIZE.x * 0.9 * doorT, DOOR_SIZE.y * 0.9 * doorT, 0.1), "SafeGlow"),
      bodyPart("shelf", v3(0, -0.5, DOOR_FACE_Z - 0.4), v3(DOOR_SIZE.x * 0.78, 0.1, 0.55), "StageGold"),
    );
    if (plan !== null && rarity !== "loss" && doorT > 0.35) {
      const at = addV3(SAFE_CENTER, v3(0, -0.2, DOOR_FACE_Z - 0.25));
      opened.push(...rewardProp("reward", rarity, at, clamp01((doorT - 0.35) / 0.65), tick, 0.95));
    }
  }

  // The swinging door: slab, trim, three dials, handle, bolts.
  const doorSlab: SceneInstance = {
    key: "door:slab",
    material: "SafeDoor",
    mesh: "box",
    transform: hingedTransform(doorHinge, doorOffset, doorQ, DOOR_SIZE),
  };
  const doorTrim: SceneInstance = {
    key: "door:trim",
    material: "StageGold",
    mesh: "box",
    transform: hingedTransform(doorHinge, v3(DOOR_SIZE.x / 2, 0, DOOR_SIZE.z + 0.005), doorQ, v3(DOOR_SIZE.x - 0.2, DOOR_SIZE.y - 0.2, 0.03)),
  };

  const dials: SceneInstance[] = [];
  for (let k = 0; k < DIAL_COUNT; k += 1) {
    const localCenter = v3(dialLocalX(k), DIAL_Y_OFFSET, DOOR_SIZE.z + 0.02);
    const center = addV3(doorHinge, rotateByQuat(addV3(doorOffset, v3(localCenter.x - doorOffset.x, localCenter.y, localCenter.z - doorOffset.z)), doorQ));
    dials.push(...dialInstances(`dial${k}`, center, dialDisplayAngle(state, k), doorQ, symbols, dialWobble));
  }

  // Handle below the dials: hub + four spokes, spinning on a win.
  const handleLocal = v3(0, -0.75, DOOR_SIZE.z + 0.14);
  const handleCenter = addV3(doorHinge, rotateByQuat(addV3(doorOffset, v3(handleLocal.x - doorOffset.x, handleLocal.y, handleLocal.z - doorOffset.z)), doorQ));
  const handleSpin = easeOutCubic(handleT) * Math.PI * 3 + lossWiggle;
  const handle: SceneInstance[] = [
    {
      key: "door:handlehub",
      material: "SafeDark",
      mesh: "sphere",
      transform: { position: handleCenter, rotation: doorQ, scale: v3(0.24, 0.24, 0.24) },
    },
    ...Array.from({ length: 4 }, (_, k): SceneInstance => ({
      key: `door:spoke${k}`,
      material: "StageGold",
      mesh: "box",
      transform: {
        position: handleCenter,
        rotation: quatMul(doorQ, quatMul(faceCamera, quatRoll(handleSpin + (k * Math.PI) / 2))),
        scale: v3(0.62, 0.07, 0.07),
      },
    })),
  ];

  // Four chunky bolts along the right edge of the door; each retracts inward.
  const bolts: SceneInstance[] = Array.from({ length: NUM_BOLTS }, (_, k) => {
    const start = boltRetractStart(k, timeline);
    const retractT = win && revealAge >= start ? clamp01((revealAge - start) / timeline.boltRetract) : 0;
    const outX = DOOR_SIZE.x / 2 - 0.06;
    const localY = (k - (NUM_BOLTS - 1) / 2) * 0.62;
    const boltLocal = v3(outX + 0.16 - easeOutCubic(retractT) * 0.42, localY, DOOR_SIZE.z * 0.5);
    return {
      key: `door:bolt${k}`,
      material: "BoltSteel",
      mesh: "cylinder",
      transform: {
        position: addV3(doorHinge, rotateByQuat(addV3(doorOffset, v3(boltLocal.x - doorOffset.x, boltLocal.y, boltLocal.z - doorOffset.z)), doorQ)),
        rotation: quatMul(doorQ, quatPitch(Math.PI / 2 + Math.PI / 2)),
        scale: v3(0.16, 0.34, 0.16),
      },
    };
  });

  // Celebration in front of the safe.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(SAFE_CENTER, v3(0, 0.2, DOOR_FACE_Z + 0.5));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera: showcase framing, easing toward the opening door.
  const base = showcaseCamera(SAFE_CENTER, 6.2, 0.4, 0.86);
  const camera =
    doorT > 0
      ? revealFocusCamera(base, addV3(SAFE_CENTER, v3(0, 0, DOOR_FACE_Z)), doorT, runtime.settings.reducedMotion ? 0.15 : 0.32)
      : base;

  const lights: SceneLight[] = [...stageLights(addV3(SAFE_CENTER, v3(0, 0, DOOR_FACE_Z)), 0.6)];
  if (doorT > 0.1) {
    lights.push({
      key: "light:vault",
      light: {
        color: [1, 0.82, 0.45, 1],
        intensity: 1.5 * doorT,
        kind: "point",
        position: addV3(SAFE_CENTER, v3(0, 0.1, DOOR_FACE_Z + 0.6)),
      },
    });
  }

  // Pre-reveal shimmer beacon: a soft pulse over the dials during interacting.
  const shimmer: SceneInstance[] =
    session.phase === "interacting" && stopsMade(state.extra) === DIAL_COUNT
      ? [
          {
            key: "safe:shimmer",
            material: "SafeGlow",
            mesh: "sphere",
            transform: {
              position: addV3(SAFE_CENTER, v3(0, DIAL_Y_OFFSET, DOOR_FACE_Z + 0.3)),
              rotation: QUAT_IDENTITY,
              scale: v3(0.3 + pulse((tick % 40) / 40) * 0.1, 0.3, 0.3),
            },
          },
        ]
      : [];

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [
      ...stageRoom(16),
      ...body,
      ...opened,
      doorSlab,
      doorTrim,
      ...bolts,
      ...dials,
      ...handle,
      ...shimmer,
      ...celebration,
    ],
    lights,
  };
};
