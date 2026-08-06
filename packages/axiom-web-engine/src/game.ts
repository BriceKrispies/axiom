/*
 * game.ts — the PURE-FUNCTIONAL authoring contract, plus the pure reconciler that
 * turns a declarative scene into retained-store operations. This is the layer that
 * lets a game author write NOTHING but pure functions: they declare their static
 * `resources` (named meshes + materials) once, then supply `init` / `update` /
 * `view` (/ optional `sound`) — none of which touch the engine, hold a `Handle`,
 * or cause a side effect. The imperative `run-game.ts` shell owns every effect
 * (the loop, the GPU, input, audio) and drives these functions.
 *
 * `view(state)` returns a `Scene` VALUE — the whole frame described from scratch,
 * as if immediate-mode. `reconcile(prevMemory, scene)` then diffs that value
 * against the previously-applied scene and emits a `ReconcilePlan` of the minimal
 * spawn / re-pose / despawn / light ops, so the shell keeps the retained store
 * (and its per-frame cost) while the author never manages a node's lifetime.
 * Immediate-mode authoring, retained-mode execution.
 *
 * Like the rest of the spine this file is branchless and 100% covered: the diff is
 * expressed as `.filter`/`.map`/`.some` over the instance and light lists, with
 * presence tested by an absent-sentinel filter (the same idiom as `store.ts`).
 */

import type { Camera3D, EngineQuat, EngineVec3, InputFrame, Light, MaterialSpec, MeshData, MeshKind, Rgba, ToneSpec, Transform } from "./api.ts";
import { both, either, isPresent, presentOf } from "./branchless.ts";

// ── the declarative scene value ─────────────────────────────────────────────────

/**
 * A mesh a game declares in `resources`: either a named primitive kind, or its
 * own custom triangle-list geometry. Resolved to a store handle ONCE by the shell.
 *
 * `segments` is the primitive's RADIAL facet budget at full detail (the backend
 * scales it — see `createMesh`). It exists because tessellation is a property of
 * how large a primitive is on screen, not of its kind: a game that puts a
 * frame-filling disc and a thumbnail-sized rivet in the same scene must be able
 * to ask for a smooth silhouette on the first without paying for it on the
 * second. Declare the big one as its own named mesh with its own budget.
 * Omitted, it is the engine default — byte-identical to the previous fixed count.
 */
export type MeshRef = { readonly kind: MeshKind; readonly segments?: number } | { readonly data: MeshData };

/** The static, declared-once resource table. Meshes and materials are named; a
 * `SceneInstance` references them by name, so `view` never creates a resource or
 * handles one — it only arranges already-declared resources in space. */
export interface GameResources {
  readonly meshes: Readonly<Record<string, MeshRef>>;
  readonly materials: Readonly<Record<string, MaterialSpec>>;
}

/** One drawable in a `Scene`: a stable `key` (its identity across frames, how the
 * reconciler tracks it), the `mesh` + `material` resource names, and a transform.
 * A node's mesh/material are fixed at spawn, so changing either for a given key is
 * reconciled as a despawn + respawn. */
export interface SceneInstance {
  readonly key: string;
  readonly mesh: string;
  readonly material: string;
  readonly transform: Transform;
}

/**
 * A piece of TEXT placed in the scene.
 *
 * Text is a value here, not geometry, because the backends genuinely disagree
 * about what text IS. A DOM renderer draws it as text — one element, a real font,
 * crisp at any distance — which is the one thing it does better than a
 * rasterizer. The pixel backends have no font, so they build lettering out of
 * welded stroke boxes; that works for them and is exactly what a DOM renderer
 * cannot afford, because each hairline stroke costs a whole composited element
 * and the small ones get culled, which produces a PARTIAL word ("A ME") rather
 * than a missing one.
 *
 * So the scene says what the text is and where it goes, and each backend answers
 * in its own terms. A backend with no answer ignores it and the app supplies its
 * own lettering there.
 */
export interface SceneLabel {
  /** Stable identity across frames, like an instance key. */
  readonly key: string;
  readonly text: string;
  /** Where the text sits. Its +X is the reading direction and +Y is up, so the
   * rotation orients the text in the world exactly like any other node. */
  readonly transform: Transform;
  readonly color: Rgba;
  /** Cap height of one line, in world units. */
  readonly size: number;
}

