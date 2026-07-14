//! [`WorkspacePanel`] — the pure metadata of one panel in the workspace shell,
//! plus its supporting vocabulary: [`WorkspacePanelId`] (the stable identity of
//! each of the twelve panels), [`WorkspaceRegion`] (where a panel docks), and
//! [`WorkspacePanelState`] (the tagged union of every concrete panel state).
//!
//! These are pure value types describing the *shell layout*, not engine
//! behavior. No panel simulates, steps, or ticks anything.

use crate::asset_browser::AssetBrowserState;
use crate::console_log_viewer::ConsoleLogViewerState;
use crate::game_manifest_editor::GameManifestEditorState;
use crate::input_debugger::InputDebuggerState;
use crate::level_browser::LevelBrowserState;
use crate::object_inspector::ObjectInspectorState;
use crate::package_export::PackageExportState;
use crate::play_controls::PlayControlsState;
use crate::profiler_panel::ProfilerPanelState;
use crate::project_browser::ProjectBrowserState;
use crate::runtime_viewport::RuntimeViewportState;
use crate::timeline_replay::TimelineReplayState;

/// The stable identity of one workspace panel. There are exactly twelve, each
/// with a stable kebab-case string id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePanelId {
    /// The Project Browser panel.
    ProjectBrowser,
    /// The Game Manifest Editor panel.
    GameManifestEditor,
    /// The Level Browser panel.
    LevelBrowser,
    /// The Runtime Viewport panel.
    RuntimeViewport,
    /// The Object Inspector panel.
    ObjectInspector,
    /// The Asset Browser panel.
    AssetBrowser,
    /// The Console Log Viewer panel.
    ConsoleLogViewer,
    /// The Profiler panel.
    Profiler,
    /// The Input Debugger panel.
    InputDebugger,
    /// The Timeline / Replay panel.
    TimelineReplay,
    /// The Play Controls panel.
    PlayControls,
    /// The Package Export panel.
    PackageExport,
}

impl WorkspacePanelId {
    /// Every panel id in the canonical stable order.
    pub const ALL: [WorkspacePanelId; 12] = [
        WorkspacePanelId::ProjectBrowser,
        WorkspacePanelId::GameManifestEditor,
        WorkspacePanelId::LevelBrowser,
        WorkspacePanelId::RuntimeViewport,
        WorkspacePanelId::ObjectInspector,
        WorkspacePanelId::AssetBrowser,
        WorkspacePanelId::PackageExport,
        WorkspacePanelId::PlayControls,
        WorkspacePanelId::TimelineReplay,
        WorkspacePanelId::ConsoleLogViewer,
        WorkspacePanelId::Profiler,
        WorkspacePanelId::InputDebugger,
    ];

    /// The stable kebab-case string id of this panel.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspacePanelId::ProjectBrowser => "project-browser",
            WorkspacePanelId::GameManifestEditor => "game-manifest-editor",
            WorkspacePanelId::LevelBrowser => "level-browser",
            WorkspacePanelId::RuntimeViewport => "runtime-viewport",
            WorkspacePanelId::ObjectInspector => "object-inspector",
            WorkspacePanelId::AssetBrowser => "asset-browser",
            WorkspacePanelId::ConsoleLogViewer => "console-log-viewer",
            WorkspacePanelId::Profiler => "profiler",
            WorkspacePanelId::InputDebugger => "input-debugger",
            WorkspacePanelId::TimelineReplay => "timeline-replay",
            WorkspacePanelId::PlayControls => "play-controls",
            WorkspacePanelId::PackageExport => "package-export",
        }
    }
}

/// A dock region of the workspace shell. Pure layout metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRegion {
    /// The left dock (project/manifest/level browsing).
    Left,
    /// The center stage (the runtime viewport).
    Center,
    /// The right dock (inspection / asset / export).
    Right,
    /// The bottom dock (play / timeline / logs / profiler / input).
    Bottom,
}

/// The pure metadata of one workspace panel: its id, its display title, and the
/// region it docks in. Carries no state — [`WorkspacePanelState`] carries state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspacePanel {
    /// The panel's stable identity.
    pub id: WorkspacePanelId,
    /// The panel's display title.
    pub title: &'static str,
    /// The region the panel docks in.
    pub region: WorkspaceRegion,
}

