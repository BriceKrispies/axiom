/*
 * THE GAME — a DOOM-style first-person shooter authored ENTIRELY in TypeScript on
 * @axiom/game. There is no Rust in this app: the level, the first-person camera,
 * the tank movement with wall collision, the chasing enemies, the hitscan shooting,
 * and the health/ammo/score loop are all the code below, driving the engine through
 * the SDK's scene-authoring (`createMesh`/`spawnRenderable`/`setNodeTransform`/…),
 * spatial-query (`raycast`), input (`bindAction` + `sim.input`), and world
 * (`sim.world.despawn`) surfaces. The engine renders the retained 3D scene; this
 * file only describes and drives it.
 *
 * Edit a tunable below, hit save, and the browser re-runs deterministically from
 * tick 0 with your change applied (Mode-B hot reload).
 */

import {
  type Sim,
  addLight,
  bindAction,
  clearScene,
  controlFirstPerson,
  createController,
  createMaterial,
  createMesh,
  onFixedUpdate,
  raycast,
  setNodeBounds,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/game";
import type { Entity, Rgba, Transform } from "@axiom/game";

// ───────────────────────── tweak these and save ─────────────────────────
const MOVE_SPEED = 0.06; // forward/back/strafe units per tick
const TURN_SPEED = 0.045; // keyboard turn radians per tick
const ENEMY_SPEED = 0.025; // enemy chase units per tick (matches the Rust DOOM)
const FIRE_RANGE = 14; // hitscan reach in world units
const FIRE_COOLDOWN = 10; // ticks between shots
const CONTACT_RADIUS = 0.8; // how close an enemy must be to bite
const CONTACT_DAMAGE = 4; // HP lost per bite
const HURT_COOLDOWN = 12; // ticks between bites
const MAX_HEALTH = 200;
const START_AMMO = 50;
const AMMO_PER_KILL = 5;
const KILL_SCORE = 100;
const FOV_Y_DEG = 70; // vertical field of view
const MOUSE_SENSITIVITY = 0.0025; // radians of look per pixel of mouse movement
// (Pitch is clamped by the engine's controller, ~±1.5 rad.)
// ─────────────────────────────────────────────────────────────────────────

const EYE_HEIGHT = 1; // camera height off the floor
const ENEMY_Y = 0.5; // enemy cube centre height
const WALL_HEIGHT = 2;
const DEG_TO_RAD = Math.PI / 180;

// The level: '#' wall, '.' floor, 'S' player start, 'E' enemy spawn. Two rooms
// split by a vertical wall with a doorway, 18 columns × 10 rows — the same shape
// the Rust DOOM app's level.axiom describes.
const LEVEL: readonly string[] = [
  "##################",
  "#.......E#.......#",
  "#........#..E....#",
  "#..E.....#.......#",
  "#........#.......#",
  "#................#",
  "#........#.......#",
  "#S.E.....#.......#",
  "#........#.......#",
  "##################",
];
const COLS = LEVEL[0]!.length;
const ROWS = LEVEL.length;

// Linear-RGB palette (the engine's lit-colour materials).
const WALL_A: Rgba = [0.55, 0.27, 0.24, 1];
const WALL_B: Rgba = [0.24, 0.3, 0.5, 1];
const FLOOR_COLOR: Rgba = [0.13, 0.13, 0.15, 1];
const CEILING_COLOR: Rgba = [0.09, 0.08, 0.07, 1];
const ENEMY_COLOR: Rgba = [0.95, 0.35, 0.28, 1];

/** A node we can move and shoot: its scene entity, whether it is alive, and its world XZ. */
interface Enemy {
  node: Entity;
  alive: boolean;
  x: number;
  z: number;
  readonly homeCol: number;
  readonly homeRow: number;
}

/** All mutable game state — reset on the player's death and on a hot-reload re-run. */
interface State {
  px: number;
  pz: number;
  yaw: number;
  health: number;
  ammo: number;
  score: number;
  fireTimer: number;
  hurtTimer: number;
  enemies: Enemy[];
}

const cellAt = (col: number, row: number): string =>
  (row >= 0 && row < ROWS && col >= 0 && col < COLS && LEVEL[row]![col]) || "#";

const isWall = (col: number, row: number): boolean => cellAt(col, row) === "#";

/** An identity-rotation transform at `(x, y, z)` scaled `(sx, sy, sz)`. */
const xform = (x: number, y: number, z: number, sx: number, sy: number, sz: number): Transform => ({
  position: { x, y, z },
  rotation: [0, 0, 0, 1],
  scale: { x: sx, y: sy, z: sz },
});

/** The cells flagged with a given level character, as `{ col, row }`. */
const cellsOf = (mark: string): { col: number; row: number }[] => {
  const found: { col: number; row: number }[] = [];
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLS; col += 1) {
      if (cellAt(col, row) === mark) {
        found.push({ col, row });
      }
    }
  }
  return found;
};

