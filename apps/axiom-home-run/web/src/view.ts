/*
 * view.ts — the PURE presentation of the game. This is the file that used to be
 * `scene.ts`, the one place that imperatively spawned and re-posed engine nodes.
 * It is now a pure function: `sceneOf(view, nowMs)` reads the session's read-only
 * `SceneView` (+ wall-clock ms for the sun) and RETURNS a `Scene` value — the whole
 * toy stadium described from scratch every frame as keyed instances. It calls no
 * engine function, holds no handle, and mutates nothing; `@axiom/web-engine`'s
 * reconciler turns the returned data into the minimal spawn/re-pose/despawn ops.
 *
 * Because it is immediate-mode data, hidden actors (the ball between pitches, an
 * unspawned trail dot) are simply NOT EMITTED — the old off-screen "parking" trick
 * is gone. Static geometry is emitted every frame too; its transforms never change,
 * so the reconciler spawns it once and never touches it again.
 *
 * Mesh conventions (unchanged): `box` is a UNIT CUBE (scale = full extents);
 * `sphere` is UNIT DIAMETER (scale = 2·radius); `cylinder` is UNIT (radius 0.5,
 * height 1, Y axis — scale = (diameter, height, diameter)).
 */

import type { Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import { type Quat, type Vec3, IDENTITY_QUAT, clamp, clamp01, hash01, mix, quatFromEulerXyz, vec3 } from "./vec.ts";
import { batDir, batPlaneY } from "./swing.ts";
import type { RunnerView, SceneView, SwingState } from "./types.ts";
import { TAG_FACEMASK, TAG_HELMET, TAG_JERSEY, TAG_PANTS, TAG_SHOES, TAG_SKIN, TAG_TRIM, bodyTransform, posedParts } from "./figure.ts";
import { type JointPose, battingPose, idlePose, reachArmsTo, runningPose } from "./figure-pose.ts";
import * as C from "./constants.ts";

// ── instance builders (pure data, no engine) ────────────────────────────────────

const MIN_EXTENT = 0.01;
const boxScale = (s: Vec3): Vec3 => vec3(Math.max(s.x, MIN_EXTENT), Math.max(s.y, MIN_EXTENT), Math.max(s.z, MIN_EXTENT));
const sphereScale = (r: number): Vec3 => vec3(r * 2, r * 2, r * 2);

const mk = (key: string, mesh: string, material: string, position: Vec3, scale: Vec3, rotation: Quat): SceneInstance => ({
  key,
  material,
  mesh,
  transform: { position, rotation, scale },
});
const box = (key: string, mat: string, pos: Vec3, scale: Vec3, rot: Quat = IDENTITY_QUAT): SceneInstance =>
  mk(key, "box", mat, pos, boxScale(scale), rot);
const cyl = (key: string, mat: string, pos: Vec3, scale: Vec3, rot: Quat = IDENTITY_QUAT): SceneInstance =>
  mk(key, "cylinder", mat, pos, scale, rot);
const orb = (key: string, mat: string, pos: Vec3, radius: number): SceneInstance =>
  mk(key, "sphere", mat, pos, sphereScale(radius), IDENTITY_QUAT);

// ── rigged player figure (ported end-zone skeleton) ─────────────────────────────

/** A team kit: which material renders each figure part tag. */
type Palette = Readonly<Record<number, string>>;

/** The home (blue) kit — the batter and its base runners. */
const HOME_PALETTE: Palette = {
  [TAG_FACEMASK]: "FigMask",
  [TAG_HELMET]: "HomeHelmet",
  [TAG_JERSEY]: "HomeJersey",
  [TAG_PANTS]: "HomePants",
  [TAG_SHOES]: "FigShoe",
  [TAG_SKIN]: "FigSkin",
  [TAG_TRIM]: "HomeTrim",
};

/** The away (red/white) kit — the fielders. */
const AWAY_PALETTE: Palette = {
  [TAG_FACEMASK]: "FigMask",
  [TAG_HELMET]: "AwayHelmet",
  [TAG_JERSEY]: "AwayJersey",
  [TAG_PANTS]: "AwayPants",
  [TAG_SHOES]: "FigShoe",
  [TAG_SKIN]: "FigSkin",
  [TAG_TRIM]: "AwayTrim",
};

/** Emit one rigged figure as 17 keyed boxes: resolve the pose to world part boxes
 * under a body transform at `ground`/`facing`, styled by the team `palette`. */
const emitFigure = (out: SceneInstance[], keyPrefix: string, pose: JointPose, ground: Vec3, facing: number, palette: Palette): void => {
  const body = bodyTransform(ground, facing, pose, 0);
  for (const [i, part] of posedParts(pose, body).entries()) {
    out.push({ key: `${keyPrefix}/${i}`, material: palette[part.tag] ?? "FigSkin", mesh: part.mesh, transform: part.transform });
  }
};

const YAW_POS = quatFromEulerXyz(0, Math.PI / 4, 0);
const YAW_NEG = quatFromEulerXyz(0, -Math.PI / 4, 0);

// ── the sun (pure wall-clock presentation) ──────────────────────────────────────

const SUN_LAP_MS = 40 * 60 * 1000;
const SUN_ELEV_LOW = 0.14;
const SUN_ELEV_HIGH = 0.42;
const SUN_GROUND = 0.28;
const SUN_GLARE_MAX = 1.5;
export const SUN_NOON_MS = SUN_LAP_MS / 2;
export const SUN_START_MS = SUN_LAP_MS * 0.3;
const SHADOW_STRETCH_MAX = 1.5;

interface SunState {
  readonly light: SceneLight;
  /** Unit XZ direction the projected shadows run (away from the sun). */
  readonly dx: number;
  readonly dz: number;
  /** Shadow length per unit of caster height (cot of elevation, capped). */
  readonly stretch: number;
}

/** Compute the sun for wall-clock `timeMs`: its directional light plus the shadow
 * projection the ground ellipses use — the pure port of the old `applySun`. */
const computeSun = (timeMs: number): SunState => {
  const azimuth = ((timeMs % SUN_LAP_MS) / SUN_LAP_MS) * Math.PI * 2;
  const height = 0.5 - 0.5 * Math.cos(azimuth);
  const elev = mix(SUN_ELEV_LOW, SUN_ELEV_HIGH, height);
  const sunX = Math.cos(elev) * Math.sin(azimuth);
  const sunY = Math.sin(elev);
  const sunZ = Math.cos(elev) * Math.cos(azimuth);
  const horiz = Math.hypot(sunX, sunZ);
  const glow = Math.sqrt(height);
  return {
    dx: -sunX / horiz,
    dz: -sunZ / horiz,
    light: {
      key: "sun",
      light: {
        color: [1, mix(0.62, 0.82, glow), mix(0.34, 0.6, glow), 1],
        direction: vec3(-sunX, -sunY, -sunZ),
        intensity: Math.min(SUN_GROUND / Math.sin(elev), SUN_GLARE_MAX),
        kind: "directional",
      },
    },
    stretch: Math.min(horiz / sunY, SHADOW_STRETCH_MAX),
  };
};

const FILL_LIGHT: SceneLight = {
  key: "fill",
  light: { color: [0.72, 0.8, 1, 1], direction: vec3(0.45, -0.5, -0.4), intensity: 0.65, kind: "directional" },
};

/** Ground height under XZ (infield dirt sits higher than the striped grass). */
const groundYAt = (x: number, z: number): number => {
  const onInfieldDirt = Math.abs(x) + Math.abs(z - 7.5) <= 7.6;
  const onHomeCircle = Math.hypot(x, z) <= 2.7;
  return onHomeCircle ? 0.1 : onInfieldDirt ? 0.066 : 0.03;
};

/** A caster's projected sun-shadow: a flat translucent ellipse at the feet
 * `(x, z)`, running along the sun's shadow direction, `height·stretch` long. */
const castShadow = (key: string, sun: SunState, x: number, z: number, height: number, width: number, lift: number): SceneInstance => {
  const len = Math.max(height * sun.stretch, width * 1.2);
  const cx = x + sun.dx * (len / 2 - width * 0.25);
  const cz = z + sun.dz * (len / 2 - width * 0.25);
  const yaw = quatFromEulerXyz(0, Math.atan2(sun.dx, sun.dz), 0);
  return cyl(key, "shadow", vec3(cx, groundYAt(cx, cz) + 0.012 + lift, cz), vec3(width, 0.01, len), yaw);
};

// ── static field + stadium (constant every frame) ───────────────────────────────

const buildGround = (out: SceneInstance[]): void => {
  out.push(box("g/ground", "GroundGreen", vec3(0, -0.07, 14), vec3(76, 0.1, 64)));
  out.push(box("g/deck", "DeckBrown", vec3(0, -0.005, -2), vec3(46, 0.06, 15)));
  for (const side of [1, -1]) {
    out.push(box(`g/seam/${side}`, "DirtLight", vec3(side * 2.6, 0.028, -1.8), vec3(0.35, 0.02, 8)));
  }
  for (let k = 0; k < 14; k += 1) {
    const zc = 1.2 + k * 2.4;
    const halfW = Math.min(zc + 1.2, C.WALL_LINE - zc + 1.2);
    if (halfW <= 0.4) {
      continue;
    }
    out.push(box(`g/grass/${k}`, k % 2 === 0 ? "GrassLight" : "GrassDark", vec3(0, 0.002, zc), vec3(halfW * 2, 0.03, 2.4)));
  }
  out.push(box("g/idirt", "Dirt", vec3(0, 0.03, 7.5), vec3(10.6, 0.05, 10.6), YAW_POS));
  out.push(box("g/igrass", "GrassLight", vec3(0, 0.045, 7.5), vec3(8, 0.04, 8), YAW_POS));
  for (let k = 0; k < 4; k += 1) {
    const zc = 3.3 + k * 3.2;
    const halfW = 5.66 - Math.abs(zc - 7.5) - 0.35;
    if (halfW <= 0.3) {
      continue;
    }
    out.push(box(`g/idiamond/${k}`, "GrassDark", vec3(0, 0.068, zc), vec3(halfW * 2, 0.012, 1.6)));
  }
  out.push(cyl("g/mound", "DirtLight", vec3(C.MOUND.x, 0.075, C.MOUND.z), vec3(3.6, 0.14, 3.6)));
  out.push(cyl("g/homecircle", "Dirt", vec3(0, 0.045, 0), vec3(5.4, 0.09, 5.4)));
  out.push(box("g/plate", "BaseWhite", vec3(0, 0.13, 0), vec3(0.5, 0.02, 0.5), YAW_POS));
  for (const side of [1, -1]) {
    const s = side > 0 ? "p" : "n";
    out.push(box(`g/box/${s}/0`, "Line", vec3(side * 0.5, 0.125, 0), vec3(0.14, 0.012, 1.33)));
    out.push(box(`g/box/${s}/1`, "Line", vec3(side * 1.0, 0.125, 0.6), vec3(1.14, 0.012, 0.14)));
    out.push(box(`g/box/${s}/2`, "Line", vec3(side * 1.0, 0.125, -0.6), vec3(1.14, 0.012, 0.14)));
    out.push(box(`g/box/${s}/3`, "Line", vec3(side * 1.5, 0.125, 0), vec3(0.14, 0.012, 1.33)));
  }
  const b = C.BASE_CORNER;
  [
    [-b, b],
    [0, 2 * b],
    [b, b],
  ].forEach(([bx, bz], i) => out.push(box(`g/base/${i}`, "BaseWhite", vec3(bx!, 0.12, bz!), vec3(0.6, 0.14, 0.6), YAW_POS)));
  out.push(box("g/foul/p", "Line", vec3(8.5, 0.105, 8.5), vec3(24, 0.012, 0.32), YAW_NEG));
  out.push(box("g/foul/n", "Line", vec3(-8.5, 0.105, 8.5), vec3(24, 0.012, 0.32), YAW_POS));
  out.push(box("g/track/p", "DirtLight", vec3(7.86, 0.028, 24.86), vec3(24.5, 0.02, 1.7), YAW_POS));
  out.push(box("g/track/n", "DirtLight", vec3(-7.86, 0.028, 24.86), vec3(24.5, 0.02, 1.7), YAW_NEG));
};

const buildStadium = (out: SceneInstance[]): void => {
  out.push(box("s/bowl/c", "SkyBowl", vec3(0, 16, 52), vec3(150, 34, 1.5)));
  for (const side of [1, -1]) {
    const s = side > 0 ? "p" : "n";
    out.push(box(`s/bowl/${s}`, "SkyBowl", vec3(side * 42, 16, 18), vec3(1.5, 34, 80)));
    const yaw = side > 0 ? YAW_POS : YAW_NEG;
    const cx = side * 8.9;
    out.push(box(`s/wall/${s}`, "WallBlue", vec3(cx, C.WALL_HEIGHT / 2, 25.9), vec3(25.8, C.WALL_HEIGHT, 0.9), yaw));
    out.push(box(`s/trim/${s}`, "WallTrim", vec3(cx, C.WALL_HEIGHT + 0.12, 25.9), vec3(25.8, 0.26, 1.04), yaw));
    for (let k = 0; k < 4; k += 1) {
      const off = 1.4 + k * 1.55;
      out.push(box(`s/seat/${s}/${k}`, k % 2 === 0 ? "SeatBlue" : "SeatBlueDark", vec3(cx + side * off * 0.707, 1.3 + k * 0.85, 25.9 + off * 0.707), vec3(27.5 + k * 1.4, 1.7, 1.6), yaw));
    }
    out.push(box(`s/fence/${s}`, "WallBlue", vec3(side * 17.6, 1.1, 5), vec3(0.9, 2.2, 25)));
    out.push(box(`s/fencetrim/${s}`, "WallTrim", vec3(side * 17.6, 2.32, 5), vec3(1.04, 0.24, 25)));
    for (let k = 0; k < 3; k += 1) {
      out.push(box(`s/sideseat/${s}/${k}`, k % 2 === 0 ? "SeatBlue" : "SeatBlueDark", vec3(side * (19 + k * 1.5), 0.95 + k * 0.8, 5), vec3(1.6, 1.5, 25)));
    }
    out.push(box(`s/corner/${s}`, "CornerBlue", vec3(side * 14.2, 1.2, -5.2), vec3(6.5, 2.6, 6)));
    out.push(box(`s/cornercap/${s}`, "SeatBlueDark", vec3(side * 15.4, 2.9, -5.6), vec3(4.5, 1.2, 5)));
  }
};

const buildScorePanels = (out: SceneInstance[]): void => {
  out.push(box("sp/panelL", "PanelNavy", vec3(4.7, 0.045, -2.7), vec3(3.4, 0.08, 2.1)));
  out.push(box("sp/panelLbar", "Line", vec3(4.7, 0.05, -1.78), vec3(3.4, 0.09, 0.18)));
  for (let k = 0; k < 2; k += 1) {
    out.push(box(`sp/digit/${k}`, "digit", vec3(5.25 - k * 1.15, 0.1, -2.85), vec3(0.62, 0.02, 1.05)));
  }
  out.push(box("sp/panelR", "PanelNavy", vec3(-4.7, 0.045, -2.7), vec3(3.4, 0.08, 2.1)));
  const dotRows: readonly (readonly [string, number])[] = [
    ["DotBlue", -2.15],
    ["DotYellow", -2.7],
    ["DotRed", -3.25],
  ];
  dotRows.forEach(([mat, rz], row) => {
    out.push(box(`sp/dotbar/${row}`, "Line", vec3(-3.6, 0.09, rz), vec3(0.3, 0.02, 0.3)));
    for (let k = 0; k < 3; k += 1) {
      out.push(orb(`sp/dot/${row}/${k}`, mat, vec3(-4.35 - k * 0.62, 0.11, rz), 0.13));
    }
  });
};

const buildPatrolCircles = (out: SceneInstance[]): void => {
  C.FIELDER_SPOTS.forEach((spot, i) => {
    const infield = spot.z < 13.5;
    const d = spot.radius * 1.9;
    out.push(cyl(`pc/${i}`, infield ? "PatrolDirt" : "PatrolGreen", vec3(spot.x, infield ? 0.062 : 0.026, spot.z), vec3(d, 0.015, d)));
  });
};

// ── dynamic actors (re-posed / shown from the SceneView) ─────────────────────────

/** The bat's stepped taper: [innerR, outerR, width] per segment. */
const BAT_SEGMENTS: readonly (readonly [number, number, number])[] = [
  [C.BAT_GRIP_R, C.BAT_BARREL_R, C.BAT_HANDLE_W],
  [C.BAT_BARREL_R, (C.BAT_BARREL_R + C.BAT_TIP_R) / 2, C.BAT_BARREL_W],
  [(C.BAT_BARREL_R + C.BAT_TIP_R) / 2, C.BAT_TIP_R, C.BAT_TIP_W],
];

const batTilt = (state: SwingState, readiness: number): number => {
  if (state === "swing" || state === "follow") {
    return 0.1;
  }
  return mix(0.1, 0.68, readiness);
};

/** The bat lying on the ground by home once the batter has dropped it and run. */
const buildDroppedBat = (out: SceneInstance[]): void => {
  const px = C.BATTER_START_X - 0.25;
  const pz = C.BATTER_Z - 0.55;
  const py = 0.13;
  const theta = 0.5; // a fixed sprawl angle; the bat lies flat on the dirt
  const d = batDir(theta);
  const rot = quatFromEulerXyz(0, theta + Math.PI / 2, 0);
  BAT_SEGMENTS.forEach(([r0, r1, w], i) => {
    const rc = (r0 + r1) / 2;
    out.push(box(`dropbat/${i}`, "bat", vec3(px + d.x * rc, py, pz + d.z * rc), vec3(r1 - r0 + 0.02, w, w), rot));
  });
  out.push(box("dropbat/knob", "BatKnob", vec3(px + d.x * C.BAT_GRIP_R, py, pz + d.z * C.BAT_GRIP_R), vec3(0.15, 0.15, 0.15), rot));
};

const buildBatter = (out: SceneInstance[], sun: SunState, view: SceneView): void => {
  // Once the batter has put the ball in play he IS the lead runner — hide the
  // plate figure and its held bat, and leave the bat lying by home (he let go).
  if (view.batterRunning) {
    buildDroppedBat(out);
    return;
  }
  const bx = view.batterX;
  const bz = C.BATTER_Z;
  const s = view.swing;
  // Coil while wound (readiness), unwind through the swing (twist toward square).
  const coil = s.state === "swing" || s.state === "follow" ? 0 : s.readiness;
  const twist = clamp01(1 - Math.abs(s.theta - C.THETA_SWEET) / 2.4);

  out.push(castShadow("batter/shadow", sun, bx, bz, 1.2, 0.8, 0.004));

  // The bat: same oversized toy geometry as before, swung from the batter's hands.
  const tilt = batTilt(s.state, s.readiness);
  const d = batDir(s.theta);
  const pivotY = batPlaneY(s.theta) + 0.05;
  const reach = Math.cos(tilt);
  const rot = quatFromEulerXyz(0, s.theta + Math.PI / 2, tilt);

  // The batter grips the bat: solve both hands to a point just up the handle so
  // the arms hold — and swing — the bat (they track it as θ sweeps).
  const gripR = C.BAT_GRIP_R + 0.08;
  const grip = vec3(bx + d.x * gripR * reach, pivotY + Math.sin(tilt) * gripR, bz + d.z * gripR * reach);
  const pose = reachArmsTo(battingPose(coil, twist), vec3(bx, 0, bz), C.BATTER_FACING, grip);
  emitFigure(out, "batter", pose, vec3(bx, 0, bz), C.BATTER_FACING, HOME_PALETTE);
  BAT_SEGMENTS.forEach(([r0, r1, w], i) => {
    const rc = (r0 + r1) / 2;
    const center = vec3(bx + d.x * rc * reach, pivotY + Math.sin(tilt) * rc, bz + d.z * rc * reach);
    out.push(box(`bat/${i}`, "bat", center, vec3(r1 - r0 + 0.02, w, w), rot));
  });
  out.push(box("bat/knob", "BatKnob", vec3(bx + d.x * C.BAT_GRIP_R, pivotY, bz + d.z * C.BAT_GRIP_R), vec3(0.15, 0.15, 0.15), rot));
};

const buildMachine = (out: SceneInstance[], sun: SunState, view: SceneView): void => {
  const mz = C.MOUND.z;
  out.push(castShadow("machine/shadow", sun, 0, mz, 1.35, 1.15, 0.002));
  // Static machine chassis (constant).
  out.push(box("machine/base", "MachineDark", vec3(0, 0.28, mz), vec3(1.15, 0.26, 0.9)));
  for (const side of [1, -1]) {
    out.push(cyl(`machine/wheel/${side}`, "MachineDark", vec3(side * 0.62, 0.3, mz), vec3(0.4, 0.14, 0.4), quatFromEulerXyz(0, 0, Math.PI / 2)));
  }
  out.push(cyl("machine/muzzle", "BaseWhite", vec3(0, 1.16, mz - 0.88), vec3(0.3, 0.06, 0.3), quatFromEulerXyz(Math.PI / 2, 0, 0)));
  // Animated parts.
  const squash = 1 - 0.2 * view.windup;
  const recoil = 0.26 * view.windup - 0.34 * view.muzzleFlash;
  out.push(box("machine/body", "MachineOrange", vec3(0, 0.41 + 0.62 * squash * 0.5, mz), vec3(0.9 * (1 + 0.12 * view.windup), 0.62 * squash, 0.78)));
  out.push(cyl("machine/barrel", "MachineDark", vec3(0, 1.16, mz - 0.35 + recoil), vec3(0.26, 1.1, 0.26), quatFromEulerXyz(Math.PI / 2, 0, 0)));
  out.push(box("machine/hopper", "MachineOrange", vec3(0, 0.98 + 0.16 * squash, mz + 0.42), vec3(0.56, 0.34, 0.46)));
  const blink = view.windup > 0.82 ? 0.12 + 0.07 * Math.sin(view.tick * 0.9) : 0;
  const flash = Math.max(view.muzzleFlash * 0.4, blink);
  if (flash > 0.01) {
    out.push(orb("machine/flash", "flash", vec3(0, 1.16, mz - 0.95), flash));
  }
};

/** The ball's squash/stretch pulse — a brief, bounded visual flourish for the
 * home-run cinematic's contact beat (view.impactFlash already decays every real
 * tick regardless of slow motion, so this fades on its own without new state). */
const ballSquashScale = (view: SceneView): Vec3 => {
  const radius = C.BALL_RADIUS * 1.15;
  const squash = view.cinematicPhase === "contact" ? view.impactFlash : 0;
  return vec3(radius * 2 * (1 + 0.5 * squash), radius * 2 * (1 - 0.35 * squash), radius * 2 * (1 + 0.5 * squash));
};

const buildBall = (out: SceneInstance[], view: SceneView): void => {
  if (view.ballVisible) {
    out.push(mk("ball", "sphere", "BallWhite", view.ball, ballSquashScale(view), IDENTITY_QUAT));
    const gy = groundYAt(view.ball.x, view.ball.z);
    const s = 0.36 * (1 - clamp01(view.ball.y / 14) * 0.6);
    out.push(cyl("ball/shadow", "shadow", vec3(view.ball.x, gy + 0.006, view.ball.z), vec3(s, 0.01, s)));
  }
  // A restrained "speed line" treatment: the existing bounded trail widens
  // during the cinematic's high-velocity beats (contact → ball-follow), then
  // returns to its ordinary width — no new geometry, just a wider read on the
  // SAME fixed-size trail array every other hit already uses.
  const cinematicTrail = view.cinematicPhase === "contact" || view.cinematicPhase === "ballFollow";
  const trailWidth = cinematicTrail ? 1.5 : 1;
  const n = view.trail.length;
  for (let i = 0; i < 14; i += 1) {
    const idx = n - 14 + i;
    const p = idx >= 0 ? view.trail[idx] : undefined;
    if (view.ballInPlay && p !== undefined) {
      out.push(orb(`trail/${i}`, "trail", p, (0.04 + 0.09 * ((i + 1) / 14)) * trailWidth));
    }
  }
  const f = view.impactFlash;
  const anchor = view.trail.length > 0 ? view.trail[0]! : view.ball;
  if (f > 0.02 && view.ballInPlay) {
    out.push(orb("impact", "impact", anchor, 0.2 + (1 - f) * 0.9));
  }
};

/** A fielder is walking (not standing) above this speed (u/s). */
const FIELDER_WALK_SPEED = 0.6;
/** How far a standing fielder's head may twist to watch (rad) — a human neck, not
 * an owl; a target further round leaves the head at this cap. */
const HEAD_YAW_MAX = 1.4;

/** A cheap, per-player-desynced idle "breath" (−1…1): one sine whose frequency and
 * phase are seeded off `slot`, so no two standing players idle in lockstep. Runs on
 * the gated gameplay `tick` (so it eases with the rest of the scene in slow-mo). */
const idleBreath = (tick: number, slot: number): number =>
  Math.sin(tick * mix(0.045, 0.08, hash01(slot, 1)) + hash01(slot, 2) * Math.PI * 2);

const buildFielders = (out: SceneInstance[], sun: SunState, view: SceneView): void => {
  const batter = vec3(view.batterX, 0, C.BATTER_Z);
  view.fielders.forEach((f, i) => {
    const ground = vec3(f.x, 0, f.z);
    const walking = f.speed > FIELDER_WALK_SPEED;
    let facing: number;
    let pose;
    if (walking) {
      // Running to chase (or jogging back) — the procedural IK gait, no floating.
      facing = f.facing;
      pose = runningPose(ground, f.facing, f.speed, f.traveled);
    } else {
      // Standing squared up to home; ONLY the head twists to watch the batter or
      // the ball — whichever is closer — so the body never pivots to look.
      const dBatter = Math.hypot(batter.x - f.x, batter.z - f.z);
      const dBall = view.ballVisible ? Math.hypot(view.ball.x - f.x, view.ball.z - f.z) : Infinity;
      const watch = dBall < dBatter ? view.ball : batter;
      facing = Math.atan2(-f.x, -f.z); // body faces home plate
      const toWatch = Math.atan2(watch.x - f.x, watch.z - f.z);
      const rel = Math.atan2(Math.sin(toWatch - facing), Math.cos(toWatch - facing));
      pose = idlePose(0, clamp(rel, -HEAD_YAW_MAX, HEAD_YAW_MAX), idleBreath(view.tick, i));
    }
    out.push(castShadow(`fielder/${i}/shadow`, sun, f.x, f.z, 1.15, 0.66, 0.006 + i * 0.002));
    emitFigure(out, `fielder/${i}`, pose, ground, facing, AWAY_PALETTE);
  });
};

/** The ball a fielder has thrown to a bag during a defensive play — a bright
 * sphere with a ground shadow, so the relay to the base reads clearly. */
const buildThrow = (out: SceneInstance[], sun: SunState, view: SceneView): void => {
  if (!view.throwBall.visible) {
    return;
  }
  const p = view.throwBall.pos;
  out.push(orb("throwball", "BallWhite", p, C.BALL_RADIUS * 1.15));
  out.push(castShadow("throwball/shadow", sun, p.x, p.z, 0.3, 0.3, 0.02));
};

/** Draw each base runner as the rigged figure: the running gait while advancing,
 * a standing (gently breathing) idle while resting on a base. */
const buildRunners = (out: SceneInstance[], sun: SunState, runners: readonly RunnerView[], tick: number): void => {
  runners.forEach((r, i) => {
    const ground = vec3(r.x, 0, r.z);
    // Offset the breath slot so a resting runner never idles in sync with a fielder.
    const pose = r.moving ? runningPose(ground, r.facing, C.RUNNER_SPEED, r.traveled) : idlePose(0, 0, idleBreath(tick, i + 50));
    out.push(castShadow(`runner/${i}/shadow`, sun, r.x, r.z, 1.15, 0.66, 0.008 + i * 0.002));
    emitFigure(out, `runner/${i}`, pose, ground, r.facing, HOME_PALETTE);
  });
};

/** The whole frame as pure data: the toy stadium arranged for this `view`, lit by
 * the wall-clock sun. No engine call, no handle, no mutation. */
export const sceneOf = (view: SceneView, nowMs: number): Scene => {
  const sun = computeSun(nowMs);
  const instances: SceneInstance[] = [];
  buildGround(instances);
  buildStadium(instances);
  buildScorePanels(instances);
  buildPatrolCircles(instances);
  buildBatter(instances, sun, view);
  buildMachine(instances, sun, view);
  buildBall(instances, view);
  buildFielders(instances, sun, view);
  buildRunners(instances, sun, view.runners, view.tick);
  buildThrow(instances, sun, view);
  return {
    camera: { far: C.CAMERA_FAR, fovY: view.cameraFovY, near: C.CAMERA_NEAR, position: view.cameraPos, target: view.cameraTarget },
    clearColor: [0.62, 0.72, 0.95, 1],
    instances,
    lights: [sun.light, FILL_LIGHT],
  };
};
