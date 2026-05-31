//! Deterministic tests for the render frame-capture boundary
//! ([`RenderApi::capture_receipt`] → `RenderReceipt`).
//!
//! These prove the capture is a reproducible, engine-owned artifact: the same
//! frame captured twice is byte-identical, and a meaningful render-visible
//! change (the cube's world transform) changes the capture. There are **no
//! pixels** here — no screenshot, no GPU readback, no canvas, no presentation.
//! Everything is built through the deterministic `RenderApi` contract.

use axiom_kernel::{FrameIndex, Tick};
use axiom_math::{Mat4, Quat, Transform, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;

const VIEWPORT_W: u32 = 800;
const VIEWPORT_H: u32 = 600;

/// The deterministic rotating-cube angle for a tick: one revolution / 360
/// ticks about +Y. Matches the engine's rotating-cube slice.
fn cube_rotation_angle(tick: u64) -> f32 {
    ((tick % 360) as f32) * std::f32::consts::PI / 180.0
}

/// The cube's world matrix for a rotation angle about +Y.
fn cube_world(angle: f32) -> Mat4 {
    let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, angle).expect("unit axis, finite angle");
    Transform::from_rotation(rotation).to_matrix()
}

/// Build the deterministic rotating-cube frame capture for `(frame, tick)`
/// with the cube placed by `world`, and return its serialized bytes + hash.
/// The `RenderReceipt` itself is module-internal, so it lives only as an
/// inferred local here and is observed through `RenderApi`.
fn cube_capture(api: &RenderApi, frame: u64, tick: u64, world: Mat4) -> (Vec<u8>, u64) {
    let mut input = api.new_input(VIEWPORT_W, VIEWPORT_H);
    api.set_input_clear_color(&mut input, [0.05, 0.06, 0.08, 1.0]);
    api.set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
    api.add_input_directional_light(&mut input, Vec3::new(0.3, -1.0, 0.4), Vec3::ONE, 1.0);
    let mesh = api.add_input_mesh(
        &mut input,
        1,
        vec![Vec3::ZERO; 24],
        vec![Vec3::UNIT_Y; 24],
        vec![Vec2::ZERO; 24],
        (0..36).collect(),
    );
    let material = api.add_input_basic_lit_material(&mut input, 1, Vec4::new(0.8, 0.4, 0.2, 1.0));
    api.add_input_object(&mut input, world, mesh, material, true);

    let list = api.build_command_list(&input);
    let receipt = api.capture_receipt(FrameIndex::new(frame), Tick::new(tick), &list);
    (api.receipt_bytes(&receipt).to_vec(), api.receipt_hash(&receipt))
}

/// Build a two-object frame capture; `swap` reverses the object order so the
/// emitted command stream is the same commands in a different order.
fn two_object_capture(api: &RenderApi, swap: bool) -> Vec<u8> {
    let mut input = api.new_input(VIEWPORT_W, VIEWPORT_H);
    api.set_input_clear_color(&mut input, [0.0, 0.0, 0.0, 1.0]);
    let mesh_a = api.add_input_mesh(
        &mut input,
        1,
        vec![Vec3::ZERO; 3],
        vec![Vec3::UNIT_Y; 3],
        vec![Vec2::ZERO; 3],
        (0..3).collect(),
    );
    let mesh_b = api.add_input_mesh(
        &mut input,
        2,
        vec![Vec3::ZERO; 3],
        vec![Vec3::UNIT_Y; 3],
        vec![Vec2::ZERO; 3],
        (0..3).collect(),
    );
    let mat_a = api.add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
    let mat_b = api.add_input_basic_lit_material(&mut input, 2, Vec4::new(0.1, 0.2, 0.3, 1.0));
    if swap {
        api.add_input_object(&mut input, Mat4::IDENTITY, mesh_b, mat_b, true);
        api.add_input_object(&mut input, Mat4::IDENTITY, mesh_a, mat_a, true);
    } else {
        api.add_input_object(&mut input, Mat4::IDENTITY, mesh_a, mat_a, true);
        api.add_input_object(&mut input, Mat4::IDENTITY, mesh_b, mat_b, true);
    }
    let list = api.build_command_list(&input);
    let receipt = api.capture_receipt(FrameIndex::new(0), Tick::new(0), &list);
    api.receipt_bytes(&receipt).to_vec()
}

// 1. Replay determinism.
#[test]
fn rotating_cube_frame_120_replay_produces_byte_identical_frame_capture() {
    let api = RenderApi::new();
    let world = cube_world(cube_rotation_angle(120));
    let (bytes_a, hash_a) = cube_capture(&api, 120, 120, world);
    let (bytes_b, hash_b) = cube_capture(&api, 120, 120, world);
    assert_eq!(bytes_a, bytes_b, "same frame must serialize byte-identically");
    assert_eq!(hash_a, hash_b, "same frame must hash identically");
}

// 2. Meaningful difference: same frame index + tick, different cube world
//    transform rotation (a render-visible value).
#[test]
fn rotating_cube_frame_120_meaningful_render_change_changes_frame_capture() {
    let api = RenderApi::new();
    let baseline = cube_world(cube_rotation_angle(120));
    let rotated = cube_world(cube_rotation_angle(121)); // different cube rotation only
    let (bytes_a, hash_a) = cube_capture(&api, 120, 120, baseline);
    let (bytes_b, hash_b) = cube_capture(&api, 120, 120, rotated);
    assert_ne!(bytes_a, bytes_b, "a different cube world transform must change the capture");
    assert_ne!(hash_a, hash_b, "a different cube world transform must change the hash");
}

// 3. Command ordering: the same commands in a different order must capture
//    differently.
#[test]
fn render_command_order_changes_frame_capture() {
    let api = RenderApi::new();
    let in_order = two_object_capture(&api, false);
    let swapped = two_object_capture(&api, true);
    assert_ne!(in_order, swapped, "command order must affect the capture");
}

// 4. Negative proof / guardrail.
//
// PROOF-TEST ONLY — keep this `#[ignore]`d. It deliberately asserts equality
// *after* a meaningful render-visible change (a different cube rotation), so it
// MUST fail when run explicitly. Its only purpose is to demonstrate that the
// capture suite would catch a broken capture that ignored render changes. It
// is not a real test of behaviour and must never run in the normal suite.
#[test]
#[ignore]
fn intentional_failure_meaningful_render_change_should_not_match() {
    let api = RenderApi::new();
    let (bytes_a, _) = cube_capture(&api, 120, 120, cube_world(cube_rotation_angle(120)));
    let (bytes_b, _) = cube_capture(&api, 120, 120, cube_world(cube_rotation_angle(121)));
    // Intentionally WRONG: these differ by design, so this assertion fails.
    assert_eq!(
        bytes_a, bytes_b,
        "guardrail: a meaningful render change is expected to change the capture, \
         so this equality assertion is designed to fail"
    );
}