// A single cube mesh + the materials, registered once and reused for every
// instance (walls, floor, ceiling, enemies are all scaled cubes — exactly how the
// Rust DOOM renders). Filled by `registerAssets`.
let cube = 0;
let wallAMat = 0;
let wallBMat = 0;
let floorMat = 0;
let ceilMat = 0;
let enemyMat = 0;

/** Register the cube mesh + every lit-colour material, capturing their handles. */
const registerAssets = (): void => {
  cube = createMesh("box");
  wallAMat = createMaterial({ baseColor: WALL_A });
  wallBMat = createMaterial({ baseColor: WALL_B });
  floorMat = createMaterial({ baseColor: FLOOR_COLOR });
  ceilMat = createMaterial({ baseColor: CEILING_COLOR });
  enemyMat = createMaterial({ baseColor: ENEMY_COLOR });
};

/** The floor + ceiling slabs and every wall cube, bounded so they occlude hitscan. */
const buildLevel = (): void => {
  // Floor slab (top surface at y=0) and ceiling slab (bottom at y=WALL_HEIGHT).
  spawnRenderable(cube, floorMat, xform(COLS / 2, -0.5, ROWS / 2, COLS, 1, ROWS));
  spawnRenderable(cube, ceilMat, xform(COLS / 2, WALL_HEIGHT + 0.5, ROWS / 2, COLS, 1, ROWS));
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLS; col += 1) {
      if (isWall(col, row)) {
        const material = (col + row) % 2 === 0 ? wallAMat : wallBMat;
        const node = spawnRenderable(
          cube,
          material,
          xform(col + 0.5, WALL_HEIGHT / 2, row + 0.5, 1, WALL_HEIGHT, 1),
        );
        setNodeBounds(node, { x: 0.5, y: WALL_HEIGHT / 2, z: 0.5 });
      }
    }
  }
};

/** Spawn one enemy cube at its home cell, bounded so a ray can hit it. */
const spawnEnemy = (homeCol: number, homeRow: number): Enemy => {
  const x = homeCol + 0.5;
  const z = homeRow + 0.5;
  const node = spawnRenderable(cube, enemyMat, xform(x, ENEMY_Y, z, 0.7, 0.9, 0.7));
  setNodeBounds(node, { x: 0.35, y: 0.45, z: 0.35 });
  return { alive: true, homeCol, homeRow, node, x, z };
};

/** (Re)spawn every enemy at its home cell, despawning any current ones first. */
const resetEnemies = (sim: Sim, state: State): void => {
  for (const enemy of state.enemies) {
    sim.world.despawn(enemy.node);
  }
  state.enemies = cellsOf("E").map((cell) => spawnEnemy(cell.col, cell.row));
};

