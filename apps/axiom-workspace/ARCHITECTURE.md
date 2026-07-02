# axiom-workspace — Architecture

## What this crate is

`axiom-workspace` is an **app / composition root**. It is a leaf in the Axiom
dependency graph: an application that composes engine layers/modules, and that
nothing else in the engine is allowed to depend on.

It is **not**:

- **not an engine layer** — it has no `layer.toml`, declares no `depends_on`
  edge in the layer DAG, and adds no reusable engine capability.
- **not an engine module** — it has no `module.toml` and exposes no isolated
  capability for other crates to compose.
- **not the runtime** — it does not step a simulation, own a game loop, or
  advance a clock.
- **not a game** — it implements **zero game behavior**. It contains no
  physics, rendering backend, ECS, scene system, asset importer, audio,
  animation, plugin system, networking, or gameplay simulation. Nothing in
  this crate is a stand-in "fake runtime" for the real one.

## What it does

The workspace is a developer-facing shell that **launches, observes, inspects,
records, replays, and packages** real runtime sessions by composing engine
layers/modules — it never becomes a second game engine. The shell is a
modular, twelve-panel workbench: each panel is a typed placeholder view onto a
future engine contract, wired through one shared shell state and one explicit
event reducer.

The underlying workflow vocabulary is typed and canonical:

| Concept             | Contract (`src/`)                    | Role                                              |
|---------------------|--------------------------------------|---------------------------------------------------|
| Open a workspace    | `WorkspaceManifest`                  | which workspace, where, at what schema version    |
| Open a project      | `GameProject`                        | *what* project/game is being opened               |
| Describe a launch   | `LaunchSpec`                         | exact project/version/config/entrypoint to launch |
| Drop In             | `DropInSpec` (attached to a spec)    | editor-derived spawn context                      |
| A launched session  | `PlaySession` / `PlaySessionStatus`  | a handle to a real runtime session (observed)     |
| Record a session    | `SessionRecord` + `RecordedInput` / `RecordedSnapshot` | ordered input + snapshot-hash artifacts |
| Replay a session    | `ReplayRequest`                      | points a future replayer at a prior record        |
| Shell layout        | `WorkspacePanel` / `WorkspacePanelId` / `WorkspacePanelState` / `WorkspaceRegion` / `WorkspaceLayoutSnapshot` | the twelve-panel workbench and where each panel sits |
| Shell mode          | `WorkspaceMode`                      | which workflow the shell is currently presenting  |

`WorkspaceApi` (in `src/workspace_api.rs`) is the single public facade that
constructs and validates every one of these.

## Panels are modular: one file, one state contract, one region

Each of the twelve panels owns exactly one Rust module with one clear typed
state contract, and exactly one browser panel file with one render function.
No panel reaches into another panel's state, and no panel owns more than the
slice of the shell it renders.

The table below is in the canonical stable order (`WorkspacePanelId::ALL`),
and its `Region` column is the exact region `WorkspacePanel::for_id` assigns in
`src/workspace_panel.rs` — the source of truth.

| Panel id                | Rust state type            | Web panel file                    | Region   |
|--------------------------|------------------------------|--------------------------------------|----------|
| `project-browser`        | `ProjectBrowserState`        | `project_browser_panel.ts`           | Left     |
| `game-manifest-editor`   | `GameManifestEditorState`    | `game_manifest_editor_panel.ts`      | Left     |
| `level-browser`          | `LevelBrowserState`          | `level_browser_panel.ts`             | Left     |
| `runtime-viewport`       | `RuntimeViewportState`       | `runtime_viewport_panel.ts`          | Center   |
| `object-inspector`       | `ObjectInspectorState`       | `object_inspector_panel.ts`          | Right    |
| `asset-browser`          | `AssetBrowserState`          | `asset_browser_panel.ts`             | Right    |
| `package-export`         | `PackageExportState`         | `package_export_panel.ts`            | Right    |
| `play-controls`          | `PlayControlsState`          | `play_controls_panel.ts`             | Bottom   |
| `timeline-replay`        | `TimelineReplayState`        | `timeline_replay_panel.ts`           | Bottom   |
| `console-log-viewer`     | `ConsoleLogViewerState`      | `console_log_viewer_panel.ts`        | Bottom   |
| `profiler`               | `ProfilerPanelState`         | `profiler_panel.ts`                  | Bottom   |
| `input-debugger`         | `InputDebuggerState`         | `input_debugger_panel.ts`            | Bottom   |

