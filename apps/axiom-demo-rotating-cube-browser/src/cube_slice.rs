//! The deterministic rotating-cube slice driver.
//!
//! This is the browser app's equivalent of the headless app's
//! `DemoRotatingCubeApi`. Apps are leaves in the dependency graph, so this
//! app may not depend on `apps/axiom-demo-rotating-cube`; instead it composes
//! the **same engine modules** (`scene`, `resources`, `render`, `webgpu`) and
//! layers to produce the **same** deterministic `GpuSubmission` shape. It is
//! entirely browser-free and natively testable.
//!
//! The only difference from the headless app is *which* `WebGpuApi` the
//! submission is fed into: the headless app uses recording mode; here the
//! caller passes a live-mode `WebGpuApi`. The submission contract is
//! identical, so there is no second command model and no forked render path.

use axiom_frame::{FrameApi, FrameBuilder};
use axiom_host::{HostApi, HostFrameInput, HostLifecycleSignal, HostStepDriver, HostViewport};
use axiom_math::{MathApi, Mat4, Transform, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;
use axiom_resources::ResourcesApi;
use axiom_runtime::{Runtime, RuntimeConfig};
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;

use crate::scene_content::{demo_scene, SceneContent};

/// Fixed 1 ms simulation step (matches the headless slice).
pub(crate) const FIXED_STEP_NANOS: u64 = 1_000_000;

/// Number of cubes the demo scene draws (one per spin axis).
pub(crate) const NUM_CUBES: usize = 3;

/// One cube's GPU instance data: its full model-view-projection matrix
/// (column-major, wgpu clip-depth corrected) and RGBA colour. Both are derived
/// from engine artifacts — the render command list's camera + per-draw world,
/// and the material colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubeInstance {
    pub mvp_cols: [f32; 16],
    pub color: [f32; 4],
}

/// The deterministic summary of one driven tick.
#[derive(Debug, Clone, PartialEq)]
pub struct TickOutcome {
    pub tick: u64,
    /// Number of GPU commands in the submission (clear + camera + pipeline +
    /// 3 × (mesh + material + draw) + present = 13 for three cubes).
    pub gpu_command_count: usize,
    pub clear_color: [f32; 4],
    /// One entry per drawn cube, in submission order.
    pub cubes: Vec<CubeInstance>,
    /// Whether the backend presented real pixels. `false` until a live
    /// device/surface is bound (see VISIBLE_SLICE.md).
    pub presented: bool,
    /// Whether the recording backend produced this report.
    pub recorded: bool,
}

impl TickOutcome {
    /// Flatten the cube instances to GPU-upload floats — `[mvp(16), color(4)]`
    /// per cube — for the wasm live binding's per-instance buffer.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn instance_floats(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.cubes.len() * 20);
        for c in &self.cubes {
            out.extend_from_slice(&c.mvp_cols);
            out.extend_from_slice(&c.color);
        }
        out
    }
}

/// The cube's static CPU geometry, resolved from `axiom-resources`: vertices
/// interleaved as `[px, py, pz, nx, ny, nz]` (6 floats each) plus the index
/// list. This is the engine's cube mesh, not a hand-authored one.
#[derive(Debug, Clone, PartialEq)]
pub struct CubeGeometry {
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
}

/// Column-major matrix that remaps OpenGL clip depth `z' = (z + w) / 2` so the
/// engine's `[-1,1]` projection lands in wgpu's `[0,1]` clip space.
const GL_TO_WGPU_DEPTH: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.5, 0.0, //
    0.0, 0.0, 0.5, 1.0, //
];

/// Persistent deterministic driver state carried across ticks.
#[derive(Debug)]
pub struct CubeSliceDriver {
    math: MathApi,
    frame_api: FrameApi,
    scene_api: SceneApi,
    resources_api: ResourcesApi,
    render_api: RenderApi,
    runtime: Runtime,
    driver: HostStepDriver,
    frame_builder: FrameBuilder,
    viewport: HostViewport,
    /// The scene content, as data. Interpreted into the world each tick.
    content: SceneContent,
}

