/*
 * scene.ts — Fishing Cast presentation: a bright pond at a gentle angle with
 * lily pads, reeds, region ring buoys, a wooden dock with rod and line, the
 * cast reticle, the bobber's flight/float/dip/reel, splash droplets and
 * expanding ripples (all bounded), and the catch rising at the dock — a
 * region-family prize on a win, a warm little joke on a loss. Pure view.
 */

import type { EngineQuat, EngineVec3, GameResources, MaterialSpec, Rgba, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { rewardBeam, REWARD_MATERIALS, rewardMaterialOf } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, pulse } from "../../presentation/stage/easing.ts";
import { STAGE_MATERIALS, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatAxisAngle, quatPitch, quatRoll, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { FishingSpec, FishingState } from "./game.ts";
import {
  bobberAt,
  castTimeline,
  CATCH_POINT,
  committedAim,
  familyOfRegion,
  lossCatchOf,
  POND_RADIUS,
  ROD_TIP,
  WATER_Y,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  BobberRed: { baseColor: [0.95, 0.28, 0.24, 1], emissive: [0.28, 0.06, 0.05, 1] },
  BobberWhite: { baseColor: [0.98, 0.96, 0.9, 1] },
  CrabCoral: { baseColor: [0.96, 0.45, 0.35, 1] },
  DockWood: { baseColor: [0.52, 0.35, 0.2, 1] },
  DockWoodDark: { baseColor: [0.38, 0.26, 0.15, 1] },
  Droplet: { baseColor: [0.7, 0.88, 0.96, 1], emissive: [0.2, 0.3, 0.35, 1], opacity: 0.85 },
  DuckBeak: { baseColor: [1, 0.62, 0.2, 1] },
  DuckYellow: { baseColor: [1, 0.87, 0.3, 1], emissive: [0.2, 0.16, 0.03, 1] },
  FishBody: { baseColor: [0.72, 0.82, 0.9, 1], emissive: [0.1, 0.13, 0.16, 1] },
  FishEye: { baseColor: [0.12, 0.14, 0.18, 1] },
  Grass: { baseColor: [0.4, 0.66, 0.3, 1] },
  LeafGreen: { baseColor: [0.42, 0.54, 0.26, 1] },
  LilyPad: { baseColor: [0.26, 0.56, 0.32, 1] },
  LineSilk: { baseColor: [0.28, 0.3, 0.34, 1] },
  PondBed: { baseColor: [0.72, 0.58, 0.38, 1] },
  PondWater: { baseColor: [0.14, 0.44, 0.52, 1], emissive: [0.03, 0.1, 0.12, 1], opacity: 0.85 },
  RegionRing: { baseColor: [0.95, 0.95, 0.85, 1], opacity: 0.35 },
  RegionRingLit: { baseColor: [1, 0.92, 0.55, 1], emissive: [0.5, 0.42, 0.15, 1], opacity: 0.6 },
  Reed: { baseColor: [0.34, 0.56, 0.28, 1] },
  RockGrey: { baseColor: [0.62, 0.62, 0.66, 1] },
  RockGreyDark: { baseColor: [0.44, 0.45, 0.5, 1] },
  Reticle: { baseColor: [1, 0.85, 0.4, 1], emissive: [0.65, 0.5, 0.15, 1], opacity: 0.8 },
  Ripple: { baseColor: [0.85, 0.95, 0.98, 1], opacity: 0.4 },
  RodWood: { baseColor: [0.55, 0.36, 0.22, 1] },
  SmallChest: { baseColor: [0.58, 0.37, 0.2, 1] },
};

/**
 * Fishing Cast's own dusk sky: a deep navy that seats the moody pond, in place
 * of the shared pavilion's pastel daylight clear. Atmosphere is expressed
 * entirely through the clear color here (this TS engine has no post/tonemap
 * node), so a dark backdrop is what pushes the frame off high-key pastel.
 */
const NIGHT_SKY: Rgba = [0.09, 0.14, 0.26, 1];

export const FISHING_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── small builders ──────────────────────────────────────────────────────────────

/** A thin box stretched between two points (rod, fishing line). */
const segment = (key: string, material: string, a: EngineVec3, b: EngineVec3, thickness: number): SceneInstance => {
  const d = v3(b.x - a.x, b.y - a.y, b.z - a.z);
  const len = Math.max(1e-4, Math.hypot(d.x, d.y, d.z));
  const unit = v3(d.x / len, d.y / len, d.z / len);
  // Rotate +Y onto the segment direction, about the axis Y × unit = (uz, 0, -ux).
  const axisX = unit.z;
  const axisZ = -unit.x;
  const axisLen = Math.hypot(axisX, axisZ);
  const angle = Math.acos(Math.min(1, Math.max(-1, unit.y)));
  const rotation: EngineQuat =
    axisLen < 1e-5 ? QUAT_IDENTITY : quatAxisAngle(v3(axisX / axisLen, 0, axisZ / axisLen), angle);
  return {
    key,
    material,
    mesh: "box",
    transform: {
      position: v3((a.x + b.x) / 2, (a.y + b.y) / 2, (a.z + b.z) / 2),
      rotation,
      scale: v3(thickness, len, thickness),
    },
  };
};

/** An angular low-poly shore boulder: a rotated box body with a tilted facet cap. */
const rock = (key: string, at: EngineVec3, size: EngineVec3, yaw: number): readonly SceneInstance[] => {
  const tilt = 1 / Math.hypot(0.4, 1, 0.25);
  return [
    {
      key: `${key}:body`,
      material: "RockGrey",
      mesh: "box",
      transform: { position: at, rotation: quatYaw(yaw), scale: size },
    },
    {
      key: `${key}:facet`,
      material: "RockGreyDark",
      mesh: "box",
      transform: {
        position: addV3(at, v3(0, size.y * 0.28, 0)),
        rotation: quatAxisAngle(v3(0.4 * tilt, tilt, 0.25 * tilt), yaw + 0.9),
        scale: v3(size.x * 0.68, size.y * 0.72, size.z * 0.68),
      },
    },
  ];
};

const flatRing = (key: string, material: string, at: EngineVec3, radius: number, height = 0.02): SceneInstance => ({
  key,
  material,
  mesh: "cylinder",
  transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(radius * 2, height, radius * 2) },
});

