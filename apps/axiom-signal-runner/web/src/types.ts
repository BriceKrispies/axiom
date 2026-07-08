/*
 * The whole Signal Runner game state, as plain value types. Nothing here imports
 * `@axiom/game` — the deterministic core (level generation + the fixed-step sim)
 * must be constructible and steppable in a bare Node test with no wasm, so its data
 * shapes are self-contained. `Vec` is the local 2D point; the render layer converts
 * these into the SDK's `Vec2`/`Rgba` at the draw boundary.
 *
 * Game-specific by construction: none of these nouns (shard, plate, storm, sled)
 * belong in an engine layer, so they live here in the app.
 */

/** A 2D point in whatever space the field documents (world path-space or screen). */
export interface Vec {
  readonly x: number;
  readonly y: number;
}

/** The lifecycle phase of a run. */
export type Phase = "run" | "win" | "lose";

/** Why a run ended in a loss (drives the game-over copy). */
export type LoseReason = "storm" | "time" | "crashed" | "fell";

/** The four deployable abilities, in card order. */
export type AbilityKind = "boost" | "shield" | "pulse" | "drone";

/** One centerline sample of the generated route. */
export interface PathNode {
  /** The lateral world-x of the path center at this node (accumulated curvature). */
  readonly cx: number;
  /** The path half-width at this node (widened at plate nodes). */
  readonly width: number;
  /** The signed curvature (Δcx per node) at this node — drives turn lean + drift. */
  readonly curve: number;
}

/** A collectible blue signal shard floating over the path. */
export interface Shard {
  /** Distance along the route. */
  readonly z: number;
  /** Lateral offset from the centerline. */
  readonly lateral: number;
  /** Set once when the runner (or a helper drone) crosses it. */
  collected: boolean;
}

/** A yellow pressure plate on a widened path spot. */
export interface Plate {
  readonly z: number;
  readonly lateral: number;
  activated: boolean;
}

/** A static hazard the runner must avoid (rock, fallen column). */
export interface Obstacle {
  readonly z: number;
  readonly lateral: number;
  /** Collision half-extent in lateral units. */
  readonly radius: number;
  readonly kind: "rock" | "column";
}

/** A drone hazard: a bobbing winged hazard that a pulse can disable. */
export interface DroneHazard {
  readonly z: number;
  /** The lateral center it bobs around. */
  readonly baseLateral: number;
  /** Lateral bob amplitude. */
  readonly sway: number;
  disabled: boolean;
}

/** A purely decorative side prop (never touches the sim). */
export interface Deco {
  readonly z: number;
  readonly lateral: number;
  readonly kind: "tree" | "rock" | "pillar";
  /** A per-prop size jitter in [0.8, 1.3]. */
  readonly scale: number;
}

/** A background mountain silhouette (parallax, never touches the sim). */
export interface Mountain {
  /** Screen-fraction horizontal center in [0, 1]. */
  readonly cx: number;
  /** Peak height as a screen fraction. */
  readonly height: number;
  /** Half-width as a screen fraction. */
  readonly halfWidth: number;
  /** Palette shade index (0 = nearest/darkest ridge). */
  readonly shade: number;
}

/** The immutable generated route + all placed entities. */
export interface Level {
  readonly seed: number;
  readonly segLen: number;
  /** Total route length in world units (the beacon sits at `length`). */
  readonly length: number;
  readonly nodes: readonly PathNode[];
  readonly shards: Shard[];
  readonly plates: Plate[];
  readonly obstacles: readonly Obstacle[];
  readonly drones: DroneHazard[];
  readonly decos: readonly Deco[];
  readonly mountains: readonly Mountain[];
  /** Distance along the route where the final beacon/relay stands. */
  readonly beaconZ: number;
}

/** The runner's live physical state. */
export interface Runner {
  /** Distance travelled along the route (world units). */
  dist: number;
  /** Lateral offset from the centerline. */
  lateral: number;
  /** Forward speed (world units / second). */
  speed: number;
  /** Lateral velocity (world units / second). */
  latVel: number;
  /** Ability charge in [0, 1]. */
  charge: number;
  /** Remaining shield ticks (absorbs one crash while > 0). */
  shieldTicks: number;
  /** Remaining boost ticks. */
  boostTicks: number;
  /** Remaining crash-invulnerability ticks (also drives the hit flash). */
  invulnTicks: number;
  /** How many crashes have happened. */
  crashes: number;
  /** Smoothed turn lean in [-1, 1] (presentation-facing, still deterministic). */
  lean: number;
}

/** A deployed helper drone that flies ahead collecting shards. */
export interface HelperDrone {
  active: boolean;
  z: number;
  lateral: number;
  ticks: number;
}

/** Per-ability cooldown ticks (0 = ready). */
export interface AbilityState {
  boostCd: number;
  shieldCd: number;
  pulseCd: number;
  droneCd: number;
  helper: HelperDrone;
}

/** The advancing storm wall. */
export interface Storm {
  /** Distance along the route the storm front has reached. */
  dist: number;
  /** Pressure in [0, 1] — how close the storm is, drives tint/shake. */
  intensity: number;
}

/** The complete, replayable game state. */
export interface State {
  readonly level: Level;
  runner: Runner;
  ability: AbilityState;
  storm: Storm;
  /** Monotonic fixed-tick index. */
  tick: number;
  /** Elapsed simulated seconds. */
  elapsed: number;
  /** Seconds remaining before the storm timer expires. */
  timeLeft: number;
  shardsCollected: number;
  platesActivated: number;
  beaconRestored: boolean;
  /** True when the runner is in the beacon's activation zone with objectives met. */
  beaconReady: boolean;
  phase: Phase;
  loseReason: LoseReason | null;
}

/** The per-tick control intent — a pure value the sim consumes (no I/O). */
export interface Intent {
  /** Digital steer from A/D/arrows: -1 left, 0, +1 right. */
  readonly steer: number;
  /** Analog steer target in [-1, 1] from mouse/touch drag, or null when absent. */
  readonly steerTo: number | null;
  readonly brake: boolean;
  /** Ability activation edges (true only on the tick the key went down). */
  readonly boost: boolean;
  readonly shield: boolean;
  readonly pulse: boolean;
  readonly drone: boolean;
  /** Enter edge — starts/activates/restarts depending on phase. */
  readonly confirm: boolean;
}

/** A neutral, all-false intent (idle tick / tests). */
export const IDLE_INTENT: Intent = {
  boost: false,
  brake: false,
  confirm: false,
  drone: false,
  pulse: false,
  shield: false,
  steer: 0,
  steerTo: null,
};
