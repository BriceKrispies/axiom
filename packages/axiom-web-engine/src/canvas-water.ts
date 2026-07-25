/*
 * canvas-water.ts — a small reusable Canvas2D "stylized water surface" effect,
 * part of the Canvas2D module (the platform-edge sibling of backend-canvas2d.ts).
 * It draws a subtle, broken cellular highlight network inside an app-supplied
 * boundary so a flat blue region reads as water — no shader, no texture, no fluid
 * simulation, no per-pixel image processing, no randomness.
 *
 * The caller owns the BOUNDARY (it traces the pool path, and optionally the holes
 * where objects stand on the water); this module owns the RENDERING. Draw order,
 * per the spec:
 *   save → trace boundary → clip → (optional base fill) → broken cellular
 *   highlights → soft inner-perimeter cover that fades the pattern before the
 *   shoreline → restore.
 *
 * Determinism: the pattern is a hex lattice whose edges are kept/dropped by a
 * pure COORDINATE HASH (never Math.random), so the same inputs always draw the
 * same picture. Motion, if any, is a tiny deterministic drift of the whole
 * pattern derived from an explicit `timeSeconds` the caller passes in — the
 * renderer never reads a wall clock. The generated segment geometry is cached by
 * (cellSize, bounds, keep) so it is built once and only re-translated per frame.
 *
 * This is a browser-API boundary (CanvasRenderingContext2D), so like the other
 * Canvas2D module files it sits outside the branchless / 100%-coverage spine laws
 * (see test-exempt.json and the .oxlintrc platform group); its correctness is
 * proven on the live browser path.
 */

/** A short line segment of the cellular net, in canvas pixels. */
interface Segment {
  readonly x1: number;
  readonly y1: number;
  readonly x2: number;
  readonly y2: number;
}