impl WorkspacePanel {
    /// The pure metadata (title + region) for a panel id.
    #[must_use]
    pub fn for_id(id: WorkspacePanelId) -> WorkspacePanel {
        let (title, region) = match id {
            WorkspacePanelId::ProjectBrowser => ("Project Browser", WorkspaceRegion::Left),
            WorkspacePanelId::GameManifestEditor => ("Game Manifest Editor", WorkspaceRegion::Left),
            WorkspacePanelId::LevelBrowser => ("Level Browser", WorkspaceRegion::Left),
            WorkspacePanelId::RuntimeViewport => ("Runtime Viewport", WorkspaceRegion::Center),
            WorkspacePanelId::ObjectInspector => ("Object Inspector", WorkspaceRegion::Right),
            WorkspacePanelId::AssetBrowser => ("Asset Browser", WorkspaceRegion::Right),
            WorkspacePanelId::PackageExport => ("Package Export", WorkspaceRegion::Right),
            WorkspacePanelId::PlayControls => ("Play Controls", WorkspaceRegion::Bottom),
            WorkspacePanelId::TimelineReplay => ("Timeline / Replay", WorkspaceRegion::Bottom),
            WorkspacePanelId::ConsoleLogViewer => ("Console Log Viewer", WorkspaceRegion::Bottom),
            WorkspacePanelId::Profiler => ("Profiler", WorkspaceRegion::Bottom),
            WorkspacePanelId::InputDebugger => ("Input Debugger", WorkspaceRegion::Bottom),
        };
        WorkspacePanel { id, title, region }
    }
}

/// The tagged union of every concrete panel state. Each variant wraps the typed
/// placeholder state of one panel, and [`WorkspacePanelState::id`] recovers the
/// panel identity.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkspacePanelState {
    /// The Project Browser panel's state.
    ProjectBrowser(ProjectBrowserState),
    /// The Game Manifest Editor panel's state.
    GameManifestEditor(GameManifestEditorState),
    /// The Level Browser panel's state.
    LevelBrowser(LevelBrowserState),
    /// The Runtime Viewport panel's state.
    RuntimeViewport(RuntimeViewportState),
    /// The Object Inspector panel's state.
    ObjectInspector(ObjectInspectorState),
    /// The Asset Browser panel's state.
    AssetBrowser(AssetBrowserState),
    /// The Console Log Viewer panel's state.
    ConsoleLogViewer(ConsoleLogViewerState),
    /// The Profiler panel's state.
    Profiler(ProfilerPanelState),
    /// The Input Debugger panel's state.
    InputDebugger(InputDebuggerState),
    /// The Timeline / Replay panel's state.
    TimelineReplay(TimelineReplayState),
    /// The Play Controls panel's state.
    PlayControls(PlayControlsState),
    /// The Package Export panel's state.
    PackageExport(PackageExportState),
}

impl WorkspacePanelState {
    /// The identity of the panel this state belongs to.
    #[must_use]
    pub fn id(&self) -> WorkspacePanelId {
        match self {
            WorkspacePanelState::ProjectBrowser(_) => WorkspacePanelId::ProjectBrowser,
            WorkspacePanelState::GameManifestEditor(_) => WorkspacePanelId::GameManifestEditor,
            WorkspacePanelState::LevelBrowser(_) => WorkspacePanelId::LevelBrowser,
            WorkspacePanelState::RuntimeViewport(_) => WorkspacePanelId::RuntimeViewport,
            WorkspacePanelState::ObjectInspector(_) => WorkspacePanelId::ObjectInspector,
            WorkspacePanelState::AssetBrowser(_) => WorkspacePanelId::AssetBrowser,
            WorkspacePanelState::ConsoleLogViewer(_) => WorkspacePanelId::ConsoleLogViewer,
            WorkspacePanelState::Profiler(_) => WorkspacePanelId::Profiler,
            WorkspacePanelState::InputDebugger(_) => WorkspacePanelId::InputDebugger,
            WorkspacePanelState::TimelineReplay(_) => WorkspacePanelId::TimelineReplay,
            WorkspacePanelState::PlayControls(_) => WorkspacePanelId::PlayControls,
            WorkspacePanelState::PackageExport(_) => WorkspacePanelId::PackageExport,
        }
    }
}
