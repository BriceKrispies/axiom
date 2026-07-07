/*
 * The pure manifest DIFF and update CLASSIFIER (hot-reload architecture §5.5 / §6.6).
 * This is the testable heart of HMR: given the live manifest and a freshly-imported
 * one, `diffManifest` computes what changed id-by-id, and `classifyUpdate` maps that
 * delta to the single safe response — `hot_patch`, `soft_app_reload`,
 * `full_page_reload`, or `engine_restart_required`. It touches no wasm and no
 * browser, so it is covered exhaustively against hand-built manifests.
 *
 * Every comparison is branchless: id membership is a `Map.has` test, run/hook/value
 * changes are `!==` / JSON compares, and the classifier is an ordered
 * severity table resolved with `Array.find` (never an `if`/`switch`). The classifier
 * classifies UP: when several severities apply, the most severe wins, so an
 * ambiguous change never silently downgrades to an unsafe in-place patch.
 */

import type { AppManifest, ComponentDef, ResourceDef, SceneDef, SystemDef } from "./manifest.ts";
import { orElse, present } from "./control-flow.ts";

/** The four responses to a manifest change, from mildest to most disruptive. */
export type UpdateClass = "hot_patch" | "soft_app_reload" | "full_page_reload" | "engine_restart_required";

/** How the system set changed: `added`/`removed` by id, `runSwapped` (same id, new body), `remounted` (same id, new mount/dispose). */
export interface SystemsDiff {
  readonly added: readonly SystemDef[];
  readonly removed: readonly string[];
  readonly runSwapped: readonly SystemDef[];
  readonly remounted: readonly SystemDef[];
}

/** How the resource set changed: `added`/`removed` by id, `patched` (same id, new value). */
export interface ResourcesDiff {
  readonly added: readonly ResourceDef[];
  readonly removed: readonly string[];
  readonly patched: readonly ResourceDef[];
}

/** How the component set changed: `added`/`removed` by id, `migrated` (new version + migrator), `unmigratable` (new version, no migrator). */
export interface ComponentsDiff {
  readonly added: readonly ComponentDef[];
  readonly removed: readonly string[];
  readonly migrated: readonly ComponentDef[];
  readonly unmigratable: readonly string[];
}

/** How the scene set changed: the `changed` scenes (new id OR bumped version) the reconciler re-authors. */
export interface ScenesDiff {
  readonly changed: readonly SceneDef[];
}

/** The whole computed delta between two manifests. */
export interface ManifestDiff {
  readonly systems: SystemsDiff;
  readonly resources: ResourcesDiff;
  readonly components: ComponentsDiff;
  readonly scenes: ScenesDiff;
  /** Whether the engine config (fixedHz / seed / surface) changed — forces an engine restart. */
  readonly configChanged: boolean;
}

/** Index a (possibly-absent) definition list by its stable id. */
const indexById = <Value extends { readonly id: string }>(defs?: readonly Value[]): Map<string, Value> =>
  new Map(orElse<readonly Value[]>(defs, []).map((def): readonly [string, Value] => [def.id, def]));

/** The stable-id keys present in `next` but not in `prev`. */
const addedDefs = <Value extends { readonly id: string }>(
  prev: Map<string, Value>,
  next: readonly Value[],
): readonly Value[] => next.filter((def) => !prev.has(def.id));

/** The stable ids present in `prev` but not in `next`. */
const removedIds = <Value extends { readonly id: string }>(
  prev: readonly Value[],
  next: Map<string, Value>,
): readonly string[] => prev.filter((def) => !next.has(def.id)).map((def) => def.id);

/** The `next` definitions whose id also exists in `prev` (the reconcilable overlap). */
const commonDefs = <Value extends { readonly id: string }>(
  prev: Map<string, Value>,
  next: readonly Value[],
): readonly Value[] => next.filter((def) => prev.has(def.id));

/** Whether a system's `mount`/`dispose` hook identity changed between two defs (either hook differs). */
const hooksDiffer = (prev: SystemDef, next: SystemDef): boolean =>
  [prev.spec.mount !== next.spec.mount, prev.spec.dispose !== next.spec.dispose].some(Boolean);

/** Compute the system delta between two manifests. */
const diffSystems = (prev: AppManifest, next: AppManifest): SystemsDiff => {
  const prevMap = indexById(prev.systems);
  const nextList = orElse<readonly SystemDef[]>(next.systems, []);
  const common = commonDefs(prevMap, nextList);
  const priorOf = (def: SystemDef): SystemDef => present(prevMap.get(def.id), "common system missing from prev");
  const remounted = common.filter((def) => hooksDiffer(priorOf(def), def));
  const runSwapped = common
    .filter((def) => priorOf(def).spec.run !== def.spec.run)
    .filter((def) => !hooksDiffer(priorOf(def), def));
  return {
    added: addedDefs(prevMap, nextList),
    remounted,
    removed: removedIds(orElse<readonly SystemDef[]>(prev.systems, []), indexById(next.systems)),
    runSwapped,
  };
};

