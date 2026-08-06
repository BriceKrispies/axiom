/*
 * canvas-water.ts — a small reusable Canvas2D "stylized water surface" effect,
 * part of the Canvas2D module (the platform-edge sibling of backend-canvas2d.ts).
 * It makes a flat blue region read as a body of water — no shader, no texture, no
 * fluid simulation, no per-pixel image processing, no randomness.
 *
 * The caller owns the BOUNDARY (it traces the pool path, and optionally the holes
 * where objects stand on the water); this module owns the RENDERING. What reads as
 * "water" is a few cheap, stacked cues, drawn in this order inside the clip:
 *   1. a DEPTH gradient (deeper toward the middle) so it has volume, not flat paint;
 *   2. a soft sun GLINT (a broad sheen offset toward the light) — the reflective cue;
 *   3. a broken cellular RIPPLE net of BOWED filaments (two-tone + sparkles) that
 *      drifts slowly — the caustic cue (see `waviness`);
 *   4. a shoreline FADE that dissolves the net before the edge (no sharp line ends);
 *   5. a lighter SHALLOW rim just inside the edge, where water meets the shore.
 * Every layer is optional — pass only the cues you want.
 *
 * Determinism: the ripple net's edges, their bow and the sparkles are all
 * kept/dropped/shaped by a pure COORDINATE HASH (never Math.random). All motion is
 * a tiny deterministic drift derived from an explicit `timeSeconds` the caller
 * passes in — the renderer never reads a wall clock. The generated filament
 * geometry is cached by (cellSize, bounds) so it is built once and only
 * re-translated per frame; `waviness` scales a stored unit bow at stroke time, so
 * dialling the curvature never rebuilds the lattice.
 *
 * This is a browser-API boundary (CanvasRenderingContext2D), so like the other
 * Canvas2D module files it sits outside the branchless / 100%-coverage spine laws
 * (see test-exempt.json and the .oxlintrc platform group); its correctness is
 * proven on the live browser path.
 */

/**
 * One filament of the ripple net, in canvas pixels: the two hex-edge endpoints,
 * the edge MIDPOINT, and the unit BOW that bends it away from that midpoint.
 *
 * A filament is a quadratic Bézier rather than a straight line because a straight
 * one cannot read as water. The net's topology is a hexagonal lattice, which is the
 * right topology for caustics (irregular closed cells), but drawn with `lineTo` the
 * cells are literal polygons: at a large `cellSize` the eye reads hex TILES, and at
 * a small one it reads a wire mesh. Neither is a shortcoming of the caller's
 * parameters — the straightness is in the geometry, so it is fixed here.
 *
 * `bx`/`by` is the control-point offset at `waviness` 1: perpendicular to the edge,
 * scaled by the edge's own length, and hash-SIGNED so neighbouring filaments bend
 * opposite ways. Storing it pre-scaled keeps the cached lattice independent of
 * `waviness` (see `cachedSegments`).
 */
