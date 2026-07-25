/*
 * water-surface.ts — a tiny reusable "stylized water surface" primitive: it turns
 * a circular region into a SPARSE honeycomb line net that makes a flat blue area
 * read as water, without a shader, a texture, or a VFX system.
 *
 * It is pure DATA, not a drawing effect: `waterSurface(options)` returns a small
 * bundle of `MaterialSpec`s and `SceneInstance`s that a game merges into its scene
 * exactly like any other geometry. So it renders on BOTH backends (WebGL2 and the
 * Canvas2D software path), it is occluded correctly by anything opaque sitting on
 * the water (the strips are ordinary depth-tested translucent geometry), and it is
 * fully deterministic — no clock, no randomness.
 *
 * The pattern is a HEXAGON grid: hexagons are laid on a hex lattice across the
 * disc and their edges are drawn as short segments, DEDUPLICATED so each shared
 * edge is one line (not a double-weight seam). The result is a large-celled
 * honeycomb. Each edge is TWO stacked translucent strips — a wide faint HALO and a
 * narrower CORE — so its edges blend softly (alpha layering) instead of reading as
 * a razor-sharp wire. Low opacity and a light-cyan tint keep the whole thing a
 * subtle, low-contrast surface pattern. Everything is expressed as array
 * transforms (no control flow), so it stays inside the branchless spine.
 */

import type { EngineQuat, MaterialSpec, Rgba, Transform } from "./api.ts";
import { orElse, presentOf } from "./branchless.ts";
import type { SceneInstance } from "./game.ts";

/** A circular water region to dress, plus the knobs that shape the net. Only
 * `radius` is required; every other field has a default tuned to drop the
 * treasure-chest pool look in immediately. */
export interface WaterSurfaceOptions {
  /** Radius of the water disc the net covers (world units). */
  readonly radius: number;
  /** Center of the disc on the ground plane (default origin). */
  readonly center?: { readonly x: number; readonly z: number };
  /** Height of the strips — just above the water surface, below anything on it. */
  readonly y?: number;
  /** Hexagon size (center-to-vertex). Larger = fewer, bigger cells. */
  readonly cellSize?: number;
  /** Core strip width (world units). */
  readonly lineWidth?: number;
  /** Halo width as a multiple of `lineWidth` — how far the soft edge feathers. */
  readonly softness?: number;
  /** Strip tint. Kept light and desaturated so the net stays low-contrast. */
  readonly lineColor?: Rgba;
  /** Core strip opacity (the halo is a fraction of this). */
  readonly opacity?: number;
  /** Shift of the whole lattice; 0 is static. A game may drift this slowly for a
   * barely-there motion (at the cost of re-posing the strips). */
  readonly drift?: number;
  /** An optional solid base disc drawn under the net (its own blue fill). Omit it
   * to lay the net over a blue region the scene already draws. */
  readonly baseColor?: Rgba;
  /** Prefix for the generated material + instance names (default "water"). */
  readonly keyPrefix?: string;
}

/** The generated bundle: name→material specs to register, and the strip
 * instances to place. A game spreads both into its resources and its scene. */
export interface WaterSurface {
  readonly materials: Readonly<Record<string, MaterialSpec>>;
  readonly instances: readonly SceneInstance[];
}

// ── defaults + tuning (hoisted so no value is a bare magic number) ────────────────
const DEFAULT_CELL_SIZE = 2.1;
const DEFAULT_LINE_WIDTH = 0.05;
const DEFAULT_SOFTNESS = 6;
const DEFAULT_OPACITY = 0.08;
const DEFAULT_Y = -0.02;
const DEFAULT_DRIFT = 0;
const DEFAULT_PREFIX = "water";
const HALF = 0.5;
/** Strip vertical thickness — flat, just enough to face the tabletop camera. */
const STRIP_HEIGHT = 0.02;
/** The halo strip is much fainter than the core, so the layered edge feathers
 * out to nothing rather than reading as a razor line. */
const HALO_OPACITY_FRAC = 0.4;
/** A whisper of emissive lifts a translucent strip off "dark grey blob" toward a
 * light water crest (the same trick the scene's other translucent overlays use).
 * Kept low so the net stays a subtle, low-contrast surface pattern, not a glow. */
