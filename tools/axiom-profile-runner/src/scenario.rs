//! The deterministic native CPU stress scenario, with per-phase **and
//! per-subphase** wall-clock timing, optional warmup, and focus modes.
//!
//! This is the only part of the profiler that touches the Axiom engine. It
//! composes the highest public facades available today — `axiom-runtime`,
//! `axiom-scene`, `axiom-math`, and `axiom-render` — into a fixed,
//! integer-seeded workload. Wall-clock timing lives **only here and in
//! `main`**, never in an engine crate.
//!
//! ## Subphase honesty — why transform_update is a reconstruction
//! `render_command_build` is decomposed by timing the harness's own calls into
//! the public `axiom-render` API, so its subphases are genuine engine work and
//! sum exactly to the parent.
//!
//! `transform_update`'s real engine implementation
//! (`axiom_scene::SceneApi::update_world_transforms`) is a **single opaque
//! call**: its internal id-collection, parent lookup, combine, and world-write
//! steps cannot be timed from outside without modifying engine code, which this
//! pass forbids. So the profiler measures a **faithful reconstruction** of the
//! same algorithm over the same scene, built from public APIs: `parent_of` /
//! `local_transform` (the engine's own `BTreeMap`-backed reads), the exact same
//! `Transform::combine`, and a per-frame scratch `BTreeMap` mirroring the
//! engine's. The reconstruction is valid here because the stress scene is
//! depth-2 (one rotating root, N leaf children), so the per-node work can be
//! staged into separately-timed passes without violating the parent→child
//! dependency. Its phase `kind` is `real_engine_model`, and the report says so.

use std::collections::BTreeMap;
use std::time::Instant;

use axiom_kernel::{HandleId, MetricValue, Ratio, TelemetryMetric};
use axiom_math::{Aabb, Frustum, Mat4, Quat, Transform, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;
use axiom_runtime::{Runtime, RuntimeConfig, RuntimeContext, RuntimeResult, RuntimeSystem};
use axiom_scene::SceneApi;

use crate::report::{kind, ChurnCounters, FrameTimings, Phase};

const FIXED_STEP_NANOS: u64 = 1_000_000;
const VIEWPORT_WIDTH: u32 = 1280;
const VIEWPORT_HEIGHT: u32 = 720;
const LATTICE_SPACING: f32 = 2.0;
const OBJECT_HALF_EXTENT: f32 = 0.5;
const CUBE_MESH_ID: u64 = 1;
const CUBE_MATERIAL_ID: u64 = 1;
const MESH_IDX: u32 = 0;
const MATERIAL_IDX: u32 = 0;

const TF_PREPARE: &str = "transform_prepare_inputs";
const TF_PARENT: &str = "transform_parent_lookup";
const TF_COMBINE: &str = "transform_combine_or_matrix_math";
const TF_WRITE: &str = "transform_write_world_state";
const TF_SNAPSHOT: &str = "transform_snapshot_or_output_collection";

const RC_CREATE: &str = "render_input_create_or_reset";
const RC_CLONE: &str = "render_mesh_data_clone_or_reference";
const RC_PUSH: &str = "render_object_push";
const RC_FINALIZE: &str = "render_command_finalize";

/// The phase this run measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPhase {
    Full,
    TransformUpdate,
    RenderCommandBuild,
}

impl FocusPhase {
    /// Parse a `--focus-phase` value, rejecting anything else with a clear
    /// error listing the allowed values.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "full" => Ok(FocusPhase::Full),
            "transform_update" => Ok(FocusPhase::TransformUpdate),
            "render_command_build" => Ok(FocusPhase::RenderCommandBuild),
            other => Err(format!(
                "unknown --focus-phase `{other}` (allowed: full, transform_update, render_command_build)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FocusPhase::Full => "full",
            FocusPhase::TransformUpdate => "transform_update",
            FocusPhase::RenderCommandBuild => "render_command_build",
        }
    }
}

/// Configuration for one scenario run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioConfig {
    pub object_count: u64,
    pub measured_frames: u64,
    pub warmup_frames: u64,
    pub focus: FocusPhase,
}

