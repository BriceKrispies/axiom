/*
 * game.ts — the Scratch Reveal controller. SINGLE-REVEAL mechanic with the
 * afterCommit "interact" hand-off: the moment the player commits (primary press
 * or first pointer-down on the foil) the win/tier is SEALED — before a single
 * tile is scratched. The player then scratches the foil (a tile-grid mask over
 * the symbol); each pointer-down removes the tiles near the cursor. Once ≥ 55%
 * of the SYMBOL-AREA tiles are gone the remaining foil dissolves and the hidden
 * symbol (a tier-colored gem on a win, a friendly cloud on a loss) is revealed.
 *
 * Tile bookkeeping is bounded: only the tiles in the grid neighborhood of the
 * scratch point are examined each tick (grid math, never a full scan), and the
 * scratched set is copied-on-write only on the ticks it actually changes.
 */

import type { InputFrame, PointerSample, TickContext, ToneSpec } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../../presentation/cameras/picking.ts";
import { speedTicks } from "../round-state.ts";
import type { CasinoState } from "../round-state.ts";

// ── the game-specific spec ──────────────────────────────────────────────────────

export interface ScratchSpec {
  /** Foil mask columns (8–30). */
  readonly columns: number;
  /** Foil mask rows (6–16). */
  readonly rows: number;
}

export const DEFAULT_SCRATCH_SPEC: ScratchSpec = { columns: 18, rows: 10 };

/** Fraction of symbol-area tiles that must be scratched to trigger the reveal. */
export const REVEAL_THRESHOLD = 0.55;

/** The scratch brush radius, in logical canvas pixels. */
export const SCRATCH_RADIUS_PX = 46;

// ── the foil layout (logical 960×600 canvas space) ───────────────────────────────

export interface FoilLayout {
  readonly x0: number;
  readonly y0: number;
  readonly width: number;
  readonly height: number;
  readonly columns: number;
  readonly rows: number;
}

const FOIL_WIDTH = 520;
const FOIL_HEIGHT = 300;

export const foilLayout = (spec: ScratchSpec): FoilLayout => ({
  columns: spec.columns,
  height: FOIL_HEIGHT,
  rows: spec.rows,
  width: FOIL_WIDTH,
  x0: (CANVAS_WIDTH - FOIL_WIDTH) / 2,
  y0: (CANVAS_HEIGHT - FOIL_HEIGHT) / 2,
});

export const tileCount = (layout: FoilLayout): number => layout.columns * layout.rows;

/** Logical-space center of the tile at (col, row). */
export const tileCenter = (layout: FoilLayout, col: number, row: number): { readonly x: number; readonly y: number } => ({
  x: layout.x0 + ((col + 0.5) / layout.columns) * layout.width,
  y: layout.y0 + ((row + 0.5) / layout.rows) * layout.height,
});

/** True when tile (col, row)'s center lies inside the central symbol ellipse. */
export const inSymbolArea = (layout: FoilLayout, col: number, row: number): boolean => {
  const c = tileCenter(layout, col, row);
  const cx = layout.x0 + layout.width / 2;
  const cy = layout.y0 + layout.height / 2;
  const rx = layout.width * 0.42;
  const ry = layout.height * 0.42;
  const nx = (c.x - cx) / rx;
  const ny = (c.y - cy) / ry;
  return nx * nx + ny * ny <= 1;
};

/** The indices of every tile overlapping the symbol area (computed once). */
export const symbolAreaTiles = (layout: FoilLayout): readonly number[] => {
  const out: number[] = [];
  for (let row = 0; row < layout.rows; row += 1) {
    for (let col = 0; col < layout.columns; col += 1) {
      if (inSymbolArea(layout, col, row)) {
        out.push(row * layout.columns + col);
      }
    }
  }
  return out;
};

/** The scratched fraction OVER THE SYMBOL AREA (symbol-area tiles only). */
export const revealedFraction = (scratched: ReadonlySet<number>, symbolTiles: readonly number[]): number => {
  if (symbolTiles.length === 0) {
    return 0;
  }
  const hit = symbolTiles.reduce((n, index) => n + (scratched.has(index) ? 1 : 0), 0);
  return hit / symbolTiles.length;
};

/**
 * Add every tile whose center is within `SCRATCH_RADIUS_PX` of `(px, py)` to
 * `scratched`, examining ONLY the grid neighborhood of the point (never all
 * tiles). Returns the same set object when nothing changed (copy-on-write). */
