/*
 * The pseudo-3D world renderer: sky, faceted mountains, the advancing storm wall,
 * the winding path ribbon, and every projected world prop and entity (trees, rocks,
 * ruin pillars, shards, plates, obstacles, drones, and the final beacon). Everything
 * is procedural draw2d geometry — no image assets — and everything world-space goes
 * through `project()` so it shrinks toward the horizon and the path narrows with
 * distance, matching the reference's flat illustrated chase view.
 *
 * Pure presentation: it reads `State` and draws; it never mutates game state.
 */

import * as P from "./palette.ts";
import { HEIGHT, WIDTH } from "./constants.ts";
import { type Camera, HORIZON, makeCamera, project } from "./projection.ts";
import { box, diamond, disc, poly, seg, stroke } from "./draw.ts";
import { centerlineAt, pathWidthAt } from "./level.ts";
import type { Deco, DroneHazard, Level, State } from "./types.ts";
import type { Frame, Vec2 } from "@axiom/game";
import { droneLateralAt } from "./sim.ts";

const L_SKY = 0;
const L_GROUND = 2;
const L_STORM = 6;
const L_MOUNTAIN = 8;
const L_PATH = 20;
const L_PATH_EDGE = 22;
const L_OBJ = 30;
const L_TINT = 74;

/** Sky wash + snowy ground plane. */
const drawBackdrop = (frame: Frame): void => {
  box(frame, 0, 0, WIDTH, HEIGHT, P.SKY_TOP, L_SKY);
  box(frame, 0, HORIZON - 2, WIDTH, HEIGHT - HORIZON + 2, P.GROUND, L_GROUND);
};

/** One faceted, snow-capped mountain from its screen-fraction descriptor. */
const drawMountain = (frame: Frame, cx: number, peakY: number, halfW: number, shade: number): void => {
  const baseY = HORIZON;
  const peak: Vec2 = { x: cx, y: peakY };
  const left: Vec2 = { x: cx - halfW, y: baseY };
  const right: Vec2 = { x: cx + halfW, y: baseY };
  const mid: Vec2 = { x: cx + halfW * 0.18, y: baseY };
  poly(frame, [peak, left, right], (P.MOUNTAIN[shade] ?? P.MOUNTAIN[1])!, L_MOUNTAIN, P.OUTLINE_SOFT, 2);
  // A lighter left facet gives the low-poly crease.
  poly(frame, [peak, left, mid], (P.MOUNTAIN[Math.min(2, shade + 1)] ?? P.MOUNTAIN[2])!, L_MOUNTAIN);
  // Snow cap near the peak.
  const capH = (baseY - peakY) * 0.32;
  poly(
    frame,
    [peak, { x: cx - halfW * 0.22, y: peakY + capH }, { x: cx + halfW * 0.18, y: peakY + capH }],
    P.MOUNTAIN_SNOW,
    L_MOUNTAIN,
  );
};

const drawMountains = (frame: Frame, level: Level): void => {
  for (const m of level.mountains) {
    drawMountain(frame, m.cx * WIDTH, HORIZON - m.height * HEIGHT, m.halfWidth * WIDTH, m.shade);
  }
};

/** The jagged purple storm wall on the right, closing in with intensity. */
const drawStorm = (frame: Frame, intensity: number): void => {
  const edge = WIDTH * (0.86 - intensity * 0.52);
  const steps = 9;
  const jag: Vec2[] = [];
  for (let i = 0; i <= steps; i += 1) {
    const y = (i / steps) * HEIGHT;
    const zig = (i % 2 === 0 ? -1 : 1) * 46 + Math.sin(i * 1.7) * 18;
    jag.push({ x: edge + zig, y });
  }
  const fill: Vec2[] = [...jag, { x: WIDTH, y: HEIGHT }, { x: WIDTH, y: 0 }];
  poly(frame, fill, P.STORM, L_STORM, undefined, 0, 0.9);
  poly(frame, fill, P.STORM_DARK, L_STORM, undefined, 0, 0.25);
  stroke(frame, jag, P.STORM_BOLT, 3, L_STORM, 0.9);
};