/// The aggregated, engine-free result of a run. `main` appends the
/// `report_write` phase and serializes it.
#[derive(Debug)]
pub struct ScenarioOutput {
    pub object_count: u64,
    pub phases: Vec<Phase>,
    pub frames: FrameTimings,
    pub churn: ChurnCounters,
    pub placeholder_phases: Vec<String>,
    pub notes: Vec<String>,
}

/// The runtime system run every step in `full` mode — records one telemetry
/// gauge derived from the deterministic tick, exercising the scheduler and
/// telemetry path. No per-object gameplay simulation yet.
#[derive(Debug)]
struct StressStepSystem;

impl RuntimeSystem for StressStepSystem {
    fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
        let tick = ctx.step().tick();
        ctx.metric(TelemetryMetric::gauge(
            "stress.tick",
            MetricValue::float(tick.raw() as f32),
            Some(tick),
        ));
        Ok(())
    }
}

/// Cube vertex data shared by every object. Cloned into each frame's render
/// input — the clone is exactly what `render_mesh_data_clone_or_reference`
/// measures (this pass measures the clones; it does not remove them).
struct CubeMesh {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

/// Run the scenario for the configured focus phase, with warmup excluded from
/// the measured totals.
pub fn run(config: ScenarioConfig) -> ScenarioOutput {
    let object_count = config.object_count as usize;
    let measured = config.measured_frames;
    let warmup = config.warmup_frames;

    let mut setup = Phase::new("setup", kind::HARNESS);
    let setup_start = Instant::now();

    let render_api = RenderApi::new();
    let mut runtime =
        Runtime::new(RuntimeConfig::new(FIXED_STEP_NANOS).with_diagnostics_enabled(false))
            .expect("fixed step is a valid runtime config");
    runtime
        .initialize()
        .expect("runtime initialize cannot fail");
    runtime.start().expect("runtime start cannot fail");
    runtime
        .scheduler_mut()
        .register(
            HandleId::from_raw(1),
            "stress-step",
            1,
            Box::new(StressStepSystem),
        )
        .expect("registering the single stress system cannot fail");

    let mut scene = SceneApi::new();
    let root = scene.create_node();
    let side = lattice_side(object_count);
    let child_ids: Vec<_> = (0..object_count)
        .map(|i| {
            let local = Transform::from_translation(lattice_translation(i, side));
            let id = scene.create_node_with_transform(local);
            scene
                .set_parent(id, root)
                .expect("root and child were just created");
            id
        })
        .collect();

    let view_distance = (side as f32) * LATTICE_SPACING + 10.0;
    let view = Mat4::look_at(Vec3::new(0.0, 0.0, view_distance), Vec3::ZERO, Vec3::UNIT_Y)
        .expect("camera eye and target differ");
    let aspect = VIEWPORT_WIDTH as f32 / VIEWPORT_HEIGHT as f32;
    let projection = Mat4::perspective(std::f32::consts::FRAC_PI_3, aspect, 0.1, 100_000.0)
        .expect("perspective parameters are valid");
    let frustum = Frustum::from_view_projection(projection.multiply(view))
        .expect("view-projection is invertible");

    let cube = unit_cube();
    let extents = Vec3::new(OBJECT_HALF_EXTENT, OBJECT_HALF_EXTENT, OBJECT_HALF_EXTENT);
    let light_dir = Vec3::new(-0.5, -1.0, -0.5);
    let light_color = Vec3::new(1.0, 1.0, 1.0);
    let base_color = Vec4::new(0.6, 0.6, 0.7, 1.0);
    setup.record(setup_start.elapsed().as_nanos());

    // The transform reconstruction (5 timed passes), as a closure so it can own
    // the only `&mut scene` borrow. Returns each child's world matrix.
    let mut tf_tick: u64 = 0;
    let mut tf_iter = |phase: &mut Phase, churn: &mut ChurnCounters| -> Vec<Mat4> {
        // transform_prepare_inputs: rotate the root, allocate the per-frame
        // scratch world store (mirroring the engine's fresh BTreeMap), and seed
        // the root's world (it has no parent).
        let t = Instant::now();
        tf_tick += 1;
        let angle = ((tf_tick % 360) as f32) * std::f32::consts::PI / 180.0;
        let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, angle).expect("unit axis, finite angle");
        scene
            .set_local_transform(root, Transform::from_rotation(rotation))
            .expect("root node exists");
        let mut scratch: BTreeMap<u64, Transform> = BTreeMap::new();
        churn.transform_scratch_maps_allocated += 1;
        let root_local = scene.local_transform(root).expect("root node exists");
        scratch.insert(root.raw(), root_local);
        churn.world_transforms_written += 1;
        phase.record_subphase(TF_PREPARE, t.elapsed().as_nanos());

        // transform_parent_lookup: per child, resolve its parent and read the
        // parent's world plus the child's local (the engine's BTreeMap reads).
        let t = Instant::now();
        let inputs: Vec<(Transform, Transform)> = child_ids
            .iter()
            .map(|&c| {
                churn.transform_parent_lookups += 1;
                let parent_world = scene
                    .parent_of(c)
                    .and_then(|p| scratch.get(&p.raw()).copied())
                    .unwrap_or(Transform::IDENTITY);
                let local = scene.local_transform(c).expect("child node exists");
                (parent_world, local)
            })
            .collect();
        phase.record_subphase(TF_PARENT, t.elapsed().as_nanos());

        // transform_combine_or_matrix_math: compose each child's world.
        let t = Instant::now();
        let worlds: Vec<Transform> = inputs
            .iter()
            .map(|&(parent_world, local)| Transform::combine(parent_world, local))
            .collect();
        phase.record_subphase(TF_COMBINE, t.elapsed().as_nanos());

        // transform_write_world_state: write each world into the scratch store
        // (mirrors the engine's `worlds.insert` into its component column).
        let t = Instant::now();
        child_ids.iter().zip(worlds.iter()).for_each(|(&c, &w)| {
            scratch.insert(c.raw(), w);
            churn.world_transforms_written += 1;
        });
        phase.record_subphase(TF_WRITE, t.elapsed().as_nanos());

        // transform_snapshot_or_output_collection: read the world store back
        // into the matrix array the downstream phases consume.
        let t = Instant::now();
        let output: Vec<Mat4> = child_ids
            .iter()
            .map(|&c| {
                scratch
                    .get(&c.raw())
                    .copied()
                    .unwrap_or(Transform::IDENTITY)
                    .to_matrix()
            })
            .collect();
        phase.record_subphase(TF_SNAPSHOT, t.elapsed().as_nanos());
        output
    };