/** One light in a `Scene`, keyed like an instance so it can be re-posed or dropped. */
export interface SceneLight {
  readonly key: string;
  readonly light: Light;
}

/** The whole frame as a pure value: the camera, an optional clear color and
 * ambient, the lights, and the drawable instances. `view(state)` returns one of
 * these; nothing about it references the engine. */
export interface Scene {
  readonly camera: Camera3D;
  readonly clearColor?: Rgba;
  /** The scene's AMBIENT (sky/bounce) light — the omni-directional diffuse floor
   * every surface receives regardless of orientation. Omitted, the engine default
   * (a neutral 0.12 grey) applies, so an existing scene is unchanged. Author it
   * to describe the ENVIRONMENT: a warm value for a sunlit beach where sand
   * bounces light back up, near-zero for a hard key-only void. See `setAmbient`. */
  readonly ambient?: Rgba;
  readonly lights: readonly SceneLight[];
  readonly instances: readonly SceneInstance[];
  /** Text placed in the scene. Backends that cannot draw text ignore it — see
   * `SceneLabel`. Few enough per frame that the shell replaces the whole list
   * rather than diffing it, so the reconciler does not deal in labels at all. */
  readonly labels?: readonly SceneLabel[];
}

// ── the game the author writes ───────────────────────────────────────────────────

/** The fixed-step simulation context handed to `update`. */
export interface TickContext {
  /** This fixed step's monotonic tick number (1, 2, 3, …). */
  readonly tick: number;
  /** The fixed step period in seconds (`1 / fixedHz`). */
  readonly dt: number;
}

/** The presentation context handed to `view`. `nowMs` is an EXPLICIT wall-clock
 * input (supplied by the shell), so a game whose look depends on real time — a
 * moving sun, a shimmer — stays a pure function of `(state, nowMs)` rather than
 * reaching for a hidden clock. Deterministic captures pin it. */
export interface ViewContext {
  readonly nowMs: number;
}

/**
 * A game as PURE DATA + PURE FUNCTIONS. `resources` and `actions` are static
 * tables; `init` seeds the state; `update` folds one fixed tick of input into new
 * state; `view` renders state (+ wall time) to a `Scene`; optional `sound` compares
 * the pre/post state of a tick and returns the tones to play. Not one of these may
 * touch the engine or mutate anything — the shell supplies every effect.
 */
export interface Game<State> {
  readonly resources: GameResources;
  /** Action name → the `KeyboardEvent.code` tokens that trigger it. */
  readonly actions: Readonly<Record<string, readonly string[]>>;
  readonly init: (seed: number) => State;
  readonly update: (state: State, input: InputFrame, ctx: TickContext) => State;
  readonly view: (state: State, ctx: ViewContext) => Scene;
  readonly sound?: (previous: State, next: State) => readonly ToneSpec[];
}

// ── reconciler value types ───────────────────────────────────────────────────────

/** What the reconciler remembers about one applied instance, to diff the next. */
interface InstanceSig {
  readonly mesh: string;
  readonly material: string;
  readonly transform: Transform;
}

/** The reconciler's memory of the last applied scene (keyed instances + lights). */
export interface SceneMemory {
  readonly instances: ReadonlyMap<string, InstanceSig>;
  readonly lights: ReadonlyMap<string, Light>;
}

/** A re-pose op: move an existing keyed node to a new transform. */
export interface ReposeOp {
  readonly key: string;
  readonly transform: Transform;
}

/**
 * The minimal set of retained-store operations that turn the previously-applied
 * scene into the new one. The shell executes these against its key→entity maps:
 * despawn first (freeing gone/replaced keys), then spawn, then re-pose; and the
 * light ops in the same remove/add/set order.
 */
export interface ReconcilePlan {
  readonly despawns: readonly string[];
  readonly spawns: readonly SceneInstance[];
  readonly reposes: readonly ReposeOp[];
  readonly removeLights: readonly string[];
  readonly addLights: readonly SceneLight[];
  readonly setLights: readonly SceneLight[];
}

// ── value equality (branchless) ──────────────────────────────────────────────────

/*
 * These run once per instance per frame — several hundred times at 60Hz — so they
 * are written with `both`/`either` rather than the `[a, b].every(Boolean)` idiom
 * used elsewhere in the spine. Both are equally branchless; the difference is that
 * the array form allocates a throwaway array on every call, and at this call rate
 * that churn is the reconciler's largest contribution to GC. Same truth table, no
 * garbage.
 */
