/*
 * THE GAME — a retro FPS authored ENTIRELY in TypeScript on @axiom/game, now as a
 * `defineApp` manifest (hot-reload architecture §5). It exports:
 *   - `scene("arena", { version, build })` — the level geometry, lights, camera, and
 *     enemy spawns. Bump `SCENE_VERSION` and save to re-author the level LIVE (the hot
 *     runtime clears + rebuilds the scene in the running engine — no page reload).
 *   - `system("fps.sim", …)` — the per-tick sim (look/move/shoot/enemies). Edit a
 *     tunable below (e.g. `ENEMY_SPEED`) and save: the hot runtime swaps this system's
 *     body on the next tick WITHOUT recreating the engine — enemies keep their
 *     positions (durable game state is handed across the reload via `import.meta.hot.data`).
 *   - `component("Velocity", { version, migrate })` — a schema for the "wave marker"
 *     entity. Bump `COMPONENT_VERSION` (adding a `migrate`) and save: a `soft_app_reload`
 *     rewrites the marker's LIVE component bytes in place (the migration proof).
 */

import {
  type Sim,
  addLight,
  bindAction,
  component,
  controlFirstPerson,
  createController,
  createMaterial,
  createMesh,
  defineApp,
  raycast,
  scene,
  setNodeBounds,
  setNodeTransform,
  spawnRenderable,
  system,
} from "@axiom/game";
import type { Component, Entity, Rgba, Transform } from "@axiom/game";

/** The `Velocity` component value (the engine's `[x, y]` f32 layout) — the migration-proof datum. */
interface VelocityValue extends Component {
  readonly kind: "Velocity";
  readonly x: number;
  readonly y: number;
}

// ───────────────────────── tweak these and save ─────────────────────────
const MOVE_SPEED = 0.06; // forward/back/strafe units per tick
const TURN_SPEED = 0.045; // keyboard turn radians per tick
const ENEMY_SPEED = 0.025; // enemy chase units per tick — EDIT + save = live hot_patch
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

// Bump SCENE_VERSION + save to re-author the level live (scene reconcile proof).
const SCENE_VERSION = 1;
// Bump COMPONENT_VERSION + add a migrate to `waveComponent` to migrate live bytes.
const COMPONENT_VERSION = 1;
// ─────────────────────────────────────────────────────────────────────────

const EYE_HEIGHT = 1; // camera height off the floor
const ENEMY_Y = 0.5; // enemy cube centre height
const WALL_HEIGHT = 2;
const DEG_TO_RAD = Math.PI / 180;

// The level: '#' wall, '.' floor, 'S' player start, 'E' enemy spawn. Edit + bump
// SCENE_VERSION to reconcile the geometry live.
const LEVEL: readonly string[] = [
  "##################",
  "#.......E#.......#",
  "#........#..E....#",
  "#..E.....#.......#",
  "#........#.......#",
  "#........#.......#",
  "#........#.......#",
  "#S.E.....#.......#",
  "#........#.......#",
  "##################",
];
const COLS = LEVEL[0]!.length;
const ROWS = LEVEL.length;

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
}

/** All mutable game state — handed ACROSS a hot reload via `import.meta.hot.data` so a
 * system-body edit keeps player/enemy positions instead of resetting to spawn. */
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
  // The migration-proof marker entity carrying a `Velocity` component (undefined until
  // the sim's first tick spawns it), plus the last-read wave value for the proof.
  marker: Entity | undefined;
  wave: number;
}

// Durable state lives in `import.meta.hot.data` (present under Vite HMR) so it survives
// a `./game.ts` re-import; without HMR (static bundle) it is a plain module holder.
interface HotStore {
  state?: State;
}
const store: HotStore = (import.meta.hot?.data as HotStore | undefined) ?? {};
const persist = (state: State): State => {
  if (import.meta.hot) {
    (import.meta.hot.data as HotStore).state = state;
  }
  store.state = state;
  return state;
};

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

// A single cube mesh + materials, re-registered by `build` on each (re)author.
let cube = 0;
let enemyMat = 0;

/** Author the level geometry, lights, first-person camera, and enemies; return fresh state.
 * `clearScene` is run by the reconciler BEFORE this, so `build` only describes geometry. */
const build = (): void => {
  cube = createMesh("box");
  const wallAMat = createMaterial({ baseColor: WALL_A });
  const wallBMat = createMaterial({ baseColor: WALL_B });
  const floorMat = createMaterial({ baseColor: FLOOR_COLOR });
  const ceilMat = createMaterial({ baseColor: CEILING_COLOR });
  enemyMat = createMaterial({ baseColor: ENEMY_COLOR });

  spawnRenderable(cube, floorMat, xform(COLS / 2, -0.5, ROWS / 2, COLS, 1, ROWS));
  spawnRenderable(cube, ceilMat, xform(COLS / 2, WALL_HEIGHT + 0.5, ROWS / 2, COLS, 1, ROWS));
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLS; col += 1) {
      if (isWall(col, row)) {
        const material = (col + row) % 2 === 0 ? wallAMat : wallBMat;
        const node = spawnRenderable(cube, material, xform(col + 0.5, WALL_HEIGHT / 2, row + 0.5, 1, WALL_HEIGHT, 1));
        setNodeBounds(node, { x: 0.5, y: WALL_HEIGHT / 2, z: 0.5 });
      }
    }
  }
  addLight({ color: [1, 0.97, 0.9, 1], direction: { x: 0.3, y: -1, z: 0.35 }, intensity: 1, kind: "directional" });

  const start = cellsOf("S")[0] ?? { col: 1, row: 1 };
  const px = start.col + 0.5;
  const pz = start.row + 0.5;
  createController({ far: 100, fovY: FOV_Y_DEG * DEG_TO_RAD, near: 0.05, position: { x: px, y: EYE_HEIGHT, z: pz } });

  const enemies: Enemy[] = cellsOf("E").map((cell) => {
    const x = cell.col + 0.5;
    const z = cell.row + 0.5;
    const node = spawnRenderable(cube, enemyMat, xform(x, ENEMY_Y, z, 0.7, 0.9, 0.7));
    setNodeBounds(node, { x: 0.35, y: 0.45, z: 0.35 });
    return { alive: true, node, x, z };
  });
  bindAction("forward", ["KeyW", "ArrowUp"]);
  bindAction("back", ["KeyS", "ArrowDown"]);
  bindAction("turnLeft", ["ArrowLeft"]);
  bindAction("turnRight", ["ArrowRight"]);
  bindAction("strafeLeft", ["KeyA"]);
  bindAction("strafeRight", ["KeyD"]);
  bindAction("fire", ["Space", "Mouse0"]);
  persist({ ammo: START_AMMO, enemies, fireTimer: 0, health: MAX_HEALTH, hurtTimer: 0, marker: undefined, px, pz, score: 0, wave: 1, yaw: 0 });
};

