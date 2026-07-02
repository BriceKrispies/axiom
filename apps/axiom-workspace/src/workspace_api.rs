//! [`WorkspaceApi`] — the single public facade of the workspace app.
//!
//! Every contract the workspace hands out is constructed through this facade so
//! callers (the browser shell, tests, a future host bootstrap) name exactly one
//! entry point. The facade is a zero-sized handle: it owns no state and performs
//! no I/O — it is a namespace of constructor/validator functions over the typed
//! scaffold contracts. It launches, observes, records, and replays; it never
//! simulates game rules.

use axiom_kernel::{EntityId, SchemaVersion, StableHash, Tick};

use crate::asset_browser::AssetBrowserState;
use crate::console_log_viewer::ConsoleLogViewerState;
use crate::drop_in_spec::DropInSpec;
use crate::game_manifest_editor::GameManifestEditorState;
use crate::game_project::GameProject;
use crate::input_debugger::InputDebuggerState;
use crate::launch_spec::LaunchSpec;
use crate::level_browser::LevelBrowserState;
use crate::object_inspector::ObjectInspectorState;
use crate::package_export::PackageExportState;
use crate::play_controls::PlayControlsState;
use crate::play_session::PlaySession;
use crate::profiler_panel::ProfilerPanelState;
use crate::project_browser::ProjectBrowserState;
use crate::replay_request::ReplayRequest;
use crate::runtime_viewport::RuntimeViewportState;
use crate::session_record::{RecordedInput, RecordedSnapshot, SessionRecord};
use crate::timeline_replay::TimelineReplayState;
use crate::workspace_manifest::WorkspaceManifest;
use crate::workspace_panel::{WorkspacePanel, WorkspacePanelId, WorkspacePanelState};

/// A validation failure produced while constructing a workspace contract.
///
/// It names the field at fault so callers and tests can assert precisely which
/// invariant was violated. It carries no `std::error` machinery — the workspace
/// contracts are pure value types, and a boring, comparable error keeps the
/// scaffold deterministic and easy to test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    /// A required text field was empty (after trimming).
    MissingField {
        /// The name of the field that was required but empty.
        field: &'static str,
    },
}

/// The mode the workspace shell is currently presenting.
///
/// These names describe the **workspace shell**, not engine behavior: they select
/// which arrangement/emphasis of panels the shell shows. No mode runs, steps, or
/// simulates anything — the engine is unaffected by the shell's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    /// Authoring the project (browsers + inspector + manifest editor).
    Edit,
    /// Watching a launched session in the runtime viewport.
    Play,
    /// Launching into a specific spot via an editor-derived spawn context.
    DropIn,
    /// Reviewing a recorded session on the timeline.
    Replay,
    /// Staging a package export.
    Package,
}

/// A deterministic snapshot of the shell layout for a given [`WorkspaceMode`]: the
/// current mode, the ordered metadata of all twelve panels, and their default
/// placeholder states — all in the canonical stable order.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceLayoutSnapshot {
    mode: WorkspaceMode,
    panels: Vec<WorkspacePanel>,
    states: Vec<WorkspacePanelState>,
}

impl WorkspaceLayoutSnapshot {
    /// The mode this layout was built for.
    #[must_use]
    pub fn mode(&self) -> WorkspaceMode {
        self.mode
    }

    /// The ordered metadata of all twelve panels, in canonical stable order.
    #[must_use]
    pub fn panels(&self) -> &[WorkspacePanel] {
        &self.panels
    }

    /// The ordered default states of all twelve panels, aligned with
    /// [`WorkspaceLayoutSnapshot::panels`].
    #[must_use]
    pub fn states(&self) -> &[WorkspacePanelState] {
        &self.states
    }

    /// The ordered panel ids, in canonical stable order.
    #[must_use]
    pub fn panel_ids(&self) -> Vec<WorkspacePanelId> {
        self.panels.iter().map(|panel| panel.id).collect()
    }
}

/// The workspace app's single public facade.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceApi;

impl WorkspaceApi {
    /// Create a new facade handle. Stateless — every call is equivalent.
    #[must_use]
    pub const fn new() -> Self {
        WorkspaceApi
    }

