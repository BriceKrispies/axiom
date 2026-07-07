/*
 * THE GAME AUTHOR'S FILE — the only file a game developer edits in this harness.
 *
 * It EXPORTS a `defineApp` manifest (hot-reload architecture §5): stable-ID-keyed
 * systems the engine reconciles into the running instance. Change a number below,
 * hit save, and the hot runtime swaps the affected system's body on the next fixed
 * tick — WITHOUT recreating the `WasmGame`, the world, or the canvas. The tick
 * counter keeps climbing across the edit; that continuity is the proof the engine
 * stayed alive.
 *
 * The systems author against the REAL @axiom/game surfaces: `orb.spin`
 * (`fixedUpdate`) advances deterministic state from the live wasm fixed-step
 * accumulator + seeded RNG; `orb.draw` (`render`) draws through the real
 * `frame.circle/sprite/text` 2D surface (recorded into the native draw2d builder,
 * which the harness's `present2d` boot presenter then rasterizes).
 *
 * Coordinates are in surface units (the canvas is 960×540); colours are
 * `[r, g, b, a]` with each channel in 0..1.
 */

import { type Frame, type Rgba, type Sim, defineApp, loadTexture, system } from "@axiom/game";

// A tiny 32×32 four-quadrant texture, embedded as a data URL so the harness can
// `fetch`+decode it with no static asset — the Tier-0 sprite-texture proof.
const BADGE =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAASklEQVR42mP4P8CAYdA44LWDCVnYZEUwWXjUAaMOGHXAqANGHTDqgMHnAKfWL2Th33tYycKjDhh1wKgDRh0w6oBRBww+B4zY3jEA0bZlqrmptnsAAAAASUVORK5CYII=";

// The texture handle, resolved lazily on the first render (after the host binds).
let badge = 0;

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

// Per-frame scratch derived from the LIVE wasm tick each fixed update. These reset
// to 0 on a re-import, but `orb.spin` recomputes them from `sim.tick` — which is
// engine-owned and survives the hot patch — so the animation continues seamlessly.
let phase = 0;
let shimmer = 0;
let tick = 0;

const orbSpin = system("orb.spin", {
  phase: "fixedUpdate",
  run: (sim: Sim): void => {
    tick = sim.tick;
    phase = sim.tick * SPIN_PER_TICK;
    shimmer = sim.rng.next(); // a real deterministic draw, proving the engine seam is live
  },
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

const orbDraw = system("orb.draw", {
  phase: "render",
  run: (frame: Frame): void => {
    for (let i = 0; i < ORB_COUNT; i++) {
      const t = i / ORB_COUNT;
      const angle = phase + t * TAU;
      const x = CENTER_X + Math.cos(angle) * RING_RADIUS;
      const y = CENTER_Y + Math.sin(angle) * RING_RADIUS;
      frame.circle({ x, y }, ORB_RADIUS * (0.85 + shimmer * 0.3), { fill: hue(HUE_BASE + t * HUE_SPREAD) });
    }
    // a calm pulsing core
    frame.circle({ x: CENTER_X, y: CENTER_Y }, 12 + Math.sin(phase * 2) * 4, { fill: [1, 1, 1, 1] });

    // Tier-0 sprite: the badge texture, spun and pulsed at the centre. `loadTexture`
    // must run after the host binds, so resolve it lazily on the first frame.
    if (badge === 0) {
      badge = loadTexture(BADGE);
    }
    frame.sprite(badge, {
      anchor: { x: 0.5, y: 0.5 },
      pos: { x: CENTER_X, y: CENTER_Y },
      rotation: -phase,
      scale: { x: 2.5 + shimmer, y: 2.5 + shimmer },
    });

    // Tier-0 text HUD: a title and a live tick read-out, top-left.
    frame.text("AXIOM ENGINE", { color: [1, 1, 1, 1], font: { family: "monospace", size: 28 }, pos: { x: 24, y: 22 } });
    frame.text(`tick ${tick}`, { color: [0.6, 0.9, 1, 1], font: { family: "monospace", size: 20 }, pos: { x: 24, y: 60 } });
  },
});

export default defineApp({
  config: { fixedHz: 60, seed: 1n, surface: "c" },
  id: "orbs",
  systems: [orbSpin, orbDraw],
});
