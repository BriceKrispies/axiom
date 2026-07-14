//! Panel-state value tests for the `axiom-workspace` app.
//!
//! These prove the individual panel states are **pure, ordered data contracts**,
//! not a runtime: play/drop-in controls only *hold* launch intent as data, the
//! timeline/console/profiler/input/asset/project/level panels preserve insertion
//! order exactly, and the package-export panel is a request/status contract that
//! performs no packaging. Identity/layout of the twelve-panel shell lives in
//! `workspace_panels.rs`.

use axiom_kernel::{EntityId, StableHash, Tick};
use axiom_workspace::{
    AssetEntry, ConsoleLevel, ConsoleRecord, GameProject, InputEventRecord, InspectorField,
    LevelEntry, PackageStatus, ProfilerSample, ProjectEntry, RuntimeViewportView, TimelineTick,
    WorkspaceApi, WorkspaceMode,
};

fn api() -> WorkspaceApi {
    WorkspaceApi::new()
}

fn sample_project() -> GameProject {
    api()
        .load_project("proj.demo", "Demo Project", "1.2.3", "main")
        .expect("valid project")
}

#[test]
fn runtime_viewport_toggles_between_placeholder_and_backend_triptych() {
    // The viewport defaults to the plain placeholder view, and `with_view` is a
    // pure data toggle to the backend-comparison view and back — it attaches no
    // runtime and embeds no surface.
    let viewport = api().runtime_viewport_state();
    assert_eq!(viewport.view(), RuntimeViewportView::Placeholder);
    assert!(!viewport.attached());

    let triptych = viewport.with_view(RuntimeViewportView::BackendTriptych);
    assert_eq!(triptych.view(), RuntimeViewportView::BackendTriptych);
    assert!(
        !triptych.attached(),
        "toggling the view attaches no runtime"
    );

    let back = triptych.with_view(RuntimeViewportView::Placeholder);
    assert_eq!(back.view(), RuntimeViewportView::Placeholder);
}

#[test]
fn runtime_viewport_names_the_three_comparison_backends_in_order() {
    // The comparison view names exactly the gallery's three backends, in the
    // engine's own preference order, as fixed canonical data.
    let viewport = api().runtime_viewport_state();
    let backends = viewport.comparison_backends();
    let ids: Vec<&str> = backends.iter().map(|b| b.id()).collect();
    let names: Vec<&str> = backends.iter().map(|b| b.name()).collect();
    assert_eq!(ids, ["webgpu", "webgl2", "canvas2d"]);
    assert_eq!(names, ["WebGPU", "WebGL2", "Canvas2D"]);
    assert!(backends.iter().all(|b| !b.note().is_empty()));
}

#[test]
fn play_controls_are_pure_state_only_transitions() {
    // Play Controls holds mode as shell state. Setting the mode is a pure data
    // transition — it never stages a launch or drop-in as a side effect.
    let controls = api().play_controls_state();
    assert_eq!(controls.mode(), WorkspaceMode::Edit);

    let mut controls = controls;
    controls.set_mode(WorkspaceMode::Play);
    assert_eq!(controls.mode(), WorkspaceMode::Play);
    assert!(
        controls.launch().is_none(),
        "changing mode does not launch anything"
    );
    assert!(controls.drop_in().is_none());

    controls.set_mode(WorkspaceMode::Package);
    assert_eq!(controls.mode(), WorkspaceMode::Package);
}