    /// Create and validate a [`WorkspaceManifest`] from typed data.
    pub fn create_manifest(
        self,
        name: &str,
        workspace_root: &str,
        schema_version: SchemaVersion,
    ) -> Result<WorkspaceManifest, WorkspaceError> {
        WorkspaceManifest::new(name, workspace_root, schema_version)
    }

    /// Load and validate a [`GameProject`] descriptor from typed data.
    pub fn load_project(
        self,
        id: &str,
        name: &str,
        version: &str,
        entrypoint: &str,
    ) -> Result<GameProject, WorkspaceError> {
        GameProject::new(id, name, version, entrypoint)
    }

    /// Build a canonical [`LaunchSpec`] for a project.
    #[must_use]
    pub fn launch_spec(self, project: &GameProject) -> LaunchSpec {
        LaunchSpec::for_project(project)
    }

    /// Build an editor-derived [`DropInSpec`] spawn context.
    #[must_use]
    pub fn drop_in(
        self,
        level_id: &str,
        position: [f32; 3],
        yaw: f32,
        selected_entity: Option<EntityId>,
    ) -> DropInSpec {
        DropInSpec::new(level_id, position, yaw, selected_entity)
    }

    /// Start a launchable [`PlaySession`] from a launch spec. The session begins
    /// in [`crate::play_session::PlaySessionStatus::Created`] and simulates
    /// nothing.
    #[must_use]
    pub fn play_session(self, launch_spec: LaunchSpec) -> PlaySession {
        PlaySession::created(launch_spec)
    }

    /// Open an empty [`SessionRecord`] over a session's launch spec, ready to
    /// accumulate input and snapshot-hash artifacts.
    #[must_use]
    pub fn record_session(self, record_id: &str, session: &PlaySession) -> SessionRecord {
        SessionRecord::new(record_id, session.launch_spec().clone())
    }

    /// Construct a [`ReplayRequest`] that points at a prior session record.
    #[must_use]
    pub fn replay_request(self, record: &SessionRecord) -> ReplayRequest {
        ReplayRequest::for_record(record)
    }

    /// A recorded input artifact at a tick — the canonical shape the workspace
    /// stores, independent of any concrete input backend.
    #[must_use]
    pub fn recorded_input(self, tick: Tick, input_code: u32) -> RecordedInput {
        RecordedInput::new(tick, input_code)
    }

    /// A recorded snapshot-hash artifact at a tick. The hash is a diagnostic
    /// index over opaque snapshot bytes — byte equality remains the verdict.
    #[must_use]
    pub fn recorded_snapshot(self, tick: Tick, snapshot_bytes: &[u8]) -> RecordedSnapshot {
        RecordedSnapshot::of_bytes(tick, snapshot_bytes)
    }

    /// Deterministic summary digest of a session record, for tests and for the
    /// shell's status panels. Contract-only: it hashes the record's artifacts,
    /// it does not run anything.
    #[must_use]
    pub fn summarize(self, record: &SessionRecord) -> StableHash {
        record.digest()
    }

    /// A default (empty placeholder) [`ProjectBrowserState`].
    #[must_use]
    pub fn project_browser_state(self) -> ProjectBrowserState {
        ProjectBrowserState::default()
    }

    /// A default (all-`"<unset>"` placeholder) [`GameManifestEditorState`].
    #[must_use]
    pub fn game_manifest_editor_state(self) -> GameManifestEditorState {
        GameManifestEditorState::default()
    }

    /// A default (empty placeholder) [`LevelBrowserState`].
    #[must_use]
    pub fn level_browser_state(self) -> LevelBrowserState {
        LevelBrowserState::default()
    }

    /// A default (unattached placeholder) [`RuntimeViewportState`].
    #[must_use]
    pub fn runtime_viewport_state(self) -> RuntimeViewportState {
        RuntimeViewportState::default()
    }

    /// A default (empty placeholder) [`ObjectInspectorState`].
    #[must_use]
    pub fn object_inspector_state(self) -> ObjectInspectorState {
        ObjectInspectorState::default()
    }

