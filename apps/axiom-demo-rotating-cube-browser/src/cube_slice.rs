//! The deterministic rotating-cube slice driver.
//!
//! This is the browser app's equivalent of the headless app's
//! `DemoRotatingCubeApi`. Apps are leaves in the dependency graph, so this
//! app may not depend on `apps/axiom-demo-rotating-cube`; instead it composes
//! the same engine pieces. The scene→render→GPU *translation* no longer lives
//! here — it is the `axiom-render-pipeline` feature module. This driver only:
//! builds the scene once from data, drives the engine frame, advances the
//! world, and hands it to the pipeline with a WebGPU backend.

use axiom_frame::{FrameApi, FrameBuilder};
use axiom_host::{HostApi, HostFrameInput, HostLifecycleSignal, HostStepDriver, HostViewport};
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{MathApi, Transform, Vec2, Vec3, Vec4};
use axiom_render_pipeline::RenderPipelineApi;
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
    /// Number of GPU commands in the submission.
    pub gpu_command_count: usize,
    pub clear_color: [f32; 4],
    /// One entry per drawn cube, in submission order.
    pub cubes: Vec<CubeInstance>,
    /// Whether the backend presented real pixels.
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

/// The cube's static CPU geometry: interleaved `[px, py, pz, nx, ny, nz]` (6
/// floats each) plus the index list. This is the engine's cube mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct CubeGeometry {
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
}

/// The cube mesh resolved into the separate vertex streams the pipeline wants,
/// extracted from `axiom-resources` once and held for per-frame reuse.
#[derive(Debug, Clone)]
struct CubeMeshData {
    id: u64,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

/// Persistent deterministic driver state carried across ticks.
#[derive(Debug)]
pub struct CubeSliceDriver {
    frame_api: FrameApi,
    pipeline: RenderPipelineApi,
    runtime: Runtime,
    driver: HostStepDriver,
    frame_builder: FrameBuilder,
    viewport: HostViewport,
    /// The scene content, as data (clear colour + light direction read each
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

        // --- Resources, resolved once: shared cube mesh + one material per cube.
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
        // Extract the resolved cube mesh into plain data (the resolved value is
        // not nameable outside its module, so this stays inline).
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
        //     renderable; plus a camera and a light.
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
                Radians::new(content.camera.fovy_radians).expect("fovy is finite"),
                Ratio::new(aspect).expect("aspect is finite"),
                Meters::new(content.camera.near).expect("near plane is finite"),
                Meters::new(content.camera.far).expect("far plane is finite"),
            )
            .expect("camera intrinsics are valid");
        scene
            .add_directional_light(
                &math,
                light_node,
                content.light.color,
                Ratio::new(content.light.intensity).expect("light intensity is finite"),
            )
            .expect("light parameters are valid");

        CubeSliceDriver {
            frame_api,
            pipeline: RenderPipelineApi::new(),
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

    /// Drive one tick: advance the held world, then hand it to the render
    /// pipeline with the given `WebGpuApi` backend and summarise the outcome.
    pub fn drive_tick(&mut self, webgpu: &WebGpuApi, tick: u64) -> TickOutcome {
        let width = self.viewport.physical_width();
        let height = self.viewport.physical_height();

        // 1. Drive one host frame; build the engine frame.
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
        self.scene.advance(tick, &frame_ctx);

        // 3. Hand the world + assets to the render pipeline.
        let mut frame = self.pipeline.new_frame(
            width,
            height,
            self.content.clear_color,
            self.content.light.direction_world,
        );
        self.pipeline.frame_add_mesh(
            &mut frame,
            self.mesh.id,
            self.mesh.positions.clone(),
            self.mesh.normals.clone(),
            self.mesh.uvs.clone(),
            self.mesh.indices.clone(),
        );
        for &(id, color) in &self.materials {
            self.pipeline.frame_add_material(&mut frame, id, color);
        }
        let report = self.pipeline.submit(&frame, &self.scene, webgpu);

        // 4. Build per-cube GPU instances: mvp = view_projection * world.
        let view_projection = self.pipeline.report_view_projection(&report);
        let draw_count = self.pipeline.report_draw_count(&report);
        let mut cubes = Vec::with_capacity(draw_count);
        for i in 0..draw_count {
            let world = self.pipeline.report_draw_world(&report, i).expect("draw index in range");
            let color = self.pipeline.report_draw_color(&report, i).expect("draw index in range");
            cubes.push(CubeInstance {
                mvp_cols: view_projection.multiply(world).as_cols_array(),
                color,
            });
        }

        TickOutcome {
            tick,
            gpu_command_count: self.pipeline.report_command_count(&report),
            clear_color: self.pipeline.report_clear_color(&report),
            cubes,
            presented: self.pipeline.report_presented(&report),
            recorded: self.pipeline.report_recorded(&report),
        }
    }

    /// The engine's built-in cube mesh as interleaved position+normal vertices
    /// and indices, for upload to the live GPU binding.
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
            .viewport(
                w,
                h,
                Ratio::new(1.0).expect("unit scale factor is finite"),
            )
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
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let c = driver.drive_tick(&webgpu, 45).cubes;
        assert_ne!(c[0].mvp_cols, c[1].mvp_cols);
        assert_ne!(c[1].mvp_cols, c[2].mvp_cols);
        assert_ne!(c[0].mvp_cols, c[2].mvp_cols);
    }

    #[test]
    fn a_held_world_evolves_across_ticks() {
        let webgpu = WebGpuApi::new_recording();
        let mut driver = CubeSliceDriver::new(viewport(800, 600));
        let early = driver.drive_tick(&webgpu, 10).cubes;
        let later = driver.drive_tick(&webgpu, 200).cubes;
        assert_ne!(early[0].mvp_cols, later[0].mvp_cols);
    }

    #[test]
    fn a_driver_built_from_a_document_matches_the_built_in_scene() {
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
