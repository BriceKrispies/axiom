# Hot-Reload Architecture Audit — `@axiom/game` SDK

**Status:** audit only — no code changed in this pass.
**Author:** engine-architecture audit, 2026-07-07.
**Goal:** move the `@axiom/game` browser SDK from its current *full-restart-per-save*
dev loop to **Vite-speed hot-module reload (HMR)** where the browser page, canvas,
input, render backend, WASM engine, and world state stay alive while TypeScript app
behavior is re-imported and *reconciled* into the running engine.

This document is concrete enough that a follow-up session can implement the
**First Implementation Slice** (§9) directly.

---

## 0. TL;DR

Today a "game" in `@axiom/game` is a **throwaway**: on every save the browser
constructs a brand-new `WasmGame`, mints a brand-new `GameRegistry`, re-imports the
author module, re-binds the canvas surface, re-uploads every texture, and re-runs
the sim from tick 0. Nothing survives except the compiled wasm *module*. This is the
"deterministic re-run" model the harnesses proudly document — it is correct for a
determinism demo and **structurally hostile to HMR**.

The engine already contains the missing primitive. The native Rust `RunningApp`
exposes `reauthor(setup)` — "an external editor hands the engine a new scene
description at a tick boundary and the next frame renders it" — plus
`snapshot_sim()` / `restore_sim()`. **None of these are surfaced to the TypeScript
SDK.** `WasmGame` exposes only `snapshot()`, never `reauthor` or `restore`.

The move is therefore threefold:

1. **Author model** → introduce an explicit `defineApp(manifest)` model. App modules
   export **stable, ID-keyed definitions** (systems, resources, components, prefabs,
   scenes, input bindings). The engine owns runtime state; the app owns definitions.
2. **Runtime** → add a long-lived `HotRuntime` that holds the `WasmGame` + registries
   across reloads and **diffs old manifest against new**, classifying each change as
   `hot_patch` / `soft_app_reload` / `full_page_reload` / `engine_restart_required`,
   then applies it at a **tick barrier** (surfacing `RunningApp::reauthor` through a
   new `WasmGame.reauthor` + a `NativeBridge.reauthor` seam).
3. **Dev server** → replace the bespoke SSE-triggers-full-restart server with **Vite**
   (file watch, ESM transform, module graph, HMR transport) + a thin Axiom Vite
   plugin. The dev client accepts new modules via `import.meta.hot.accept` and calls
   `hotRuntime.apply(newManifest)`. The SSE full-restart path is demoted to a
   **fallback** for `full_page_reload`/`engine_restart_required`.

---

## 1. Current hot-reload flow (file change → running app restart)

The current loop is a bespoke SSE-driven **full deterministic re-run**. No Vite, no
`import.meta.hot`, no module graph. (Grep confirms `vite` appears only transitively
in lockfiles; `import.meta.hot` appears only in the *Rust* gallery's `retro_fps`
level-reload, a different mechanism — see §4.)

**Actors:**

- `scripts/axiom_dev_server.mjs` — zero-dependency Node dev server (repo tooling).
- `apps/axiom-game-runtime/web/src/harness.ts` — 2D browser boot harness (platform edge).
- `apps/axiom-retro-fps-ts-browser/web/src/harness.ts` — 3D browser boot harness.
- `apps/<app>/web/src/game.ts` — **the only file the author edits**.
- `packages/axiom-game/dist/*` — the SDK, served once at `/vendor/axiom-game/`.
- `apps/axiom-game-runtime/web/pkg/*` — the shared wasm engine, served at `/pkg/`.

**Sequence** (`scripts/axiom_dev_server.mjs`):

1. `main()` (line 174) runs an initial `compile()` (tsgo over `web/tsconfig.json`,
   line 74), then `watch(SRC_DIR, {recursive:true})` (line 177) for `*.ts` saves.
2. On save → `rebuild()` (debounced 120 ms, line 162) → `compile()` again. On exit 0
   it bumps `version = Date.now()` (line 166) and `broadcast()` (line 67), writing
   `event: reload\ndata: <version>\n\n` to every open SSE client (`/events`, line 126).
3. The server serves the SDK from `packages/axiom-game/dist` at `/vendor/axiom-game/`
   (line 139), the wasm from `.../web/pkg` at `/pkg/` (line 145), and everything else
   from the app `web/` dir with `Cache-Control: no-store` (line 113).

**Sequence** (`harness.ts`, the browser side — 2D at `apps/axiom-game-runtime/web/src/harness.ts`):

4. `boot_()` (line 121) calls `initWasm()` **once** (line 127), bakes the font atlas
   once, then defines a `load(version)` closure (line 134).
5. `load()` is the **restart unit**. Every call:
   - `teardown?.()` — stops the previous RAF loop + DOM listeners (line 135).
   - `new WasmGame(fixedStepNanos, MAX_STEPS_PER_FRAME)` — **a brand-new engine
     instance** (line 136).
   - `game.bind2dSurface("c")` — **re-binds the canvas/backend** (line 138).
   - `game.upload2dTexture(FONT_ATLAS_TEXTURE, …)` — **re-uploads the font atlas** (line 140).
   - `createGame({fixedHz, seed, surface})` — **mints a fresh `GameRegistry`** and
     installs it as active (line 142).
   - `await import('/dist/game.js?v=${version}')` — cache-busted re-import; the
     author's top-level `onFixedUpdate`/`onRender` calls register into the new
     registry as an **import side effect** (line 145).
   - `onRender(...)` installs the presenter last (line 148).
   - `app.start()` + `boot(game, app, {canvas})` builds a fresh `GameLoop` and drives
     RAF (line 153-154).
6. `load(0)` runs once at boot (line 156). Then an `EventSource('/events')` (line 164)
   listens for `reload` and calls `load(Number(event.data))` (line 174) — i.e. **the
   whole of step 5 re-runs on every save.** The 3D harness
   (`apps/axiom-retro-fps-ts-browser/web/src/harness.ts`, line 69-97) is identical in
   shape: `new WasmGame(...)` + `createGame(...)` + re-import + `boot(...)` per reload,
   explicitly documented as "mint a fresh `WasmGame` (same seed) … and re-run from
   tick 0."

**Net effect:** a save destroys and rebuilds the entire engine + app, re-runs from
tick 0. Sim state, world entities, physics, uploaded textures, and the accumulator
are all discarded. This is a *cold restart wearing a hot-reload costume.*