    // A single representative transform+bounds+cull pass: gives an accurate
    // visible-object count for the notes, and the stable visible set the
    // render focus mode reuses each iteration.
    let stable_worlds = tf_iter(
        &mut Phase::new("warmup", kind::HARNESS),
        &mut ChurnCounters::default(),
    );
    let stable_visible: Vec<Mat4> = stable_worlds
        .iter()
        .filter(|m| {
            let center = m.transform_point(Vec3::ZERO);
            let aabb = Aabb::from_center_extents(center, extents).expect("valid extents");
            frustum.intersects_aabb(&aabb)
        })
        .copied()
        .collect();
    let visible_count = stable_visible.len();

    let mut frames = FrameTimings::new();

    let (phases, churn) = match config.focus {
        FocusPhase::Full => {
            let mut sink = FullSink::new();
            let mut warm = FullSink::new();
            let mut full_iter = |s: &mut FullSink| {
                let t = Instant::now();
                runtime
                    .step()
                    .expect("a started runtime steps deterministically");
                s.runtime_step.record(t.elapsed().as_nanos());

                let worlds = tf_iter(&mut s.transform, &mut s.churn);

                let t = Instant::now();
                let bounds: Vec<Aabb> = worlds
                    .iter()
                    .map(|m| {
                        let center = m.transform_point(Vec3::ZERO);
                        Aabb::from_center_extents(center, extents).expect("valid extents")
                    })
                    .collect();
                s.bounds.record(t.elapsed().as_nanos());

                let t = Instant::now();
                let visible: Vec<Mat4> = worlds
                    .iter()
                    .zip(bounds.iter())
                    .filter(|(_, aabb)| frustum.intersects_aabb(aabb))
                    .map(|(m, _)| *m)
                    .collect();
                s.visibility.record(t.elapsed().as_nanos());

                render_command_build_workload(
                    &render_api,
                    &cube,
                    view,
                    projection,
                    light_dir,
                    light_color,
                    base_color,
                    &visible,
                    &mut s.render,
                    &mut s.churn,
                );
            };

            (0..warmup).for_each(|_| full_iter(&mut warm));
            (0..measured).for_each(|_| {
                let t = Instant::now();
                full_iter(&mut sink);
                frames.record(t.elapsed().as_nanos());
            });

            sink.transform.finalize_from_subphases(measured);
            sink.render.finalize_from_subphases(measured);
            (
                vec![
                    setup,
                    sink.runtime_step,
                    sink.transform,
                    sink.bounds,
                    sink.visibility,
                    sink.render,
                ],
                sink.churn,
            )
        }
        FocusPhase::TransformUpdate => {
            let mut phase = Phase::new("transform_update", kind::REAL_ENGINE_MODEL);
            let mut warm = Phase::new("transform_update", kind::REAL_ENGINE_MODEL);
            let mut churn = ChurnCounters::default();
            let mut warm_churn = ChurnCounters::default();

            (0..warmup).for_each(|_| {
                let _ = tf_iter(&mut warm, &mut warm_churn);
            });
            (0..measured).for_each(|_| {
                let t = Instant::now();
                let _ = tf_iter(&mut phase, &mut churn);
                frames.record(t.elapsed().as_nanos());
            });

            phase.finalize_from_subphases(measured);
            (vec![phase], churn)
        }
        FocusPhase::RenderCommandBuild => {
            let mut phase = Phase::new("render_command_build", kind::REAL_ENGINE);
            let mut warm = Phase::new("render_command_build", kind::REAL_ENGINE);
            let mut churn = ChurnCounters::default();
            let mut warm_churn = ChurnCounters::default();

            (0..warmup).for_each(|_| {
                render_command_build_workload(
                    &render_api,
                    &cube,
                    view,
                    projection,
                    light_dir,
                    light_color,
                    base_color,
                    &stable_visible,
                    &mut warm,
                    &mut warm_churn,
                );
            });
            (0..measured).for_each(|_| {
                let t = Instant::now();
                render_command_build_workload(
                    &render_api,
                    &cube,
                    view,
                    projection,
                    light_dir,
                    light_color,
                    base_color,
                    &stable_visible,
                    &mut phase,
                    &mut churn,
                );
                frames.record(t.elapsed().as_nanos());
            });

            phase.finalize_from_subphases(measured);
            (vec![phase], churn)
        }
    };

