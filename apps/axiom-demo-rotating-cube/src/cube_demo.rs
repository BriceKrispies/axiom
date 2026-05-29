//! The rotating-cube demo app: builds the full module pipeline and
//! drives one tick at a time.

use axiom_host::{HostFrameInput, HostFrameReport};
use axiom_math::{Quat, Transform, Vec3};
use axiom_render::RenderApi;
use axiom_webgpu::WebGpuApi;

use crate::app_state::{AppState, VIEWPORT_HEIGHT, VIEWPORT_WIDTH};
use crate::cube_frame::CubeFrame;
use crate::translation::{
    cube_rotation_for_tick, view_matrix_from_world, DEMO_CLEAR_COLOR,
    DEMO_CUBE_BASE_COLOR, DEMO_LIGHT_COLOR, DEMO_LIGHT_DIRECTION_WORLD,
    DEMO_LIGHT_INTENSITY,
};

/// The deterministic rotating-cube vertical-slice app.
///
/// `RotatingCubeDemo::run_tick(tick)` runs the full pipeline for one
/// tick and returns a [`CubeFrame`] summarising every boundary
/// artifact. Two runs of the same tick produce equal `CubeFrame`s.
#[derive(Debug)]
pub struct RotatingCubeDemo {
    state: AppState,
}

impl RotatingCubeDemo {
    pub fn new() -> Self {
        RotatingCubeDemo {
            state: AppState::new(),
        }
    }

    /// Drive one demo tick through the entire vertical slice.
    ///
    /// The function is intentionally one block of code: most of the
    /// types involved (`Scene`, `ResourceTable`, `SceneSnapshot`,
    /// `ResolvedResources`, `RenderInput`, `RenderCommandList`,
    /// `GpuSubmission`, `GpuSubmissionReport`) are owned by their
    /// modules and not nameable here, so splitting the pipeline into
    /// helper functions would have to use generic / impl-Trait
    /// gymnastics for no benefit.
    pub fn run_tick(&mut self, tick: u64) -> CubeFrame {
        let s = &mut self.state;

        // -------- 1. Build a fresh scene with rotation derived from tick. --------
        let mut scene = s.scene_api.empty_scene();

        // Parent: rotation node spinning around +Y.
        let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, cube_rotation_for_tick(tick))
            .expect("axis is unit and angle is finite");
        let cube_root = s.scene_api.create_node_with_transform(
            &mut scene,
            Transform::from_rotation(rotation),
        );

        // Child: the cube itself at the local origin (so it rotates around root).
        let cube_child = s
            .scene_api
            .create_node_with_transform(&mut scene, Transform::IDENTITY);
        s.scene_api
            .set_parent(&mut scene, cube_child, cube_root)
            .expect("cube_child and cube_root just-created");