#[test]
fn play_controls_hold_launch_and_drop_in_as_data() {
    // Drop-In state is only launch + drop-in data attached to the panel. Staging
    // them stores readable values and produces no side effect (mode stays put).
    let launch = api().launch_spec(&sample_project());
    let drop = api().drop_in(
        "level.aa",
        [1.0, 2.0, 3.0],
        0.25,
        Some(EntityId::from_raw(9)),
    );

    let staged = api()
        .play_controls_state()
        .with_launch(launch.clone())
        .with_drop_in(drop);

    let held_launch = staged.launch().expect("launch staged as data");
    assert_eq!(held_launch.identity(), launch.identity());
    assert_eq!(held_launch.project_id(), "proj.demo");

    let held_drop = staged.drop_in().expect("drop-in staged as data");
    assert_eq!(held_drop.level_id().as_bytes(), b"level.aa");
    assert_eq!(held_drop.selected_entity(), Some(EntityId::from_raw(9)));

    // Staging intent is pure data: the mode is unchanged (still the Edit default).
    assert_eq!(staged.mode(), WorkspaceMode::Edit);
}

#[test]
fn timeline_replay_preserves_tick_order_and_holds_a_replay_request() {
    let mut timeline = api().timeline_replay_state();

    // Ticks are stored in exactly the order they are marked — not sorted.
    timeline.mark_tick(TimelineTick::new(Tick::new(5), None));
    timeline.mark_tick(TimelineTick::new(
        Tick::new(2),
        Some(StableHash::of_bytes(b"snap")),
    ));
    timeline.mark_tick(TimelineTick::new(Tick::new(9), None));
    let order: Vec<u64> = timeline
        .ticks()
        .iter()
        .map(|entry| entry.tick.raw())
        .collect();
    assert_eq!(
        order,
        [5, 2, 9],
        "tick entries keep insertion order exactly"
    );
    assert!(timeline.ticks()[1].snapshot.is_some());

    // Replay-mode state references a ReplayRequest purely as data.
    let session = api().play_session(api().launch_spec(&sample_project()));
    let mut record = api().record_session("rec.tl", &session);
    record.record_input(api().recorded_input(Tick::new(1), 1));
    let replay = api().replay_request(&record);

    assert!(timeline.replay().is_none());
    timeline.set_replay(Some(replay.clone()));
    let held = timeline.replay().expect("replay held as data");
    assert_eq!(held.record_id(), replay.record_id());
    assert_eq!(held.expected_digest(), replay.expected_digest());
}

#[test]
fn console_log_viewer_preserves_insertion_order() {
    let mut console = api().console_log_viewer_state();
    console.record(ConsoleRecord::new(ConsoleLevel::Info, "boot", Tick::new(0)));
    console.record(ConsoleRecord::new(
        ConsoleLevel::Warn,
        "slow frame",
        Tick::new(1),
    ));
    console.record(ConsoleRecord::new(
        ConsoleLevel::Error,
        "panic",
        Tick::new(2),
    ));

    let messages: Vec<&str> = console
        .records()
        .iter()
        .map(|r| r.message.as_str())
        .collect();
    assert_eq!(messages, ["boot", "slow frame", "panic"]);
    let levels: Vec<ConsoleLevel> = console.records().iter().map(|r| r.level).collect();
    assert_eq!(
        levels,
        [ConsoleLevel::Info, ConsoleLevel::Warn, ConsoleLevel::Error]
    );
}

#[test]
fn profiler_preserves_sample_order() {
    let mut profiler = api().profiler_state();
    profiler.record_sample(ProfilerSample::new("frame", 1_600, Tick::new(0)));
    profiler.record_sample(ProfilerSample::new("render", 900, Tick::new(0)));
    profiler.record_sample(ProfilerSample::new("submit", 120, Tick::new(0)));

    let labels: Vec<&str> = profiler
        .samples()
        .iter()
        .map(|s| s.label.as_str())
        .collect();
    assert_eq!(labels, ["frame", "render", "submit"]);
    let micros: Vec<u64> = profiler.samples().iter().map(|s| s.micros).collect();
    assert_eq!(micros, [1_600, 900, 120]);
}

