/*
 * definition.ts — Treasure Chest Pick: nine carved-wood chests, one pick.
 * Choice-population mechanic: the configured target win rate controls how many
 * of the nine chests hold prizes (stochastic rounding of 9·p), assigned before
 * the player chooses.
 */

import type { RenderQualityInput } from "@axiom/web-engine";
import type { CasinoGameConfig, RewardTier } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { brandIssues, DEFAULT_BRAND } from "../../presentation/branding/brand.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ChestSpec } from "./game.ts";
import { chestCues, commitBeatTicks, initialChestExtra, stepChest } from "./game.ts";
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
 * get, a pearl or Crabigail is a good day, and the ring is rare enough to be
 * worth the ritual. Every one counts as a win — there is no consolation object
 * here, so the chest never opens onto a shrug.
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
    id: "pearl-clam",
    label: "Pearl Clam",
    // The clam replaced an old boot, and it is NOT the boot's tier wearing a new
    // object: a boot was the joke prize at one point, and a pearl is a genuine
    // find. So it moves up a rung to uncommon and its reward with it. `rarity`
    // drives celebration intensity and the HUD accent, so leaving it on `common`
    // would have handed the player a pearl with a consolation-prize fanfare.
    rarity: "uncommon",
    reward: { amount: 50, kind: "prize", label: "a Pearl" },
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

/**
 * Rasterization defaults for this game, and the values RESET QUALITY restores.
 *
 * These reproduce the look and the frame rate the game shipped with before the
 * quality controls existed. That is a deliberate choice, not an oversight: the
 * settings are here so a player can spend frames on smoother edges, but the
 * DEFAULT should not quietly hand everyone a slower game than the one they had.
 *
 * The setting that governs the frame rate is `renderScale`, because the software
 * rasterizer costs very nearly one unit of time per backing pixel and this scene
 * is dense (≈365 nodes). Measured end-to-end on this scene at 936×585 CSS, after
 * the rasterizer's span and back-face-cull fixes:
 *
 *     0.5×  →  137k samples  →  ~59 fps   (this default; the engine's former look)
 *     1.0×  →  547k samples  →  ~31 fps   (native 1:1 with the display)
 *     2.0×  →  2.19M samples →  single digits
 *
 * `0.5` is not "half quality" in the abstract — it is the resolution this engine
 * always drew at, back when that was hard-coded and unchangeable. The difference
 * now is that it is a floor a player can leave, not a ceiling nobody could see
 * past. The rung worth reaching for is `1.0`, which removes the upscale entirely.
 *
 * `pixelRatioMode` is deliberately `fixed-1x` rather than following the display:
 * on a GPU renderer honouring a 2× device ratio is close to free, and on this one
 * it quadruples the per-frame work. A player on a HiDPI screen who wants those
 * samples can ask for them; nobody should be handed a 4× bill by default for
 * owning a nice monitor. The engine's `maxSamples` then bounds the maximised-
 * window case whatever the other settings say.
 */
const CHEST_RENDER_QUALITY: RenderQualityInput = {
  curveDetail: 1,
  lineCap: "round",
  lineJoin: "round",
  maxPixelRatio: 2,
  pixelRatioMode: "fixed-1x",
  renderScale: 0.5,
};

// Win every time; what varies is WHICH treasure. `targetWinRate: 1` makes all
// nine chests winners (9·1 = 9), so any pick opens onto something — and the five
// tiers above decide what. Tune either in the Set Up panel.
const defaultConfig = (): CasinoGameConfig<ChestSpec> =>
  baseConfig("treasure-chest-pick", "Treasure Chest Pick", "tabletop", { brand: DEFAULT_BRAND, danceLiveliness: 0.7 }, {
    choiceCount: 9,
    renderQuality: CHEST_RENDER_QUALITY,
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
    // The commit beat carries the crab's walk to the chest AND the chest's spiral
    // into its hero framing, so it runs for the length of both rather than the
    // shared default pause. `commitBeatTicks` owns that sum — the reveal cannot
    // begin until the crab has the lid in his claws and the flight has landed.
    commitPauseTicks: commitBeatTicks,
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