const CORE_EMISSIVE_FRAC = 0.14;
const HALO_EMISSIVE_FRAC = 0.07;
const COLOR_R = 0.66;
const COLOR_G = 0.88;
const COLOR_B = 0.92;
const OPAQUE = 1;
const DEFAULT_COLOR: Rgba = [COLOR_R, COLOR_G, COLOR_B, OPAQUE];
const ORIGIN = { x: 0, z: 0 } as const;
const IDENTITY_QUAT: EngineQuat = [0, 0, 0, 1];
// Hex-lattice geometry.
const HEX_SIDES = 6;
const THREE = 3;
const SQRT3 = Math.sqrt(THREE);
const TAU = Math.PI + Math.PI;
/** Flat-top column spacing: a hexagon center steps 1.5·size in x per column. */
const COLUMN_SPACING = 1.5;
/** Snap endpoints to this many units when deduplicating shared edges. */
const SNAP = 100;
/** The six edge starts 0..5 (each edge joins vertex i to vertex i+1). */
const EDGE_STARTS: readonly number[] = Array.from({ length: HEX_SIDES }, (_slot, index): number => index);

// ── internal shapes ──────────────────────────────────────────────────────────────
interface Point {
  readonly x: number;
  readonly z: number;
}
interface Cell {
  readonly col: number;
  readonly row: number;
}
interface Edge {
  readonly from: Point;
  readonly to: Point;
}
interface ResolvedWater {
  readonly radius: number;
  readonly center: { readonly x: number; readonly z: number };
  readonly y: number;
  readonly cellSize: number;
  readonly lineWidth: number;
  readonly softness: number;
  readonly opacity: number;
  readonly drift: number;
  readonly color: Rgba;
  readonly prefix: string;
}
interface LineLayer {
  readonly name: string;
  readonly width: number;
}
interface BuildContext {
  readonly config: ResolvedWater;
  readonly layers: readonly LineLayer[];
}
interface StripSpec {
  readonly x: number;
  readonly z: number;
  readonly y: number;
  readonly angle: number;
  readonly length: number;
  readonly width: number;
}

/** A tint scaled toward black by `factor`, as an emissive triple-plus-alpha. */
const emissiveOf = (color: Rgba, factor: number): Rgba => {
  const [red, green, blue] = color;
  return [red * factor, green * factor, blue * factor, OPAQUE];
};

/** A flat strip's transform: a thin box lying on the ground plane, its long axis
 * (`length`) rotated to run along the edge direction `angle`. */
const stripTransform = (spec: StripSpec): Transform => {
  const { x, z, y, angle, length, width } = spec;
  const half = angle * HALF;
  // Yaw about +Y by −angle so the box's local +X points along (cos a, 0, sin a).
  const rotation: EngineQuat = [0, -Math.sin(half), 0, Math.cos(half)];
  return { position: { x, y, z }, rotation, scale: { x: length, y: STRIP_HEIGHT, z: width } };
};

/** Apply every option's default. */
const resolveOptions = (options: WaterSurfaceOptions): ResolvedWater => ({
  cellSize: orElse(options.cellSize, DEFAULT_CELL_SIZE),
  center: orElse(options.center, ORIGIN),
  color: orElse(options.lineColor, DEFAULT_COLOR),
  drift: orElse(options.drift, DEFAULT_DRIFT),
  lineWidth: orElse(options.lineWidth, DEFAULT_LINE_WIDTH),
  opacity: orElse(options.opacity, DEFAULT_OPACITY),
  prefix: orElse(options.keyPrefix, DEFAULT_PREFIX),
  radius: options.radius,
  softness: orElse(options.softness, DEFAULT_SOFTNESS),
  y: orElse(options.y, DEFAULT_Y),
});

/** The two feathering layers, widest-and-faintest first so the core lands on top. */
const layersOf = (config: ResolvedWater): readonly LineLayer[] => [
  { name: `${config.prefix}Halo`, width: config.lineWidth * config.softness },
  { name: `${config.prefix}Core`, width: config.lineWidth },
];

/** A symmetric integer range −span..span. */
const axialRange = (span: number): readonly number[] => Array.from({ length: span + span + 1 }, (_slot, index): number => index - span);

/** The world center of the flat-top hexagon at axial cell (col, row). */
const hexCenter = (config: ResolvedWater, cell: Cell): Point => {
  const size = config.cellSize;
  return {
    x: config.center.x + size * COLUMN_SPACING * cell.col + config.drift,
    z: config.center.z + size * SQRT3 * (cell.row + cell.col * HALF) + config.drift,
  };
};

