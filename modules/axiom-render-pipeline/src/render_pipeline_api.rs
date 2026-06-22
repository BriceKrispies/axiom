//! The single public facade of the `axiom-render-pipeline` feature module.

use std::collections::HashMap;

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

/// Build the directional shadow caster's light view-projection from the sun's
/// world travel `direction`: an orthographic box of half-size [`SHADOW_EXTENT`]
/// looking from up-sun back at the origin, depth-corrected to wgpu's `[0,1]`
/// clip depth (the same `GL_TO_WGPU_DEPTH` fix the camera uses). `None` for a
/// degenerate (zero) direction — the caller substitutes identity, disabling
/// shadows. Branchless: the up vector is a table pick and the fallible matrix
/// steps are `Option` combinators.
fn shadow_light_view_proj(direction: Vec3) -> Option<Mat4> {
    /// Orthographic half-extent (world units) the shadow map covers around origin.
    const SHADOW_EXTENT: f32 = 20.0;
    /// Distance up-sun the shadow camera sits.
    const SHADOW_DISTANCE: f32 = 40.0;
    const NEAR: f32 = 0.1;
    const FAR: f32 = 100.0;

    let len = (direction.x * direction.x
        + direction.y * direction.y
        + direction.z * direction.z)
        .sqrt();
    let n = Vec3::new(direction.x / len, direction.y / len, direction.z / len);
    let eye = Vec3::new(
        -n.x * SHADOW_DISTANCE,
        -n.y * SHADOW_DISTANCE,
        -n.z * SHADOW_DISTANCE,
    );
    // A near-vertical sun would make the default up parallel to the view; pick a
    // sideways up in that case (table index, no branch).
    let up = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)][(n.y.abs() > 0.99) as usize];
    let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
    Mat4::look_at(eye, Vec3::ZERO, up).ok().and_then(|view| {
        Mat4::orthographic(
            -SHADOW_EXTENT,
            SHADOW_EXTENT,
            -SHADOW_EXTENT,
            SHADOW_EXTENT,
            NEAR,
            FAR,
        )
        .ok()
        .map(|proj| depth_fix.multiply(proj).multiply(view))
    })
}

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

/// One material asset supplied to a frame: a linear-RGBA base colour and an
/// opaque albedo texture id (`0` = untextured), keyed by the id the scene's
/// renderables reference.
#[derive(Debug)]
struct MaterialAsset {
    id: u64,
    color: [f32; 4],
    texture_id: u64,
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
/// clear colour, the wgpu-ready view-projection, one
/// `(world, colour, mesh_id, material_id)` per drawn object in submission order,
/// and the backend flags. The contract type is reached only through
/// [`RenderPipelineApi`] accessors.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderReport {
    command_count: usize,
    clear_color: [f32; 4],
    view_projection: Mat4,
    draws: Vec<(Mat4, [f32; 4], u64, u64)>,
    /// The frame's resolved lights: `(kind, vec, colour, intensity)` where
    /// `kind` is `0` directional / `1` point, and `vec` is the world-space
    /// to-light direction (directional) or the light's world position (point).
    lights: Vec<(u32, [f32; 3], [f32; 3], f32)>,
    /// The directional shadow caster's light view-projection (wgpu-ready): the
    /// backend renders the scene depth through this to build a shadow map and
    /// re-projects fragments into it for the PCF lookup. Identity when there is
    /// no directional light (shadows then have no effect).
    light_view_proj: Mat4,
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

    /// Register a material asset (base colour) for this frame. Untextured.
    pub fn frame_add_material(&self, frame: &mut RenderFrame, id: u64, color: [f32; 4]) {
        self.frame_add_textured_material(frame, id, color, 0);
    }

