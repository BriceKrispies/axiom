/*
 * language.ts — the typed, declarative effect language every Arena Forge card is
 * authored in. Cards carry DATA, never code: no JavaScript callbacks, no `eval`,
 * no `new Function`. An ability is a `{ trigger, conditions?, operations }`
 * record; the deterministic interpreter (`../effects/interpreter.ts`) is the ONLY
 * place that knows how to execute each `kind`. Adding a card never touches the
 * interpreter — it only adds more of these records. Adding a genuinely new verb
 * (a new operation `kind`) is the one thing that extends the interpreter, and it
 * is a deliberate language change, reviewed as such.
 *
 * Every bound (repeat counts, summon counts, nesting depth) is a hard integer
 * limit validated at content-load time and re-checked at runtime, so no authored
 * card — however malformed — can hang combat.
 */

import type { CardId, GroupId, KeywordId, TokenId } from "../ids.ts";

/** When an ability fires. Each trigger belongs to exactly one execution context
 * (economy or combat), fixed by {@link TRIGGER_CONTEXT}. */
export type Trigger =
  | "on_buy"
  | "on_play"
  | "on_sell"
  | "shop_start"
  | "round_end"
  | "combat_start"
  | "before_attack"
  | "after_attack"
  | "on_damage"
  | "on_survive_damage"
  | "on_death"
  | "on_friendly_summon"
  | "on_enemy_summon"
  | "passive_aura";

/** The two execution environments. Economy triggers mutate a player's shop /
 * warband / gold between combats; combat triggers mutate the live battlefield. */
export type EffectContext = "economy" | "combat";

/** The fixed, data-defined context of every trigger — no card overrides this. */
export const TRIGGER_CONTEXT: Readonly<Record<Trigger, EffectContext>> = {
  on_buy: "economy",
  on_play: "economy",
  on_sell: "economy",
  shop_start: "economy",
  round_end: "economy",
  combat_start: "combat",
  before_attack: "combat",
  after_attack: "combat",
  on_damage: "combat",
  on_survive_damage: "combat",
  on_death: "combat",
  on_friendly_summon: "combat",
  on_enemy_summon: "combat",
  passive_aura: "combat",
};

export const ALL_TRIGGERS: readonly Trigger[] = Object.keys(TRIGGER_CONTEXT) as Trigger[];

/**
 * A predicate gating whether an ability's operations run. All conditions on an
 * ability must hold (logical AND). Every kind reads only from explicit unit /
 * board / round state — never wall-clock, never randomness.
 */
export type Condition =
  | { readonly kind: "source_is_forged" }
  | { readonly kind: "source_is_normal" }
  | { readonly kind: "source_attack_at_least"; readonly value: number }
  | { readonly kind: "source_health_at_least"; readonly value: number }
  | { readonly kind: "source_in_group"; readonly group: GroupId }
  | { readonly kind: "source_has_keyword"; readonly keyword: KeywordId }
  | { readonly kind: "source_position_leftmost" }
  | { readonly kind: "source_position_rightmost" }
  | { readonly kind: "adjacent_in_group"; readonly group: GroupId }
  | { readonly kind: "round_at_least"; readonly value: number }
  | { readonly kind: "friendly_group_count_at_least"; readonly group: GroupId; readonly value: number }
  | { readonly kind: "empty_warband_slots_at_least"; readonly value: number };

export const CONDITION_KINDS: readonly Condition["kind"][] = [
  "source_is_forged",
  "source_is_normal",
  "source_attack_at_least",
  "source_health_at_least",
  "source_in_group",
  "source_has_keyword",
  "source_position_leftmost",
  "source_position_rightmost",
  "adjacent_in_group",
  "round_at_least",
  "friendly_group_count_at_least",
  "empty_warband_slots_at_least",
];

/**
 * Names a set of units (or slots) an operation acts on, resolved deterministically
 * against the current board. "Friendly"/"enemy" are relative to the ability's
 * source. Random selectors draw from the combat's single seeded `Rng` in stable
 * board order, so they are reproducible.
 */
export type Selector =
  | { readonly kind: "self" }
  | { readonly kind: "attacker" }
  | { readonly kind: "defender" }
  | { readonly kind: "adjacent_friendly" }
  | { readonly kind: "leftmost_friendly" }
  | { readonly kind: "rightmost_friendly" }
  | { readonly kind: "random_friendly" }
  | { readonly kind: "random_enemy" }
  | { readonly kind: "lowest_attack_friendly" }
  | { readonly kind: "highest_attack_enemy" }
  | { readonly kind: "all_friendly" }
  | { readonly kind: "all_enemy" }
  | { readonly kind: "friendly_in_group"; readonly group: GroupId }
  | { readonly kind: "empty_friendly_slot" };