    let notes = build_notes(&config, object_count, visible_count);

    ScenarioOutput {
        object_count: config.object_count,
        phases,
        frames,
        churn,
        // Always disclosed, regardless of focus mode, so placeholders are never
        // hidden even when a focus run does not execute them.
        placeholder_phases: vec![
            "bounds_update_placeholder".to_string(),
            "visibility_or_culling_placeholder".to_string(),
        ],
        notes,
    }
}

/// Holds the `full`-mode phase accumulators and churn for one run.
struct FullSink {
    runtime_step: Phase,
    transform: Phase,
    bounds: Phase,
    visibility: Phase,
    render: Phase,
    churn: ChurnCounters,
}

impl FullSink {
    fn new() -> Self {
        FullSink {
            runtime_step: Phase::new("runtime_step", kind::REAL_ENGINE),
            transform: Phase::new("transform_update", kind::REAL_ENGINE_MODEL),
            bounds: Phase::new("bounds_update_placeholder", kind::PLACEHOLDER),
            visibility: Phase::new("visibility_or_culling_placeholder", kind::PLACEHOLDER),
            render: Phase::new("render_command_build", kind::REAL_ENGINE),
            churn: ChurnCounters::default(),
        }
    }
}

/// The render_command_build workload: build a real `RenderInput` from the
/// visible objects and compile a real `RenderCommandList`, timing four
/// subphases. Every step is a genuine `axiom-render` public call, so the
/// subphases sum to the parent. No GPU work is performed.
#[allow(clippy::too_many_arguments)]
fn render_command_build_workload(
    render_api: &RenderApi,
    cube: &CubeMesh,
    view: Mat4,
    projection: Mat4,
    light_dir: Vec3,
    light_color: Vec3,
    base_color: Vec4,
    visible_worlds: &[Mat4],
    phase: &mut Phase,
    churn: &mut ChurnCounters,
) {
    // render_input_create_or_reset
    let t = Instant::now();
    let mut input = render_api.new_input(VIEWPORT_WIDTH, VIEWPORT_HEIGHT);
    render_api.set_input_clear_color(&mut input, [0.0, 0.0, 0.0, 1.0]);
    render_api.set_input_camera(&mut input, view, projection);
    render_api.add_input_directional_light(
        &mut input,
        light_dir,
        light_color,
        Ratio::new(1.0).expect("unit intensity is finite"),
    );
    churn.render_inputs_created += 1;
    phase.record_subphase(RC_CREATE, t.elapsed().as_nanos());

    // render_mesh_data_clone_or_reference — the per-frame cube data clones this
    // pass measures (and deliberately does not remove).
    let t = Instant::now();
    let positions = cube.positions.clone();
    let normals = cube.normals.clone();
    let uvs = cube.uvs.clone();
    let indices = cube.indices.clone();
    churn.mesh_vec_clones += 4;
    render_api.add_input_mesh(&mut input, CUBE_MESH_ID, positions, normals, uvs, indices);
    render_api.add_input_basic_lit_material(&mut input, CUBE_MATERIAL_ID, base_color);
    phase.record_subphase(RC_CLONE, t.elapsed().as_nanos());

    // render_object_push
    let t = Instant::now();
    visible_worlds.iter().enumerate().for_each(|(i, w)| {
        render_api.add_input_object(&mut input, i as u64, *w, MESH_IDX, MATERIAL_IDX, true);
    });
    churn.render_objects_pushed += visible_worlds.len() as u64;
    phase.record_subphase(RC_PUSH, t.elapsed().as_nanos());

    // render_command_finalize
    let t = Instant::now();
    let commands = render_api.build_command_list(&input);
    assert!(render_api.command_count(&commands) >= 3);
    churn.render_command_lists_built += 1;
    phase.record_subphase(RC_FINALIZE, t.elapsed().as_nanos());
}