/** A resource value fingerprint — the pure-data spec serialized for equality (resource specs carry no functions). */
const resourceFingerprint = (def: ResourceDef): string => JSON.stringify(def.spec);

/** Compute the resource delta between two manifests. */
const diffResources = (prev: AppManifest, next: AppManifest): ResourcesDiff => {
  const prevMap = indexById(prev.resources);
  const nextList = orElse<readonly ResourceDef[]>(next.resources, []);
  const priorOf = (def: ResourceDef): ResourceDef => present(prevMap.get(def.id), "common resource missing from prev");
  const patched = commonDefs(prevMap, nextList).filter(
    (def) => resourceFingerprint(priorOf(def)) !== resourceFingerprint(def),
  );
  return {
    added: addedDefs(prevMap, nextList),
    patched,
    removed: removedIds(orElse<readonly ResourceDef[]>(prev.resources, []), indexById(next.resources)),
  };
};

/** Whether a component schema carries a migrator (a `typeof`-function presence test — no absent-literal). */
const hasMigrate = (def: ComponentDef): boolean => [def.migrate].some((fn) => typeof fn === "function");

/** Compute the component delta between two manifests. */
const diffComponents = (prev: AppManifest, next: AppManifest): ComponentsDiff => {
  const prevMap = indexById(prev.components);
  const nextList = orElse<readonly ComponentDef[]>(next.components, []);
  const priorOf = (def: ComponentDef): ComponentDef =>
    present(prevMap.get(def.id), "common component missing from prev");
  const versionChanged = commonDefs(prevMap, nextList).filter((def) => priorOf(def).version !== def.version);
  return {
    added: addedDefs(prevMap, nextList),
    migrated: versionChanged.filter((def) => hasMigrate(def)),
    removed: removedIds(orElse<readonly ComponentDef[]>(prev.components, []), indexById(next.components)),
    unmigratable: versionChanged.filter((def) => !hasMigrate(def)).map((def) => def.id),
  };
};

/** Compute the scene delta: a scene is `changed` when it is new OR its structural version differs from prev. */
const diffScenes = (prev: AppManifest, next: AppManifest): ScenesDiff => {
  const prevMap = indexById(prev.scenes);
  const nextList = orElse<readonly SceneDef[]>(next.scenes, []);
  // Changed when NEW (absent from prev) OR its version bumped. Branchless: both signals
  // Into an array `.some(Boolean)`. A new scene defaults its "prior" to itself, so the
  // Version test alone would miss it — the `!has` term catches it.
  const isChanged = (def: SceneDef): boolean =>
    [!prevMap.has(def.id), orElse(prevMap.get(def.id), def).version !== def.version].some(Boolean);
  return { changed: nextList.filter((def) => isChanged(def)) };
};

/** A config fingerprint (fixedHz / seed / surface) — a seed is a bigint, so serialize its string form. */
const configFingerprint = (manifest: AppManifest): string =>
  [manifest.config.fixedHz, manifest.config.seed.toString(), manifest.config.surface].join("|");

/** Whether the engine config changed between two manifests. */
const configChanged = (prev: AppManifest, next: AppManifest): boolean =>
  configFingerprint(prev) !== configFingerprint(next);

/** Compute the full delta between the live manifest `prev` and the freshly-imported `next`. */
export const diffManifest = (prev: AppManifest, next: AppManifest): ManifestDiff => ({
  components: diffComponents(prev, next),
  configChanged: configChanged(prev, next),
  resources: diffResources(prev, next),
  scenes: diffScenes(prev, next),
  systems: diffSystems(prev, next),
});

/** Whether any component schema was added, removed, or migrated (a change that needs a snapshot→migrate→restore). */
const componentsNeedSoftReload = (components: ComponentsDiff): boolean =>
  [components.added.length, components.removed.length, components.migrated.length].some((count) => count > 0);

/*
 * Classify a diff into its single safe response. An ordered severity table
 * (most-severe first) is resolved with `find`, so the first applicable rule wins
 * and the trailing `[true, "hot_patch"]` row is the total default — every diff maps
 * to exactly one class with no branch.
 */
export const classifyUpdate = (diff: ManifestDiff): UpdateClass => {
  const table: readonly (readonly [boolean, UpdateClass])[] = [
    [diff.configChanged, "engine_restart_required"],
    [diff.components.unmigratable.length > 0, "full_page_reload"],
    [componentsNeedSoftReload(diff.components), "soft_app_reload"],
    [true, "hot_patch"],
  ];
  return present(
    table.find((row) => row[0]),
    "classify table always has its default row",
  )[1];
};
