/*
 * ids.ts — the noun vocabulary of Arena Forge. Content ids (cards, groups,
 * keywords, tokens, visual profiles, archetypes) are stable authored strings;
 * runtime instance ids (a specific unit a player owns, a specific shop slot) are
 * integers allocated in stable order from a per-match counter. Simulation
 * ordering NEVER depends on object-key order or unordered traversal — every
 * collection is an array keyed by one of these ids.
 */

// ── authored content ids (stable strings from the content files) ───────────────
/** A card definition id, e.g. `"iron_sentinel"`. */
export type CardId = string;
/** A group ("tribe") id, e.g. `"ironbound"`. */
export type GroupId = string;
/** A keyword id, e.g. `"guard"`. */
export type KeywordId = string;
/** A summonable token id, e.g. `"bloom_sprout"`. */
export type TokenId = string;
/** A visual-profile id, e.g. `"vp_iron_sentinel"`. */
export type VisualProfileId = string;
/** An archetype id, e.g. `"formation"`. */
export type ArchetypeId = string;

// ── runtime instance ids (integers, allocated in stable order) ─────────────────
/** A specific card instance a player owns (in shop, hand, or warband). */
export type InstanceId = number;
/** A stable player slot id, `0..7`. */
export type PlayerId = number;

/**
 * A monotonic allocator for runtime instance ids. One lives on the match; every
 * card that enters play (bought, summoned, forged, granted) takes the next id.
 * Ids are never reused, so an event log can reference a unit unambiguously even
 * after it dies. The counter is part of the serialized match state.
 */
export class InstanceIdAllocator {
  private next: number;

  public constructor(start = 1) {
    this.next = start;
  }

  public allocate(): InstanceId {
    const id = this.next;
    this.next += 1;
    return id;
  }

  public snapshot(): number {
    return this.next;
  }

  public restore(next: number): void {
    this.next = next;
  }
}
