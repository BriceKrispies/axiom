/*
 * The HOT RUNTIME (hot-reload architecture §6.2) — the long-lived owner of the
 * engine host that survives across app-code reloads. It is the platform-edge
 * counterpart of `boot.ts`: it does the one-time boot wiring ONCE (via
 * `bootSession`), holds the single `WasmGame` + `GameLoop` + keyed `GameRegistry`
 * for the app's whole life, and then RECONCILES a freshly-imported manifest INTO
 * that running engine instead of recreating it.
 *
 * This is the browser/live-wasm edge, so — exactly like `boot.ts` — it is listed in
 * `test-exempt.json` and its `.oxlintrc.json` override scopes off the branch ban and
 * the async/unsafe rules; it carries NO unit test (its correctness is the live wasm
 * path, proven via the integration / Playwright proof). It is THIN wiring: every
 * DECISION lives in the fully-covered pure core it delegates to — `diffManifest` /
 * `classifyUpdate` (`diff.ts`) decide what changed and how to respond, and the keyed
 * `GameRegistry` (`registry.ts`) and the `GameLoop` tick barrier (`game-loop.ts`)
 * apply it. This file only routes a classification to the calls that enact it.
 *
 * Update classes:
 *   - `hot_patch` — swap system bodies / add / remove / remount in place, enqueued on
 *     the loop's tick barrier so it lands between frames. The world state (owned by
 *     the still-alive `WasmGame`) is untouched, so a system-body edit keeps the tick
 *     counter and every entity exactly where they were.
 *   - `soft_app_reload` — rebuild the whole system set (dispose all, re-mount from the
 *     next manifest) while the live engine keeps the world. Byte-level component
 *     migration (snapshot → migrate → restore) is a documented future extension: it
 *     needs a Rust-side per-component migrator, since the snapshot is opaque bytes
 *     here; today the world simply persists in the live instance.
 *   - `full_page_reload` / `engine_restart_required` — handed to the caller's injected
 *     callbacks (the harness reloads the page or rebuilds the `WasmGame`).
 */

import type { AppManifest, SystemContext, SystemDef } from "./manifest.ts";
import { type BootGame, type BootOptions, bootSession } from "./boot.ts";
import { type ManifestDiff, type UpdateClass, classifyUpdate, diffManifest } from "./diff.ts";
import type { GameLoop } from "./game-loop.ts";
import type { GameRegistry } from "./registry.ts";
import type { NativeBridge } from "./native-bridge.ts";
import { clearScene } from "./scene3d.ts";
import { createGame } from "./game.ts";
import { migrateComponents } from "./reconcile.ts";

/** The reload/restart escalations the hot runtime cannot apply in place, handed back to the harness. */
export interface HotRuntimeOptions extends BootOptions {
  /** Invoked for `engine_restart_required` — the harness rebuilds the `WasmGame` (the legacy full restart). */
  readonly onEngineRestart?: (next: AppManifest) => void;
  /** Invoked for `full_page_reload` — the harness reloads the page (invalid/unmigratable change). */
  readonly onFullPageReload?: (next: AppManifest) => void;
}

/** The live hot-reload handle a dev harness drives from `import.meta.hot.accept`. */
export interface HotRuntime {
  /** Diff `next` against the live manifest, classify it, and apply the result at the next tick barrier. Returns the class. */
  readonly apply: (next: AppManifest) => UpdateClass;
  /** The manifest currently mounted. */
  readonly current: () => AppManifest;
  /** Stop the loop and remove DOM listeners (the engine instance itself is the caller's to drop). */
  readonly dispose: () => void;
}

/** The systems a manifest declares (its empty list when it declares none). */
const systemsOf = (manifest: AppManifest): readonly SystemDef[] => manifest.systems ?? [];

/** Register a system and run its one-time `mount` hook. */
const mountSystem = (registry: GameRegistry, def: SystemDef, context: SystemContext): void => {
  registry.upsert(def);
  def.spec.mount?.(context);
};

/** Run the currently-mounted system's `dispose` hook (if any) before it is replaced/removed. */
const disposeSystem = (registry: GameRegistry, id: string, context: SystemContext): void => {
  registry.get(id)?.spec.dispose?.(context);
};

/** Mount every system a manifest declares, under its config's context. */
const mountAll = (registry: GameRegistry, manifest: AppManifest): void => {
  const context: SystemContext = { config: manifest.config };
  for (const def of systemsOf(manifest)) {
    mountSystem(registry, def, context);
  }
};

/**
 * (Re)author every scene a manifest declares: clear the current scene, then run each
 * `build`. `clearScene` (which bumps the engine mesh generation so the `present3d`
 * loop re-uploads) means a scene rebuild is idempotent — the reconciler owns the
 * clear so an author's `build` only describes geometry. A manifest with no scenes
 * touches nothing (the 2D path, which draws through render systems, not a scene).
 */