/** Sample the path centerline into projected left/right screen edges, far→near. */
interface Ribbon {
  readonly left: Vec2[];
  readonly right: Vec2[];
  readonly z: number[];
  readonly visible: boolean[];
}

const sampleRibbon = (cam: Camera, level: Level, dist: number): Ribbon => {
  const left: Vec2[] = [];
  const right: Vec2[] = [];
  const z: number[] = [];
  const visible: boolean[] = [];
  let dd = 24;
  while (dd < 2200) {
    const zz = cam.camZ + dd;
    const clamped = Math.max(0, Math.min(level.beaconZ, zz));
    const cx = centerlineAt(level.nodes, clamped);
    const w = pathWidthAt(level.nodes, clamped);
    const l = project(cam, cx - w, zz);
    const r = project(cam, cx + w, zz);
    left.push({ x: l.x, y: l.y });
    right.push({ x: r.x, y: r.y });
    z.push(zz);
    visible.push(l.visible && zz <= level.beaconZ + 40);
    dd *= 1.09;
  }
  void dist;
  return { left, right, visible, z };
};

const drawPath = (frame: Frame, cam: Camera, level: Level, dist: number): void => {
  const rib = sampleRibbon(cam, level, dist);
  for (let i = rib.left.length - 2; i >= 0; i -= 1) {
    if (!rib.visible[i] || !rib.visible[i + 1]) {
      continue;
    }
    const band = Math.floor((rib.z[i] ?? 0) / 44) % 2 === 0 ? P.PATH : P.PATH_BAND;
    poly(frame, [rib.left[i]!, rib.right[i]!, rib.right[i + 1]!, rib.left[i + 1]!], band, L_PATH);
  }
  const visL = rib.left.filter((_, i) => rib.visible[i]);
  const visR = rib.right.filter((_, i) => rib.visible[i]);
  stroke(frame, visL, P.PATH_EDGE, 3, L_PATH_EDGE, 0.85);
  stroke(frame, visR, P.PATH_EDGE, 3, L_PATH_EDGE, 0.85);
};

/** A procedural pine tree standing at screen `(bx, by)` with pixel height `h`. */
const drawTree = (frame: Frame, bx: number, by: number, h: number): void => {
  box(frame, bx - h * 0.035, by - h * 0.13, h * 0.07, h * 0.15, P.TREE_TRUNK, L_OBJ);
  const tier = (baseF: number, apexF: number, halfF: number): void => {
    poly(
      frame,
      [
        { x: bx, y: by - h * apexF },
        { x: bx - h * halfF, y: by - h * baseF },
        { x: bx + h * halfF, y: by - h * baseF },
      ],
      P.TREE[0],
      L_OBJ,
      P.TREE[1],
      1.5,
    );
  };
  tier(0.1, 0.5, 0.26);
  tier(0.34, 0.72, 0.21);
  tier(0.56, 0.96, 0.15);
};

/** A faceted grey rock at screen `(bx, by)` with pixel width `w`. */
const drawRock = (frame: Frame, bx: number, by: number, w: number, dark = false): void => {
  const h = w * 0.72;
  poly(
    frame,
    [
      { x: bx - w * 0.5, y: by },
      { x: bx - w * 0.32, y: by - h * 0.7 },
      { x: bx + w * 0.05, y: by - h },
      { x: bx + w * 0.42, y: by - h * 0.55 },
      { x: bx + w * 0.5, y: by },
    ],
    dark ? P.ROCK_DARK : P.ROCK,
    L_OBJ,
    P.OUTLINE_SOFT,
    2,
  );
  poly(
    frame,
    [
      { x: bx + w * 0.05, y: by - h },
      { x: bx + w * 0.42, y: by - h * 0.55 },
      { x: bx + w * 0.5, y: by },
      { x: bx + w * 0.12, y: by },
    ],
    P.ROCK_DARK,
    L_OBJ,
  );
};

/** A standing ruin pillar (broken column) at screen `(bx, by)`, pixel height `h`. */
const drawPillar = (frame: Frame, bx: number, by: number, h: number): void => {
  const w = h * 0.26;
  box(frame, bx - w / 2, by - h, w, h, P.RUIN, L_OBJ, P.OUTLINE_SOFT, 2);
  box(frame, bx - w / 2, by - h, w * 0.42, h, P.RUIN_DARK, L_OBJ);
  box(frame, bx - w * 0.6, by - h - h * 0.08, w * 1.2, h * 0.1, P.RUIN, L_OBJ, P.OUTLINE_SOFT, 2);
};