        // Camera node at (0, 0, 5) looking at the origin.
        let camera_node = s.scene_api.create_node_with_transform(
            &mut scene,
            Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)),
        );

        // Light node at the world origin; the light's actual direction is a
        // world-space constant.
        let light_node = s
            .scene_api
            .create_node_with_transform(&mut scene, Transform::IDENTITY);

        // -------- 2. Build a fresh resource table with the built-in cube + material. --------
        let mut resources = s.resources_api.empty_table();
        let mesh_id = s.resources_api.register_cube_mesh(&mut resources);
        let material_id = s
            .resources_api
            .register_basic_lit_material(&mut resources, DEMO_CUBE_BASE_COLOR);

        // -------- 3. Attach scene components. --------
        let _camera_id = s
            .scene_api
            .add_perspective_camera(
                &s.math,
                &mut scene,
                camera_node,
                std::f32::consts::FRAC_PI_3,
                VIEWPORT_WIDTH as f32 / VIEWPORT_HEIGHT as f32,
                0.1,
                100.0,
            )
            .expect("camera intrinsics are valid");
        let _light_id = s
            .scene_api
            .add_directional_light(
                &s.math,
                &mut scene,
                light_node,
                DEMO_LIGHT_COLOR,
                DEMO_LIGHT_INTENSITY,
            )
            .expect("light parameters are valid");
        let _renderable_id = s
            .scene_api
            .add_renderable(
                &mut scene,
                cube_child,
                s.scene_api.mesh_ref(mesh_id.raw()),
                s.scene_api.material_ref(material_id.raw()),
            )
            .expect("renderable refs are valid");

        // -------- 4. Drive one host frame through the runtime. --------
        let host_sequence = tick + 1;
        let elapsed_nanos = crate::app_state::FIXED_STEP_NANOS;
        let host_input = HostFrameInput::new(host_sequence, elapsed_nanos, s.viewport);
        let host_report: HostFrameReport = s
            .driver
            .drive(&mut s.runtime, host_input)
            .expect("driver inputs are deterministic and valid");

        // -------- 5. Build the engine frame contract. --------
        let engine_frame = s
            .frame_builder
            .build(&host_report, Vec::new())
            .expect("host report is monotone");
        let frame_ctx = s.frame_api.frame_context(&engine_frame);

        // -------- 6. Update world transforms and snapshot the scene. --------
        let scene_snapshot = s
            .scene_api
            .advance(&mut scene, &frame_ctx)
            .expect("scene advance never fails for a valid frame ctx");

        // -------- 7. Resolve resources. --------
        let resolved = s.resources_api.resolve(&resources);

        // -------- 8. Translate (snapshot + resolved) → RenderInput. --------
        let mut render_input = s
            .render_api
            .new_input(VIEWPORT_WIDTH, VIEWPORT_HEIGHT);
        s.render_api
            .set_input_clear_color(&mut render_input, DEMO_CLEAR_COLOR);

        // Camera: find the camera node's world transform in the snapshot,
        // compute view, and use the camera's intrinsics for projection.
        let cam_snapshot = scene_snapshot
            .cameras()
            .first()
            .expect("the demo always has one camera");
        let camera_node_world = scene_snapshot
            .nodes()
            .iter()
            .find(|n| n.id() == cam_snapshot.node())
            .expect("camera-node id exists in the node list")
            .world();
        let view = view_matrix_from_world(camera_node_world);
        let projection = s
            .math
            .mat4_perspective(
                cam_snapshot.fovy_radians(),
                cam_snapshot.aspect(),
                cam_snapshot.near(),
                cam_snapshot.far(),
            )
            .expect("camera intrinsics were validated at scene insertion time");
        s.render_api
            .set_input_camera(&mut render_input, view, projection);

        // Lights: translate each light snapshot to a render-facing light.
        for light in scene_snapshot.lights() {
            // Directional lights use the static demo world direction;
            // point lights use the node's world translation.
            let node_world = scene_snapshot
                .nodes()
                .iter()
                .find(|n| n.id() == light.node())
                .expect("light-node id exists in the node list")
                .world();
            let _ = node_world; // (kept for richer future light translation)
            // For the demo every light is directional with a static world dir.
            s.render_api.add_input_directional_light(
                &mut render_input,
                DEMO_LIGHT_DIRECTION_WORLD,
                light.color(),
                light.intensity(),
            );
        }

        // Meshes / materials: walk the resolved resources and add each one;
        // remember the resulting render-side index so renderables can refer
        // to them by index.
        let mut mesh_index_by_id: Vec<(u64, u32)> = Vec::new();
        for i in 0..s.resources_api.resolved_mesh_count(&resolved) {
            let mesh_id = s
                .resources_api
                .resolved_mesh_id_at(&resolved, i)
                .expect("index in range");
            let vert_count = s
                .resources_api
                .resolved_mesh_vertex_count(&resolved, mesh_id)
                .expect("mesh present");
            let mut positions = Vec::with_capacity(vert_count);
            let mut normals = Vec::with_capacity(vert_count);
            let mut uvs = Vec::with_capacity(vert_count);
            for v in 0..vert_count {
                let p = s
                    .resources_api
                    .resolved_mesh_position_at(&resolved, mesh_id, v)
                    .expect("vert in range");
                let n = s
                    .resources_api
                    .resolved_mesh_normal_at(&resolved, mesh_id, v)
                    .expect("vert in range");
                let u = s
                    .resources_api
                    .resolved_mesh_uv_at(&resolved, mesh_id, v)
                    .expect("vert in range");
                positions.push(Vec3::new(p[0], p[1], p[2]));
                normals.push(Vec3::new(n[0], n[1], n[2]));
                uvs.push(axiom_math::Vec2::new(u[0], u[1]));
            }
            let indices: Vec<u32> = s
                .resources_api
                .resolved_mesh_indices(&resolved, mesh_id)
                .expect("mesh present")
                .to_vec();
            let idx = s
                .render_api
                .add_input_mesh(&mut render_input, mesh_id, positions, normals, uvs, indices);
            mesh_index_by_id.push((mesh_id, idx));
        }
        let mut material_index_by_id: Vec<(u64, u32)> = Vec::new();
        for i in 0..s.resources_api.resolved_material_count(&resolved) {
            let mat_id = s
                .resources_api
                .resolved_material_id_at(&resolved, i)
                .expect("index in range");
            let color = s
                .resources_api
                .resolved_material_base_color(&resolved, mat_id)
                .expect("material present");
            let idx = s.render_api.add_input_basic_lit_material(
                &mut render_input,
                mat_id,
                axiom_math::Vec4::new(color[0], color[1], color[2], color[3]),
            );
            material_index_by_id.push((mat_id, idx));
        }

        // Objects: one per renderable snapshot, with the world transform
        // pulled from the renderable's node.
        for renderable in scene_snapshot.renderables() {
            let node_world = scene_snapshot
                .nodes()
                .iter()
                .find(|n| n.id() == renderable.node())
                .expect("renderable-node id exists in the node list")
                .world();
            let world_mat = node_world.to_matrix();
            let mesh_id = renderable.mesh().raw();
            let mat_id = renderable.material().raw();
            let mesh_idx = mesh_index_by_id
                .iter()
                .find(|(id, _)| *id == mesh_id)
                .map(|(_, idx)| *idx)
                .expect("renderable's mesh ref resolves to a registered mesh");
            let mat_idx = material_index_by_id
                .iter()
                .find(|(id, _)| *id == mat_id)
                .map(|(_, idx)| *idx)
                .expect("renderable's material ref resolves to a registered material");
            s.render_api.add_input_object(
                &mut render_input,
                world_mat,
                mesh_idx,
                mat_idx,
                renderable.visible(),
            );
        }

        // -------- 9. Compile RenderInput → RenderCommandList. --------
        let render_commands = s.render_api.build_command_list(&render_input);

        // -------- 10. Translate RenderCommandList → GpuSubmission. --------
        let mut gpu_sub = s.webgpu_api.new_submission(VIEWPORT_WIDTH, VIEWPORT_HEIGHT);
        let n = s.render_api.command_count(&render_commands);
        for i in 0..n {
            let kind = s
                .render_api
                .command_kind_at(&render_commands, i)
                .expect("idx in range");
            match kind {
                RenderApi::KIND_CLEAR_FRAME => {
                    let c = s
                        .render_api
                        .command_clear_color_at(&render_commands, i)
                        .expect("clear payload present");
                    s.webgpu_api.submission_clear_frame(&mut gpu_sub, c);
                }
                RenderApi::KIND_SET_CAMERA => {
                    let (v, p) = s
                        .render_api
                        .command_camera_at(&render_commands, i)
                        .expect("camera payload present");
                    s.webgpu_api.submission_set_camera(&mut gpu_sub, v, p);
                }
                RenderApi::KIND_SET_PIPELINE => {
                    let id = s
                        .render_api
                        .command_pipeline_at(&render_commands, i)
                        .expect("pipeline payload present");
                    s.webgpu_api.submission_set_pipeline(&mut gpu_sub, id);
                }
                RenderApi::KIND_SET_MESH => {
                    let id = s
                        .render_api
                        .command_mesh_id_at(&render_commands, i)
                        .expect("mesh payload present");
                    s.webgpu_api.submission_set_mesh(&mut gpu_sub, id);
                }
                RenderApi::KIND_SET_MATERIAL => {
                    let id = s
                        .render_api
                        .command_material_id_at(&render_commands, i)
                        .expect("material payload present");
                    s.webgpu_api.submission_set_material(&mut gpu_sub, id);
                }
                RenderApi::KIND_DRAW_INDEXED => {
                    let (count, world) = s
                        .render_api
                        .command_draw_indexed_at(&render_commands, i)
                        .expect("draw payload present");
                    s.webgpu_api
                        .submission_draw_indexed(&mut gpu_sub, count, world);
                }
                _ => { /* unreachable for the demo's command set */ }
            }
        }
        s.webgpu_api.submission_present(&mut gpu_sub);

        // -------- 11. Submit and capture the report. --------
        let gpu_report = s.webgpu_api.submit(gpu_sub);

        // -------- 12. Summarise everything into a CubeFrame. --------
        let render_command_kinds: Vec<u32> = (0..n)
            .map(|i| {
                s.render_api
                    .command_kind_at(&render_commands, i)
                    .unwrap_or(0)
            })
            .collect();
        let gpu_command_kinds: Vec<u32> = (0..s.webgpu_api.report_command_count(&gpu_report))
            .map(|i| s.webgpu_api.report_kind_at(&gpu_report, i).unwrap_or(0))
            .collect();

        let render_clear_color = s
            .render_api
            .command_clear_color_at(&render_commands, 0)
            .unwrap_or([0.0; 4]);
        let (view, projection) = s
            .render_api
            .command_camera_at(&render_commands, 1)
            .unwrap_or((axiom_math::Mat4::ZERO, axiom_math::Mat4::ZERO));
        let pipeline_id = s
            .render_api
            .command_pipeline_at(&render_commands, 2)
            .unwrap_or(0);
        let (draw_count, draw_world) = s
            .render_api
            .command_draw_indexed_at(&render_commands, 5)
            .unwrap_or((0, axiom_math::Mat4::IDENTITY));

        CubeFrame {
            tick,
            engine_frame_index: engine_frame.engine_frame_index(),
            host_frame_sequence: engine_frame.host_frame_sequence(),
            runtime_step_count: engine_frame.runtime_step_count(),
            scene_node_count: scene_snapshot.nodes().len() as u32,
            scene_renderable_count: scene_snapshot.renderables().len() as u32,
            render_command_kinds,
            render_clear_color,
            render_camera_view: view,
            render_camera_projection: projection,
            render_pipeline_id: pipeline_id,
            render_draw_index_count: draw_count,
            render_draw_world: draw_world,
            gpu_command_kinds,
            gpu_clear_count: s.webgpu_api.report_clear_count(&gpu_report),
            gpu_draw_count: s.webgpu_api.report_draw_count(&gpu_report),
            gpu_present_count: s.webgpu_api.report_present_count(&gpu_report),
            gpu_target_width: VIEWPORT_WIDTH,
            gpu_target_height: VIEWPORT_HEIGHT,
        }
    }
}

