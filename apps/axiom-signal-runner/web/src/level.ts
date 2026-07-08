/*
 * Deterministic route generation. `generateLevel(seed)` returns the same `Level`
 * for the same seed forever: a winding centerline, exactly `SHARD_GOAL` shards and
 * `PLATE_GOAL` plates, sparse obstacles/drones that always leave a fair centerline
 * lane, decorative scatter, background mountains, and a final beacon.
 *
 * The one and only randomness source is the seeded `Rng` (rng.ts); no wall clock, no
 * unseeded draws, no unordered iteration — so the whole world is a pure function of
 * the seed, which the tests pin.
 */

import {
  INTRO_Z,
  NODE_COUNT,
  PATH_HALF,
  PLATE_GOAL,
  PLATE_HALF,
  SEG_LEN,
  SHARD_GOAL,
} from "./constants.ts";
import type { Deco, DroneHazard, Level, Mountain, Obstacle, PathNode, Plate, Shard } from "./types.ts";
import { type Rng, makeRng } from "./rng.ts";

/** A guarded z-range other placements avoid, so a fair centerline lane survives. */
interface Reserved {
  readonly z: number;
  readonly pad: number;
}

/** True if `z` is within `pad` of any reserved objective band. */
const nearReserved = (z: number, reserved: readonly Reserved[]): boolean =>
  reserved.some((r) => Math.abs(z - r.z) < r.pad);

/** Build the winding centerline: a smooth random-walk heading integrated into `cx`. */
const buildNodes = (rng: Rng, plateZs: readonly number[]): PathNode[] => {
  const nodes: PathNode[] = [];
  let cx = 0;
  let heading = 0; // lateral slope (dcx per world unit)
  let curveTarget = 0;
  for (let i = 0; i < NODE_COUNT; i += 1) {
    const z = i * SEG_LEN;
    // Occasionally pick a new gentle target slope; the intro stays straight.
    const retarget = z > INTRO_Z && rng.chance(0.012);
    curveTarget = retarget ? rng.range(-0.9, 0.9) : curveTarget;
    // Ease the heading toward the target, then integrate into the centerline.
    heading += (curveTarget - heading) * 0.06;
    heading = z < INTRO_Z ? heading * 0.4 : heading;
    const curve = heading * 0.5;
    cx += curve * SEG_LEN;
    // Widen the path smoothly around each plate node.
    const nearPlate = plateZs.reduce((w, pz) => Math.max(w, 1 - Math.min(1, Math.abs(z - pz) / 220)), 0);
    const width = PATH_HALF + (PLATE_HALF - PATH_HALF) * nearPlate;
    nodes.push({ curve, cx, width });
  }
  return nodes;
};

/** The centerline world-x at distance `z` (linear-interpolated between nodes). */
const centerAt = (nodes: readonly PathNode[], z: number): number => {
  const f = z / SEG_LEN;
  const i = Math.max(0, Math.min(nodes.length - 1, Math.floor(f)));
  const j = Math.min(nodes.length - 1, i + 1);
  const t = f - i;
  return (nodes[i] as PathNode).cx * (1 - t) + (nodes[j] as PathNode).cx * t;
};

/** The path half-width at distance `z`. */
const widthAt = (nodes: readonly PathNode[], z: number): number => {
  const i = Math.max(0, Math.min(nodes.length - 1, Math.round(z / SEG_LEN)));
  return (nodes[i] as PathNode).width;
};

/** Place `PLATE_GOAL` plates at fixed usable-length fractions (centerline). */
const buildPlates = (usable: number): { plates: Plate[]; zs: number[] } => {
  const fractions = [0.28, 0.55, 0.8].slice(0, PLATE_GOAL);
  const zs = fractions.map((f) => INTRO_Z + f * (usable - INTRO_Z));
  const plates = zs.map((z): Plate => ({ activated: false, lateral: 0, z }));
  return { plates, zs };
};

/** Place exactly `SHARD_GOAL` shards, weaving near the centerline within the lane. */
const buildShards = (rng: Rng, nodes: readonly PathNode[], usable: number): { shards: Shard[]; zs: number[] } => {
  const first = INTRO_Z + 120;
  const last = usable - 160;
  const shards: Shard[] = [];
  const zs: number[] = [];
  for (let k = 0; k < SHARD_GOAL; k += 1) {
    const z = first + ((last - first) * k) / (SHARD_GOAL - 1);
    const lane = widthAt(nodes, z) * 0.5;
    // A weaving pattern plus a small deterministic jitter, clamped to the lane.
    const weave = Math.sin(k * 0.9) * lane * 0.7 + rng.range(-lane * 0.25, lane * 0.25);
    const lateral = Math.max(-lane, Math.min(lane, weave));
    shards.push({ collected: false, lateral, z });
    zs.push(z);
  }
  return { shards, zs };
};