const drawDeco = (frame: Frame, cam: Camera, level: Level, deco: Deco): void => {
  const wx = centerlineAt(level.nodes, deco.z) + deco.lateral;
  const p = project(cam, wx, deco.z);
  if (!p.visible || p.x < -260 || p.x > WIDTH + 260 || p.scale < 0.018) {
    return;
  }
  if (deco.kind === "tree") {
    drawTree(frame, p.x, p.y, 150 * deco.scale * p.scale);
  } else if (deco.kind === "rock") {
    drawRock(frame, p.x, p.y, 120 * deco.scale * p.scale);
  } else {
    drawPillar(frame, p.x, p.y, 220 * deco.scale * p.scale);
  }
};

/** A floating cyan shard (bobbing) at world `(wx, z)`. */
const drawShard = (frame: Frame, cam: Camera, wx: number, z: number, tick: number): void => {
  const bob = Math.sin(tick * 0.06 + z * 0.02) * 8;
  const p = project(cam, wx, z, 78 + bob);
  if (!p.visible || p.scale < 0.02 || p.scale > 2.4) {
    return;
  }
  const rx = 22 * p.scale;
  const ry = 34 * p.scale;
  disc(frame, p.x, p.y, ry * 1.5, P.alpha(P.SHARD, 0.16), L_OBJ);
  diamond(frame, p.x, p.y, rx, ry, P.SHARD, L_OBJ, P.SHARD_EDGE, Math.max(1, 2 * p.scale));
  diamond(frame, p.x, p.y, rx * 0.42, ry * 0.42, P.SHARD_CORE, L_OBJ);
};

/** A yellow pressure-plate marker flat on the ground. */
const drawPlate = (frame: Frame, cam: Camera, wx: number, z: number, active: boolean): void => {
  const p = project(cam, wx, z, 2);
  if (!p.visible || p.scale < 0.02) {
    return;
  }
  const rx = 46 * p.scale;
  const ry = 20 * p.scale;
  diamond(frame, p.x, p.y, rx * 1.35, ry * 1.35, P.alpha(P.PLATE, active ? 0.2 : 0.35), L_OBJ);
  diamond(frame, p.x, p.y, rx, ry, active ? P.alpha(P.PLATE, 0.5) : P.PLATE, L_OBJ, P.PLATE_EDGE, Math.max(1, 2 * p.scale));
};

/** A winged drone hazard (grey body, red core) hovering over the path. */
const drawDrone = (frame: Frame, cam: Camera, drone: DroneHazard, level: Level, tick: number): void => {
  const lateral = droneLateralAt(drone, tick);
  const wx = centerlineAt(level.nodes, drone.z) + lateral;
  const p = project(cam, wx, drone.z, 96);
  if (!p.visible || p.scale < 0.02) {
    return;
  }
  const r = 15 * p.scale;
  const a = drone.disabled ? 0.35 : 1;
  const wob = Math.sin(tick * 0.4 + drone.z) * r * 0.5;
  for (const s of [-1, 1]) {
    seg(frame, { x: p.x + s * r, y: p.y }, { x: p.x + s * r * 2.4, y: p.y - r - wob }, P.DRONE_BODY, Math.max(1.5, 3 * p.scale), L_OBJ, a);
    disc(frame, p.x + s * r * 2.4, p.y - r - wob, r * 0.5, P.DRONE_BODY, L_OBJ, P.OUTLINE_SOFT, 1, a);
  }
  disc(frame, p.x, p.y, r, P.DRONE_BODY, L_OBJ, P.OUTLINE, Math.max(1, 2 * p.scale), a);
  disc(frame, p.x, p.y, r * 0.5, P.DRONE_CORE, L_OBJ, undefined, 0, a);
};