    /// Register a material asset with a base colour and an albedo texture id
    /// (`0` = untextured) for this frame.
    pub fn frame_add_textured_material(
        &self,
        frame: &mut RenderFrame,
        id: u64,
        color: [f32; 4],
        texture_id: u64,
    ) {
        frame.materials.push(MaterialAsset {
            id,
            color,
            texture_id,
        });
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
        // is reported for callers that build per-instance MVPs. `map_or`
        // collapses the present/absent arms into a single expression: absent
        // yields identity, present sets the camera command and returns the
        // depth-corrected view-projection.
        let view_projection = snapshot.cameras().first().map_or(Mat4::IDENTITY, |cam| {
            let cam_world = snapshot
                .node(cam.node())
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
            depth_fix.multiply(projection).multiply(view)
        });

        // Lights are resolved into the report below (a frame-uniform set), not
        // collapsed into one global direction: each scene light keeps its own kind
        // and geometry — a directional carries a world *direction*, a point its
        // node's world *position* — so the live backend lights the scene with all
        // of them. (The directional direction is still the frame's sun direction;
        // per-directional directions are a later scene-model extension.)

        // Meshes / materials: registration order defines the render-side index.
        // The id->index maps resolve each renderable's mesh/material in O(1)
        // (the lists carry no duplicate ids), and `material_color` lets the
        // per-draw pass below recover a command's colour without a scan.
        let mesh_index: HashMap<u64, u32> = frame
            .meshes
            .iter()
            .map(|mesh| {
                let idx = render.add_input_mesh(
                    &mut input,
                    mesh.id,
                    mesh.positions.clone(),
                    mesh.normals.clone(),
                    mesh.uvs.clone(),
                    mesh.indices.clone(),
                );
                (mesh.id, idx)
            })
            .collect();
        let mut material_color: HashMap<u64, [f32; 4]> =
            HashMap::with_capacity(frame.materials.len());
        let material_index: HashMap<u64, u32> = frame
            .materials
            .iter()
            .map(|material| {
                let c = material.color;
                let idx = render.add_input_textured_material(
                    &mut input,
                    material.id,
                    Vec4::new(c[0], c[1], c[2], c[3]),
                    material.texture_id,
                );
                material_color.insert(material.id, c);
                (material.id, idx)
            })
            .collect();

        // Objects: one per renderable, resolving its mesh/material ids to the
        // render-side indices. The frame must supply an asset for every id the
        // scene references.
        snapshot.renderables().iter().for_each(|renderable| {
            let world = snapshot
                .node(renderable.node())
                .expect("renderable node is present in the snapshot")
                .world()
                .to_matrix();
            let mesh_idx = mesh_index
                .get(&renderable.mesh().raw())
                .copied()
                .expect("frame supplies a mesh asset for every renderable");
            let material_idx = material_index
                .get(&renderable.material().raw())
                .copied()
                .expect("frame supplies a material asset for every renderable");
            render.add_input_object(
                &mut input,
                renderable.node().raw(),
                world,
                mesh_idx,
                material_idx,
                renderable.visible(),
            );
        });

        // ---- Compile and translate to a GPU submission. ----
        let commands = render.build_command_list(&input);
        let count = render.command_count(&commands);
        let mut submission = webgpu.new_submission(frame.width, frame.height);
        // Per-kind accessors return `Some` only for their command, so each arm
        // is exercised across a real command list — no unreachable catch-all.
        // `for_each` over the index range plus `Option::map` on each accessor
        // replaces the index `for` and the six `if let` guards branchlessly:
        // a `None` accessor maps to nothing, a `Some` applies its submission.
        (0..count).for_each(|i| {
            render
                .command_clear_color_at(&commands, i)
                .into_iter()
                .for_each(|c| webgpu.submission_clear_frame(&mut submission, c));
            render
                .command_camera_at(&commands, i)
                .into_iter()
                .for_each(|(v, p)| webgpu.submission_set_camera(&mut submission, v, p));
            render
                .command_pipeline_at(&commands, i)
                .into_iter()
                .for_each(|id| webgpu.submission_set_pipeline(&mut submission, id));
            render
                .command_mesh_id_at(&commands, i)
                .into_iter()
                .for_each(|id| webgpu.submission_set_mesh(&mut submission, id));
            render
                .command_material_id_at(&commands, i)
                .zip(render.command_material_texture_id_at(&commands, i))
                .into_iter()
                .for_each(|(id, tex)| webgpu.submission_set_material(&mut submission, id, tex));
            render
                .command_draw_indexed_at(&commands, i)
                .into_iter()
                .for_each(|(index_count, world)| {
                    webgpu.submission_draw_indexed(&mut submission, index_count, world)
                });
        });
        webgpu.submission_present(&mut submission);

        let clear_color = render
            .command_clear_color_at(&commands, 0)
            .unwrap_or([0.0; 4]);

        // Per-draw data: walk the command list once. Each material command sets
        // the colour and material id, and each mesh command the mesh id, that the
        // following draws use; each draw carries its world. All are state threaded
        // across commands, so a `fold` carries
        // `(current_color, current_mesh, current_material, draws)`: a material/mesh
        // command replaces its value (else keeps it via `map_or`/`unwrap_or`), a
        // draw command appends `(world, colour, mesh, material)`.
        let (_, _, _, draws): ([f32; 4], u64, u64, Vec<(Mat4, [f32; 4], u64, u64)>) = (0..count)
            .fold(
                ([1.0_f32; 4], 0_u64, 0_u64, Vec::new()),
                |(current_color, current_mesh, current_material, mut acc), i| {
                    let next_color = render
                        .command_material_id_at(&commands, i)
                        .map_or(current_color, |id| {
                            material_color.get(&id).copied().unwrap_or([1.0; 4])
                        });
                    let next_material = render
                        .command_material_id_at(&commands, i)
                        .unwrap_or(current_material);
                    let next_mesh = render
                        .command_mesh_id_at(&commands, i)
                        .unwrap_or(current_mesh);
                    render
                        .command_draw_indexed_at(&commands, i)
                        .into_iter()
                        .for_each(|(_, world)| {
                            acc.push((world, next_color, next_mesh, next_material))
                        });
                    (next_color, next_mesh, next_material, acc)
                },
            );

        // Resolve the frame's lights: directional → world to-light direction
        // (`-sun`, normalised in the shader); point → its node's world position.
        // `is_point()` (a bool) selects the geometry branchlessly via a 2-entry
        // table; colour/intensity carry through unchanged.
        let sun = frame.light_direction;
        let to_sun = [-sun.x, -sun.y, -sun.z];
        let lights: Vec<(u32, [f32; 3], [f32; 3], f32)> = snapshot
            .lights()
            .iter()
            .map(|light| {
                let world_pos = snapshot
                    .node(light.node())
                    .map(|n| {
                        let m = n.world().to_matrix().as_cols_array();
                        [m[12], m[13], m[14]]
                    })
                    .unwrap_or([0.0, 0.0, 0.0]);
                let point = light.is_point();
                let vec = [to_sun, world_pos][point as usize];
                let c = light.color();
                (point as u32, vec, [c.x, c.y, c.z], light.intensity().get())
            })
            .collect();

        // The directional shadow caster's light view-projection (identity when
        // there is no usable sun direction → shadows are a no-op).
        let light_view_proj =
            shadow_light_view_proj(frame.light_direction).unwrap_or(Mat4::IDENTITY);

        let gpu_report = webgpu.submit(submission);
        RenderReport {
            command_count: gpu_report.submitted_command_count(),
            clear_color,
            view_projection,
            draws,
            lights,
            light_view_proj,
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
        report.draws.get(i).map(|(world, _, _, _)| *world)
    }

    /// The colour of the `i`-th drawn object, if present.
    pub fn report_draw_color(&self, report: &RenderReport, i: usize) -> Option<[f32; 4]> {
        report.draws.get(i).map(|(_, color, _, _)| *color)
    }

    /// The mesh id of the `i`-th drawn object, if present. Lets a caller group
    /// draws by mesh for per-mesh instance batching.
    pub fn report_draw_mesh_id(&self, report: &RenderReport, i: usize) -> Option<u64> {
        report.draws.get(i).map(|(_, _, mesh_id, _)| *mesh_id)
    }

    /// The material id of the `i`-th drawn object, if present. Lets a caller
    /// group draws by `(mesh, material)` and bind the matching texture.
    pub fn report_draw_material_id(&self, report: &RenderReport, i: usize) -> Option<u64> {
        report
            .draws
            .get(i)
            .map(|(_, _, _, material_id)| *material_id)
    }

    /// The directional shadow caster's wgpu-ready light view-projection
    /// (column-major, 16 floats). The backend renders a shadow map through this
    /// and re-projects fragments into it; identity disables shadows.
    pub fn report_light_view_proj(&self, report: &RenderReport) -> [f32; 16] {
        report.light_view_proj.as_cols_array()
    }

    /// How many lights this frame resolved.
    pub fn report_light_count(&self, report: &RenderReport) -> usize {
        report.lights.len()
    }

    /// The `i`-th resolved light: `(kind, vec, colour, intensity)` — `kind` is
    /// `0` directional / `1` point; `vec` is the world to-light direction
    /// (directional) or world position (point). `None` if `i` is out of range.
    pub fn report_light_at(
        &self,
        report: &RenderReport,
        i: usize,
    ) -> Option<(u32, [f32; 3], [f32; 3], f32)> {
        report.lights.get(i).copied()
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
        // The draw carries its mesh id (mesh 1 in the scene), for batching.
        assert_eq!(api.report_draw_mesh_id(&report, 0), Some(1));
        assert!(api.report_draw_mesh_id(&report, 9).is_none());
        // ...and its material id (material 2 in the scene), for texture binding.
        assert_eq!(api.report_draw_material_id(&report, 0), Some(2));
        assert!(api.report_draw_material_id(&report, 9).is_none());
        // The frame resolves its one directional light: kind 0, to-light dir is
        // the negated frame sun direction (0.3, -1.0, 0.4).
        assert_eq!(api.report_light_count(&report), 1);
        let (kind, vec, _color, _intensity) = api.report_light_at(&report, 0).unwrap();
        assert_eq!(kind, 0);
        assert_eq!(vec, [-0.3, 1.0, -0.4]);
        assert!(api.report_light_at(&report, 9).is_none());
        // A real camera makes the view-projection non-identity.
        assert_ne!(api.report_view_projection(&report), Mat4::IDENTITY);
        // The directional light yields a non-identity shadow light view-projection.
        assert_ne!(
            api.report_light_view_proj(&report),
            Mat4::IDENTITY.as_cols_array()
        );
        assert!(api.report_recorded(&report));
        assert!(!api.report_presented(&report));
    }

    #[test]
    fn shadow_light_view_proj_covers_tilted_vertical_and_degenerate_suns() {
        // A tilted sun builds a finite, non-identity light view-projection.
        let tilted = shadow_light_view_proj(Vec3::new(0.3, -1.0, 0.4)).unwrap();
        assert_ne!(tilted, Mat4::IDENTITY);
        // A near-vertical sun (|n.y| > 0.99) takes the sideways-up table arm and
        // still yields a valid matrix (look-at forward is not parallel to up).
        let vertical = shadow_light_view_proj(Vec3::new(0.0, -1.0, 0.0)).unwrap();
        assert_ne!(vertical, Mat4::IDENTITY);
        // A zero direction is degenerate (look-at eye == target) → None, so the
        // caller falls back to identity and shadows become a no-op.
        assert!(shadow_light_view_proj(Vec3::ZERO).is_none());
    }

    #[test]
    fn point_light_resolves_to_its_node_world_position() {
        // A point light on a translated node resolves to kind 1 with that node's
        // world position as its geometry vector (not the sun direction).
        let api = RenderPipelineApi::new();
        let mut scene = SceneApi::new();
        let n = scene.create_node();
        let mesh = scene.mesh_ref(1);
        let material = scene.material_ref(2);
        scene.add_renderable(n, mesh, material).unwrap();
        let light_node = scene.create_node_with_transform(axiom_math::Transform::from_translation(
            Vec3::new(2.0, 3.0, -4.0),
        ));
        scene
            .add_point_light(
                &math(),
                light_node,
                Vec3::new(1.0, 0.0, 0.0),
                Ratio::new(2.5).unwrap(),
            )
            .unwrap();
        scene.update_world_transforms();

        let frame = frame_with_assets(&api);
        let webgpu = WebGpuApi::new_recording();
        let report = api.submit(&frame, &scene, &webgpu);

        assert_eq!(api.report_light_count(&report), 1);
        let (kind, vec, color, intensity) = api.report_light_at(&report, 0).unwrap();
        assert_eq!(kind, 1);
        assert_eq!(vec, [2.0, 3.0, -4.0]);
        assert_eq!(color, [1.0, 0.0, 0.0]);
        assert_eq!(intensity, 2.5);
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
