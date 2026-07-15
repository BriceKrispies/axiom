/*
 * phases.ts — the explicit game-phase machine every casino game runs on.
 * The phases and their legal transitions are fixed here; `session.ts` throws
 * on any illegal transition, so a game cannot invent its own flow (or skip
 * commitment). Input is hard-locked during the protected phases.
 */

export type GamePhase =
  | "intro"
  | "ready"
  | "committing"
  | "interacting"
  | "revealing"
  | "celebrating"
  | "complete"
  | "resetting";

export const GAME_PHASES: readonly GamePhase[] = [
  "intro",
  "ready",
  "committing",
  "interacting",
  "revealing",
  "celebrating",
  "complete",
  "resetting",
];

/**
 * The legal transition graph. "interacting" covers both pre-commitment play
 * (aiming, steering the claw) and post-commitment interaction whose outcome is
 * already sealed (scratching an already-committed ticket). "resetting" is
 * reachable from any unprotected phase (New Round / Replay). The session layer
 * additionally guards that "revealing" is unreachable without a committed
 * outcome (see `transition` in session.ts).
 */
export const LEGAL_TRANSITIONS: Readonly<Record<GamePhase, readonly GamePhase[]>> = {
  celebrating: ["complete", "resetting"],
  committing: ["revealing", "interacting"],
  complete: ["resetting"],
  interacting: ["committing", "revealing", "resetting"],
  intro: ["ready"],
  ready: ["interacting", "committing", "resetting"],
  resetting: ["ready"],
  revealing: ["celebrating"],
};

export const isLegalTransition = (from: GamePhase, to: GamePhase): boolean =>
  LEGAL_TRANSITIONS[from].includes(to);

/** Phases during which player input must be ignored (reveal is locked). */
export const INPUT_LOCKED_PHASES: readonly GamePhase[] = ["committing", "revealing", "resetting"];

export const isInputLockedPhase = (phase: GamePhase): boolean => INPUT_LOCKED_PHASES.includes(phase);
