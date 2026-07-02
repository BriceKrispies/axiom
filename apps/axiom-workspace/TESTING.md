# axiom-workspace — Testing

The workspace is an app (a composition leaf), so it is outside the engine's
100%-coverage and branchless spine gates. It still ships with the tests its
behavior warrants: typed-contract tests, panel/ordering tests, and
architecture-boundary tests.

Run them with the rest of the workspace:

```sh
cargo test --workspace          # runs the crate's tests plus the repo-wide architecture test
cargo xtask check-architecture  # validates app.toml classification + graph
```

`cargo test -p axiom-workspace` runs just this crate's tests; the two commands
above are the ones to reach for as the canonical validation pair, and are the
ones referenced elsewhere in this repo's docs.

## Typed contract tests (`tests/workspace_contracts.rs`)

Every scaffold contract is a pure value type, so each is tested directly
through the `WorkspaceApi` facade:

1. **Manifest validation** — `WorkspaceManifest` accepts fully-specified data
   and rejects an empty/blank name or root with a precise
   `WorkspaceError::MissingField`.
2. **Project validation** — `GameProject` rejects empty id, name, version, and
   entrypoint, and accepts a valid descriptor.
3. **Launch spec stability** — `LaunchSpec::identity()` is equal for two specs
   built from the same project/version/config, and differs when the version
   differs. This is the determinism property the workspace promises about a
   launch identity.
4. **Drop-in is additive** — attaching a `DropInSpec` leaves the launch
   identity and every launch-identity field unchanged, and the drop context
   becomes readable on the spec. This proves Drop In does not mutate the
   launch.
5. **Session status** — a `PlaySession` starts in `PlaySessionStatus::Created`.
6. **Input order** — `SessionRecord` preserves the exact order inputs were
   recorded.
7. **Snapshot-hash order** — `SessionRecord` preserves the exact order
   snapshot hashes were recorded, and each stored hash matches an
   independently recomputed one (the hash is a diagnostic index over opaque
   bytes).
8. **Replay references the record** — a `ReplayRequest` built from a record
   carries the record's id, its launch identity, and its digest.
9. **Deterministic summary** — `WorkspaceApi::summarize` is a pure function of
   a record (same record → same digest), the property tests and shell status
   panels rely on.

## Panel contract tests

Twelve panels, each with its own typed state, need contract tests proving
the facade constructs a legal shell and every panel is well-formed:

1. **`WorkspaceApi` constructs every panel default state.** A single test
   builds a `WorkspaceLayoutSnapshot` via the facade and asserts every one of
   the twelve panel state types (`ProjectBrowserState`,
   `GameManifestEditorState`, `LevelBrowserState`, `RuntimeViewportState`,
   `ObjectInspectorState`, `AssetBrowserState`, `ConsoleLogViewerState`,
   `ProfilerPanelState`, `InputDebuggerState`, `TimelineReplayState`,
   `PlayControlsState`, `PackageExportState`) is present and equal to its own
   `Default`. A snapshot missing or duplicating a panel state is a bug this
   test exists to catch.
2. **`WorkspaceLayoutSnapshot` includes exactly the expected panel ids.** The
   snapshot's panel id set is asserted equal to the closed list of twelve
   kebab-case ids (`project-browser`, `game-manifest-editor`, `level-browser`,
   `runtime-viewport`, `object-inspector`, `asset-browser`,
   `console-log-viewer`, `profiler`, `input-debugger`, `timeline-replay`,
   `play-controls`, `package-export`) — no more, no fewer.
3. **`WorkspaceMode` represents all five modes.** A test enumerates
   `WorkspaceMode::Edit`, `Play`, `DropIn`, `Replay`, and `Package` and
   asserts each round-trips through the facade (e.g. through whatever
   mode-setting/reading API `WorkspaceApi` exposes) without collapsing two
   modes into the same representation.
4. **Panel ids are stable strings.** Each `WorkspacePanelId` is asserted
   against its literal kebab-case string; a rename of a panel id is a
   deliberate, visible diff in this test, not a silent drift.
5. **Panel ordering is stable.** The order panels appear in
   `WorkspaceLayoutSnapshot` (and the order `WorkspacePanel::all()` or
   equivalent enumerates them) is asserted against a fixed, literal sequence,
   so panel order never silently reshuffles between runs or refactors.

## Insertion-order tests