/** Expanding, staggered splash ripples (2–3 rings, strictly bounded). */
const ripples = (keyPrefix: string, at: EngineVec3, age: number, count = 3): readonly SceneInstance[] =>
  Array.from({ length: count }, (_, i) => {
    const local = age - i * 8;
    const life = 42;
    if (local < 0 || local > life) {
      return null;
    }
    const t = local / life;
    return flatRing(`${keyPrefix}:${i}`, "Ripple", v3(at.x, WATER_Y + 0.012 + i * 0.004, at.z), 0.16 + t * 1.1, 0.012);
  }).filter((r): r is SceneInstance => r !== null);

/** A bounded ring of splash droplets on ballistic arcs (10 spheres). */
const splashDroplets = (at: EngineVec3, seed: number, age: number, life: number): readonly SceneInstance[] => {
  if (age < 0 || age > life) {
    return [];
  }
  const t = age / 24;
  return Array.from({ length: 10 }, (_, i) => {
    const angle = (i / 10) * Math.PI * 2 + sample01(seed, "particles", i, 20) * 0.5;
    const speed = 0.5 + sample01(seed, "particles", i, 21) * 0.7;
    const up = 1.2 + sample01(seed, "particles", i, 22) * 0.8;
    const y = at.y + up * t - 3.4 * t * t;
    if (y < WATER_Y - 0.02) {
      return null;
    }
    const size = 0.05 + sample01(seed, "particles", i, 23) * 0.03;
    return {
      key: `splash:${i}`,
      material: "Droplet",
      mesh: "sphere",
      transform: {
        position: v3(at.x + Math.cos(angle) * speed * t, y, at.z + Math.sin(angle) * speed * t),
        rotation: QUAT_IDENTITY,
        scale: v3(size, size, size),
      },
    } satisfies SceneInstance;
  }).filter((d): d is SceneInstance => d !== null);
};

// ── the catch props ─────────────────────────────────────────────────────────────

