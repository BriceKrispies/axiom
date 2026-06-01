//! The deterministic rotating-cube slice driver.
//!
//! This is the browser app's equivalent of the headless app's
//! `DemoRotatingCubeApi`. Apps are leaves in the dependency graph, so this
//! app may not depend on `apps/axiom-demo-rotating-cube`; instead it composes
//! the **same engine modules** (`scene`, `resources`, `render`, `webgpu`) and
//! layers to produce the **same** deterministic `GpuSubmission` shape. It is
//! entirely browser-free and natively testable.
//!
//! The scene is built **once** from data ([`crate::scene_content`]) and held as
//! a durable `SceneApi` world; each tick only advances it (the engine animates
//! the cubes' `Spin`) and re-derives the per-frame render input from the
//! resulting snapshot. The world is authored state that evolves, not a graph
//! rebuilt every frame.

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
/// (column-major, wgpu clip-depth corrected) and RGBA colour.
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

/// The cube mesh resolved into the separate vertex streams a `RenderInput`
/// wants, extracted from `axiom-resources` once and held for per-frame reuse.
#[derive(Debug, Clone)]
struct CubeMeshData {
    id: u64,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
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
    render_api: RenderApi,
    runtime: Runtime,
    driver: HostStepDriver,
    frame_builder: FrameBuilder,
    viewport: HostViewport,
    /// The scene content, as data (clear colour + light direction are read each
    /// frame; the rest was consumed building the world once).
    content: SceneContent,
    /// The durable world: built once from `content`, advanced every tick.
    scene: SceneApi,
    /// The cube mesh, resolved once.
    mesh: CubeMeshData,
    /// `(material id, colour)` for each cube's material, resolved once.
    materials: Vec<(u64, [f32; 4])>,
}

impl CubeSliceDriver {
    /// Build the driver from the built-in demo scene.
    pub fn new(viewport: HostViewport) -> Self {
        Self::build_from(demo_scene(), viewport)
    }

