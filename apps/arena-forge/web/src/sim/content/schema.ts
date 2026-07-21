/*
 * schema.ts — the typed, declarative content format. A `ContentBundle` is pure
 * DATA: keyword, group, card, token, and visual-profile records. There is no
 * behavior here and no card-specific code anywhere in the engine — a card's
 * behavior is entirely its `normal`/`forged` ability lists in the effect
 * language. `validate.ts` checks a bundle at load and fails with card-specific
 * errors; `load.ts` canonicalizes it into an indexed, order-stable `LoadedContent`.
 */

import type { ArchetypeId, CardId, GroupId, KeywordId, TokenId, VisualProfileId } from "../ids.ts";
import type { Ability } from "../effects/language.ts";

/** Unit tier; also gates shop availability by forge rank. */
export type Tier = 1 | 2 | 3 | 4 | 5 | 6;
export const TIERS: readonly Tier[] = [1, 2, 3, 4, 5, 6];

/** A keyword: a named combat/economy property units can carry (e.g. `guard`). */
export interface KeywordDefinition {
  readonly id: KeywordId;
  readonly name: string;
  readonly description: string;
}

/** A group ("tribe"): the data-defined archetype vocabulary that replaces
 * hardcoded tribes. Units carry zero or more group ids. */
export interface GroupDefinition {
  readonly id: GroupId;
  readonly name: string;
  readonly description: string;
  readonly archetype: ArchetypeId;
  readonly visualTheme: string;
  readonly preferredTags: readonly string[];
  /** Relative shop-appearance weighting metadata (advisory; the pool is the
   * hard source of what can roll). */
  readonly shopWeight: number;
  readonly presentationCues: readonly string[];
  /** A group-specific UI accent color (hex). */
  readonly accent: string;
}

/** The forged stat rule: flat bonuses added to base stats when a unit forges. */
export interface ForgedStatRule {
  readonly attack: number;
  readonly health: number;
}

/** One data-driven forge reward, granted once when a unit forges. */
export type ForgeReward =
  | { readonly kind: "gold"; readonly amount: number }
  | { readonly kind: "discount"; readonly amount: number };

/** A card definition — the entire behavior + presentation of a unit, as data. */
export interface CardDefinition {
  readonly id: CardId;
  readonly name: string;
  readonly rulesText: string;
  readonly tier: Tier;
  readonly cost: number;
  readonly baseAttack: number;
  readonly baseHealth: number;
  readonly groups: readonly GroupId[];
  readonly keywords: readonly KeywordId[];
  readonly normal: readonly Ability[];
  readonly forged: readonly Ability[];
  readonly forgedStats: ForgedStatRule;
  readonly visualProfile: VisualProfileId;
  readonly forgedVisualProfile: VisualProfileId;
  /** Copies of this card in the shared pool. 0 for tokens / forge-only cards. */
  readonly poolCount: number;
  /** Tokens this card can summon (must reference real token cards). */
  readonly tokens?: readonly TokenId[];
  /** When false, the card never appears in a shop (tokens, forge-only). */
  readonly collectible: boolean;
  /** Optional per-card forge reward overriding the economy default. */
  readonly forgeReward?: ForgeReward;
  readonly contentVersion: number;
}

/**
 * Presentation-only data for a card / unit stage, kept entirely separate from the
 * card's rules so art can change without touching behavior. Every field is a
 * data-defined id or budget the presentation layer interprets.
 */
export interface VisualProfile {
  readonly id: VisualProfileId;
  readonly frame: string;
  readonly portrait: string;
  readonly border: string;
  readonly base: string;
  readonly idle: string;
  readonly entrance: string;
  readonly attackTrail: string;
  readonly impact: string;
  readonly death: string;
  readonly aura: string;
  /** Group-color treatment (hex). */
  readonly groupColor: string;
  readonly particleBudget: number;
  readonly soundCues: readonly string[];
  /** Overrides applied when this profile is shown at its forged stage. */
  readonly forgedOverrides?: Partial<Omit<VisualProfile, "id" | "forgedOverrides">>;
}

/** An archetype: the strategic identity a group expresses. */
export interface ArchetypeDefinition {
  readonly id: ArchetypeId;
  readonly name: string;
  readonly description: string;
}

/** The raw authored content, before validation/canonicalization. */
export interface ContentBundle {
  readonly version: number;
  readonly archetypes: readonly ArchetypeDefinition[];
  readonly keywords: readonly KeywordDefinition[];
  readonly groups: readonly GroupDefinition[];
  /** Collectible cards AND tokens share this list; tokens have `collectible:false`. */
  readonly cards: readonly CardDefinition[];
  readonly visualProfiles: readonly VisualProfile[];
}