/** Build the whole scene from scratch and return the fresh game state. */
const build = (sim: Sim): State => {
  clearScene();
  registerAssets();
  buildLevel();
  addLight({ color: [1, 0.97, 0.9, 1], direction: { x: 0.3, y: -1, z: 0.35 }, intensity: 1, kind: "directional" });
  const start = cellsOf("S")[0] ?? { col: 1, row: 1 };
  const px = start.col + 0.5;
  const pz = start.row + 0.5;
  // The first-person camera is the engine's CONTROLLER node: the engine yaws,
  // pitches, and moves it each frame from the FirstPersonInput we feed — we never
  // author a camera transform. It starts at the player's eye, facing -Z (yaw 0).
  createController({ far: 100, fovY: FOV_Y_DEG * DEG_TO_RAD, near: 0.05, position: { x: px, y: EYE_HEIGHT, z: pz } });
  const state: State = {
    ammo: START_AMMO,
    enemies: [],
    fireTimer: 0,
    health: MAX_HEALTH,
    hurtTimer: 0,
    px,
    pz,
    score: 0,
    yaw: 0,
  };
  resetEnemies(sim, state);
  return state;
};

/** Bind the tank-control action names to their physical keys (once, host-bound). */
const bindKeys = (): void => {
  bindAction("forward", ["KeyW", "ArrowUp"]);
  bindAction("back", ["KeyS", "ArrowDown"]);
  bindAction("turnLeft", ["ArrowLeft"]);
  bindAction("turnRight", ["ArrowRight"]);
  bindAction("strafeLeft", ["KeyA"]);
  bindAction("strafeRight", ["KeyD"]);
  // Space OR the left mouse button (reported as "Mouse0" while the pointer is locked).
  bindAction("fire", ["Space", "Mouse0"]);
};

/*
 * The unit forward and right vectors on the floor plane for `yaw`, in the
 * engine's controller convention: yaw is a rotation about world +Y, so forward is
 * `Ry(yaw)·(0,0,-1)` and right is `Ry(yaw)·(1,0,0)`. At yaw 0 forward is -Z and
 * right is +X; increasing yaw turns left. Matching the engine here means the yaw
 * we track stays exactly in lockstep with the controller's own accumulated yaw.
 */
const heading = (yaw: number): { fx: number; fz: number; rx: number; rz: number } => ({
  fx: -Math.sin(yaw),
  fz: -Math.cos(yaw),
  rx: Math.cos(yaw),
  rz: -Math.sin(yaw),
});

/** Slide `(x, z)` by `(dx, dz)` against the wall grid, one axis at a time. */
const slide = (x: number, z: number, dx: number, dz: number): { x: number; z: number } => {
  let nx = x;
  let nz = z;
  if (!isWall(Math.floor(x + dx), Math.floor(z))) {
    nx = x + dx;
  }
  if (!isWall(Math.floor(nx), Math.floor(z + dz))) {
    nz = z + dz;
  }
  return { x: nx, z: nz };
};

/*
 * Look, move, and drive the engine's first-person controller from this tick's
 * input — the same shape the Rust DOOM hands the engine. We compute the look
 * deltas (keyboard turn + mouse) and the collision-resolved WORLD move, then feed
 * the engine one `FirstPersonInput`: the yaw/pitch deltas and the move rotated
 * into the camera's own frame (`Ry(-yaw)·worldΔ`). The engine yaws/pitches/moves
 * the camera node itself — we never author the camera. We mirror only the yaw and
 * floor position the game needs for collision and hitscan.
 */
