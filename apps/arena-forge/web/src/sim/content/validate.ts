/*
 * validate.ts — the content schema validator. It runs when a `ContentBundle` is
 * loaded and returns a list of specific, card-named errors (empty means valid).
 * Broken content NEVER reaches the match engine: `load.ts` throws on any error,
 * so the interpreter can assume every reference resolves and every bound holds.
 *
 * The checks cover exactly the failures the spec enumerates: duplicate ids,
 * missing groups / keywords / visual profiles, invalid tiers, negative stats,
 * unsupported triggers / operations / selectors, invalid selectors for a context,
 * recursive token summon graphs, references to missing cards, and unbounded
 * effect definitions (repeat / summon counts and nesting depth past the caps).
 */

import type { Ability, Condition, Operation, Selector, Trigger } from "../effects/language.ts";
import {
  ALL_TRIGGERS,
  CONDITION_KINDS,
  EFFECT_BOUNDS,
  OPERATION_CONTEXT,
  OPERATION_KINDS,
  SELECTOR_KINDS,
  TRIGGER_CONTEXT,
} from "../effects/language.ts";
import type { CardDefinition, ContentBundle } from "./schema.ts";
import { TIERS } from "./schema.ts";

const dupCheck = (label: string, ids: readonly string[], errors: string[]): void => {
  const seen = new Set<string>();
  for (const id of ids) {
    if (seen.has(id)) {
      errors.push(`${label}: duplicate id '${id}'`);
    }
    seen.add(id);
  }
};

const validateSelector = (where: string, sel: Selector, errors: string[]): void => {
  if (!SELECTOR_KINDS.includes(sel.kind)) {
    errors.push(`${where}: unsupported selector '${(sel as { kind: string }).kind}'`);
  }
};

const validateCondition = (where: string, cond: Condition, errors: string[]): void => {
  if (!CONDITION_KINDS.includes(cond.kind)) {
    errors.push(`${where}: unsupported condition '${(cond as { kind: string }).kind}'`);
  }
};

/** Validate one operation, recursively bounding `repeat` nesting and counts. */
const validateOperation = (
  where: string,
  op: Operation,
  context: "economy" | "combat",
  depth: number,
  errors: string[],
): void => {
  if (!OPERATION_KINDS.includes(op.kind)) {
    errors.push(`${where}: unsupported operation '${(op as { kind: string }).kind}'`);
    return;
  }
  const allowed = OPERATION_CONTEXT[op.kind];
  if (!allowed.includes(context)) {
    errors.push(`${where}: operation '${op.kind}' is not allowed in a ${context} trigger`);
  }
  if ("target" in op) {
    validateSelector(where, op.target, errors);
  }
  if (op.kind === "swap_with") {
    validateSelector(where, op.other, errors);
  }
  if (op.kind === "summon_token" || op.kind === "copy_ability") {
    validateSelector(where, op.kind === "summon_token" ? op.at : op.from, errors);
  }
  if (op.kind === "summon_token" && (op.count < 1 || op.count > EFFECT_BOUNDS.maxSummonPerOperation)) {
    errors.push(`${where}: summon count ${op.count} exceeds bound 1..${EFFECT_BOUNDS.maxSummonPerOperation}`);
  }
  if (op.kind === "repeat") {
    if (op.times < 1 || op.times > EFFECT_BOUNDS.maxRepeat) {
      errors.push(`${where}: repeat times ${op.times} exceeds bound 1..${EFFECT_BOUNDS.maxRepeat}`);
    }
    if (depth + 1 > EFFECT_BOUNDS.maxOperationDepth) {
      errors.push(`${where}: repeat nesting exceeds depth ${EFFECT_BOUNDS.maxOperationDepth}`);
    } else {
      validateOperation(where, op.op, context, depth + 1, errors);
    }
  }
};

const validateAbility = (where: string, ability: Ability, errors: string[]): void => {
  if (!ALL_TRIGGERS.includes(ability.trigger)) {
    errors.push(`${where}: unsupported trigger '${ability.trigger as Trigger}'`);
    return;
  }
  const context = TRIGGER_CONTEXT[ability.trigger];
  for (const cond of ability.conditions ?? []) {
    validateCondition(where, cond, errors);
  }
  if (ability.operations.length === 0) {
    errors.push(`${where}: ability has no operations`);
  }
  if (ability.operations.length > EFFECT_BOUNDS.maxOperationsPerAbility) {
    errors.push(`${where}: ${ability.operations.length} operations exceeds bound ${EFFECT_BOUNDS.maxOperationsPerAbility}`);
  }
  for (const op of ability.operations) {
    validateOperation(where, op, context, 0, errors);
  }
};

/** Collect every token cardId a card can summon, for cycle detection. */
const summonTargets = (card: CardDefinition): string[] => {
  const out = new Set<string>(card.tokens ?? []);
  const scan = (op: Operation): void => {
    if (op.kind === "summon_token") {
      out.add(op.token);
    }
    if (op.kind === "repeat") {
      scan(op.op);
    }
  };
  for (const ability of [...card.normal, ...card.forged]) {
    for (const op of ability.operations) {
      scan(op);
    }
  }
  return [...out];
};

