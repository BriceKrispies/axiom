//! Phase 1 — stored golden capture for the deterministic rotating-cube
//! vertical slice.
//!
//! The vertical-slice tests in `tests/vertical_slice.rs` prove determinism
//! *in memory* ("run the tick twice, the artifacts compare equal"). That
//! catches a regression only *within a single process run*; it cannot catch a
//! change that alters the output *across commits* — the artifacts are rebuilt
//! from the (changed) code on both sides of the `assert_eq!`, so they still
//! agree. This file closes that gap: it serializes each boundary artifact into
//! the kernel's canonical little-endian byte form (`BinaryWriter`, the same
//! encoding `Reflect` uses) and pins those bytes as **committed golden files**
//! under `tests/golden/`. A future change that alters any boundary's output is
//! caught here as a byte diff against the stored baseline.
//!
//! Why canonical bytes and not `Debug`/struct serde: the artifacts carry
//! `f32` fields (transforms, matrices, colours). `BinaryWriter::write_f32`
//! emits the exact IEEE-754 little-endian bit pattern, which is deterministic
//! across runs and platforms for the finite, non-NaN values these artifacts
//! hold (the audit confirmed these are the no-NaN, owned-data boundaries).
//! That is the same stance the engine already takes (`axiom-math` types are
//! `Reflect`, the retro_fps replay test encodes its HUD with `to_le_bytes`).
//!
//! **Boundaries are compared independently** (the test plan's rule): each of
//! the six vertical-slice boundaries is its own golden file, so a diff
//! localizes the regression to the first boundary that changed rather than to
//! one opaque blob. Tick 0 and tick 60 are both captured so the
//! tick-N-vs-tick-N+60 difference is itself pinned.
//!
//! ## Regenerating the goldens (the only sanctioned update path)
//!
//! Goldens are never hand-edited. A *missing* golden is captured on the next
//! run (the file is written and the test passes), so the very first run after
//! adding a boundary bootstraps its baseline; thereafter the committed file is
//! the source of truth and any drift fails. To re-capture after an *intended*
//! output change, either delete the affected golden file(s) and re-run, or set
//! the regen flag to force a rewrite of all of them in place — then review the
//! resulting diff as the evidence the change is what was intended:
//!
//! ```text
//! AXIOM_REGOLD=1 cargo test -p axiom-demo-rotating-cube --test golden_artifacts
//! ```
//!
//! An unexplained golden diff with no corresponding intended change is a
//! determinism bug, exactly as a coverage drop is.

use std::path::PathBuf;

use axiom_demo_rotating_cube::{
    DemoRotatingCubeApi, GpuCommandArtifact, GpuSubmissionArtifact, GpuSubmissionReportArtifact,
    RenderCommandArtifact, RenderCommandListArtifact, RenderInputArtifact,
    ResolvedResourcesArtifact, SceneSnapshotArtifact, VerticalSliceArtifact,
};
use axiom_kernel::BinaryWriter;
use axiom_math::{Mat4, Transform, Vec3};

// Each `enc_*` appends a fixed sequence of primitives, so the same artifact
// always yields the same bytes. Collections are length-prefixed (a u32 count)
// so a structural change (e.g. an extra node) shifts the bytes detectably.

fn enc_vec3(w: &mut BinaryWriter, v: Vec3) {
    v.write_to(w);
}

fn enc_mat4(w: &mut BinaryWriter, m: Mat4) {
    m.write_to(w);
}

fn enc_transform(w: &mut BinaryWriter, t: Transform) {
    t.write_to(w);
}

fn enc_f32_array<const N: usize>(w: &mut BinaryWriter, a: [f32; N]) {
    a.iter().for_each(|&f| w.write_f32(f));
}

fn enc_scene_snapshot(w: &mut BinaryWriter, s: &SceneSnapshotArtifact) {
    w.write_u32(s.nodes.len() as u32);
    for n in &s.nodes {
        w.write_u64(n.id);
        // Option<parent>: a presence byte then (if present) the id.
        w.write_bool(n.parent.is_some());
        w.write_u64(n.parent.unwrap_or(0));
        enc_transform(w, n.local);
        enc_transform(w, n.world);
    }
    w.write_u32(s.cameras.len() as u32);
    for c in &s.cameras {
        w.write_u64(c.node);
        w.write_f32(c.fovy_radians);
        w.write_f32(c.aspect);
        w.write_f32(c.near);
        w.write_f32(c.far);
    }
    w.write_u32(s.lights.len() as u32);
    for l in &s.lights {
        w.write_u64(l.node);
        enc_vec3(w, l.color);
        w.write_f32(l.intensity);
    }
    w.write_u32(s.renderables.len() as u32);
    for r in &s.renderables {
        w.write_u64(r.id);
        w.write_u64(r.node);
        w.write_u64(r.mesh_id);
        w.write_u64(r.material_id);
        w.write_u64(r.texture_id);
        w.write_u64(r.animation_id);
        w.write_bool(r.visible);
    }
    w.write_u32(s.tags.len() as u32);
    for t in &s.tags {
        w.write_u64(t.node);
        w.write_u32(t.kind_code);
    }
    w.write_u32(s.bounds.len() as u32);
    for b in &s.bounds {
        w.write_u64(b.node);
        enc_f32_array(w, b.half_extents);
    }
}

