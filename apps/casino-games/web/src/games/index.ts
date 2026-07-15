/*
 * index.ts — the one place every game definition is registered. The registry
 * built here is the single source of truth: the catalog renders it, the shell
 * mounts through it, the workbench edits its configs, and `registry.test.ts`
 * asserts the active ids exist exactly once. Only the seven currently active
 * games are imported/registered here; the rest of the original catalog was
 * removed from the repository rather than merely disabled.
 */

import type { CasinoGameConfig } from "../chance-engine/configuration/schema.ts";
import type { MechanicInit } from "../chance-engine/outcomes/result-source.ts";
import type { CasinoGameDefinition } from "../chance-engine/registry/definition.ts";
import { CasinoGameRegistry } from "../chance-engine/registry/registry.ts";

import { TREASURE_CHEST_PICK } from "./treasure-chest-pick/definition.ts";
import { PRIZE_WHEEL } from "./prize-wheel/definition.ts";
import { DICE_VAULT } from "./dice-vault/definition.ts";
import { SCRATCH_REVEAL } from "./scratch-reveal/definition.ts";
import { PRESENT_POP } from "./present-pop/definition.ts";
import { FISHING_CAST } from "./fishing-cast/definition.ts";
import { GEM_MINE } from "./gem-mine/definition.ts";

/** The seven active games, in catalog order. */
export const ALL_GAMES: readonly CasinoGameDefinition<never>[] = [
  TREASURE_CHEST_PICK,
  PRIZE_WHEEL,
  DICE_VAULT,
  SCRATCH_REVEAL,
  PRESENT_POP,
  FISHING_CAST,
  GEM_MINE,
] as unknown as readonly CasinoGameDefinition<never>[];

export const buildRegistry = (): CasinoGameRegistry => {
  const registry = new CasinoGameRegistry();
  for (const definition of ALL_GAMES) {
    registry.register(definition);
  }
  return registry;
};

/**
 * A representative `MechanicInit` for a definition, derived from its declared
 * mechanic kind. This is used by `registry.test.ts` to smoke-create a session
 * for every game (create + advance to "ready"); the concrete per-game
 * mechanic (a wheel's real segment slots, a game's real combination space) is
 * assembled inside each definition's own `mount`, and is exercised by that
 * game's own tests. For "choice" games the choice count matters (it drives the
 * preassigned population), so it is threaded through faithfully; the
 * destination/combination placeholders are structurally valid stand-ins that
 * `createSession` carries forward untouched until the game's mount supplies
 * the real one.
 */
export const mechanicInitFor = (id: string, config: CasinoGameConfig<unknown>): MechanicInit => {
  const definition = ALL_GAMES.find((entry) => entry.id === id);
  const firstWinningTier = config.rewardTiers.find((tier) => tier.countsAsWin)?.id ?? config.rewardTiers[0]?.id ?? "common";
  switch (definition?.mechanic) {
    case "choice-population":
      return { choiceCount: config.choiceCount ?? 9, kind: "choice" };
    case "destination":
      return {
        kind: "destination",
        slots: [
          { id: "win", mass: 1, tierId: firstWinningTier },
          { id: "miss", mass: 1, tierId: null },
        ],
      };
    case "combination":
      return {
        kind: "combination",
        space: { reels: 1, symbolsPerReel: 2, winningCombos: [{ combo: [0], tierId: firstWinningTier }] },
      };
    default:
      return { kind: "single" };
  }
};
