/*
 * The Phaser-style `Scene` authoring shell (SPEC-14). An author subclasses
 * `Scene`, overrides `preload`/`create`/`update`, and reaches the engine through
 * the factory namespaces (`this.add`, `this.input`, `this.time`, `this.tweens`,
 * `this.sound`, `this.cameras`, `this.physics`). At M0 the factories are typed
 * placeholders — discriminated stub objects each later spec replaces with a real
 * factory that projects its §4.2 surface (e.g. `this.add.*` -> world spawn,
 * SPEC-02; `this.physics.add.*` -> SPEC-03; `this.input.keyboard` -> SPEC-05).
 *
 * The game-object model is retained-ECS: a created object is a handle wrapping an
 * entity, so the lifecycle hooks return the handles they author (empty at M0,
 * until the world projection lands). The actual ECS wiring is a later spec; this
 * is the skeleton it fills.
 */

/** `this.add` — spawn/display factory (SPEC-02 world). */
export interface AddFactory {
  readonly subsystem: "add";
}

/** `this.input` — keyboard/pointer factory (SPEC-05 input). */
export interface InputFactory {
  readonly subsystem: "input";
}

/** `this.time` — timers/delayed-call factory (SPEC-00 frame model). */
export interface TimeFactory {
  readonly subsystem: "time";
}

/** `this.tweens` — interpolation factory (SPEC-08 animation). */
export interface TweensFactory {
  readonly subsystem: "tweens";
}

/** `this.sound` — audio factory (SPEC-10 audio). */
export interface SoundFactory {
  readonly subsystem: "sound";
}

/** `this.cameras` — camera/view factory (SPEC-04 2D surface). */
export interface CamerasFactory {
  readonly subsystem: "cameras";
}

/** `this.physics` — physics-body factory (SPEC-03 physics). */
export interface PhysicsFactory {
  readonly subsystem: "physics";
}

/** The base scene an author subclasses. The defaults author nothing. */
export class Scene {
  public readonly add: AddFactory = { subsystem: "add" };
  public readonly input: InputFactory = { subsystem: "input" };
  public readonly time: TimeFactory = { subsystem: "time" };
  public readonly tweens: TweensFactory = { subsystem: "tweens" };
  public readonly sound: SoundFactory = { subsystem: "sound" };
  public readonly cameras: CamerasFactory = { subsystem: "cameras" };
  public readonly physics: PhysicsFactory = { subsystem: "physics" };

  // The handles this scene has authored — empty until `create`/`update` spawn
  // Entities (retained-ECS framing). The default hooks return this list, so even
  // The base no-op hooks read real instance state.
  readonly #authored: readonly number[] = [];

  // The asset keys this scene declares — empty until `preload` lists any.
  readonly #assets: readonly string[] = [];

  /** Declare the asset keys to load before `create`. Default: none. */
  public preload(): readonly string[] {
    return this.#assets;
  }

  /** Author the scene's initial entities, returning their handles. Default: none. */
  public create(): readonly number[] {
    return this.#authored;
  }

  /** Advance the scene one fixed tick, returning the handles it mutated. Default: none. */
  public update(_tick: number, _dt: number): readonly number[] {
    return this.#authored;
  }
}
