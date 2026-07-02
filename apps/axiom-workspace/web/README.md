# Axiom Workspace — Browser Shell

This directory is the **browser shell** for the `axiom-workspace` app: a
modular, placeholder-only developer surface written in **vanilla
TypeScript, HTML, and CSS**. It uses **no UI framework**, **no bundler
assumptions**, and **no embedded frames**. It attaches **no runtime** — every
value on screen is clearly-labeled placeholder data, and every button updates
shell view-state only. It never simulates real game behavior.

## What it is

A dense, square, light-themed instrument panel that lays out the surfaces a
future in-browser Axiom workspace would need. It is organized as a header, a
main area with a **left** authoring column, a large **center** viewport, and a
**right** inspect/export column, plus a full-width **bottom** observe/control
bar.

## Panels (stable ids)

| Region | Panel id | Panel |
|--------|----------|-------|
| left   | `project-browser`      | Project Browser |
| left   | `game-manifest-editor` | Game Manifest Editor (typed fields, not a raw textarea) |
| left   | `level-browser`        | Level Browser |
| center | `runtime-viewport`     | Runtime Viewport (shows "Runtime Viewport Placeholder") |
| right  | `object-inspector`     | Object Inspector |
| right  | `asset-browser`        | Asset Browser |
| right  | `package-export`       | Package / Export |
| bottom | `play-controls`        | Play Controls (Edit / Play / Drop In / Replay / Package) |
| bottom | `timeline-replay`      | Timeline / Replay |
| bottom | `console-log-viewer`   | Console Log Viewer |
| bottom | `profiler`             | Profiler |
| bottom | `input-debugger`       | Input Debugger |

The stable order and region of each panel lives in `src/workspace_layout.ts`
(`WORKSPACE_LAYOUT`). Each panel has its own file under `src/panels/` exporting
**exactly one** render function with the standardized signature
`(state: WorkspaceBrowserState, dispatch: Dispatch) => HTMLElement`.
`src/panel_registry.ts` maps each panel id to its renderer.

## Typed state / event / reducer design

- **State** (`src/workspace_state.ts`) — one `readonly` typed state object per
  panel, composed into a root `WorkspaceBrowserState`. There is **no `any`**.
  `initialWorkspaceState()` fills every panel with a few placeholder rows so the
  shell renders visibly populated.
- **Events** (`src/workspace_events.ts`) — a typed discriminated union
  `WorkspaceEvent` (tags: `workspace.mode.set`, `project.placeholder.open`,
  `launch.placeholder.create`, `drop_in.placeholder.create`,
  `input.placeholder.record`, `snapshot.placeholder.record`,
  `replay.placeholder.create`, `package.placeholder.request`,
  `object.placeholder.select`, `asset.placeholder.select`,
  `level.placeholder.select`). A shared `Dispatch = (event: WorkspaceEvent) =>
  void` type is threaded explicitly through the panels — **no globals**.
- **Reducer** (`applyWorkspaceEvent`) — the **single** place state changes. It
  switches exhaustively over the event tag and returns a **new**
  `WorkspaceBrowserState` (clone the root, replace the affected slice, build new
  arrays) for each event. It updates placeholder view-state only.
- **Mounting** (`src/dom_mount.ts`) — builds the region layout, places each
  panel in `WORKSPACE_LAYOUT` order/region, and owns the dispatch loop: a
  dispatch applies the event through the reducer, stores the new state, and
  re-renders. `src/main.ts` is the boring entry point.

## No framework, no embedded frames

This shell is deliberately dependency-free vanilla web code. It contains no
framework reference and no embedded frame element anywhere. Those constraints
are enforced by the Rust architecture tests in `../tests/architecture.rs`, which
scan every file under `web/`.

## Smoke test

There is **no TS test runner wired for this app** — the repo's TypeScript gates
are scoped to `packages/` only. The shell's existence and shape are enforced by
the Rust architecture tests (`../tests/architecture.rs`). To exercise it by hand:

1. Serve this `web/` directory with any static file server.
2. Open `index.html`.
3. Click the **Play Controls** mode buttons (Edit / Play / Drop In / Replay /
   Package) and the workflow buttons in the other panels (open a project, select
   a level/asset/object, record a placeholder input, record a placeholder
   snapshot, request a placeholder replay, request a placeholder package).

Each click dispatches a **typed** `WorkspaceEvent` through
`applyWorkspaceEvent`, which produces a new state and re-renders the affected
panels — confirming the state / event / reducer wiring end-to-end.