/** Detect any cycle in the summon graph (a recursive token definition). */
const detectSummonCycles = (cards: readonly CardDefinition[], errors: string[]): void => {
  const graph = new Map<string, string[]>();
  for (const card of cards) {
    graph.set(card.id, summonTargets(card));
  }
  const state = new Map<string, number>(); // 0 visiting, 1 done
  const stack: string[] = [];
  const visit = (id: string): void => {
    if (state.get(id) === 1) {
      return;
    }
    if (state.get(id) === 0) {
      errors.push(`content: recursive token summon cycle: ${[...stack, id].join(" -> ")}`);
      return;
    }
    state.set(id, 0);
    stack.push(id);
    for (const next of graph.get(id) ?? []) {
      if (graph.has(next)) {
        visit(next);
      }
    }
    stack.pop();
    state.set(id, 1);
  };
  for (const card of cards) {
    if (state.get(card.id) !== 1) {
      visit(card.id);
    }
  }
};

/** Validate a bundle; returns all errors (empty ⇒ valid). */
export const validateContent = (bundle: ContentBundle): string[] => {
  const errors: string[] = [];
  if (!Number.isInteger(bundle.version) || bundle.version < 1) {
    errors.push(`content: version must be a positive integer, got ${bundle.version}`);
  }

  dupCheck("archetypes", bundle.archetypes.map((a) => a.id), errors);
  dupCheck("keywords", bundle.keywords.map((k) => k.id), errors);
  dupCheck("groups", bundle.groups.map((g) => g.id), errors);
  dupCheck("visualProfiles", bundle.visualProfiles.map((v) => v.id), errors);
  dupCheck("cards", bundle.cards.map((c) => c.id), errors);

  const groupIds = new Set(bundle.groups.map((g) => g.id));
  const keywordIds = new Set(bundle.keywords.map((k) => k.id));
  const archetypeIds = new Set(bundle.archetypes.map((a) => a.id));
  const profileIds = new Set(bundle.visualProfiles.map((v) => v.id));
  const cardIds = new Set(bundle.cards.map((c) => c.id));

  for (const group of bundle.groups) {
    if (!archetypeIds.has(group.archetype)) {
      errors.push(`group '${group.id}': references missing archetype '${group.archetype}'`);
    }
  }

  for (const card of bundle.cards) {
    const w = `card '${card.id}'`;
    if (!TIERS.includes(card.tier)) {
      errors.push(`${w}: invalid tier ${card.tier} (expected 1..6)`);
    }
    if (card.cost < 0) {
      errors.push(`${w}: negative cost ${card.cost}`);
    }
    if (card.baseAttack < 0) {
      errors.push(`${w}: negative base attack ${card.baseAttack}`);
    }
    if (card.baseHealth < 1) {
      errors.push(`${w}: base health ${card.baseHealth} must be >= 1`);
    }
    if (card.poolCount < 0) {
      errors.push(`${w}: negative pool count ${card.poolCount}`);
    }
    if (card.collectible && card.poolCount < 1) {
      errors.push(`${w}: collectible card must have pool count >= 1`);
    }
    for (const g of card.groups) {
      if (!groupIds.has(g)) {
        errors.push(`${w}: references missing group '${g}'`);
      }
    }
    for (const k of card.keywords) {
      if (!keywordIds.has(k)) {
        errors.push(`${w}: references missing keyword '${k}'`);
      }
    }
    if (!profileIds.has(card.visualProfile)) {
      errors.push(`${w}: references missing visual profile '${card.visualProfile}'`);
    }
    if (!profileIds.has(card.forgedVisualProfile)) {
      errors.push(`${w}: references missing forged visual profile '${card.forgedVisualProfile}'`);
    }
    if (card.normal.length > EFFECT_BOUNDS.maxAbilitiesPerProfile) {
      errors.push(`${w}: too many normal abilities (${card.normal.length})`);
    }
    if (card.forged.length > EFFECT_BOUNDS.maxAbilitiesPerProfile) {
      errors.push(`${w}: too many forged abilities (${card.forged.length})`);
    }
    for (const t of card.tokens ?? []) {
      if (!cardIds.has(t)) {
        errors.push(`${w}: references missing token card '${t}'`);
      }
    }
    card.normal.forEach((ability, i) => validateAbility(`${w} normal[${i}]`, ability, errors));
    card.forged.forEach((ability, i) => validateAbility(`${w} forged[${i}]`, ability, errors));
    // Cross-reference operation card targets.
    const checkOpRefs = (opWhere: string, op: Operation): void => {
      if (op.kind === "summon_token" && !cardIds.has(op.token)) {
        errors.push(`${opWhere}: summons missing token '${op.token}'`);
      }
      if (op.kind === "transform_card" && !cardIds.has(op.into)) {
        errors.push(`${opWhere}: transforms into missing card '${op.into}'`);
      }
      if (op.kind === "repeat") {
        checkOpRefs(opWhere, op.op);
      }
    };
    for (const ability of [...card.normal, ...card.forged]) {
      ability.operations.forEach((op, i) => checkOpRefs(`${w} op[${i}]`, op));
    }
  }

  detectSummonCycles(bundle.cards, errors);
  return errors;
};