fn enc_resolved_resources(w: &mut BinaryWriter, r: &ResolvedResourcesArtifact) {
    w.write_u32(r.meshes.len() as u32);
    for m in &r.meshes {
        w.write_u64(m.id);
        w.write_u32(m.positions.len() as u32);
        m.positions.iter().for_each(|&p| enc_f32_array(w, p));
        w.write_u32(m.normals.len() as u32);
        m.normals.iter().for_each(|&n| enc_f32_array(w, n));
        w.write_u32(m.uvs.len() as u32);
        m.uvs.iter().for_each(|&u| enc_f32_array(w, u));
        w.write_u32(m.indices.len() as u32);
        m.indices.iter().for_each(|&i| w.write_u32(i));
    }
    w.write_u32(r.materials.len() as u32);
    for m in &r.materials {
        w.write_u64(m.id);
        enc_f32_array(w, m.base_color);
    }
}

fn enc_render_input(w: &mut BinaryWriter, ri: &RenderInputArtifact) {
    w.write_u32(ri.viewport_width);
    w.write_u32(ri.viewport_height);
    enc_f32_array(w, ri.clear_color);
    w.write_bool(ri.camera.is_some());
    if let Some(cam) = &ri.camera {
        enc_mat4(w, cam.view);
        enc_mat4(w, cam.projection);
    }
    w.write_u32(ri.lights.len() as u32);
    for l in &ri.lights {
        w.write_u32(l.kind_code);
        enc_vec3(w, l.vector_world);
        enc_vec3(w, l.color);
        w.write_f32(l.intensity);
    }
    w.write_u32(ri.meshes.len() as u32);
    for m in &ri.meshes {
        w.write_u64(m.id);
        w.write_u32(m.positions.len() as u32);
        m.positions.iter().for_each(|&p| enc_f32_array(w, p));
        w.write_u32(m.normals.len() as u32);
        m.normals.iter().for_each(|&n| enc_f32_array(w, n));
        w.write_u32(m.uvs.len() as u32);
        m.uvs.iter().for_each(|&u| enc_f32_array(w, u));
        w.write_u32(m.indices.len() as u32);
        m.indices.iter().for_each(|&i| w.write_u32(i));
    }
    w.write_u32(ri.materials.len() as u32);
    for m in &ri.materials {
        w.write_u64(m.id);
        enc_f32_array(w, m.base_color);
    }
    w.write_u32(ri.objects.len() as u32);
    for o in &ri.objects {
        enc_mat4(w, o.world);
        w.write_u32(o.mesh_idx);
        w.write_u32(o.material_idx);
        w.write_u64(o.texture_id);
        w.write_u32(o.pipeline);
        w.write_u32(o.tag);
        w.write_bool(o.visible);
    }
}

fn enc_render_command(w: &mut BinaryWriter, c: &RenderCommandArtifact) {
    w.write_u32(c.kind());
    if let Some(color) = c.as_clear_frame() {
        enc_f32_array(w, color);
    }
    if let Some((view, projection)) = c.as_set_camera() {
        enc_mat4(w, view);
        enc_mat4(w, projection);
    }
    if let Some(pipeline) = c.as_set_pipeline() {
        w.write_u32(pipeline);
    }
    if let Some(mesh) = c.as_set_mesh() {
        w.write_u64(mesh);
    }
    if let Some((material, texture)) = c.as_set_material() {
        w.write_u64(material);
        w.write_u64(texture);
    }
    if let Some((index_count, world, tag)) = c.as_draw_indexed() {
        w.write_u32(index_count);
        enc_mat4(w, world);
        w.write_u32(tag);
    }
}

fn enc_render_command_list(w: &mut BinaryWriter, list: &RenderCommandListArtifact) {
    w.write_u32(list.commands.len() as u32);
    list.commands.iter().for_each(|c| enc_render_command(w, c));
}

fn enc_gpu_command(w: &mut BinaryWriter, c: &GpuCommandArtifact) {
    w.write_u32(c.kind());
    if let Some(color) = c.as_clear_frame() {
        enc_f32_array(w, color);
    }
    if let Some((view, projection)) = c.as_set_camera() {
        enc_mat4(w, view);
        enc_mat4(w, projection);
    }
    if let Some(pipeline) = c.as_set_pipeline() {
        w.write_u32(pipeline);
    }
    if let Some(mesh) = c.as_set_mesh() {
        w.write_u64(mesh);
    }
    if let Some((material, texture)) = c.as_set_material() {
        w.write_u64(material);
        w.write_u64(texture);
    }
    if let Some((index_count, world)) = c.as_draw_indexed() {
        w.write_u32(index_count);
        enc_mat4(w, world);
    }
    w.write_bool(c.is_present());
}

fn enc_gpu_submission(w: &mut BinaryWriter, s: &GpuSubmissionArtifact) {
    w.write_u32(s.target_width);
    w.write_u32(s.target_height);
    w.write_u32(s.commands.len() as u32);
    s.commands.iter().for_each(|c| enc_gpu_command(w, c));
}