    /// Build the driver from a serialized scene **document** (kernel `Reflect`
    /// bytes) — the same content authored as data that could come from a file,
    /// a network fetch, or an agent, instead of the in-code default.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn from_document(
        bytes: &[u8],
        viewport: HostViewport,
    ) -> axiom_kernel::KernelResult<Self> {
        Ok(Self::build_from(SceneContent::from_bytes(bytes)?, viewport))
    }

    /// Build the driver from decoded scene content. The scene and its resources
    /// are constructed **once** here.
    fn build_from(content: SceneContent, viewport: HostViewport) -> Self {
        let math = MathApi::new();
        let host_api = HostApi::new();
        let frame_api = FrameApi::new();
        let resources_api = ResourcesApi::new();
        let render_api = RenderApi::new();

        let mut runtime =
            Runtime::new(RuntimeConfig::new(FIXED_STEP_NANOS).with_diagnostics_enabled(false))
                .expect("runtime config is valid for the demo fixed step");
        runtime.initialize().expect("runtime initialize cannot fail");
        runtime.start().expect("runtime start cannot fail");

        let boundary_config = host_api
            .boundary_config(FIXED_STEP_NANOS, 1)
            .expect("max-steps-per-frame = 1 is valid");
        let mut driver = host_api.step_driver(boundary_config);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);

        let frame_builder = frame_api.frame_builder(FIXED_STEP_NANOS);

        // --- Resources, resolved once. The shared cube mesh + one material per
        //     cube, extracted into plain data the per-frame render input reuses.
        let mut resources = resources_api.empty_table();
        let mesh_id = resources_api.register_cube_mesh(&mut resources);
        let mut materials: Vec<(u64, [f32; 4])> = Vec::with_capacity(content.cubes.len());
        for cube in &content.cubes {
            let material_id = resources_api.register_basic_lit_material(
                &mut resources,
                Vec4::new(cube.color[0], cube.color[1], cube.color[2], cube.color[3]),
            );
            materials.push((material_id.raw(), cube.color));
        }
        // Extract the resolved cube mesh into plain data (the resolved-resources
        // value is not nameable outside its module, so this stays inline).
        let resolved = resources_api.resolve(&resources);
        let mesh = {
            let id = mesh_id.raw();
            let vc = resources_api
                .resolved_mesh_vertex_count(&resolved, id)
                .expect("cube mesh present");
            let mut positions = Vec::with_capacity(vc);
            let mut normals = Vec::with_capacity(vc);
            let mut uvs = Vec::with_capacity(vc);
            for v in 0..vc {
                let p = resources_api.resolved_mesh_position_at(&resolved, id, v).expect("vertex in range");
                let n = resources_api.resolved_mesh_normal_at(&resolved, id, v).expect("vertex in range");
                let u = resources_api.resolved_mesh_uv_at(&resolved, id, v).expect("vertex in range");
                positions.push(Vec3::new(p[0], p[1], p[2]));
                normals.push(Vec3::new(n[0], n[1], n[2]));
                uvs.push(Vec2::new(u[0], u[1]));
            }
            let indices = resources_api
                .resolved_mesh_indices(&resolved, id)
                .expect("cube mesh present")
                .to_vec();
            CubeMeshData { id, positions, normals, uvs, indices }
        };

        // --- The world, built once from the content data. Each cube is a
        //     translation parent + a child carrying an engine `Spin` and a
        //     renderable; plus a camera and a light. Advancing animates it.
        let aspect = viewport.physical_width() as f32 / viewport.physical_height() as f32;
        let mut scene = SceneApi::new();
        for (cube, &(material_id, _)) in content.cubes.iter().zip(materials.iter()) {
            let parent = scene
                .create_node_with_transform(Transform::from_translation(Vec3::new(cube.offset_x, 0.0, 0.0)));
            let child = scene.create_node();
            scene.set_parent(child, parent).expect("cube nodes were just created");
            scene
                .add_spin(child, cube.spin_axis, cube.period_ticks)
                .expect("cube child was just created");
            let mesh_ref = scene.mesh_ref(mesh.id);
            let material_ref = scene.material_ref(material_id);
            scene
                .add_renderable(child, mesh_ref, material_ref)
                .expect("renderable refs are valid");
        }
        let camera_node = scene
            .create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 0.0, content.camera.offset_z)));
        let light_node = scene.create_node_with_transform(Transform::IDENTITY);
        scene
            .add_perspective_camera(
                &math,
                camera_node,
                content.camera.fovy_radians,
                aspect,
                content.camera.near,
                content.camera.far,
            )
            .expect("camera intrinsics are valid");
        scene
            .add_directional_light(&math, light_node, content.light.color, content.light.intensity)
            .expect("light parameters are valid");

        CubeSliceDriver {
            math,
            frame_api,
            render_api,
            runtime,
            driver,
            frame_builder,
            viewport,
            content,
            scene,
            mesh,
            materials,
        }
    }

    /// Drive one tick: advance the held world, derive the deterministic
    /// `GpuSubmission`, submit it through the given `WebGpuApi`, and summarise.
    pub fn drive_tick(&mut self, webgpu: &WebGpuApi, tick: u64) -> TickOutcome {
        let width = self.viewport.physical_width();
        let height = self.viewport.physical_height();

        // 1. Drive one host frame through the runtime, build the engine frame.
        let host_input = HostFrameInput::new(tick + 1, FIXED_STEP_NANOS, self.viewport);
        let host_report = self
            .driver
            .drive(&mut self.runtime, host_input)
            .expect("driver inputs are deterministic and valid");
        let engine_frame = self
            .frame_builder
            .build(&host_report, Vec::new())
            .expect("host report sequence is monotone");
        let frame_ctx = self.frame_api.frame_context(&engine_frame);

        // 2. Advance the durable world at this tick (animates Spin + propagates).
        let snapshot = self.scene.advance(tick, &frame_ctx);

        // 3. Build the per-frame render input from the snapshot + held resources.
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
        let mesh_render_idx = self.render_api.add_input_mesh(
            &mut input,
            self.mesh.id,
            self.mesh.positions.clone(),
            self.mesh.normals.clone(),
            self.mesh.uvs.clone(),
            self.mesh.indices.clone(),
        );
        let mut material_index_by_id: Vec<(u64, u32)> = Vec::with_capacity(self.materials.len());
        for &(id, color) in &self.materials {
            let render_idx = self.render_api.add_input_basic_lit_material(
                &mut input,
                id,
                Vec4::new(color[0], color[1], color[2], color[3]),
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
            let material_idx = material_index_by_id
                .iter()
                .find(|(id, _)| *id == renderable.material().raw())
                .map(|(_, i)| *i)
                .expect("material ref resolves");
            self.render_api
                .add_input_object(&mut input, world, mesh_render_idx, material_idx, renderable.visible());
        }

        // 4. Compile RenderInput -> RenderCommandList.
        let commands = self.render_api.build_command_list(&input);

        // 5. Translate RenderCommandList -> GpuSubmission (same contract).
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
                        current_color = self
                            .materials
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

        // 6. Submit through the (live or recording) backend.
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
        let mut vertices = Vec::with_capacity(self.mesh.positions.len() * 6);
        for (p, n) in self.mesh.positions.iter().zip(self.mesh.normals.iter()) {
            vertices.extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z]);
        }
        CubeGeometry {
            vertices,
            indices: self.mesh.indices.clone(),
        }
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
        // After a non-zero tick the three MVPs differ (different axes +
        // offsets). Engine-driven Spin animates them from the tick.
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let c = driver.drive_tick(&webgpu, 45).cubes;
        assert_ne!(c[0].mvp_cols, c[1].mvp_cols);
        assert_ne!(c[1].mvp_cols, c[2].mvp_cols);
        assert_ne!(c[0].mvp_cols, c[2].mvp_cols);
    }

    #[test]
    fn a_held_world_evolves_across_ticks() {
        // The same driver (one built world) at two ticks yields different cube
        // MVPs — the world is advanced, not rebuilt-from-identical-data.
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let early = driver.drive_tick(&webgpu, 10).cubes;
        let later = driver.drive_tick(&webgpu, 200).cubes;
        assert_ne!(early[0].mvp_cols, later[0].mvp_cols);
    }

    #[test]
    fn a_driver_built_from_a_document_matches_the_built_in_scene() {
        // Serialize the demo scene to a document, load a driver from those
        // bytes, and confirm it renders identically to the in-code default —
        // i.e. the scene is genuinely loadable as data.
        let webgpu = WebGpuApi::new_recording();
        let bytes = demo_scene().to_bytes();
        let mut from_doc =
            CubeSliceDriver::from_document(&bytes, viewport(800, 600)).expect("document loads");
        let mut built_in = CubeSliceDriver::new(viewport(800, 600));
        assert_eq!(from_doc.drive_tick(&webgpu, 30), built_in.drive_tick(&webgpu, 30));
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