/** Every hexagon center whose middle falls inside the disc. */
const hexCenters = (config: ResolvedWater): readonly Point[] => {
  const size = config.cellSize;
  const cols = axialRange(Math.ceil(config.radius / (size * COLUMN_SPACING)) + 1);
  const rows = axialRange(Math.ceil(config.radius / (size * SQRT3)) + 1);
  return cols
    .flatMap((col): readonly Point[] => rows.map((row): Point => hexCenter(config, { col, row })))
    .filter((point): boolean => Math.hypot(point.x - config.center.x, point.z - config.center.z) < config.radius);
};

/** One vertex of a flat-top hexagon (vertex `index` at 60°·index). */
const vertexOf = (center: Point, size: number, index: number): Point => {
  const angle = (index * TAU) / HEX_SIDES;
  return { x: center.x + size * Math.cos(angle), z: center.z + size * Math.sin(angle) };
};

/** Every hexagon edge across the disc, with shared edges still duplicated. */
const rawEdges = (config: ResolvedWater): readonly Edge[] =>
  hexCenters(config).flatMap((center): readonly Edge[] =>
    EDGE_STARTS.map((index): Edge => ({ from: vertexOf(center, config.cellSize, index), to: vertexOf(center, config.cellSize, index + 1) })));

/** A canonical key for an edge, so the two hexagons sharing it collapse to one. */
const edgeKey = (edge: Edge): string => {
  const snap = (value: number): number => Math.round(value * SNAP);
  const ends = [`${snap(edge.from.x)},${snap(edge.from.z)}`, `${snap(edge.to.x)},${snap(edge.to.z)}`];
  return ends.toSorted((left, right): number => left.localeCompare(right)).join("|");
};

/** The two strips (halo + core) welded along one edge, both boxes flat on the plane. */
const edgeStrips = (context: BuildContext, edge: Edge, edgeIndex: number): readonly SceneInstance[] => {
  const { from, to } = edge;
  const dx = to.x - from.x;
  const dz = to.z - from.z;
  const angle = Math.atan2(dz, dx);
  const length = Math.hypot(dx, dz) + context.config.lineWidth;
  const midX = (from.x + to.x) * HALF;
  const midZ = (from.z + to.z) * HALF;
  return context.layers.map((layer, layerIndex): SceneInstance => ({
    key: `${context.config.prefix}:${edgeIndex}:${layerIndex}`,
    material: layer.name,
    mesh: "box",
    transform: stripTransform({ angle, length, width: layer.width, x: midX, y: context.config.y, z: midZ }),
  }));
};

/** An optional solid base disc under the net (its own blue fill). */
const baseOf = (config: ResolvedWater, baseColor: Rgba | undefined): readonly SceneInstance[] =>
  presentOf(baseColor).map((): SceneInstance => ({
    key: `${config.prefix}:base`,
    material: `${config.prefix}Base`,
    mesh: "cylinder",
    transform: {
      position: { x: config.center.x, y: config.y - STRIP_HEIGHT, z: config.center.z },
      rotation: IDENTITY_QUAT,
      scale: { x: config.radius + config.radius, y: STRIP_HEIGHT, z: config.radius + config.radius },
    },
  }));

/** The halo/core (and optional base) material specs. */
const materialsOf = (config: ResolvedWater, baseColor: Rgba | undefined): Readonly<Record<string, MaterialSpec>> => {
  const { color, opacity, prefix } = config;
  const lineMaterials: Record<string, MaterialSpec> = {
    [`${prefix}Core`]: { baseColor: color, emissive: emissiveOf(color, CORE_EMISSIVE_FRAC), opacity },
    [`${prefix}Halo`]: { baseColor: color, emissive: emissiveOf(color, HALO_EMISSIVE_FRAC), opacity: opacity * HALO_OPACITY_FRAC },
  };
  const baseMaterials = Object.fromEntries(presentOf(baseColor).map((fill): readonly [string, MaterialSpec] => [`${prefix}Base`, { baseColor: fill }]));
  return Object.assign(lineMaterials, baseMaterials);
};

/** Build the stylized water-surface bundle for a circular region. */
export const waterSurface = (options: WaterSurfaceOptions): WaterSurface => {
  const config = resolveOptions(options);
  const context: BuildContext = { config, layers: layersOf(config) };
  const uniqueEdges = [...new Map(rawEdges(config).map((edge): readonly [string, Edge] => [edgeKey(edge), edge])).values()];
  const lineInstances = uniqueEdges.flatMap((edge, edgeIndex): readonly SceneInstance[] => edgeStrips(context, edge, edgeIndex));
  const instances = [...baseOf(config, options.baseColor), ...lineInstances];
  return { instances, materials: materialsOf(config, options.baseColor) };
};
