//! Typed-contract tests for the `axiom-workspace` scaffold.
//!
//! These prove the workspace's value contracts behave: manifests validate,
//! project descriptors reject empty fields, launch specs have a stable identity,
//! drop-in is additive, sessions start `Created`, records preserve artifact
//! order, and replay requests reference the record they replay. There is no
//! runtime here to test — real runtime integration does not exist yet (see
//! `TESTING.md`).

use axiom_kernel::{EntityId, SchemaVersion, Tick};
use axiom_workspace::{
    PlaySessionStatus, RecordedSnapshot, WorkspaceApi, WorkspaceError,
};

fn api() -> WorkspaceApi {
    WorkspaceApi::new()
}

fn sample_project() -> axiom_workspace::GameProject {
    api()
        .load_project("proj.demo", "Demo Project", "1.2.3", "main")
        .expect("valid project")
}

#[test]
fn workspace_manifest_validates_required_fields() {
    let ok = api().create_manifest("My Workspace", "/work/demo", SchemaVersion::new(1, 0));
    let manifest = ok.expect("a fully-specified manifest validates");
    assert_eq!(manifest.name(), "My Workspace");
    assert_eq!(manifest.workspace_root(), "/work/demo");
    assert_eq!(manifest.schema_version(), SchemaVersion::new(1, 0));

    assert_eq!(
        api().create_manifest("   ", "/work/demo", SchemaVersion::new(1, 0)),
        Err(WorkspaceError::MissingField {
            field: "workspace.name"
        })
    );
    assert_eq!(
        api().create_manifest("My Workspace", "", SchemaVersion::new(1, 0)),
        Err(WorkspaceError::MissingField {
            field: "workspace.root"
        })
    );
}

#[test]
fn game_project_rejects_empty_ids_names_and_entrypoints() {
    assert_eq!(
        api().load_project("", "Demo", "1.0.0", "main"),
        Err(WorkspaceError::MissingField { field: "project.id" })
    );
    assert_eq!(
        api().load_project("proj.demo", "  ", "1.0.0", "main"),
        Err(WorkspaceError::MissingField {
            field: "project.name"
        })
    );
    assert_eq!(
        api().load_project("proj.demo", "Demo", "", "main"),
        Err(WorkspaceError::MissingField {
            field: "project.version"
        })
    );
    assert_eq!(
        api().load_project("proj.demo", "Demo", "1.0.0", "   "),
        Err(WorkspaceError::MissingField {
            field: "project.entrypoint"
        })
    );

    let project = sample_project();
    assert_eq!(project.id(), "proj.demo");
    assert_eq!(project.entrypoint(), "main");
}

#[test]
fn launch_spec_identity_is_stable_for_same_project_version_config() {
    let a = api().launch_spec(&sample_project());
    let b = api().launch_spec(&sample_project());
    assert_eq!(
        a.identity(),
        b.identity(),
        "same project/version/config → same launch identity"
    );

    let other_project = api()
        .load_project("proj.demo", "Demo Project", "9.9.9", "main")
        .expect("valid");
    let c = api().launch_spec(&other_project);
    assert_ne!(
        a.identity(),
        c.identity(),
        "a different version → a different launch identity"
    );
}

#[test]
fn drop_in_is_additive_and_preserves_unrelated_launch_fields() {
    let base = api().launch_spec(&sample_project());
    let base_identity = base.identity();
    assert!(base.drop_in().is_none());

    let drop = api().drop_in("level.01", [1.0, 2.0, 3.0], 0.5, Some(EntityId::from_raw(7)));
    let dropped = base.clone().with_drop_in(drop);

    // Identity and every launch-identity field are unchanged by attaching drop-in.
    assert_eq!(dropped.identity(), base_identity);
    assert_eq!(dropped.project_id(), base.project_id());
    assert_eq!(dropped.game_version(), base.game_version());
    assert_eq!(dropped.entrypoint(), base.entrypoint());
    assert_eq!(dropped.fixed_step_nanos(), base.fixed_step_nanos());

    // The drop context is now attached and carries the editor-derived spawn data.
    let attached = dropped.drop_in().expect("drop-in attached");
    assert_eq!(attached.level_id().as_bytes(), b"level.01");
    assert_eq!(attached.selected_entity(), Some(EntityId::from_raw(7)));
}

#[test]
fn play_session_starts_created() {
    let session = api().play_session(api().launch_spec(&sample_project()));
    assert_eq!(session.status(), PlaySessionStatus::Created);
}

#[test]
fn session_record_preserves_input_order() {
    let session = api().play_session(api().launch_spec(&sample_project()));
    let mut record = api().record_session("rec.001", &session);
    record.record_input(api().recorded_input(Tick::new(10), 0xA1));
    record.record_input(api().recorded_input(Tick::new(11), 0xB2));
    record.record_input(api().recorded_input(Tick::new(12), 0xC3));

    let ticks: Vec<u64> = record.inputs().iter().map(|i| i.tick().raw()).collect();
    let codes: Vec<u32> = record.inputs().iter().map(|i| i.input_code()).collect();
    assert_eq!(ticks, [10, 11, 12]);
    assert_eq!(codes, [0xA1, 0xB2, 0xC3]);
}

#[test]
fn session_record_preserves_snapshot_hash_order() {
    let session = api().play_session(api().launch_spec(&sample_project()));
    let mut record = api().record_session("rec.002", &session);
    record.record_snapshot(api().recorded_snapshot(Tick::new(0), b"frame-0"));
    record.record_snapshot(api().recorded_snapshot(Tick::new(60), b"frame-60"));

    let order: Vec<u64> = record.snapshots().iter().map(|s| s.tick().raw()).collect();
    assert_eq!(order, [0, 60]);
    // The stored hash matches an independently recomputed one — a diagnostic index.
    let expected = RecordedSnapshot::of_bytes(Tick::new(60), b"frame-60").hash();
    assert_eq!(record.snapshots()[1].hash(), expected);
}

#[test]
fn replay_request_references_the_session_record_identity() {
    let session = api().play_session(api().launch_spec(&sample_project()));
    let mut record = api().record_session("rec.003", &session);
    record.record_input(api().recorded_input(Tick::new(1), 1));

    let replay = api().replay_request(&record);
    assert_eq!(replay.record_id(), record.record_id());
    assert_eq!(replay.expected_digest(), record.digest());
    assert_eq!(replay.launch_spec().identity(), record.launch_spec().identity());
}

#[test]
fn summary_is_deterministic() {
    let session = api().play_session(api().launch_spec(&sample_project()));
    let mut record = api().record_session("rec.004", &session);
    record.record_snapshot(api().recorded_snapshot(Tick::new(3), b"s"));

    assert_eq!(
        api().summarize(&record),
        api().summarize(&record),
        "the summary digest is a pure function of the record"
    );
}
