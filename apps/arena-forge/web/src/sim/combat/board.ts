/*
 * board.ts — the live combat battlefield and its helpers. A `Board` is two
 * seven-slot sides of `CombatUnit | null`; empty slots are meaningful positions,
 * so units keep their slot index and moves/summons target specific slots. Combat
 * mutates these transient `CombatUnit`s only — it is built from an immutable
 * warband snapshot and never writes back to player state. Every helper reads the
 * board in stable slot order so combat is fully deterministic.
 */

import type { LoadedContent } from "../content/load.ts";
import type { CombatSide } from "../events.ts";
import type { CardId, InstanceId, KeywordId } from "../ids.ts";
import type { UnitInstance, WarbandSnapshot } from "../model.ts";
import { WARBAND_SLOTS } from "../model.ts";

/** A transient fighting unit. `hasAttacked` tracks the current attack cycle. */
export interface CombatUnit {
  readonly instanceId: InstanceId;
  cardId: CardId;
  readonly side: CombatSide;
  slot: number;
  attack: number;
  health: number;
  forged: boolean;
  alive: boolean;
  hasAttacked: boolean;
  keywords: KeywordId[];
}

export interface Board {
  readonly a: (CombatUnit | null)[];
  readonly b: (CombatUnit | null)[];
}

export const GUARD: KeywordId = "guard";
export const ARMORED: KeywordId = "armored";

export const sideOf = (board: Board, side: CombatSide): (CombatUnit | null)[] => (side === "a" ? board.a : board.b);

export const other = (side: CombatSide): CombatSide => (side === "a" ? "b" : "a");

/** Living units on a side, in slot order. */
export const living = (board: Board, side: CombatSide): CombatUnit[] =>
  sideOf(board, side).filter((u): u is CombatUnit => u !== null && u.alive);

export const unitKeywords = (content: LoadedContent, unit: CombatUnit): KeywordId[] => [
  ...content.card(unit.cardId).keywords,
  ...unit.keywords,
];

export const hasKeyword = (content: LoadedContent, unit: CombatUnit, keyword: KeywordId): boolean =>
  unitKeywords(content, unit).includes(keyword);

const cloneUnit = (u: UnitInstance): UnitInstance => ({
  instanceId: u.instanceId,
  cardId: u.cardId,
  forged: u.forged,
  attack: u.attack,
  health: u.health,
  grantedKeywords: u.grantedKeywords.slice(),
  visualStage: u.visualStage,
});

/** Deep-copy a player's current warband into an immutable combat snapshot. */
export const snapshotWarband = (
  ownerId: number,
  forgeRank: number,
  warband: readonly (UnitInstance | null)[],
  ghost: boolean,
): WarbandSnapshot => ({
  ownerId,
  forgeRank,
  ghost,
  slots: warband.map((u) => (u === null ? null : cloneUnit(u))),
});

/** Instantiate a side's live units from its snapshot. */
export const buildSide = (snapshot: WarbandSnapshot, side: CombatSide): (CombatUnit | null)[] =>
  snapshot.slots.map((u, slot) =>
    u === null
      ? null
      : {
          instanceId: u.instanceId,
          cardId: u.cardId,
          side,
          slot,
          attack: u.attack,
          health: u.health,
          forged: u.forged,
          alive: true,
          hasAttacked: false,
          keywords: u.grantedKeywords.slice(),
        },
  );

/** The empty slot on `side` nearest to `anchor` (ties resolve toward slot 0),
 * or -1 if the side is full. Used to place summons "at the closest empty slot". */
export const nearestEmptySlot = (board: Board, side: CombatSide, anchor: number): number => {
  const arr = sideOf(board, side);
  let best = -1;
  let bestDist = WARBAND_SLOTS + 1;
  for (let i = 0; i < arr.length; i += 1) {
    if (arr[i] === null) {
      const dist = Math.abs(i - anchor);
      if (dist < bestDist) {
        bestDist = dist;
        best = i;
      }
    }
  }
  return best;
};

/** Sum of the tier of each living unit on a side — a consequence-formula input. */
export const survivingTierSum = (content: LoadedContent, board: Board, side: CombatSide): number =>
  living(board, side).reduce((sum, u) => sum + content.card(u.cardId).tier, 0);

/** Build a single fresh combat unit from a card id (used for summoned tokens). */
export const buildSideUnit = (
  content: LoadedContent,
  allocate: () => InstanceId,
  cardId: CardId,
  side: CombatSide,
  slot: number,
): CombatUnit => {
  const def = content.card(cardId);
  return {
    instanceId: allocate(),
    cardId,
    side,
    slot,
    attack: def.baseAttack,
    health: def.baseHealth,
    forged: false,
    alive: true,
    hasAttacked: false,
    keywords: [],
  };
};
