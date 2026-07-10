/*
 * agent.ts — an autonomous agent that plays Minimal 3v3 Basketball and tries to
 * score, mirroring the engine's `axiom-agent` module (modules/axiom-agent) at app
 * tier the same way the retro FPS native driver does (apps/axiom-gallery/src/
 * retro_fps/agent.rs). The Rust module is same-binary-only (no wasm/TS binding),
 * so this is its TypeScript twin, speaking the same vocabulary:
 *
 *   observe → decide → emit player-equivalent intents → record a decision report
 *
 * - The game state is translated into a neutral `Observation` of `(kind, subject,
 *   x, y, z, value)` facts in fixed-point micro-units — the agent never touches
 *   the session directly.
 * - The brain returns a CONTROL-CODE BITMASK (the retro FPS convention: one bit
 *   per held control, OR-combined per tick — `ActionQueue::combined_control_code`).
 * - The driver lowers the mask into the exact same `Intent` a keyboard produces
 *   (`intentOfControlCode`, with press/release edges from the previous mask), so
 *   the agent is player-equivalent by construction.
 * - Every tick yields a `DecisionReport { tick, controlCode, reasonCode }`.
 *
 * The policy (`ApexScorerBrain`): walk the handler into shooting range, sidestep
 * a defender squatting in the path, WAIT OUT an airborne contest jump, then
 * gather and release exactly at the jump apex. Possessions vary range (and swing
 * a pass to the right wing every third possession) so the seeded shot rolls vary
 * — the agent keeps playing until it scores.
 *
 * Run headless (no wasm, no DOM):  node apps/axiom-minimal-3v3/web/src/agent.ts
 */

import { Mini3v3Session } from "./session.ts";
import type { Intent, ResultKind, TimingTag } from "./types.ts";
import * as C from "./constants.ts";

// ── control-code bitmask (app convention, as in retro_fps/agent.rs) ───────────

export const CONTROL_MOVE_POS_X = 1 << 0; // world +x (screen-left)
export const CONTROL_MOVE_NEG_X = 1 << 1; // world -x (screen-right)
export const CONTROL_MOVE_POS_Z = 1 << 2; // toward the hoop
export const CONTROL_MOVE_NEG_Z = 1 << 3; // away from the hoop
export const CONTROL_GATHER = 1 << 4; // held Space (press = gather, release = shoot)
export const CONTROL_PASS_LEFT = 1 << 5;
export const CONTROL_PASS_RIGHT = 1 << 6;

/**
 * Lower a held-control bitmask into the session `Intent`, deriving the
 * press/release edges from the previous tick's mask — exactly what the SDK's
 * `sim.input.pressed/released` derive from real keyboard state.
 */
export const intentOfControlCode = (code: number, prevCode: number): Intent => ({
  gatherHeld: (code & CONTROL_GATHER) !== 0,
  gatherPressed: (code & CONTROL_GATHER) !== 0 && (prevCode & CONTROL_GATHER) === 0,
  gatherReleased: (code & CONTROL_GATHER) === 0 && (prevCode & CONTROL_GATHER) !== 0,
  moveX: ((code & CONTROL_MOVE_POS_X) !== 0 ? 1 : 0) - ((code & CONTROL_MOVE_NEG_X) !== 0 ? 1 : 0),
  moveZ: ((code & CONTROL_MOVE_POS_Z) !== 0 ? 1 : 0) - ((code & CONTROL_MOVE_NEG_Z) !== 0 ? 1 : 0),
  passLeft: (code & CONTROL_PASS_LEFT) !== 0 && (prevCode & CONTROL_PASS_LEFT) === 0,
  passRight: (code & CONTROL_PASS_RIGHT) !== 0 && (prevCode & CONTROL_PASS_RIGHT) === 0,
  reset: false,
});

// ── observation (the axiom-agent fact shape: kind/subject/x/y/z/value, micro-units) ──