---

## 2. Current SDK/app ownership model

| Concern | Owner today | Lifetime |
|---|---|---|
| Engine lifetime (`WasmGame`) | **the harness** (`harness.ts` `load()`) | recreated **every save** |
| WASM module (compiled code) | `initWasm()` | once per page |
| ECS world / scene / physics / RNG / outcome | `GameBridge` → `RunningApp`, inside `WasmGame` | recreated every save |
| Render backend + canvas binding | `WasmGame.windowing` (`WindowingApi`) | re-bound every save |
| Uploaded 2D textures / 3D mesh set | `WasmGame.textures_2d` / `render_meshes` | wiped + re-uploaded every save |
| Callback registry (systems) | `GameRegistry` in `registry.ts` | fresh per `createGame` (every save) |
| Active-registry pointer | module-global `state.active` in `registry.ts` | survives (SDK loaded once) |
| Bound host channel | module-global `session.host` in `host-binding.ts` | survives, but `bindNative` re-runs per save |
| App lifecycle (`Game` status FSM) | `game.ts` `GameImpl` | fresh per `createGame` |
| Author sim state | **top-level `let` in `game.ts`** (`let phase`, `let tick`, `let badge`) | reset every save |
| DOM input listeners | `driveDomInput` (from `boot`) | torn down + rebuilt every save |
| RAF loop | `driveRaf` (from `boot`) | torn down + rebuilt every save |

**Two ownership facts matter most:**

- **The app owns the engine lifetime.** `new WasmGame()` lives in app harness code,
  and the *natural* authoring pattern (`game.ts`) puts durable state in module-level
  `let` bindings that reset on re-import. Both are HMR-hostile (see §3).
- **The only things that already survive a reload** are the wasm module and the two
  SDK module-global holders (`registry.ts` `state.active`, `host-binding.ts`
  `session.host`). There is **no long-lived object that owns "the running game"**
  across reloads — the harness `load()` closure is the closest thing, and it throws
  everything away each call.

The engine itself is well-layered for this: the deterministic spine (`GameLoop`,
`stepFrame`, the `Sim`/`Frame` projections) talks only to the `NativeBridge`
interface (`native-bridge.ts`), never to a live wasm object. That seam is the right
place to add reconciliation verbs.

---

## 3. Blockers to proper HMR

Each blocker cites the exact site and why it blocks Vite-speed reload.

### B1 — App code owns and recreates the engine lifetime
`apps/*/web/src/harness.ts` `load()` calls `new WasmGame(...)` on **every** reload
(2D: `harness.ts:136`; 3D: `harness.ts:71`). The engine is a child of the reload
unit, so a reload *is* an engine restart. HMR requires the inverse: a long-lived
engine that outlives app-code reloads.

### B2 — Author state lives in top-level module variables
`apps/axiom-game-runtime/web/src/game.ts` holds sim state in module-level `let`
(`let phase = 0` line 42, `let shimmer` line 43, `let tick` line 45, `let badge = 0`
line 24). A cache-busted `import('/dist/game.js?v=N')` creates a **new module
instance**, so these reset to their initializers every save. Durable gameplay state
must live in the *engine* (the ECS world), not in app-module globals, or HMR loses it.

### B3 — Systems are anonymous, append-only, and un-replaceable
`GameRegistry` (`registry.ts:21`) stores callbacks in two arrays
(`#fixedUpdates`, `#renders`) with **`push`-only** registration (`onFixedUpdate`
line 26, `onRender` line 31) and **no identity, no removal, no replacement**. The
free `onFixedUpdate`/`onRender` (`registry.ts:62/67`) just append to the active
registry. There is no way to say "replace the system named `ball.physics` with this
new function" — the only way to change a system is to drop the whole registry and
re-append everything, which is what `createGame` forces (`game.ts:94-98`). This is
*the* central blocker to hot-patching a system body.

### B4 — No manifest/definition layer between app code and engine
App modules communicate with the runtime purely through **import side effects**
(`harness.ts:145` comment: "registers its onFixedUpdate / onRender … as an import
side effect"). There is no exported, inspectable description of "what this app is"
to diff against. Without a manifest, the runtime cannot compute what changed and
therefore cannot do anything but a full re-run.

### B5 — Authored entities/resources have no stable IDs
Entities are minted by monotonic native handles: `world_spawn` (`bridge.rs:251`)
returns a fresh id; `spawnObject` (`game-object.ts:203`) and `BridgeWorld.spawn`
(`world.ts:57`) call `worldSpawn` with no author key. Mesh/material/texture handles
are the same — `createMesh`/`createMaterial` (`scene3d.ts:30/91`) return opaque
engine-minted handles; `loadTexture` returns a `TextureId` keyed by the engine.
Re-running `create()` produces **new** handles for the *same conceptual entity*, so
there is nothing to reconcile against. Handles are stable only across an *identical
replay* and reset to `1` on any world rebuild or `clearScene`
(`scene3d.rs:217-222`, proven by `scene3d.rs:400 authoring_mints_stable_distinct_handles_and_replays`).
The **one** exception is textures: `AssetRegistry::load_texture`
(`assets.rs:47-54`) dedups by URL — the same URL recalls the same handle — which is
the only content-addressable identity in the crate and the seed of the stable-ID model
(see R3). Everything else needs author-supplied stable IDs before scene reconciliation
by identity is possible.

### B6 — No lifecycle/dispose hooks for systems
A system is a bare `(sim) => void` (`loop-core.ts:15`). It cannot register a
subscription, a timer, or a listener and later tear it down before replacement.
Replacing such a function would **leak** whatever it set up. HMR needs a
`mount(ctx)/run(world,dt)/dispose()` shape so teardown runs before a swap.

### B7 — No tick/frame barrier for applying updates
`GameLoop.advance` (`game-loop.ts:68`) reads the callback lists *live* every frame
(`this.#registry.fixedUpdates()` line 80). There is no point where "pending hot
updates" are drained safely between ticks. Mutating the arrays mid-`stepFrame` (which
can run N fixed updates per frame, `loop-core.ts:41`) would apply a change mid-tick.
A defined barrier — "apply queued manifest diffs after the current frame's last
fixed update, before the next `advance`" — does not exist.

