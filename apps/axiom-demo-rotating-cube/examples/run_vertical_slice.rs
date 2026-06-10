//! Run the headless deterministic rotating-cube vertical slice and print a
//! human-readable summary of every boundary artifact for a few ticks.
//!
//! This is a *demonstration* runner — the authoritative proofs live in
//! `tests/vertical_slice.rs`. There is no window: `axiom-webgpu` records a
//! deterministic submission report instead of presenting pixels.
//!
//! ```sh
//! cargo run -p axiom-demo-rotating-cube --example run_vertical_slice
//! ```

use axiom_demo_rotating_cube::{DemoRotatingCubeApi, RenderCommandArtifact, VerticalSliceArtifact};

fn main() {
    println!("Axiom — headless deterministic rotating-cube vertical slice");
    println!("(no window; axiom-webgpu records a submission report, no pixels)\n");

    // Drive a fresh app across a sequence so engine/host counters advance
    // monotonically, and capture tick 0 and tick 60.
    let mut demo = DemoRotatingCubeApi::new();
    let mut tick_0 = None;
    let mut tick_60 = None;
    for tick in 0..=60 {
        let artifact = demo.run_tick(tick);
        if tick == 0 {
            tick_0 = Some(artifact);
        } else if tick == 60 {
            tick_60 = Some(artifact);
        }
    }
    let tick_0 = tick_0.unwrap();
    let tick_60 = tick_60.unwrap();

    print_summary(&tick_0);
    print_summary(&tick_60);

    // Proof 1/3: replaying tick 0 from a fresh app is byte-equal.
    let replay_0 = DemoRotatingCubeApi::new().run_tick(0);
    let fresh_0 = DemoRotatingCubeApi::new().run_tick(0);
    println!("== determinism ==");
    println!(
        "  tick 0 replayed from a fresh app is byte-equal : {}",
        replay_0 == fresh_0
    );

    // Proof 2: tick 60 cube world transform differs from tick 0.
    println!(
        "  tick 60 cube world transform differs from tick 0: {}",
        tick_0.cube_transform.world != tick_60.cube_transform.world
    );
    println!(
        "  tick 60 drawn world matrix differs from tick 0  : {}",
        draw_world(&tick_0) != draw_world(&tick_60)
    );
}

fn print_summary(a: &VerticalSliceArtifact) {
    println!("== tick {} ==", a.tick);
    println!(
        "  engine_frame_index={}  host_frame_sequence={}  runtime_steps={}",
        a.engine_frame_index, a.host_frame_sequence, a.runtime_step_count
    );
    println!(
        "  cube: node_id={} mesh_id={} material_id={}",
        a.cube.node_id, a.cube.mesh_id, a.cube.material_id
    );
    println!(
        "  cube world translation = {:?}",
        a.cube_transform.world.translation
    );
    println!(
        "  SceneSnapshot      : {} nodes, {} cameras, {} lights, {} renderables",
        a.scene_snapshot.nodes.len(),
        a.scene_snapshot.cameras.len(),
        a.scene_snapshot.lights.len(),
        a.scene_snapshot.renderables.len()
    );
    println!(
        "  ResolvedResources  : {} meshes ({} verts / {} indices), {} materials",
        a.resolved_resources.meshes.len(),
        a.resolved_resources
            .meshes
            .first()
            .map_or(0, |m| m.positions.len()),
        a.resolved_resources
            .meshes
            .first()
            .map_or(0, |m| m.indices.len()),
        a.resolved_resources.materials.len()
    );
    println!(
        "  RenderInput        : camera={} lights={} meshes={} materials={} objects={}",
        a.render_input.camera.is_some(),
        a.render_input.lights.len(),
        a.render_input.meshes.len(),
        a.render_input.materials.len(),
        a.render_input.objects.len()
    );
    println!(
        "  RenderCommandList  : {} commands {:?}",
        a.render_command_list.commands.len(),
        a.render_command_list
            .commands
            .iter()
            .map(command_name)
            .collect::<Vec<_>>()
    );
    println!(
        "  GpuSubmission      : {}x{}, {} commands",
        a.gpu_submission.target_width,
        a.gpu_submission.target_height,
        a.gpu_submission.commands.len()
    );
    println!(
        "  GpuSubmissionReport: {} commands (clear={} draw={} present={})\n",
        a.gpu_submission_report.command_count,
        a.gpu_submission_report.clear_count,
        a.gpu_submission_report.draw_count,
        a.gpu_submission_report.present_count
    );
}

fn command_name(c: &RenderCommandArtifact) -> &'static str {
    match c {
        RenderCommandArtifact::ClearFrame { .. } => "ClearFrame",
        RenderCommandArtifact::SetCamera { .. } => "SetCamera",
        RenderCommandArtifact::SetPipeline { .. } => "SetPipeline",
        RenderCommandArtifact::SetMesh { .. } => "SetMesh",
        RenderCommandArtifact::SetMaterial { .. } => "SetMaterial",
        RenderCommandArtifact::DrawIndexed { .. } => "DrawIndexed",
    }
}

fn draw_world(a: &VerticalSliceArtifact) -> axiom_math::Mat4 {
    a.render_command_list
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommandArtifact::DrawIndexed { world, .. } => Some(*world),
            _ => None,
        })
        .expect("cube draw command present")
}