const winCatch = (
  family: "fish" | "treasure" | "capsule",
  tierMaterial: string,
  at: EngineVec3,
  spin: EngineQuat,
): readonly SceneInstance[] => {
  const prop = (suffix: string, material: string, offset: EngineVec3, scale: EngineVec3, rotation = spin): SceneInstance => ({
    key: `catch:${suffix}`,
    material,
    mesh: "box",
    transform: { position: addV3(at, offset), rotation, scale },
  });
  if (family === "fish") {
    return [
      {
        key: "catch:body",
        material: "FishBody",
        mesh: "sphere",
        transform: { position: at, rotation: spin, scale: v3(0.56, 0.3, 0.22) },
      },
      prop("tail", tierMaterial, v3(-0.32, 0, 0), v3(0.16, 0.2, 0.05)),
      prop("fin", tierMaterial, v3(0.02, 0.18, 0), v3(0.2, 0.12, 0.04)),
      {
        key: "catch:eye",
        material: "FishEye",
        mesh: "sphere",
        transform: { position: addV3(at, v3(0.2, 0.05, 0.1)), rotation: QUAT_IDENTITY, scale: v3(0.05, 0.05, 0.05) },
      },
    ];
  }
  if (family === "treasure") {
    return [
      prop("body", "SmallChest", v3(0, -0.05, 0), v3(0.44, 0.26, 0.3)),
      prop("lid", "SmallChest", v3(0, 0.14, -0.12), v3(0.46, 0.1, 0.3), quatPitch(-0.9)),
      {
        key: "catch:gem",
        material: tierMaterial,
        mesh: "sphere",
        transform: { position: addV3(at, v3(0, 0.22, 0)), rotation: spin, scale: v3(0.18, 0.18, 0.18) },
      },
    ];
  }
  return [
    {
      key: "catch:shell",
      material: tierMaterial,
      mesh: "cylinder",
      transform: { position: at, rotation: spin, scale: v3(0.3, 0.34, 0.3) },
    },
    {
      key: "catch:cap",
      material: "BobberWhite",
      mesh: "sphere",
      transform: { position: addV3(at, v3(0, 0.2, 0)), rotation: spin, scale: v3(0.3, 0.18, 0.3) },
    },
  ];
};