fn lattice_side(count: usize) -> usize {
    match count {
        0 => 1,
        n => (n as f64).cbrt().ceil() as usize,
    }
    .max(1)
}

fn lattice_translation(i: usize, side: usize) -> Vec3 {
    let x = i % side;
    let y = (i / side) % side;
    let z = i / (side * side);
    let half = (side as f32) / 2.0;
    Vec3::new(
        ((x as f32) - half) * LATTICE_SPACING,
        ((y as f32) - half) * LATTICE_SPACING,
        ((z as f32) - half) * LATTICE_SPACING,
    )
}

fn unit_cube() -> CubeMesh {
    let h = OBJECT_HALF_EXTENT;
    let positions = vec![
        Vec3::new(-h, -h, -h),
        Vec3::new(h, -h, -h),
        Vec3::new(h, h, -h),
        Vec3::new(-h, h, -h),
        Vec3::new(-h, -h, h),
        Vec3::new(h, -h, h),
        Vec3::new(h, h, h),
        Vec3::new(-h, h, h),
    ];
    let normals = positions.clone();
    let uvs = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    #[rustfmt::skip]
    let indices = vec![
        0, 1, 2, 0, 2, 3,
        5, 4, 7, 5, 7, 6,
        4, 0, 3, 4, 3, 7,
        1, 5, 6, 1, 6, 2,
        4, 5, 1, 4, 1, 0,
        3, 2, 6, 3, 6, 7,
    ];
    CubeMesh {
        positions,
        normals,
        uvs,
        indices,
    }
}

