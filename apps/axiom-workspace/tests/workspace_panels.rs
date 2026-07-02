//! Panel-shell tests for the `axiom-workspace` app.
//!
//! These prove the twelve-panel workspace shell's *identity and layout* contract:
//! every panel id is a stable kebab string, `WorkspacePanelId::ALL` and every
//! `WorkspaceLayoutSnapshot` present the panels in one canonical stable order, the
//! facade can construct every panel's default state (directly and by id), and a
//! layout snapshot round-trips each `WorkspaceMode`. Value-ordering behaviour of
//! individual panel states lives in `workspace_records.rs`.

use axiom_workspace::{
    WorkspaceApi, WorkspaceMode, WorkspacePanel, WorkspacePanelId, WorkspaceRegion,
};

fn api() -> WorkspaceApi {
    WorkspaceApi::new()
}

/// The twelve panel ids in the single canonical stable order, as kebab strings.
/// This is the order the shell renders and every snapshot must reproduce.
const CANONICAL_IDS: [&str; 12] = [
    "project-browser",
    "game-manifest-editor",
    "level-browser",
    "runtime-viewport",
    "object-inspector",
    "asset-browser",
    "package-export",
    "play-controls",
    "timeline-replay",
    "console-log-viewer",
    "profiler",
    "input-debugger",
];

/// Every `WorkspaceMode`, so a test can prove the snapshot round-trips each.
const ALL_MODES: [WorkspaceMode; 5] = [
    WorkspaceMode::Edit,
    WorkspaceMode::Play,
    WorkspaceMode::DropIn,
    WorkspaceMode::Replay,
    WorkspaceMode::Package,
];

#[test]
fn panel_ids_are_stable_kebab_strings() {
    // Pin every id to its exact kebab literal. These strings are a wire/DOM
    // contract with the browser shell; a rename here is a breaking change.
    assert_eq!(WorkspacePanelId::ProjectBrowser.as_str(), "project-browser");
    assert_eq!(
        WorkspacePanelId::GameManifestEditor.as_str(),
        "game-manifest-editor"
    );
    assert_eq!(WorkspacePanelId::LevelBrowser.as_str(), "level-browser");
    assert_eq!(WorkspacePanelId::RuntimeViewport.as_str(), "runtime-viewport");
    assert_eq!(WorkspacePanelId::ObjectInspector.as_str(), "object-inspector");
    assert_eq!(WorkspacePanelId::AssetBrowser.as_str(), "asset-browser");
    assert_eq!(
        WorkspacePanelId::ConsoleLogViewer.as_str(),
        "console-log-viewer"
    );
    assert_eq!(WorkspacePanelId::Profiler.as_str(), "profiler");
    assert_eq!(WorkspacePanelId::InputDebugger.as_str(), "input-debugger");
    assert_eq!(WorkspacePanelId::TimelineReplay.as_str(), "timeline-replay");
    assert_eq!(WorkspacePanelId::PlayControls.as_str(), "play-controls");
    assert_eq!(WorkspacePanelId::PackageExport.as_str(), "package-export");
}

#[test]
fn panel_all_is_the_canonical_order_of_twelve() {
    assert_eq!(WorkspacePanelId::ALL.len(), 12);
    let ordered: Vec<&str> = WorkspacePanelId::ALL.iter().map(|id| id.as_str()).collect();
    assert_eq!(
        ordered,
        CANONICAL_IDS.to_vec(),
        "WorkspacePanelId::ALL is the canonical stable order"
    );
}

#[test]
fn layout_snapshot_has_exactly_the_twelve_canonical_panel_ids_in_order() {
    let snapshot = api().layout_snapshot(WorkspaceMode::Edit);

    // panel_ids() is the canonical order …
    let ids: Vec<&str> = snapshot.panel_ids().iter().map(|id| id.as_str()).collect();
    assert_eq!(ids, CANONICAL_IDS.to_vec());

    // … and it matches ALL, and the panels()/states() vectors are the same length
    // and aligned to it.
    assert_eq!(snapshot.panels().len(), 12);
    assert_eq!(snapshot.states().len(), 12);
    assert_eq!(snapshot.panel_ids(), WorkspacePanelId::ALL.to_vec());
    for (index, id) in WorkspacePanelId::ALL.iter().enumerate() {
        assert_eq!(snapshot.panels()[index].id, *id);
        assert_eq!(snapshot.states()[index].id(), *id);
    }
}