#[test]
fn input_debugger_preserves_input_order() {
    let mut input = api().input_debugger_state();
    input.record_input(InputEventRecord::new(Tick::new(0), 1, "forward-down"));
    input.record_input(InputEventRecord::new(Tick::new(1), 2, "forward-up"));
    input.record_input(InputEventRecord::new(Tick::new(2), 3, "jump"));

    let codes: Vec<u32> = input.inputs().iter().map(|e| e.code).collect();
    assert_eq!(codes, [1, 2, 3]);
    let labels: Vec<&str> = input.inputs().iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["forward-down", "forward-up", "jump"]);
}

#[test]
fn asset_browser_preserves_asset_order() {
    let mut assets = api().asset_browser_state();
    assets.add_asset(AssetEntry::new("m.cube", "mesh", "Cube"));
    assets.add_asset(AssetEntry::new("t.grid", "texture", "Grid"));
    assets.add_asset(AssetEntry::new("m.sphere", "mesh", "Sphere"));

    let ids: Vec<&str> = assets.assets().iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["m.cube", "t.grid", "m.sphere"]);
    let kinds: Vec<&str> = assets.assets().iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(kinds, ["mesh", "texture", "mesh"]);
}

#[test]
fn project_and_level_browsers_preserve_order() {
    let mut projects = api().project_browser_state();
    projects.add_project(ProjectEntry::new("p.one", "One"));
    projects.add_project(ProjectEntry::new("p.two", "Two"));
    projects.add_project(ProjectEntry::new("p.three", "Three"));
    let project_ids: Vec<&str> = projects.projects().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(project_ids, ["p.one", "p.two", "p.three"]);
    projects.select(Some(1));
    assert_eq!(projects.selected(), Some(1));

    let mut levels = api().level_browser_state();
    levels.add_level(LevelEntry::new("l.01", "Level 1"));
    levels.add_level(LevelEntry::new("l.02", "Level 2"));
    let level_ids: Vec<&str> = levels.levels().iter().map(|l| l.id.as_str()).collect();
    assert_eq!(level_ids, ["l.01", "l.02"]);
}

#[test]
fn object_inspector_preserves_field_order_and_selection() {
    let mut inspector = api().object_inspector_state();
    assert!(inspector.selected().is_none());

    inspector.select(Some(EntityId::from_raw(3)));
    inspector.add_field(InspectorField::new("hp", "100"));
    inspector.add_field(InspectorField::new("name", "hero"));
    inspector.add_field(InspectorField::new("pos", "0,0,0"));

    assert_eq!(inspector.selected(), Some(EntityId::from_raw(3)));
    let names: Vec<&str> = inspector.fields().iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["hp", "name", "pos"]);
    let values: Vec<&str> = inspector
        .fields()
        .iter()
        .map(|f| f.value.as_str())
        .collect();
    assert_eq!(values, ["100", "hero", "0,0,0"]);
}

#[test]
fn game_manifest_editor_edits_are_pure_immutable_field_updates() {
    let edited = api()
        .game_manifest_editor_state()
        .with_title("My Game")
        .with_version("1.0.0")
        .with_entrypoint("boot")
        .with_default_level("l.01");

    assert_eq!(edited.title(), "My Game");
    assert_eq!(edited.version(), "1.0.0");
    assert_eq!(edited.entrypoint(), "boot");
    assert_eq!(edited.default_level(), "l.01");
}

#[test]
fn package_export_is_a_request_status_contract_only() {
    let mut package = api().package_export_state();

    // Default is an idle, un-requested contract with no target.
    assert_eq!(package.status(), PackageStatus::Idle);
    assert_eq!(package.target(), "");
    assert!(!package.requested());

    // Requesting an export ONLY flips the request/status/target — no real packaging
    // runs, so the status advances no further than `Requested`.
    package.request("dist/game.axpkg");
    assert!(package.requested());
    assert_eq!(package.status(), PackageStatus::Requested);
    assert_eq!(package.target(), "dist/game.axpkg");
    assert_ne!(package.status(), PackageStatus::InProgress);
    assert_ne!(package.status(), PackageStatus::Done);
}