fn enc_gpu_submission_report(w: &mut BinaryWriter, r: &GpuSubmissionReportArtifact) {
    w.write_u32(r.target_width);
    w.write_u32(r.target_height);
    w.write_u32(r.command_count as u32);
    w.write_u32(r.command_kinds.len() as u32);
    r.command_kinds.iter().for_each(|&k| w.write_u32(k));
    w.write_u32(r.clear_count);
    w.write_u32(r.draw_count);
    w.write_u32(r.present_count);
}

/// Canonical bytes of the whole artifact tree (every boundary + frame
/// bookkeeping + cube identity/transform), in a fixed field order.
fn enc_full_artifact(a: &VerticalSliceArtifact) -> Vec<u8> {
    let mut w = BinaryWriter::new();
    w.write_u64(a.tick);
    w.write_u64(a.engine_frame_index);
    w.write_u64(a.host_frame_sequence);
    w.write_u32(a.runtime_step_count);
    w.write_u64(a.cube.node_id);
    w.write_u64(a.cube.mesh_id);
    w.write_u64(a.cube.material_id);
    enc_transform(&mut w, a.cube_transform.local);
    enc_transform(&mut w, a.cube_transform.world);
    enc_scene_snapshot(&mut w, &a.scene_snapshot);
    enc_resolved_resources(&mut w, &a.resolved_resources);
    enc_render_input(&mut w, &a.render_input);
    enc_render_command_list(&mut w, &a.render_command_list);
    enc_gpu_submission(&mut w, &a.gpu_submission);
    enc_gpu_submission_report(&mut w, &a.gpu_submission_report);
    w.into_bytes()
}

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("golden");
    p.push(format!("{name}.bin"));
    p
}

/// Compare `actual` to the committed golden `name`. A *missing* golden (or the
/// `AXIOM_REGOLD` force flag) writes the file and passes — the sanctioned
/// capture/regeneration path. An *existing* golden must match byte-for-byte.
fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    let force = std::env::var_os("AXIOM_REGOLD").is_some();
    let existing = std::fs::read(&path).ok();
    match existing {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}` ({} bytes actual vs {} bytes golden): \
             the rotating-cube output drifted. If this change is intended, bump \
             the relevant version and re-capture (delete this golden or set \
             AXIOM_REGOLD=1).",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

fn run(tick: u64) -> VerticalSliceArtifact {
    DemoRotatingCubeApi::new().run_tick(tick)
}

#[test]
fn golden_scene_snapshot_tick0() {
    let a = run(0);
    let mut w = BinaryWriter::new();
    enc_scene_snapshot(&mut w, &a.scene_snapshot);
    assert_golden("scene_snapshot_tick0", w.as_bytes());
}

#[test]
fn golden_resolved_resources_tick0() {
    let a = run(0);
    let mut w = BinaryWriter::new();
    enc_resolved_resources(&mut w, &a.resolved_resources);
    assert_golden("resolved_resources_tick0", w.as_bytes());
}

#[test]
fn golden_render_input_tick0() {
    let a = run(0);
    let mut w = BinaryWriter::new();
    enc_render_input(&mut w, &a.render_input);
    assert_golden("render_input_tick0", w.as_bytes());
}

#[test]
fn golden_render_command_list_tick0() {
    let a = run(0);
    let mut w = BinaryWriter::new();
    enc_render_command_list(&mut w, &a.render_command_list);
    assert_golden("render_command_list_tick0", w.as_bytes());
}

#[test]
fn golden_gpu_submission_tick0() {
    let a = run(0);
    let mut w = BinaryWriter::new();
    enc_gpu_submission(&mut w, &a.gpu_submission);
    assert_golden("gpu_submission_tick0", w.as_bytes());
}

#[test]
fn golden_gpu_submission_report_tick0() {
    let a = run(0);
    let mut w = BinaryWriter::new();
    enc_gpu_submission_report(&mut w, &a.gpu_submission_report);
    assert_golden("gpu_submission_report_tick0", w.as_bytes());
}

#[test]
fn golden_full_artifact_tick0() {
    assert_golden("full_artifact_tick0", &enc_full_artifact(&run(0)));
}

#[test]
fn golden_full_artifact_tick60() {
    let mut demo = DemoRotatingCubeApi::new();
    let mut a = demo.run_tick(0);
    for tick in 1..=60 {
        a = demo.run_tick(tick);
    }
    assert_golden("full_artifact_tick60", &enc_full_artifact(&a));
}

// Sanity guards on the golden machinery itself, so the harness can't silently
// pass on absent/degenerate data.

#[test]
fn the_two_tick_goldens_differ() {
    let tick0 = enc_full_artifact(&run(0));
    let mut demo = DemoRotatingCubeApi::new();
    let mut later = demo.run_tick(0);
    for tick in 1..=60 {
        later = demo.run_tick(tick);
    }
    assert_ne!(tick0, enc_full_artifact(&later));
}

#[test]
fn encoding_is_stable_within_a_run() {
    let a = run(0);
    assert_eq!(enc_full_artifact(&a), enc_full_artifact(&a));
}