#[test]
fn panel_metadata_is_stable_and_matches_the_facade() {
    // The facade's `panel(id)` returns the same pure metadata as `for_id`, and each
    // panel's docked region is the canonical one.
    let expected_regions = [
        (WorkspacePanelId::ProjectBrowser, WorkspaceRegion::Left),
        (WorkspacePanelId::GameManifestEditor, WorkspaceRegion::Left),
        (WorkspacePanelId::LevelBrowser, WorkspaceRegion::Left),
        (WorkspacePanelId::RuntimeViewport, WorkspaceRegion::Center),
        (WorkspacePanelId::ObjectInspector, WorkspaceRegion::Right),
        (WorkspacePanelId::AssetBrowser, WorkspaceRegion::Right),
        (WorkspacePanelId::PackageExport, WorkspaceRegion::Right),
        (WorkspacePanelId::PlayControls, WorkspaceRegion::Bottom),
        (WorkspacePanelId::TimelineReplay, WorkspaceRegion::Bottom),
        (WorkspacePanelId::ConsoleLogViewer, WorkspaceRegion::Bottom),
        (WorkspacePanelId::Profiler, WorkspaceRegion::Bottom),
        (WorkspacePanelId::InputDebugger, WorkspaceRegion::Bottom),
    ];
    for (id, region) in expected_regions {
        let via_facade: WorkspacePanel = api().panel(id);
        let via_for_id = WorkspacePanel::for_id(id);
        assert_eq!(via_facade, via_for_id);
        assert_eq!(via_facade.id, id);
        assert_eq!(via_facade.region, region);
        assert!(!via_facade.title.is_empty(), "every panel has a display title");
    }
}

#[test]
fn default_panel_state_matches_its_id_for_every_panel() {
    // Building the default state by id yields a state whose recovered identity is
    // that same id — the tagged union stays consistent across all twelve.
    for id in WorkspacePanelId::ALL {
        let state = api().default_panel_state(id);
        assert_eq!(state.id(), id);
    }
}

#[test]
fn every_default_state_constructor_yields_sane_empty_defaults() {
    let api = api();

    // List/collection panels default to empty with no selection.
    let projects = api.project_browser_state();
    assert!(projects.projects().is_empty());
    assert!(projects.selected().is_none());

    let levels = api.level_browser_state();
    assert!(levels.levels().is_empty());
    assert!(levels.selected().is_none());

    let assets = api.asset_browser_state();
    assert!(assets.assets().is_empty());
    assert!(assets.selected().is_none());

    let inspector = api.object_inspector_state();
    assert!(inspector.selected().is_none());
    assert!(inspector.fields().is_empty());

    let console = api.console_log_viewer_state();
    assert!(console.records().is_empty());

    let profiler = api.profiler_state();
    assert!(profiler.samples().is_empty());

    let input = api.input_debugger_state();
    assert!(input.inputs().is_empty());

    let timeline = api.timeline_replay_state();
    assert!(timeline.ticks().is_empty());
    assert!(timeline.replay().is_none());

    // The manifest editor defaults every field to the explicit `<unset>` sentinel.
    let editor = api.game_manifest_editor_state();
    assert_eq!(editor.title(), "<unset>");
    assert_eq!(editor.version(), "<unset>");
    assert_eq!(editor.entrypoint(), "<unset>");
    assert_eq!(editor.default_level(), "<unset>");

    // The runtime viewport is an unattached placeholder.
    let viewport = api.runtime_viewport_state();
    assert!(!viewport.attached());
    assert_eq!(viewport.placeholder_label(), "Runtime Viewport Placeholder");

    // Play controls default to Edit with nothing staged.
    let controls = api.play_controls_state();
    assert_eq!(controls.mode(), WorkspaceMode::Edit);
    assert!(controls.launch().is_none());
    assert!(controls.drop_in().is_none());

    // Package export defaults to an idle, un-requested contract.
    let package = api.package_export_state();
    assert!(!package.requested());
    assert_eq!(package.target(), "");
}

#[test]
fn layout_snapshot_round_trips_every_mode() {
    // The mode is pure shell state: a snapshot reports back exactly the mode it was
    // built for, and the panel set is identical across every mode (the mode changes
    // emphasis, not which panels exist).
    for mode in ALL_MODES {
        let snapshot = api().layout_snapshot(mode);
        assert_eq!(snapshot.mode(), mode);
        assert_eq!(snapshot.panel_ids(), WorkspacePanelId::ALL.to_vec());
    }
}
