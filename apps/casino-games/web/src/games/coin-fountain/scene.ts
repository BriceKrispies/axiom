/*
 * scene.ts — Coin Fountain presentation: a tiered pastel fountain (stacked
 * cylinders) crowned by a cheerful statue, translucent water sheets, and a
 * floating glow ring in the basin. The player aims a bright token, charges the
 * arc, and tosses; the token flies a continuous ballistic arc, splashes with
 * bounded droplets and ripple rings, then the reveal plays: a splash column, a
 * tier-colored bowl glow, and a reward rising on a spout (win), or a friendly
 * blue shimmer and a statue nod (loss). Pure view.
 */

import type { EngineVec3, GameResources, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { rewardBeam, REWARD_MATERIALS, rewardMaterialOf, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, easeOutCubic, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { FountainSpec, FountainState } from "./game.ts";
import {
  chargeStrength,
  committedAim,
  committedStrength,
  LEDGE,
  tokenAt,
  tossTimeline,
  WATER_Y,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  BasinWater: { baseColor: [0.35, 0.68, 0.82, 1], emissive: [0.06, 0.16, 0.2, 1], opacity: 0.8 },
  Droplet: { baseColor: [0.78, 0.92, 0.98, 1], emissive: [0.24, 0.34, 0.4, 1], opacity: 0.85 },
  FountainStone: { baseColor: [0.86, 0.82, 0.92, 1] },
  FountainStoneDark: { baseColor: [0.68, 0.64, 0.78, 1] },
  GlowRing: { baseColor: [0.6, 0.9, 1, 1], emissive: [0.4, 0.7, 0.9, 1], opacity: 0.5 },
  Reticle: { baseColor: [1, 0.85, 0.4, 1], emissive: [0.65, 0.5, 0.15, 1], opacity: 0.8 },
  Ripple: { baseColor: [0.88, 0.96, 1, 1], opacity: 0.4 },
  StatueBody: { baseColor: [0.9, 0.86, 0.78, 1], emissive: [0.1, 0.09, 0.07, 1] },
  Token: { baseColor: [1, 0.85, 0.35, 1], emissive: [0.6, 0.48, 0.14, 1] },
  WaterSheet: { baseColor: [0.7, 0.9, 0.98, 1], opacity: 0.35 },
};

export const FOUNTAIN_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

const flatRing = (key: string, material: string, at: EngineVec3, radius: number, height = 0.02): SceneInstance => ({
  key,
  material,
  mesh: "cylinder",
  transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(radius * 2, height, radius * 2) },
});

/** Expanding, staggered splash ripples (bounded ring count). */
const ripples = (keyPrefix: string, at: EngineVec3, age: number, count = 3): readonly SceneInstance[] =>
  Array.from({ length: count }, (_, i) => {
    const local = age - i * 8;
    const life = 40;
    if (local < 0 || local > life) {
      return null;
    }
    const t = local / life;
    return flatRing(`${keyPrefix}:${i}`, "Ripple", v3(at.x, WATER_Y + 0.02 + i * 0.004, at.z), 0.18 + t * 1.0, 0.012);
  }).filter((r): r is SceneInstance => r !== null);

/** A bounded ring of splash droplets on ballistic arcs (12 spheres). */
const splashDroplets = (at: EngineVec3, seed: number, age: number, life: number): readonly SceneInstance[] => {
  if (age < 0 || age > life) {
    return [];
  }
  const t = age / 26;
  return Array.from({ length: 12 }, (_, i) => {
    const angle = (i / 12) * Math.PI * 2 + sample01(seed, "particles", i, 40) * 0.5;
    const speed = 0.5 + sample01(seed, "particles", i, 41) * 0.8;
    const up = 1.4 + sample01(seed, "particles", i, 42) * 1.0;
    const y = at.y + up * t - 3.6 * t * t;
    if (y < WATER_Y - 0.02) {
      return null;
    }
    const size = 0.05 + sample01(seed, "particles", i, 43) * 0.035;
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

// ── the scene ───────────────────────────────────────────────────────────────────

export const fountainScene = (runtime: GameRuntime<FountainSpec>, state: FountainState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const tick = session.tick;
  const plan = session.committed;
  const tl = tossTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? tl.total
        : -1;
  const aim = revealAge >= 0 ? committedAim(spec, session) : { x: state.extra.aimX, z: state.extra.aimZ };
  const strength = revealAge >= 0 ? committedStrength(session) : chargeStrength(state.extra.chargeTicks);
  const rarity = outcomeRarity(session);
  const bowlLit = plan !== null && plan.win && revealAge >= tl.splashEnd;

  // Basin, tiers, water sheets, glow ring.
  const basin: SceneInstance[] = [
    flatRing("f:basin", "FountainStone", v3(0, 0.3, 0), spec.basinRadius + 0.42, 0.6),
    flatRing("f:basinlip", "FountainStoneDark", v3(0, 0.6, 0), spec.basinRadius + 0.42, 0.06),
    flatRing("f:water", bowlLit ? "GlowRing" : "BasinWater", v3(0, WATER_Y, 0), spec.basinRadius, 0.06),
    flatRing("f:tier1", "FountainStone", v3(0, 1.0, 0), spec.basinRadius * 0.62, 0.8),
    flatRing("f:tier1water", "WaterSheet", v3(0, WATER_Y + 0.02, 0), spec.basinRadius * 0.9, 0.02),
    flatRing("f:tier2", "FountainStoneDark", v3(0, 1.55, 0), spec.basinRadius * 0.34, 0.5),
    flatRing("f:tier2bowl", bowlLit ? "GlowRing" : "BasinWater", v3(0, 1.82, 0), spec.basinRadius * 0.3, 0.05),
    // The floating glow ring drifting in the basin (ambient bob).
    flatRing("f:glowring", "GlowRing", v3(Math.cos(tick * 0.02) * 0.3, WATER_Y + 0.08 + Math.sin(tick * 0.05) * 0.03, Math.sin(tick * 0.02) * 0.3), 0.55, 0.03),
  ];

  // Water sheets spilling from the upper bowls (thin translucent cylinders).
  const sheets: SceneInstance[] = [0, 1, 2, 3].map((i) => {
    const a = (i / 4) * Math.PI * 2 + tick * 0.01;
    return {
      key: `f:sheet${i}`,
      material: "WaterSheet",
      mesh: "cylinder",
      transform: { position: v3(Math.cos(a) * spec.basinRadius * 0.32, 1.3, Math.sin(a) * spec.basinRadius * 0.32), rotation: QUAT_IDENTITY, scale: v3(0.14, 0.6, 0.14) },
    } satisfies SceneInstance;
  });

  // The cheerful statue on top: sphere head + tapered box body, arms, a bowl.
  const nod = plan !== null && !plan.win && revealAge >= tl.splashEnd ? pulse(clamp01((revealAge - tl.splashEnd) / 30)) * 0.18 : 0;
  const statueBase = v3(0, 1.95, 0);
  const statue: SceneInstance[] = [
    { key: "st:body", material: "StatueBody", mesh: "box", transform: { position: addV3(statueBase, v3(0, 0.32, 0)), rotation: quatPitch(nod), scale: v3(0.42, 0.64, 0.4) } },
    { key: "st:waist", material: "StatueBody", mesh: "box", transform: { position: addV3(statueBase, v3(0, 0.05, 0)), rotation: QUAT_IDENTITY, scale: v3(0.5, 0.24, 0.46) } },
    { key: "st:head", material: "StatueBody", mesh: "sphere", transform: { position: addV3(statueBase, v3(0, 0.78 + nod * 0.1, 0.02)), rotation: quatPitch(nod), scale: v3(0.34, 0.34, 0.34) } },
    { key: "st:armL", material: "StatueBody", mesh: "box", transform: { position: addV3(statueBase, v3(-0.32, 0.4, 0.1)), rotation: quatPitch(-0.5), scale: v3(0.12, 0.4, 0.12) } },
    { key: "st:armR", material: "StatueBody", mesh: "box", transform: { position: addV3(statueBase, v3(0.32, 0.4, 0.1)), rotation: quatPitch(-0.5), scale: v3(0.12, 0.4, 0.12) } },
    { key: "st:bowl", material: bowlLit ? rewardMaterialOf(rarity === "loss" ? "common" : rarity) : "FountainStone", mesh: "cylinder", transform: { position: addV3(statueBase, v3(0, 0.6, 0.34)), rotation: QUAT_IDENTITY, scale: v3(0.36, 0.1, 0.36) } },
  ];

  // The tossing ledge (a small stone lip toward the camera).
  const ledge: SceneInstance[] = [
    { key: "f:ledge", material: "FountainStoneDark", mesh: "box", transform: { position: v3(LEDGE.x, LEDGE.y - 0.2, LEDGE.z), rotation: QUAT_IDENTITY, scale: v3(1.4, 0.2, 0.4) } },
  ];

  // The token: in hand at the ledge before the toss, mid-arc during flight.
  const token: SceneInstance[] = [];
  if (session.phase === "ready") {
    token.push({
      key: "token",
      material: "Token",
      mesh: "cylinder",
      transform: { position: v3(LEDGE.x, LEDGE.y + 0.05 + pulse((tick % 40) / 40) * 0.03, LEDGE.z), rotation: quatPitch(Math.PI / 2), scale: v3(0.22, 0.05, 0.22) },
    });
  } else if (revealAge >= 0 && revealAge <= tl.flightEnd) {
    const pos = tokenAt(spec, aim, strength, tl, revealAge);
    token.push({
      key: "token",
      material: "Token",
      mesh: "cylinder",
      transform: { position: pos, rotation: quatYaw(revealAge * 0.4), scale: v3(0.22, 0.05, 0.22) },
    });
  }

  // Aiming reticle + charge meter (ready only).
  const aimUi: SceneInstance[] = [];
  if (session.phase === "ready") {
    aimUi.push(
      flatRing("reticle:ring", "Reticle", v3(aim.x, WATER_Y + 0.05, aim.z), 0.26 + pulse((tick % 40) / 40) * 0.03, 0.012),
      { key: "reticle:dot", material: "Reticle", mesh: "sphere", transform: { position: v3(aim.x, WATER_Y + 0.08, aim.z), rotation: QUAT_IDENTITY, scale: v3(0.06, 0.06, 0.06) } },
    );
    if (strength > 0) {
      // A charge bead climbing over the ledge to show arc height.
      aimUi.push({
        key: "charge:bead",
        material: "Token",
        mesh: "sphere",
        transform: { position: v3(LEDGE.x + 0.5, LEDGE.y + 0.2 + strength * 0.9, LEDGE.z), rotation: QUAT_IDENTITY, scale: v3(0.08 + strength * 0.05, 0.08 + strength * 0.05, 0.08) },
      });
    }
  }

  // Water theater: splash + ripples, and the splash column on a win.
  const seed = plan?.presentationSeed ?? session.seed;
  const aimAt = v3(aim.x, WATER_Y, aim.z);
  const water: SceneInstance[] = [];
  if (revealAge >= tl.flightEnd) {
    water.push(...splashDroplets(aimAt, seed, revealAge - tl.flightEnd, 26));
    water.push(...ripples("ripple", aimAt, revealAge - tl.flightEnd));
    if (plan !== null && plan.win && revealAge < tl.columnEnd) {
      const colT = clamp01((revealAge - tl.splashEnd) / (tl.columnEnd - tl.splashEnd));
      water.push({
        key: "column",
        material: "Droplet",
        mesh: "cylinder",
        transform: { position: v3(0, WATER_Y + easeOutCubic(colT) * 0.9, 0), rotation: QUAT_IDENTITY, scale: v3(0.4, easeOutCubic(colT) * 1.8, 0.4) },
      });
    }
  }

  // The reward rising on a water spout at the fountain center (win only).
  const rewardInstances: SceneInstance[] = [];
  if (plan !== null && revealAge >= tl.columnEnd) {
    const riseT = clamp01((revealAge - tl.columnEnd) / (tl.riseEnd - tl.columnEnd));
    if (plan.win && rarity !== "loss") {
      const at = v3(0, WATER_Y + 0.4 + easeOutBack(riseT) * 0.9, 0);
      rewardInstances.push(...rewardProp("reward", rarity, at, riseT, tick));
      rewardInstances.push({
        key: "spout",
        material: "WaterSheet",
        mesh: "cylinder",
        transform: { position: v3(0, WATER_Y + (at.y - WATER_Y) / 2, 0), rotation: QUAT_IDENTITY, scale: v3(0.22, at.y - WATER_Y, 0.22) },
      });
      if (celebrationFor(runtime.settings, session).beam) {
        rewardInstances.push(rewardBeam("reward:beam", v3(0, WATER_Y, 0), riseT));
      }
    } else {
      // A gentle blue shimmer over the water — the warm try-again.
      rewardInstances.push(...sparkleRing("shimmer", v3(0, WATER_Y + 0.1, 0), 8, plan.presentationSeed, revealAge - tl.columnEnd, 60));
    }
  }

  // Celebration above the basin.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = v3(0, WATER_Y + 1.4, 0);
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  const lights: SceneLight[] = [...stageLights(v3(0, WATER_Y + 0.5, 0), 0.6)];
  if (bowlLit) {
    lights.push({ key: "light:bowl", light: { color: [0.7, 0.92, 1, 1], intensity: 1.2, kind: "point", position: v3(0, WATER_Y + 0.4, 0) } });
  }

  return {
    camera: showcaseCamera(v3(0, 1.35, 0), 7.6, 1.7, 0.82),
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(20), ...basin, ...sheets, ...statue, ...ledge, ...token, ...aimUi, ...water, ...rewardInstances, ...celebration],
    lights,
  };
};
