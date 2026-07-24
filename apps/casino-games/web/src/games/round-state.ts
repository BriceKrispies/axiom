/*
 * round-state.ts — the PURE half of the game harness: the per-tick fold every
 * mounted game shares. No engine value imports live here (engine shapes appear
 * as types only), so game controllers and the session tests exercise this
 * logic under bare `node --test` with no DOM. `casino-mount.ts` is the thin
 * impure shell that runs this fold inside the engine's `runGame`.
 *
 * What the fold owns (identically for all 20 games):
 * - tick advance + intro → ready;
 * - the commitment hand-off: a game enters "committing" with a context; the
 *   fold resolves it through the result source (commit-once is enforced by
 *   the session layer) and advances to the reveal or the sealed interaction;
 * - hard input-locking during protected phases;
 * - celebrating → complete on the resolved celebration clock;
 * - the reset flow (New Round / Replay Same Seed → a genuinely new session).
 */

import type { GameResources, InputFrame, Scene, TickContext, ToneSpec, ViewContext } from "@axiom/web-engine";
import type { CasinoGameConfig, Rarity } from "../chance-engine/configuration/schema.ts";
import type { OutcomeResolutionContext } from "../chance-engine/outcomes/plan.ts";
import type { ChanceResultSource, MechanicInit } from "../chance-engine/outcomes/result-source.ts";
import type { CasinoHud, PresentationSettings } from "../chance-engine/registry/definition.ts";
import type { SessionState } from "../chance-engine/sessions/session.ts";
import {
  auditOf,
  commitOutcome,
  createSession,
  inputLocked,
  phaseAge,
  tickSession,
  transition,
} from "../chance-engine/sessions/session.ts";
import type { ResolvedCelebration } from "../presentation/celebrations/intensity.ts";
import { celebrationOf } from "../presentation/celebrations/intensity.ts";
import { resultTextOf } from "../presentation/rewards/tiers.ts";

/** The state every mounted game folds: the session, the game's own animation
 * state, the pending commitment context, and the pending reset request. */
export interface CasinoState<TExtra> {
  readonly session: SessionState;
  readonly extra: TExtra;
  readonly pendingContext: OutcomeResolutionContext | null;
  readonly pendingReset: { readonly round: number; readonly replay: boolean } | null;
}

/** What one game supplies to the harness. `step` runs AFTER the harness's own
 * phase mechanics each tick, with input already stripped when locked. */
export interface CasinoMountSpec<TExtra> {
  readonly resources: GameResources;
  /** Extra key bindings merged over the shared set. */
  readonly actions?: Readonly<Record<string, readonly string[]>>;
  readonly mechanic: MechanicInit;
  /** After commitment resolves: go straight to the reveal, or hand control
   * back to the player with the outcome already sealed (scratch cards). */
  readonly afterCommit?: "reveal" | "interact";
  /** How long the sealed-but-not-yet-revealed pause lasts, in ticks (before
   * speed scaling). Defaults to `COMMIT_PAUSE_TICKS` — the beat most games
   * want. A game whose commit beat carries a real staging animation (the
   * chest's spiral to the hero framing) declares the length that animation
   * needs here, rather than racing the shared default. The outcome is already
   * committed either way, so this is purely how long the presentation holds
   * before the reveal; it can never change what was drawn. */
  readonly commitPauseTicks?: number;
  /** Build the game's per-round animation state. `previous` is the prior round's
   * extra on a New Round / Replay reset (null on the first round of a page load),
   * so a game can carry forward anything that should outlive a single round
   * (e.g. player-placed decor positions) while resetting the rest. */
  readonly initExtra: (session: SessionState, previous: TExtra | null) => TExtra;
  readonly step: (state: CasinoState<TExtra>, input: InputFrame, ctx: TickContext) => CasinoState<TExtra>;
  readonly viewScene: (state: CasinoState<TExtra>, ctx: ViewContext) => Scene;
  readonly instructionOf?: (state: CasinoState<TExtra>) => string | null;
  readonly sound?: (prev: CasinoState<TExtra>, next: CasinoState<TExtra>) => readonly ToneSpec[];
}

/** Everything the pure fold needs from the mount (a subset of GameRuntime). */
export interface RoundEnvironment {
  readonly config: CasinoGameConfig<unknown>;
  readonly seed: number;
  readonly source: ChanceResultSource;
  readonly settings: PresentationSettings;
}

/** Shared bindings: primary action, choice navigation, round controls. The
 * `Synthetic:` codes are fed by the DOM shell's buttons and touch surfaces. */
export const COMMON_ACTIONS: Readonly<Record<string, readonly string[]>> = {
  down: ["ArrowDown", "KeyS"],
  left: ["ArrowLeft", "KeyA"],
  newRound: ["KeyN", "Synthetic:NewRound"],
  primary: ["Space", "Enter", "Synthetic:Primary"],
  replaySeed: ["KeyR", "Synthetic:Replay"],
  right: ["ArrowRight", "KeyD"],
  up: ["ArrowUp", "KeyW"],
};

const EMPTY_SET: ReadonlySet<string> = new Set();

/** The input frame a game sees while the session is input-locked: nothing. */
export const lockedFrame = (input: InputFrame): InputFrame => ({
  down: EMPTY_SET,
  look: input.look,
  pointer: undefined,
  pressed: EMPTY_SET,
  released: EMPTY_SET,
});

/** Ticks scaled by the config's presentation speed (faster speed = fewer). */
export const speedTicks = (base: number, presentationSpeed: number): number =>
  Math.max(1, Math.round(base / presentationSpeed));