### B8 — No resource-patch path; resources are recreated with the app
There is no "update material `grass` in place" verb. `createMaterial`
(`scene3d.ts:91`) always mints a new handle. On reload the whole scene is rebuilt
(`new WasmGame`), so resources are recreated wholesale. Patching a material color
without recreating the app is not expressible today.

### B9 — No component schema / migration concept
Components cross the bridge as `(kind, bytes)` pairs (`bridge.rs` world section;
`native-bridge.ts:72`). `ComponentKind` is a bare string; there is **no version, no
migrate**. A changed component layout would silently mis-decode. HMR needs
`component("health", { version, migrate })` so a layout change is classified as
"needs migration → soft reload" rather than corrupting state.

### B10 — `WasmGame` surfaces the engine's re-author primitive only *destructively*
The native `RunningApp` already has `reauthor(setup)` (`modules/axiom/src/app.rs:403`)
— "the host driver requires it … an external editor hands the engine a new scene
description at a tick boundary and the next frame renders it" — and `snapshot_sim`/
`restore_sim` (`app.rs:428/436`). Crucially, **the wasm runtime already calls
`reauthor` in place** — `WasmGame.clearScene` → `GameBridge` → `RunningApp::reauthor`
with an empty closure (`apps/axiom-game-runtime/src/scene3d.rs:217-222`), keeping the
GPU/canvas binding alive and bumping the mesh generation. So the *in-place reauthor
path from wasm is proven to work* — it just exists only as a **destructive "wipe the
whole scene"** verb, not as a keyed reconcile. And `WasmGame` exposes only
`snapshot()` (line 272): there is **no additive `reauthor`, no `restore`** at the wasm
boundary (`restore_sim` exists on `RunningApp` but is **not wired through `GameBridge`**),
and no `NativeBridge.reauthor` seam (`native-bridge.ts` has none). The capability
exists at the engine layer and is *stranded* below the SDK — and the one exposed hook
resets all handle ids to 1 (see B5).

### B11 — Dev server couples file-change directly to full-restart
`scripts/axiom_dev_server.mjs` has exactly one response to any `*.ts` change:
recompile → bump version → `broadcast('reload')` (line 162-172). The client's only
handler re-runs `load()` (full restart, `harness.ts:174`). There is no message taxonomy
(patch vs reload), no module-graph awareness, no partial update. The transport is
hard-wired to the sledgehammer.

### B12 — Vite/HMR entirely absent
No `vite.config`, no `import.meta.hot.accept`, no module-graph-aware invalidation. The
`import('/dist/game.js?v=N')` cache-bust is a hand-rolled, whole-module substitute for
what Vite's HMR does per-module with dependency tracking. Building real HMR on the
bespoke SSE server would mean re-implementing Vite badly.

### B13 — Example apps teach the HMR-hostile pattern
`game.ts` is explicitly documented as "re-runs deterministically with your edit
applied **from tick 0**" and stores state in top-level `let`. The retro-FPS harness
documents "Mode B … re-run from tick 0." Every example trains authors to expect a
cold restart and to keep state in module globals — the exact patterns HMR forbids.

---

## 4. Existing pieces we can reuse

The engine is closer to HMR-ready than the SDK surface suggests. Reusable assets:

### R1 — `RunningApp::reauthor` (the keystone)
`modules/axiom/src/app.rs:403`. Swaps the scene (`self.scene`, `light_direction`,
`meshes`, `materials`, `renderables`) **in place**, keeping the engine, backend, and
instance buffer alive. Its doc-comment *literally describes the HMR use case*:
"an external editor hands the engine a new scene description at a tick boundary and the
next frame renders it." Its constraint — mesh **geometry** is fixed at bind (the live
backend's vertex buffer is sized at startup; reauthor changes instance transforms,
material colours, and renderable count up to the instance-buffer capacity, never base
mesh geometry) — **defines our hard boundary** (§12): geometry changes need an engine
restart. Test: `app.rs:922 reauthor_replaces_the_scene_and_renderable_count_in_place`.
**It is already reachable from wasm** — `WasmGame.clearScene` calls it with an empty
closure (`scene3d.rs:217-222`), keeping the live GPU binding intact — so §6.4 *extends
an existing wasm→reauthor path* from destructive to additive, not new plumbing.

### R2 — `snapshot_sim` / `restore_sim`
`app.rs:428/436`, surfaced through `GameBridge::snapshot_sim` (`bridge.rs:205`) and
`WasmGame::snapshot` (`wasm.rs:272`). Serializes durable world state (entity identity,
component columns, player/controller maps) to bytes and restores it, with the tick
continuing forward. This is the **state-preservation mechanism** for `soft_app_reload`
(rebuild the app but restore the world) and for schema migration.

### R3 — The generation-counter re-upload pattern
`WasmGame.textures_2d_generation` (`wasm.rs:117`) and `render_meshes_generation`
(`wasm.rs:126`) + `GameBridge::mesh_generation` (`bridge.rs:146`). The presenter
re-uploads to the GPU **only when the generation changed**, never per frame. This is
exactly the diff-gated resource-patch pattern HMR needs — generalize it: bump a
generation on a resource patch, re-upload only the changed resource. Note
`upload2dTexture` (`wasm.rs:363-371`) already **replaces a texture in place by id** and
bumps the generation — an existing in-place resource patch we widen to materials/meshes.
And `AssetRegistry::load_texture` (`assets.rs:47-54`) already **dedups textures by URL**
(same URL ⇒ same handle) — the content-addressable identity we extend into the general
stable-ID model.

### R4 — The `NativeBridge` seam
`native-bridge.ts:52`. The deterministic spine depends only on this interface, never
on a live wasm object; every projection is tested against a *fake* bridge. Adding
reconciliation verbs (`reauthor`, `patchResource`, `snapshot`/`restore` are already
partly there) to this seam keeps them fully testable without wasm.

### R5 — The per-game registry indirection
`registry.ts` already decouples the free authoring functions from a specific registry
via the `state.active` holder (`registry.ts:51`, `useRegistry` line 57). The
mechanism for "point the free functions at a different registration set" exists — we
extend it from *swap-whole-registry* to *keyed replace-one-system*.

### R6 — The scene lifecycle + tick scheduling
`GameLoop` already drives a `MountedScene` with a `create` one-shot and a per-tick
`update` (`game-loop.ts:60-87`, `scene-runtime.ts:84`), and owns a `#pendingStart`
one-shot drained on the first `advance` (`game-loop.ts:49,71`). This is the exact
place to add a `#pendingHotUpdates` drain — the tick barrier (§6) is a small extension
of machinery that already exists.