/** The tall ruin-machine beacon with a cyan gem, projected at the route's end. */
const drawBeacon = (frame: Frame, cam: Camera, level: Level, state: State): void => {
  const wx = centerlineAt(level.nodes, level.beaconZ);
  const base = project(cam, wx, level.beaconZ, 4);
  if (!base.visible || base.scale < 0.015) {
    return;
  }
  const s = base.scale;
  const w = 150 * s;
  const h = 300 * s;
  box(frame, base.x - w / 2, base.y - h * 0.2, w, h * 0.2, P.RUIN, L_OBJ, P.OUTLINE_SOFT, 2);
  for (const side of [-1, 1]) {
    box(frame, base.x + side * w * 0.34 - w * 0.09, base.y - h, w * 0.18, h, P.RUIN, L_OBJ, P.OUTLINE_SOFT, 2);
  }
  box(frame, base.x - w * 0.5, base.y - h - h * 0.12, w, h * 0.14, P.RUIN_DARK, L_OBJ, P.OUTLINE_SOFT, 2);
  const glow = state.beaconRestored ? 0.9 : 0.4 + Math.sin(state.tick * 0.1) * 0.2;
  disc(frame, base.x, base.y - h * 0.62, w * 0.34, P.alpha(P.SLED_GLOW, glow * 0.4), L_OBJ);
  diamond(frame, base.x, base.y - h * 0.62, w * 0.16, w * 0.24, P.SHARD, L_OBJ, P.SHARD_EDGE, Math.max(1, 2 * s));
};

/** A sortable world entity: its route distance and its draw thunk. */
interface WorldItem {
  readonly z: number;
  readonly draw: () => void;
}

/** Draw all projected props + entities, painter-sorted far→near. */
const drawWorldObjects = (frame: Frame, cam: Camera, state: State): void => {
  const { level, tick } = state;
  const items: WorldItem[] = [];
  for (const d of level.decos) {
    items.push({ draw: () => drawDeco(frame, cam, level, d), z: d.z });
  }
  for (const o of level.obstacles) {
    const wx = centerlineAt(level.nodes, o.z) + o.lateral;
    items.push({
      draw: () => {
        const p = project(cam, wx, o.z);
        if (p.visible && p.scale > 0.02) {
          if (o.kind === "column") {
            drawPillar(frame, p.x, p.y, 120 * p.scale);
          } else {
            drawRock(frame, p.x, p.y, 130 * p.scale, true);
          }
        }
      },
      z: o.z,
    });
  }
  for (const s of level.shards) {
    if (!s.collected) {
      const wx = centerlineAt(level.nodes, s.z) + s.lateral;
      items.push({ draw: () => drawShard(frame, cam, wx, s.z, tick), z: s.z });
    }
  }
  for (const pl of level.plates) {
    const wx = centerlineAt(level.nodes, pl.z) + pl.lateral;
    items.push({ draw: () => drawPlate(frame, cam, wx, pl.z, pl.activated), z: pl.z });
  }
  for (const dr of level.drones) {
    items.push({ draw: () => drawDrone(frame, cam, dr, level, tick), z: dr.z });
  }
  items.push({ draw: () => drawBeacon(frame, cam, level, state), z: level.beaconZ });
  items.sort((a, b) => b.z - a.z);
  // Skip anything essentially at or behind the courier — it would project huge and
  // overlap the player (the shard/plate is collected as you reach it anyway).
  const nearCut = state.runner.dist + 22;
  for (const item of items) {
    if (item.z > nearCut) {
      item.draw();
    }
  }
};

/** A subtle whole-screen storm tint when the storm is closing in. */
const drawStormTint = (frame: Frame, intensity: number): void => {
  if (intensity > 0.02) {
    box(frame, 0, 0, WIDTH, HEIGHT, P.alpha(P.STORM, intensity * 0.16), L_TINT);
  }
};

/** Render the whole world for `state` (backdrop → storm → path → entities → tint). */
export const renderWorld = (frame: Frame, state: State): void => {
  const cam = makeCamera(state);
  drawBackdrop(frame);
  drawMountains(frame, state.level);
  drawStorm(frame, state.storm.intensity);
  drawPath(frame, cam, state.level, state.runner.dist);
  drawWorldObjects(frame, cam, state);
  drawStormTint(frame, state.storm.intensity);
};