const lossCatch = (kind: "leaf" | "duck" | "crab", at: EngineVec3, riseT: number, tick: number): readonly SceneInstance[] => {
  if (kind === "leaf") {
    return [
      {
        key: "catch:leaf",
        material: "LeafGreen",
        mesh: "box",
        transform: { position: at, rotation: quatYaw(tick * 0.04), scale: v3(0.42, 0.02, 0.26) },
      },
    ];
  }
  if (kind === "duck") {
    return [
      {
        key: "catch:duckbody",
        material: "DuckYellow",
        mesh: "sphere",
        transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(0.34, 0.26, 0.3) },
      },
      {
        key: "catch:duckhead",
        material: "DuckYellow",
        mesh: "sphere",
        transform: { position: addV3(at, v3(0.13, 0.2, 0)), rotation: QUAT_IDENTITY, scale: v3(0.18, 0.18, 0.18) },
      },
      {
        key: "catch:duckbeak",
        material: "DuckBeak",
        mesh: "box",
        transform: { position: addV3(at, v3(0.25, 0.18, 0)), rotation: QUAT_IDENTITY, scale: v3(0.1, 0.04, 0.06) },
      },
    ];
  }
  // The crab waves a claw, then (via the pulse-shaped rise) plops back in.
  const wave = Math.sin(riseT * Math.PI * 6) * 0.12;
  return [
    {
      key: "catch:crab",
      material: "CrabCoral",
      mesh: "box",
      transform: { position: at, rotation: quatRoll(wave * 0.4), scale: v3(0.36, 0.16, 0.26) },
    },
    {
      key: "catch:clawL",
      material: "CrabCoral",
      mesh: "sphere",
      transform: { position: addV3(at, v3(-0.24, 0.14 + wave, 0.06)), rotation: QUAT_IDENTITY, scale: v3(0.12, 0.12, 0.12) },
    },
    {
      key: "catch:clawR",
      material: "CrabCoral",
      mesh: "sphere",
      transform: { position: addV3(at, v3(0.24, 0.14 - wave, 0.06)), rotation: QUAT_IDENTITY, scale: v3(0.12, 0.12, 0.12) },
    },
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const fishingScene = (runtime: GameRuntime<FishingSpec>, state: FishingState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const tick = session.tick;
  const plan = session.committed;
  const tl = castTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? tl.total
        : -1;
  const aim = revealAge >= 0 ? committedAim(session) : { x: state.extra.aimX, z: state.extra.aimZ };
  const aimAt = v3(aim.x, WATER_Y, aim.z);

  // The pond, its bed, grass shore, lily pads, and reeds.
  const pond: SceneInstance[] = [
    flatRing("pond:grass", "Grass", v3(0, 0.005, 0), POND_RADIUS + 1.7, 0.03),
    flatRing("pond:bed", "PondBed", v3(0, 0.028, 0), POND_RADIUS + 0.28, 0.03),
    flatRing("pond:water", "PondWater", v3(0, WATER_Y, 0), POND_RADIUS, 0.05),
    ...[
      { r: 0.34, x: -0.5, z: 1.6 },
      { r: 0.26, x: -2.1, z: -0.7 },
      { r: 0.3, x: 0.2, z: -2.05 },
      { r: 0.22, x: 2.2, z: 0.15 },
    ].map((pad, i) => flatRing(`pond:lily${i}`, "LilyPad", v3(pad.x, WATER_Y + 0.03, pad.z), pad.r, 0.02)),
    ...[0, 1, 2, 3].map((i) => {
      const a = 0.6 + i * 0.32;
      const sway = Math.sin(tick * 0.03 + i * 1.7) * 0.05;
      return {
        key: `pond:reed${i}`,
        material: "Reed",
        mesh: "box",
        transform: {
          position: v3(2.35 + Math.cos(a) * 0.55 + sway, 0.42, 1.35 + Math.sin(a) * 0.5),
          rotation: quatRoll(sway * 2),
          scale: v3(0.05, 0.85, 0.05),
        },
      } satisfies SceneInstance;
    }),
  ];

  // Grey angular boulders around the grass shore (a far-left cluster + accents).
  const shore: SceneInstance[] = [
    ...rock("rock0", v3(-2.55, 0.5, -2.7), v3(1.5, 1.1, 1.3), 0.6),
    ...rock("rock1", v3(-1.6, 0.32, -2.95), v3(0.8, 0.7, 0.85), 2.1),
    ...rock("rock2", v3(-3.5, 0.36, 0.1), v3(0.95, 0.8, 1.05), 1.2),
    ...rock("rock3", v3(2.7, 0.42, -2.5), v3(1.05, 0.9, 1.0), 3.0),
    ...rock("rock4", v3(3.5, 0.32, 0.65), v3(0.85, 0.7, 0.9), 0.3),
  ];

  // Region ring buoys, highlighted while the reticle rests inside.
  const hot = session.phase === "ready" ? spec.regions.findIndex((r) => Math.hypot(aim.x - r.x, aim.z - r.z) <= r.radius) : -1;
  const regions: SceneInstance[] = spec.regions.flatMap((region, i) => {
    const ring = flatRing(`region${i}:ring`, hot === i ? "RegionRingLit" : "RegionRing", v3(region.x, WATER_Y + 0.02, region.z), region.radius, 0.012);
    const buoys = [0, 1, 2, 3].map((b) => {
      const a = (b / 4) * Math.PI * 2 + i * 0.5;
      return {
        key: `region${i}:buoy${b}`,
        material: "BobberRed",
        mesh: "sphere",
        transform: {
          position: v3(region.x + Math.cos(a) * region.radius, WATER_Y + 0.05 + Math.sin(tick * 0.06 + b + i) * 0.015, region.z + Math.sin(a) * region.radius),
          rotation: QUAT_IDENTITY,
          scale: v3(0.08, 0.08, 0.08),
        },
      } satisfies SceneInstance;
    });
    return [ring, ...buoys];
  });

  // Dock, rod (with the cast flick), and line down to the bobber.
  const flickT = revealAge >= 0 ? clamp01(revealAge / tl.flickEnd) : 0;
  const flick = revealAge >= 0 ? pulse(flickT) * 0.35 : 0;
  const rodBase = v3(0.85, 0.45, 3.55);
  const rodTip = v3(ROD_TIP.x, ROD_TIP.y + flick, ROD_TIP.z);
  const dock: SceneInstance[] = [
    {
      key: "dock:planks",
      material: "DockWood",
      mesh: "box",
      transform: { position: v3(0, 0.3, 3.6), rotation: QUAT_IDENTITY, scale: v3(2.6, 0.1, 1.2) },
    },
    {
      key: "dock:edge",
      material: "DockWoodDark",
      mesh: "box",
      transform: { position: v3(0, 0.26, 3.02), rotation: QUAT_IDENTITY, scale: v3(2.6, 0.18, 0.1) },
    },
    ...[-1, 1].map((s) => ({
      key: `dock:post${s > 0 ? "R" : "L"}`,
      material: "DockWoodDark",
      mesh: "box",
      transform: { position: v3(s * 1.15, 0.12, 3.08), rotation: QUAT_IDENTITY, scale: v3(0.14, 0.5, 0.14) },
    })),
    segment("rod", "RodWood", rodBase, rodTip, 0.045),
  ];

  // Bobber + line: idle by the dock before the cast, flying/floating after.
  const bobber = revealAge >= 0 ? bobberAt(aim, tl, revealAge) : v3(0.4, WATER_Y + 0.02, 2.6);
  const showBobber = revealAge < 0 || revealAge <= tl.reelEnd;
  const tackle: SceneInstance[] = showBobber
    ? [
        segment("line", "LineSilk", rodTip, bobber, 0.014),
        {
          key: "bobber:top",
          material: "BobberRed",
          mesh: "sphere",
          transform: { position: v3(bobber.x, bobber.y + 0.05, bobber.z), rotation: QUAT_IDENTITY, scale: v3(0.11, 0.09, 0.11) },
        },
        {
          key: "bobber:base",
          material: "BobberWhite",
          mesh: "sphere",
          transform: { position: bobber, rotation: QUAT_IDENTITY, scale: v3(0.1, 0.1, 0.1) },
        },
      ]
    : [segment("line", "LineSilk", rodTip, v3(CATCH_POINT.x, Math.max(bobberCatchY(plan, tl, revealAge), WATER_Y), CATCH_POINT.z), 0.014)];

  // Aiming reticle (ready only).
  const reticle: SceneInstance[] =
    session.phase === "ready"
      ? [
          flatRing("reticle:ring", "Reticle", v3(aimAt.x, WATER_Y + 0.05, aimAt.z), 0.24 + pulse((tick % 40) / 40) * 0.03, 0.012),
          {
            key: "reticle:dot",
            material: "Reticle",
            mesh: "sphere",
            transform: { position: v3(aimAt.x, WATER_Y + 0.07, aimAt.z), rotation: QUAT_IDENTITY, scale: v3(0.06, 0.06, 0.06) },
          },
        ]
      : [];

  // Water theater: splash on landing, ripples, dip ripple, reel churn.
  const seed = plan?.presentationSeed ?? session.seed;
  const water: SceneInstance[] = [];
  if (revealAge >= 0) {
    water.push(...splashDroplets(aimAt, seed, revealAge - tl.flightEnd, 24));
    water.push(...ripples("ripple", aimAt, revealAge - tl.flightEnd));
    water.push(...ripples("dipripple", aimAt, revealAge - tl.dipStart, 2));
    const reelT = clamp01((revealAge - tl.dipEnd) / (tl.reelEnd - tl.dipEnd));
    if (reelT > 0 && reelT < 1) {
      water.push(
        ...[0, 1, 2, 3, 4].map((i) => {
          const a = sample01(seed, "particles", i, 30) * Math.PI * 2 + revealAge * 0.2;
          const r = 0.12 + sample01(seed, "particles", i, 31) * 0.14;
          const size = 0.035 + sample01(seed, "particles", i, 32) * 0.02;
          return {
            key: `churn:${i}`,
            material: "Droplet",
            mesh: "sphere",
            transform: {
              position: v3(bobber.x + Math.cos(a) * r, WATER_Y + 0.05 + Math.abs(Math.sin(a * 2)) * 0.06, bobber.z + Math.sin(a) * r),
              rotation: QUAT_IDENTITY,
              scale: v3(size, size, size),
            },
          } satisfies SceneInstance;
        }),
      );
    }
  }

  // The catch, rising at the dock once the reel-in completes.
  const rewardInstances: SceneInstance[] = [];
  if (plan !== null && revealAge >= tl.reelEnd) {
    const riseT = clamp01((revealAge - tl.reelEnd) / (tl.riseEnd - tl.reelEnd));
    const rarity = outcomeRarity(session);
    if (plan.win && rarity !== "loss") {
      const family = familyOfRegion(plan.manifestation.kind === "single" ? plan.manifestation.focusIndex : 0);
      const at = v3(CATCH_POINT.x, WATER_Y + easeOutBack(riseT) * 0.85, CATCH_POINT.z);
      rewardInstances.push(...winCatch(family, rewardMaterialOf(rarity), at, quatYaw(tick * 0.03)));
      if (celebrationFor(runtime.settings, session).beam) {
        rewardInstances.push(rewardBeam("catch:beam", v3(CATCH_POINT.x, WATER_Y, CATCH_POINT.z), riseT));
      }
    } else {
      const kind = lossCatchOf(plan.presentationSeed);
      // The crab's rise is pulse-shaped (up and plop back); leaf/duck float up.
      const lift = kind === "crab" ? pulse(riseT) * 0.55 : easeOutBack(riseT) * 0.4;
      rewardInstances.push(...lossCatch(kind, v3(CATCH_POINT.x, WATER_Y + lift, CATCH_POINT.z), riseT, tick));
      if (kind === "crab" && riseT >= 0.96) {
        rewardInstances.push(...ripples("plop", v3(CATCH_POINT.x, WATER_Y, CATCH_POINT.z), revealAge - tl.riseEnd, 2));
      }
    }
  }

  // Celebration at the dock.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = v3(CATCH_POINT.x, WATER_Y + 1.1, CATCH_POINT.z);
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Fishing Cast is a moody dusk pond, not the bright pavilion: a strong warm
  // key dropped to a low, side-raking elevation models the water and shore, and
  // the cool fill is cut hard so the darks stay deep and saturated instead of
  // washing flat. The warm focus point stays on the reveal at the dock.
  const lights: SceneLight[] = [
    { key: "light:key", light: { color: [1, 0.9, 0.72, 1], direction: v3(-0.5, -0.5, -0.34), intensity: 1.05, kind: "directional" } },
    { key: "light:fill", light: { color: [0.6, 0.74, 0.95, 1], direction: v3(0.5, -0.35, 0.75), intensity: 0.12, kind: "directional" } },
    { key: "light:focus", light: { color: [1, 0.9, 0.66, 1], intensity: 0.7, kind: "point", position: v3(CATCH_POINT.x, 2, CATCH_POINT.z + 0.6) } },
  ];

  return {
    camera: showcaseCamera(v3(0, -0.1, 0.2), 5.2, 3.1, 0.86),
    clearColor: NIGHT_SKY,
    instances: [...stageRoom(19), ...pond, ...shore, ...regions, ...dock, ...tackle, ...reticle, ...water, ...rewardInstances, ...celebration],
    lights,
  };
};

/** Where the line ends while the catch rises (tracks the catch's lift). */
const bobberCatchY = (
  plan: FishingState["session"]["committed"],
  tl: ReturnType<typeof castTimeline>,
  revealAge: number,
): number => {
  if (plan === null) {
    return WATER_Y;
  }
  const riseT = clamp01((revealAge - tl.reelEnd) / (tl.riseEnd - tl.reelEnd));
  const lossKind = plan.win ? null : lossCatchOf(plan.presentationSeed);
  const lift = plan.win ? easeOutBack(riseT) * 0.85 : lossKind === "crab" ? pulse(riseT) * 0.55 : easeOutBack(riseT) * 0.4;
  return WATER_Y + lift + 0.15;
};
