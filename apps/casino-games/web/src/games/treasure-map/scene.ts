/*
 * scene.ts — Treasure Map presentation: a parchment sheet seen from above, a
 * painted island (overlapping flat green cylinders on a blue water border),
 * dashed footpaths between the X-marked dig sites, palm trees, a gently
 * swaying compass rose, and the dig ceremony — the little digger hops to the
 * chosen X, the shovel bobs through three beats of dust, and the hole reveals
 * its preassigned contents (or an honest empty bottom). Pure view:
 * `mapScene(runtime, state)` returns a Scene value.
 */

import type { MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineVec3, GameResources } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01 } from "../../presentation/stage/easing.ts";
import { contactShadow, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { MapSpec, MapState } from "./game.ts";
import {
  compassAngle,
  digPosition,
  diggerPose,
  mapCamera,
  mapChoiceCount,
  mapRevealTimeline,
  markerPulseScale,
  shovelSwing,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  CompassBase: { baseColor: [0.96, 0.93, 0.8, 1] },
  CompassNeedle: { baseColor: [0.85, 0.28, 0.24, 1] },
  DiggerBody: { baseColor: [0.34, 0.55, 0.85, 1] },
  DiggerHead: { baseColor: [0.98, 0.85, 0.7, 1] },
  DustBrown: { baseColor: [0.74, 0.62, 0.46, 1], emissive: [0.12, 0.09, 0.06, 1] },
  HoleDark: { baseColor: [0.12, 0.09, 0.07, 1] },
  IslandGrass: { baseColor: [0.55, 0.82, 0.5, 1] },
  IslandGrassDeep: { baseColor: [0.42, 0.72, 0.44, 1] },
  MapWater: { baseColor: [0.45, 0.71, 0.92, 1] },
  PalmLeaf: { baseColor: [0.3, 0.68, 0.38, 1] },
  PalmTrunk: { baseColor: [0.6, 0.44, 0.28, 1] },
  Parchment: { baseColor: [0.93, 0.85, 0.66, 1] },
  PathDash: { baseColor: [0.72, 0.52, 0.32, 1] },
  ShovelBlade: { baseColor: [0.65, 0.68, 0.72, 1] },
  ShovelHandle: { baseColor: [0.55, 0.4, 0.24, 1] },
  XDim: { baseColor: [0.62, 0.45, 0.42, 1] },
  XRed: { baseColor: [0.92, 0.25, 0.2, 1], emissive: [0.22, 0.05, 0.04, 1] },
};

export const MAP_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── the fixed island illustration (pure tables, no streams) ────────────────────

const ISLAND_BLOBS: readonly { readonly at: EngineVec3; readonly d: number }[] = [
  { at: v3(0, 0, -0.1), d: 4.8 },
  { at: v3(-1.7, 0, 0.55), d: 2.9 },
  { at: v3(1.8, 0, -0.45), d: 3.1 },
  { at: v3(0.6, 0, 1.05), d: 2.6 },
];

const ISLAND_MEADOWS: readonly { readonly at: EngineVec3; readonly d: number }[] = [
  { at: v3(-0.8, 0, -0.3), d: 2.0 },
  { at: v3(1.1, 0, 0.6), d: 1.7 },
];

const PALMS: readonly EngineVec3[] = [v3(-2.25, 0, -1.35), v3(2.4, 0, 1.35), v3(2.55, 0, -1.45)];

const COMPASS_AT: EngineVec3 = v3(-3.0, 0, -2.0);

const flat = (key: string, material: string, at: EngineVec3, d: number, y: number, h = 0.05): SceneInstance => ({
  key,
  material,
  mesh: "cylinder",
  transform: { position: v3(at.x, y, at.z), rotation: QUAT_IDENTITY, scale: v3(d, h, d) },
});

const islandSheet = (): readonly SceneInstance[] => [
  {
    key: "map:parchment",
    material: "Parchment",
    mesh: "box",
    transform: { position: v3(0, 0.02, 0), rotation: QUAT_IDENTITY, scale: v3(7.6, 0.06, 5.4) },
  },
  {
    key: "map:water",
    material: "MapWater",
    mesh: "box",
    transform: { position: v3(0, 0.06, 0), rotation: QUAT_IDENTITY, scale: v3(6.8, 0.04, 4.7) },
  },
  ...ISLAND_BLOBS.map((blob, i) => flat(`map:island${i}`, "IslandGrass", blob.at, blob.d, 0.09, 0.06)),
  ...ISLAND_MEADOWS.map((blob, i) => flat(`map:meadow${i}`, "IslandGrassDeep", blob.at, blob.d, 0.115, 0.03)),
];

const palmTree = (key: string, at: EngineVec3): readonly SceneInstance[] => [
  {
    key: `${key}:trunk`,
    material: "PalmTrunk",
    mesh: "cylinder",
    transform: { position: v3(at.x, 0.36, at.z), rotation: QUAT_IDENTITY, scale: v3(0.09, 0.5, 0.09) },
  },
  {
    key: `${key}:canopy`,
    material: "PalmLeaf",
    mesh: "sphere",
    transform: { position: v3(at.x, 0.64, at.z), rotation: QUAT_IDENTITY, scale: v3(0.5, 0.22, 0.5) },
  },
  {
    key: `${key}:crown`,
    material: "PalmLeaf",
    mesh: "sphere",
    transform: { position: v3(at.x + 0.12, 0.7, at.z - 0.08), rotation: QUAT_IDENTITY, scale: v3(0.3, 0.14, 0.3) },
  },
];

/** Dashed footpath between consecutive dig sites — a pure function of the
 * fixed layout table (no streams). */
const pathDashes = (count: number): readonly SceneInstance[] =>
  Array.from({ length: Math.max(0, count - 1) }, (_, seg) => {
    const from = digPosition(seg);
    const to = digPosition(seg + 1);
    const dx = to.x - from.x;
    const dz = to.z - from.z;
    const len = Math.sqrt(dx * dx + dz * dz);
    const yaw = Math.atan2(dx, dz);
    const dashes = Math.max(1, Math.floor(len / 0.5) - 1);
    return Array.from({ length: dashes }, (_, j) => {
      const t = (j + 1) / (dashes + 1);
      return {
        key: `path:${seg}:${j}`,
        material: "PathDash",
        mesh: "box",
        transform: {
          position: v3(from.x + dx * t, 0.13, from.z + dz * t),
          rotation: quatYaw(yaw),
          scale: v3(0.06, 0.02, 0.2),
        },
      };
    });
  }).flat();

const compassRose = (tick: number, seed: number, liveliness: number): readonly SceneInstance[] => {
  const angle = compassAngle(tick, seed, liveliness);
  return [
    flat("compass:base", "CompassBase", COMPASS_AT, 0.6, 0.09, 0.03),
    {
      key: "compass:ns",
      material: "CompassNeedle",
      mesh: "box",
      transform: { position: v3(COMPASS_AT.x, 0.12, COMPASS_AT.z), rotation: quatYaw(angle), scale: v3(0.05, 0.02, 0.52) },
    },
    {
      key: "compass:ew",
      material: "CompassNeedle",
      mesh: "box",
      transform: {
        position: v3(COMPASS_AT.x, 0.12, COMPASS_AT.z),
        rotation: quatYaw(angle + Math.PI / 2),
        scale: v3(0.05, 0.02, 0.36),
      },
    },
    {
      key: "compass:pin",
      material: "StageGold",
      mesh: "sphere",
      transform: { position: v3(COMPASS_AT.x, 0.14, COMPASS_AT.z), rotation: QUAT_IDENTITY, scale: v3(0.08, 0.08, 0.08) },
    },
  ];
};

/** One X marker: two crossed thin red boxes (dim once the round has moved on). */
const xMarker = (key: string, at: EngineVec3, scale: number, dim: boolean, ring: "none" | "focus" | "hover"): readonly SceneInstance[] => {
  const material = dim ? "XDim" : "XRed";
  const arm = (suffix: string, yaw: number): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh: "box",
    transform: { position: v3(at.x, 0.155, at.z), rotation: quatYaw(yaw), scale: v3(0.1 * scale, 0.045, 0.44 * scale) },
  });
  const rings: SceneInstance[] =
    ring === "none"
      ? []
      : [
          {
            key: `${key}:ring`,
            material: ring === "hover" ? "StageGold" : "StageFloorAccent",
            mesh: "cylinder",
            transform: { position: v3(at.x, 0.125, at.z), rotation: QUAT_IDENTITY, scale: v3(0.82, 0.015, 0.82) },
          },
        ];
  return [arm("a", Math.PI / 4), arm("b", -Math.PI / 4), ...rings];
};

