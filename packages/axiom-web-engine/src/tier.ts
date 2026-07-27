/*
 * tier.ts — the capability LADDER and the pure decision that walks it. No DOM,
 * no probing, no timers: given a record of per-tier probe outcomes and a
 * crash-guard ceiling, this file says which tier the engine runs at. Every rule
 * that decides what a user actually sees lives here, under `node --test`,
 * rather than inside the browser-only probe files where it could only ever be
 * verified by hand.
 *
 * The ladder, best first:
 *
 *     webgpu → webgl2 → webgl1 → canvas2d → css3d
 *
 * `css3d` is the terminal rung and the fail-safe: it needs no drawing context
 * at all (see `backend-css.ts`), so it is what the engine falls back to when
 * every probe above it failed, threw, or was never allowed to run.
 *
 * The ceiling is the crash guard's contribution. `override.ts` writes a
 * sentinel before a tier is initialised and clears it once a frame has actually
 * been drawn; a sentinel still present at boot means the previous attempt died
 * mid-init, so the ladder starts one rung BELOW the tier that died. A machine
 * that hard-crashes on WebGL2 therefore reaches a working renderer on its
 * second load instead of crashing forever.
 */

import { both, pick, presentOf } from "./branchless.ts";
import type { ReadbackTrust } from "./probe-pattern.ts";

/** One rung of the capability ladder. */
export type Tier = "webgpu" | "webgl2" | "webgl1" | "canvas2d" | "css3d";

/** The ladder, best first. Index in this array IS the tier's rank. */
export const TIER_ORDER: readonly Tier[] = ["webgpu", "webgl2", "webgl1", "canvas2d", "css3d"];

/** The best rung — where an unconstrained ladder starts. */
export const TOP_TIER: Tier = "webgpu";

/** The terminal rung: context-free DOM rendering, chosen when nothing else
 * survives its probe. */
export const FALLBACK_TIER: Tier = "css3d";

/** How a tier's probe ended.
 *   - `pass`     — the tier proved itself, pixels and all.
 *   - `degraded` — usable, but with a caveat worth reporting: a software
 *                  (major-performance-caveat) GL context, or a structural pass
 *                  granted because readback was neutralised and no pixel
 *                  evidence was admissible. Still selectable.
 *   - `fail`     — unavailable, wrong pixels, an API error, or a throw.
 *   - `skipped`  — never run: below a tier that already passed, above the
 *                  ceiling, or deliberately skipped to save its time budget. */
export type TierOutcome = "pass" | "degraded" | "fail" | "skipped";

/** What one probe learned about one tier. */
export interface TierProbe {
  /** True only when this probe proved GPU HARDWARE acceleration — a GL context
   * that survived `failIfMajorPerformanceCaveat`. Chrome disables WebGPU
   * entirely when hardware acceleration is off, so this is what lets the
   * orchestrator skip the whole async WebGPU budget instead of spending ~2.5s
   * discovering the same thing the synchronous probes already proved. */
  readonly accelerated: boolean;
  /** Human-readable reason, for the report and for telemetry. */
  readonly detail: string;
  readonly outcome: TierOutcome;
}

/** The probe result per tier. Every tier always has an entry — `skipped` is a
 * result, not an absence. */
export type TierProbes = Readonly<Record<Tier, TierProbe>>;

export type TierOutcomes = Readonly<Record<Tier, TierOutcome>>;

/** Where the running tier came from. */
export type TierSource = "url" | "session" | "probe";

/** The whole detection, as a value an app or a harness can assert on. */
export interface DetectionReport {
  /** The crash-guard ceiling the ladder was allowed to start at. */
  readonly ceiling: Tier;
  readonly elapsedMs: number;
  readonly probes: TierProbes;
  /** The control probe's verdict, which gated every other probe's evidence. */
  readonly readback: ReadbackTrust;
  readonly source: TierSource;
  readonly tier: Tier;
}

/** Position on the ladder: 0 is the best tier. An unknown value ranks below
 * every real tier, so it can never be selected. */
export const rank = (tier: Tier): number => TIER_ORDER.indexOf(tier);