The canonical region grouping is therefore:

- **Left** — `project-browser`, `game-manifest-editor`, `level-browser`
- **Center** — `runtime-viewport`
- **Right** — `object-inspector`, `asset-browser`, `package-export`
- **Bottom** — `play-controls`, `timeline-replay`, `console-log-viewer`,
  `profiler`, `input-debugger`

Panel ids are stable, kebab-case strings (`WorkspacePanelId`) — never
renumbered, and never reused for a different panel. `WorkspacePanel` pairs an
id with the `WorkspaceRegion` it renders into; `WorkspacePanelState` is the
closed set of per-panel state values a `WorkspaceLayoutSnapshot` carries, one
per panel, in the table's fixed order. `WorkspaceRegion` is the small, closed
set of layout slots above (`Left`, `Center`, `Right`, `Bottom`) — a coarse
docking position, not a pixel-precise layout engine; the browser shell owns the
actual CSS placement within a region (its `WorkspaceRegion` string union in
`web/src/workspace_layout.ts` — `"left" | "center" | "right" | "bottom"` —
mirrors these four names).

## The single-facade rule

`WorkspaceApi` is the crate's one public behavioral facade — every panel
default, every shell contract, and every mutation helper is reached through it
or through the value types it returns. Alongside the facade, `lib.rs`
re-exports the typed **vocabulary** the facade traffics in: the twelve panel
state types above, `WorkspaceMode`, `WorkspacePanel`, `WorkspacePanelId`,
`WorkspacePanelState`, `WorkspaceRegion`, `WorkspaceLayoutSnapshot`, and the
pre-existing scaffold contracts (`WorkspaceManifest`, `GameProject`,
`LaunchSpec`, `DropInSpec`, `DropLevel`, `PlaySession`, `PlaySessionStatus`,
`SessionRecord`, `RecordedInput`, `RecordedSnapshot`, `ReplayRequest`). These
are pure value types — the nouns the facade hands back — not a second
behavioral surface. No other top-level `pub`/`pub use` belongs in `lib.rs`.

## `WorkspaceMode` describes the shell, not engine behavior

```text
WorkspaceMode::Edit     — browsing/editing project, manifest, levels, assets
WorkspaceMode::Play     — a live runtime session is (will be) owned and observed
WorkspaceMode::DropIn   — Edit plus an attached DropInSpec, ready to launch into
WorkspaceMode::Replay   — a recorded session is (will be) played back
WorkspaceMode::Package  — export/build orchestration is (will be) in progress
```

`WorkspaceMode` is presentation state: it selects which panels are
foregrounded and which workflow the toolbar exposes. It never causes the
crate to simulate, render, or step anything — switching to `Play` does not
start a game loop, switching to `Replay` does not decode a record. The mode
names describe what the *shell* is showing the user, not what the engine is
doing, because today the engine is not doing anything here at all (see
"Future engine contracts needed" below).

## Runtime Viewport is a placeholder

The `runtime-viewport` panel (`RuntimeViewportState`) renders a
clearly-labeled **"Runtime Viewport Placeholder"**. It embeds no game
surface, draws no frame, and owns no GPU/canvas resource, because the host
contract for a real runtime viewport does not exist yet. `RuntimeViewportState`
carries only the placeholder's own display data (the label text, a
"not yet connected" flag, and which placeholder *view* is selected) — never a
frame buffer, canvas handle, or render command list. When a public
embeddable-viewport contract exists in a layer/module, this panel is the seam
it attaches to; until then it stays honestly empty.

The viewport is **toggleable** between two placeholder views
(`RuntimeViewportView`), switched by a tab bar in the shell and driven through
the single reducer (event `viewport.tab.set`):