fn build_notes(config: &ScenarioConfig, object_count: usize, visible_count: usize) -> Vec<String> {
    let mut notes = vec![format!(
        "Native CPU wall-clock profiling only. Focus phase: {}. {} measured frames, \
         {} warmup frames (warmup is excluded from all measured totals).",
        config.focus.as_str(),
        config.measured_frames,
        config.warmup_frames
    )];
    match config.focus {
        FocusPhase::Full => notes.push(
            "full mode runs the complete per-frame loop: runtime_step, transform_update, \
             bounds, culling, and render_command_build."
                .to_string(),
        ),
        FocusPhase::TransformUpdate => notes.push(
            "FOCUSED transform_update run: only the transform_update workload was measured each \
             iteration; runtime_step, bounds, culling, and render_command_build were NOT run."
                .to_string(),
        ),
        FocusPhase::RenderCommandBuild => notes.push(format!(
            "FOCUSED render_command_build run: a stable set of {visible_count} visible objects was \
             computed once during setup, then only render_command_build was measured each \
             iteration; transform_update, bounds, and culling were NOT run."
        )),
    }
    notes.push(format!(
        "Scenario: one rotating root with {object_count} child objects on a lattice derived from \
         integer indices (no randomness). {visible_count} objects fell inside the camera frustum."
    ));
    notes.push(
        "transform_update kind=real_engine_model: axiom-scene's update_world_transforms is a \
         single opaque engine call, so the profiler measures a faithful reconstruction of the \
         same propagation using public APIs (parent_of/local_transform reads, the same \
         Transform::combine, and a per-frame scratch BTreeMap mirroring the engine's). The \
         absolute number differs from a direct engine-call measurement; the value is the \
         subphase split."
            .to_string(),
    );
    notes.push(
        "render_command_build kind=real_engine: genuine axiom-render calls; subphases sum exactly \
         to the parent. render_mesh_data_clone_or_reference measures the per-frame cube data \
         clones (this pass measures them; it does not remove them). No GPU work, no rendering."
            .to_string(),
    );
    notes.push(
        "PLACEHOLDERS: bounds_update_placeholder and visibility_or_culling_placeholder use real \
         axiom-math (Aabb / Frustum) but are not yet engine-owned systems. They run only in full \
         mode; they are still disclosed in placeholder_phases in every mode."
            .to_string(),
    );
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(focus: FocusPhase, measured: u64, warmup: u64) -> ScenarioConfig {
        ScenarioConfig {
            object_count: 64,
            measured_frames: measured,
            warmup_frames: warmup,
            focus,
        }
    }

    #[test]
    fn focus_phase_parses_known_values_and_rejects_unknown() {
        assert_eq!(FocusPhase::parse("full"), Ok(FocusPhase::Full));
        assert_eq!(
            FocusPhase::parse("transform_update"),
            Ok(FocusPhase::TransformUpdate)
        );
        assert_eq!(
            FocusPhase::parse("render_command_build"),
            Ok(FocusPhase::RenderCommandBuild)
        );
        let err = FocusPhase::parse("gpu").unwrap_err();
        assert!(err.contains("gpu"));
        assert!(
            err.contains("transform_update"),
            "error lists the allowed values"
        );
    }

    #[test]
    fn full_mode_reports_every_phase_with_transform_and_render_subphases() {
        let out = run(config(FocusPhase::Full, 3, 1));
        let names: Vec<&str> = out.phases.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "setup",
                "runtime_step",
                "transform_update",
                "bounds_update_placeholder",
                "visibility_or_culling_placeholder",
                "render_command_build",
            ]
        );
        let transform = out
            .phases
            .iter()
            .find(|p| p.name == "transform_update")
            .unwrap();
        assert_eq!(transform.subphases.len(), 5);
        assert_eq!(transform.kind, kind::REAL_ENGINE_MODEL);
        let render = out
            .phases
            .iter()
            .find(|p| p.name == "render_command_build")
            .unwrap();
        assert_eq!(render.subphases.len(), 4);
        assert_eq!(render.kind, kind::REAL_ENGINE);
        // Composite parent totals equal the exact sum of their subphases.
        let tf_sum: u128 = transform.subphases.iter().map(|s| s.total_ns).sum();
        assert_eq!(transform.total_ns, tf_sum);
        // Per-iteration sample counts.
        assert_eq!(transform.sample_count, 3);
        assert_eq!(render.sample_count, 3);
    }

    #[test]
    fn focused_transform_mode_excludes_unrelated_frame_phases() {
        let out = run(config(FocusPhase::TransformUpdate, 3, 1));
        let names: Vec<&str> = out.phases.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["transform_update"]);
        // None of the other frame phases were measured.
        for excluded in [
            "runtime_step",
            "bounds_update_placeholder",
            "visibility_or_culling_placeholder",
            "render_command_build",
            "setup",
        ] {
            assert!(!names.contains(&excluded), "{excluded} should be excluded");
        }
        // Render churn stayed at zero — render never ran.
        assert_eq!(out.churn.render_command_lists_built, 0);
        assert_eq!(out.churn.mesh_vec_clones, 0);
        // Transform churn reflects the measured frames only.
        assert_eq!(out.churn.transform_scratch_maps_allocated, 3);
    }

    #[test]
    fn focused_render_mode_excludes_unrelated_frame_phases() {
        let out = run(config(FocusPhase::RenderCommandBuild, 3, 1));
        let names: Vec<&str> = out.phases.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["render_command_build"]);
        // Transform churn stayed at zero in the measured loop — transform never
        // ran (the one-shot stable-visible pass uses a throwaway counter).
        assert_eq!(out.churn.transform_scratch_maps_allocated, 0);
        assert_eq!(out.churn.transform_parent_lookups, 0);
        // Render churn reflects the measured frames: 3 frames * 4 clones.
        assert_eq!(out.churn.render_command_lists_built, 3);
        assert_eq!(out.churn.mesh_vec_clones, 12);
    }

    #[test]
    fn warmup_is_excluded_from_measured_sample_counts() {
        let measured = 4;
        let out = run(config(FocusPhase::TransformUpdate, measured, 10));
        // Despite 10 warmup iterations, the measured sample count is exactly 4.
        assert_eq!(out.frames.count(), measured);
        let transform = &out.phases[0];
        assert_eq!(transform.sample_count, measured);
        // And churn counts only the 4 measured frames, not the 10 warmups.
        assert_eq!(out.churn.transform_scratch_maps_allocated, measured);
    }

    #[test]
    fn zero_warmup_still_measures_every_frame() {
        let out = run(config(FocusPhase::RenderCommandBuild, 5, 0));
        assert_eq!(out.frames.count(), 5);
        assert_eq!(out.phases[0].sample_count, 5);
    }

    #[test]
    fn placeholder_phases_are_disclosed_even_in_focus_modes() {
        let out = run(config(FocusPhase::TransformUpdate, 2, 0));
        assert_eq!(
            out.placeholder_phases,
            vec![
                "bounds_update_placeholder".to_string(),
                "visibility_or_culling_placeholder".to_string()
            ]
        );
    }

    #[test]
    fn lattice_side_holds_count_and_is_at_least_one() {
        assert_eq!(lattice_side(0), 1);
        assert_eq!(lattice_side(8), 2);
        assert_eq!(lattice_side(25_000), 30);
        assert!(lattice_side(25_000).pow(3) >= 25_000);
    }

    #[test]
    fn unit_cube_has_eight_vertices_and_twelve_triangles() {
        let cube = unit_cube();
        assert_eq!(cube.positions.len(), 8);
        assert_eq!(cube.indices.len(), 36);
        assert!(cube
            .indices
            .iter()
            .all(|&i| (i as usize) < cube.positions.len()));
    }
}
