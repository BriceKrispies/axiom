/*
 * envelopes.ts — the transport-neutral command/event envelopes. The UI (and any
 * future remote client) never mutates match state; it sends a `CommandEnvelope`
 * to the authoritative host and receives `EventBatch`es back. These shapes are
 * deliberately serializable and carry the sequence numbers a real network needs
 * for ordering, dedup, and reconnect — so swapping the in-process local host for
 * a remote authoritative host changes only the transport, not these contracts.
 */

import type { Command } from "../sim/commands.ts";
import type { SimEvent } from "../sim/events.ts";
import type { PlayerId } from "../sim/ids.ts";

/** A command as it crosses the authority boundary: the authenticated player, the
 * client-assigned command sequence, and the command itself. */
export interface CommandEnvelope {
  readonly clientSeq: number;
  readonly playerId: PlayerId;
  readonly command: Command;
}

/** A contiguous batch of events from a cursor, with the next cursor to request. */
export interface EventBatch {
  readonly events: readonly SimEvent[];
  readonly cursor: number;
}
