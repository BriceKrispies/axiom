//! Integration tests for the headless deterministic rotating-cube
//! vertical slice.
//!
//! These prove the slice's determinism and boundary completeness through
//! the public facade, plus the module-isolation invariants the app relies
//! on (read directly from the module manifests). The Axiom architecture
//! checker (`cargo xtask check-architecture`, wired into the workspace
//! `cargo test` run) is the mechanical enforcer of the layer/module/app
//! dependency rules; these tests are the app-level companion proofs.

use axiom_demo_rotating_cube::{DemoRotatingCubeApi, GpuCommandArtifact, RenderCommandArtifact};

/// Run a fresh demo for exactly one tick.
fn run_fresh(tick: u64) -> axiom_demo_rotating_cube::VerticalSliceArtifact {
    DemoRotatingCubeApi::new().run_tick(tick)
}

// ---------------------------------------------------------------------------
// Manifest / isolation proofs.
// ---------------------------------------------------------------------------

#[test]
fn app_manifest_classifies_as_an_app_with_declared_layers_and_modules() {
    let app_toml = include_str!("../app.toml");
    assert!(app_toml.contains("[app]"), "app.toml must declare [app]");
    assert!(app_toml.contains("crate_name = \"axiom-demo-rotating-cube\""));
    for layer in ["kernel", "runtime", "math", "host", "frame", "ecs", "introspect"] {
        assert!(
            app_toml.contains(&format!("\"{layer}\"")),
            "app.toml must allow layer `{layer}`"
        );
    }
    // The scene module (ECS-native) is the shared world model; the app also
    // composes resources/render/webgpu.
    for module in ["scene", "resources", "render", "webgpu"] {
        assert!(
            app_toml.contains(&format!("\"{module}\"")),
            "app.toml must allow module `{module}`"
        );
    }
}

#[test]
fn modules_declare_no_dependency_on_other_modules() {
    // The app composes these modules; each must remain isolated
    // (`allowed_modules = []`).
    for manifest in [
        include_str!("../../../modules/axiom-scene/module.toml"),
        include_str!("../../../modules/axiom-resources/module.toml"),
        include_str!("../../../modules/axiom-render/module.toml"),
        include_str!("../../../modules/axiom-webgpu/module.toml"),
    ] {
        assert!(
            manifest.contains("allowed_modules = []"),
            "every composed module must declare `allowed_modules = []`"
        );
    }
}

// ---------------------------------------------------------------------------
// Determinism proofs.
// ---------------------------------------------------------------------------

#[test]
fn tick_zero_replay_is_byte_for_byte_equal() {
    assert_eq!(run_fresh(0), run_fresh(0));
}

#[test]
fn render_command_list_is_deterministic_for_the_same_tick() {
    assert_eq!(
        run_fresh(60).render_command_list,
        run_fresh(60).render_command_list
    );
}

#[test]
fn gpu_submission_report_is_deterministic_for_the_same_tick() {
    assert_eq!(
        run_fresh(60).gpu_submission_report,
        run_fresh(60).gpu_submission_report
    );
}

#[test]
fn driven_sequence_replays_identically() {
    let drive = || {
        let mut demo = DemoRotatingCubeApi::new();
        for tick in 0..60 {
            demo.run_tick(tick);
        }
        demo.run_tick(60)
    };
    assert_eq!(drive(), drive());
}

// ---------------------------------------------------------------------------
// Cube transform proof.
// ---------------------------------------------------------------------------

#[test]
fn cube_world_transform_changes_as_the_simulation_advances() {
    // The cube spin is driven by the simulation clock (the cube-spin system),
    // so the rotation differs between an early frame and a later one across a
    // driven sequence — not between two fresh single-step runs.
    let mut demo = DemoRotatingCubeApi::new();
    let first = demo.run_tick(0);
    let mut later = first.clone();
    for tick in 1..=60 {
        later = demo.run_tick(tick);
    }
    assert_ne!(
        first.cube_transform.world, later.cube_transform.world,
        "the rotating cube must have a different world transform 60 ticks later"
    );
    // The drawn object's world matrix must differ too.
    assert_ne!(draw_world(&first), draw_world(&later));
}

/// Extract the draw command's world matrix from the render command list.
fn draw_world(
    artifact: &axiom_demo_rotating_cube::VerticalSliceArtifact,
) -> axiom_math::Mat4 {
    artifact
        .render_command_list
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommandArtifact::DrawIndexed { world, .. } => Some(*world),
            _ => None,
        })
        .expect("the cube draw command is present")
}

// ---------------------------------------------------------------------------
// Boundary-completeness proofs.
// ---------------------------------------------------------------------------

