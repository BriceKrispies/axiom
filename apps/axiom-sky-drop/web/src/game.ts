/*
 * game.ts — THE game, wired to the engine. Registering an `onFixedUpdate` as an import
 * side effect, it builds the scene on the first tick, reads pointer + keyboard into a
 * plain `Intent`, advances the deterministic SDK-free `SkyDropSession`, and mirrors the
 * result into the 3D scene. It exports `readHud()` for the harness's DOM overlay and
 * `configureViewport()`, which the grab test needs to project the ball correctly.
 *
 * ## The HUD withholds the score
 *
 * While a round is in play the HUD carries only what a thrower needs to throw: how many
 * balls are left, and what the wind is doing. `score` and `results` are populated ONLY
 * in the `"results"` phase — the type makes that explicit by leaving them `null` until
 * then, so the harness cannot render a running total by accident. See `round.ts` for
 * why the whole rack is scored in silence.
 *
 * Controls: grab a ball and throw it · R (or tap on the scoreboard) for a fresh rack.
 */

import { type Sim, bindAction, onFixedUpdate } from "@axiom/game";
import { type SceneHandles, applyFrame, buildScene } from "./scene.ts";
import { type Intent, SkyDropSession } from "./session.ts";
import type { RoundPhase } from "./round.ts";
import { type Vec2, vec2 } from "./vec.ts";
import { DEFAULT_VIEWPORT } from "./constants.ts";

/** One landing, as the end-of-round scoreboard shows it. */
export interface HudThrow {
  readonly index: number;
  readonly label: string;
  readonly distance: number;
  readonly points: number;
  readonly onTarget: boolean;
}

/** The end-of-round scoreboard. `null` until every ball is down. */
export interface HudScoreboard {
  readonly total: number;
  readonly best: number;
  readonly isRecord: boolean;
  readonly bullseyes: number;
  readonly onTarget: number;
  /** The tightest landing of the round, in metres. */
  readonly tightest: number | null;
  readonly throws: readonly HudThrow[];
}

/** The HUD snapshot the harness renders each frame. */
export interface Hud {
  readonly phase: RoundPhase;
  /** Which ball of the rack is in hand (1-based). */
  readonly ball: number;
  readonly ballsTotal: number;
  readonly ballsLeft: number;
  /** How many balls are still falling. */
  readonly inFlight: number;
  /** Wind drift speed (m/s) for this round. */
  readonly windSpeed: number;
  /** Wind bearing in DEGREES clockwise from screen-up, for the HUD arrow. */
  readonly windAngle: number;
  /** How far the stand is from the target (m). */
  readonly standDistance: number;
  /** True while a ball is in hand. */
  readonly holding: boolean;
  /** Populated ONLY once the round is over. */
  readonly scoreboard: HudScoreboard | null;
}

let handles: SceneHandles | undefined;
let session = new SkyDropSession();
let prevDown = false;
let viewport: Vec2 = vec2(DEFAULT_VIEWPORT.x, DEFAULT_VIEWPORT.y);

const bindKeys = (): void => {
  bindAction("reset", ["KeyR"]);
};

/** Fold this tick's pointer + keyboard into the session `Intent`. */
const readIntent = (sim: Sim): Intent => {
  const sample = sim.input.pointer();
  const down = sample !== undefined ? sample.down : false;
  const pressed = down && !prevDown;
  const released = !down && prevDown;
  prevDown = down;
  const pointer = sample !== undefined ? vec2(sample.pos.x, sample.pos.y) : null;
  return { pointer, pressed, released, reset: sim.input.pressed("reset"), viewport };
};

onFixedUpdate((sim: Sim): void => {
  if (handles === undefined) {
    bindKeys();
    handles = buildScene();
    session = new SkyDropSession();
  }
  session.advance(readIntent(sim));
  applyFrame(handles, session);
});

/**
 * The wind bearing as a screen angle. The camera looks down the stand→target line, so
 * world "toward the target" is screen-up; the arrow is rotated to match what the player
 * is looking at rather than to a fixed world axis.
 */
const windAngleDegrees = (): number => {
  const basis = session.basis;
  const wind = session.windVector;
  const alongForward = wind.x * basis.forward.x + wind.z * basis.forward.z;
  const alongRight = wind.x * basis.right.x + wind.z * basis.right.z;
  return (Math.atan2(alongRight, alongForward) * 180) / Math.PI;
};

/** The scoreboard, or `null` while the round is still being thrown. */
const readScoreboard = (): HudScoreboard | null => {
  if (session.phase !== "results") {
    return null;
  }
  const total = session.score;
  return {
    best: session.best,
    bullseyes: session.bullseyes,
    isRecord: total > 0 && total >= session.best,
    onTarget: session.onTarget,
    throws: session.results.map((result): HudThrow => ({
      distance: result.distance,
      index: result.index,
      label: result.label,
      onTarget: result.ring !== "off",
      points: result.points,
    })),
    tightest: session.tightest,
    total,
  };
};

/** The HUD the harness reads each frame. */
export const readHud = (): Hud => ({
  ball: session.ballNumber,
  ballsLeft: session.ballsLeft,
  ballsTotal: session.ballsTotal,
  holding: session.holding,
  inFlight: session.inFlight,
  phase: session.phase,
  scoreboard: readScoreboard(),
  standDistance: session.standDistance,
  windAngle: windAngleDegrees(),
  windSpeed: session.windSpeed,
});

/** Report the real canvas backing size (px) — the grab test projects against it. */
export const configureViewport = (width: number, height: number): void => {
  viewport = vec2(width, height);
};
