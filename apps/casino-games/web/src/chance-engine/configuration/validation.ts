/*
 * validation.ts — structural validation of a `CasinoGameConfig`, run before a
 * session may start. Nothing downstream (resolver, adapters, presentation) is
 * asked to defend against a bad config: the workbench surfaces these issues as
 * readable errors and refuses to start until the list is empty.
 */

import type { CasinoGameConfig, RewardTier } from "./schema.ts";
import { CONFIG_SCHEMA_VERSION } from "./schema.ts";

/** One readable validation failure, anchored to the config path it names. */
export interface ConfigIssue {
  readonly path: string;
  readonly message: string;
}

const issue = (path: string, message: string): ConfigIssue => ({ message, path });

const isFiniteNumber = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);

const tierIssues = (tier: RewardTier, index: number): readonly ConfigIssue[] => {
  const path = `rewardTiers[${index}]`;
  const out: ConfigIssue[] = [];
  if (typeof tier.id !== "string" || tier.id.length === 0) {
    out.push(issue(`${path}.id`, "tier id must be a non-empty string"));
  }
  if (!isFiniteNumber(tier.weight) || tier.weight < 0) {
    out.push(issue(`${path}.weight`, `tier weight must be a finite number ≥ 0 (got ${String(tier.weight)})`));
  }
  if (!isFiniteNumber(tier.reward?.amount) || tier.reward.amount < 0) {
    out.push(issue(`${path}.reward.amount`, "reward amount must be a finite number ≥ 0"));
  }
  return out;
};

/** Optional per-game bounds a definition can impose on `choiceCount`. */
export interface ChoiceBounds {
  readonly min: number;
  readonly max: number;
}

/**
 * Validate the shared configuration shape. Returns every issue found (empty =
 * valid). Game-specific `gameSpecific` blocks are validated by the game
 * definition's own `validateSpec`, appended by the caller.
 */
export const validateConfig = (config: CasinoGameConfig<unknown>, choiceBounds?: ChoiceBounds): readonly ConfigIssue[] => {
  const out: ConfigIssue[] = [];

  if (config.schemaVersion !== CONFIG_SCHEMA_VERSION) {
    out.push(
      issue(
        "schemaVersion",
        `unknown schema version ${String(config.schemaVersion)} — this build reads version ${CONFIG_SCHEMA_VERSION}`,
      ),
    );
  }
  if (typeof config.gameId !== "string" || config.gameId.length === 0) {
    out.push(issue("gameId", "gameId must be a non-empty string"));
  }
  if (!isFiniteNumber(config.targetWinRate) || config.targetWinRate < 0 || config.targetWinRate > 1) {
    out.push(
      issue("targetWinRate", `targetWinRate must be a finite number in [0, 1] (got ${String(config.targetWinRate)})`),
    );
  }

  if (!Array.isArray(config.rewardTiers) || config.rewardTiers.length === 0) {
    out.push(issue("rewardTiers", "at least one reward tier is required"));
  } else {
    config.rewardTiers.forEach((tier, index) => out.push(...tierIssues(tier, index)));
    const ids = config.rewardTiers.map((tier) => tier.id);
    if (new Set(ids).size !== ids.length) {
      out.push(issue("rewardTiers", "tier ids must be unique"));
    }
    const winnable = config.rewardTiers.filter((tier) => tier.countsAsWin && isFiniteNumber(tier.weight) && tier.weight > 0);
    if (winnable.length === 0 && isFiniteNumber(config.targetWinRate) && config.targetWinRate > 0) {
      out.push(
        issue(
          "rewardTiers",
          "wins are possible (targetWinRate > 0) but no tier counts as a win with a usable weight > 0",
        ),
      );
    }
  }

  if (config.choiceCount !== undefined) {
    const bounds = choiceBounds ?? { max: 64, min: 2 };
    const ok = Number.isInteger(config.choiceCount) && config.choiceCount >= bounds.min && config.choiceCount <= bounds.max;
    if (!ok) {
      out.push(
        issue(
          "choiceCount",
          `choiceCount must be an integer in [${bounds.min}, ${bounds.max}] (got ${String(config.choiceCount)})`,
        ),
      );
    }
  }

  if (!isFiniteNumber(config.presentationSpeed) || config.presentationSpeed < 0.25 || config.presentationSpeed > 3) {
    out.push(issue("presentationSpeed", "presentationSpeed must be a finite number in [0.25, 3]"));
  }
  if (!isFiniteNumber(config.celebrationIntensity) || config.celebrationIntensity < 0 || config.celebrationIntensity > 2) {
    out.push(issue("celebrationIntensity", "celebrationIntensity must be a finite number in [0, 2]"));
  }

  return out;
};