### R7 — The native `axiom-dev-reload` precedent (Rust gallery)
`tools/axiom-dev-reload` + `apps/axiom-gallery/src/retro_fps/web.rs:198` already
demonstrate **the target pattern in Rust**: a watched `level.axiom` file is pushed over
SSE, and `reload_retro_fps(&mut running, &new_doc)` (`retro_fps/mod.rs:767`) calls
`running.reauthor(...)` to re-author the scene **without recreating the engine**
("keeping the engine ticking"). This is a working, in-repo proof that reauthor-in-place
is the right shape — the TS SDK simply has no equivalent. Reuse the *shape*, not the
transport (we use Vite, not this SSE tool).

### R8 — DOM input / RAF drivers are already isolated
`driveDomInput` and `driveRaf` (bound in `boot.ts:116,126`) are self-contained and
return teardowns. In the HMR model these bind **once** to the long-lived runtime and
never tear down on a hot patch — only the systems change.

---

## 5. Required SDK API changes

Opinionated, single design. The app module stops *doing* and starts *declaring*.

### 5.1 `defineApp` — the manifest entry point

Replace "import side effects register callbacks" with a single default export:

```ts
// @axiom/game
export interface AppManifest {
  readonly id: string;                     // stable app id (HMR identity root)
  readonly config: GameConfig;             // fixedHz, seed, surface
  readonly components?: readonly ComponentDef[];
  readonly resources?: readonly ResourceDef[];
  readonly prefabs?: readonly PrefabDef[];
  readonly scenes?: readonly SceneDef[];
  readonly systems?: readonly SystemDef[];
  readonly input?: readonly InputBindingDef[];
}

export const defineApp = (manifest: AppManifest): AppManifest => manifest;
```

Author file becomes:

```ts
// apps/*/web/src/game.ts  — NEW shape
import { defineApp, system, resource, component, prefab, scene } from "@axiom/game";

export default defineApp({
  id: "orbs",
  config: { fixedHz: 60, seed: 1n, surface: "c" },
  resources: [ resource("orb.material", { baseColor: [0.4, 0.8, 1, 1] }) ],
  components: [ component("spin", { version: 1, fields: { rate: "f32" } }) ],
  systems: [
    system("orb.spin", { phase: "fixedUpdate", run: (world, dt) => { /* … */ } }),
    system("orb.draw", { phase: "render",       run: (frame, alpha) => { /* … */ } }),
  ],
});
```

### 5.2 Stable-ID definition factories

Every definition is keyed by an author-supplied stable ID (the diff key):

```ts
export const system:   (id: string, def: SystemSpec)    => SystemDef;
export const resource: (id: string, spec: ResourceSpec) => ResourceDef;
export const component: (id: string, schema: ComponentSchema) => ComponentDef;
export const prefab:   (id: string, def: PrefabSpec)    => PrefabDef;
export const scene:    (id: string, def: SceneSpec)     => SceneDef;
```

Stable IDs are mandatory and match exactly the shapes the task requested:
`system("ball.physics", …)`, `resource("grass.material", …)`,
`prefab("player", …)`, `component("health", { version, migrate })`,
`scene("main", …)`, and authored scene entities carry a stable `id` in their prefab
placement.

### 5.3 System lifecycle shape

```ts
export type SystemPhase = "fixedUpdate" | "update" | "render" | "editor";

export interface SystemSpec {
  readonly phase: SystemPhase;
  /** Set up subscriptions/listeners/timers; return value ignored. Runs once when the system is (re)mounted. */
  readonly mount?: (ctx: SystemContext) => void;
  /** Per-tick/frame behavior. THE hot-swappable pointer — a hot patch replaces only this. */
  readonly run: SystemRun;              // (world, dt) for sim phases; (frame, alpha) for render
  /** Teardown before replacement (unsubscribe/clear timers). Runs on dispose OR before a run-swap that also changes mount. */
  readonly dispose?: (ctx: SystemContext) => void;
  /** Optional explicit order key within a phase (defaults to declaration order). */
  readonly order?: number;
}
```

**Hot-swap rule:** if only `run` changed between manifests (same `id`, same `phase`,
same `mount`/`dispose` identity), the runtime swaps the `run` pointer with **no
`dispose`/`mount`** — the system's subscriptions survive. If `mount`/`dispose`/`phase`
changed, the runtime runs `dispose()` → replaces → runs `mount()` (a system-scoped
soft reconcile). If the *set* of systems changed (add/remove), only the added get
`mount`, only the removed get `dispose`.

### 5.4 The hot runtime handle

```ts
export interface HotRuntime {
  /** Diff `next` against the live manifest, classify, and apply at the next tick barrier. Returns the classification. */
  readonly apply: (next: AppManifest) => UpdateClass;
  /** The live app manifest currently mounted. */
  readonly current: () => AppManifest;
  readonly dispose: () => void;
}

export type UpdateClass =
  | "hot_patch"                 // system run-bodies / resource values / prefab placements — reconcile in place
  | "soft_app_reload"           // snapshot world → rebuild app manifest → restore world (schema migration, system-set churn)
  | "full_page_reload"          // manifest invalid, or a change the runtime cannot reconcile → reload the page
  | "engine_restart_required";  // mesh geometry / instance-cap / wasm ABI change → new WasmGame

export const createHotRuntime: (game: BootGame, manifest: AppManifest, options: BootOptions) => HotRuntime;
```

`createHotRuntime` **replaces `boot()` as the primary entry** for a dev/HMR build (it
internally does the one-time `boot` wiring — host bind, loop, input, RAF — then owns
the live registries). `boot()` stays for statically-packaged production bundles.

### 5.5 Pure, testable diff + classify

Exported for unit testing against fake manifests (no wasm, no browser):

```ts
export interface ManifestDiff {
  readonly systems: { added: SystemDef[]; removed: string[]; runSwapped: SystemDef[]; remounted: SystemDef[] };
  readonly resources: { added: ResourceDef[]; removed: string[]; patched: ResourceDef[] };
  readonly components: { added: ComponentDef[]; removed: string[]; migrated: ComponentDef[]; unmigratable: string[] };
  readonly prefabs: { added: PrefabDef[]; removed: string[]; changed: PrefabDef[] };
  readonly scenes: { changed: string[] };
  readonly configChanged: boolean;      // fixedHz/seed/surface change → engine_restart_required
}
export const diffManifest:    (prev: AppManifest, next: AppManifest) => ManifestDiff;
export const classifyUpdate:  (diff: ManifestDiff) => UpdateClass;
```

