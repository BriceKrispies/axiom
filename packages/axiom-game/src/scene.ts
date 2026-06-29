/*
 * The Phaser-style `Scene` authoring shell (SPEC-14). An author subclasses
 * `Scene`, overrides `preload`/`create`/`update`, and reaches the engine through
 * the factory namespaces (`this.add`, `this.input`, `this.time`, `this.tweens`,
 * `this.sound`, `this.cameras`, `this.physics`).
 *
 * Wave 3c filled the §2 factory stubs: the namespaces are no longer typed
 * `{ subsystem }` placeholders — they are the REAL per-tick projections. The
 * runtime (`scene-runtime.ts`) drives the lifecycle and, before each hook, binds a
 * `SceneFactories` set scoped to the running tick via `bindFactories`: `this.add`/
 * `this.physics` are the bridge-backed retained surfaces (SPEC-02/10), `this.input`/
 * `this.time`/`this.tweens` are bound to the running tick's snapshot/pump
 * (SPEC-05/07/09), and `this.sound` is the free audio surface (SPEC-08). `this.cameras`
 * remains a documented stub — no 2D `camera2D` projection exists yet (SPEC-04).
 *
 * The game-object model is retained-ECS: a created object is a handle wrapping an
 * entity, so the lifecycle hooks return the handles they author. `this.*` is only
 * legal once the runtime has mounted the scene (the getters fail loudly before
 * that), so reading a factory outside a driven lifecycle is a clear error, not a
 * silent `undefined`.
 */

import type { MusicOptions, ScheduleOptions, SoundOptions, ToneSpec } from "./host-binding.ts";
import type { Add } from "./game-object.ts";
import type { Handle } from "./vocabulary.ts";
import type { Input } from "./input.ts";
import type { Physics } from "./physics.ts";
import type { Time } from "./time.ts";
import type { Tweens } from "./tweens.ts";
import { present } from "./control-flow.ts";

/** `this.sound` — the audio factory (SPEC-08), forwarding to the presentation channel. */
export interface Sound {
  /** Register a sound asset by URL, returning its handle immediately. */
  readonly load: (url: string) => Handle;
  /** Start a voice playing sound `id`; return the voice handle. */
  readonly play: (id: Handle, opts?: SoundOptions) => Handle;
  /** Stop a playing voice (a stale handle is a clean no-op). */
  readonly stop: (voice: Handle) => void;
  /** Start a crossfaded music playlist; return its voice handle. */
  readonly music: (urls: readonly string[], opts?: MusicOptions) => Handle;
  /** Synthesize and play a tone from its spec; return the voice handle. */
  readonly tone: (spec: ToneSpec) => Handle;
  /** Schedule sound `id` to start at `atSeconds` on the audio clock; return the voice handle. */
  readonly schedule: (id: Handle, atSeconds: number, opts?: ScheduleOptions) => Handle;
  /** Set the master output gain in `[0, 1]`. */
  readonly setMasterVolume: (volume: number) => void;
  /** Mute or unmute all output. */
  readonly setMuted: (muted: boolean) => void;
}

/*
 * `this.cameras` — the 2D camera/view factory (SPEC-04). A documented deferred
 * stub: no `camera2D` draw2d export exists yet (the 2D surface owns no camera
 * verb), so this namespace carries no projected method — it names the slot the 2D
 * camera projection will fill, exactly as the M0 factory stubs did.
 */
export interface Cameras {
  readonly subsystem: "cameras";
}

/** The per-tick factory set the runtime binds onto a `Scene` before each lifecycle hook (SPEC-14 §4.2). */
export interface SceneFactories {
  /** `this.add` — the retained game-object factory (SPEC-02). */
  readonly add: Add;
  /** `this.input` — input over the running tick's snapshot (SPEC-05). */
  readonly input: Input;
  /** `this.physics` — physics bodies + world config (SPEC-10). */
  readonly physics: Physics;
  /** `this.time` — tick-driven timers + state machines (SPEC-07). */
  readonly time: Time;
  /** `this.tweens` — tick-sampled tweens (SPEC-09). */
  readonly tweens: Tweens;
  /** `this.sound` — the audio factory (SPEC-08). */
  readonly sound: Sound;
  /** `this.cameras` — the 2D camera factory (SPEC-04, deferred stub). */
  readonly cameras: Cameras;
}

/** The error a `this.*` read raises before the runtime has mounted (and bound) the scene. */
const UNMOUNTED = "scene factory read before the runtime mounted the scene";

/** The base scene an author subclasses. The defaults author nothing. */
export class Scene {
  // The tick-scoped factories the runtime binds before each lifecycle hook; absent
  // Until mount, so a factory read on an unmounted scene fails loudly.
  #factories?: SceneFactories;

  // The handles this scene has authored — empty until `create`/`update` spawn
  // Entities (retained-ECS framing). The default hooks return this list, so even
  // The base no-op hooks read real instance state.
  readonly #authored: readonly number[] = [];

  // The asset keys this scene declares — empty until `preload` lists any.
  readonly #assets: readonly string[] = [];

  /** `this.add` — spawn/display factory (SPEC-02 world). */
  public get add(): Add {
    return present(this.#factories, UNMOUNTED).add;
  }

  /** `this.input` — keyboard/pointer factory over the running tick (SPEC-05 input). */
  public get input(): Input {
    return present(this.#factories, UNMOUNTED).input;
  }

  /** `this.physics` — physics-body factory (SPEC-10 physics). */
  public get physics(): Physics {
    return present(this.#factories, UNMOUNTED).physics;
  }

  /** `this.time` — timers/delayed-call factory (SPEC-07). */
  public get time(): Time {
    return present(this.#factories, UNMOUNTED).time;
  }

  /** `this.tweens` — interpolation factory (SPEC-09). */
  public get tweens(): Tweens {
    return present(this.#factories, UNMOUNTED).tweens;
  }

  /** `this.sound` — audio factory (SPEC-08 audio). */
  public get sound(): Sound {
    return present(this.#factories, UNMOUNTED).sound;
  }

  /** `this.cameras` — camera/view factory (SPEC-04 2D surface, deferred stub). */
  public get cameras(): Cameras {
    return present(this.#factories, UNMOUNTED).cameras;
  }

  /** Scope `this.*` to the running tick's projections — called by the runtime mount before each lifecycle hook. */
  public bindFactories(factories: SceneFactories): void {
    this.#factories = factories;
  }

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
