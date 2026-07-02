# axiom-workspace

A developer-facing **workspace app** for Axiom: a modular, twelve-panel
workbench for opening a game project, browsing its manifest/levels/assets,
launching and observing a real runtime session, inspecting it, recording and
replaying it, and packaging it for export.

It **launches, observes, inspects, records, replays, and packages** real
runtime sessions by composing engine layers. It is **not** the runtime and
implements **no game behavior** — it is a composition-leaf app, and nothing in
the engine depends on it.

> Real runtime integration is deliberately deferred behind typed panel state
> contracts until the public engine APIs each panel needs exist. See
> [`ARCHITECTURE.md`](ARCHITECTURE.md) for the placement rationale and the
> list of missing engine contracts, and [`TESTING.md`](TESTING.md) for the
> test map.

## What this is

A modular workspace shell: each of twelve panels has one Rust state contract
and one browser render function, wired through a single typed shell state and
a single explicit event reducer. Concretely, the current scaffold has:

- **Twelve typed panels** (list below), each a self-contained Rust module plus
  a self-contained browser panel file.
- **Modular Rust contracts.** No shared mutable state between panels — each
  panel's state lives in its own type, constructed and validated through the
  one public facade, `WorkspaceApi`.
- **A vanilla browser shell.** TypeScript/HTML/CSS, no framework, no iframe;
  typed `WorkspaceState` + typed `WorkspaceEvent`s flowing through one reducer,
  `applyWorkspaceEvent(state, event)`.
- **Placeholder-only behavior.** Every panel renders real, typed shell state,
  but none of them yet drives a real runtime, real assets, real input, real
  logs, or a real build — those all wait on public engine contracts that do
  not exist yet (see `ARCHITECTURE.md`).

## Panels

| Panel id              | What it will show                                   |
|-------------------------|------------------------------------------------------|
| `project-browser`      | Known/openable game projects                          |
| `game-manifest-editor`  | The opened project's manifest fields                  |
| `level-browser`        | The opened project's levels                            |
| `runtime-viewport`     | A labeled placeholder for the live runtime view        |
| `object-inspector`     | The selected entity/object's state                     |
| `asset-browser`        | The opened project's assets                            |
| `console-log-viewer`   | Structured log output                                  |
| `profiler`             | Profiling samples                                      |
| `input-debugger`       | Captured input records                                 |
| `timeline-replay`      | A scrubber over a recorded session                      |
| `play-controls`        | Start/pause/step/stop for a launched session            |
| `package-export`       | Export/build target and progress                        |

Panel ids are stable kebab-case strings; the shell's five modes
(`Edit`/`Play`/`DropIn`/`Replay`/`Package`) describe which workflow the shell
is presenting — not what the (currently absent) engine integration is doing.
See `ARCHITECTURE.md` for the full panel-to-region mapping and the future
engine contract each panel is waiting on.

## Layout

```text
apps/axiom-workspace/
  app.toml                     # app manifest: allowed_layers = [kernel, runtime]
  Cargo.toml
  src/
    lib.rs                     # crate root; re-exports WorkspaceApi + the typed vocabulary
    workspace_api.rs           # WorkspaceApi — the single public facade
    workspace_manifest.rs      # WorkspaceManifest
    game_project.rs            # GameProject
    launch_spec.rs             # LaunchSpec
    drop_in_spec.rs            # DropInSpec + DropLevel (Drop In)
    play_session.rs            # PlaySession + PlaySessionStatus
    session_record.rs          # SessionRecord + RecordedInput + RecordedSnapshot
    replay_request.rs          # ReplayRequest
    workspace_panel.rs         # WorkspacePanel / WorkspacePanelId / WorkspacePanelState /
                                #   WorkspaceRegion / WorkspaceLayoutSnapshot / WorkspaceMode
    project_browser.rs         # ProjectBrowserState
    game_manifest_editor.rs    # GameManifestEditorState
    level_browser.rs           # LevelBrowserState
    runtime_viewport.rs        # RuntimeViewportState
    object_inspector.rs        # ObjectInspectorState
    asset_browser.rs           # AssetBrowserState
    console_log_viewer.rs      # ConsoleLogViewerState
    profiler_panel.rs          # ProfilerPanelState
    input_debugger.rs          # InputDebuggerState
    timeline_replay.rs         # TimelineReplayState
    play_controls.rs           # PlayControlsState
    package_export.rs          # PackageExportState
  tests/
    workspace_contracts.rs     # typed contract tests
    architecture.rs            # boundary tests (leaf app, no browser API in Rust, …)
  web/                         # vanilla TS/HTML/CSS shell (no framework, no iframe)
    index.html
    README.md                  # shell-specific notes
    src/
      main.ts                  # initial state → mount → dispatch → re-render
      workspace_state.ts       # typed WorkspaceState + applyWorkspaceEvent reducer
      workspace_events.ts      # typed WorkspaceEvent union
      workspace_layout.ts      # panel id -> WorkspaceRegion mapping
      panel_registry.ts        # the twelve panels the shell mounts
      dom_mount.ts             # mounts each region's panels into its DOM slot
      panels/
        project_browser_panel.ts
        game_manifest_editor_panel.ts
        level_browser_panel.ts
        runtime_viewport_panel.ts
        object_inspector_panel.ts
        asset_browser_panel.ts
        console_log_viewer_panel.ts
        profiler_panel.ts
        input_debugger_panel.ts
        timeline_replay_panel.ts
        play_controls_panel.ts
        package_export_panel.ts
    styles/workspace.css       # square, dense, light theme
```

## Running validation

```sh
cargo test --workspace
cargo xtask check-architecture
```

The first runs this crate's tests (typed contracts, panel/ordering tests,
architecture-boundary tests) plus the repo-wide architecture test; the second
validates `app.toml` classification and the layer/module dependency graph.
See [`TESTING.md`](TESTING.md) for what each test category covers and what is
intentionally left untested (placeholder-only behavior).

## More detail

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — why this crate is an app, not a layer
  or module; the panel-to-region table; the single-facade rule; what each
  workspace mode means; and the full list of future engine contracts each
  panel is waiting on.
- [`TESTING.md`](TESTING.md) — the test map, including panel contract tests,
  insertion-order tests, architecture-boundary tests, and the browser smoke
  check.
- [`web/README.md`](web/README.md) — how to serve and smoke-test the browser
  shell.
