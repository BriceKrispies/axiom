/*
 * scene.ts — Lucky Lanterns presentation: a bright twilight sky (lavender-blue
 * clear + a warm horizon band), a release platform with a railing, pastel
 * pagoda silhouettes, and a drift of ambient lanterns. The player's lantern
 * rises through the sky, its paper glow and a tinted point light gradually
 * shifting toward the committed band's color across a few height thresholds;
 * it settles into its band, which softly brightens, then blossoms (win) or
 * bows and drifts off (loss). Pure view.
 */

import type { EngineVec3, GameResources, MaterialSpec, Rgba, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { Rarity } from "../../chance-engine/configuration/schema.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, rewardBeam, REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack, lerp, pulse } from "../../presentation/stage/easing.ts";
import { stageRoom, STAGE_MATERIALS } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { LanternSpec, LanternState } from "./game.ts";
import {
  bandRange,
  committedBandIndex,
  lanternHeightAt,
  lanternSwayAt,
  PLATFORM_Y,
  riseFraction,
  riseTimeline,
} from "./game.ts";

// ── twilight palette ────────────────────────────────────────────────────────────

/** A lighter twilight: cheerful lavender-blue, never dark. */
const TWILIGHT_CLEAR: Rgba = [0.44, 0.42, 0.66, 1];
const WARM_CREAM: Rgba = [1, 0.9, 0.68, 1];
const HORIZON_Y = 2.2;

const glowMaterial = (color: Rgba, strength: number): MaterialSpec => ({
  baseColor: color,
  emissive: [color[0] * strength, color[1] * strength, color[2] * strength, 1],
});

const sheetMaterial = (color: Rgba): MaterialSpec => ({
  baseColor: color,
  emissive: [color[0] * 0.35, color[1] * 0.35, color[2] * 0.35, 1],
  opacity: 0.22,
});

/** The band's accent color, from its tier's rarity (null → pale lavender). */
const bandColorOf = (runtime: GameRuntime<LanternSpec>, tierId: string | null): Rgba => {
  const tier = runtime.config.rewardTiers.find((t) => t.id === tierId);
  return tier === undefined ? [0.82, 0.8, 0.92, 1] : RARITY_COLORS[tier.rarity];
};

/** Per-mount materials: three lantern-glow stops per band + a band sky sheet. */
export const lanternResources = (runtime: GameRuntime<LanternSpec>): GameResources => {
  const spec = runtime.config.gameSpecific;
  const materials: Record<string, MaterialSpec> = {
    ...STAGE_MATERIALS,
    ...REWARD_MATERIALS,
    ...CONFETTI_MATERIALS,
    Firefly: { baseColor: [1, 0.95, 0.6, 1], emissive: [0.9, 0.82, 0.4, 1] },
    HorizonWarm: { baseColor: [1, 0.72, 0.5, 1], emissive: [0.55, 0.32, 0.2, 1] },
    LanternPaper: glowMaterial(WARM_CREAM, 0.7),
    LanternWarm: glowMaterial(WARM_CREAM, 0.7),
    PagodaDusk: { baseColor: [0.5, 0.46, 0.66, 1], emissive: [0.14, 0.12, 0.2, 1] },
    PagodaDuskDark: { baseColor: [0.4, 0.36, 0.56, 1] },
    PlatformDeck: { baseColor: [0.7, 0.6, 0.72, 1] },
    PlatformRail: { baseColor: [0.86, 0.78, 0.9, 1], emissive: [0.2, 0.16, 0.24, 1] },
  };
  spec.bands.forEach((band, i) => {
    const color = bandColorOf(runtime, band.tierId);
    materials[`band${i}`] = sheetMaterial(color);
    materials[`lglow${i}_1`] = glowMaterial([lerp(WARM_CREAM[0], color[0], 0.5), lerp(WARM_CREAM[1], color[1], 0.5), lerp(WARM_CREAM[2], color[2], 0.5), 1], 0.75);
    materials[`lglow${i}_2`] = glowMaterial(color, 0.9);
  });
  return { materials, meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } } };
};

const flatSheet = (key: string, material: string, y: number, width: number, height: number, z: number): SceneInstance => ({
  key,
  material,
  mesh: "box",
  transform: { position: v3(0, y, z), rotation: QUAT_IDENTITY, scale: v3(width, height, 0.1) },
});

/** A single paper lantern (glowing box body + a small flame bead) at `at`. */
const lantern = (keyPrefix: string, at: EngineVec3, glowMat: string, scale: number, flameFlicker: number): readonly SceneInstance[] => [
  { key: `${keyPrefix}:body`, material: glowMat, mesh: "box", transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(0.32 * scale, 0.4 * scale, 0.32 * scale) } },
  { key: `${keyPrefix}:cap`, material: "PlatformRail", mesh: "box", transform: { position: addV3(at, v3(0, 0.24 * scale, 0)), rotation: QUAT_IDENTITY, scale: v3(0.36 * scale, 0.05 * scale, 0.36 * scale) } },
  { key: `${keyPrefix}:flame`, material: "Firefly", mesh: "sphere", transform: { position: addV3(at, v3(0, -0.05 * scale, 0)), rotation: QUAT_IDENTITY, scale: v3(0.08 * scale * flameFlicker, 0.12 * scale * flameFlicker, 0.08 * scale) } },
];