- `Placeholder` — the plain "Runtime Viewport Placeholder" surface.
- `BackendTriptych` — a placeholder that names the same three render backends
  the demo gallery compares side by side: WebGPU, WebGL2, and Canvas2D
  (`ComparisonBackend`, canonical fixed data in the engine's own backend
  preference order). The **live** comparison — one deterministic demo (e.g. the
  retro_fps demo) rendered through all three backends at once — lives in the demo
  gallery (`apps/axiom-gallery/web/triptych.html`), which owns the real render
  surfaces and pins each pane to a backend via `?backend=`. The workspace only
  mirrors that comparison as labeled placeholder data; it embeds no live
  surface and no nested browsing context, so the app's no-embedded-frame rule
  is preserved. Attaching the real surfaces is the same future
  embeddable-viewport contract noted above.

## Drop In: a launch contract plus editor-derived spawn context

**Drop In = a canonical `LaunchSpec` + an editor-derived `DropInSpec`.** The
launch spec says *what* to launch (project, version, entrypoint, fixed step);
the drop context says *where to spawn into* — a level id, a world position and
yaw (typically camera-derived in an editor), and an optionally selected entity.
Attaching a drop context is **additive**: `LaunchSpec::identity()` is a
`StableHash` over the launch-identity fields only, so dropping in never changes
the launch identity. That is what lets the same launch be replayed with or
without a drop context and still be recognized as the same launch. In the
shell, entering `WorkspaceMode::DropIn` attaches a `DropInSpec` derived from
the current `level-browser` selection and `object-inspector` selection — it
does not fork a second launch identity.

## Play Mode is future real runtime ownership, not fake simulation

`WorkspaceMode::Play` and the `play-controls` panel (`PlayControlsState`)
describe the intent to launch a `LaunchSpec` into a real `axiom-runtime`
session and own its lifecycle (start/pause/step/stop) from the shell. Today
`PlayControlsState` stores only the placeholder button/status state a panel
needs to render — it does not step a clock, does not advance a tick, and does
not synthesize simulated frames. Building a fake per-tick simulator inside
the workspace to make Play Mode "look alive" would be exactly the
second-engine mistake this crate exists to avoid; Play Mode stays inert until
the launch-to-runtime-construction contract (below) is public.

## Replay Mode is future recorded input/session playback

`WorkspaceMode::Replay` and the `timeline-replay` panel
(`TimelineReplayState`) describe the intent to take a `ReplayRequest`,
re-launch its `LaunchSpec`, and re-drive the `SessionRecord`'s recorded
inputs tick by tick, scrubbing through them and comparing resulting snapshot
hashes against `expected_digest`. Today `TimelineReplayState` holds only the
ordered, placeholder tick markers a scrubber UI needs to render — it does not
decode or replay real input. This waits on the same replay-drive contract
already listed as a future integration point below.

## Package Mode is future export/build orchestration

`WorkspaceMode::Package` and the `package-export` panel
(`PackageExportState`) describe the intent to drive a real build/export
pipeline (asset baking, platform packaging, artifact output) from the shell
and surface its progress and result. Today `PackageExportState` holds only
placeholder target/status fields — it triggers no build, invokes no external
tool, and writes no artifact. This waits on a public package/export runner
contract (below).

## Why the workspace must not be imported by engine code

