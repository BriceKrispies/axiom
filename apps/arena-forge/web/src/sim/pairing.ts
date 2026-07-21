/*
 * pairing.ts — opponent reseeding and pairing. After each resolution, active
 * players are reseeded deterministically (health desc, forge rank desc, warband
 * power desc, stable id asc), then paired adjacently. Immediate rematches are
 * avoided by a deterministic local swap when a valid one exists — never at the
 * cost of determinism. An odd active count pairs the lowest-seeded player against
 * a GHOST: the most recent snapshot of the highest-placing eliminated player not
 * used as last round's ghost. A ghost cannot take player damage.
 */

import type { LoadedContent } from "./content/load.ts";
import type { MatchState, Pairing, PlayerState, WarbandSnapshot } from "./model.ts";
import type { PlayerId } from "./ids.ts";
import { warbandPower } from "./stage.ts";

const player = (state: MatchState, id: PlayerId): PlayerState => state.players[id] as PlayerState;

/** The deterministic seeding order of the currently active players. */
export const reseedActive = (state: MatchState): PlayerId[] =>
  state.players
    .filter((p) => !p.eliminated)
    .slice()
    .sort(
      (a, b) =>
        b.health - a.health ||
        b.forgeRank - a.forgeRank ||
        warbandPower(b) - warbandPower(a) ||
        a.id - b.id,
    )
    .map((p) => p.id);

export interface GhostStore {
  /** Last valid warband snapshot of each eliminated player. */
  readonly snapshots: Map<PlayerId, WarbandSnapshot>;
}

/** Choose the ghost: highest-placing eliminated player (lowest placement number)
 * with a stored snapshot, excluding last round's ghost. Null ⇒ no ghost (a bye). */
export const chooseGhost = (state: MatchState, store: GhostStore, excluded: PlayerId | null): PlayerId | null => {
  const candidates = state.players
    .filter((p) => p.eliminated && p.id !== excluded && store.snapshots.has(p.id))
    .slice()
    .sort((a, b) => a.placement - b.placement || a.id - b.id);
  return candidates.length > 0 ? (candidates[0] as PlayerState).id : null;
};

/**
 * Compute this round's pairings from the reseeded active order. Returns the
 * pairings and the ghost player id chosen this round (for rematch bookkeeping).
 */
export const computePairings = (
  state: MatchState,
  _content: LoadedContent,
  store: GhostStore,
  ghostUsedLastRound: PlayerId | null,
): { readonly pairings: Pairing[]; readonly ghostChosen: PlayerId | null } => {
  const order = reseedActive(state);
  let ghostFor: PlayerId | null = null;
  if (order.length % 2 === 1) {
    ghostFor = order.pop() ?? null; // lowest-seeded plays the ghost
  }

  const pairings: Pairing[] = [];
  for (let i = 0; i + 1 < order.length; i += 2) {
    const a = order[i] as PlayerId;
    // Avoid an immediate rematch by swapping the partner with a later player.
    if (player(state, a).lastOpponent === order[i + 1]) {
      for (let j = i + 2; j < order.length; j += 1) {
        if (player(state, a).lastOpponent !== order[j] && player(state, order[j] as PlayerId).lastOpponent !== a) {
          const tmp = order[i + 1] as PlayerId;
          order[i + 1] = order[j] as PlayerId;
          order[j] = tmp;
          break;
        }
      }
    }
    pairings.push({ a, b: order[i + 1] as PlayerId, ghostOf: null });
  }

  let ghostChosen: PlayerId | null = null;
  if (ghostFor !== null) {
    ghostChosen = chooseGhost(state, store, ghostUsedLastRound);
    pairings.push({ a: ghostFor, b: null, ghostOf: ghostChosen });
  }
  return { pairings, ghostChosen };
};
