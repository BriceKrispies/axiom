/*
 * match-api.ts — the transport-neutral authoritative match interface. Every actor
 * (the human UI, a bot, a future remote client) talks to the match ONLY through
 * this surface: submit a command, read the current view, pull events since a
 * cursor, ask whether the match is over. It is deliberately free of any
 * networking, DOM, or renderer concept. The in-process `LocalMatchHost`
 * implements it today; a remote authoritative server would implement the exact
 * same interface over a socket, and the client code above it would not change.
 */

import type { CommandResult } from "../sim/commands.ts";
import type { MatchState } from "../sim/model.ts";
import type { CommandEnvelope, EventBatch } from "./envelopes.ts";

export interface MatchApi {
  /** Submit one authenticated command; returns accept/reject. Illegal commands
   * are rejected transactionally and never change unrelated state. */
  submit(env: CommandEnvelope): CommandResult;
  /** The current authoritative match view (read-only). */
  view(): MatchState;
  /** Events with `seq >= cursor`, plus the next cursor to request. */
  eventsSince(cursor: number): EventBatch;
  /** Whether the match has reached `match_complete`. */
  isComplete(): boolean;
}
