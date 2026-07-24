/*
 * power.ts — the Battle Simulator's deterministic power model and enemy matcher.
 * "Power" is a single integer summarizing how strong a warband is, so the sim can
 * build an ENEMY team of roughly equal strength for any preset — a 3-unit elite
 * squad and a 7-unit swarm each get a fair fight. It is pure, seeded, and DOM-free
 * (unit-tested headless): the same target + seed always yield the same enemy team.
 *
 * The model reuses the same inputs combat itself reads — a unit's attack, health,
 * and defensive keywords — so "equal power" tracks the numbers that actually decide
 * fights, not a cosmetic score. It draws only from the shared card pool (collectible
 * cards with copies), the same universe the real game rolls from.
 */

import type { LoadedContent } from "../../sim/content/load.ts";
import { Rng } from "../../sim/rng.ts";
import type { PresetUnit } from "./presets.ts";

/** Defensive keywords are worth extra power beyond raw stats — a guard soaks hits
 * and armored halves them, both materially harder to kill than the stat line says. */
const KEYWORD_POWER: Readonly<Record<string, number>> = { guard: 2, armored: 3 };

/** The number of slots a warband can field (mirrors sim WARBAND_SLOTS). */
const TEAM_SLOTS = 7;

/** How many "closest to ideal" candidates the enemy builder samples per slot. A
 * small window keeps every pick near the power target while the seed still varies
 * WHICH near-ideal unit is chosen, so rematches produce different-but-fair teams. */
const CANDIDATE_WINDOW = 6;

/** Stop adding enemy units once we are within this much of the target power. */
const CLOSE_ENOUGH = 3;

/** The power contribution of a single unit at the given forge state. */
export const unitPower = (content: LoadedContent, cardId: string, forged: boolean): number => {
  const card = content.card(cardId);
  const attack = card.baseAttack + (forged ? card.forgedStats.attack : 0);
  const health = card.baseHealth + (forged ? card.forgedStats.health : 0);
  const keywords = card.keywords.reduce((sum, k) => sum + (KEYWORD_POWER[k] ?? 0), 0);
  return attack + health + keywords;
};

/** The total power of a team of preset units. */
export const teamPower = (content: LoadedContent, units: readonly PresetUnit[]): number =>
  units.reduce((sum, unit) => sum + unitPower(content, unit.cardId, unit.forged), 0);

/** A stable unsigned-32-bit hash of a string, so a preset id can seed the RNG. */
export const hashString = (s: string): number => {
  let h = 0x811c9dc5 >>> 0;
  for (let i = 0; i < s.length; i += 1) {
    h = (h ^ s.charCodeAt(i)) >>> 0;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
};

interface Candidate {
  readonly cardId: string;
  readonly forged: boolean;
  readonly power: number;
}

/** Every collectible card as both a normal and a forged candidate. */
const candidatePool = (content: LoadedContent): Candidate[] =>
  content.collectibleCards.flatMap((card) => [
    { cardId: card.id, forged: false, power: unitPower(content, card.id, false) },
    { cardId: card.id, forged: true, power: unitPower(content, card.id, true) },
  ]);

/**
 * Build an enemy team whose total power lands near `target`, drawn deterministically
 * from `seed`. Greedy per-slot: it aims each pick at the power still needed spread
 * over the slots remaining, chooses among the units closest to that ideal (the seed
 * breaks the tie for variety), and stops early once close enough — so a low target
 * yields a small team of weak units and a high target a strong or wide one.
 */
export const buildEnemyTeam = (content: LoadedContent, target: number, seed: number): PresetUnit[] => {
  const rng = new Rng(seed >>> 0);
  const pool = candidatePool(content);
  const team: PresetUnit[] = [];
  let power = 0;
  for (let slot = 0; slot < TEAM_SLOTS; slot += 1) {
    const remaining = target - power;
    if (remaining <= CLOSE_ENOUGH) {
      break;
    }
    const slotsLeft = TEAM_SLOTS - slot;
    const ideal = remaining / slotsLeft;
    const ranked = pool
      .slice()
      .sort((a, b) => Math.abs(a.power - ideal) - Math.abs(b.power - ideal));
    const window = ranked.slice(0, Math.min(CANDIDATE_WINDOW, ranked.length));
    const choice = rng.pick(window) ?? ranked[0];
    if (choice === undefined) {
      break;
    }
    team.push({ cardId: choice.cardId, forged: choice.forged });
    power += choice.power;
  }
  return team;
};
