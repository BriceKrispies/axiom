/*
 * contract.ts — the wire shape of `/api/pick` and `/api/new`, declared ONCE.
 *
 * The stand-in server (`tools/axiom-chest-server`) imports these types and the
 * enhanced tiers of `resilient/main.ts` import them too, so producer and
 * consumer cannot drift: there is no second, hand-copied definition of the
 * payload to keep in sync.
 *
 * This file is types + tiny pure helpers only. It touches no DOM and no Node
 * API, which is what lets both sides have it.
 *
 * THE INVARIANT THAT MATTERS: every tier POSTs to the same endpoint and gets
 * the same decision. The zero-JS form navigation and the `fetch` call differ
 * only in what the SERVER renders back (an HTML page vs. this JSON) — never in
 * how the outcome was decided. What we test is what ships.
 */

/** The reward behind one chest, once it is open. */
export interface RevealedReward {
  readonly tierId: string;
  readonly tierLabel: string;
  readonly rarity: string;
  readonly rewardLabel: string;
}

/** One chest's post-round state. `reward` is null for an empty chest. */
export interface RevealedChest {
  readonly index: number;
  readonly reward: RevealedReward | null;
}

/** The answer to a pick: what you opened, and what the whole board held. */
export interface PickResponse {
  readonly kind: "pick";
  /** Which chest the player opened. */
  readonly picked: number;
  readonly won: boolean;
  readonly reward: RevealedReward | null;
  /** The full board, revealed after the fact — the population was committed
   * before the pick, so showing it cannot leak an outcome that was not already
   * decided. */
  readonly board: readonly RevealedChest[];
  readonly winnerCount: number;
  readonly chestCount: number;
  readonly seed: number;
  readonly round: number;
  readonly targetWinRate: number;
  /** Whether this response replayed an already-recorded pick (a refresh, a
   * double POST) rather than deciding a new one. */
  readonly replay: boolean;
}

/** The answer to "deal me a fresh board". */
export interface RoundResponse {
  readonly kind: "round";
  readonly chestCount: number;
  readonly seed: number;
  readonly round: number;
  readonly targetWinRate: number;
}

/** Something the request asked for that the server will not do. */
export interface ErrorResponse {
  readonly kind: "error";
  readonly message: string;
}

export type ApiResponse = PickResponse | RoundResponse | ErrorResponse;

/** The endpoints, named once so neither side hard-codes a string twice. */
export const PICK_ENDPOINT = "/api/pick";
export const NEW_ROUND_ENDPOINT = "/api/new";

/** The form field a chest button submits. Identical on every tier: the JSON
 * body uses the same key, so the two paths are one path. */
export const PICK_FIELD = "pick";

/**
 * Read a submitted pick out of raw field text. Shared so the server's
 * urlencoded parse and its JSON parse cannot disagree about what "5" means, and
 * so "out of range" is one rule rather than two.
 */
export const parsePick = (raw: string | undefined, chestCount: number): number | null => {
  const trimmed = (raw ?? "").trim();
  const value = Number(trimmed);
  const valid = trimmed !== "" && Number.isInteger(value) && value >= 0 && value < chestCount;
  return valid ? value : null;
};