interface Segment {
  readonly bx: number;
  readonly by: number;
  readonly mx: number;
  readonly my: number;
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

/** A soft directional sun sheen on the surface — the single strongest "this is a
 * reflective liquid" cue. `dirX`/`dirY` point toward the light in screen space
 * (x right, y down); the sheen sits offset that way from the pool center. */
export interface WaterGlint {
  readonly dirX: number;
  readonly dirY: number;
  readonly color: string;
  /** 0..1 — how far the sheen is offset and how strong it reads. */
  readonly strength: number;
}

/** Options for `drawStylizedWaterSurface`. Colors are any CSS color string. */
export interface StylizedWaterOptions {
  /** Trace the water boundary as a closed path (the caller owns the shape). Used
   * for the clip, the shoreline fade, and the shallow rim. */
  readonly tracePool: (ctx: CanvasRenderingContext2D) => void;
  /** Optionally add holes (objects standing on the water) to the CLIP path, so
   * nothing is drawn over them. Traced right after `tracePool` (even-odd rule). */
  readonly traceHoles?: (ctx: CanvasRenderingContext2D) => void;
  /** Where to generate the lattice (a bounding box of the pool, in canvas px). */
  readonly bounds: WaterBounds;
  /** Optional flat base fill drawn first (omit to overlay a region already blue). */
  readonly baseColor?: string;
  /** Optional DEEP-water tint painted as a radial gradient (this color at the
   * middle, fading to transparent at the rim) so the pool reads as having depth. */
  readonly depthColor?: string;
  /** Optional soft sun sheen. */
  readonly glint?: WaterGlint;
  /** The ripple highlight color. */
  readonly lineColor: string;
  /** Optional darker companion drawn just under each highlight, so a line reads as
   * a ripple crest-and-trough rather than a flat wire. */
  readonly troughColor?: string;
  /** Optional tiny glints dropped at some ripple vertices (light catching a peak). */
  readonly sparkleColor?: string;
  /** The water color used to fade the ripples out at the shoreline (matches the
   * pool so the cover is invisible except that it hides the net). */
  readonly edgeColor: string;
  /** Optional lighter SHALLOW band just inside the edge (water meeting the shore). */
  readonly shallowColor?: string;
  /** Hexagon size (center-to-vertex) in canvas px — large = sparse, big cells. */
  readonly cellSize: number;
  /**
   * How far each ripple filament BOWS, as a fraction of the authored full bow.
   * `0` (the default) is the straight-edged lattice this effect drew before the
   * option existed — byte-identical output, so an existing caller is unaffected —
   * and `1` is a pronounced caustic curve. Values above 1 are accepted and simply
   * bend further; the cells stop closing cleanly well before that is useful.
   *
   * Water is the one thing in a stylized scene with no straight lines in it. Real
   * caustics are a net of curved, wandering filaments, and the difference between
   * "curved net" and "straight net" is the difference between a pool and a hex
   * board — it dominates the read at every cell size, which is why this is a
   * property of the effect rather than something a caller can approximate by
   * choosing `cellSize`.
   */
  readonly waviness?: number;
  /** Highlight line width in px. */
  readonly lineWidth: number;
  /** How ripple and shoreline strokes are joined and capped. Omitted, they
   * INHERIT whatever the caller configured on the context — which is how a host's
   * rendering-quality setting reaches this effect without every caller having to
   * thread it through. The effect was authored at `"round"`/`"round"`, which is
   * also the Canvas2D-wide default this ends up at when nobody sets anything. */
  readonly lineJoin?: CanvasLineJoin;
  readonly lineCap?: CanvasLineCap;
  /** Miter limit applied when `lineJoin` is `"miter"`. Inherited when omitted. */
  readonly miterLimit?: number;
  /** Ripple opacity (0..1) — keep low for a subtle, low-contrast net. */
  readonly opacity: number;
  /** Blur radius in px for the soft, non-razor edges. Keep small. */
  readonly softnessPx: number;
  /** Width of the soft shoreline cover in px — how far in the net fades. */
  readonly edgeFadePx: number;
  /** Peak drift of the net in px (0 = static). */
  readonly driftAmount: number;
  /** Deterministic time in seconds (the caller passes the explicit engine clock). */
  readonly timeSeconds: number;
}

/** Fraction of hex edges kept (the rest are dropped to break up the lattice so it
 * reads as stylized markings, not board-game tiles). */
const EDGE_KEEP_PERCENT = 58;
/** Fraction of ripple vertices that get a sparkle. */
const SPARKLE_PERCENT = 12;
/** Each hexagon owns three of its six edges (0, 2, 4), so a shared edge is
 * generated once — no double-weight seams. */
const OWNED_EDGE_STARTS = [0, 2, 4];
const HEX_SIDES = 6;
const SQRT3 = Math.sqrt(3);
const COLUMN_SPACING = 1.5;
const DRIFT_X_RATE = 0.18;
const DRIFT_Y_RATE = 0.14;
const DRIFT_Y_SCALE = 0.75;
/** The slow second ripple layer: fainter, drifting on its own rate for parallax. */
const DRIFT2_X_RATE = 0.11;
const DRIFT2_Y_RATE = 0.09;
const DRIFT2_SCALE = 0.6;
const DRIFT2_OPACITY = 0.5;
const TROUGH_OFFSET = 1.6;
const SPARKLE_RADIUS = 1.8;
const GLINT_DRIFT_RATE = 0.1;

/** Peak control-point offset of a filament at `waviness` 1, as a fraction of its
 * own length. A quadratic Bézier only reaches HALF its control offset, so the
 * visible sagitta is half of this — enough that no straight run survives anywhere
 * in the net, not enough for a filament to cross its neighbour. */
const BOW_FRACTION = 0.5;

/** Coordinate shift that gives the BOW its own hash stream, independent of the
 * keep/drop decision. Sharing one stream would correlate the two: the surviving
 * edges (`coordHash < EDGE_KEEP_PERCENT`, i.e. the low half) would all take the
 * same side of the bow range and the whole net would bend one way. */
const BOW_HASH_SHIFT = 7919;

/** A tiny deterministic hash of an integer pixel coordinate pair → 0..99. Not
 * cryptographic — just a stable, seed-free way to keep "some" edges/sparkles. */
const coordHash = (ix: number, iy: number): number => {
  let h = (ix * 73856093) ^ (iy * 19349663);
  h = (h ^ (h >>> 13)) >>> 0;
  return h % 100;
};

/** Which way and how hard one filament bends, in [-1, 1) — a stable function of its
 * midpoint, drawn from the bow's own hash stream (see `BOW_HASH_SHIFT`). */
const bowSwing = (ix: number, iy: number): number => coordHash(ix + BOW_HASH_SHIFT, iy - BOW_HASH_SHIFT) / 50 - 1;

/** Build the (undrifted) kept hex-edge filaments across `bounds`. Deterministic. */
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
        // The hash key is the ROUNDED midpoint (shared by both hexagons that own
        // this edge, so keep/drop and bow agree); the bow itself is built off the
        // exact midpoint, so a filament stays centred on its edge.
        const hx = Math.round((x1 + x2) * 0.5);
        const hy = Math.round((y1 + y2) * 0.5);
        if (coordHash(hx, hy) < EDGE_KEEP_PERCENT) {
          // The bow is the edge's own perpendicular, `(-ey, ex)`, scaled by the
          // hash-signed swing. The edge length cancels exactly: a perpendicular
          // UNIT times (BOW_FRACTION · length) is the same vector, so no square
          // root and no divide-by-zero guard are needed, and the bow is
          // automatically proportional to the cell — a smaller `cellSize` curves
          // by the same amount relative to its own filaments.
          const swing = bowSwing(hx, hy) * BOW_FRACTION;
          segments.push({
            bx: -(y2 - y1) * swing,
            by: (x2 - x1) * swing,
            mx: (x1 + x2) * 0.5,
            my: (y1 + y2) * 0.5,
            x1,
            x2,
            y1,
            y2,
          });
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

/** Fill the clipped region with a radial DEPTH gradient (deep in the middle,
 * fading to nothing at the rim) so the pool reads as a volume with a deep centre. */
const paintDepth = (ctx: CanvasRenderingContext2D, bounds: WaterBounds, color: string): void => {
  const cx = bounds.x + bounds.width * 0.5;
  const cy = bounds.y + bounds.height * 0.5;
  const radius = Math.max(bounds.width, bounds.height) * 0.5;
  const gradient = ctx.createRadialGradient(cx, cy, radius * 0.15, cx, cy, radius);
  gradient.addColorStop(0, color);
  gradient.addColorStop(1, "rgba(0,0,0,0)");
  ctx.fillStyle = gradient;
  ctx.fillRect(bounds.x, bounds.y, bounds.width, bounds.height);
};

/** Paint the soft sun sheen: a couple of broad, blurred, additive light blobs
 * offset from the pool centre toward the light, drifting a hair over time. */
const paintGlint = (ctx: CanvasRenderingContext2D, spec: { readonly bounds: WaterBounds; readonly glint: WaterGlint; readonly timeSeconds: number }): void => {
  const { bounds, glint, timeSeconds } = spec;
  const cx = bounds.x + bounds.width * 0.5;
  const cy = bounds.y + bounds.height * 0.5;
  const reach = Math.min(bounds.width, bounds.height) * 0.5;
  const wobble = Math.sin(timeSeconds * GLINT_DRIFT_RATE) * reach * 0.06;
  ctx.save();
  ctx.globalCompositeOperation = "lighter";
  ctx.filter = `blur(${reach * 0.1}px)`;
  // Offset the sheen well toward the light (the open water near the far rim) and
  // keep the blobs modest, so it does not halo whatever sits at the pool center.
  const blobs = [
    { gx: cx + glint.dirX * reach * 0.66 + wobble, gy: cy + glint.dirY * reach * 0.6, rad: reach * 0.4, alpha: 0.85 },
    { gx: cx + glint.dirX * reach * 0.98 - wobble, gy: cy + glint.dirY * reach * 0.9, rad: reach * 0.22, alpha: 0.55 },
  ];
  for (const blob of blobs) {
    const gradient = ctx.createRadialGradient(blob.gx, blob.gy, 0, blob.gx, blob.gy, blob.rad);
    gradient.addColorStop(0, glint.color);
    gradient.addColorStop(1, "rgba(0,0,0,0)");
    ctx.globalAlpha = blob.alpha * glint.strength;
    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.ellipse(blob.gx, blob.gy, blob.rad, blob.rad * 0.62, 0, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
};

/** The stroke shaping the caller had configured on the context when it called in
 * — captured BEFORE the first `ctx.save()`, so each layer can fall back to it
 * even though the effect saves and restores around every pass. */
interface InheritedStroke {
  readonly lineJoin: CanvasLineJoin;
  readonly lineCap: CanvasLineCap;
  readonly miterLimit: number;
}

/** One drifted, alpha-scaled pass of the ripple net. */
interface RippleLayer {
  readonly segments: readonly Segment[];
  readonly options: StylizedWaterOptions;
  readonly inherited: InheritedStroke;
  readonly driftX: number;
  readonly driftY: number;
  readonly alpha: number;
}

/** Trace every filament of the net into the current path, offset vertically by
 * `dy` (the trough pass rides just under the highlight). At `waviness` 0 the
 * control point IS the midpoint, and a quadratic whose control lies on the chord
 * is exactly that chord — which is why the default reproduces the old straight
 * lattice rather than merely approximating it. */
const traceRipples = (ctx: CanvasRenderingContext2D, layer: RippleLayer, dy: number): void => {
  const bow = layer.options.waviness ?? 0;
  ctx.beginPath();
  for (const s of layer.segments) {
    ctx.moveTo(s.x1, s.y1 + dy);
    ctx.quadraticCurveTo(s.mx + s.bx * bow, s.my + s.by * bow + dy, s.x2, s.y2 + dy);
  }
};

/** Stroke one drifted layer of the ripple net (optional darker trough under each
 * highlight, then the highlight), softened by a small blur. */
const strokeRipples = (ctx: CanvasRenderingContext2D, layer: RippleLayer): void => {
  const { options, driftX, driftY, alpha, inherited } = layer;
  ctx.save();
  ctx.translate(driftX, driftY);
  ctx.filter = `blur(${options.softnessPx}px)`;
  ctx.lineCap = options.lineCap ?? inherited.lineCap;
  ctx.lineJoin = options.lineJoin ?? inherited.lineJoin;
  ctx.miterLimit = options.miterLimit ?? inherited.miterLimit;
  ctx.lineWidth = options.lineWidth;
  if (options.troughColor !== undefined) {
    ctx.globalAlpha = alpha * 0.8;
    ctx.strokeStyle = options.troughColor;
    traceRipples(ctx, layer, TROUGH_OFFSET);
    ctx.stroke();
  }
  ctx.globalAlpha = alpha;
  ctx.strokeStyle = options.lineColor;
  traceRipples(ctx, layer, 0);
  ctx.stroke();
  ctx.restore();
};

/** One drifted sparkle pass. */
interface SparkleLayer {
  readonly segments: readonly Segment[];
  readonly color: string;
  readonly opacity: number;
  readonly driftX: number;
  readonly driftY: number;
}

/** Drop tiny bright sparkles on a hash-selected subset of ripple vertices. */
const paintSparkles = (ctx: CanvasRenderingContext2D, layer: SparkleLayer): void => {
  const { segments, color, opacity, driftX, driftY } = layer;
  ctx.save();
  ctx.translate(driftX, driftY);
  ctx.globalAlpha = opacity;
  ctx.fillStyle = color;
  ctx.filter = "blur(0.4px)";
  ctx.beginPath();
  for (const s of segments) {
    if (coordHash(Math.round(s.x1), Math.round(s.y1)) < SPARKLE_PERCENT) {
      ctx.moveTo(s.x1 + SPARKLE_RADIUS, s.y1);
      ctx.arc(s.x1, s.y1, SPARKLE_RADIUS, 0, Math.PI * 2);
    }
  }
  ctx.fill();
  ctx.restore();
};

/**
 * Draw a stylized water surface inside the caller's boundary. Small, boring, and
 * deterministic — see the file header for the full contract.
 */
export const drawStylizedWaterSurface = (ctx: CanvasRenderingContext2D, options: StylizedWaterOptions): void => {
  const { bounds } = options;
  const inherited: InheritedStroke = { lineCap: ctx.lineCap, lineJoin: ctx.lineJoin, miterLimit: ctx.miterLimit };
  ctx.save();

  // Clip to the pool, minus any holes (objects on the water).
  ctx.beginPath();
  options.tracePool(ctx);
  options.traceHoles?.(ctx);
  ctx.clip("evenodd");

  // 1. Base + depth: a flat fill (optional) then a deep-centre radial tint.
  if (options.baseColor !== undefined) {
    ctx.fillStyle = options.baseColor;
    ctx.fillRect(bounds.x, bounds.y, bounds.width, bounds.height);
  }
  if (options.depthColor !== undefined) {
    paintDepth(ctx, bounds, options.depthColor);
  }

  // 2. The reflective sun sheen.
  if (options.glint !== undefined) {
    paintGlint(ctx, { bounds, glint: options.glint, timeSeconds: options.timeSeconds });
  }

  // 3. The broken ripple net — two drifting layers for a little parallax life —
  // plus sparkles catching the light on some peaks.
  const driftX = Math.sin(options.timeSeconds * DRIFT_X_RATE) * options.driftAmount;
  const driftY = Math.cos(options.timeSeconds * DRIFT_Y_RATE) * options.driftAmount * DRIFT_Y_SCALE;
  const driftX2 = Math.sin(options.timeSeconds * DRIFT2_X_RATE + 1.7) * options.driftAmount * DRIFT2_SCALE;
  const driftY2 = Math.cos(options.timeSeconds * DRIFT2_Y_RATE + 0.5) * options.driftAmount * DRIFT2_SCALE;
  const segments = cachedSegments(bounds, options.cellSize);
  strokeRipples(ctx, { alpha: options.opacity * DRIFT2_OPACITY, driftX: driftX2, driftY: driftY2, inherited, options, segments });
  strokeRipples(ctx, { alpha: options.opacity, driftX, driftY, inherited, options, segments });
  if (options.sparkleColor !== undefined) {
    paintSparkles(ctx, { color: options.sparkleColor, driftX, driftY, opacity: options.opacity, segments });
  }

  // 4. Shoreline fade: a thick, blurred stroke of the water color along the pool
  // path. Half lies outside the clip (discarded); the inside half covers the net
  // near the edge, so it dissolves before the shoreline instead of ending in
  // sharp stubs. Two passes deepen the cover without a hard rim.
  ctx.filter = `blur(${options.edgeFadePx * 0.5}px)`;
  ctx.strokeStyle = options.edgeColor;
  ctx.lineJoin = options.lineJoin ?? inherited.lineJoin;
  ctx.lineCap = options.lineCap ?? inherited.lineCap;
  ctx.miterLimit = options.miterLimit ?? inherited.miterLimit;
  ctx.globalAlpha = 0.7;
  ctx.lineWidth = options.edgeFadePx * 2;
  ctx.beginPath();
  options.tracePool(ctx);
  ctx.stroke();
  ctx.stroke();

  // 5. Shallow rim: a soft lighter band right at the shore (water over sand).
  if (options.shallowColor !== undefined) {
    ctx.filter = `blur(${options.edgeFadePx * 0.4}px)`;
    ctx.strokeStyle = options.shallowColor;
    ctx.globalAlpha = 1;
    ctx.lineWidth = options.edgeFadePx;
    ctx.beginPath();
    options.tracePool(ctx);
    ctx.stroke();
  }

  ctx.restore();
};