/** The unit forward/right vectors on the floor plane for `yaw` (engine controller convention). */
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

/** Look/move + drive the engine's first-person controller from this tick's input. */
const driveController = (sim: Sim, state: State): void => {
  const look = sim.input.look();
  const yawDelta =
    (Number(sim.input.isDown("turnLeft")) - Number(sim.input.isDown("turnRight"))) * TURN_SPEED -
    look.x * MOUSE_SENSITIVITY;
  const pitchDelta = -look.y * MOUSE_SENSITIVITY;
  state.yaw += yawDelta;

  const drive = Number(sim.input.isDown("forward")) - Number(sim.input.isDown("back"));
  const strafe = Number(sim.input.isDown("strafeRight")) - Number(sim.input.isDown("strafeLeft"));
  const { fx, fz, rx, rz } = heading(state.yaw);
  const moved = slide(state.px, state.pz, (fx * drive + rx * strafe) * MOVE_SPEED, (fz * drive + rz * strafe) * MOVE_SPEED);
  const dx = moved.x - state.px;
  const dz = moved.z - state.pz;
  state.px = moved.x;
  state.pz = moved.z;

  const c = Math.cos(state.yaw);
  const s = Math.sin(state.yaw);
  controlFirstPerson({ moveLocal: { x: dx * c - dz * s, y: 0, z: dx * s + dz * c }, pitchDelta, yawDelta });
};

/** Fire a hitscan shot; kill the enemy it strikes first. */
const fire = (sim: Sim, state: State): void => {
  if (!(sim.input.isDown("fire") && state.ammo > 0 && state.fireTimer === 0)) {
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
      const moved = slide(enemy.x, enemy.z, (toX / dist) * ENEMY_SPEED, (toZ / dist) * ENEMY_SPEED);
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

/** Spawn the migration-proof marker (once): an entity carrying a `Velocity` whose `x` is the wave number. */
const ensureMarker = (sim: Sim, state: State): void => {
  if (state.marker === undefined) {
    const value: VelocityValue = { kind: "Velocity", x: state.wave, y: 0 };
    state.marker = sim.world.spawn(value);
  }
  // Mirror the marker's LIVE velocity.x into `wave` each tick — a migration doubles it,
  // and the proof reads `wave` to observe the transformed bytes.
  const read = sim.world.get(state.marker, "Velocity") as VelocityValue | undefined;
  state.wave = read === undefined ? state.wave : read.x;
};

const fpsSim = system("fps.sim", {
  phase: "fixedUpdate",
  run: (sim: Sim): void => {
    const state = store.state;
    if (state === undefined) {
      return;
    }
    ensureMarker(sim, state);
    state.fireTimer = Math.max(state.fireTimer - 1, 0);
    state.hurtTimer = Math.max(state.hurtTimer - 1, 0);
    driveController(sim, state);
    fire(sim, state);
    stepEnemies(sim, state);
    if (state.health <= 0) {
      state.health = MAX_HEALTH;
      state.ammo = START_AMMO;
    }
  },
});

// The wave-marker component schema. Bump COMPONENT_VERSION + add a `migrate` (e.g. the
// commented `doubleVelocityX`) and save: a soft_app_reload rewrites the marker bytes live.
const doubleVelocityX = (bytes: Uint8Array): Uint8Array => {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  view.setFloat32(0, view.getFloat32(0, true) * 2, true);
  return bytes;
};
const waveComponent =
  COMPONENT_VERSION > 1
    ? component("Velocity", { migrate: doubleVelocityX, version: COMPONENT_VERSION })
    : component("Velocity", { version: COMPONENT_VERSION });

/** The HUD the harness reads each frame to update the DOM read-out. */
export const readHud = (): { health: number; score: number; ammo: number; enemies: number; wave: number } => ({
  ammo: store.state?.ammo ?? START_AMMO,
  enemies: store.state?.enemies.filter((enemy) => enemy.alive).length ?? 0,
  health: store.state?.health ?? MAX_HEALTH,
  score: store.state?.score ?? 0,
  wave: store.state?.wave ?? 1,
});

export default defineApp({
  components: [waveComponent],
  config: { fixedHz: 60, seed: 1n, surface: "axiom-canvas" },
  id: "retro-fps",
  scenes: [scene("arena", { build, version: SCENE_VERSION })],
  systems: [fpsSim],
});
