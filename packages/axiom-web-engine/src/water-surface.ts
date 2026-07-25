/*
 * water-surface.ts — a tiny reusable "stylized water surface" primitive: it turns
 * a circular region into a SPARSE cellular line net that makes a flat blue area
 * read as water, without a shader, a texture, or a VFX system.
 *
 * It is pure DATA, not a drawing effect: `waterSurface(options)` returns a small
 * bundle of `MaterialSpec`s and `SceneInstance`s that a game merges into its scene
 * exactly like any other geometry. So it renders on BOTH backends (WebGL2 and the
 * Canvas2D software path), it is occluded correctly by anything opaque sitting on
 * the water (the strips are ordinary depth-tested translucent geometry), and it is
 * fully deterministic — no clock, no randomness.
 *
 * The pattern is three families of parallel chords at 0deg/60deg/120deg, clipped
 * to the circle: their union is a large-celled triangular/honeycomb net. Each line
 * is TWO stacked translucent strips — a wide faint HALO and a narrower CORE — so
 * its edges blend softly (alpha layering) instead of reading as a razor-sharp wire.
 * Low opacity and a light-cyan tint keep the whole thing a subtle, low-contrast
 * surface pattern. Everything is expressed as array transforms (no control flow),
 * so it stays inside the branchless spine.
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
  /** Spacing between parallel lines (larger = fewer, bigger cells). */
  readonly cellSize?: number;
  /** Core strip width (world units). */
  readonly lineWidth?: number;
  /** Halo width as a multiple of `lineWidth` — how far the soft edge feathers. */
  readonly softness?: number;
  /** Strip tint. Kept light and desaturated so the net stays low-contrast. */
  readonly lineColor?: Rgba;
  /** Core strip opacity (the halo is a fraction of this). */
  readonly opacity?: number;
  /** Perpendicular shift of the whole net; 0 is static. A game may drift this
   * slowly for a barely-there motion (at the cost of re-posing the strips). */
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
const DEFAULT_CELL_SIZE = 1.9;
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
/** Three line directions 0deg/60deg/120deg; their union is the cellular net. */
const FAMILY_DIVISOR = 3;
const SIXTY = Math.PI / FAMILY_DIVISOR;
const FAMILIES: readonly number[] = [0, SIXTY, SIXTY + SIXTY];

// ── internal shapes ──────────────────────────────────────────────────────────────
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
  readonly offsets: readonly number[];
}
interface Chord {
  readonly angle: number;
  readonly familyIndex: number;
  readonly offset: number;
  readonly offsetIndex: number;
  readonly perpX: number;
  readonly perpZ: number;
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
 * (`length`) rotated to run along the family direction `angle`. */
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

/** Perpendicular offsets from the center, one line per `cellSize` step to the rim
 * (out-of-disc chords are filtered later). */
const offsetsOf = (config: ResolvedWater): readonly number[] => {
  const steps = Math.max(OPAQUE, Math.ceil(config.radius / config.cellSize));
  return Array.from({ length: steps + steps + OPAQUE }, (_slot, index): number => (index - steps) * config.cellSize + config.drift);
};

/** The two strips (halo + core) of one chord, both boxes flat on the plane. */
const chordStrips = (config: ResolvedWater, layers: readonly LineLayer[], chord: Chord): readonly SceneInstance[] => {
  const halfLen = Math.sqrt(config.radius * config.radius - chord.offset * chord.offset);
  const cx = config.center.x + chord.perpX * chord.offset;
  const cz = config.center.z + chord.perpZ * chord.offset;
  return layers.map((layer, layerIndex): SceneInstance => ({
    key: `${config.prefix}:${chord.familyIndex}:${chord.offsetIndex}:${layerIndex}`,
    material: layer.name,
    mesh: "box",
    transform: stripTransform({ angle: chord.angle, length: halfLen + halfLen, width: layer.width, x: cx, y: config.y, z: cz }),
  }));
};

/** Every chord of one line family, clipped to the disc. */
const familyLines = (context: BuildContext, angle: number, familyIndex: number): readonly SceneInstance[] => {
  const { config, layers, offsets } = context;
  const dirX = Math.cos(angle);
  const dirZ = Math.sin(angle);
  return offsets
    .filter((offset): boolean => Math.abs(offset) < config.radius)
    .flatMap((offset, offsetIndex): readonly SceneInstance[] =>
      chordStrips(config, layers, { angle, familyIndex, offset, offsetIndex, perpX: -dirZ, perpZ: dirX }));
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
  const context: BuildContext = { config, layers: layersOf(config), offsets: offsetsOf(config) };
  const lineInstances = FAMILIES.flatMap((angle, familyIndex): readonly SceneInstance[] => familyLines(context, angle, familyIndex));
  const instances = [...baseOf(config, options.baseColor), ...lineInstances];
  return { instances, materials: materialsOf(config, options.baseColor) };
};
