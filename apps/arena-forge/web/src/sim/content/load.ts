/*
 * load.ts — turns a raw `ContentBundle` into an indexed, canonically-ordered
 * `LoadedContent` the engine reads all match long. Loading VALIDATES first and
 * throws on any error (so nothing downstream ever sees broken content), then
 * sorts every collection into a stable canonical order that is independent of
 * source-file discovery order: cards by `(tier, id)`, everything else by `id`.
 * That stable order is what makes pool rolls and effect resolution reproducible.
 */

import type {
  ArchetypeDefinition,
  CardDefinition,
  ContentBundle,
  GroupDefinition,
  KeywordDefinition,
  Tier,
  VisualProfile,
} from "./schema.ts";
import { validateContent } from "./validate.ts";
import type { CardId, GroupId, VisualProfileId } from "../ids.ts";

const byId = <T extends { id: string }>(items: readonly T[]): T[] =>
  items.slice().sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));

/** The validated, indexed, order-stable content the match engine consumes. */
export class LoadedContent {
  public readonly version: number;
  public readonly archetypes: readonly ArchetypeDefinition[];
  public readonly keywords: readonly KeywordDefinition[];
  public readonly groups: readonly GroupDefinition[];
  /** All cards (collectible + tokens), canonical `(tier, id)` order. */
  public readonly cards: readonly CardDefinition[];
  /** Only collectible cards with pool copies, canonical order. */
  public readonly collectibleCards: readonly CardDefinition[];
  public readonly visualProfiles: readonly VisualProfile[];

  private readonly cardIndex: Map<CardId, CardDefinition>;
  private readonly groupIndex: Map<GroupId, GroupDefinition>;
  private readonly visualIndex: Map<VisualProfileId, VisualProfile>;

  public constructor(bundle: ContentBundle) {
    const errors = validateContent(bundle);
    if (errors.length > 0) {
      throw new Error(`Arena Forge content failed validation:\n  ${errors.join("\n  ")}`);
    }
    this.version = bundle.version;
    this.archetypes = byId(bundle.archetypes);
    this.keywords = byId(bundle.keywords);
    this.groups = byId(bundle.groups);
    this.visualProfiles = byId(bundle.visualProfiles);
    this.cards = bundle.cards.slice().sort((a, b) => (a.tier - b.tier) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    this.collectibleCards = this.cards.filter((c) => c.collectible && c.poolCount > 0);
    this.cardIndex = new Map(this.cards.map((c) => [c.id, c]));
    this.groupIndex = new Map(this.groups.map((g) => [g.id, g]));
    this.visualIndex = new Map(this.visualProfiles.map((v) => [v.id, v]));
  }

  public card(id: CardId): CardDefinition {
    const def = this.cardIndex.get(id);
    if (def === undefined) {
      throw new Error(`Arena Forge: unknown card '${id}'`);
    }
    return def;
  }

  public group(id: GroupId): GroupDefinition {
    const def = this.groupIndex.get(id);
    if (def === undefined) {
      throw new Error(`Arena Forge: unknown group '${id}'`);
    }
    return def;
  }

  public visual(id: VisualProfileId): VisualProfile {
    const def = this.visualIndex.get(id);
    if (def === undefined) {
      throw new Error(`Arena Forge: unknown visual profile '${id}'`);
    }
    return def;
  }

  /** Collectible cards of a given tier, canonical order — the roll candidates. */
  public collectibleOfTier(tier: Tier): readonly CardDefinition[] {
    return this.collectibleCards.filter((c) => c.tier === tier);
  }
}