**Public surface delta:** add `defineApp`, `system`, `resource`, `component`,
`prefab`, `scene`, `createHotRuntime`, `diffManifest`, `classifyUpdate`, and the
`AppManifest`/`*Def`/`*Spec`/`SystemContext`/`UpdateClass`/`ManifestDiff` types to
`packages/axiom-game/src/index.ts`. Keep `createGame`/`onFixedUpdate`/`onRender`/
`boot` exported for the migration window (§8), then deprecate the free-function path.

---

## 6. Required runtime/internal changes

### 6.1 A keyed system registry (replaces the append-only arrays)
Rework `registry.ts`: `GameRegistry` gains a **keyed** store —
`Map<string, MountedSystem>` per phase, where `MountedSystem` wraps the current `run`
pointer plus its `mount`/`dispose` and `SystemContext`. New verbs:
`replaceRun(id, run)` (hot-patch — swaps the closure, no teardown),
`upsert(def)` / `remove(id)` (mount/dispose lifecycle). The free
`onFixedUpdate`/`onRender` become thin adapters that call `upsert` with a generated id
(kept only for legacy migration). `GameLoop.advance` reads `run` pointers through this
registry so a swapped pointer takes effect on the next tick.

### 6.2 A `HotRuntime` owning long-lived state
New `packages/axiom-game/src/hot-runtime.ts`. Holds the single `WasmGame`, the
`GameLoop`, the keyed registry, the resource table, the component-schema table, and
the current `AppManifest`. It does the one-time `boot` wiring on construction and
**never** recreates the engine on `apply()` unless classification is
`engine_restart_required`. It owns the tick barrier (6.5) and the update dispatch
(6.6).

### 6.3 Stable-ID → engine-handle maps
The runtime keeps `Map<string, Entity>` (authored scene-entity id → live handle),
`Map<string, Handle>` (resource id → material/mesh handle), and
`Map<string, TextureId>`. On reconcile it looks up the existing handle by stable id
and **patches in place** instead of spawning a new one. This is what makes "scene
changes reconcile by stable ID" real (B5). Handles are still engine-minted; the map is
the *stable-name → volatile-handle* indirection the author never sees.

### 6.4 Surface `reauthor` + resource patch through the bridge
- Add `WasmGame.reauthor(...)` (`apps/axiom-game-runtime/src/wasm.rs`) that forwards to
  `GameBridge` → `RunningApp::reauthor` (R1). This is the one-shot "rebuild the scene
  from this description at a tick boundary" verb.
- Add `WasmGame.patchMaterial(handle, descriptor)` / `patchTexture(id, …)` that mutate
  an existing resource and bump the generation counter (R3) so only the changed
  resource re-uploads.
- Mirror both on the `NativeBridge` interface (`native-bridge.ts`) —
  `reauthor(scene: SceneDescriptor)`, `patchResource(handle, spec)` — so the spine and
  its fake-bridge tests can exercise reconciliation without wasm (R4).
- `restore`: add `WasmGame.restore(bytes)` forwarding to `RunningApp::restore_sim`
  (R2), used by `soft_app_reload`.

### 6.5 The tick barrier
Extend `GameLoop` (`game-loop.ts`). Add a `#pendingHotUpdates: (() => void)[]` queue
and an `enqueueHotUpdate(fn)` method. In `advance`, **after** `stepFrame` completes
this frame's fixed updates and render, drain `#pendingHotUpdates` (mirroring the
existing `#pendingStart` drain at line 71). Applying a system-run swap, a resource
patch, or a `reauthor` only happens on this boundary — never mid-`stepFrame`. This
closes B7. A hot update queued during frame F becomes active on frame F+1's first
fixed update — the "next safe tick/frame boundary" the goal requires.

### 6.6 Update classification + dispatch
`HotRuntime.apply(next)`:
1. `diff = diffManifest(current, next)` (pure).
2. `cls = classifyUpdate(diff)`.
3. Dispatch:
   - `hot_patch` → enqueue on the tick barrier: `registry.replaceRun` for each
     `runSwapped`; `mount/dispose` for `added`/`removed`; `bridge.patchResource` for
     `patched`; `bridge.reauthor` for changed prefab placements/scenes that stay within
     instance-cap and geometry constraints.
   - `soft_app_reload` → `snapshot = bridge.snapshot()`; rebuild the app manifest onto
     the same `WasmGame` (dispose all systems, re-mount from `next`, run component
     `migrate` over the snapshot), `bridge.restore(migrated)`; engine, canvas,
     textures preserved.
   - `full_page_reload` → signal the dev client to `location.reload()` (fallback).
   - `engine_restart_required` → signal the dev client to run the **legacy `load()`
     full restart** (new `WasmGame`) — the current path, now demoted to fallback.
4. Return `cls` so the dev client + overlay can report what happened.

### 6.7 Component schema table + migration
The runtime holds `Map<string, ComponentDef>` (id → `{version, fields, migrate}`).
`diffManifest` marks a component **migrated** when its `version` changed and a
`migrate(oldBytes) → newBytes` exists, **unmigratable** when the layout changed with no
`migrate`. Unmigratable ⇒ classification floors at `full_page_reload` (or
`engine_restart_required` if the kind is structurally gone). This closes B9.

---

## 7. Required Vite / dev-server changes

**Replace `scripts/axiom_dev_server.mjs`'s reload role with Vite.** Vite owns file
watching, TS/ESM transform (via esbuild — we keep tsgo only for the type-check gate,
not for dev serving), cache-busting, the module graph, and the HMR transport. The
bespoke SSE server is **demoted to a static fallback** and to serving `/pkg/` (the
wasm) + `/vendor/` if we keep serving the SDK as a sibling rather than a workspace dep.

### 7.1 `vite.config.ts` per web app
`apps/*/web/vite.config.ts`: root at `web/`, `@axiom/game` resolved to the workspace
package (`packages/axiom-game/src` in dev so its own edits hot-reload too, or `/dist`),
`/pkg/` served as a static dir (wasm is a build artifact, not transformed), and the
Axiom plugin (7.3) registered.