Workspace, panel, mode, editor, launch, drop-in, record, replay, and package
are *tooling* concepts. Putting any of them into `axiom-kernel`,
`axiom-runtime`, or any layer/module would pull editor policy into the
reusable engine spine — exactly the inward leak the Layer Law and Module Law
forbid ("no editor concepts inward"). The engine stays a black box the
workspace *observes*; the workspace stays a leaf that composes it. This keeps
the engine reusable by a future headless harness, a native app, and a CI
replayer that never open a workspace at all. It is mechanically enforced: no
layer or module may declare or exercise a dependency on `axiom-workspace`
(`check-architecture`'s `AppImportedBySomething` rule and `tests/architecture.rs`).

## Future engine contracts needed

Real runtime integration is intentionally **not** implemented anywhere in
this crate, because the public engine contracts it requires do not yet exist
cleanly. Each item below marks the seam a future agent will attach to, and the
panel(s) it unblocks:

1. **Project/game manifest loading.** A public contract that resolves a
   `GameProject`/`WorkspaceManifest` from real on-disk or packaged data
   (`project-browser`, `game-manifest-editor`).
2. **Asset manifest resolution.** A public asset-listing contract
   (`asset-browser`) — today `AssetBrowserState` holds placeholder rows only.
3. **Scene snapshot inspection.** A public contract exposing a live or
   recorded scene's entities/components for read-only inspection
   (`object-inspector`, and `level-browser`'s level listing).
4. **Render viewport embedding.** A host/windowing presentation surface the
   shell can observe (`runtime-viewport`; see "Runtime Viewport is a
   placeholder" above).
5. **Input stream capture.** A public input-event contract to turn real
   input into `RecordedInput` (`input-debugger`, and session recording).
6. **Structured logs.** A public log-sink contract the shell can subscribe to
   (`console-log-viewer`) instead of only showing shell-local messages.
7. **Profiler samples.** A public profiling-sample contract (`profiler`) —
   today `ProfilerPanelState` holds no real timing data.
8. **Replay/session record loading.** A public contract to load a persisted
   `SessionRecord` for scrubbing (`timeline-replay`).
9. **Package/export runner.** A public build/export orchestration contract
   (`package-export`; see "Package Mode" above).

When those contracts land as legal, public layer/module APIs, they get added
to `app.toml`'s `allowed_layers`/`allowed_modules` and consumed through
`WorkspaceApi` — never by widening the engine to know about the workspace.

## Dependency stance

Declared in `app.toml`:

```toml
allowed_layers = ["kernel", "runtime"]
allowed_modules = []
```

- **kernel** — the workspace's contracts are built from kernel primitives:
  `Tick` (tick/frame markers), `EntityId` (selected entity), `StableHash`
  (launch identity, snapshot hashes, record digests), `SchemaVersion`
  (manifest), and `Meters`/`Radians` (drop-in spawn transform, units explicit
  at the boundary).
- **runtime** — a `LaunchSpec` defaults its deterministic fixed step to
  `RuntimeConfig::DEFAULT_FIXED_STEP_NANOS`, so the launch config the workspace
  hands out agrees with the runtime it will eventually drive.
- **no engine module** — the workspace deliberately depends on **no** module
  yet, in Rust or in the twelve panels above. It does not guess at future
  module contracts; it defers real integration behind the placeholder panel
  states and the "Future engine contracts needed" list.

Because the workspace is an app, **no layer or module may import it**
(mechanically enforced by `check-architecture`'s `AppImportedBySomething` rule
and by `tests/architecture.rs`).

## The browser shell (`web/`)

A minimal, vanilla **TypeScript / HTML / CSS** shell — no framework, no
iframe, no bundler assumptions — presents the twelve panels in a light,
square, dense theme. The shell's own architecture mirrors the Rust crate's
discipline:

- **Typed panel state.** Each panel file (`web/src/panels/*_panel.ts`) owns
  one render function taking that panel's slice of shell state and returning
  DOM updates — never reaching into another panel's state.
- **Typed events, one reducer.** All shell mutation flows through a single
  explicit function, `applyWorkspaceEvent(state, event)`
  (`web/src/panels/workspace_events.ts` / `workspace_state.ts`), that takes
  the current typed `WorkspaceState` and a typed `WorkspaceEvent` and returns
  the next `WorkspaceState`. There is no ambient global mutation: a button
  click, a mode switch, or a panel selection all construct a typed event and
  dispatch it through this one reducer.
- **Regions and layout.** `workspace_layout.ts` maps each panel id to its
  `WorkspaceRegion` (mirroring the Rust table above), `panel_registry.ts`
  enumerates the twelve panels the shell mounts, and `dom_mount.ts` mounts
  each region's panels into its DOM slot. `main.ts` wires it together:
  initial state → mount → event dispatch → re-render.

This is the same shape as the Rust crate for the same reason: one clear state
contract per panel, one explicit place mutation happens, and no ambient
coupling between panels.
