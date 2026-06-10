//! The single public facade of the `axiom-render-pipeline` feature module.

use axiom_math::{Mat4, MathApi, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;

/// Column-major matrix that remaps OpenGL clip depth `z' = (z + w) / 2` so the
/// engine's `[-1,1]` projection lands in wgpu's `[0,1]` clip space. The report's
/// `view_projection` is pre-multiplied by this, so a caller's
/// `view_projection * world` is a wgpu-ready model-view-projection.
const GL_TO_WGPU_DEPTH: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.5, 0.0, //
    0.0, 0.0, 0.5, 1.0, //
];

/// One mesh asset supplied to a frame: the resolved CPU geometry the renderer
/// uploads, keyed by the same id the scene's renderables reference.
#[derive(Debug)]
struct MeshAsset {
    id: u64,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

/// One material asset supplied to a frame: a linear-RGBA base colour, keyed by
/// the id the scene's renderables reference.
#[derive(Debug)]
struct MaterialAsset {
    id: u64,
    color: [f32; 4],
}

/// A frame's caller-supplied inputs: viewport, clear colour, the world-space
/// light direction, and the mesh/material assets the scene's renderables refer
/// to. Built through [`RenderPipelineApi`]; the contract type is never named by
/// callers (it is an opaque value they thread back into [`RenderPipelineApi::submit`]).
#[derive(Debug)]
pub struct RenderFrame {
    width: u32,
    height: u32,
    clear_color: [f32; 4],
    light_direction: Vec3,
    meshes: Vec<MeshAsset>,
    materials: Vec<MaterialAsset>,
}

/// The deterministic result of submitting one frame: the GPU command count, the
/// clear colour, the wgpu-ready view-projection, one `(world, colour)` per drawn
/// object in submission order, and the backend flags. The contract type is
/// reached only through [`RenderPipelineApi`] accessors.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderReport {
    command_count: usize,
    clear_color: [f32; 4],
    view_projection: Mat4,
    draws: Vec<(Mat4, [f32; 4])>,
    presented: bool,
    recorded: bool,
}

/// The only public export of `axiom-render-pipeline`: the per-frame render
/// pipeline that composes scene + render + webgpu.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderPipelineApi {
    _sealed: (),
}

