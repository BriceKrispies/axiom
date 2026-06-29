/*
 * THE GAME AUTHOR'S FILE — the only file a game developer edits in this harness.
 *
 * Change a number below, hit save, and the browser re-runs deterministically with
 * your edit applied from tick 0. This game is authored against the REAL
 * @axiom/game surfaces: `onFixedUpdate` for the deterministic sim step (driven by
 * the live wasm fixed-step accumulator + seeded RNG), and `onRender` drawing
 * through the real `frame.circle/line/rect` 2D surface (which records into the
 * native draw2d builder; the harness rasterizes the finished command list).
 *
 * Coordinates are in surface units (the canvas is 960×540); colours are
 * `[r, g, b, a]` with each channel in 0..1.
 */

import { type Frame, type Sim, onFixedUpdate, onRender } from "@axiom/game";
import type { Rgba } from "@axiom/game";

// ───────────────────────── tweak these and save ─────────────────────────
const ORB_COUNT = 7; // how many orbs ride the ring
const SPIN_PER_TICK = 0.02; // ring rotation speed (radians per fixed tick)
const RING_RADIUS = 200; // ring radius in surface units
const ORB_RADIUS = 46; // orb radius in surface units
const HUE_BASE = 90; // base hue (0–360)
const HUE_SPREAD = 220; // how far the palette fans across the ring
// ─────────────────────────────────────────────────────────────────────────

const WIDTH = 960;
const HEIGHT = 540;
const CENTER_X = WIDTH / 2;
const CENTER_Y = HEIGHT / 2;
const TAU = Math.PI * 2;

// Deterministic sim state, advanced each fixed tick from the live engine.
let phase = 0;
let shimmer = 0;

onFixedUpdate((sim: Sim): void => {
  phase = sim.tick * SPIN_PER_TICK;
  shimmer = sim.rng.next(); // a real deterministic draw, proving the engine seam is live
});

/** A simple HSV→[r,g,b,a] (each 0..1), enough for a hue ring. */
const hue = (degrees: number): Rgba => {
  const h = (((degrees % 360) + 360) % 360) / 60;
  const x = 1 - Math.abs((h % 2) - 1);
  const seg = Math.floor(h) % 6;
  const rgb: readonly [number, number, number] =
    seg === 0
      ? [1, x, 0]
      : seg === 1
        ? [x, 1, 0]
        : seg === 2
          ? [0, 1, x]
          : seg === 3
            ? [0, x, 1]
            : seg === 4
              ? [x, 0, 1]
              : [1, 0, x];
  return [rgb[0], rgb[1], rgb[2], 1];
};

onRender((frame: Frame): void => {
  for (let i = 0; i < ORB_COUNT; i++) {
    const t = i / ORB_COUNT;
    const angle = phase + t * TAU;
    const x = CENTER_X + Math.cos(angle) * RING_RADIUS;
    const y = CENTER_Y + Math.sin(angle) * RING_RADIUS;
    frame.circle({ x, y }, ORB_RADIUS * (0.85 + shimmer * 0.3), { fill: hue(HUE_BASE + t * HUE_SPREAD) });
  }
  // a calm pulsing core
  frame.circle({ x: CENTER_X, y: CENTER_Y }, 12 + Math.sin(phase * 2) * 4, { fill: [1, 1, 1, 1] });
});
