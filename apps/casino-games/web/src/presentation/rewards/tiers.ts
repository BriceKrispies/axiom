/*
 * tiers.ts — the shared reward-presentation language: rarity colors, reward
 * materials, the floating reward prop revealed by every game, and the result
 * banner text. One vocabulary so a rare reward reads as rare in every game.
 */

import type { MaterialSpec, Rgba, SceneInstance } from "@axiom/web-engine";
import type { EngineVec3 } from "@axiom/web-engine";
import type { OutcomePlan } from "../../chance-engine/outcomes/plan.ts";
import type { Rarity } from "../../chance-engine/configuration/schema.ts";
import { easeOutBack, progress } from "../stage/easing.ts";
import { quatYaw, v3 } from "../stage/vectors.ts";

/** Rarity accent colors (UI + materials agree on these). */
export const RARITY_COLORS: Readonly<Record<Rarity, Rgba>> = {
  common: [0.55, 0.85, 1, 1],
  jackpot: [1, 0.78, 0.25, 1],
  rare: [0.85, 0.55, 1, 1],
  uncommon: [0.45, 1, 0.7, 1],
};

export const RARITY_CSS: Readonly<Record<Rarity, string>> = {
  common: "#8cd9ff",
  jackpot: "#ffc740",
  rare: "#d98cff",
  uncommon: "#73ffb3",
};

const glow = (color: Rgba, strength: number): MaterialSpec => ({
  baseColor: color,
  emissive: [color[0] * strength, color[1] * strength, color[2] * strength, 1],
});

/** Spread into a game's materials to use `rewardProp` / `rewardBeam`. */
export const REWARD_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  RewardBeam: { baseColor: [1, 0.95, 0.7, 1], emissive: [0.9, 0.8, 0.45, 1], opacity: 0.28 },
  RewardCommon: glow(RARITY_COLORS.common, 0.55),
  RewardJackpot: glow(RARITY_COLORS.jackpot, 0.85),
  RewardRare: glow(RARITY_COLORS.rare, 0.7),
  RewardUncommon: glow(RARITY_COLORS.uncommon, 0.6),
  TryAgain: { baseColor: [0.75, 0.8, 0.9, 1], emissive: [0.22, 0.24, 0.3, 1] },
};

export const rewardMaterialOf = (rarity: Rarity): string =>
  ({ common: "RewardCommon", jackpot: "RewardJackpot", rare: "RewardRare", uncommon: "RewardUncommon" })[rarity];

/**
 * The reward prop rising out of a reveal: a spinning rarity-colored gem
 * (sphere + box facets read as a capsule/gem at this scale). `t` is reveal
 * progress in [0,1]; the prop rises with an overshoot settle and spins slowly.
 */
export const rewardProp = (
  keyPrefix: string,
  rarity: Rarity,
  at: EngineVec3,
  t: number,
  tick: number,
  scale = 1,
): readonly SceneInstance[] => {
  const rise = easeOutBack(t) * 0.55 * scale;
  const material = rewardMaterialOf(rarity);
  const spin = quatYaw(tick * 0.035);
  const size = (0.3 + (rarity === "jackpot" ? 0.14 : rarity === "rare" ? 0.08 : 0)) * scale * (0.2 + 0.8 * t);
  return [
    {
      key: `${keyPrefix}:core`,
      material,
      mesh: "sphere",
      transform: { position: v3(at.x, at.y + rise, at.z), rotation: spin, scale: v3(size, size, size) },
    },
    {
      key: `${keyPrefix}:facet`,
      material,
      mesh: "box",
      transform: {
        position: v3(at.x, at.y + rise, at.z),
        rotation: quatYaw(tick * 0.035 + 0.7),
        scale: v3(size * 0.72, size * 0.72, size * 0.72),
      },
    },
  ];
};

/** The warm vertical reward beam behind an uncommon+ reveal. */
export const rewardBeam = (key: string, at: EngineVec3, t: number, height = 2.4): SceneInstance => ({
  key,
  material: "RewardBeam",
  mesh: "cylinder",
  transform: {
    position: v3(at.x, at.y + height / 2, at.z),
    rotation: [0, 0, 0, 1],
    scale: v3(0.5 * progress(t, 1), height * progress(t, 1), 0.5 * progress(t, 1)),
  },
});

/** The result banner line the DOM chrome shows once revealed. */
export const resultTextOf = (plan: OutcomePlan): string => {
  if (!plan.win || plan.reward === null) {
    return "So close — try again!";
  }
  return `You won: ${plan.reward.label}!`;
};
