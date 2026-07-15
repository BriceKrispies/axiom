/*
 * config-store.ts — per-game configuration overrides. The workbench saves an
 * edited config here; the game screen plays whatever is stored (falling back
 * to the definition's default). Anything invalid on load is discarded — a
 * stale or hand-edited entry can never start a session.
 */

import type { CasinoGameConfig } from "../chance-engine/configuration/schema.ts";
import { validateConfig } from "../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition } from "../chance-engine/registry/definition.ts";

const keyOf = (gameId: string): string => `casino-games:config:${gameId}`;

export const storedConfigOf = (definition: CasinoGameDefinition<unknown>): CasinoGameConfig<unknown> => {
  try {
    const raw = localStorage.getItem(keyOf(definition.id));
    if (raw !== null) {
      const parsed = JSON.parse(raw) as CasinoGameConfig<unknown>;
      const issues = [...validateConfig(parsed), ...definition.validateSpec(parsed.gameSpecific)];
      if (issues.length === 0 && parsed.gameId === definition.id) {
        return parsed;
      }
    }
  } catch {
    // Fall through to the default.
  }
  return definition.defaultConfig();
};

export const storeConfig = (config: CasinoGameConfig<unknown>): void => {
  try {
    localStorage.setItem(keyOf(config.gameId), JSON.stringify(config));
  } catch {
    // Storage unavailable — the edit lives for this session only.
  }
};

export const clearStoredConfig = (gameId: string): void => {
  try {
    localStorage.removeItem(keyOf(gameId));
  } catch {
    // Nothing to clear.
  }
};