Every panel or record type that accumulates rows/entries over time is tested
for exact insertion-order preservation, the same property already proven for
`SessionRecord`'s inputs and snapshot hashes:

- **Console log records** — `ConsoleLogViewerState` returns appended log
  entries in the exact order they were recorded.
- **Profiler samples** — `ProfilerPanelState` returns appended samples in the
  exact order they were recorded.
- **Input records** — `InputDebuggerState` returns appended input records in
  the exact order they were recorded.
- **Timeline ticks** — `TimelineReplayState` returns its tick markers in the
  exact order they were added (ascending, unreordered).
- **Asset/project/level rows** — `AssetBrowserState`, `ProjectBrowserState`,
  and `LevelBrowserState` each return their rows in the exact order they were
  added, never re-sorted by the state type itself (any sorting is a
  presentation concern for the browser shell, not the Rust contract).

## Architecture-boundary tests (`tests/architecture.rs`)

These pin the workspace's structural boundaries:

- `app.toml` lists only the kernel and runtime layers and no engine module.
- the portable Rust crate references no browser/GPU/DOM API and no `iframe`
  (those are confined to `web/`).
- the Rust crate uses no wall-clock time or randomness.
- no placeholder macros or console output (`todo!`, `unimplemented!`,
  `println!`, `eprintln!`, `dbg!`) in the crate.
- the crate imports only `axiom-kernel`, `axiom-runtime`, and itself — no
  engine module and no other app.
- `lib.rs` exposes exactly the `WorkspaceApi` facade plus the typed vocabulary
  it traffics in (the twelve panel state types, `WorkspaceMode`,
  `WorkspacePanel`, `WorkspacePanelId`, `WorkspacePanelState`,
  `WorkspaceRegion`, `WorkspaceLayoutSnapshot`, and the pre-expansion
  scaffold contracts) — no other top-level `pub`/`pub use` exists.
- no junk-drawer folder or module (`utils`/`helpers`/`common`/`misc`) exists
  in the crate or the shell.
- no engine layer or module depends on `axiom-workspace` (it is a leaf).
- a browser panel file exists for every one of the twelve panels
  (`web/src/panels/*_panel.ts`, one per panel id in the table in
  `ARCHITECTURE.md`) — a panel added on the Rust side without its web
  counterpart (or vice versa) fails this check.
- the browser shell uses **no iframe** and **no UI framework**
  (React/Vue/Svelte/Angular/Next/Remix/Electron/Tauri) anywhere under `web/`.

## Browser shell smoke expectations

The shell is static vanilla TS/HTML/CSS and is not wired into a bundler/gate
yet. A manual smoke check: serve `web/` and click through the mode buttons
(Edit / Play / Drop In / Replay / Package) and the workflow buttons (open
project → build launch spec → drop in → record input → record snapshot hash →
replay). Each click must:

1. construct a typed `WorkspaceEvent`,
2. dispatch it through `applyWorkspaceEvent(state, event)` — never mutate
   `WorkspaceState` in place from a click handler,
3. re-render only the panel(s) whose state the reducer changed, and
4. append a corresponding entry to the `console-log-viewer` panel.

There is no automated browser test yet because there is no real runtime to
drive; the smoke check only verifies the typed-event/reducer/re-render loop
itself is wired correctly, not any engine behavior.

## What is intentionally placeholder-only

Because real runtime integration does not exist (the public engine contracts
for launch/spawn/input/snapshot/logs/profiling/replay/package/viewport are not
available — see `ARCHITECTURE.md`, "Future engine contracts needed"), the
following are **out of scope** until those contracts land, and no test should
assert real behavior for them:

- launching an actual `axiom-runtime` session from a `LaunchSpec`,
- spawning a scene from a `DropInSpec`,
- capturing real input into `RecordedInput` or `InputDebuggerState`,
- producing real snapshot bytes for `RecordedSnapshot` or the
  `object-inspector`/`level-browser` panels,
- streaming real structured logs into `console-log-viewer`,
- sampling real timing data into `profiler`,
- driving an end-to-end record → replay → byte-equality proof through
  `timeline-replay`,
- running a real build/export through `package-export`,
- rendering into the Runtime Viewport Placeholder.

These are deliberately deferred, not forgotten: the scaffold contracts and
panel states exist so that each becomes a small, testable adapter once its
engine contract is public. A test that fakes any of the above to make a panel
"look live" would be testing a fiction, not the workspace's actual contract.