impl Default for RotatingCubeDemo {
    fn default() -> Self {
        RotatingCubeDemo::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_demo() -> RotatingCubeDemo {
        RotatingCubeDemo::new()
    }

    #[test]
    fn tick_zero_produces_six_render_commands() {
        let mut demo = new_demo();
        let f = demo.run_tick(0);
        // ClearFrame + SetCamera + SetPipeline + SetMesh + SetMaterial +
        // DrawIndexed
        assert_eq!(f.render_command_kinds.len(), 6);
        assert_eq!(f.render_command_kinds[0], RenderApi::KIND_CLEAR_FRAME);
        assert_eq!(f.render_command_kinds[1], RenderApi::KIND_SET_CAMERA);
        assert_eq!(f.render_command_kinds[2], RenderApi::KIND_SET_PIPELINE);
        assert_eq!(f.render_command_kinds[3], RenderApi::KIND_SET_MESH);
        assert_eq!(f.render_command_kinds[4], RenderApi::KIND_SET_MATERIAL);
        assert_eq!(f.render_command_kinds[5], RenderApi::KIND_DRAW_INDEXED);
    }

    #[test]
    fn tick_zero_uses_basic_lit_pipeline() {
        let mut demo = new_demo();
        let f = demo.run_tick(0);
        assert_eq!(f.render_pipeline_id, RenderApi::PIPELINE_BASIC_LIT);
    }

    #[test]
    fn cube_draw_has_thirty_six_indices() {
        let mut demo = new_demo();
        let f = demo.run_tick(0);
        assert_eq!(f.render_draw_index_count, 36);
    }

    #[test]
    fn gpu_submission_has_clear_draw_and_present() {
        let mut demo = new_demo();
        let f = demo.run_tick(0);
        assert_eq!(f.gpu_clear_count, 1);
        assert_eq!(f.gpu_draw_count, 1);
        assert_eq!(f.gpu_present_count, 1);
        assert_eq!(f.gpu_target_width, VIEWPORT_WIDTH);
        assert_eq!(f.gpu_target_height, VIEWPORT_HEIGHT);
    }

    #[test]
    fn scene_has_four_nodes_one_renderable() {
        let mut demo = new_demo();
        let f = demo.run_tick(0);
        // cube_root, cube_child, camera_node, light_node.
        assert_eq!(f.scene_node_count, 4);
        assert_eq!(f.scene_renderable_count, 1);
    }

    #[test]
    fn tick_zero_and_tick_60_produce_different_cube_transforms() {
        let mut demo_a = new_demo();
        let mut demo_b = new_demo();
        // Advance a to tick 60.
        for tick in 0..=59 {
            demo_a.run_tick(tick);
        }
        let f60 = demo_a.run_tick(60);
        let f0 = demo_b.run_tick(0);
        assert_ne!(f60.render_draw_world, f0.render_draw_world);
    }

    #[test]
    fn tick_60_is_value_for_value_deterministic_across_runs() {
        let drive_to_60 = || {
            let mut demo = new_demo();
            for tick in 0..60 {
                demo.run_tick(tick);
            }
            demo.run_tick(60)
        };
        assert_eq!(drive_to_60(), drive_to_60());
    }

    #[test]
    fn engine_frame_index_increments_each_tick() {
        let mut demo = new_demo();
        let f0 = demo.run_tick(0);
        let f1 = demo.run_tick(1);
        let f2 = demo.run_tick(2);
        assert_eq!(f0.engine_frame_index, 0);
        assert_eq!(f1.engine_frame_index, 1);
        assert_eq!(f2.engine_frame_index, 2);
    }

    #[test]
    fn each_tick_runs_exactly_one_runtime_step() {
        let mut demo = new_demo();
        let f = demo.run_tick(0);
        assert_eq!(f.runtime_step_count, 1);
    }

    // Keep imports live so dead-code lints don't fire even if a test is
    // commented out during local development.
    #[allow(dead_code)]
    fn _imports_live() {
        let _ = WebGpuApi::new();
    }
}