/** A small dust puff (brown motes on analytic arcs, PARTICLES stream only). */
const dustPuff = (keyPrefix: string, origin: EngineVec3, seed: number, age: number, life = 12): readonly SceneInstance[] => {
  if (age < 0 || age > life) {
    return [];
  }
  const t = age / life;
  return Array.from({ length: 6 }, (_, i) => {
    const angle = sample01(seed, "particles", 60 + i, 0) * Math.PI * 2;
    const dist = 0.1 + (0.14 + sample01(seed, "particles", 60 + i, 1) * 0.26) * t;
    const size = 0.05 * (1 - t) + 0.015;
    return {
      key: `${keyPrefix}:${i}`,
      material: "DustBrown",
      mesh: "sphere",
      transform: {
        position: v3(origin.x + Math.cos(angle) * dist, 0.12 + t * 0.3, origin.z + Math.sin(angle) * dist),
        rotation: QUAT_IDENTITY,
        scale: v3(size, size, size),
      },
    };
  });
};

/** The digger (body + head) and, during the dig beats, the bobbing shovel. */
const diggerInstances = (spot: EngineVec3, travelT: number, swing: number): readonly SceneInstance[] => {
  const stand = addV3(spot, v3(0.34, 0, 0.24));
  const at = travelT < 1 ? diggerPose(stand, travelT) : stand;
  const digging = swing > 0;
  const shovel: SceneInstance[] = digging
    ? [
        {
          key: "digger:handle",
          material: "ShovelHandle",
          mesh: "box",
          transform: {
            position: v3(at.x - 0.2, at.y + 0.22 - swing * 0.08, at.z - 0.06),
            rotation: quatPitch(-0.9 + swing * 0.8),
            scale: v3(0.04, 0.42, 0.04),
          },
        },
        {
          key: "digger:blade",
          material: "ShovelBlade",
          mesh: "box",
          transform: {
            position: v3(at.x - 0.3, at.y + 0.06 - swing * 0.05, at.z - 0.1),
            rotation: quatPitch(-0.9 + swing * 0.8),
            scale: v3(0.13, 0.15, 0.03),
          },
        },
      ]
    : [];
  return [
    {
      key: "digger:body",
      material: "DiggerBody",
      mesh: "cylinder",
      transform: { position: v3(at.x, at.y + 0.19, at.z), rotation: QUAT_IDENTITY, scale: v3(0.24, 0.36, 0.24) },
    },
    {
      key: "digger:head",
      material: "DiggerHead",
      mesh: "sphere",
      transform: { position: v3(at.x, at.y + 0.45, at.z), rotation: QUAT_IDENTITY, scale: v3(0.17, 0.17, 0.17) },
    },
    contactShadow("digger:shadow", v3(at.x, 0, at.z), 0.2),
    ...shovel,
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const mapScene = (runtime: GameRuntime<MapSpec>, state: MapState): Scene => {
  const session = state.session;
  const count = mapChoiceCount(session);
  const seed = session.seed;
  const tick = session.tick;
  const spec = runtime.config.gameSpecific;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const timeline = mapRevealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? timeline.total
        : -1;
  const idle = session.phase === "ready" || session.phase === "intro";
  const pulseIntensity = idle ? spec.markerPulse : 0;
  const hitsPassed = revealAge >= 0 ? timeline.hits.filter((hit) => revealAge >= hit).length : 0;

  // Markers: pulse while idle; the chosen X vanishes once the hole opens.
  const markers = Array.from({ length: count }, (_, index) => {
    const at = digPosition(index);
    const isSelected = selected === index;
    if (isSelected && hitsPassed > 0) {
      return [];
    }
    const ring: "none" | "focus" | "hover" =
      session.phase === "ready"
        ? state.extra.choice.hovered === index
          ? "hover"
          : state.extra.choice.focused === index
            ? "focus"
            : "none"
        : "none";
    return xMarker(`x${index}`, at, markerPulseScale(index, tick, seed, pulseIntensity), revealAge >= 0 && !isSelected, ring);
  }).flat();

  // The dig ceremony at the chosen site.
  const ceremony: SceneInstance[] = [];
  if (selected !== null && revealAge >= 0) {
    const spot = digPosition(selected);
    const travelT = clamp01(revealAge / timeline.travelEnd);
    ceremony.push(...diggerInstances(spot, travelT, shovelSwing(timeline, revealAge)));
    if (hitsPassed > 0) {
      const holeD = 0.24 + hitsPassed * 0.14;
      ceremony.push(flat("dig:hole", "HoleDark", spot, holeD, 0.135, 0.03));
    }
    if (plan !== null) {
      timeline.hits.forEach((hit, k) => {
        ceremony.push(...dustPuff(`dust${k}`, spot, plan.presentationSeed, revealAge - hit));
      });
      if (revealAge >= timeline.digEnd) {
        const riseT = clamp01((revealAge - timeline.digEnd) / (timeline.riseEnd - timeline.digEnd));
        const rarity = outcomeRarity(session);
        if (plan.win && rarity !== "loss") {
          ceremony.push(...rewardProp("reward", rarity, v3(spot.x, 0.2, spot.z), riseT, tick));
        } else {
          ceremony.push(...sparkleRing("empty", v3(spot.x, 0.2, spot.z), 5, plan.presentationSeed, revealAge - timeline.digEnd, 50));
        }
      }
    }
  }

  // Celebration at the dig site.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(digPosition(selected), v3(0, 0.7, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera: overhead table framing, easing toward the chosen X during the dig.
  const base = mapCamera(count);
  const focusT = selected !== null && revealAge >= 0 ? clamp01(revealAge / timeline.travelEnd) : 0;
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(base, addV3(digPosition(selected), v3(0, 0.3, 0)), focusT, runtime.settings.reducedMotion ? 0.18 : 0.4)
      : base;

  // Warm light over the hole once the contents come up (wins only).
  const lights: SceneLight[] = [...stageLights(selected !== null ? digPosition(selected) : v3(0, 0, 0), 0.5)];
  if (selected !== null && plan !== null && plan.win && revealAge >= timeline.digEnd) {
    lights.push({
      key: "light:dig",
      light: {
        color: [1, 0.85, 0.5, 1],
        intensity: 1.2,
        kind: "point",
        position: addV3(digPosition(selected), v3(0, 1.2, 0.3)),
      },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [
      ...stageRoom(16),
      ...islandSheet(),
      ...pathDashes(count),
      ...PALMS.flatMap((at, i) => palmTree(`palm${i}`, at)),
      ...compassRose(tick, seed, idle ? spec.compassLiveliness : 0),
      ...markers,
      ...ceremony,
      ...celebration,
    ],
    lights,
  };
};