    /// A default (empty placeholder) [`AssetBrowserState`].
    #[must_use]
    pub fn asset_browser_state(self) -> AssetBrowserState {
        AssetBrowserState::default()
    }

    /// A default (empty placeholder) [`ConsoleLogViewerState`].
    #[must_use]
    pub fn console_log_viewer_state(self) -> ConsoleLogViewerState {
        ConsoleLogViewerState::default()
    }

    /// A default (empty placeholder) [`ProfilerPanelState`].
    #[must_use]
    pub fn profiler_state(self) -> ProfilerPanelState {
        ProfilerPanelState::default()
    }

    /// A default (empty placeholder) [`InputDebuggerState`].
    #[must_use]
    pub fn input_debugger_state(self) -> InputDebuggerState {
        InputDebuggerState::default()
    }

    /// A default (empty placeholder) [`TimelineReplayState`].
    #[must_use]
    pub fn timeline_replay_state(self) -> TimelineReplayState {
        TimelineReplayState::default()
    }

    /// A default (Edit-mode, nothing-staged placeholder) [`PlayControlsState`].
    #[must_use]
    pub fn play_controls_state(self) -> PlayControlsState {
        PlayControlsState::default()
    }

    /// A default (idle placeholder) [`PackageExportState`].
    #[must_use]
    pub fn package_export_state(self) -> PackageExportState {
        PackageExportState::default()
    }

    /// The pure metadata (title + region) of a panel by id.
    #[must_use]
    pub fn panel(self, id: WorkspacePanelId) -> WorkspacePanel {
        WorkspacePanel::for_id(id)
    }

    /// The default placeholder state of a panel by id, wrapped in the tagged
    /// [`WorkspacePanelState`] union.
    #[must_use]
    pub fn default_panel_state(self, id: WorkspacePanelId) -> WorkspacePanelState {
        match id {
            WorkspacePanelId::ProjectBrowser => {
                WorkspacePanelState::ProjectBrowser(self.project_browser_state())
            }
            WorkspacePanelId::GameManifestEditor => {
                WorkspacePanelState::GameManifestEditor(self.game_manifest_editor_state())
            }
            WorkspacePanelId::LevelBrowser => {
                WorkspacePanelState::LevelBrowser(self.level_browser_state())
            }
            WorkspacePanelId::RuntimeViewport => {
                WorkspacePanelState::RuntimeViewport(self.runtime_viewport_state())
            }
            WorkspacePanelId::ObjectInspector => {
                WorkspacePanelState::ObjectInspector(self.object_inspector_state())
            }
            WorkspacePanelId::AssetBrowser => {
                WorkspacePanelState::AssetBrowser(self.asset_browser_state())
            }
            WorkspacePanelId::ConsoleLogViewer => {
                WorkspacePanelState::ConsoleLogViewer(self.console_log_viewer_state())
            }
            WorkspacePanelId::Profiler => WorkspacePanelState::Profiler(self.profiler_state()),
            WorkspacePanelId::InputDebugger => {
                WorkspacePanelState::InputDebugger(self.input_debugger_state())
            }
            WorkspacePanelId::TimelineReplay => {
                WorkspacePanelState::TimelineReplay(self.timeline_replay_state())
            }
            WorkspacePanelId::PlayControls => {
                WorkspacePanelState::PlayControls(self.play_controls_state())
            }
            WorkspacePanelId::PackageExport => {
                WorkspacePanelState::PackageExport(self.package_export_state())
            }
        }
    }

    /// Build the full shell layout for a mode: all twelve panels' metadata and
    /// their default placeholder states, in the canonical stable order
    /// ([`WorkspacePanelId::ALL`]).
    #[must_use]
    pub fn layout_snapshot(self, mode: WorkspaceMode) -> WorkspaceLayoutSnapshot {
        let panels = WorkspacePanelId::ALL
            .iter()
            .map(|&id| self.panel(id))
            .collect();
        let states = WorkspacePanelId::ALL
            .iter()
            .map(|&id| self.default_panel_state(id))
            .collect();
        WorkspaceLayoutSnapshot {
            mode,
            panels,
            states,
        }
    }
}