### 7.2 The dev client accepts modules via `import.meta.hot.accept`
The harness stops being a restart loop. New shape:

```ts
// apps/*/web/src/harness.ts  — NEW
import { createHotRuntime } from "@axiom/game";
import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";
import manifest from "./game.ts";        // the default-exported AppManifest

await initWasm();
const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
const runtime = createHotRuntime(game as BootGame, manifest, { canvas });

if (import.meta.hot) {
  import.meta.hot.accept("./game.ts", (mod) => {
    const cls = runtime.apply(mod.default);       // diff + classify + apply at tick barrier
    if (cls === "full_page_reload") import.meta.hot!.invalidate();
    // "engine_restart_required" is handled inside runtime.apply via the custom event (7.3)
  });
}
```

The engine, canvas, input, and RAF are created **once**, above the `hot.accept`. A
save re-imports only `game.ts`, and `runtime.apply` reconciles. `import.meta.hot.accept`
+ Vite's module graph give us per-module invalidation for free (B12).

### 7.3 The Axiom Vite plugin (custom HMR events, only where needed)
`packages/axiom-vite/` (a new tool, outside the engine graph). Responsibilities:
- Detect `engine_restart_required` triggers that Vite can't see (e.g. a change to a
  `component` schema or a `*.wasm` rebuild) and send a **custom HMR event**
  (`server.ws.send({ type: "custom", event: "axiom:engine-restart" })`); the dev
  client listens (`import.meta.hot.on("axiom:engine-restart", …)`) and runs the legacy
  full restart.
- Watch `/pkg/*.wasm`; on a wasm rebuild, always send `axiom:engine-restart` (ABI may
  have changed — §12).
- Optionally transform the author module to validate the `defineApp` manifest at build
  time and emit a friendly error overlay on an invalid manifest (→ `full_page_reload`).

### 7.4 SSE server: demoted, not deleted
`scripts/axiom_dev_server.mjs` stays as the **zero-dep static/fallback** server for
`make package`-style static bundles and CI smoke runs (no HMR there). Its `reload`
broadcast becomes the transport for **only** `full_page_reload`/`engine_restart_required`
when Vite is not in use. In the Vite dev path it serves nothing dynamic. The
"file-change → broadcast reload" coupling (B11) is removed from the primary loop.

---

## 8. Migration strategy (minimize thrashing, preserve demos)

**Phase 0 — internal seams, no author-visible change.**
- Add `WasmGame.reauthor`/`restore`/`patchMaterial` (§6.4) + the `NativeBridge`
  reconciliation verbs + fake-bridge tests. Nothing consumes them yet. Ships green.

**Phase 1 — keyed registry under the old surface.**
- Rework `GameRegistry` to keyed storage (§6.1) with `onFixedUpdate`/`onRender` as
  legacy adapters that synthesize ids. Behavior identical; existing `game.ts` files
  unchanged. Adds `replaceRun`/`upsert`/`remove` + tests.

**Phase 2 — the tick barrier + `HotRuntime` (no manifest yet).**
- Add `#pendingHotUpdates` to `GameLoop` (§6.5). Add `createHotRuntime` that wraps the
  current `boot` wiring and, for now, only supports `engine_restart_required`
  (delegates to a full rebuild) — a drop-in for the harness `load()` that centralizes
  lifetime ownership. Convert both harnesses to use it. Still full-restart behavior,
  but the engine-owns-lifetime inversion (B1) is done.

