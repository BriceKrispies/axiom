/*
 * THE GAME AUTHOR'S FILE.
 *
 * This is the only file a game developer edits in this spike. Change a number
 * below, hit save, and watch the browser update in real time — the WASM engine
 * keeps running underneath, so the ring keeps spinning from exactly where it was.
 *
 * It uses the real @axiom/game authoring surface: `onFixedUpdate` for the
 * deterministic simulation step (driven by the live wasm fixed-step accumulator
 * and seeded RNG), plus an exported `draw` for presentation (the spike's
 * stand-in for the not-yet-wired 2D surface).
 */

import { type Sim, onFixedUpdate } from "@axiom/game";
import type { Frame } from "@axiom/game";

// ───────────────────────── tweak these and save ─────────────────────────
const ORB_COUNT = 20; // how many orbs ride the ring
const SPIN_PER_TICK = 0.02; // ring rotation speed (radians per fixed tick)
const RING_RADIUS_FRAC = 0.36; // ring radius as a fraction of the smaller screen edge
const ORB_RADIUS_FRAC = 0.085; // orb size as a fraction of the smaller screen edge
const HUE_BASE = 20; // base colour (0–360)
const HUE_SPREAD = 60; // how far the palette fans across the ring
const BACKGROUND = "#07090e"; // canvas clear colour
// ─────────────────────────────────────────────────────────────────────────

// Deterministic simulation state, advanced every fixed tick from the live engine.
let phase = 0;
let shimmer = 0;

onFixedUpdate((sim: Sim): void => {
  // `sim.tick` and `sim.rng` come from the real wasm engine. Deriving `phase`
  // from the tick is what makes a hot reload seamless: the new module recomputes
  // it from the engine's still-advancing tick on the very next frame.
  phase = sim.tick * SPIN_PER_TICK;
  shimmer = sim.rng.next(); // a real deterministic draw, proving the engine seam is live
});

const TWO_PI = Math.PI * 2;

export const draw = (ctx: CanvasRenderingContext2D, width: number, height: number, _frame: Frame): void => {
  ctx.fillStyle = BACKGROUND;
  ctx.fillRect(0, 0, width, height);

  const cx = width / 2;
  const cy = height / 2;
  const edge = Math.min(width, height);
  const ringR = edge * RING_RADIUS_FRAC;
  const orbR = edge * ORB_RADIUS_FRAC * (0.85 + shimmer * 0.3);

  for (let i = 0; i < ORB_COUNT; i++) {
    const t = i / ORB_COUNT;
    const angle = phase + t * TWO_PI;
    const x = cx + Math.cos(angle) * ringR;
    const y = cy + Math.sin(angle) * ringR;
    const hue = HUE_BASE + t * HUE_SPREAD;

    const glow = ctx.createRadialGradient(x, y, 0, x, y, orbR);
    glow.addColorStop(0, `hsl(${hue}, 90%, 70%)`);
    glow.addColorStop(1, `hsla(${hue}, 90%, 50%, 0)`);
    ctx.fillStyle = glow;
    ctx.beginPath();
    ctx.arc(x, y, orbR, 0, TWO_PI);
    ctx.fill();
  }

  // a calm pulsing core, so the centre of the scene reads as "alive"
  const coreR = edge * 0.03 * (1 + Math.sin(phase * 2) * 0.25);
  const core = ctx.createRadialGradient(cx, cy, 0, cx, cy, coreR);
  core.addColorStop(0, "#ffffff");
  core.addColorStop(1, `hsla(${HUE_BASE}, 90%, 60%, 0)`);
  ctx.fillStyle = core;
  ctx.beginPath();
  ctx.arc(cx, cy, coreR, 0, TWO_PI);
  ctx.fill();
};
