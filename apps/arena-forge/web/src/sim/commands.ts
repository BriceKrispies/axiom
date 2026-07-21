/*
 * commands.ts — the player command vocabulary. A command is the ONLY way any
 * actor (the human, a bot, or a future remote client) changes match state: the
 * UI never mutates state directly, it submits one of these through the
 * authoritative match API (`../api/match-api.ts`). Commands carry no player id —
 * the API wraps them in a `CommandEnvelope` that supplies the authenticated
 * player and the match-wide command sequence number.
 *
 * Every command is a shop-phase action; the phase machine rejects any command
 * that arrives outside `shop`. Validation lives with the systems that own the
 * affected state (economy / shop / forge), and every rejection is an explicit
 * `command_rejected` event carrying a stable `RejectionReason` — a cancelled or
 * illegal action never silently loses the player gold or a card.
 */

import type { InstanceId } from "./ids.ts";

/** Where a bought card lands. `hand` fills the next hand slot; `warband` targets
 * a specific slot (buy-and-place in one gesture, e.g. a drag). */
export type BuyDestination = { readonly to: "hand" } | { readonly to: "warband"; readonly slot: number };

/** A single player command. Discriminated by `type`; the applier switches on it. */
export type Command =
  | { readonly type: "buy"; readonly shopIndex: number; readonly destination: BuyDestination }
  | { readonly type: "sell"; readonly instanceId: InstanceId }
  | { readonly type: "reroll" }
  | { readonly type: "set_freeze"; readonly frozen: boolean }
  | { readonly type: "upgrade_forge_rank" }
  | { readonly type: "play_card"; readonly instanceId: InstanceId; readonly slot: number }
  | { readonly type: "return_to_hand"; readonly instanceId: InstanceId }
  | { readonly type: "reorder"; readonly instanceId: InstanceId; readonly slot: number };

export type CommandType = Command["type"];

export const COMMAND_TYPES: readonly CommandType[] = [
  "buy",
  "sell",
  "reroll",
  "set_freeze",
  "upgrade_forge_rank",
  "play_card",
  "return_to_hand",
  "reorder",
];

/** Stable machine-readable rejection reasons (surfaced in `command_rejected`). */
export const REJECT = {
  wrongPhase: "wrong_phase",
  notActive: "player_eliminated",
  unknownPlayer: "unknown_player",
  badShopIndex: "bad_shop_index",
  notEnoughGold: "not_enough_gold",
  handFull: "hand_full",
  warbandFull: "warband_full",
  slotOccupied: "slot_occupied",
  badSlot: "bad_slot",
  unknownInstance: "unknown_instance",
  notInHand: "not_in_hand",
  notInWarband: "not_in_warband",
  maxForgeRank: "max_forge_rank",
  nothingToReturn: "nothing_to_return",
  noChange: "no_change",
} as const;

export type RejectionReason = (typeof REJECT)[keyof typeof REJECT];

/** The outcome of applying a command. On success it lists the events produced
 * (already appended to the log); on failure it carries the reason (an explicit
 * `command_rejected` event is also emitted). */
export type CommandResult = { readonly ok: true } | { readonly ok: false; readonly reason: RejectionReason };
