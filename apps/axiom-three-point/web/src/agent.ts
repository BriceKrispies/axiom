/*
 * agent.ts — a headless autonomous driver that plays a complete 15-shot
 * Three-Point Shootout run through the real SDK-free session (the same
 * observe → decide → emit player-equivalent intents shape as the sibling
 * minimal-3v3 driver). It performs the three skill actions a human does on every
 * ball: re-acquire the hoop horizontally with synthetic pointer-lock look deltas
 * (the follow-through drifts the aim toward the next rack slot), ride the rising
 * shot motion, and release Space at a planned point in the rise — so the run is
 * player-equivalent by construction: the real physics decides every make and miss.
 *
 * The shot plan deliberately mixes ideal-window releases with early / late / wide
 * misses, proving makes, rim-outs, backboard misses, streak scoring, rack
 * transitions, and the results screen in one deterministic run.
 *
 * Run headless (no wasm, no DOM):  node apps/axiom-three-point/web/src/agent.ts
 */

import { SHOT_TUNING, STATIONS, TOTAL_SHOTS } from "./constants.ts";
import { RISE_START_TICKS } from "./gameplay.ts";
import { type Intent, IDLE_INTENT } from "./types.ts";
import type { Results, ShotOutcome } from "./types.ts";
import { ThreePointSession } from "./session.ts";

/** One planned shot: aim offset from the hoop-facing base, and where in the rise
 * (motion progress 0..1) to let go. */
export interface ShotPlan {
  readonly yawOffset: number;
  readonly releaseAt: number;
  readonly note: string;
}

const IDEAL = (SHOT_TUNING.idealWindowStart + SHOT_TUNING.idealWindowEnd) / 2;

/** 15 shots: mostly ideal-window releases, with deliberate misses mixed in. */
export const SHOT_PLANS: readonly ShotPlan[] = [
  // Rack 1 — left wing.
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "deliberate early (short)", releaseAt: 0.2, yawOffset: 0 },
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "golden, ideal window", releaseAt: IDEAL, yawOffset: 0 },
  // Rack 2 — top of the arc.
  { note: "deliberate late (glass)", releaseAt: 1, yawOffset: 0 },
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "wide left", releaseAt: IDEAL, yawOffset: -0.12 },
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "golden, ideal window", releaseAt: IDEAL, yawOffset: 0 },
  // Rack 3 — right wing.
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "slightly early (rim)", releaseAt: 0.53, yawOffset: 0 },
  { note: "ideal window", releaseAt: IDEAL, yawOffset: 0 },
  { note: "golden, ideal window", releaseAt: IDEAL, yawOffset: 0 },
];

const intent = (over: Partial<Intent>): Intent => ({ ...IDLE_INTENT, ...over });

export interface ShotReport {
  readonly shot: number;
  readonly rack: number;
  readonly golden: boolean;
  readonly note: string;
  readonly outcome: ShotOutcome;
  readonly releaseProgress: number;
  readonly scoreAfter: number;
  readonly streakAfter: number;
}

export interface AgentRun {
  readonly reports: readonly ShotReport[];
  readonly results: Results;
  readonly ticks: number;
  readonly hash: number;
}

/** Play the full 15-shot run; the physics decides every outcome. */
export const runAgent = (): AgentRun => {
  const session = new ThreePointSession();
  const reports: ShotReport[] = [];
  let ticks = 0;
  const step = (i: Intent): void => {
    session.advance(i);
    ticks += 1;
  };
  // Skill action 1: bring the drifted aim back onto the hoop with look deltas.
  const aim = (targetYaw: number): void => {
    for (let i = 0; i < 120; i += 1) {
      const dYaw = targetYaw - session.yaw;
      if (Math.abs(dYaw) < 1e-6) return;
      step(intent({ lookDx: dYaw / SHOT_TUNING.aimYawSensitivity }));
    }
  };

  for (let shot = 0; shot < TOTAL_SHOTS; shot += 1) {
    const plan = SHOT_PLANS[shot]!;
    // The next ball is dealt the moment the previous one leaves the hand; wait
    // only for it to reach the chest (and for rack glides).
    for (let guard = 0; guard < 3000 && !(session.phase === "ready" && session.ballInHand); guard += 1) step(IDLE_INTENT);
    const rack = session.stationIndex;
    const golden = session.ballIndex === 4;
    aim(STATIONS[rack]!.baseYaw + plan.yawOffset);
    // Skill actions 2 + 3: ride the rise, release at the planned progress.
    const holdTicks = RISE_START_TICKS + Math.round(plan.releaseAt * SHOT_TUNING.shotRiseTicks);
    step(intent({ shootHeld: true, shootPressed: true }));
    for (let i = 1; i < holdTicks - 1; i += 1) step(intent({ shootHeld: true }));
    step(intent({ shootReleased: true }));
    // This driver plays sequentially: let the shot resolve so the report can
    // attribute the outcome (a human can already be shooting the next ball).
    for (let guard = 0; guard < 3000 && session.ballsInFlight > 0; guard += 1) step(IDLE_INTENT);
    reports.push({
      golden,
      note: plan.note,
      outcome: session.lastOutcome ?? "miss",
      rack,
      releaseProgress: session.lastReleaseProgress,
      scoreAfter: session.score,
      shot,
      streakAfter: session.streak,
    });
  }
  for (let guard = 0; guard < 3000 && session.phase !== "results"; guard += 1) step(IDLE_INTENT);
  if (session.results === undefined) throw new Error("run did not reach results");
  return { hash: session.hash(), reports, results: session.results, ticks };
};

// ── CLI (headless play-by-play; a no-op when imported by tests) ───────────────

declare const process: { argv?: readonly string[] } | undefined;

const isMain = typeof process !== "undefined" && (process?.argv?.[1] ?? "").replace(/\\/g, "/").endsWith("/agent.ts");

if (isMain) {
  const run = runAgent();
  console.log("Three-Point Shootout — headless driver (player-equivalent intents, real physics)\n");
  for (const r of run.reports) {
    const tag = r.golden ? " [GOLDEN]" : "";
    console.log(
      `shot ${String(r.shot + 1).padStart(2)}  rack ${r.rack + 1}${tag}  ${r.note.padEnd(26)} p=${r.releaseProgress.toFixed(2)} → ` +
        `${r.outcome.toUpperCase().padEnd(9)} score ${String(r.scoreAfter).padStart(3)}  streak ${r.streakAfter}`,
    );
  }
  const res = run.results;
  console.log(
    `\nRESULTS: ${res.score} points, ${res.makes}/15 makes, best streak ${res.bestStreak} — "${res.label}"` +
      `\n(${run.ticks} ticks, replay hash ${run.hash})`,
  );
}