/** Outcomes that still count as "this tier can render". */
const SELECTABLE: ReadonlySet<TierOutcome> = new Set<TierOutcome>(["pass", "degraded"]);

export const isSelectable = (outcome: TierOutcome): boolean => SELECTABLE.has(outcome);

export const isTier = (value: string): value is Tier => TIER_ORDER.some((tier) => tier === value);

/** The rungs at or below `ceiling`, best first — the ladder as the crash guard
 * left it. */
export const ladderFrom = (ceiling: Tier): readonly Tier[] => TIER_ORDER.slice(rank(ceiling));

/** One rung lower; the terminal tier demotes to itself. */
export const demote = (tier: Tier): Tier => pick(TIER_ORDER, Math.min(rank(tier) + 1, TIER_ORDER.length - 1));

/** The ceiling a boot starts at. A crash sentinel left behind by the previous
 * attempt caps the ladder one rung BELOW the tier that died; no sentinel means
 * the ladder is unconstrained. */
export const ceilingAfterCrash = (crashed: Tier | undefined): Tier => {
  const found = presentOf(crashed).map((tier) => demote(tier));
  return pick([TOP_TIER, ...found], found.length);
};

/** Project the probe record down to bare outcomes. Written out rather than
 * derived through `Object.fromEntries`, which would need a type assertion to
 * claim the exhaustiveness the compiler can see for free here. */
export const outcomesOf = (probes: TierProbes): TierOutcomes => ({
  canvas2d: probes.canvas2d.outcome,
  css3d: probes.css3d.outcome,
  webgl1: probes.webgl1.outcome,
  webgl2: probes.webgl2.outcome,
  webgpu: probes.webgpu.outcome,
});

/**
 * The decision: the best tier at or below `ceiling` whose probe says it can
 * render, or the terminal `css3d` when none can.
 *
 * Note what is NOT here: any notion of "the context was non-null". A tier is
 * selected because its probe produced evidence, which is the whole point of the
 * ladder — `getContext("webgl2") !== null` is true on machines that then render
 * nothing at all.
 */
export const chooseTier = (outcomes: TierOutcomes, ceiling: Tier): Tier => {
  const eligible = ladderFrom(ceiling).filter((tier) => isSelectable(outcomes[tier]));
  return pick([FALLBACK_TIER, ...eligible], Math.min(eligible.length, 1));
};

/** True when some probe proved real GPU hardware acceleration. */
export const hardwareAccelerated = (probes: TierProbes): boolean => TIER_ORDER.some((tier) => probes[tier].accelerated);

/** True when the async WebGPU budget is worth spending: only when the ceiling
 * still allows the webgpu rung AND the synchronous probes proved there is
 * hardware acceleration at all. On a machine with acceleration off (the Citrix
 * / remote-desktop case) Chrome disables WebGPU too, so probing it can only
 * ever cost the full adapter+device timeout to learn nothing. */
export const shouldProbeWebgpu = (probes: TierProbes, ceiling: Tier): boolean =>
  both(rank(ceiling) <= rank(TOP_TIER), hardwareAccelerated(probes));

/** An explicitly requested tier, or `auto` to detect. */
export type TierChoice = Tier | "auto";

/** Accepted `?render=` spellings. A Map, not a Record, so a hostile query
 * string (`?render=constructor`) cannot reach `Object.prototype`. */
const TIER_ALIASES: ReadonlyMap<string, TierChoice> = new Map<string, TierChoice>([
  ["auto", "auto"],
  ["canvas", "canvas2d"],
  ["canvas2d", "canvas2d"],
  ["css", "css3d"],
  ["css3d", "css3d"],
  ["dom", "css3d"],
  ["gpu", "webgpu"],
  ["webgl", "webgl1"],
  ["webgl1", "webgl1"],
  ["webgl2", "webgl2"],
  ["webgpu", "webgpu"],
]);

/** Parse a `?render=` value. Unknown spellings resolve to absent, never to a
 * silent "auto": an unrecognised override is a typo the report should show, not
 * a request the engine should invent an answer for. */
export const parseTierChoice = (raw: string | undefined): TierChoice | undefined =>
  TIER_ALIASES.get(String(raw).trim().toLowerCase());