impl CubeSliceDriver {
    /// Build the driver for a viewport of `width` x `height` logical pixels.
    pub fn new(viewport: HostViewport) -> Self {
        let math = MathApi::new();
        let host_api = HostApi::new();
        let frame_api = FrameApi::new();
        let scene_api = SceneApi::new();
        let resources_api = ResourcesApi::new();
        let render_api = RenderApi::new();

        let mut runtime = Runtime::new(
            RuntimeConfig::new(FIXED_STEP_NANOS).with_diagnostics_enabled(false),
        )
        .expect("runtime config is valid for the demo fixed step");
        runtime.initialize().expect("runtime initialize cannot fail");
        runtime.start().expect("runtime start cannot fail");

        let boundary_config = host_api
            .boundary_config(FIXED_STEP_NANOS, 1)
            .expect("max-steps-per-frame = 1 is valid");
        let mut driver = host_api.step_driver(boundary_config);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);

        let frame_builder = frame_api.frame_builder(FIXED_STEP_NANOS);

        CubeSliceDriver {
            math,
            frame_api,
            scene_api,
            resources_api,
            render_api,
            runtime,
            driver,
            frame_builder,
            viewport,
            content: demo_scene(),
        }
    }

    /// Drive one tick: build the rotating-cube scene, run the engine step,
    /// produce the deterministic `GpuSubmission`, submit it through the given
    /// `WebGpuApi`, and summarise the outcome.
    pub fn drive_tick(&mut self, webgpu: &WebGpuApi, tick: u64) -> TickOutcome {
        let width = self.viewport.physical_width();
        let height = self.viewport.physical_height();

        // 1. Interpret the scene content (data) into the world. Each cube is a
        //    translation parent + a child carrying an engine `Spin` (so the
        //    engine animates the rotation each advance — no per-tick rotation
        //    code here) and a renderable. The shared cube mesh is registered
        //    once; each cube gets its own material from its data colour.
        let mut scene = self.scene_api.empty_scene();
        let mut resources = self.resources_api.empty_table();
        let mesh_id = self.resources_api.register_cube_mesh(&mut resources);
        let mesh_raw = mesh_id.raw();

        let mut material_colors: Vec<(u64, [f32; 4])> = Vec::with_capacity(self.content.cubes.len());
        for cube in &self.content.cubes {
            let parent = self.scene_api.create_node_with_transform(
                &mut scene,
                Transform::from_translation(Vec3::new(cube.offset_x, 0.0, 0.0)),
            );
            let child = self.scene_api.create_node(&mut scene);
            self.scene_api
                .set_parent(&mut scene, child, parent)
                .expect("cube nodes were just created");
            self.scene_api
                .add_spin(&mut scene, child, cube.spin_axis, cube.period_ticks)
                .expect("cube child was just created");
            let material_id = self.resources_api.register_basic_lit_material(
                &mut resources,
                Vec4::new(cube.color[0], cube.color[1], cube.color[2], cube.color[3]),
            );
            material_colors.push((material_id.raw(), cube.color));
            let mesh_ref = self.scene_api.mesh_ref(mesh_raw);
            let material_ref = self.scene_api.material_ref(material_id.raw());
            self.scene_api
                .add_renderable(&mut scene, child, mesh_ref, material_ref)
                .expect("renderable refs are valid");
        }

        // 2. Camera + light, from data.
        let camera_node = self.scene_api.create_node_with_transform(
            &mut scene,
            Transform::from_translation(Vec3::new(0.0, 0.0, self.content.camera.offset_z)),
        );
        let light_node = self
            .scene_api
            .create_node_with_transform(&mut scene, Transform::IDENTITY);
        let aspect = width as f32 / height as f32;
        self.scene_api
            .add_perspective_camera(
                &self.math,
                &mut scene,
                camera_node,
                self.content.camera.fovy_radians,
                aspect,
                self.content.camera.near,
                self.content.camera.far,
            )
            .expect("camera intrinsics are valid");
        self.scene_api
            .add_directional_light(
                &self.math,
                &mut scene,
                light_node,
                self.content.light.color,
                self.content.light.intensity,
            )
            .expect("light parameters are valid");

        // 4. Drive one host frame through the runtime.
        let host_input = HostFrameInput::new(tick + 1, FIXED_STEP_NANOS, self.viewport);
        let host_report = self
            .driver
            .drive(&mut self.runtime, host_input)
            .expect("driver inputs are deterministic and valid");

        // 5. Engine frame + 6. snapshot.
        let engine_frame = self
            .frame_builder
            .build(&host_report, Vec::new())
            .expect("host report sequence is monotone");
        let frame_ctx = self.frame_api.frame_context(&engine_frame);
        let snapshot = self.scene_api.advance(&mut scene, tick, &frame_ctx);

        // 7. Resolve resources.
        let resolved = self.resources_api.resolve(&resources);

        // 8. Build render input (scene + resources -> RenderInput).
        let mut input = self.render_api.new_input(width, height);
        self.render_api.set_input_clear_color(&mut input, self.content.clear_color);
        let cam = snapshot.cameras().first().expect("the demo has one camera");
        let cam_world = snapshot
            .nodes()
            .iter()
            .find(|n| n.id() == cam.node())
            .expect("camera node present")
            .world();
        let view = cam_world
            .inverse()
            .expect("camera node has identity scale")
            .to_matrix();
        let projection = self
            .math
            .mat4_perspective(cam.fovy_radians(), cam.aspect(), cam.near(), cam.far())
            .expect("camera intrinsics validated at insertion");
        self.render_api.set_input_camera(&mut input, view, projection);
        for light in snapshot.lights() {
            self.render_api.add_input_directional_light(
                &mut input,
                self.content.light.direction_world,
                light.color(),
                light.intensity(),
            );
        }
        let mut mesh_index_by_id: Vec<(u64, u32)> = Vec::new();
        for i in 0..self.resources_api.resolved_mesh_count(&resolved) {
            let id = self
                .resources_api
                .resolved_mesh_id_at(&resolved, i)
                .expect("mesh index in range");
            let vc = self
                .resources_api
                .resolved_mesh_vertex_count(&resolved, id)
                .expect("mesh present");
            let mut positions = Vec::with_capacity(vc);
            let mut normals = Vec::with_capacity(vc);
            let mut uvs = Vec::with_capacity(vc);
            for v in 0..vc {
                let p = self
                    .resources_api
                    .resolved_mesh_position_at(&resolved, id, v)
                    .expect("vertex in range");
                let n = self
                    .resources_api
                    .resolved_mesh_normal_at(&resolved, id, v)
                    .expect("vertex in range");
                let u = self
                    .resources_api
                    .resolved_mesh_uv_at(&resolved, id, v)
                    .expect("vertex in range");
                positions.push(Vec3::new(p[0], p[1], p[2]));
                normals.push(Vec3::new(n[0], n[1], n[2]));
                uvs.push(Vec2::new(u[0], u[1]));
            }
            let indices = self
                .resources_api
                .resolved_mesh_indices(&resolved, id)
                .expect("mesh present")
                .to_vec();
            let render_idx =
                self.render_api.add_input_mesh(&mut input, id, positions, normals, uvs, indices);
            mesh_index_by_id.push((id, render_idx));
        }
        let mut material_index_by_id: Vec<(u64, u32)> = Vec::new();
        for i in 0..self.resources_api.resolved_material_count(&resolved) {
            let id = self
                .resources_api
                .resolved_material_id_at(&resolved, i)
                .expect("material index in range");
            let c = self
                .resources_api
                .resolved_material_base_color(&resolved, id)
                .expect("material present");
            let render_idx = self.render_api.add_input_basic_lit_material(
                &mut input,
                id,
                Vec4::new(c[0], c[1], c[2], c[3]),
            );
            material_index_by_id.push((id, render_idx));
        }
        for renderable in snapshot.renderables() {
            let world = snapshot
                .nodes()
                .iter()
                .find(|n| n.id() == renderable.node())
                .expect("renderable node present")
                .world()
                .to_matrix();
            let mesh_idx = mesh_index_by_id
                .iter()
                .find(|(id, _)| *id == renderable.mesh().raw())
                .map(|(_, i)| *i)
                .expect("mesh ref resolves");
            let material_idx = material_index_by_id
                .iter()
                .find(|(id, _)| *id == renderable.material().raw())
                .map(|(_, i)| *i)
                .expect("material ref resolves");
            self.render_api
                .add_input_object(&mut input, world, mesh_idx, material_idx, renderable.visible());
        }

        // 9. Compile RenderInput -> RenderCommandList.
        let commands = self.render_api.build_command_list(&input);

        // 10. Translate RenderCommandList -> GpuSubmission (same contract).
        let mut submission = webgpu.new_submission(width, height);
        let count = self.render_api.command_count(&commands);
        for i in 0..count {
            match self.render_api.command_kind_at(&commands, i) {
                Some(RenderApi::KIND_CLEAR_FRAME) => {
                    if let Some(c) = self.render_api.command_clear_color_at(&commands, i) {
                        webgpu.submission_clear_frame(&mut submission, c);
                    }
                }
                Some(RenderApi::KIND_SET_CAMERA) => {
                    if let Some((v, p)) = self.render_api.command_camera_at(&commands, i) {
                        webgpu.submission_set_camera(&mut submission, v, p);
                    }
                }
                Some(RenderApi::KIND_SET_PIPELINE) => {
                    if let Some(id) = self.render_api.command_pipeline_at(&commands, i) {
                        webgpu.submission_set_pipeline(&mut submission, id);
                    }
                }
                Some(RenderApi::KIND_SET_MESH) => {
                    if let Some(id) = self.render_api.command_mesh_id_at(&commands, i) {
                        webgpu.submission_set_mesh(&mut submission, id);
                    }
                }
                Some(RenderApi::KIND_SET_MATERIAL) => {
                    if let Some(id) = self.render_api.command_material_id_at(&commands, i) {
                        webgpu.submission_set_material(&mut submission, id);
                    }
                }
                Some(RenderApi::KIND_DRAW_INDEXED) => {
                    if let Some((c, w)) = self.render_api.command_draw_indexed_at(&commands, i) {
                        webgpu.submission_draw_indexed(&mut submission, c, w);
                    }
                }
                _ => {}
            }
        }
        webgpu.submission_present(&mut submission);

        let clear_color = self
            .render_api
            .command_clear_color_at(&commands, 0)
            .unwrap_or([0.0; 4]);

        // Walk the command list once: each SetMaterial selects the colour the
        // following DrawIndexed uses, and each DrawIndexed carries that cube's
        // world matrix. Build one CubeInstance per draw, with the wgpu
        // clip-depth-corrected MVP = depth_fix * projection * view * world.
        let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
        let view_proj = depth_fix.multiply(projection).multiply(view);
        let mut cubes: Vec<CubeInstance> = Vec::with_capacity(NUM_CUBES);
        let mut current_color = [1.0_f32; 4];
        for i in 0..count {
            match self.render_api.command_kind_at(&commands, i) {
                Some(RenderApi::KIND_SET_MATERIAL) => {
                    if let Some(id) = self.render_api.command_material_id_at(&commands, i) {
                        current_color = material_colors
                            .iter()
                            .find(|(m, _)| *m == id)
                            .map(|(_, c)| *c)
                            .unwrap_or([1.0; 4]);
                    }
                }
                Some(RenderApi::KIND_DRAW_INDEXED) => {
                    if let Some((_, world)) = self.render_api.command_draw_indexed_at(&commands, i) {
                        let mvp = view_proj.multiply(world);
                        cubes.push(CubeInstance {
                            mvp_cols: mvp.as_cols_array(),
                            color: current_color,
                        });
                    }
                }
                _ => {}
            }
        }

        // 11. Submit through the (live or recording) backend.
        let report = webgpu.submit(submission);

        TickOutcome {
            tick,
            gpu_command_count: report.submitted_command_count(),
            clear_color,
            cubes,
            presented: report.presented(),
            recorded: report.is_recorded(),
        }
    }

    /// Resolve the engine's built-in cube mesh into interleaved
    /// position+normal vertices and indices, for upload to the live GPU
    /// binding. Browser-free and deterministic.
    pub(crate) fn cube_geometry(&self) -> CubeGeometry {
        let mut table = self.resources_api.empty_table();
        let mesh_id = self.resources_api.register_cube_mesh(&mut table);
        let resolved = self.resources_api.resolve(&table);
        let id = mesh_id.raw();
        let vertex_count = self
            .resources_api
            .resolved_mesh_vertex_count(&resolved, id)
            .expect("cube mesh present");
        let mut vertices = Vec::with_capacity(vertex_count * 6);
        for v in 0..vertex_count {
            let p = self
                .resources_api
                .resolved_mesh_position_at(&resolved, id, v)
                .expect("vertex in range");
            let n = self
                .resources_api
                .resolved_mesh_normal_at(&resolved, id, v)
                .expect("vertex in range");
            vertices.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2]]);
        }
        let indices = self
            .resources_api
            .resolved_mesh_indices(&resolved, id)
            .expect("cube mesh present")
            .to_vec();
        CubeGeometry { vertices, indices }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport(w: u32, h: u32) -> HostViewport {
        HostApi::new()
            .viewport(&MathApi::new(), w, h, 1.0)
            .expect("valid viewport")
    }

    #[test]
    fn recording_driver_produces_three_cube_submission() {
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let outcome = driver.drive_tick(&webgpu, 0);
        // Clear + SetCamera + SetPipeline + 3 x (SetMesh + SetMaterial +
        // DrawIndexed) + Present = 13.
        assert_eq!(outcome.gpu_command_count, 13);
        assert_eq!(outcome.cubes.len(), NUM_CUBES);
        assert!(outcome.recorded);
        assert!(!outcome.presented);
        assert_eq!(outcome.clear_color, demo_scene().clear_color);
    }

    #[test]
    fn the_three_cubes_have_distinct_colors() {
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let c = driver.drive_tick(&webgpu, 0).cubes;
        assert_eq!(c.len(), 3);
        assert_ne!(c[0].color, c[1].color);
        assert_ne!(c[1].color, c[2].color);
        assert_ne!(c[0].color, c[2].color);
    }

    #[test]
    fn the_three_cubes_spin_on_different_axes() {
        // After a non-zero rotation the three MVPs differ (different axes +
        // different offsets), and at tick 0 the vertical and horizontal cubes
        // differ only by position.
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let c = driver.drive_tick(&webgpu, 45).cubes;
        assert_ne!(c[0].mvp_cols, c[1].mvp_cols);
        assert_ne!(c[1].mvp_cols, c[2].mvp_cols);
        assert_ne!(c[0].mvp_cols, c[2].mvp_cols);
    }

    #[test]
    fn same_tick_is_deterministic() {
        let webgpu = WebGpuApi::new_recording();
        let mut a = CubeSliceDriver::new(viewport(800, 600));
        let mut b = CubeSliceDriver::new(viewport(800, 600));
        assert_eq!(a.drive_tick(&webgpu, 7), b.drive_tick(&webgpu, 7));
    }

    #[test]
    fn cube_geometry_is_the_engine_cube() {
        let driver = CubeSliceDriver::new(viewport(800, 600));
        let geo = driver.cube_geometry();
        assert_eq!(geo.vertices.len(), 24 * 6);
        assert_eq!(geo.indices.len(), 36);
        assert!(geo.vertices.iter().all(|c| c.is_finite()));
    }

    #[test]
    fn instance_floats_pack_twenty_floats_per_cube() {
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let outcome = driver.drive_tick(&webgpu, 0);
        assert_eq!(outcome.instance_floats().len(), NUM_CUBES * 20);
    }
}