**Phase 3 — `defineApp` manifest + diff/classify.**
- Add `defineApp` + definition factories + `diffManifest`/`classifyUpdate` (pure,
  fully tested). Port **one** demo (`axiom-game-runtime`'s orb `game.ts`) to the
  manifest shape as the reference. Keep the free-function demos working via the legacy
  adapter. First real `hot_patch` (system-run swap) works end to end.

**Phase 4 — Vite + `import.meta.hot`.**
- Add `apps/axiom-game-runtime/web/vite.config.ts` + the Axiom Vite plugin (§7). Switch
  that app's dev command to Vite. Prove system-body hot-patch with live world state.
  Leave `scripts/axiom_dev_server.mjs` for the other apps until they migrate.

**Phase 5 — resource/scene/component reconciliation + soft reload.**
- Wire `patchResource`, `reauthor`-backed scene reconcile, snapshot/restore soft
  reload, and component migration. Port the retro-FPS TS app. Retire the free-function
  authoring path (keep `boot()` for static production bundles only).

Each phase is independently green (branchless/coverage/ts-gate). Demos keep running:
the legacy adapter means an un-migrated `game.ts` still boots (as a full-restart app)
until its owner ports it.

---

## 9. First implementation slice (the concrete target scenario)

**Scenario:** edit a TypeScript system body in `apps/axiom-game-runtime/web/src/game.ts`,
save, and the running app uses the new function **on the next fixed tick** — without
recreating the `WasmGame`, world, canvas, or app.

**Smallest end-to-end vertical that proves it** (subset of Phases 1-4, one demo, 2D
only, no resource/component/scene reconcile yet):

1. **Keyed registry (§6.1, minimal):** `GameRegistry` gains
   `upsertSystem(id, phase, run)` and `replaceRun(id, run)`; `fixedUpdates()`/
   `renders()` read the current `run` pointers. `onFixedUpdate`/`onRender` synthesize
   ids so nothing else breaks. **Tests:** replace-run swaps the pointer; order
   preserved.
2. **Tick barrier (§6.5, minimal):** `GameLoop` gains `#pendingHotUpdates` +
   `enqueueHotUpdate`, drained after `stepFrame` in `advance`. **Test:** an update
   enqueued during frame F's callbacks runs against frame F+1, never mid-frame.
3. **`defineApp` (§5.1) — systems only:** `defineApp({ id, config, systems })` +
   `system(id, { phase, run })`. `diffManifest`/`classifyUpdate` implemented for the
   **systems-only** case (run-swap ⇒ `hot_patch`; add/remove ⇒ `hot_patch` with
   mount/dispose; config change ⇒ `engine_restart_required`). **Tests:** pure diff +
   classify tables.
4. **`createHotRuntime` (§5.4, minimal):** does the one-time `boot` wiring, holds the
   `WasmGame` + registry + manifest; `apply(next)` handles only `hot_patch`
   (enqueue `replaceRun`) and `engine_restart_required` (delegate to legacy `load()`).
5. **Port the orb demo `game.ts`** to `export default defineApp({ … systems: [
   system("orb.spin", {phase:"fixedUpdate", run}), system("orb.draw", {phase:"render",
   run}) ] })`. Move the module-level `let phase/tick/shimmer` **into the ECS world**
   (spawn one "ring" entity carrying a `spin` component; the system reads/writes it via
   `world`) so state survives the swap (fixes B2 for this demo).
6. **Vite dev path (§7.1-7.2):** `vite.config.ts` for the app + the `import.meta.hot.accept("./game.ts", m => runtime.apply(m.default))` harness. `/pkg/` served static.
7. **Proof (browser):** run the app, let the tick counter climb, edit `orb.spin`'s
   `run` (e.g. change `SPIN_PER_TICK`), save. Assert in the Playwright controller that
   (a) the tick counter **did not reset to 0**, (b) the new rotation rate is visible,
   (c) `WasmGame` was constructed exactly once (expose a construct-count on `window`
   for the test).

This slice touches only: `registry.ts`, `game-loop.ts`, new `manifest.ts` +
`diff.ts` + `hot-runtime.ts`, the app's `game.ts` + `harness.ts` + new
`vite.config.ts`. It defers resources, prefabs, scenes, components, soft-reload, and
the retro-FPS 3D app to later phases.

---

## 10. Files to change (checklist)

**New — SDK core (`packages/axiom-game/src/`):**
- `manifest.ts` — `defineApp`, `system`/`resource`/`component`/`prefab`/`scene`
  factories, `AppManifest` + `*Def`/`*Spec` types, `SystemPhase`, `SystemContext`.
  *Why:* the declaration layer (B4) — the thing HMR diffs.
- `diff.ts` — `diffManifest`, `classifyUpdate`, `ManifestDiff`, `UpdateClass`.
  *Why:* pure, fully-testable change classification (§5.5, §6.6).
- `hot-runtime.ts` — `createHotRuntime`, `HotRuntime`. *Why:* the long-lived owner of
  engine + registries across reloads (B1, §6.2).

**Modified — SDK core:**
- `registry.ts` — keyed system store; `upsertSystem`/`replaceRun`/`remove`; legacy
  `onFixedUpdate`/`onRender` adapters. *Why:* replaceable systems (B3, §6.1).
- `game-loop.ts` — `#pendingHotUpdates` + `enqueueHotUpdate` + drain in `advance`;
  read `run` pointers through the keyed registry. *Why:* the tick barrier (B7, §6.5).
- `loop-core.ts` — `FixedUpdate`/`Render` gain no signature change, but `stepFrame`
  reads the run list from the keyed registry snapshot taken at frame start. *Why:*
  a swap mid-frame must not take effect until the barrier.
- `native-bridge.ts` — add `reauthor(scene)`, `patchResource(handle, spec)`,
  `restore(bytes)` verbs. *Why:* the reconciliation seam the spine + fake-bridge tests
  use (B10, §6.4, R4).
- `boot.ts` — factor the one-time wiring so `createHotRuntime` reuses it; `boot()`
  stays for static bundles. *Why:* single wiring path, two entry points.
- `index.ts` — export the new surface (§5). *Why:* public API.

**Modified — wasm runtime (`apps/axiom-game-runtime/src/`):**
- `wasm.rs` — add `#[wasm_bindgen]` `reauthor`, `restore`, `patchMaterial`/
  `patchTexture` on `WasmGame`. *Why:* surface the stranded engine primitive (B10).
- `bridge.rs` — `GameBridge::reauthor` → `RunningApp::reauthor`; `restore` →
  `restore_sim`; resource patch → mutate + bump generation. *Why:* forward to the
  engine (R1, R2, R3).

**New — dev tooling (outside the engine graph):**
- `packages/axiom-vite/` — the Axiom Vite plugin (custom `axiom:engine-restart` event,
  wasm-watch, manifest validation). *Why:* HMR events Vite can't infer (§7.3).
- `apps/axiom-game-runtime/web/vite.config.ts` (and per app) — Vite dev config.
  *Why:* file watch + ESM + module graph + HMR transport (§7.1).

**Modified — apps:**
- `apps/axiom-game-runtime/web/src/harness.ts` — `createHotRuntime` +
  `import.meta.hot.accept`; engine created once. *Why:* the dev client (B1, §7.2).
- `apps/axiom-game-runtime/web/src/game.ts` — port to `export default defineApp(...)`,
  move sim state into the world. *Why:* reference manifest app (B2, B13).
- `apps/axiom-retro-fps-ts-browser/web/{harness,game}.ts` — same port (Phase 5).
- `scripts/axiom_dev_server.mjs` — keep as static/fallback only; drop its role as the
  primary reload trigger. *Why:* demote SSE full-restart (B11, §7.4).
- `Makefile` — add a `game-runtime-dev` (Vite) target alongside the existing static
  ones. *Why:* the new dev command.

---

## 11. Test plan

**Unit — diff/classification (pure, no wasm, no browser; `packages/axiom-game`):**
- `diff.test.ts`: run-body-only change ⇒ `runSwapped` populated, everything else empty.
- system added / removed ⇒ correct `added`/`removed`; classify ⇒ `hot_patch`.
- resource value change ⇒ `patched`; classify ⇒ `hot_patch`.
- component `version` bump **with** `migrate` ⇒ `migrated`; classify ⇒ `soft_app_reload`.
- component layout change **without** `migrate` ⇒ `unmigratable`; classify ⇒
  `full_page_reload`.
- `config.fixedHz`/`seed`/`surface` change ⇒ `configChanged`; classify ⇒
  `engine_restart_required`.
- invalid manifest (missing `id`, duplicate system id) ⇒ `full_page_reload`.
- **100% coverage** (Coverage Law): every classification arm is a distinct region.

**Unit — keyed registry + tick barrier (fake bridge):**
- `replaceRun` swaps the pointer; the next `stepFrame` calls the new closure, the same
  frame's already-started fixed updates do not.
- `enqueueHotUpdate` drains **after** the frame's last fixed update, never between the
  N fixed updates of one `advance`.
- `upsert` runs `mount` once; `remove` runs `dispose` once; a run-only swap runs
  neither.

**Integration — system replacement over a fake bridge (`hot-runtime.test.ts`):**
- Build a `HotRuntime` on a fake `WasmGame`. Advance 100 ticks. `apply(manifestV2)`
  with a changed `orb.spin` run. Advance one more frame. Assert: the new run executed,
  the tick counter continued (no reset), the fake `WasmGame` construct-count is 1,
  world state set before the swap is still present.
- `apply` with a component migration: assert `snapshot`→`migrate`→`restore` called in
  order and the migrated bytes reach `restore`.

**Rust — wasm surface (`apps/axiom-game-runtime`):**
- `reauthor_replaces_scene_keeps_engine`: construct `GameBridge`, advance, `reauthor`,
  assert tick preserved + renderable count changed (mirrors the existing
  `app.rs:922` test one layer up).
- `patch_material_bumps_generation_only`: patch → generation +1, mesh generation
  unchanged.

**Browser/dev — state survives a system edit (Playwright controller, §9 proof):**
- `scripts/playwright_controller.py`: `goto` the Vite dev URL, `wait`, read tick via
  `eval`. Rewrite `game.ts`'s `orb.spin` body on disk, `wait` for HMR, `eval` the tick
  again → **strictly greater, never reset**; `eval window.__wasmGameConstructCount`
  → `1`; `screenshot` before/after shows the new behavior. This is the acceptance test
  for the whole slice.

Apps/tooling are outside the 100% gate but ship with these integration/browser tests
(per the Coverage Law's app/tooling scope line).

---

## 12. Risks and hard boundaries (still trigger a full reload / restart)

These are **not** hot-patchable and must classify to `engine_restart_required` (new
`WasmGame`) or `full_page_reload`:

- **WASM ABI / `*.wasm` rebuild** → `engine_restart_required`. Any rebuild of
  `axiom_game_runtime_bg.wasm` may change the exported surface or memory layout; the JS
  glue and `WasmGame` must be reconstructed. The Vite plugin watches `/pkg/*.wasm` and
  forces `axiom:engine-restart` unconditionally (§7.3).
- **Mesh geometry change** → `engine_restart_required`. `RunningApp::reauthor`
  explicitly **cannot** change base mesh geometry (`app.rs:399` — "the live windowing
  backend's vertex buffer is fixed at startup"). A prefab that changes a mesh's vertex
  data, or exceeds the instance-buffer capacity the surface was bound with
  (`bindSurface`'s `max_instances`), needs a new engine bind.
- **`GameConfig` change** (`fixedHz`, `seed`, `surface`) → `engine_restart_required`.
  Seed keys the RNG hub before tick 0 (`wasm.rs:155`); fixedHz sets the accumulator;
  surface identity binds the backend. None are re-seedable in place.
- **Component schema change without `migrate`** → `full_page_reload` (or
  `engine_restart_required` if the kind is structurally removed). Components cross as
  raw `(kind, bytes)` (B9); a layout change with no migrator would mis-decode live
  columns. Refuse to reconcile.
- **Invalid manifest** (missing/duplicate stable id, malformed `defineApp`, a system
  with no `run`) → `full_page_reload` with a Vite error overlay. Never apply a partial
  or ambiguous manifest to a live engine.
- **Unsafe lifecycle change** — a system whose `mount` established host-level state
  (input bindings, net subscriptions) changing its `mount`/`dispose` identity → runs
  `dispose`→`mount` (system-scoped reconcile), and if that fails, floors to
  `soft_app_reload`. A `run`-only swap never touches lifecycle and is always safe.
- **SDK protocol change** — a change to `@axiom/game`'s own `NativeBridge`/manifest
  contract (not just an app edit) → `full_page_reload`; the runtime and app must agree
  on the contract version. (In dev, editing the SDK source is itself an
  `import.meta.hot` module; a contract-shape change invalidates the whole graph.)

The guiding rule: **prefer the smallest safe update, but never trade correctness for
speed.** When the diff is ambiguous or a migrator is missing, classify *up* (toward
full reload), never down. A wrong hot-patch that corrupts live world state is exactly
the "temporary mistake wearing a fake mustache" the No-Shortcuts rule forbids — the
fallback restart path exists precisely so reconciliation can stay conservative.

---

## Appendix — key file:line references

| Fact | Location |
|---|---|
| SSE dev server, reload broadcast | `scripts/axiom_dev_server.mjs:67,162-172` |
| 2D harness full-restart `load()` | `apps/axiom-game-runtime/web/src/harness.ts:134-155` |
| 3D harness full-restart `load()` | `apps/axiom-retro-fps-ts-browser/web/src/harness.ts:69-97` |
| Author state in module globals | `apps/axiom-game-runtime/web/src/game.ts:24,42-45` |
| Append-only callback registry | `packages/axiom-game/src/registry.ts:21-44,62-69` |
| `createGame` mints fresh registry | `packages/axiom-game/src/game.ts:94-98` |
| Loop reads callbacks live; `#pendingStart` one-shot | `packages/axiom-game/src/game-loop.ts:49,68-87` |
| `NativeBridge` seam (no reconcile verbs) | `packages/axiom-game/src/native-bridge.ts:52` |
| Anonymous entity spawn | `packages/axiom-game/src/game-object.ts:203`, `world.ts:57` |
| Engine-minted mesh/material handles | `packages/axiom-game/src/scene3d.ts:30,91` |
| `WasmGame` fields (all per-instance) | `apps/axiom-game-runtime/src/wasm.rs:98-130` |
| `WasmGame` exposes only `snapshot` | `apps/axiom-game-runtime/src/wasm.rs:272` |
| `RunningApp::reauthor` (the keystone) | `modules/axiom/src/app.rs:403-414` |
| `snapshot_sim` / `restore_sim` | `modules/axiom/src/app.rs:428-438` |
| Generation-gated re-upload pattern | `apps/axiom-game-runtime/src/wasm.rs:117,126`; `bridge.rs:146` |
| In-place texture patch by id | `apps/axiom-game-runtime/src/wasm.rs:363-371` |
| `clearScene` → `reauthor` in place (wasm) | `apps/axiom-game-runtime/src/scene3d.rs:217-222` |
| Texture URL→handle dedup (content id) | `apps/axiom-game-runtime/src/assets.rs:47-54` |
| No wasm-side global state (all per-`WasmGame`) | grep: 0 `thread_local!`/`static`/`OnceCell` in crate |
| Native reauthor-in-place precedent | `apps/axiom-gallery/src/retro_fps/mod.rs:767`; `web.rs:198` |