const authorScenes = (manifest: AppManifest): void => {
  const scenes = manifest.scenes ?? [];
  if (scenes.length > 0) {
    clearScene();
    for (const scene of scenes) {
      scene.build();
    }
  }
};

/*
 * Apply a `hot_patch` system delta in place. Removed ids dispose then drop first, so a
 * removed-then-re-added id cannot alias; remounted ids dispose then re-mount; swapped
 * ids upsert (position preserved); added ids upsert + mount.
 */
const applySystemDelta = (registry: GameRegistry, context: SystemContext, diff: ManifestDiff): void => {
  for (const id of diff.systems.removed) {
    disposeSystem(registry, id, context);
    registry.remove(id);
  }
  for (const def of diff.systems.remounted) {
    disposeSystem(registry, def.id, context);
    mountSystem(registry, def, context);
  }
  for (const def of diff.systems.runSwapped) {
    registry.upsert(def);
  }
  for (const def of diff.systems.added) {
    mountSystem(registry, def, context);
  }
};

/** One reconciliation step's inputs: the prior + next manifest and their computed delta. */
interface Transition {
  readonly prev: AppManifest;
  readonly next: AppManifest;
  readonly diff: ManifestDiff;
}

/** The live wiring the reconciler drives: the keyed registry, the loop's tick barrier, the engine handle, its bridge, and the escalation callbacks. */
interface HotSession {
  readonly registry: GameRegistry;
  readonly loop: GameLoop;
  readonly options: HotRuntimeOptions;
  readonly game: BootGame;
  readonly bridge: NativeBridge;
}

/** Re-author scenes ONLY when a scene's structural version bumped — a pure system edit (new build closure, same version) leaves the scene, and the world, untouched. */
const reauthorChangedScenes = (step: Transition): void => {
  if (step.diff.scenes.changed.length > 0) {
    authorScenes(step.next);
  }
};

/** `hot_patch`: apply the system delta in place, then re-author only the scenes whose version bumped. */
const applyHotPatch = (session: HotSession, step: Transition): void => {
  applySystemDelta(session.registry, { config: step.next.config }, step.diff);
  reauthorChangedScenes(step);
};

/**
 * `soft_app_reload`: snapshot a transactional CHECKPOINT, migrate the live component
 * bytes (rolling back to the checkpoint if a migrator throws), then dispose + re-mount
 * the whole system set and re-author the scenes. The engine stays alive, so the world
 * (minus any migrated columns) persists across the rebuild.
 */
const rebuildSystems = (registry: GameRegistry, prev: AppManifest, next: AppManifest): void => {
  const prevContext: SystemContext = { config: prev.config };
  for (const def of systemsOf(prev)) {
    disposeSystem(registry, def.id, prevContext);
    registry.remove(def.id);
  }
  mountAll(registry, next);
};

const applySoftReload = (session: HotSession, step: Transition): void => {
  const { registry, game, bridge } = session;
  const checkpoint = game.snapshot();
  try {
    migrateComponents(bridge, step.diff.components.migrated);
  } catch {
    game.restore(checkpoint);
  }
  rebuildSystems(registry, step.prev, step.next);
  reauthorChangedScenes(step);
};

/** Diff + classify `prev`→`next` and enqueue/dispatch the response; returns the class. */
const reconcile = (session: HotSession, prev: AppManifest, next: AppManifest): UpdateClass => {
  const { loop, options } = session;
  const step: Transition = { diff: diffManifest(prev, next), next, prev };
  const kind = classifyUpdate(step.diff);
  const dispatch: Record<UpdateClass, () => void> = {
    engine_restart_required: (): void => {
      options.onEngineRestart?.(next);
    },
    full_page_reload: (): void => {
      options.onFullPageReload?.(next);
    },
    hot_patch: (): void => {
      loop.enqueueHotUpdate((): void => {
        applyHotPatch(session, step);
      });
    },
    soft_app_reload: (): void => {
      loop.enqueueHotUpdate((): void => {
        applySoftReload(session, step);
      });
    },
  };
  dispatch[kind]();
  return kind;
};

/**
 * Create the hot runtime: mint the app + keyed registry from the manifest config,
 * mount the initial systems, boot the shared session (host + loop + input + RAF), and
 * return the `apply` reconciler.
 */
export const createHotRuntime = (game: BootGame, manifest: AppManifest, options: HotRuntimeOptions): HotRuntime => {
  const app = createGame(manifest.config);
  const { registry } = app;
  mountAll(registry, manifest);
  const { loop, bridge, teardown } = bootSession(game, app, options);
  app.start();
  // Author the initial scenes now that the host channel is bound (before the first
  // Frame, so `present3d` binds the surface after the scene exists).
  authorScenes(manifest);
  const session: HotSession = { bridge, game, loop, options, registry };
  const state = { current: manifest };
  return {
    apply: (next: AppManifest): UpdateClass => {
      const kind = reconcile(session, state.current, next);
      state.current = next;
      return kind;
    },
    current: (): AppManifest => state.current,
    dispose: teardown,
  };
};
