/*
 * scene.ts — Prize Wheel presentation: a carnival wheel whose wedge meshes are
 * generated from the COMPILED probability arcs (the drawing is the odds), a
 * gilded rim with marquee bulbs, a fixed top pointer, a charge meter that
 * fills while the launch is held, and the tier-scaled celebration at the
 * pointer. Shallow-3D: the wheel faces the camera.
 */

import type { MaterialSpec, MeshData, Scene, SceneInstance } from "@axiom/web-engine";
import type { EngineVec3, GameResources, Rgba } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, REWARD_MATERIALS } from "../../presentation/rewards/tiers.ts";
import { pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { QUAT_IDENTITY, quatPitch, quatRoll, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor } from "../round-state.ts";
import type { WheelSpec, WheelState } from "./game.ts";
import { chargeStrength, POINTER_ANGLE, segmentArcs, wheelAngle } from "./game.ts";

const HUB = v3(0, 2.15, 0);
const RADIUS = 1.75;

/** A flat wedge (front/back fan + outer rim) spanning `arc` radians around +X. */
const wedgeMesh = (arc: number): MeshData => {
  const steps = Math.max(2, Math.ceil(arc / 0.16));
  const half = arc / 2;
  const depth = 0.07;
  const positions: EngineVec3[] = [];
  const normals: EngineVec3[] = [];
  const indices: number[] = [];
  // Front fan (z = +depth), back fan (z = -depth).
  for (const side of [1, -1]) {
    const baseIndex = positions.length;
    positions.push(v3(0, 0, depth * side));
    normals.push(v3(0, 0, side));
    for (let i = 0; i <= steps; i += 1) {
      const a = -half + (arc * i) / steps;
      positions.push(v3(Math.cos(a), Math.sin(a), depth * side));
      normals.push(v3(0, 0, side));
    }
    for (let i = 0; i < steps; i += 1) {
      if (side === 1) {
        indices.push(baseIndex, baseIndex + 1 + i, baseIndex + 2 + i);
      } else {
        indices.push(baseIndex, baseIndex + 2 + i, baseIndex + 1 + i);
      }
    }
  }
  // Outer rim strip.
  const rimBase = positions.length;
  for (let i = 0; i <= steps; i += 1) {
    const a = -half + (arc * i) / steps;
    const n = v3(Math.cos(a), Math.sin(a), 0);
    positions.push(v3(n.x, n.y, depth), v3(n.x, n.y, -depth));
    normals.push(n, n);
  }
  for (let i = 0; i < steps; i += 1) {
    const a = rimBase + i * 2;
    indices.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
  }
  return { indices, normals, positions };
};

const LOSS_COLORS: readonly Rgba[] = [
  [0.78, 0.86, 0.98, 1],
  [0.95, 0.87, 0.98, 1],
];

const rarityOfTier = (runtime: GameRuntime<WheelSpec>, tierId: string | null): Rgba => {
  const tier = runtime.config.rewardTiers.find((t) => t.id === tierId);
  return tier === undefined ? LOSS_COLORS[0] as Rgba : RARITY_COLORS[tier.rarity];
};

/** Declared once per mount: wedge meshes sized by the compiled arcs. */
export const wheelResources = (runtime: GameRuntime<WheelSpec>): GameResources => {
  const spec = runtime.config.gameSpecific;
  const arcs = segmentArcs(spec, runtime.config.targetWinRate);
  const meshes = Object.fromEntries([
    ["box", { kind: "box" as const }],
    ["cylinder", { kind: "cylinder" as const }],
    ["sphere", { kind: "sphere" as const }],
    ...arcs.map((arc, i) => [`wedge${i}`, { data: wedgeMesh(Math.max(0.02, arc.end - arc.start)) }] as const),
  ]);
  const materials: Record<string, MaterialSpec> = {
    ...STAGE_MATERIALS,
    ...REWARD_MATERIALS,
    ...CONFETTI_MATERIALS,
    Bulb: { baseColor: [1, 0.95, 0.75, 1], emissive: [0.9, 0.8, 0.5, 1] },
    BulbDim: { baseColor: [0.75, 0.7, 0.6, 1], emissive: [0.12, 0.1, 0.06, 1] },
    ChargeFill: { baseColor: [1, 0.7, 0.3, 1], emissive: [0.8, 0.5, 0.15, 1] },
    ChargeTrack: { baseColor: [0.25, 0.3, 0.4, 1] },
    PointerGold: { baseColor: [1, 0.8, 0.3, 1], emissive: [0.4, 0.28, 0.06, 1] },
    WheelHub: { baseColor: [0.98, 0.96, 0.9, 1], emissive: [0.2, 0.18, 0.14, 1] },
  };
  let lossIndex = 0;
  spec.segments.forEach((segment, i) => {
    const color: Rgba =
      segment.tierId === null ? (LOSS_COLORS[(lossIndex += 1) % 2] as Rgba) : rarityOfTier(runtime, segment.tierId);
    materials[`seg${i}`] = {
      baseColor: color,
      emissive: [color[0] * 0.18, color[1] * 0.18, color[2] * 0.18, 1],
    };
  });
  return { materials, meshes };
};

export const wheelScene = (runtime: GameRuntime<WheelSpec>, state: WheelState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const arcs = segmentArcs(spec, runtime.config.targetWinRate);
  const angle = wheelAngle(state);
  const tick = session.tick;

  const wedges: SceneInstance[] = arcs.map((arc, i) => ({
    key: `wedge${i}`,
    material: `seg${i}`,
    mesh: `wedge${i}`,
    transform: {
      position: HUB,
      rotation: quatRoll(angle + arc.center),
      scale: v3(RADIUS, RADIUS, 1),
    },
  }));

  // Marquee bulbs on the rim: gentle two-phase twinkle (pure decoration).
  const bulbs: SceneInstance[] = Array.from({ length: 12 }, (_, i) => {
    const a = (i / 12) * Math.PI * 2 + angle;
    const lit = (i + Math.floor(tick / 24)) % 2 === 0;
    return {
      key: `bulb${i}`,
      material: lit ? "Bulb" : "BulbDim",
      mesh: "sphere",
      transform: {
        position: v3(HUB.x + Math.cos(a) * (RADIUS + 0.13), HUB.y + Math.sin(a) * (RADIUS + 0.13), HUB.z + 0.17),
        rotation: QUAT_IDENTITY,
        scale: v3(0.09, 0.09, 0.09),
      },
    };
  });

  const chrome: SceneInstance[] = [
    { key: "rim", material: "StageGold", mesh: "cylinder", transform: { position: v3(HUB.x, HUB.y, HUB.z - 0.02), rotation: quatPitch(Math.PI / 2), scale: v3(RADIUS * 2 + 0.16, 0.1, RADIUS * 2 + 0.16) } },
    { key: "hub", material: "WheelHub", mesh: "cylinder", transform: { position: v3(HUB.x, HUB.y, HUB.z + 0.1), rotation: quatPitch(Math.PI / 2), scale: v3(0.42, 0.12, 0.42) } },
    { key: "stand", material: "StageHousingDark", mesh: "box", transform: { position: v3(0, 0.65, -0.14), rotation: QUAT_IDENTITY, scale: v3(0.35, 1.7, 0.22) } },
    { key: "base", material: "StageHousing", mesh: "box", transform: { position: v3(0, 0.06, -0.1), rotation: QUAT_IDENTITY, scale: v3(1.9, 0.3, 0.9) } },
    {
      key: "pointer",
      material: "PointerGold",
      mesh: "box",
      transform: {
        position: v3(HUB.x + Math.cos(POINTER_ANGLE) * (RADIUS + 0.28), HUB.y + Math.sin(POINTER_ANGLE) * (RADIUS + 0.28), HUB.z + 0.1),
        rotation: quatRoll(POINTER_ANGLE + Math.PI / 4),
        scale: v3(0.26, 0.26, 0.1),
      },
    },
  ];

  // Charge meter while aiming: a track under the wheel that fills with hold.
  const meter: SceneInstance[] = [];
  if (session.phase === "ready") {
    const strength = chargeStrength(state.extra.chargeTicks);
    meter.push(
      { key: "charge:track", material: "ChargeTrack", mesh: "box", transform: { position: v3(0, 0.32, 0.4), rotation: QUAT_IDENTITY, scale: v3(2.2, 0.12, 0.12) } },
      {
        key: "charge:fill",
        material: "ChargeFill",
        mesh: "box",
        transform: {
          position: v3(-1.1 + (2.2 * strength) / 2, 0.32, 0.42),
          rotation: QUAT_IDENTITY,
          scale: v3(Math.max(0.02, 2.2 * strength), 0.14 + pulse((tick % 30) / 30) * 0.02 * strength, 0.12),
        },
      },
    );
  }

  // Celebration at the pointer.
  const celebration: SceneInstance[] = [];
  const plan = session.committed;
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = v3(HUB.x, HUB.y + RADIUS + 0.3, HUB.z + 0.2);
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  return {
    camera: showcaseCamera(v3(0, 1.9, 0), 6.8, 0.35, 0.82),
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(15), ...wedges, ...bulbs, ...chrome, ...meter, ...celebration],
    lights: stageLights(v3(HUB.x, HUB.y, HUB.z + 1), 0.6),
  };
};
