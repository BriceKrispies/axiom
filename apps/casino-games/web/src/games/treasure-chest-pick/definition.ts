/*
 * definition.ts — Treasure Chest Pick: nine carved-wood chests, one pick.
 * Choice-population mechanic: the configured target win rate controls how many
 * of the nine chests hold prizes (stochastic rounding of 9·p), assigned before
 * the player chooses.
 */

import type { CasinoGameConfig, RewardTier } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { brandIssues, DEFAULT_BRAND } from "../../presentation/branding/brand.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ChestSpec } from "./game.ts";
import { CHEST_TIMING, chestCues, initialChestExtra, stepChest } from "./game.ts";
import { chestResources, chestScene, chestWaterOverlay } from "./scene.ts";

/**
 * The five treasures a chest can hold.
 *
 * A tier id here IS a `PrizeKind` (see `prizes/index.ts`): that identity is the
 * whole binding between the fairness machinery and the presentation. The
 * choice-population adapter draws one of these per chest at commit time, before
 * the player can influence anything, and the reveal simply looks up which
 * object to build. Nothing in the view chooses a prize.
 *
 * `weight` is conditional on winning, so these are the odds of WHICH treasure a
 * winning chest holds. They read as a prize ladder: coins are what you usually
 * get, the boot is the running joke, and the ring is rare enough to be worth
 * the ritual. Every one counts as a win — even the boot, which pops out of the
 * chest with the same ceremony as the ring and is funnier for it.
 *
 * `rarity` drives celebration intensity, the HUD accent, and the fallback in
 * `prizeKindOf`; it is deliberately in step with the weights.
 */
const PRIZE_TIERS: readonly RewardTier[] = [
  {
    countsAsWin: true,
    id: "gold-coin",
    label: "Gold Coin",
    rarity: "common",
    reward: { amount: 10, kind: "points", label: "a Gold Coin" },
    weight: 34,
  },
  {
    countsAsWin: true,
    id: "leather-boot",
    label: "Old Boot",
    rarity: "common",
    reward: { amount: 1, kind: "prize", label: "an Old Boot" },
    weight: 20,
  },
  {
    countsAsWin: true,
    id: "crab-bride",
    // She has a name. The beach crab is scenery; the one in the chest is a
    // character the player is being handed, and "the Crab's Girlfriend" defined
    // her by the other crab. `id` stays `crab-bride` — it is the binding to
    // `prizes/crab-bride.ts` and to any saved config, and renaming a tier id
    // silently repoints every chest that drew it.
    label: "Crabigail",
    rarity: "uncommon",
    reward: { amount: 25, kind: "toy", label: "Crabigail" },
    weight: 24,
  },
  {
    countsAsWin: true,
    id: "gold-bar",
    label: "Gold Bar",
    rarity: "rare",
    reward: { amount: 100, kind: "points", label: "a Gold Bar" },
    weight: 16,
  },
  {
    countsAsWin: true,
    id: "wedding-ring",
    label: "Diamond Ring",
    rarity: "jackpot",
    reward: { amount: 500, kind: "prize", label: "a Diamond Ring" },
    weight: 6,
  },
];

// Win every time; what varies is WHICH treasure. `targetWinRate: 1` makes all
// nine chests winners (9·1 = 9), so any pick opens onto something — and the five
// tiers above decide what. Tune either in the Set Up panel.
const defaultConfig = (): CasinoGameConfig<ChestSpec> =>
  baseConfig("treasure-chest-pick", "Treasure Chest Pick", "tabletop", { brand: DEFAULT_BRAND, danceLiveliness: 0.7 }, {
    choiceCount: 9,
    rewardTiers: PRIZE_TIERS,
    targetWinRate: 1,
  });

const validateSpec = (spec: ChestSpec): readonly ConfigIssue[] => {
  const liveliness =
    typeof spec.danceLiveliness === "number" && Number.isFinite(spec.danceLiveliness) && spec.danceLiveliness >= 0 && spec.danceLiveliness <= 1
      ? []
      : [{ message: "danceLiveliness must be a finite number in [0, 1]", path: "gameSpecific.danceLiveliness" }];
  return [...liveliness, ...brandIssues(spec.brand, "gameSpecific.brand")];
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<ChestSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    // The commit beat carries the chest's spiral into its hero framing, so it
    // runs for the length of that flight rather than the shared default pause.
    commitPauseTicks: CHEST_TIMING.spiralTicks,
    initExtra: initialChestExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Pick a chest — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: runtime.config.choiceCount ?? 9, kind: "choice" },
    overlay: (state, ctx, view) => chestWaterOverlay(state, ctx, view),
    resources: chestResources(runtime.config.gameSpecific.brand),
    sound: (prev, next) => chestCues(prev, next),
    step: (state, input, ctx) => stepChest(runtime, state, input, ctx),
    viewScene: (state) => chestScene(runtime, state),
  });

export const TREASURE_CHEST_PICK: CasinoGameDefinition<ChestSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Treasure Chest Pick",
  id: "treasure-chest-pick",
  instruction: "Pick one of nine chests. Some hold prizes — the latch tells you nothing.",
  interaction: "pick one of nine",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<ChestSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Nine carved chests, golden latches, one choice. Dance all they like — only the pick decides.",
  thumbnail: { accent: "#ffcc4d", bottom: "#8a5a2b", glyph: "chest", top: "#8fd0ff" },
  validateSpec,
};