/** Sparse rocks/columns near a path edge, never inside the reserved lane bands. */
const buildObstacles = (rng: Rng, nodes: readonly PathNode[], usable: number, reserved: readonly Reserved[]): Obstacle[] => {
  const out: Obstacle[] = [];
  for (let z = INTRO_Z + 400; z < usable - 200; z += 190) {
    const jz = z + rng.range(-60, 60);
    if (nearReserved(jz, reserved) || rng.chance(0.35)) {
      continue;
    }
    const width = widthAt(nodes, jz);
    const side = rng.chance(0.5) ? -1 : 1;
    const lateral = side * rng.range(0.5, 0.92) * width;
    const kind = rng.chance(0.5) ? "rock" : "column";
    out.push({ kind, lateral, radius: kind === "column" ? 40 : 34, z: jz });
  }
  return out;
};

/** A handful of bobbing drone hazards in the back half, clear of objective bands. */
const buildDrones = (rng: Rng, nodes: readonly PathNode[], usable: number, reserved: readonly Reserved[]): DroneHazard[] => {
  const out: DroneHazard[] = [];
  for (let z = usable * 0.35; z < usable - 300; z += 320) {
    const jz = z + rng.range(-80, 80);
    if (nearReserved(jz, reserved)) {
      continue;
    }
    const width = widthAt(nodes, jz);
    out.push({
      baseLateral: rng.range(-0.35, 0.35) * width,
      disabled: false,
      sway: rng.range(0.15, 0.32) * width,
      z: jz,
    });
  }
  return out;
};

/** Decorative side props (trees, rocks, ruin pillars) scattered beyond the edges. */
const buildDecos = (rng: Rng, nodes: readonly PathNode[], usable: number): Deco[] => {
  const out: Deco[] = [];
  // Mostly pines + rocks, with the occasional broken ruin pillar (scattered, not a colonnade).
  const kinds: Deco["kind"][] = ["tree", "tree", "tree", "tree", "rock", "rock", "pillar"];
  for (let z = 60; z < usable; z += rng.range(46, 108)) {
    const width = widthAt(nodes, z);
    for (const side of [-1, 1]) {
      if (rng.chance(0.4)) {
        continue;
      }
      const lateral = side * (width + rng.range(60, 900));
      out.push({ kind: rng.pick(kinds), lateral, scale: rng.range(0.8, 1.35), z });
    }
  }
  return out;
};

/** Background mountain silhouettes (screen-fraction space, static parallax). */
const buildMountains = (rng: Rng): Mountain[] => {
  const out: Mountain[] = [];
  const count = 7 + rng.int(3);
  for (let i = 0; i < count; i += 1) {
    out.push({
      cx: rng.range(-0.05, 1.05),
      halfWidth: rng.range(0.12, 0.26),
      height: rng.range(0.18, 0.4),
      shade: rng.int(3),
    });
  }
  // Farthest (lightest) first so nearer ridges paint over them.
  return out.sort((a, b) => b.shade - a.shade);
};

/** Generate the full, deterministic level for `seed`. */
export const generateLevel = (seed: number): Level => {
  const rng = makeRng(seed);
  const length = (NODE_COUNT - 1) * SEG_LEN;
  const beaconZ = length - 40;
  const usable = beaconZ;

  const { plates, zs: plateZs } = buildPlates(usable);
  const nodes = buildNodes(rng, plateZs);
  const { shards, zs: shardZs } = buildShards(rng, nodes, usable);

  const reserved: Reserved[] = [
    ...plateZs.map((z): Reserved => ({ pad: 160, z })),
    ...shardZs.map((z): Reserved => ({ pad: 90, z })),
    { pad: 260, z: beaconZ },
  ];

  return {
    beaconZ,
    decos: buildDecos(rng, nodes, usable),
    drones: buildDrones(rng, nodes, usable, reserved),
    length,
    mountains: buildMountains(rng),
    nodes,
    obstacles: buildObstacles(rng, nodes, usable, reserved),
    plates,
    seed,
    segLen: SEG_LEN,
    shards,
  };
};

/** The centerline world-x at distance `z` (re-exported for sim + render). */
export const centerlineAt = centerAt;
/** The path half-width at distance `z` (re-exported for sim + render). */
export const pathWidthAt = widthAt;