const vec3Eq = (lhs: EngineVec3, rhs: EngineVec3): boolean =>
  both(both(lhs.x === rhs.x, lhs.y === rhs.y), lhs.z === rhs.z);
const quatEq = (lhs: EngineQuat, rhs: EngineQuat): boolean => lhs.every((value, index): boolean => value === rhs[index]);
const transformEq = (lhs: Transform, rhs: Transform): boolean =>
  both(both(vec3Eq(lhs.position, rhs.position), vec3Eq(lhs.scale, rhs.scale)), quatEq(lhs.rotation, rhs.rotation));

/** Whether an instance's mesh/material differs from a remembered signature — a
 * change either forces a despawn+respawn (the node's resources are fixed at spawn). */
const resourceChanged = (sig: InstanceSig, instance: SceneInstance): boolean =>
  either(sig.mesh !== instance.mesh, sig.material !== instance.material);

/** The reconciler's memory before any scene has been applied. */
export const emptyMemory = (): SceneMemory => ({ instances: new Map(), lights: new Map() });

/**
 * Diff `scene` against `prev` (the last applied scene) into the minimal
 * `ReconcilePlan`, and return the memory to carry into the next frame.
 *
 * Instances: a key absent from `prev`, or one whose mesh/material changed, is a
 * spawn (and, if it existed, also a despawn of the stale node); a key that
 * survives with the same resources but a new transform is a re-pose; a key `prev`
 * had that `scene` drops is a despawn. Lights: new keys are adds, surviving keys
 * are re-set unconditionally (a handful of lights — cheaper than diffing), dropped
 * keys are removes.
 *
 * COMPLEXITY. `nextByKey` is one keyed map of the new scene, built once and used
 * for everything that asks "is this key in the new scene, and as what". It
 * replaces a `Set` of keys plus a per-key `scene.instances.find(...)` in the
 * despawn pass — a linear scan nested inside a walk of the previous scene, which
 * made the reconciler QUADRATIC in the node count. A ~580-node scene paid roughly
 * 340,000 key comparisons per frame, 20 million a second at 60Hz, to discover that
 * almost nothing had been removed. It is now one hash lookup per previous key:
 * O(n) instead of O(n²).
 *
 * That same map is ALSO the memory carried into the next frame, because
 * "key → the instance as applied" is exactly what that memory is. A
 * `SceneInstance` satisfies `InstanceSig` structurally (it is that, plus the key),
 * so the second `Map` and the several-hundred freshly allocated signature objects
 * this used to build were re-deriving, every frame, a structure it had just
 * finished building.
 */
export const reconcile = (prev: SceneMemory, scene: Scene): { readonly plan: ReconcilePlan; readonly memory: SceneMemory } => {
  const nextByKey = new Map<string, SceneInstance>(
    scene.instances.map((instance): readonly [string, SceneInstance] => [instance.key, instance]),
  );
  const nextLights = new Map<string, Light>(
    scene.lights.map((entry): readonly [string, Light] => [entry.key, entry.light]),
  );

  const spawns = scene.instances.filter((instance): boolean => {
    const previous = prev.instances.get(instance.key);
    return either(
      !isPresent(previous),
      presentOf(previous).some((sig): boolean => resourceChanged(sig, instance)),
    );
  });

  const reposes = scene.instances
    .filter((instance): boolean =>
      presentOf(prev.instances.get(instance.key)).some(
        (sig): boolean => both(!resourceChanged(sig, instance), !transformEq(sig.transform, instance.transform)),
      ),
    )
    .map((instance): ReposeOp => ({ key: instance.key, transform: instance.transform }));

  const despawns = [...prev.instances.entries()]
    .filter(([key, sig]): boolean =>
      either(
        !nextByKey.has(key),
        presentOf(nextByKey.get(key)).some((instance): boolean => resourceChanged(sig, instance)),
      ),
    )
    .map(([key]): string => key);

  const addLights = scene.lights.filter((entry): boolean => !isPresent(prev.lights.get(entry.key)));
  const setLights = scene.lights.filter((entry): boolean => isPresent(prev.lights.get(entry.key)));
  const removeLights = [...prev.lights.keys()].filter((key): boolean => !nextLights.has(key));

  return {
    memory: { instances: nextByKey, lights: nextLights },
    plan: { addLights, despawns, removeLights, reposes, setLights, spawns },
  };
};
