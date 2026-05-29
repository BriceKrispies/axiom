//! Golden-style integration test for the rotating-cube vertical slice.
//!
//! Asserts meaningful command content at tick 60: the render command
//! shape, the GPU submission shape, the pipeline marker, the cube
//! draw index count, and byte-equality of two replays of the same
//! tick sequence.

use axiom_demo_rotating_cube::{CubeFrame, RotatingCubeDemo};
use axiom_render::RenderApi;
use axiom_webgpu::WebGpuApi;

fn drive_to_tick(target_tick: u64) -> CubeFrame {
    let mut demo = RotatingCubeDemo::new();
    for tick in 0..target_tick {
        demo.run_tick(tick);
    }
    demo.run_tick(target_tick)
}

#[test]
fn deterministic_rotating_cube_tick_60_produces_stable_render_commands() {
    let frame = drive_to_tick(60);

    // 1. Tick identity.
    assert_eq!(frame.tick, 60);

    // 2. Engine / host bookkeeping.
    assert_eq!(frame.engine_frame_index, 60);
    assert_eq!(frame.host_frame_sequence, 61);
    assert_eq!(frame.runtime_step_count, 1);

    // 3. Render command shape: six commands in the expected order.
    let kinds = &frame.render_command_kinds;
    assert_eq!(kinds.len(), 6);
    assert_eq!(kinds[0], RenderApi::KIND_CLEAR_FRAME);
    assert_eq!(kinds[1], RenderApi::KIND_SET_CAMERA);
    assert_eq!(kinds[2], RenderApi::KIND_SET_PIPELINE);
    assert_eq!(kinds[3], RenderApi::KIND_SET_MESH);
    assert_eq!(kinds[4], RenderApi::KIND_SET_MATERIAL);
    assert_eq!(kinds[5], RenderApi::KIND_DRAW_INDEXED);

    // 4. The basic-lit pipeline marker.
    assert_eq!(frame.render_pipeline_id, RenderApi::PIPELINE_BASIC_LIT);

    // 5. The cube draw uses the built-in cube's 36 indices.
    assert_eq!(frame.render_draw_index_count, 36);

    // 6. The clear colour is the demo's documented colour.
    let c = frame.render_clear_color;
    assert!((c[0] - 0.05).abs() < 1.0e-6);
    assert!((c[1] - 0.06).abs() < 1.0e-6);
    assert!((c[2] - 0.08).abs() < 1.0e-6);
    assert_eq!(c[3], 1.0);

    // 7. The GPU submission shape: every kind plus a Present at the end.
    let gpu_kinds = &frame.gpu_command_kinds;
    assert!(
        gpu_kinds.len() >= 6,
        "expected at least 6 GPU commands, got {gpu_kinds:?}"
    );
    assert_eq!(gpu_kinds[0], WebGpuApi::KIND_CLEAR_FRAME);
    assert_eq!(*gpu_kinds.last().unwrap(), WebGpuApi::KIND_PRESENT);
    assert_eq!(frame.gpu_clear_count, 1);
    assert_eq!(frame.gpu_draw_count, 1);
    assert_eq!(frame.gpu_present_count, 1);
    assert_eq!(frame.gpu_target_width, 800);
    assert_eq!(frame.gpu_target_height, 600);
}

#[test]
fn replay_of_sixty_ticks_is_byte_for_byte_equal() {
    let a = drive_to_tick(60);
    let b = drive_to_tick(60);
    assert_eq!(a, b);
}

#[test]
fn tick_zero_and_tick_60_have_different_cube_world_transforms() {
    let tick_0 = drive_to_tick(0);
    let tick_60 = drive_to_tick(60);
    assert_ne!(tick_0.render_draw_world, tick_60.render_draw_world);
}

#[test]
fn first_three_ticks_have_strictly_increasing_engine_frame_indices() {
    let mut demo = RotatingCubeDemo::new();
    let f0 = demo.run_tick(0);
    let f1 = demo.run_tick(1);
    let f2 = demo.run_tick(2);
    assert!(f0.engine_frame_index < f1.engine_frame_index);
    assert!(f1.engine_frame_index < f2.engine_frame_index);
}