export const scratchAt = (
  layout: FoilLayout,
  scratched: ReadonlySet<number>,
  px: number,
  py: number,
): ReadonlySet<number> => {
  const tileW = layout.width / layout.columns;
  const tileH = layout.height / layout.rows;
  const colSpan = Math.ceil(SCRATCH_RADIUS_PX / tileW) + 1;
  const rowSpan = Math.ceil(SCRATCH_RADIUS_PX / tileH) + 1;
  const centerCol = Math.floor(((px - layout.x0) / layout.width) * layout.columns);
  const centerRow = Math.floor(((py - layout.y0) / layout.height) * layout.rows);
  let next: Set<number> | null = null;
  const r2 = SCRATCH_RADIUS_PX * SCRATCH_RADIUS_PX;
  for (let row = Math.max(0, centerRow - rowSpan); row <= Math.min(layout.rows - 1, centerRow + rowSpan); row += 1) {
    for (let col = Math.max(0, centerCol - colSpan); col <= Math.min(layout.columns - 1, centerCol + colSpan); col += 1) {
      const c = tileCenter(layout, col, row);
      const dx = c.x - px;
      const dy = c.y - py;
      const index = row * layout.columns + col;
      if (dx * dx + dy * dy <= r2 && !scratched.has(index)) {
        next ??= new Set(scratched);
        next.add(index);
      }
    }
  }
  return next ?? scratched;
};

/** The automatic keyboard-sweep point at sweep tick `t`: a serpentine path that
 * covers the foil so keyboard-only players can clear the symbol. */
export const sweepPoint = (layout: FoilLayout, t: number): { readonly x: number; readonly y: number } => {
  const across = (Math.sin(t * 0.09) * 0.5 + 0.5) * layout.width;
  const down = ((t * 0.9) % layout.height);
  return { x: layout.x0 + across, y: layout.y0 + down };
};

// ── the dissolve timeline (ticks from entering "revealing") ─────────────────────

export interface DissolveTimeline {
  readonly dissolveEnd: number;
  readonly total: number;
}

export const dissolveTimeline = (presentationSpeed: number, reducedMotion: boolean): DissolveTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const dissolveEnd = speedTicks(Math.round(24 * scale), presentationSpeed);
  return { dissolveEnd, total: dissolveEnd + speedTicks(Math.round(40 * scale), presentationSpeed) };
};

// ── the controller ──────────────────────────────────────────────────────────────

export interface ScratchExtra {
  readonly scratched: ReadonlySet<number>;
  /** Ticks the keyboard primary has been held (drives the auto-sweep). */
  readonly sweepTicks: number;
  /** The most recent scratch point in logical canvas space, for debris. */
  readonly lastScratch: { readonly x: number; readonly y: number } | null;
}

export type ScratchState = CasinoState<ScratchExtra>;

export const initialScratchExtra = (_session: SessionState): ScratchExtra => ({
  lastScratch: null,
  scratched: new Set(),
  sweepTicks: 0,
});

/** Whether the pointer is currently over the foil rectangle. */
const pointerOnFoil = (layout: FoilLayout, pointer: PointerSample | undefined): boolean =>
  pointer !== undefined &&
  pointer.pos.x >= layout.x0 &&
  pointer.pos.x <= layout.x0 + layout.width &&
  pointer.pos.y >= layout.y0 &&
  pointer.pos.y <= layout.y0 + layout.height;

export const stepScratch = (
  runtime: GameRuntime<ScratchSpec>,
  state: ScratchState,
  input: InputFrame,
  _ctx: TickContext,
): ScratchState => {
  const session = state.session;
  const layout = foilLayout(runtime.config.gameSpecific);

  if (session.phase === "ready") {
    const startByKey = input.pressed.has("primary");
    const startByFoil = (input.pointer?.down ?? false) && pointerOnFoil(layout, input.pointer);
    if (startByKey || startByFoil) {
      return { ...state, pendingContext: {}, session: transition(session, "committing") };
    }
    return state;
  }

  if (session.phase === "interacting") {
    const symbolTiles = symbolAreaTiles(layout);
    let scratched = state.extra.scratched;
    let lastScratch = state.extra.lastScratch;
    const pointerScratch = (input.pointer?.down ?? false) && pointerOnFoil(layout, input.pointer);
    if (pointerScratch && input.pointer !== undefined) {
      scratched = scratchAt(layout, scratched, input.pointer.pos.x, input.pointer.pos.y);
      lastScratch = { x: input.pointer.pos.x, y: input.pointer.pos.y };
    }
    const keyHeld = input.down.has("primary");
    const sweepTicks = keyHeld ? state.extra.sweepTicks + 1 : state.extra.sweepTicks;
    if (keyHeld) {
      const point = sweepPoint(layout, sweepTicks);
      scratched = scratchAt(layout, scratched, point.x, point.y);
      lastScratch = point;
    }
    const extra: ScratchExtra = { lastScratch, scratched, sweepTicks };
    const fraction = revealedFraction(scratched, symbolTiles);
    if (fraction >= REVEAL_THRESHOLD) {
      return { ...state, extra, session: transition(session, "revealing") };
    }
    return { ...state, extra };
  }

  if (session.phase === "revealing" && session.committed !== null) {
    const timeline = dissolveTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Scratch grit cues: a soft rasp on the ticks the scratched set grows. */
export const scratchCues = (
  prev: ScratchState,
  next: ScratchState,
  rasp: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  if (next.session.phase !== "interacting") {
    return [];
  }
  const grew = next.extra.scratched.size > prev.extra.scratched.size;
  return grew ? rasp(next.session.seed, next.extra.scratched.size) : [];
};
