//! Scaling load test for the deterministic per-frame CPU pipeline.
//!
//! "Load testing" a deterministic engine isn't about saturating a server — it's
//! about asking *how the per-frame cost grows as the scene grows*, and where
//! that cost lives. This harness builds a scene of N animated renderables and
//! drives the real native pipeline at increasing N:
//!
//!   scene.advance(tick)          // spin animation + world-transform propagation + snapshot
//!   pipeline.submit(...)         // snapshot -> render input -> command list -> recording GPU submission
//!
//! It times those two phases *separately* so the numbers localize the cost. The
//! GPU backend is `WebGpuApi::new_recording()` — the non-presenting, fully
//! deterministic backend the engine's own tests use — so this runs natively
//! with no browser and produces stable, comparable numbers. The actual on-GPU
//! paint (the wasm `LiveGpuBinding`) is a *separate*, non-deterministic concern
//! that must be measured in a browser via the Playwright controller, not here.
//!
//! Run with:
//!   cargo run --release --manifest-path bench/pipeline-scaling/Cargo.toml

use std::hint::black_box;
use std::time::{Duration, Instant};

use axiom_frame::{EngineFrame, FrameApi, FrameContext};
use axiom_host::{
    HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal, HostLifecycleState,
    HostStepPlan, HostViewport,
};
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{MathApi, Transform, Vec2, Vec3};
use axiom_render_pipeline::RenderPipelineApi;
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;

/// Every renderable shares one mesh + one material, so asset count stays O(1)
/// and the only thing scaling is the renderable/node count.
const MESH_ID: u64 = 1;
const MATERIAL_ID: u64 = 2;

/// Scene sizes to sweep: renderables (each its own animated node).
const SWEEP: [usize; 6] = [100, 500, 1_000, 5_000, 10_000, 50_000];
/// Frames timed per phase, per trial.
const FRAMES: usize = 30;
/// Measurement repeats; we report the fastest (least-noisy) run.
const TRIALS: usize = 5;

/// Build a scene with a camera, a directional light, and `n` animated children
/// under a shared root — each a renderable with a per-node spin.
fn build_scene(n: usize) -> SceneApi {
    let math = MathApi::new();
    let mut scene = SceneApi::new();

    let camera =
        scene.create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 0.0, 80.0)));
    scene
        .add_perspective_camera(
            &math,
            camera,
            Radians::new(std::f32::consts::FRAC_PI_3).unwrap(),
            Ratio::new(16.0 / 9.0).unwrap(),
            Meters::new(0.1).unwrap(),
            Meters::new(1000.0).unwrap(),
        )
        .unwrap();

    let light = scene.create_node();
    scene
        .add_directional_light(&math, light, Vec3::ONE, Ratio::new(1.0).unwrap())
        .unwrap();

    let root = scene.create_node();
    let mesh = scene.mesh_ref(MESH_ID);
    let material = scene.material_ref(MATERIAL_ID);
    for i in 0..n {
        let f = i as f32;
        let child = scene.create_node_with_transform(Transform::from_translation(Vec3::new(
            (f * 0.37).sin() * 30.0,
            (f * 0.53).cos() * 30.0,
            (f * 0.11).sin() * 30.0,
        )));
        scene.set_parent(child, root).unwrap();
        scene.add_renderable(child, mesh, material).unwrap();
        scene
            .add_spin(child, Vec3::UNIT_Y, 120 + (i as u32 % 240))
            .unwrap();
    }
    scene.update_world_transforms();
    scene
}

/// One active engine frame, reused across ticks (only `tick` varies the spin
/// angle). Mirrors the host→frame wiring the engine's own scene tests use.
fn active_frame() -> EngineFrame {
    let elapsed = 1_000;
    let vp = HostViewport::new(1920, 1080, Ratio::new(16.0 / 9.0).unwrap()).unwrap();
    let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
    let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
    let input = HostFrameInput::new(1, elapsed, vp);
    let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
    let report = HostFrameReport::new(
        input.sequence(),
        plan,
        plan.steps(),
        Vec::new(),
        vp,
        visible,
    );
    FrameApi::new()
        .engine_frame_from_host_report(&report, elapsed, Vec::new())
        .unwrap()
}