export interface ObservationFact {
  readonly kind: number;
  readonly subject: number;
  readonly x: number;
  readonly y: number;
  readonly z: number;
  readonly value: number;
}

export interface Observation {
  readonly tick: number;
  readonly facts: readonly ObservationFact[];
}

export const FACT_SELF_POSE = 1;
export const FACT_HOOP = 2;
export const FACT_DEFENDER = 3;
export const FACT_PHASE = 4;
export const FACT_HAS_BALL = 5;

export const PHASE_CODES = { playing: 0, shooting: 1, shotResult: 2, turnoverResult: 3 } as const;

const MICRO = 1e6;
const micro = (m: number): number => Math.round(m * MICRO);

/** Translate the session's public view into a neutral agent observation. */
export const observe = (session: Mini3v3Session): Observation => {
  const view = session.view();
  const self = view.blues[view.controlledIndex];
  const facts: ObservationFact[] = [
    { kind: FACT_PHASE, subject: 0, value: PHASE_CODES[view.phase], x: 0, y: 0, z: 0 },
    { kind: FACT_HAS_BALL, subject: 0, value: session.possessionLabel === "YOU HAVE THE BALL" ? 1 : 0, x: 0, y: 0, z: 0 },
    {
      kind: FACT_SELF_POSE,
      subject: view.controlledIndex,
      value: 0,
      x: micro(self.pos.x),
      y: micro(self.pos.y),
      z: micro(self.pos.z),
    },
    { kind: FACT_HOOP, subject: 0, value: 0, x: micro(C.HOOP_POS.x), y: micro(C.HOOP_POS.y), z: micro(C.HOOP_POS.z) },
  ];
  view.defenders.forEach((d, i) => {
    facts.push({ kind: FACT_DEFENDER, subject: i, value: micro(d.jumpY), x: micro(d.pos.x), y: micro(d.pos.y), z: micro(d.pos.z) });
  });
  return { facts, tick: view.tick };
};

// ── decision reporting (the axiom-agent DecisionReport analogue) ──────────────

export const REASON_SEEK_RANGE = 1;
export const REASON_SIDESTEP_DEFENDER = 2;
export const REASON_WAIT_OUT_CONTEST = 3;
export const REASON_START_GATHER = 4;
export const REASON_RISE_TO_APEX = 5;
export const REASON_RELEASE_AT_APEX = 6;
export const REASON_SWING_PASS = 7;
export const REASON_BALL_IN_FLIGHT = 8;
export const REASON_RESULT_FREEZE = 9;

export const REASON_NAMES: Record<number, string> = {
  [REASON_SEEK_RANGE]: "seek shooting range",
  [REASON_SIDESTEP_DEFENDER]: "sidestep defender in path",
  [REASON_WAIT_OUT_CONTEST]: "wait out airborne contest",
  [REASON_START_GATHER]: "start gather",
  [REASON_RISE_TO_APEX]: "rise to apex",
  [REASON_RELEASE_AT_APEX]: "release at apex",
  [REASON_SWING_PASS]: "swing pass to the wing",
  [REASON_BALL_IN_FLIGHT]: "ball in flight",
  [REASON_RESULT_FREEZE]: "result freeze",
};

export interface DecisionReport {
  readonly tick: number;
  readonly controlCode: number;
  readonly reasonCode: number;
}

// ── the brain ─────────────────────────────────────────────────────────────────

const fact = (obs: Observation, kind: number): ObservationFact | undefined => obs.facts.find((f) => f.kind === kind);
const factsOf = (obs: Observation, kind: number): ObservationFact[] => obs.facts.filter((f) => f.kind === kind);

/** Per-possession plan: how close to drive before shooting, and whether to swing a pass first. */
const RANGE_PLAN = [3.2, 2.7, 3.7, 3.0];
const passPossession = (i: number): boolean => i % 3 === 2;

