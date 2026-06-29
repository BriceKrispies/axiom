/*
 * A capstone sample game authored ENTIRELY on the `@axiom/game` surface — a small
 * but real top-down arena that exercises a cross-section of the SDK through the
 * live `Sim` projection an `onFixedUpdate` callback receives (`sim.input`,
 * `sim.world`, `sim.add`, `sim.physics`, `sim.rng`, `sim.time`, `sim.tweens`) plus
 * the free host surface (`bindAction`, `reportOutcome`). It is the Phaser-style
 * `create`/`update` shape: the constructor is `create` (one-time authoring) and
 * `update` is the per-fixed-tick step.
 *
 * The game: a keyboard-driven player sprite carrying a kinematic physics body
 * moves around a 10x10 cell arena; reaching the pickup's cell starts a `tweens`
 * scale-pop whose completion banks the score and the pickup hops to its next cell;
 * a `time.every` timer spawns RNG-placed enemy sprites into the ECS world on a
 * fixed cadence; a `this.add.text` HUD is re-written each tick through the world
 * seam; the terminal score is reported as the host `Outcome`.
 *
 * Determinism boundary: the native physics solver owns body integration in the
 * real wasm runtime, but the projection tests inject a FAKE bridge that records
 * physics calls without integrating. So the authoritative player position here is
 * advanced in this authoring layer (a deterministic kinematic step `position +=
 * velocity * dt`), while the body still receives the velocity through the physics
 * surface. Every observable field is a pure function of (seed, input sequence),
 * which is exactly what the replay test in `test/arena.test.ts` proves: two runs
 * with the same scripted RNG draws and input sequence produce byte-identical
 * per-tick state hashes.
 */

import type { Body, PhysicsConfig } from "../physics.ts";
import type { Component, Vec2 } from "../vocabulary.ts";
import type { GameObject, RectangleStyle } from "../game-object.ts";
import { each, pick } from "../branchless.ts";
import type { Sim } from "../sim.ts";
import type { World } from "../world.ts";
import { bindAction } from "../input.ts";
import { reportOutcome } from "../host.ts";

// Arena geometry: a square play-field partitioned into a square cell grid.
const ARENA_MIN = 0;
const ARENA_MAX = 320;
const CELL_SIZE = 32;
const HALF_CELL = 16;
const GRID_CELLS = 10;
const START_X = 160;
const START_Y = 160;

// Movement + scoring tuning.
const PLAYER_SPEED = 320;
const PICKUP_POINTS = 10;
const SPAWN_INTERVAL = 30;
const KNOCK_IMPULSE_Y = 4;
const ARENA_DAMPING = 0.1;

// The pickup's scale-pop tween: from full size up to POP_TO over COLLECT_SECONDS.
const COLLECT_SECONDS = 0.1;
const POP_FROM = 1;
const POP_TO = 1.6;
const POP_EASE = "quadOut";

// Presentation: the pickup swatch and the HUD anchor.
const PICKUP_COLOR = 0xFF_D7_00;
const PICKUP_SIZE = 24;
const HUD_X = 8;
const HUD_Y = 8;

// The deterministic state digest constants (a bounded FNV-style rolling fold).
const HASH_SEED = 2_166_136_261;
const HASH_PRIME = 1_000_003;
const HASH_MODULUS = 2_147_483_648;

// Author-facing vocabulary: action names, texture/component kinds, and key maps.
const MOVE_LEFT = "left";
const MOVE_RIGHT = "right";
const MOVE_UP = "up";
const MOVE_DOWN = "down";
const PLAYER_TEXTURE = "player";
const ENEMY_TEXTURES: readonly string[] = ["grunt", "drone"];
const SPRITE_KIND = "Sprite";
const TEXT_KIND = "Text";
const SCORE_PREFIX = "Score ";
const LEFT_KEYS: readonly string[] = ["ArrowLeft", "KeyA"];
const RIGHT_KEYS: readonly string[] = ["ArrowRight", "KeyD"];
const UP_KEYS: readonly string[] = ["ArrowUp", "KeyW"];
const DOWN_KEYS: readonly string[] = ["ArrowDown", "KeyS"];