fn min_of(trials: impl Iterator<Item = Duration>) -> Duration {
    trials.min().unwrap_or(Duration::MAX)
}

/// Measure (advance_per_frame, submit_per_frame) for a scene of `n` renderables.
fn measure(n: usize) -> (Duration, Duration) {
    let mut pipeline = RenderPipelineApi::new();
    let frame = active_frame();
    let mut webgpu = WebGpuApi::new_recording();

    // Per-frame render assets: one unit cube + one material, shared by every
    // renderable, constant across the sweep so it doesn't contaminate the
    // scaling signal. The frame value's type is un-nameable outside this
    // module, so it lives only as this inferred local.
    let mut render_frame = pipeline.new_frame(
        1920,
        1080,
        [0.05, 0.06, 0.08, 1.0],
        Vec3::new(0.3, -1.0, 0.4),
    );
    pipeline.frame_add_mesh(
        &mut render_frame,
        MESH_ID,
        vec![Vec3::new(0.5, 0.5, 0.5); 24],
        vec![Vec3::new(0.0, 1.0, 0.0); 24],
        vec![Vec2::new(0.0, 0.0); 24],
        (0..36).collect(),
    );
    pipeline.frame_add_material(&mut render_frame, MATERIAL_ID, [0.8, 0.4, 0.2, 1.0]);

    let mut advance_times = Vec::with_capacity(TRIALS);
    let mut submit_times = Vec::with_capacity(TRIALS);

    for _ in 0..TRIALS {
        // Fresh scene per trial so every trial advances from the same tick 0
        // state — keeps the animation work identical across trials.
        let mut scene = build_scene(n);

        let t0 = Instant::now();
        for tick in 0..FRAMES as u64 {
            let snap = scene.advance(tick, &FrameContext::new(&frame));
            black_box(&snap);
        }
        advance_times.push(t0.elapsed());

        let t1 = Instant::now();
        for _ in 0..FRAMES {
            let report = pipeline.submit(&render_frame, &mut scene, &mut webgpu);
            black_box(&report);
        }
        submit_times.push(t1.elapsed());
    }

    let per_frame = |d: Duration| d / FRAMES as u32;
    (
        per_frame(min_of(advance_times.into_iter())),
        per_frame(min_of(submit_times.into_iter())),
    )
}

fn main() {
    println!("pipeline scaling load test");
    println!("  pipeline : scene.advance (spin + propagation + snapshot)  +  RenderPipelineApi::submit (recording backend)");
    println!("  frames   : {FRAMES} timed per phase    trials: {TRIALS} (reporting fastest)\n");

    println!(
        "{:>10} | {:>14} | {:>14} | {:>14} | {:>12} | {:>12}",
        "renderables",
        "advance us/fr",
        "submit us/fr",
        "total us/fr",
        "submit ns/obj",
        "fps @ total"
    );
    println!("{}", "-".repeat(92));

    for n in SWEEP {
        let (advance, submit) = measure(n);
        let total = advance + submit;
        let us = |d: Duration| d.as_secs_f64() * 1e6;
        let submit_ns_per_obj = submit.as_secs_f64() * 1e9 / n as f64;
        let fps = 1.0 / total.as_secs_f64();
        println!(
            "{:>10} | {:>14.1} | {:>14.1} | {:>14.1} | {:>12.1} | {:>12.0}",
            n,
            us(advance),
            us(submit),
            us(total),
            submit_ns_per_obj,
            fps
        );
    }

    println!(
        "\nRead `submit ns/obj`: if it stays flat as renderables grow, submit is linear.\n\
         If it climbs, submit is super-linear in scene size — a structural seam to fix at the source."
    );
}
