/*
 * schema.ts — the versioned configuration vocabulary every casino game is
 * driven by. A `CasinoGameConfig<TSpec>` is plain JSON-serializable data:
 * the shared fields every game shares (win rate, reward tiers, presentation
 * knobs) plus a typed `gameSpecific` block owned by the game definition.
 * Validation lives in `validation.ts`; JSON import/export in `serialization.ts`.
 */

/** The one schema version this build reads and writes. Imports with any other
 * version are rejected with a readable error — never silently coerced. */
export const CONFIG_SCHEMA_VERSION = 1;

/** What a reward IS — deliberately cheerful and non-monetary. */
export type RewardKind = "prize" | "points" | "tickets" | "stars" | "gems" | "capsules" | "toy" | "retry";

/** The concrete reward a tier grants. */
export interface RewardDefinition {
  readonly kind: RewardKind;
  readonly label: string;
  readonly amount: number;
}

/** Rarity drives celebration intensity, rarity colors, and catalog badges. */
export type Rarity = "common" | "uncommon" | "rare" | "jackpot";

export const RARITIES: readonly Rarity[] = ["common", "uncommon", "rare", "jackpot"];

/**
 * One reward tier. `weight` is CONDITIONAL ON WINNING: once the gameplay
 * stream resolves a win, the tier stream picks among the winning tiers by
 * these weights. A tier with `countsAsWin: false` never resolves from a win —
 * it exists so a game can label consolation presentation (e.g. wheel "spin
 * again" segments) with reward metadata.
 */
export interface RewardTier {
  readonly id: string;
  readonly label: string;
  readonly rarity: Rarity;
  readonly weight: number;
  readonly countsAsWin: boolean;
  readonly reward: RewardDefinition;
}

/** The shared camera presets (see presentation/cameras/presets.ts). */
export type CameraPresetId = "machine-interior" | "showcase" | "tabletop" | "reveal-focus";

/** Palette accents a config may override without a game owning new materials. */
export interface ThemeOverrides {
  readonly accent?: readonly [number, number, number];
  readonly backdrop?: readonly [number, number, number];
}

/** Reduced-motion behavior: follow the player's setting, or force it. */
export type ReducedMotionMode = "system" | "on" | "off";

/**
 * The versioned, validated configuration every game is created from.
 * `TSpec` is the game-specific block (chest count is `choiceCount`; a wheel's
 * segment layout, a claw's prize bed, etc. live in `gameSpecific`).
 */
export interface CasinoGameConfig<TSpec = Record<string, never>> {
  readonly schemaVersion: number;
  readonly gameId: string;
  readonly displayName: string;
  /** Total probability that a round wins, in [0, 1]. */
  readonly targetWinRate: number;
  /** Winning-tier weights are conditional on a win (see `RewardTier`). */
  readonly rewardTiers: readonly RewardTier[];
  /** Number of selectable objects, for choice-population games. */
  readonly choiceCount?: number;
  /** Animation-duration multiplier, 1 = authored speed. Range [0.25, 3]. */
  readonly presentationSpeed: number;
  /** Celebration scale, 1 = authored intensity. Range [0, 2]. */
  readonly celebrationIntensity: number;
  readonly cameraPreset: CameraPresetId;
  readonly reducedMotion: ReducedMotionMode;
  readonly theme?: ThemeOverrides;
  readonly gameSpecific: TSpec;
}

/** The default cheerful tier ladder games start from (games may replace it). */
export const DEFAULT_REWARD_TIERS: readonly RewardTier[] = [
  {
    countsAsWin: true,
    id: "common",
    label: "Star Token",
    rarity: "common",
    reward: { amount: 25, kind: "stars", label: "25 stars" },
    weight: 60,
  },
  {
    countsAsWin: true,
    id: "uncommon",
    label: "Ticket Bundle",
    rarity: "uncommon",
    reward: { amount: 120, kind: "tickets", label: "120 tickets" },
    weight: 28,
  },
  {
    countsAsWin: true,
    id: "rare",
    label: "Gem Trophy",
    rarity: "rare",
    reward: { amount: 1, kind: "gems", label: "Radiant gem" },
    weight: 10,
  },
  {
    countsAsWin: true,
    id: "jackpot",
    label: "Golden Capsule",
    rarity: "jackpot",
    reward: { amount: 1, kind: "capsules", label: "Golden capsule" },
    weight: 2,
  },
];

/** Shared config scaffolding for a game definition's `defaultConfig()`. */
export const baseConfig = <TSpec>(
  gameId: string,
  displayName: string,
  cameraPreset: CameraPresetId,
  gameSpecific: TSpec,
  overrides: Partial<Pick<CasinoGameConfig<TSpec>, "targetWinRate" | "rewardTiers" | "choiceCount">> = {},
): CasinoGameConfig<TSpec> => ({
  cameraPreset,
  celebrationIntensity: 1,
  displayName,
  gameId,
  gameSpecific,
  presentationSpeed: 1,
  reducedMotion: "system",
  rewardTiers: overrides.rewardTiers ?? DEFAULT_REWARD_TIERS,
  schemaVersion: CONFIG_SCHEMA_VERSION,
  targetWinRate: overrides.targetWinRate ?? 0.42,
  ...(overrides.choiceCount === undefined ? {} : { choiceCount: overrides.choiceCount }),
});