impl RenderPipelineApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        RenderPipelineApi { _sealed: () }
    }

    /// Begin a frame with its viewport, clear colour, and world-space light
    /// direction. Add the meshes/materials the scene references with
    /// [`Self::frame_add_mesh`] / [`Self::frame_add_material`], then
    /// [`Self::submit`].
    pub fn new_frame(
        &self,
        width: u32,
        height: u32,
        clear_color: [f32; 4],
        light_direction: Vec3,
    ) -> RenderFrame {
        RenderFrame {
            width,
            height,
            clear_color,
            light_direction,
            meshes: Vec::new(),
            materials: Vec::new(),
        }
    }

    /// Register a mesh asset (resolved CPU geometry) for this frame.
    pub fn frame_add_mesh(
        &self,
        frame: &mut RenderFrame,
        id: u64,
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        uvs: Vec<Vec2>,
        indices: Vec<u32>,
    ) {
        frame.meshes.push(MeshAsset {
            id,
            positions,
            normals,
            uvs,
            indices,
        });
    }

    /// Register a material asset (base colour) for this frame.
    pub fn frame_add_material(&self, frame: &mut RenderFrame, id: u64, color: [f32; 4]) {
        frame.materials.push(MaterialAsset { id, color });
    }

    /// Render `scene` for this frame: translate its snapshot + the frame's
    /// assets into render commands, submit them through `webgpu`, and return the
    /// deterministic report. `scene` is expected to have been advanced for the
    /// frame already.
    pub fn submit(
        &self,
        frame: &RenderFrame,
        scene: &SceneApi,
        webgpu: &WebGpuApi,
    ) -> RenderReport {
        let math = MathApi::new();
        let render = RenderApi::new();
        let snapshot = scene.snapshot();

        // ---- Build the neutral render input from the scene + assets. ----
        let mut input = render.new_input(frame.width, frame.height);
        render.set_input_clear_color(&mut input, frame.clear_color);

        // Camera: the first camera, if any. view = inverse(node world);
        // projection from validated intrinsics. The wgpu-ready view-projection
        // is reported for callers that build per-instance MVPs.
        let mut view_projection = Mat4::IDENTITY;
        if let Some(cam) = snapshot.cameras().first() {
            let cam_world = snapshot
                .nodes()
                .iter()
                .find(|n| n.id() == cam.node())
                .expect("camera node is present in the snapshot")
                .world();
            let view = cam_world
                .inverse()
                .expect("camera node has identity scale, so inverse succeeds")
                .to_matrix();
            let projection = math
                .mat4_perspective(
                    cam.fovy_radians().get(),
                    cam.aspect().get(),
                    cam.near().get(),
                    cam.far().get(),
                )
                .expect("camera intrinsics were validated at scene insertion");
            render.set_input_camera(&mut input, view, projection);
            let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
            view_projection = depth_fix.multiply(projection).multiply(view);
        }

        // Lights: every scene light is emitted as directional with the frame's
        // world-space direction; colour/intensity carry through.
        for light in snapshot.lights() {
            render.add_input_directional_light(
                &mut input,
                frame.light_direction,
                light.color(),
                light.intensity(),
            );
        }

        // Meshes / materials: registration order defines the render-side index.
        let mut mesh_index: Vec<(u64, u32)> = Vec::with_capacity(frame.meshes.len());
        for mesh in &frame.meshes {
            let idx = render.add_input_mesh(
                &mut input,
                mesh.id,
                mesh.positions.clone(),
                mesh.normals.clone(),
                mesh.uvs.clone(),
                mesh.indices.clone(),
            );
            mesh_index.push((mesh.id, idx));
        }
        let mut material_index: Vec<(u64, u32)> = Vec::with_capacity(frame.materials.len());
        for material in &frame.materials {
            let c = material.color;
            let idx = render.add_input_basic_lit_material(
                &mut input,
                material.id,
                Vec4::new(c[0], c[1], c[2], c[3]),
            );
            material_index.push((material.id, idx));
        }

        // Objects: one per renderable, resolving its mesh/material ids to the
        // render-side indices. The frame must supply an asset for every id the
        // scene references.
        for renderable in snapshot.renderables() {
            let world = snapshot
                .nodes()
                .iter()
                .find(|n| n.id() == renderable.node())
                .expect("renderable node is present in the snapshot")
                .world()
                .to_matrix();
            let mesh_idx = mesh_index
                .iter()
                .find(|(id, _)| *id == renderable.mesh().raw())
                .map(|(_, i)| *i)
                .expect("frame supplies a mesh asset for every renderable");
            let material_idx = material_index
                .iter()
                .find(|(id, _)| *id == renderable.material().raw())
                .map(|(_, i)| *i)
                .expect("frame supplies a material asset for every renderable");
            render.add_input_object(
                &mut input,
                world,
                mesh_idx,
                material_idx,
                renderable.visible(),
            );
        }

        // ---- Compile and translate to a GPU submission. ----
        let commands = render.build_command_list(&input);
        let count = render.command_count(&commands);
        let mut submission = webgpu.new_submission(frame.width, frame.height);
        // Per-kind accessors return `Some` only for their command, so each arm
        // is exercised across a real command list — no unreachable catch-all.
        for i in 0..count {
            if let Some(c) = render.command_clear_color_at(&commands, i) {
                webgpu.submission_clear_frame(&mut submission, c);
            }
            if let Some((v, p)) = render.command_camera_at(&commands, i) {
                webgpu.submission_set_camera(&mut submission, v, p);
            }
            if let Some(id) = render.command_pipeline_at(&commands, i) {
                webgpu.submission_set_pipeline(&mut submission, id);
            }
            if let Some(id) = render.command_mesh_id_at(&commands, i) {
                webgpu.submission_set_mesh(&mut submission, id);
            }
            if let Some(id) = render.command_material_id_at(&commands, i) {
                webgpu.submission_set_material(&mut submission, id);
            }
            if let Some((index_count, world)) = render.command_draw_indexed_at(&commands, i) {
                webgpu.submission_draw_indexed(&mut submission, index_count, world);
            }
        }
        webgpu.submission_present(&mut submission);

        let clear_color = render
            .command_clear_color_at(&commands, 0)
            .unwrap_or([0.0; 4]);

        // Per-draw data: walk the command list once. Each material command sets
        // the colour the following draws use; each draw carries its world.
        let mut draws: Vec<(Mat4, [f32; 4])> = Vec::new();
        let mut current_color = [1.0_f32; 4];
        for i in 0..count {
            if let Some(id) = render.command_material_id_at(&commands, i) {
                current_color = material_index
                    .iter()
                    .position(|(mid, _)| *mid == id)
                    .map(|pos| frame.materials[pos].color)
                    .unwrap_or([1.0; 4]);
            }
            if let Some((_, world)) = render.command_draw_indexed_at(&commands, i) {
                draws.push((world, current_color));
            }
        }

        let gpu_report = webgpu.submit(submission);
        RenderReport {
            command_count: gpu_report.submitted_command_count(),
            clear_color,
            view_projection,
            draws,
            presented: gpu_report.presented(),
            recorded: gpu_report.is_recorded(),
        }
    }

    // --- Report accessors (the report contract is read only through these) ---

    pub fn report_command_count(&self, report: &RenderReport) -> usize {
        report.command_count
    }

    pub fn report_clear_color(&self, report: &RenderReport) -> [f32; 4] {
        report.clear_color
    }

    /// The wgpu-ready view-projection: multiply by an object's world matrix to
    /// get its model-view-projection.
    pub fn report_view_projection(&self, report: &RenderReport) -> Mat4 {
        report.view_projection
    }

    pub fn report_draw_count(&self, report: &RenderReport) -> usize {
        report.draws.len()
    }

    /// The world matrix of the `i`-th drawn object, if present.
    pub fn report_draw_world(&self, report: &RenderReport, i: usize) -> Option<Mat4> {
        report.draws.get(i).map(|(world, _)| *world)
    }

    /// The colour of the `i`-th drawn object, if present.
    pub fn report_draw_color(&self, report: &RenderReport, i: usize) -> Option<[f32; 4]> {
        report.draws.get(i).map(|(_, color)| *color)
    }

    pub fn report_presented(&self, report: &RenderReport) -> bool {
        report.presented
    }

    pub fn report_recorded(&self, report: &RenderReport) -> bool {
        report.recorded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{Meters, Radians, Ratio};

    fn math() -> MathApi {
        MathApi::new()
    }

    /// Build a scene with a camera, a directional light, and one renderable
    /// (mesh id 1, material id 2) parented under a translated root.
    fn cube_scene() -> SceneApi {
        let mut scene = SceneApi::new();
        let root = scene.create_node_with_transform(axiom_math::Transform::from_translation(
            Vec3::new(0.0, 0.0, 0.0),
        ));
        let child = scene.create_node();
        scene.set_parent(child, root).unwrap();
        let mesh = scene.mesh_ref(1);
        let material = scene.material_ref(2);
        scene.add_renderable(child, mesh, material).unwrap();

        let camera = scene.create_node_with_transform(axiom_math::Transform::from_translation(
            Vec3::new(0.0, 0.0, 5.0),
        ));
        scene
            .add_perspective_camera(
                &math(),
                camera,
                Radians::new(std::f32::consts::FRAC_PI_3).unwrap(),
                Ratio::new(4.0 / 3.0).unwrap(),
                Meters::new(0.1).unwrap(),
                Meters::new(100.0).unwrap(),
            )
            .unwrap();
        let light = scene.create_node();
        scene
            .add_directional_light(&math(), light, Vec3::ONE, Ratio::new(1.0).unwrap())
            .unwrap();

        scene.update_world_transforms();
        scene
    }

    fn frame_with_assets(api: &RenderPipelineApi) -> RenderFrame {
        let mut frame = api.new_frame(800, 600, [0.05, 0.06, 0.08, 1.0], Vec3::new(0.3, -1.0, 0.4));
        api.frame_add_mesh(
            &mut frame,
            1,
            vec![Vec3::new(0.5, 0.5, 0.5); 24],
            vec![Vec3::new(0.0, 1.0, 0.0); 24],
            vec![Vec2::new(0.0, 0.0); 24],
            (0..36).collect(),
        );
        api.frame_add_material(&mut frame, 2, [0.8, 0.4, 0.2, 1.0]);
        frame
    }

    #[test]
    fn new_and_default_are_equivalent() {
        // Both construction paths submit the same scene to the same command count.
        let scene = cube_scene();
        let webgpu = WebGpuApi::new_recording();
        let n = RenderPipelineApi::new();
        let d = RenderPipelineApi::default();
        let rn = n.submit(&frame_with_assets(&n), &scene, &webgpu);
        let rd = d.submit(&frame_with_assets(&d), &scene, &webgpu);
        assert_eq!(n.report_command_count(&rn), d.report_command_count(&rd));
    }

    #[test]
    fn renders_a_scene_to_a_recording_submission() {
        let api = RenderPipelineApi::new();
        let scene = cube_scene();
        let frame = frame_with_assets(&api);
        let webgpu = WebGpuApi::new_recording();

        let report = api.submit(&frame, &scene, &webgpu);

        // Clear + SetCamera + SetPipeline + SetMesh + SetMaterial + DrawIndexed
        // + Present = 7 commands for one cube.
        assert_eq!(api.report_command_count(&report), 7);
        assert_eq!(api.report_clear_color(&report), [0.05, 0.06, 0.08, 1.0]);
        assert_eq!(api.report_draw_count(&report), 1);
        assert_eq!(
            api.report_draw_color(&report, 0),
            Some([0.8, 0.4, 0.2, 1.0])
        );
        assert!(api.report_draw_world(&report, 0).is_some());
        assert!(api.report_draw_world(&report, 9).is_none());
        assert!(api.report_draw_color(&report, 9).is_none());
        // A real camera makes the view-projection non-identity.
        assert_ne!(api.report_view_projection(&report), Mat4::IDENTITY);
        assert!(api.report_recorded(&report));
        assert!(!api.report_presented(&report));
    }

    #[test]
    fn deterministic_for_identical_input() {
        let api = RenderPipelineApi::new();
        let webgpu = WebGpuApi::new_recording();
        let a = api.submit(&frame_with_assets(&api), &cube_scene(), &webgpu);
        let b = api.submit(&frame_with_assets(&api), &cube_scene(), &webgpu);
        assert_eq!(a, b);
    }

    #[test]
    fn a_scene_with_no_camera_leaves_view_projection_identity() {
        // Covers the camera-absent branch: no camera command, identity VP, but
        // the renderable still draws.
        let api = RenderPipelineApi::new();
        let mut scene = SceneApi::new();
        let n = scene.create_node();
        let mesh = scene.mesh_ref(1);
        let material = scene.material_ref(2);
        scene.add_renderable(n, mesh, material).unwrap();
        scene
            .add_directional_light(&math(), n, Vec3::ONE, Ratio::new(1.0).unwrap())
            .unwrap();
        scene.update_world_transforms();

        let frame = frame_with_assets(&api);
        let webgpu = WebGpuApi::new_recording();
        let report = api.submit(&frame, &scene, &webgpu);

        assert_eq!(api.report_view_projection(&report), Mat4::IDENTITY);
        assert_eq!(api.report_draw_count(&report), 1);
    }
}
