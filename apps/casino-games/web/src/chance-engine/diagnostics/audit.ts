/*
 * audit.ts — the internal per-session audit record. Assembled as the session
 * progresses and exposed ONLY through the development diagnostics drawer
 * (?debug=1); ordinary player mode never renders it, and nothing reads it to
 * influence play. It exists so a completed round is fully accountable:
 * what was configured, what was committed, when, and what manifested.
 */

import type { GamePhase } from "../sessions/phases.ts";
import type { MechanicManifestation, OutcomeResolutionContext } from "../outcomes/plan.ts";
import type { StreamPurpose } from "../randomness/streams.ts";

export interface SessionAuditRecord {
  readonly gameId: string;
  readonly schemaVersion: number;
  /** Stable hash of the exact configuration the round ran under. */
  readonly configHash: string;
  /** The seeded session seed, or the injected round id. */
  readonly seedOrRoundId: string;
  readonly round: number;
  /** Derived per-purpose stream seeds (diagnostics: proves independence). */
  readonly streamSeeds: Readonly<Record<StreamPurpose, number>>;
  /** Where and when the outcome was committed (null until commitment). */
  readonly commitPhase: GamePhase | null;
  readonly commitTick: number | null;
  /** The significant player input supplied at commitment. */
  readonly inputContext: OutcomeResolutionContext | null;
  readonly win: boolean | null;
  readonly tierId: string | null;
  readonly manifestation: MechanicManifestation | null;
  readonly completePhase: GamePhase | null;
  readonly completeTick: number | null;
  /** True when this session replays an earlier seed ("Replay Same Seed"). */
  readonly replay: boolean;
}