const driveController = (sim: Sim, state: State): void => {
  const look = sim.input.look();
  // yaw+ turns left; a turn-LEFT key or mouse-RIGHT both turn that way (the engine
  // convention). Pitch+ looks up; mouse-up (negative dy) looks up.
  const yawDelta =
    (Number(sim.input.isDown("turnLeft")) - Number(sim.input.isDown("turnRight"))) * TURN_SPEED -
    look.x * MOUSE_SENSITIVITY;
  const pitchDelta = -look.y * MOUSE_SENSITIVITY;
  state.yaw += yawDelta;

  const drive = Number(sim.input.isDown("forward")) - Number(sim.input.isDown("back"));
  const strafe = Number(sim.input.isDown("strafeRight")) - Number(sim.input.isDown("strafeLeft"));
  const { fx, fz, rx, rz } = heading(state.yaw);
  const wantX = (fx * drive + rx * strafe) * MOVE_SPEED;
  const wantZ = (fz * drive + rz * strafe) * MOVE_SPEED;
  // Resolve the move against walls in WORLD space, then take the actual delta.
  const moved = slide(state.px, state.pz, wantX, wantZ);
  const dx = moved.x - state.px;
  const dz = moved.z - state.pz;
  state.px = moved.x;
  state.pz = moved.z;

  // Hand the resolved world move to the engine in the camera's own frame
  // (`Ry(-yaw)·(dx,dz)`); the engine re-applies the yaw, landing the camera here.
  const c = Math.cos(state.yaw);
  const s = Math.sin(state.yaw);
  controlFirstPerson({
    moveLocal: { x: dx * c - dz * s, y: 0, z: dx * s + dz * c },
    pitchDelta,
    yawDelta,
  });
};

/** Fire a hitscan shot along the player's facing; kill the enemy it strikes first.
 * Hold-to-fire, paced by the cooldown — `isDown` rather than the press edge so a
 * held trigger keeps shooting at the fire rate. */
const fire = (sim: Sim, state: State): void => {
  const canFire = sim.input.isDown("fire") && state.ammo > 0 && state.fireTimer === 0;
  if (!canFire) {
    return;
  }
  state.ammo -= 1;
  state.fireTimer = FIRE_COOLDOWN;
  const { fx, fz } = heading(state.yaw);
  const hit = raycast({ x: state.px, y: ENEMY_Y, z: state.pz }, { x: fx, y: 0, z: fz }, FIRE_RANGE);
  if (hit === undefined) {
    return;
  }
  const enemy = state.enemies.find((candidate) => candidate.alive && candidate.node === hit.entity);
  if (enemy) {
    enemy.alive = false;
    sim.world.despawn(enemy.node);
    state.score += KILL_SCORE;
    state.ammo += AMMO_PER_KILL;
  }
};

/** Walk each living enemy toward the player and bite on contact. */
const stepEnemies = (sim: Sim, state: State): void => {
  for (const enemy of state.enemies) {
    if (!enemy.alive) {
      continue;
    }
    const toX = state.px - enemy.x;
    const toZ = state.pz - enemy.z;
    const dist = Math.hypot(toX, toZ);
    if (dist > 0.0001) {
      const stepX = (toX / dist) * ENEMY_SPEED;
      const stepZ = (toZ / dist) * ENEMY_SPEED;
      const moved = slide(enemy.x, enemy.z, stepX, stepZ);
      enemy.x = moved.x;
      enemy.z = moved.z;
      setNodeTransform(enemy.node, xform(enemy.x, ENEMY_Y, enemy.z, 0.7, 0.9, 0.7));
    }
    if (dist < CONTACT_RADIUS && state.hurtTimer === 0) {
      state.health -= CONTACT_DAMAGE;
      state.hurtTimer = HURT_COOLDOWN;
    }
  }
};

// The live game. It is undefined until the first fixed tick builds the scene
// (which needs the host channel, bound by `boot` before the first advance).
let state: State | undefined;

onFixedUpdate((sim: Sim): void => {
  if (state === undefined) {
    bindKeys();
    state = build(sim);
  }
  state.fireTimer = Math.max(state.fireTimer - 1, 0);
  state.hurtTimer = Math.max(state.hurtTimer - 1, 0);
  driveController(sim, state);
  fire(sim, state);
  stepEnemies(sim, state);
  if (state.health <= 0) {
    state.health = MAX_HEALTH;
    state.ammo = START_AMMO;
    resetEnemies(sim, state);
  }
});

/** The HUD the harness reads each frame to update the DOM read-out. */
export const readHud = (): { health: number; score: number; ammo: number; enemies: number } => ({
  ammo: state?.ammo ?? START_AMMO,
  enemies: state?.enemies.filter((enemy) => enemy.alive).length ?? 0,
  health: state?.health ?? MAX_HEALTH,
  score: state?.score ?? 0,
});