export const SELECTOR_KINDS: readonly Selector["kind"][] = [
  "self",
  "attacker",
  "defender",
  "adjacent_friendly",
  "leftmost_friendly",
  "rightmost_friendly",
  "random_friendly",
  "random_enemy",
  "lowest_attack_friendly",
  "highest_attack_enemy",
  "all_friendly",
  "all_enemy",
  "friendly_in_group",
  "empty_friendly_slot",
];

/**
 * One verb of the effect language. The interpreter switches on `kind` — that
 * switch is generic MECHANISM, not card-specific branching. `repeat` nests an
 * operation a bounded number of times; nesting depth and every count are capped
 * by {@link EFFECT_BOUNDS} at load time.
 */
export type Operation =
  | { readonly kind: "modify_attack"; readonly target: Selector; readonly amount: number }
  | { readonly kind: "modify_health"; readonly target: Selector; readonly amount: number }
  | { readonly kind: "deal_damage"; readonly target: Selector; readonly amount: number }
  | { readonly kind: "heal"; readonly target: Selector; readonly amount: number }
  | { readonly kind: "grant_keyword"; readonly target: Selector; readonly keyword: KeywordId }
  | { readonly kind: "remove_keyword"; readonly target: Selector; readonly keyword: KeywordId }
  | { readonly kind: "summon_token"; readonly token: TokenId; readonly at: Selector; readonly count: number }
  | { readonly kind: "move_unit"; readonly target: Selector; readonly to: "leftmost" | "rightmost" }
  | { readonly kind: "swap_with"; readonly target: Selector; readonly other: Selector }
  | { readonly kind: "copy_ability"; readonly from: Selector }
  | { readonly kind: "repeat"; readonly times: number; readonly op: Operation }
  | { readonly kind: "add_gold"; readonly amount: number }
  | { readonly kind: "discount_shop"; readonly amount: number }
  | { readonly kind: "transform_card"; readonly target: Selector; readonly into: CardId }
  | { readonly kind: "emit_cue"; readonly cue: string };

export const OPERATION_KINDS: readonly Operation["kind"][] = [
  "modify_attack",
  "modify_health",
  "deal_damage",
  "heal",
  "grant_keyword",
  "remove_keyword",
  "summon_token",
  "move_unit",
  "swap_with",
  "copy_ability",
  "repeat",
  "add_gold",
  "discount_shop",
  "transform_card",
  "emit_cue",
];

/** Which operation verbs are legal in each context. Enforced at content load. */
export const OPERATION_CONTEXT: Readonly<Record<Operation["kind"], readonly EffectContext[]>> = {
  modify_attack: ["economy", "combat"],
  modify_health: ["economy", "combat"],
  deal_damage: ["combat"],
  heal: ["combat"],
  grant_keyword: ["economy", "combat"],
  remove_keyword: ["economy", "combat"],
  summon_token: ["combat"],
  move_unit: ["combat"],
  swap_with: ["combat"],
  copy_ability: ["combat"],
  repeat: ["economy", "combat"],
  add_gold: ["economy"],
  discount_shop: ["economy"],
  transform_card: ["economy", "combat"],
  emit_cue: ["economy", "combat"],
};

/** One authored ability: a trigger, optional AND-conditions, and an ordered
 * operation list executed when it fires and all conditions hold. */
export interface Ability {
  readonly trigger: Trigger;
  readonly conditions?: readonly Condition[];
  readonly operations: readonly Operation[];
}

/**
 * Every hard limit in the effect language. The per-operation limits are checked
 * when content loads (a card that violates one fails validation with a specific
 * error); the per-combat limits are enforced by the combat interpreter, which
 * terminates the offending operation and emits a diagnostic rather than looping.
 */
export const EFFECT_BOUNDS = {
  /** Max `times` on a `repeat` operation. */
  maxRepeat: 8,
  /** Max `count` on a single `summon_token` operation. */
  maxSummonPerOperation: 6,
  /** Max nesting depth of `repeat` operations. */
  maxOperationDepth: 3,
  /** Max abilities a single card definition may declare (normal + forged each). */
  maxAbilitiesPerProfile: 6,
  /** Max operations a single ability may declare. */
  maxOperationsPerAbility: 8,
  /** Runtime: max events emitted in one combat before it is force-drawn. */
  maxEventsPerCombat: 6000,
  /** Runtime: max token summons in one combat. */
  maxSummonsPerCombat: 60,
  /** Runtime: max `copy_ability` resolutions in one combat. */
  maxCopiedAbilities: 24,
  /** Runtime: max attack actions in one combat before it is force-drawn. */
  maxCombatActions: 500,
} as const;
