/*
 * phase.ts — the legal phase-transition table of the match state machine. The
 * `Match` orchestrator produces transitions by construction; this table is the
 * independent specification the harness and tests check the produced transition
 * log against, so an illegal transition is caught mechanically rather than
 * trusted. It is the machine-readable form of the phase diagram documented in
 * GAME_RULES.md.
 */

import type { Phase } from "./model.ts";

/** Every legal `from → to` phase edge. Anything else is an illegal transition. */
export const LEGAL_TRANSITIONS: Readonly<Record<Phase, readonly Phase[]>> = {
  lobby: ["shop"],
  shop: ["combat_prepare"],
  combat_prepare: ["combat"],
  combat: ["combat_resolve"],
  combat_resolve: ["round_transition"],
  round_transition: ["shop", "match_complete"],
  match_complete: [],
};

export const isLegalTransition = (from: Phase, to: Phase): boolean => (LEGAL_TRANSITIONS[from] ?? []).includes(to);
