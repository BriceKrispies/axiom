/*
 * The host bridge and persistence free functions (SPEC-12 §4.2). These bind the
 * runtime app's host channel through the installed `HostBridge` (`host-binding.ts`):
 * read the session config, signal first-frame readiness, and report the terminal
 * outcome(s).
 *
 * `reportOutcome` / `reportOutcomes` are emit-exactly-once: the session latch
 * (`latchOutcome`) lets the first call through and no-ops every later one, so a
 * game cannot report two terminal states. The conditional forward is branchless
 * — the chosen thunk is selected by slicing a one-element array to length 0 or 1.
 */

import { type Outcome, type SessionConfig, boundHost, latchOutcome } from "./host-binding.ts";
import type { PlayerId } from "./vocabulary.ts";
import { each } from "./branchless.ts";

/** Read the host's session configuration (seed + opaque params), SPEC-12 §4.2. */
export const getSessionConfig = (): SessionConfig => boundHost().getSessionConfig();

/** Signal that the first frame can render (SPEC-12 §4.2). */
export const notifyReady = (): void => {
  boundHost().notifyReady();
};

/*
 * Forward `emit` (a 0-argument thunk) to the host only if this is the first
 * terminal report of the session. `latchOutcome()` is `true` exactly once;
 * slicing `[emit]` to length `Number(!first)` keeps the thunk (first) or drops
 * it (later) — a branchless emit-exactly-once.
 */
const emitOnce = (emit: () => void): void => {
  const first = latchOutcome();
  each([emit].slice(Number(!first)), (run): void => {
    run();
  });
};

/** Emit the single terminal outcome exactly once (SPEC-12 §4.2). */
export const reportOutcome = (outcome: Outcome): void => {
  emitOnce((): void => {
    boundHost().reportOutcome(outcome);
  });
};

/** Emit the per-player room outcomes exactly once (SPEC-12 §4.2 / §16.6). */
export const reportOutcomes = (results: Readonly<Record<PlayerId, Outcome>>): void => {
  emitOnce((): void => {
    boundHost().reportOutcomes(results);
  });
};