/**
 * The scoring policy. Decides from the Observation alone (plus its own memory,
 * the `AgentMemory` analogue) and emits a control-code bitmask.
 */
export class ApexScorerBrain {
  /** The tick the gather press lands in the session; -1 while not gathering. */
  #gatherLandTick = -1;
  #possession = 0;
  #passedThisPossession = false;
  #prevPhase: number = PHASE_CODES.playing;

  get possession(): number {
    return this.#possession;
  }

  decide(obs: Observation): { controlCode: number; reasonCode: number } {
    const phase = fact(obs, FACT_PHASE)!.value;
    // New possession on the result → playing transition.
    if (phase === PHASE_CODES.playing && (this.#prevPhase === PHASE_CODES.shotResult || this.#prevPhase === PHASE_CODES.turnoverResult)) {
      this.#possession += 1;
      this.#gatherLandTick = -1;
      this.#passedThisPossession = false;
    }
    this.#prevPhase = phase;

    if (phase === PHASE_CODES.shotResult || phase === PHASE_CODES.turnoverResult) {
      return { controlCode: 0, reasonCode: REASON_RESULT_FREEZE };
    }
    if (phase === PHASE_CODES.shooting) {
      // Release lands on the advance where the session's gather tick hits the apex.
      const nextGatherTick = obs.tick + 1 - this.#gatherLandTick;
      if (nextGatherTick >= C.JUMP_APEX_TICK) {
        return { controlCode: 0, reasonCode: REASON_RELEASE_AT_APEX };
      }
      return { controlCode: CONTROL_GATHER, reasonCode: REASON_RISE_TO_APEX };
    }

    // phase === playing
    if (fact(obs, FACT_HAS_BALL)!.value === 0) {
      return { controlCode: 0, reasonCode: REASON_BALL_IN_FLIGHT };
    }
    if (passPossession(this.#possession) && !this.#passedThisPossession) {
      this.#passedThisPossession = true;
      return { controlCode: CONTROL_PASS_RIGHT, reasonCode: REASON_SWING_PASS };
    }

    const self = fact(obs, FACT_SELF_POSE)!;
    const hoop = fact(obs, FACT_HOOP)!;
    const dx = (hoop.x - self.x) / MICRO;
    const dz = (hoop.z - self.z) / MICRO;
    const distToHoop = Math.hypot(dx, dz);
    const range = RANGE_PLAN[this.#possession % RANGE_PLAN.length]!;

    if (distToHoop > range) {
      // Drive toward the hoop; sidestep a defender squatting directly in the path.
      let code = 0;
      code |= dz > 0.05 ? CONTROL_MOVE_POS_Z : 0;
      code |= dz < -0.05 ? CONTROL_MOVE_NEG_Z : 0;
      code |= dx > 0.05 ? CONTROL_MOVE_POS_X : 0;
      code |= dx < -0.05 ? CONTROL_MOVE_NEG_X : 0;
      const blocker = factsOf(obs, FACT_DEFENDER).find((d) => {
        const bx = (d.x - self.x) / MICRO;
        const bz = (d.z - self.z) / MICRO;
        return Math.hypot(bx, bz) < 1.0 && bz * Math.sign(dz || 1) > 0;
      });
      if (blocker !== undefined) {
        const side = blocker.x >= self.x ? CONTROL_MOVE_NEG_X : CONTROL_MOVE_POS_X;
        return { controlCode: (code & ~(CONTROL_MOVE_POS_X | CONTROL_MOVE_NEG_X)) | side, reasonCode: REASON_SIDESTEP_DEFENDER };
      }
      return { controlCode: code, reasonCode: REASON_SEEK_RANGE };
    }

    // In range. If a nearby defender is mid contest jump, wait them out (their
    // landing starts a long cooldown — the open window).
    const airborne = factsOf(obs, FACT_DEFENDER).some((d) => {
      const bx = (d.x - self.x) / MICRO;
      const bz = (d.z - self.z) / MICRO;
      return d.value > micro(0.05) && Math.hypot(bx, bz) < C.CONTEST_TRIGGER_RADIUS;
    });
    if (airborne) {
      return { controlCode: 0, reasonCode: REASON_WAIT_OUT_CONTEST };
    }

    this.#gatherLandTick = obs.tick + 1;
    return { controlCode: CONTROL_GATHER, reasonCode: REASON_START_GATHER };
  }
}

// ── the driver (the retro-FPS AgentSession analogue) ──────────────────────────

export interface PossessionOutcome {
  readonly possession: number;
  readonly shooter: number;
  readonly result: ResultKind;
  readonly timing: TimingTag | undefined;
  readonly endTick: number;
}

export interface AgentRun {
  readonly outcomes: readonly PossessionOutcome[];
  readonly makes: number;
  readonly attempts: number;
  readonly ticks: number;
  readonly reports: readonly DecisionReport[];
  readonly hash: number;
}

/**
 * Play up to `maxPossessions` (stopping early after `stopAfterMakes` buckets),
 * one observe → decide → lower → advance cycle per fixed tick.
 */
export const runAgent = (maxPossessions: number, stopAfterMakes = 1): AgentRun => {
  const session = new Mini3v3Session();
  const brain = new ApexScorerBrain();
  const outcomes: PossessionOutcome[] = [];
  const reports: DecisionReport[] = [];
  let prevCode = 0;
  let lastReason = -1;
  let sawResult = false;
  let ticks = 0;
  const TICK_CAP = 20_000;

  while (ticks < TICK_CAP) {
    const obs = observe(session);
    const { controlCode, reasonCode } = brain.decide(obs);
    if (reasonCode !== lastReason) {
      reports.push({ controlCode, reasonCode, tick: obs.tick });
      lastReason = reasonCode;
    }
    session.advance(intentOfControlCode(controlCode, prevCode));
    prevCode = controlCode;
    ticks += 1;

    const result = session.resultKind;
    if (result !== undefined && !sawResult) {
      sawResult = true;
      outcomes.push({
        endTick: ticks,
        possession: brain.possession,
        result,
        shooter: session.controlledIndex,
        timing: session.timingTag,
      });
      if (session.makes >= stopAfterMakes || outcomes.length >= maxPossessions) {
        break;
      }
    }
    if (result === undefined) {
      sawResult = false;
    }
  }

  return { attempts: session.attempts, hash: session.hash(), makes: session.makes, outcomes, reports, ticks };
};

// ── CLI (headless play-by-play; a no-op when imported by tests or the browser) ──

declare const process: { argv?: readonly string[] } | undefined;

const isMain =
  typeof process !== "undefined" && (process?.argv?.[1] ?? "").replace(/\\/g, "/").endsWith("/agent.ts");

if (isMain) {
  const run = runAgent(8);
  console.log("Minimal 3v3 Basketball — axiom-agent-style driver (observe → decide → emit intents)\n");
  for (const o of run.outcomes) {
    const spot = passPossession(o.possession) ? "wing (after swing pass)" : `range ${RANGE_PLAN[o.possession % RANGE_PLAN.length]}m`;
    console.log(
      `possession ${o.possession + 1}: shooter blue-${o.shooter} from ${spot} → ` +
        `${o.result.toUpperCase()}${o.timing !== undefined ? ` (release: ${o.timing})` : ""} [tick ${o.endTick}]`,
    );
  }
  console.log(`\ndecision log (${run.reports.length} transitions):`);
  for (const r of run.reports) {
    console.log(`  tick ${String(r.tick).padStart(4)}  code ${String(r.controlCode).padStart(3)}  ${REASON_NAMES[r.reasonCode]}`);
  }
  console.log(`\nMAKES ${run.makes} / ${run.attempts} in ${run.ticks} ticks (replay hash ${run.hash})`);
}