/** A screen-space rectangle the lattice is generated across (canvas pixels). */
export interface WaterBounds {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/** Options for `drawStylizedWaterSurface`. Colors are any CSS color string. */
export interface StylizedWaterOptions {
  /** Trace the water boundary as a closed path (the caller owns the shape). Used
   * for the clip and for the shoreline fade. */
  readonly tracePool: (ctx: CanvasRenderingContext2D) => void;
  /** Optionally add holes (objects standing on the water) to the CLIP path, so
   * the pattern is not drawn over them. Traced right after `tracePool` and
   * clipped with the even-odd rule. */
  readonly traceHoles?: (ctx: CanvasRenderingContext2D) => void;
  /** Where to generate the lattice (a bounding box of the pool, in canvas px). */
  readonly bounds: WaterBounds;
  /** Optional solid base fill drawn under the pattern (omit to overlay a region
   * the scene already fills blue). */
  readonly baseColor?: string;
  /** The highlight line color. */
  readonly lineColor: string;
  /** The water color used to fade the pattern out at the shoreline (should match
   * the pool so the cover is invisible except that it hides the pattern). */
  readonly edgeColor: string;
  /** Hexagon size (center-to-vertex) in canvas px — large = sparse, big cells. */
  readonly cellSize: number;
  /** Highlight line width in px. */
  readonly lineWidth: number;
  /** Pattern opacity (0..1) — keep low for a subtle, low-contrast net. */
  readonly opacity: number;
  /** Blur radius in px for the soft, non-razor edges. Keep small. */
  readonly softnessPx: number;
  /** Width of the soft shoreline cover in px — how far in the pattern fades. */
  readonly edgeFadePx: number;
  /** Peak drift of the whole pattern in px (0 = static). */
  readonly driftAmount: number;
  /** Deterministic time in seconds (the caller passes the explicit engine clock);
   * drives the subtle drift. */
  readonly timeSeconds: number;
}

/** Fraction of hex edges kept (the rest are dropped to break up the lattice so it
 * reads as stylized markings, not board-game tiles). */
const EDGE_KEEP_PERCENT = 58;
/** Each hexagon owns three of its six edges (0, 2, 4), so a shared edge is
 * generated once — no double-weight seams. */
const OWNED_EDGE_STARTS = [0, 2, 4];
const HEX_SIDES = 6;
const SQRT3 = Math.sqrt(3);
const COLUMN_SPACING = 1.5;
const DRIFT_X_RATE = 0.18;
const DRIFT_Y_RATE = 0.14;
const DRIFT_Y_SCALE = 0.75;

/** A tiny deterministic hash of an integer pixel coordinate pair → 0..99. Not
 * cryptographic — just a stable, seed-free way to keep "some" edges. */
const coordHash = (ix: number, iy: number): number => {
  let h = (ix * 73856093) ^ (iy * 19349663);
  h = (h ^ (h >>> 13)) >>> 0;
  return h % 100;
};

/** Build the (undrifted) kept hex-edge segments across `bounds`. Deterministic. */
const buildSegments = (bounds: WaterBounds, cellSize: number): Segment[] => {
  const segments: Segment[] = [];
  const size = cellSize;
  const colW = size * COLUMN_SPACING;
  const rowH = size * SQRT3;
  const margin = size;
  const qMin = Math.floor((bounds.x - margin) / colW) - 1;
  const qMax = Math.ceil((bounds.x + bounds.width + margin) / colW) + 1;
  const rMin = Math.floor((bounds.y - margin) / rowH) - 1;
  const rMax = Math.ceil((bounds.y + bounds.height + margin) / rowH) + 1;
  for (let q = qMin; q <= qMax; q += 1) {
    for (let r = rMin; r <= rMax; r += 1) {
      const cx = colW * q;
      const cy = rowH * (r + q / 2);
      if (cx < bounds.x - margin || cx > bounds.x + bounds.width + margin) continue;
      if (cy < bounds.y - margin || cy > bounds.y + bounds.height + margin) continue;
      for (const start of OWNED_EDGE_STARTS) {
        const a0 = (start * Math.PI) / (HEX_SIDES / 2);
        const a1 = ((start + 1) * Math.PI) / (HEX_SIDES / 2);
        const x1 = cx + size * Math.cos(a0);
        const y1 = cy + size * Math.sin(a0);
        const x2 = cx + size * Math.cos(a1);
        const y2 = cy + size * Math.sin(a1);
        // Keep or drop this edge by a hash of its midpoint — deterministic, so
        // the same board of "broken" hexagons draws every frame.
        const mx = Math.round((x1 + x2) * 0.5);
        const my = Math.round((y1 + y2) * 0.5);
        if (coordHash(mx, my) < EDGE_KEEP_PERCENT) {
          segments.push({ x1, x2, y1, y2 });
        }
      }
    }
  }
  return segments;
};

/** Cache of generated segment geometry, keyed by (cellSize, snapped bounds). The
 * lattice is expensive-ish to build but constant for a fixed pool + camera, so we
 * build it once and only translate it by the per-frame drift. */
const segmentCache = new Map<string, readonly Segment[]>();

const cachedSegments = (bounds: WaterBounds, cellSize: number): readonly Segment[] => {
  const key = `${Math.round(cellSize)}:${Math.round(bounds.x)}:${Math.round(bounds.y)}:${Math.round(bounds.width)}:${Math.round(bounds.height)}`;
  const hit = segmentCache.get(key);
  if (hit !== undefined) {
    return hit;
  }
  const built = buildSegments(bounds, cellSize);
  segmentCache.set(key, built);
  return built;
};

/**
 * Draw a stylized water surface inside the caller's boundary. Small, boring, and
 * deterministic — see the file header for the full contract.
 */
export const drawStylizedWaterSurface = (ctx: CanvasRenderingContext2D, options: StylizedWaterOptions): void => {
  const { bounds } = options;
  ctx.save();

  // Clip to the pool, minus any holes (objects on the water).
  ctx.beginPath();
  options.tracePool(ctx);
  options.traceHoles?.(ctx);
  ctx.clip("evenodd");

  // Optional base water fill.
  if (options.baseColor !== undefined) {
    ctx.fillStyle = options.baseColor;
    ctx.fillRect(bounds.x, bounds.y, bounds.width, bounds.height);
  }

  // The broken cellular highlight net, drifted by a subtle deterministic amount.
  const driftX = Math.sin(options.timeSeconds * DRIFT_X_RATE) * options.driftAmount;
  const driftY = Math.cos(options.timeSeconds * DRIFT_Y_RATE) * options.driftAmount * DRIFT_Y_SCALE;
  const segments = cachedSegments(bounds, options.cellSize);
  ctx.save();
  ctx.translate(driftX, driftY);
  ctx.globalAlpha = options.opacity;
  ctx.filter = `blur(${options.softnessPx}px)`;
  ctx.strokeStyle = options.lineColor;
  ctx.lineWidth = options.lineWidth;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  for (const segment of segments) {
    ctx.moveTo(segment.x1, segment.y1);
    ctx.lineTo(segment.x2, segment.y2);
  }
  ctx.stroke();
  ctx.restore();

  // Shoreline fade: a thick, blurred stroke of the water color along the pool
  // path. Half of it lies outside the clip (discarded); the inside half covers
  // the pattern near the edge, so the net fades out before the shoreline instead
  // of ending in sharp line stubs. Two passes deepen the cover without a hard rim.
  ctx.filter = `blur(${options.edgeFadePx * 0.5}px)`;
  ctx.strokeStyle = options.edgeColor;
  ctx.lineJoin = "round";
  ctx.globalAlpha = 0.7;
  ctx.lineWidth = options.edgeFadePx * 2;
  ctx.beginPath();
  options.tracePool(ctx);
  ctx.stroke();
  ctx.stroke();

  ctx.restore();
};
