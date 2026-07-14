//! # Axiom Workspace
//!
//! A developer-facing **composition-leaf app** that opens a game project, reads a
//! workspace manifest, builds a canonical launch specification, starts a
//! play-session abstraction, records input/snapshot artifacts, and constructs
//! replay requests over prior sessions.
//!
//! ## What it is — and is not
//! The workspace **launches, observes, records, and replays** real runtime
//! sessions. It is emphatically **not** a second implementation of game behavior:
//! it runs no game rules, steps no simulation, drives no rendering, and reads no
//! clock. It is an app — a leaf in the dependency graph — so no engine layer or
//! module may depend on it (enforced by `tests/architecture.rs`).
//!
//! ## Public facade
//! [`WorkspaceApi`] is the single entry point. Alongside it, the crate re-exports
//! the typed scaffold **contracts** the facade traffics in — the nouns it returns
//! — so callers (the browser shell, tests, a future host bootstrap) can name them.
//! Real runtime integration is deliberately deferred behind these contracts until
//! the public engine APIs it needs exist; the missing contracts are documented in
//! `ARCHITECTURE.md`.

mod asset_browser;
mod console_log_viewer;
mod drop_in_spec;
mod game_manifest_editor;
mod game_project;
mod input_debugger;
mod launch_spec;
mod level_browser;
mod object_inspector;
mod package_export;
mod play_controls;
mod play_session;
mod profiler_panel;
mod project_browser;
mod replay_request;
mod runtime_viewport;
mod session_record;
mod timeline_replay;
mod workspace_api;
mod workspace_manifest;
mod workspace_panel;

pub use workspace_api::{WorkspaceApi, WorkspaceError, WorkspaceLayoutSnapshot, WorkspaceMode};

pub use drop_in_spec::{DropInSpec, DropLevel};
pub use game_project::GameProject;
pub use launch_spec::LaunchSpec;
pub use play_session::{PlaySession, PlaySessionStatus};
pub use replay_request::ReplayRequest;
pub use session_record::{RecordedInput, RecordedSnapshot, SessionRecord};
pub use workspace_manifest::WorkspaceManifest;

// The workspace-shell panel vocabulary: panel identity/metadata and the typed,
// placeholder state each panel carries. These are the nouns the facade returns.
pub use workspace_panel::{WorkspacePanel, WorkspacePanelId, WorkspacePanelState, WorkspaceRegion};

pub use asset_browser::{AssetBrowserState, AssetEntry};
pub use console_log_viewer::{ConsoleLevel, ConsoleLogViewerState, ConsoleRecord};
pub use game_manifest_editor::GameManifestEditorState;
pub use input_debugger::{InputDebuggerState, InputEventRecord};
pub use level_browser::{LevelBrowserState, LevelEntry};
pub use object_inspector::{InspectorField, ObjectInspectorState};
pub use package_export::{PackageExportState, PackageStatus};
pub use play_controls::PlayControlsState;
pub use profiler_panel::{ProfilerPanelState, ProfilerSample};
pub use project_browser::{ProjectBrowserState, ProjectEntry};
pub use runtime_viewport::{ComparisonBackend, RuntimeViewportState, RuntimeViewportView};
pub use timeline_replay::{TimelineReplayState, TimelineTick};
