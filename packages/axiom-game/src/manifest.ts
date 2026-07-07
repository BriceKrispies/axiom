/*
 * The `defineApp` manifest model (hot-reload architecture, docs/hot-reload-architecture-audit.md
 * §5). An author module stops REGISTERING behaviour as an import side effect and
 * instead EXPORTS a stable, ID-keyed description of what the app IS: its systems,
 * resources, and components. The engine owns runtime state; the app owns
 * definitions. The hot runtime (`hot-runtime.ts`) diffs an old manifest against a
 * new one (`diff.ts`) and reconciles the difference INTO the already-running
 * `WasmGame`, so a save patches the live engine instead of recreating it.
 *
 * Every definition is keyed by an author-supplied STABLE ID — the diff key. Two
 * manifests are compared id-by-id: a system whose `id` is unchanged but whose
 * `run` closure differs is a hot patch (swap the function pointer); a resource
 * whose `id` is unchanged but whose value differs is an in-place resource patch; a
 * component whose `id` is unchanged but whose `version` changed needs migration.
 *
 * A `SystemDef` NESTS its `spec` (`{ id, spec }`) rather than merging the id in.
 * The nesting is deliberate: it keeps the phase↔run correlation of the discriminated
 * `SystemSpec` intact through construction WITHOUT an object spread (banned in the
 * spine) or a cast — the projection in `registry.ts` narrows on `def.spec.phase`
 * with a sound type-guard predicate.
 */

import type { FixedUpdate, Render } from "./loop-core.ts";
import type { GameConfig } from "./game.ts";
import type { Rgba } from "./vocabulary.ts";

/** The lifecycle phase a system runs in. `fixedUpdate` = deterministic sim step; `render` = per-frame presentation. */
export type SystemPhase = "fixedUpdate" | "render";

/** The context a system's `mount`/`dispose` hook receives — the app config, constant for the app's life. */
export interface SystemContext {
  readonly config: GameConfig;
}

/** A system setup/teardown hook: `mount` runs when a system is first added, `dispose` before it is removed. */
export type SystemHook = (context: SystemContext) => void;

/** A deterministic `fixedUpdate` system: its `run` is the SPEC-00 `FixedUpdate` (sim step). */
export interface FixedSystemSpec {
  readonly phase: "fixedUpdate";
  readonly run: FixedUpdate;
  /** Explicit order key within the phase (lower runs first); defaults to registration order. */
  readonly order?: number;
  /** Setup hook run once when the system is added (subscriptions/bindings). */
  readonly mount?: SystemHook;
  /** Teardown hook run before the system is removed or remounted. */
  readonly dispose?: SystemHook;
}

/** A presentation `render` system: its `run` is the SPEC-00 `Render` (frame + interpolation alpha). */
export interface RenderSystemSpec {
  readonly phase: "render";
  readonly run: Render;
  readonly order?: number;
  readonly mount?: SystemHook;
  readonly dispose?: SystemHook;
}

/** A system specification — a fixed-update or a render behaviour, discriminated on `phase`. */
export type SystemSpec = FixedSystemSpec | RenderSystemSpec;

/** A stable-ID-keyed system definition: the `id` is the diff key, the `spec` the swappable behaviour. */
export interface SystemDef {
  readonly id: string;
  readonly spec: SystemSpec;
}

/** A lit-material resource value (the reconcilable half of a material — colours patch in place). */
export interface MaterialValue {
  readonly baseColor: Rgba;
  readonly emissive?: Rgba;
  readonly roughness?: number;
  readonly opacity?: number;
}

/** A resource specification — currently a lit material; the `kind` discriminates future resource families. */
export interface MaterialResourceSpec {
  readonly kind: "material";
  readonly material: MaterialValue;
}

/** A resource specification value (extensible; `kind`-discriminated). */
export type ResourceSpec = MaterialResourceSpec;

/** A stable-ID-keyed resource definition: `id` is the diff key, `spec` the patchable value. */
export interface ResourceDef {
  readonly id: string;
  readonly spec: ResourceSpec;
}

/** A scene author: build the retained scene from scratch via the free host surface (`clearScene` is run by the reconciler first). */
export type SceneBuild = () => void;

/** A scene specification — a `version` the author bumps on a STRUCTURAL change, plus the `build` that authors it. */
export interface SceneSpec {
  /**
   * The structural version. The reconciler re-authors the scene only when this
   * changes — so a pure system-body edit (which mints a new `build` closure on
   * re-import but leaves the version alone) does NOT reset the scene, while a
   * deliberate level change (bump the version) re-authors it live. This is the
   * scene analogue of a `ComponentDef` version, for the same reason: closure
   * identity cannot distinguish "structure changed" from "module re-imported".
   */
  readonly version: number;
  readonly build: SceneBuild;
}

/** A stable-ID-keyed scene: its `id`, its structural `version`, and the `build` that authors it. */
export interface SceneDef {
  readonly id: string;
  readonly version: number;
  readonly build: SceneBuild;
}

/** A component migrator: reshape a prior version's bytes to the current layout (absent ⇒ layout is incompatible). */
export type ComponentMigrate = (priorBytes: Uint8Array) => Uint8Array;

/** A stable-ID-keyed component schema: a monotonically increasing `version` plus an optional migrator across versions. */
export interface ComponentDef {
  readonly id: string;
  readonly version: number;
  readonly migrate?: ComponentMigrate;
}

/** The whole app description: its identity, its engine config, and its ID-keyed definition sets. */
export interface AppManifest {
  /** The stable app id — the HMR identity root. */
  readonly id: string;
  /** The engine configuration (fixed cadence, seed, surface) — a change here forces an engine restart. */
  readonly config: GameConfig;
  /** The behaviour systems, in declaration order (empty when omitted). */
  readonly systems?: readonly SystemDef[];
  /** The material/resource definitions (empty when omitted). */
  readonly resources?: readonly ResourceDef[];
  /** The component schemas (empty when omitted). */
  readonly components?: readonly ComponentDef[];
  /** The retained scenes (empty when omitted) — re-authored on a version bump. */
  readonly scenes?: readonly SceneDef[];
}

/** Build a stable-ID-keyed system definition (SPEC hot-reload §5.2). */
export const system = (id: string, spec: SystemSpec): SystemDef => ({ id, spec });

/** Build a stable-ID-keyed resource definition (SPEC hot-reload §5.2). */
export const resource = (id: string, spec: ResourceSpec): ResourceDef => ({ id, spec });

/** Build a stable-ID-keyed component schema (SPEC hot-reload §5.2). */
export const component = (id: string, schema: Omit<ComponentDef, "id">): ComponentDef => ({
  id,
  migrate: schema.migrate,
  version: schema.version,
});

/** Build a stable-ID-keyed scene (SPEC hot-reload §5.2). */
export const scene = (id: string, spec: SceneSpec): SceneDef => ({
  build: spec.build,
  id,
  version: spec.version,
});

/** Identity pass-through that names an object as the app manifest — the single default export of an author module. */
export const defineApp = (manifest: AppManifest): AppManifest => manifest;