export const INTRO_TICKS = 24;
export const COMMIT_PAUSE_TICKS = 14;
export const RESET_TICKS = 16;

/** The rarity of a committed outcome, or "loss". */
export const outcomeRarity = (session: SessionState): Rarity | "loss" => {
  const plan = session.committed;
  if (plan === null || !plan.win || plan.tierId === null) {
    return "loss";
  }
  return session.config.rewardTiers.find((tier) => tier.id === plan.tierId)?.rarity ?? "common";
};

/** The resolved celebration for the current outcome under these settings. */
export const celebrationFor = (settings: PresentationSettings, session: SessionState): ResolvedCelebration =>
  celebrationOf(outcomeRarity(session), {
    cameraShake: settings.cameraShake,
    celebrationIntensity: session.config.celebrationIntensity,
    particleScale: settings.particleScale,
    presentationSpeed: session.config.presentationSpeed,
    reducedMotion: settings.reducedMotion,
  });

const canResetFrom = (phase: SessionState["phase"]): boolean =>
  phase === "ready" || phase === "interacting" || phase === "celebrating" || phase === "complete";

export const freshRoundState = <TExtra>(
  env: RoundEnvironment,
  spec: CasinoMountSpec<TExtra>,
  round: number,
  replay: boolean,
  previous: TExtra | null = null,
): CasinoState<TExtra> => {
  const session = createSession(env.config, env.seed, round, env.source, spec.mechanic, replay);
  return { extra: spec.initExtra(session, previous), pendingContext: null, pendingReset: null, session };
};

/** The harness's own phase mechanics, run before the game's step. */
const harnessStep = <TExtra>(env: RoundEnvironment, spec: CasinoMountSpec<TExtra>, state: CasinoState<TExtra>): CasinoState<TExtra> => {
  const speed = env.config.presentationSpeed;
  const s: CasinoState<TExtra> = { ...state, session: tickSession(state.session) };
  const session = s.session;
  const age = phaseAge(session);

  if (session.phase === "intro" && age >= speedTicks(INTRO_TICKS, speed)) {
    return { ...s, session: transition(session, "ready") };
  }
  if (session.phase === "committing") {
    if (session.committed === null && s.pendingContext !== null) {
      return { ...s, session: commitOutcome(session, env.source, s.pendingContext) };
    }
    if (session.committed !== null && age >= speedTicks(spec.commitPauseTicks ?? COMMIT_PAUSE_TICKS, speed)) {
      const next = spec.afterCommit === "interact" ? "interacting" : "revealing";
      return { ...s, session: transition(session, next) };
    }
    return s;
  }
  if (session.phase === "celebrating" && age >= celebrationFor(env.settings, session).durationTicks) {
    return { ...s, session: transition(session, "complete") };
  }
  if (session.phase === "resetting" && age >= speedTicks(RESET_TICKS, speed) && s.pendingReset !== null) {
    // Carry the prior extra into the fresh round so a game can persist anything
    // that should outlive the round (the chest game keeps its moved decor here).
    return freshRoundState(env, spec, s.pendingReset.round, s.pendingReset.replay, s.extra);
  }
  return s;
};

/** One full harness tick: phase mechanics, input lock, reset controls, then
 * the game's own step. This is the engine-`Game`'s `update`. */
export const foldRoundTick = <TExtra>(
  env: RoundEnvironment,
  spec: CasinoMountSpec<TExtra>,
  state: CasinoState<TExtra>,
  input: InputFrame,
  ctx: TickContext,
): CasinoState<TExtra> => {
  const stepped = harnessStep(env, spec, state);
  const session = stepped.session;
  const locked = inputLocked(session);
  const frame = locked ? lockedFrame(input) : input;

  const wantsReplay = frame.pressed.has("replaySeed");
  const wantsNew = frame.pressed.has("newRound") || (session.phase === "complete" && frame.pressed.has("primary"));
  if ((wantsNew || wantsReplay) && canResetFrom(session.phase)) {
    const replay = wantsReplay && !wantsNew;
    return {
      ...stepped,
      pendingReset: { replay, round: replay ? session.round : session.round + 1 },
      session: transition(session, "resetting"),
    };
  }
  return spec.step(stepped, frame, ctx);
};

const PHASE_INSTRUCTIONS: Partial<Record<SessionState["phase"], string>> = {
  celebrating: "",
  committing: "…",
  complete: "New Round (N) · Replay Same Seed (R)",
  intro: "Get ready…",
  resetting: "Setting up a fresh round…",
  revealing: "…",
};

/** The DOM shell's HUD projection of a round state. */
export const hudOf = <TExtra>(
  spec: CasinoMountSpec<TExtra>,
  sourceKind: "seeded" | "injected",
  state: CasinoState<TExtra>,
): CasinoHud => {
  const session = state.session;
  const plan = session.committed;
  const revealed = session.phase === "celebrating" || session.phase === "complete";
  const rarity = outcomeRarity(session);
  return {
    audit: auditOf(session, sourceKind),
    inputLocked: inputLocked(session),
    instruction: spec.instructionOf?.(state) ?? PHASE_INSTRUCTIONS[session.phase] ?? "",
    phase: session.phase,
    rarity: revealed && plan !== null && plan.win && rarity !== "loss" ? rarity : null,
    resultText: revealed && plan !== null ? resultTextOf(plan) : null,
    round: session.round,
    tierId: revealed ? (plan?.tierId ?? null) : null,
    win: revealed ? (plan?.win ?? null) : null,
  };
};