// The pickup hops through this fixed named-coordinate cell cycle on each collect.
const FIRST_PICKUP_X = 7;
const FIRST_PICKUP_Y = 5;
const SECOND_PICKUP_X = 9;
const SECOND_PICKUP_Y = 9;
const PICKUP_CELLS: readonly Vec2[] = [
  { x: FIRST_PICKUP_X, y: FIRST_PICKUP_Y },
  { x: SECOND_PICKUP_X, y: SECOND_PICKUP_Y },
];

// Top-down: no gravity, light damping on both axes.
const TOP_DOWN_CONFIG: PhysicsConfig = {
  angularDamping: ARENA_DAMPING,
  gravity: { x: 0, y: 0, z: 0 },
  linearDamping: ARENA_DAMPING,
};

// The pickup's render style (object-literal keys are alphabetized per sort-keys).
const PICKUP_STYLE: RectangleStyle = {
  color: PICKUP_COLOR,
  height: PICKUP_SIZE,
  width: PICKUP_SIZE,
};

/** A `Text` render component carrying the HUD string written through the world seam. */
interface TextLabel extends Component {
  readonly value: string;
}

/** Constrain a world-space coordinate to the arena bounds (branchless min/max). */
const clampRange = (value: number): number => Math.min(Math.max(value, ARENA_MIN), ARENA_MAX);

/** The integer cell index a world-space coordinate falls in. */
const cellOf = (value: number): number => Math.floor(value / CELL_SIZE);

/** Install the keyboard action bindings the player movement reads (SPEC-05). */
const bindActions = (): void => {
  bindAction(MOVE_LEFT, LEFT_KEYS);
  bindAction(MOVE_RIGHT, RIGHT_KEYS);
  bindAction(MOVE_UP, UP_KEYS);
  bindAction(MOVE_DOWN, DOWN_KEYS);
};

/** Spawn one RNG-placed, RNG-typed enemy sprite into the ECS world (SPEC-01/02). */
const spawnEnemy = (sim: Sim): void => {
  const texture = sim.rng.pick(ENEMY_TEXTURES);
  const cellX = sim.rng.int(GRID_CELLS);
  const cellY = sim.rng.int(GRID_CELLS);
  sim.add.sprite(texture, cellX * CELL_SIZE + HALF_CELL, cellY * CELL_SIZE + HALF_CELL);
};

/** Fold a list of state fields into one bounded deterministic digest. */
const foldHash = (fields: readonly number[]): number => {
  let digest = HASH_SEED;
  each(fields, (field): void => {
    digest = (digest * HASH_PRIME + field) % HASH_MODULUS;
  });
  return digest;
};

/** The top-down arena sample game, authored over the `Sim` surface. */
export class Arena {
  readonly #world: World;
  readonly #player: GameObject;
  readonly #playerBody: Body;
  readonly #pickup: GameObject;
  readonly #hud: GameObject;
  #score = 0;
  #pickupIndex = 0;
  #pickupCell: Vec2 = pick(PICKUP_CELLS, 0);

  /** `create`: author the world, body, pickup, HUD, and the enemy-spawn timer. */
  public constructor(sim: Sim) {
    bindActions();
    this.#world = sim.world;
    sim.physics.setConfig(TOP_DOWN_CONFIG);
    this.#player = sim.add.sprite(PLAYER_TEXTURE, START_X, START_Y);
    this.#playerBody = sim.physics.add.kinematic(this.#player);
    this.#pickup = sim.add.rectangle(this.#pickupCenter().x, this.#pickupCenter().y, PICKUP_STYLE);
    this.#hud = sim.add.text(this.#scoreLabel(), HUD_X, HUD_Y);
    // Alias the bound `time.every` closure (it carries no `this`) to a plain schedule call.
    const schedule = sim.time.every;
    schedule(SPAWN_INTERVAL, (): void => {
      spawnEnemy(sim);
    });
  }