// ── the scene ───────────────────────────────────────────────────────────────────

export const lanternScene = (runtime: GameRuntime<LanternSpec>, state: LanternState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const tick = session.tick;
  const plan = session.committed;
  const tl = riseTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? tl.total
        : -1;

  const bandCount = spec.bands.length;
  const bandIndex = committedBandIndex(session);
  const range = bandRange(bandCount, bandIndex);
  const seed = plan?.presentationSeed ?? session.seed;

  const flame = 0.85 + pulse((tick % 24) / 24) * 0.35;
  const rise = revealAge >= 0 ? riseFraction(tl, revealAge) : 0;
  const lanternY = revealAge >= 0 ? lanternHeightAt(range, tl, revealAge) : PLATFORM_Y + 0.5 + pulse((tick % 60) / 60) * 0.05;
  const sway = revealAge >= 0 ? lanternSwayAt(seed, revealAge) : 0;
  const lanternPos = v3(sway, lanternY, 0);

  // Band sky sheets: the committed band brightens once the lantern arrives.
  const bandSettled = plan !== null && revealAge >= tl.riseEnd;
  const bands: SceneInstance[] = spec.bands.map((_, i) => {
    const r = bandRange(bandCount, i);
    const isCommitted = i === bandIndex;
    const brighten = isCommitted && bandSettled ? clamp01((revealAge - tl.riseEnd) / (tl.brightenEnd - tl.riseEnd)) : 0;
    return {
      key: `sky:band${i}`,
      material: `band${i}`,
      mesh: "box",
      transform: { position: v3(0, r.center, -6), rotation: QUAT_IDENTITY, scale: v3(24, (r.high - r.low) * (0.7 + brighten * 0.5), 0.1) },
    } satisfies SceneInstance;
  });

  // Warm horizon glow band low in the sky.
  const horizon: SceneInstance[] = [flatSheet("sky:horizon", "HorizonWarm", HORIZON_Y, 30, 2.6, -6.5)];

  // Distant pagoda silhouettes.
  const pagodas: SceneInstance[] = [-7, -3.5, 4, 7.5].map((x, i) => ({
    key: `pagoda${i}`,
    material: i % 2 === 0 ? "PagodaDusk" : "PagodaDuskDark",
    mesh: "box",
    transform: { position: v3(x, 1.2 + (i % 3) * 0.4, -5.6), rotation: QUAT_IDENTITY, scale: v3(0.9, 2.4 + (i % 3) * 0.8, 0.9) },
  }));

  // The platform + railing.
  const platform: SceneInstance[] = [
    { key: "deck", material: "PlatformDeck", mesh: "cylinder", transform: { position: v3(0, PLATFORM_Y - 0.4, 0), rotation: QUAT_IDENTITY, scale: v3(3.4, 0.3, 3.4) } },
    ...[0, 1, 2, 3, 4, 5].map((i) => {
      const a = (i / 6) * Math.PI * 2;
      return {
        key: `rail${i}`,
        material: "PlatformRail",
        mesh: "box",
        transform: { position: v3(Math.cos(a) * 1.5, PLATFORM_Y - 0.05, Math.sin(a) * 1.5), rotation: QUAT_IDENTITY, scale: v3(0.08, 0.5, 0.08) },
      } satisfies SceneInstance;
    }),
  ];

  // Ambient lanterns already drifting (AMBIENT stream — never the outcome).
  const ambient: SceneInstance[] = Array.from({ length: 6 }, (_, i) => {
    const baseX = (sample01(session.seed, "ambient", i, 0) - 0.5) * 14;
    const baseZ = -2 - sample01(session.seed, "ambient", i, 1) * 4;
    const climb = ((tick * 0.01 + sample01(session.seed, "ambient", i, 2) * 8) % 10);
    const drift = Math.sin(tick * 0.02 + i) * 0.5;
    return lantern(`amb${i}`, v3(baseX + drift, 3 + climb, baseZ), "LanternWarm", 0.7, 0.9 + Math.sin(tick * 0.1 + i) * 0.2);
  }).flat();

  // The player's lantern: paper glow shifts toward the band color by height.
  const glowMat =
    revealAge < 0 || rise < 0.4 ? "LanternPaper" : rise < 0.75 ? `lglow${bandIndex}_1` : `lglow${bandIndex}_2`;
  const blossomT = plan !== null && plan.win && revealAge >= tl.brightenEnd ? clamp01((revealAge - tl.brightenEnd) / (tl.blossomEnd - tl.brightenEnd)) : 0;
  const bowT = plan !== null && !plan.win && revealAge >= tl.brightenEnd ? clamp01((revealAge - tl.brightenEnd) / (tl.blossomEnd - tl.brightenEnd)) : 0;
  const lanternScale = 1 + easeOutBack(blossomT) * 0.7;
  const bow = pulse(bowT) * 0.4;
  const player: readonly SceneInstance[] = lantern("player", lanternPos, glowMat, lanternScale, flame);
  // A gentle bow tilt on a loss: re-pose the paper via a tilted duplicate cap.
  const playerTilt: SceneInstance[] =
    bowT > 0
      ? [{ key: "player:bowcap", material: "PlatformRail", mesh: "box", transform: { position: addV3(lanternPos, v3(0, 0.24 * lanternScale, 0)), rotation: quatPitch(bow), scale: v3(0.36, 0.05, 0.36) } }]
      : [];

  // Trail sparkles + fireflies shifting toward the band color as it climbs.
  const trailMat = revealAge < 0 || rise < 0.4 ? "Firefly" : rise < 0.75 ? `lglow${bandIndex}_1` : `lglow${bandIndex}_2`;
  const trail: SceneInstance[] =
    revealAge >= 0 && revealAge <= tl.riseEnd
      ? Array.from({ length: 6 }, (_, i) => {
          const backAge = revealAge - i * 6;
          if (backAge < 0) {
            return null;
          }
          const y = lanternHeightAt(range, tl, backAge);
          const x = lanternSwayAt(seed, backAge);
          const size = 0.05 * (1 - i / 6) + 0.02;
          return {
            key: `trail${i}`,
            material: trailMat,
            mesh: "sphere",
            transform: { position: v3(x + (sample01(seed, "trajectory", 10 + i) - 0.5) * 0.3, y - 0.3 - i * 0.15, 0.1), rotation: QUAT_IDENTITY, scale: v3(size, size, size) },
          } satisfies SceneInstance;
        }).filter((s): s is SceneInstance => s !== null)
      : [];

  // Win blossom reward + beam.
  const rewardInstances: SceneInstance[] = [];
  const rarity = outcomeRarity(session);
  if (plan !== null && plan.win && rarity !== "loss" && revealAge >= tl.brightenEnd) {
    rewardInstances.push(...rewardProp("reward", rarity, addV3(lanternPos, v3(0, 0.2, 0)), blossomT, tick));
    if (celebrationFor(runtime.settings, session).beam) {
      rewardInstances.push(rewardBeam("reward:beam", v3(lanternPos.x, lanternPos.y - 0.3, 0), blossomT, 1.8));
    }
  }
  // Loss: a few warm sparkles as it releases and drifts.
  if (plan !== null && !plan.win && revealAge >= tl.brightenEnd) {
    rewardInstances.push(...sparkleRing("driftdust", lanternPos, 6, plan.presentationSeed, revealAge - tl.brightenEnd, 50));
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", lanternPos, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", lanternPos, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera: follow the lantern up, then ease focus back down to the platform.
  const followT = revealAge >= 0 ? clamp01(revealAge / tl.riseEnd) : 0;
  const returnT = revealAge >= tl.blossomEnd ? clamp01((revealAge - tl.blossomEnd) / Math.max(1, tl.settleEnd - tl.blossomEnd)) : 0;
  const focusUp = lerp(PLATFORM_Y + 1.2, lanternY - 0.6, followT * 0.85);
  const focusY = lerp(focusUp, PLATFORM_Y + 1.2, returnT);
  const camera = showcaseCamera(v3(0, focusY, 0), 10.5, 1.6, 0.9);

  // A tinted point light re-posed onto the lantern, colored toward the band.
  const bandColor = bandColorOf(runtime, spec.bands[bandIndex]?.tierId ?? null);
  const tint: Rgba = [lerp(WARM_CREAM[0], bandColor[0], rise), lerp(WARM_CREAM[1], bandColor[1], rise), lerp(WARM_CREAM[2], bandColor[2], rise), 1];
  const lights: SceneLight[] = [
    { key: "light:sky", light: { color: [0.7, 0.72, 0.9, 1], direction: v3(-0.3, -0.7, -0.4), intensity: 0.7, kind: "directional" } },
    { key: "light:warm", light: { color: [1, 0.78, 0.6, 1], direction: v3(0.4, -0.3, 0.6), intensity: 0.4, kind: "directional" } },
    { key: "light:lantern", light: { color: tint, intensity: 1.1 + rise * 0.6, kind: "point", position: v3(lanternPos.x, lanternPos.y, lanternPos.z + 0.5) } },
  ];

  return {
    camera,
    clearColor: TWILIGHT_CLEAR,
    instances: [...stageRoom(24).slice(0, 1), ...horizon, ...bands, ...pagodas, ...platform, ...ambient, ...player, ...playerTilt, ...trail, ...rewardInstances, ...celebration],
    lights,
  };
};
