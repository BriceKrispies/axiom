/*
 * serialization.ts — JSON export/import for workbench configurations, plus the
 * stable config hash recorded in every session audit record. Import re-runs
 * full validation and rejects unknown schema versions with a readable issue;
 * nothing invalid is silently accepted.
 */

import type { CasinoGameConfig } from "./schema.ts";
import type { ChoiceBounds, ConfigIssue } from "./validation.ts";
import { validateConfig } from "./validation.ts";

/** Export a config as pretty JSON (workbench "Export JSON"). */
export const exportConfigJson = (config: CasinoGameConfig<unknown>): string => JSON.stringify(config, null, 2);

/** JSON value with recursively sorted object keys, so hashing is order-stable. */
const stableStringify = (value: unknown): string => {
  if (Array.isArray(value)) {
    return `[${value.map(stableStringify).join(",")}]`;
  }
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : 1))
      .map(([k, v]) => `${JSON.stringify(k)}:${stableStringify(v)}`);
    return `{${entries.join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
};

/** A stable 8-hex-digit FNV-1a hash of the config (audit record identity). */
export const configHash = (config: CasinoGameConfig<unknown>): string => {
  const text = stableStringify(config);
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i += 1) {
    h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
};

export interface ImportResult<TSpec> {
  /** The imported config — present only when `issues` is empty. */
  readonly config: CasinoGameConfig<TSpec> | null;
  readonly issues: readonly ConfigIssue[];
}

/**
 * Import a config from workbench JSON. Parse errors, wrong game ids, unknown
 * schema versions, and every validation failure come back as readable issues.
 */
export const importConfigJson = <TSpec>(
  json: string,
  expectedGameId: string,
  choiceBounds?: ChoiceBounds,
): ImportResult<TSpec> => {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (error) {
    return { config: null, issues: [{ message: `not valid JSON: ${String(error)}`, path: "$" }] };
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { config: null, issues: [{ message: "config must be a JSON object", path: "$" }] };
  }
  const candidate = parsed as CasinoGameConfig<TSpec>;
  const issues = [...validateConfig(candidate, choiceBounds)];
  if (typeof candidate.gameId === "string" && candidate.gameId !== expectedGameId) {
    issues.push({ message: `config is for game "${candidate.gameId}", expected "${expectedGameId}"`, path: "gameId" });
  }
  return issues.length > 0 ? { config: null, issues } : { config: candidate, issues: [] };
};