  /** `update`: advance the player, resolve a pickup collection, refresh the HUD. */
  public update(sim: Sim): void {
    this.#move(sim);
    this.#resolvePickup(sim);
    this.#refreshHud();
  }

  /** Report the terminal score as the host `Outcome` (emit-exactly-once, SPEC-12). */
  public finish(): void {
    reportOutcome({ score: this.#score, won: this.#score > 0 });
  }

  /** A cheap deterministic digest of the whole observable game state this tick. */
  public hash(): number {
    return foldHash([
      Math.round(this.#player.x),
      Math.round(this.#player.y),
      this.#score,
      this.#enemyCount(),
      this.#pickupCell.x,
      this.#pickupCell.y,
    ]);
  }

  /** The banked score so far. */
  public get score(): number {
    return this.#score;
  }

  /** The live enemy count, read back from the ECS world. */
  public get enemyCount(): number {
    return this.#enemyCount();
  }

  /** The cell the player currently occupies. */
  public get playerCell(): Vec2 {
    return this.#playerCell();
  }

  #move(sim: Sim): void {
    const moveX = sim.input.axis(MOVE_LEFT, MOVE_RIGHT) * PLAYER_SPEED;
    const moveY = sim.input.axis(MOVE_UP, MOVE_DOWN) * PLAYER_SPEED;
    this.#playerBody.setVelocity({ x: moveX, y: moveY, z: 0 });
    this.#player.setPosition(
      clampRange(this.#player.x + moveX * sim.dt),
      clampRange(this.#player.y + moveY * sim.dt),
    );
  }

  #resolvePickup(sim: Sim): void {
    const onCell = this.#playerCell();
    const collected =
      Number(onCell.x === this.#pickupCell.x) * Number(onCell.y === this.#pickupCell.y);
    each(
      [
        (): void => {
          this.#collect(sim);
        },
      ].slice(0, collected),
      (run): void => {
        run();
      },
    );
  }

  #collect(sim: Sim): void {
    this.#playerBody.applyImpulse({ x: 0, y: KNOCK_IMPULSE_Y, z: 0 });
    this.#advancePickup();
    this.#startPopTween(sim);
  }

  #advancePickup(): void {
    const next = (this.#pickupIndex + 1) % PICKUP_CELLS.length;
    this.#pickupIndex = next;
    this.#pickupCell = pick(PICKUP_CELLS, next);
    this.#pickup.setPosition(this.#pickupCenter().x, this.#pickupCenter().y);
  }

  #startPopTween(sim: Sim): void {
    sim.tweens.add({
      duration: COLLECT_SECONDS,
      ease: POP_EASE,
      from: POP_FROM,
      onComplete: (): void => {
        this.#bankPoint();
      },
      onUpdate: (value): void => {
        this.#pickup.setScale(value, value);
      },
      to: POP_TO,
    });
  }

  #bankPoint(): void {
    this.#score += PICKUP_POINTS;
  }

  #refreshHud(): void {
    const label: TextLabel = { kind: TEXT_KIND, value: this.#scoreLabel() };
    this.#world.set(this.#hud.entity, label);
  }

  #enemyCount(): number {
    return this.#world.query(SPRITE_KIND).length - 1;
  }

  #playerCell(): Vec2 {
    return { x: cellOf(this.#player.x), y: cellOf(this.#player.y) };
  }

  #pickupCenter(): Vec2 {
    return {
      x: this.#pickupCell.x * CELL_SIZE + HALF_CELL,
      y: this.#pickupCell.y * CELL_SIZE + HALF_CELL,
    };
  }

  #scoreLabel(): string {
    return SCORE_PREFIX + String(this.#score);
  }
}