#[test]
fn every_boundary_artifact_is_present_and_well_formed() {
    let f = run_fresh(0);

    // Frame bookkeeping.
    assert_eq!(f.tick, 0);
    assert_eq!(f.engine_frame_index, 0);
    assert_eq!(f.host_frame_sequence, 1);
    assert_eq!(f.runtime_step_count, 1);

    // Cube identity: the renderable child, the cube mesh, the material.
    assert!(f.cube.node_id > 0);
    assert!(f.cube.mesh_id > 0);
    assert!(f.cube.material_id > 0);

    // Boundary 1 — scene snapshot: 4 nodes, 1 camera, 1 light, 1 renderable.
    assert_eq!(f.scene_snapshot.nodes.len(), 4);
    assert_eq!(f.scene_snapshot.cameras.len(), 1);
    assert_eq!(f.scene_snapshot.lights.len(), 1);
    assert_eq!(f.scene_snapshot.renderables.len(), 1);

    // Boundary 2 — resolved resources: one cube mesh (24 verts, 36 indices)
    // and one material.
    assert_eq!(f.resolved_resources.meshes.len(), 1);
    assert_eq!(f.resolved_resources.materials.len(), 1);
    assert_eq!(f.resolved_resources.meshes[0].positions.len(), 24);
    assert_eq!(f.resolved_resources.meshes[0].indices.len(), 36);

    // Boundary 3 — render input: a camera, one light, one mesh, one
    // material, one object.
    assert!(f.render_input.camera.is_some());
    assert_eq!(f.render_input.lights.len(), 1);
    assert_eq!(f.render_input.meshes.len(), 1);
    assert_eq!(f.render_input.materials.len(), 1);
    assert_eq!(f.render_input.objects.len(), 1);

    // Boundary 4 — render command list: the six expected commands in order.
    let kinds: Vec<_> = f
        .render_command_list
        .commands
        .iter()
        .map(std::mem::discriminant)
        .collect();
    assert_eq!(f.render_command_list.commands.len(), 6);
    assert!(matches!(
        f.render_command_list.commands[0],
        RenderCommandArtifact::ClearFrame { .. }
    ));
    assert!(matches!(
        f.render_command_list.commands[1],
        RenderCommandArtifact::SetCamera { .. }
    ));
    assert!(matches!(
        f.render_command_list.commands[2],
        RenderCommandArtifact::SetPipeline { .. }
    ));
    assert!(matches!(
        f.render_command_list.commands.last().unwrap(),
        RenderCommandArtifact::DrawIndexed { index_count: 36, .. }
    ));
    // (the discriminant vec exists only to assert the list is inspectable)
    assert_eq!(kinds.len(), 6);

    // Boundary 5 — GPU submission: every render command mapped plus a
    // trailing Present.
    assert_eq!(f.gpu_submission.commands.len(), 7);
    assert_eq!(
        *f.gpu_submission.commands.last().unwrap(),
        GpuCommandArtifact::Present
    );
    assert_eq!(f.gpu_submission.target_width, 800);
    assert_eq!(f.gpu_submission.target_height, 600);

    // Boundary 6 — GPU submission report: clear + draw + present recorded.
    assert_eq!(f.gpu_submission_report.command_count, 7);
    assert_eq!(f.gpu_submission_report.clear_count, 1);
    assert_eq!(f.gpu_submission_report.draw_count, 1);
    assert_eq!(f.gpu_submission_report.present_count, 1);
    assert_eq!(f.gpu_submission_report.target_width, 800);
    assert_eq!(f.gpu_submission_report.target_height, 600);
}

// ---------------------------------------------------------------------------
// Introspection proofs — the demo drives the Layer-05 IntrospectApi.
// ---------------------------------------------------------------------------

#[test]
fn introspection_records_each_tick_and_is_queryable() {
    use axiom_introspect::FrameReport;

    let mut demo = DemoRotatingCubeApi::new();
    let mut indices = Vec::new();
    for tick in 0..=60 {
        indices.push(demo.run_tick(tick).engine_frame_index);
    }

    // One retained report per tick.
    assert_eq!(demo.recent_frames(1000).len(), 61);

    // Every observed frame index is queryable; indices are monotonic; each
    // frame ran the cube-spin system and captured the angle it produced. The
    // spin is keyed to the simulation tick, which is one ahead of the 0-based
    // engine frame index (the first step is sim tick 1).
    let expected_angle = |index: u64| ((index + 1) % 360) as f32 * std::f32::consts::PI / 180.0;
    for (i, &index) in indices.iter().enumerate() {
        let report = demo.describe_frame(index).expect("frame is retained");
        assert_eq!(report.engine_frame_index(), index);

        assert_eq!(report.systems().len(), 1);
        assert_eq!(report.systems()[0].name(), "cube-spin");
        assert!(report.systems()[0].succeeded());

        let angle = report
            .metrics()
            .iter()
            .find(|m| m.name() == "cube.angle_rad")
            .and_then(|m| m.value().as_float())
            .expect("the cube angle metric is present");
        assert!((angle - expected_angle(index)).abs() < 1e-6);

        if i > 0 {
            assert!(indices[i - 1] < index);
        }
    }

    // The captured angle genuinely differs frame to frame — introspection is
    // recording state that changes, not just bookkeeping.
    let angle_at = |idx: u64| {
        demo.describe_frame(idx)
            .unwrap()
            .metrics()
            .iter()
            .find(|m| m.name() == "cube.angle_rad")
            .unwrap()
            .value()
            .as_float()
            .unwrap()
    };
    assert_ne!(angle_at(0), angle_at(60));

    // The serialized snapshot of the latest frame round-trips to an equal
    // report — the byte channel an external agent would read.
    let bytes = demo.introspection_snapshot().expect("a tick has run");
    let decoded = FrameReport::from_bytes(&bytes).unwrap();
    assert_eq!(decoded.engine_frame_index(), *indices.last().unwrap());

    // An index that was never observed misses.
    assert!(demo.describe_frame(9_999).is_none());
}

#[test]
fn engine_frame_indices_increase_across_a_driven_sequence() {
    let mut demo = DemoRotatingCubeApi::new();
    let f0 = demo.run_tick(0);
    let f1 = demo.run_tick(1);
    let f2 = demo.run_tick(2);
    assert!(f0.engine_frame_index < f1.engine_frame_index);
    assert!(f1.engine_frame_index < f2.engine_frame_index);
    assert!(f0.host_frame_sequence < f1.host_frame_sequence);
}
