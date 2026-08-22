//! The single public facade of the `axiom-render-pipeline` feature module.


use axiom_host::SdfScene;
use axiom_kernel::Ratio;
use axiom_math::{Mat4, MathApi, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;

use crate::shadow_view::{shadow_light_view_proj, shadow_volume, ORIGIN_VOLUME};

/// The report accessors — see `render_pipeline_api/report.rs`.
mod report;

/// Column-major matrix that remaps OpenGL clip depth `z' = (z + w) / 2` so the
/// engine's `[-1,1]` projection lands in wgpu's `[0,1]` clip space.
///
/// # M2 — the report bakes a backend convention (known, cross-stream follow-up)
/// The [`RenderReport`]'s `view_projection` and `light_view_proj` are still
/// pre-multiplied by this, even though the report feeds *both* the wgpu path and
/// the software Canvas2D path. That is the backend-neutrality break the
/// vertical-slice audit flags as M2. The **correct end-state** — proven by the
/// unified chain — is: keep the report (and the `GpuSubmission`, which already
/// carries the *raw* camera via `set_input_camera` below) backend-neutral, and
/// apply this remap **in the wgpu consumer**. `axiom-webgpu`'s live present does
/// exactly that (`GL_TO_WGPU_DEPTH` lives in its `live_present` module).
///
/// Fully removing the bake *here* is **not** a render-pipeline-local change: the
/// same convention threads through `axiom-render`'s `build_sdf_scene`
/// (`view_proj`/`inv_view_proj` — Stream A's contract), `axiom-gpu-backend`'s
/// mesh/shadow/SDF shaders (which today rely on the baked MVP), and
/// `axiom-canvas2d-backend`'s depth-cue, whose fog is tuned for the `[0,1]`
/// range it currently receives (`FogCue { near: 0.85, far: 1.0 }`). Neutralizing
/// this field without moving the convention into each of those consumers would
/// silently break Canvas2D fog and GPU depth compositing. It is a coordinated
/// follow-up (Stream A `axiom-render` + gpu-backend shaders + canvas2d), not a
/// local edit — see the report's field docs below.
pub(crate) const GL_TO_WGPU_DEPTH: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
];


/// One mesh asset referenced by a frame: the id an uploaded mesh was keyed
/// under, and how many indices a draw over it spans. Carries no geometry — see
/// `RenderPipelineApi::frame_add_mesh`.
#[derive(Debug)]
struct MeshAsset {
    id: u64,
    index_count: u32,
}

/// One material asset supplied to a frame: a linear-RGBA base colour, its
/// catalog surface — `emissive` self-illumination (linear RGB), `roughness`
/// (`0` mirror-smooth … `1` matte), `opacity` (`1` opaque; folded into the
/// per-draw alpha so a translucent material blends) — and an albedo texture id
/// (`0` = untextured), keyed by the id the scene's renderables reference. The
/// scalar catalog fields stay primitive `f32` at this boundary (matching the
/// `[f32; 4]` colour); `submit` sanitizes them into the render layer's `Ratio`.
///
/// `surface_program` is the appearance program the material names — the content
/// digest of an authored surface description, `0` for the built-in fixed
/// material path. Like the ids around it, it is transported, never interpreted.
#[derive(Debug)]
struct MaterialAsset {
    id: u64,
    color: [f32; 4],
    emissive: [f32; 3],
    roughness: f32,
    opacity: f32,
    texture_id: u64,
    surface_program: u64,
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
    /// The camera's **view** and **projection**, separately.
    ///
    /// Not derivable from [`Self::view_projection`]: a product cannot be split
    /// back into its factors, and a backend that works in *view space* needs
    /// both halves. Screen-space ambient occlusion is the first consumer — it
    /// reconstructs a view-space position from a linear depth, which takes the
    /// inverse projection, and scales a world radius to pixels, which takes
    /// `projection[5]`. Identity in a camera-less frame, like `view_projection`.
    ///
    /// `projection` is the RAW perspective, **without** the `GL_TO_WGPU_DEPTH`
    /// remap that `view_projection` bakes in — a consumer inverting it wants the
    /// projection the depth in a G-buffer was actually produced from.
    view: Mat4,
    projection: Mat4,
    /// One `(world, colour, emissive, specular, mesh_id, material_id,
    /// surface_program, casts_contact_shadow)` per drawn (visible) object, in
    /// submission order. The caster flag is the scene's per-renderable
    /// contact-shadow mark, carried so a grounding backend (the software canvas)
    /// knows which objects to shadow; `surface_program` is the material's
    /// appearance program (`0` = the built-in fixed material path).
    #[allow(clippy::type_complexity)]
    draws: Vec<(Mat4, [f32; 4], [f32; 3], f32, u64, u64, u64, bool)>,
    /// The frame's resolved lights: `(kind, vec, colour, intensity)` where
    /// `kind` is `0` directional / `1` point, and `vec` is the world-space
    /// to-light direction (directional) or the light's world position (point).
    lights: Vec<(u32, [f32; 3], [f32; 3], f32)>,
    /// The directional shadow caster's light view-projection (wgpu-ready): the
    /// backend renders the scene depth through this to build a shadow map and
    /// re-projects fragments into it for the PCF lookup. Identity when there is
    /// no directional light (shadows then have no effect).
    light_view_proj: Mat4,
    /// The frame's backend-neutral SDF scene, if it carries any SDF shapes and a
    /// camera. Assembled by `axiom-render` from the snapshot's SDF shapes and the
    /// same wgpu-ready view-projection the meshes use, so a backend that marches
    /// it composites the result against the rasterized meshes. `None` otherwise.
    sdf: Option<SdfScene>,
    presented: bool,
    recorded: bool,
}

/// The only public export of `axiom-render-pipeline`: the per-frame render
/// pipeline that composes scene + render + webgpu.
#[derive(Debug, Clone, Default)]
pub struct RenderPipelineApi {
    /// A retained [`RenderApi`] (its render input + command-list scratch) so the
    /// pipeline reuses those buffers across frames instead of allocating fresh
    /// each frame — the per-frame wasm-memory-churn fix. (This is why the facade
    /// is no longer `Copy`.)
    render: RenderApi,
    /// The frame's mesh-id → render-index, RETAINED and refilled each frame.
    ///
    /// Indexed by id, not hashed by it. Asset ids are minted as `len() + 1`, so
    /// they are small dense integers and a plain vector is the natural map —
    /// a lookup becomes one load, and the per-frame reset becomes a memset.
    ///
    /// This was five `HashMap`s until 2026-08-11. A throttled profile put ~10%
    /// of the frame in `hashbrown`'s bucket walk, most of it inside `clear()`:
    /// emptying five maps of ~1000 entries every frame costs more than the
    /// lookups they exist to serve.
    mesh_index: Vec<u32>,
    /// material id → everything the per-draw pass needs, in ONE entry rather
    /// than the four parallel maps this replaced.
    materials: Vec<MaterialSlot>,
}

/// The index of a mesh or material the frame did not supply.
const ABSENT: u32 = u32::MAX;

/// One material's per-frame render data, resolved once and read per draw.
///
/// The four values used to live in four separate maps keyed by the same id, so
/// assembling one material for one draw cost four independent lookups. They
/// travel together, so they are stored together.
///
/// [`MaterialSlot::MISSING`] doubles as the *fallback*: the vector is refilled
/// with it each frame, so a renderable whose material the frame never supplied
/// reads flat-white-and-matte by construction — the absent case is data, not a
/// branch, and matches the fallback the four `unwrap_or`s used to spell out.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MaterialSlot {
    index: u32,
    color: [f32; 4],
    emissive: [f32; 3],
    specular: f32,
    surface_program: u64,
}

impl MaterialSlot {
    const MISSING: MaterialSlot = MaterialSlot {
        index: ABSENT,
        color: [1.0; 4],
        emissive: [0.0; 3],
        specular: 0.0,
        surface_program: 0,
    };
}

/// Reset an id-indexed table to `fill`, sized to hold every id in `ids`.
///
/// `clear` then `resize` rather than a per-entry walk: the reset is a memset
/// over one contiguous allocation, and the capacity is retained across frames.
fn reset_table<T: Clone>(table: &mut Vec<T>, ids: impl Iterator<Item = u64>, fill: T) {
    let needed = ids.map(|id| id as usize + 1).max().unwrap_or(0);
    table.clear();
    table.resize(needed, fill);
}

impl RenderPipelineApi {
    /// Construct the facade.
    pub fn new() -> Self {
        RenderPipelineApi {
            render: RenderApi::new(),
            mesh_index: Vec::new(),
            materials: Vec::new(),
        }
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

    /// Reference an uploaded mesh for this frame: its `id` and the
    /// `index_count` a draw over it spans.
    ///
    /// **Identity, not geometry.** Mesh vertex data is bind-time resident state —
    /// uploaded to the backend once when the surface binds, never re-sent — so a
    /// frame packet naming it needs only the id the backend keyed it under. This
    /// used to take the four vertex arrays by value, which meant a caller had to
    /// clone every registered mesh's geometry into every frame, and `submit` then
    /// cloned it again into the render input. For a scene with ~1,000 registered
    /// meshes that measured as about a third of the browser's main thread, all of
    /// it to carry two scalars. See `axiom_render::RenderMesh`.
    pub fn frame_add_mesh(&self, frame: &mut RenderFrame, id: u64, index_count: u32) {
        frame.meshes.push(MeshAsset { id, index_count });
    }

    /// Register a material asset (base colour) for this frame. Untextured, no
    /// emissive, fully matte, fully opaque.
    pub fn frame_add_material(&self, frame: &mut RenderFrame, id: u64, color: [f32; 4]) {
        self.frame_add_textured_material(frame, id, color, 0);
    }

    /// Register a material asset with a base colour and an albedo texture id
    /// (`0` = untextured) for this frame, with default catalog fields (no
    /// emissive, fully matte, fully opaque).
    pub fn frame_add_textured_material(
        &self,
        frame: &mut RenderFrame,
        id: u64,
        color: [f32; 4],
        texture_id: u64,
    ) {
        let one = Ratio::finite_or_zero(1.0);
        self.frame_add_lit_material(frame, id, color, [0.0; 3], one, one, texture_id, 0);
    }

    /// Register a fully-specified lit material asset for this frame: `color`
    /// base, `emissive` self-illumination (linear RGB), `roughness`, `opacity`
    /// (`1` opaque — folded into the per-draw alpha so a translucent material
    /// blends), an albedo `texture_id` (`0` = untextured), and the
    /// `surface_program` it names (`0` = the engine's built-in fixed material
    /// path). This is the boundary that threads the umbrella `Material`'s full
    /// catalog surface to the renderer, no longer dropping emissive / roughness
    /// / opacity.
    #[allow(clippy::too_many_arguments)]
    pub fn frame_add_lit_material(
        &self,
        frame: &mut RenderFrame,
        id: u64,
        color: [f32; 4],
        emissive: [f32; 3],
        roughness: Ratio,
        opacity: Ratio,
        texture_id: u64,
        surface_program: u64,
    ) {
        frame.materials.push(MaterialAsset {
            id,
            color,
            emissive,
            roughness: roughness.get(),
            opacity: opacity.get(),
            texture_id,
            surface_program,
        });
    }

    /// Render `scene` for this frame: translate its snapshot + the frame's
    /// assets into render commands, submit them through `webgpu`, and return the
    /// deterministic report. `scene` is expected to have been advanced for the
    /// frame already.
    pub fn submit(
        &mut self,
        frame: &RenderFrame,
        scene: &mut SceneApi,
        webgpu: &mut WebGpuApi,
    ) -> RenderReport {
        let math = MathApi::new();
        // Refresh the retained snapshot in place (reusing its buffers, no fresh
        // allocation) then read it by reference — self-contained, so a frame that
        // steps then renders builds exactly one snapshot with zero churn.
        scene.refresh_snapshot();
        let snapshot = scene.snapshot_ref();

        // Destructure the retained buffers into disjoint field borrows so the
        // render input and the id-index maps can be reset + refilled together in
        // one scope (all reused across frames — the churn fix).
        let Self {
            render,
            mesh_index,
            materials,
        } = self;
        // Fill the RETAINED render input (reset + refill, no fresh alloc) via its
        // public primitive builders on the `&mut` handle — this module never names
        // the opaque RenderInput/mesh/material/object types.
        let input = render.reset_input(frame.width, frame.height);
        input.set_clear_color(frame.clear_color);

        // Camera: the first camera, if any. view = inverse(node world);
        // projection from validated intrinsics. The GpuSubmission camera command
        // carries the *raw* view/projection (neutral); only the reported
        // `view_projection` bakes the wgpu depth remap (`GL_TO_WGPU_DEPTH` M2
        // note). `map_or` collapses present/absent into one expression: absent
        // yields identity; present sets the camera command + returns the VP.
        let camera = snapshot.cameras().first().map(|cam| {
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
            let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
            let view_projection = depth_fix.multiply(projection).multiply(view);
            // The whole world transform travels, not just the position: the SDF
            // marcher wants the position.
            //
            // The shadow volume is fitted HERE rather than downstream because
            // this is the only place the camera's *intrinsics* are in scope —
            // the fit needs the fov and aspect, not just the pose, and a volume
            // sized without them is the fixed-cube defect `shadow_view` documents.
            let shadow = shadow_volume(
                cam_world,
                cam.fovy_radians().get(),
                cam.aspect().get(),
                cam.near().get(),
            );
            (view, projection, view_projection, cam_world, shadow)
        });
        // Set the input camera and read the wgpu-ready view-projection (0-or-1
        // over the Option — no branch; absent yields identity, no camera command).
        camera
            .iter()
            .for_each(|&(view, projection, _, _, _)| input.push_camera(view, projection));
        let view_projection = camera.map_or(Mat4::IDENTITY, |(_, _, vp, _, _)| vp);

        // Lights are resolved into the report below (a frame-uniform set), not
        // collapsed into one global direction: each scene light keeps its own kind
        // and geometry — a directional carries a world *direction*, a point its
        // node's world *position* — so the live backend lights the scene with all
        // of them. (The directional direction is still the frame's sun direction;
        // per-directional directions are a later scene-model extension.)

        // Meshes / materials: registration order defines the render-side index.
        // The RETAINED id->index maps resolve each renderable's mesh/material in
        // O(1) (the lists carry no duplicate ids), and `material_color` lets the
        // per-draw pass below recover a command's colour without a scan. Cleared +
        // refilled each frame (not rebuilt) so no fresh hashmap is allocated.
        reset_table(mesh_index, frame.meshes.iter().map(|m| m.id), ABSENT);
        frame.meshes.iter().for_each(|mesh| {
            let idx = input.push_mesh(mesh.id, mesh.index_count);
            mesh_index[mesh.id as usize] = idx;
        });
        reset_table(
            materials,
            frame.materials.iter().map(|m| m.id),
            MaterialSlot::MISSING,
        );
        frame.materials.iter().for_each(|material| {
            let c = material.color;
            let e = material.emissive;
            let idx = input.push_lit_material(
                material.id,
                Vec4::new(c[0], c[1], c[2], c[3]),
                Vec3::new(e[0], e[1], e[2]),
                Ratio::finite_or_zero(material.roughness),
                Ratio::finite_or_zero(material.opacity),
                material.texture_id,
                material.surface_program,
            );
            // Fold opacity into the per-draw alpha (`alpha = base.a × opacity`),
            // exactly as the render layer's neutral packet does, so the report's
            // per-draw colour — and the live/canvas instance colour built from it
            // — carries the translucency a `createMaterial`-authored material set.
            let color = [c[0], c[1], c[2], c[3] * material.opacity];
            // Emissive rides the report as its OWN per-draw term rather than being
            // folded into the colour like opacity is. Opacity may fold because alpha
            // is not light-modulated; emissive may not, because the colour is a
            // reflectance every backend multiplies by N·L, ambient and shadow —
            // folding self-illumination in there would make a tail light dim when it
            // faces away from the sun, which is exactly the bug this route removes.
            // (Recorded on the slot below, alongside the colour and specular.)
            // Specular strength IS the authored roughness, inverted.
            //
            // `roughness` has been on every material since the catalog existed —
            // documented as "0 mirror-smooth … 1 matte" — and was then thrown
            // away: it reached the render layer and no backend ever read it,
            // because the shading model was Lambert-only and had nothing to spend
            // it on. So there is no new authoring knob here and no migration; the
            // engine simply stops discarding a value apps were already setting,
            // and every material in every existing app becomes as glossy as it
            // always said it was.
            materials[material.id as usize] = MaterialSlot {
                index: idx,
                color,
                emissive: e,
                specular: 1.0 - material.roughness.clamp(0.0, 1.0),
                surface_program: material.surface_program,
            };
        });

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
                .get(renderable.mesh().raw() as usize)
                .copied()
                .filter(|&index| index != ABSENT)
                .expect("frame supplies a mesh asset for every renderable");
            let material_idx = materials
                .get(renderable.material().raw() as usize)
                .map(|slot| slot.index)
                .filter(|&index| index != ABSENT)
                .expect("frame supplies a material asset for every renderable");
            input.push_object(
                renderable.node().raw(),
                world,
                mesh_idx,
                material_idx,
                renderable.visible(),
            );
        });

        // `input`'s borrow of `render` ends above; build the RETAINED command
        // list from it and read it by reference — no fresh command-list alloc.
        render.build_commands();
        let commands = render.commands_ref();
        let count = commands.len();
        // Fill webgpu's RETAINED submission (reset + refill, no fresh alloc). The
        // `sub` handle borrows `webgpu` mutably only for this fill; its per-kind
        // helpers are called via inference so this module never names the (opaque)
        // GpuSubmission. `for_each` + `Option::map` keeps every arm branchless.
        let sub = webgpu.submission_reset(frame.width, frame.height);
        (0..count).for_each(|i| {
            render
                .command_clear_color_at(commands, i)
                .into_iter()
                .for_each(|c| sub.clear_frame(c));
            render
                .command_camera_at(commands, i)
                .into_iter()
                .for_each(|(v, p)| sub.set_camera(v, p));
            render
                .command_pipeline_at(commands, i)
                .into_iter()
                .for_each(|id| sub.set_pipeline(id));
            render
                .command_mesh_id_at(commands, i)
                .into_iter()
                .for_each(|id| sub.set_mesh(id));
            render
                .command_material_id_at(commands, i)
                .zip(render.command_material_texture_id_at(commands, i))
                .into_iter()
                .for_each(|(id, tex)| sub.set_material(id, tex));
            render
                .command_draw_indexed_at(commands, i)
                .into_iter()
                .for_each(|(index_count, world)| sub.draw_indexed(index_count, world));
        });
        sub.present();

        let clear_color = render
            .command_clear_color_at(commands, 0)
            .unwrap_or([0.0; 4]);

        // Per-draw data: one entry per *visible* renderable, in snapshot order —
        // the same set and order the command list draws (a visible renderable
        // always resolves its mesh/material above, so it always produces a draw).
        // Sourcing it straight from the snapshot (rather than re-folding the GPU
        // command stream) is what lets each draw carry the renderable's
        // `casts_contact_shadow` mark, which the command stream does not encode.
        #[allow(clippy::type_complexity)]
        let draws: Vec<(Mat4, [f32; 4], [f32; 3], f32, u64, u64, u64, bool)> = snapshot
            .renderables()
            .iter()
            .filter(|renderable| renderable.visible())
            .map(|renderable| {
                let world = snapshot
                    .node(renderable.node())
                    .expect("renderable node is present in the snapshot")
                    .world()
                    .to_matrix();
                let mesh_id = renderable.mesh().raw();
                let material_id = renderable.material().raw();
                // One lookup for all three. A renderable whose material the frame
                // never supplied reads `MISSING` — flat white, unlit, fully matte
                // — because that is what the table was refilled with.
                let slot = materials
                    .get(material_id as usize)
                    .copied()
                    .unwrap_or(MaterialSlot::MISSING);
                (
                    world,
                    slot.color,
                    slot.emissive,
                    slot.specular,
                    mesh_id,
                    material_id,
                    slot.surface_program,
                    renderable.casts_contact_shadow(),
                )
            })
            .collect();

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
        // there is no usable sun direction → shadows are a no-op). Its volume is
        // the bounding sphere of this frame's own view frustum, so the map covers
        // the action wherever in the world it happens AND at whatever field of
        // view and aspect the frame is actually being watched at; a camera-less
        // scene has no view to fit to and keeps the origin (`map_or`, no branch).
        let volume = camera.map_or(ORIGIN_VOLUME, |(_, _, _, _, v)| v);
        let light_view_proj =
            shadow_light_view_proj(frame.light_direction, volume).unwrap_or(Mat4::IDENTITY);

        // SDF shapes: translate each into the backend-neutral scene the live /
        // canvas path marches, reusing render's shared SDF-scene assembly. Built
        // only with a camera (the marcher needs the inverse view-projection), and
        // from the *same* wgpu-ready view-projection the meshes use, so the
        // marched SDF depth composites correctly against the rasterized meshes.
        let sdf = camera.and_then(|(_, _, view_proj, cam_world, _)| {
            let shapes: Vec<(u32, Mat4, Vec3, Vec4)> = snapshot
                .sdf_shapes()
                .iter()
                .map(|shape| {
                    let world = snapshot
                        .node(shape.node())
                        .expect("sdf shape node is present in the snapshot")
                        .world()
                        .to_matrix();
                    let c = shape.color();
                    (
                        shape.kind(),
                        world,
                        shape.dims(),
                        Vec4::new(c.x, c.y, c.z, 1.0),
                    )
                })
                .collect();
            render
                .build_sdf_scene(view_proj, cam_world.translation, &shapes)
        });

        // Summarise the retained submission clone-free — the report only needs
        // the command count + flags, never the commands themselves.
        let (command_count, presented, recorded) = webgpu.submit_summary();
        RenderReport {
            command_count,
            clear_color,
            view_projection,
            view: camera.map_or(Mat4::IDENTITY, |(v, ..)| v),
            projection: camera.map_or(Mat4::IDENTITY, |(_, p, ..)| p),
            draws,
            lights,
            light_view_proj,
            sdf,
            presented,
            recorded,
        }
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
        api.frame_add_mesh(&mut frame, 1, 36);
        api.frame_add_material(&mut frame, 2, [0.8, 0.4, 0.2, 1.0]);
        frame
    }

    #[test]
    fn new_and_default_are_equivalent() {
        let mut scene = cube_scene();
        let mut webgpu = WebGpuApi::new_recording();
        let mut n = RenderPipelineApi::new();
        let mut d = RenderPipelineApi::default();
        let rn = n.submit(&frame_with_assets(&n), &mut scene, &mut webgpu);
        let rd = d.submit(&frame_with_assets(&d), &mut scene, &mut webgpu);
        assert_eq!(n.report_command_count(&rn), d.report_command_count(&rd));
        assert!(n.report_sdf_scene(&rn).is_none());
    }

    #[test]
    fn report_carries_an_sdf_scene_for_a_scene_with_an_sdf_shape() {
        use axiom_math::Transform;
        let mut api = RenderPipelineApi::new();
        let mut scene = SceneApi::new();
        // An SDF sphere placed by a translation-only world transform (scale 1),
        // plus a camera (required for the SDF scene's rays). No renderables, so
        // the frame needs no mesh/material assets.
        let shape_node =
            scene.create_node_with_transform(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        scene
            .add_sdf_sphere(
                &math(),
                shape_node,
                Meters::new(0.5).unwrap(),
                Vec3::new(1.0, 0.0, 0.0),
            )
            .unwrap();
        let camera =
            scene.create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)));
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
        scene.update_world_transforms();

        let mut webgpu = WebGpuApi::new_recording();
        let frame = api.new_frame(800, 600, [0.0, 0.0, 0.0, 1.0], Vec3::new(0.0, -1.0, 0.0));
        let report = api.submit(&frame, &mut scene, &mut webgpu);
        let sdf = api
            .report_sdf_scene(&report)
            .expect("the scene's SDF shape yields a scene");
        assert_eq!(sdf.primitives().len(), 1);
        let p = sdf.primitives()[0];
        assert_eq!(p.kind(), 0); // sphere
                                 // dims (0.5, 0.5, 0.5) carried; translation-only world → uniform scale 1.
        assert_eq!(p.params(), [0.5, 0.5, 0.5, 1.0]);
        // The scene's RGB rides through with an opaque alpha synthesized.
        assert_eq!(p.color(), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn caster_mark_is_reported_and_invisible_renderables_are_filtered() {
        use axiom_math::Transform;
        let mut api = RenderPipelineApi::new();
        let mut scene = SceneApi::new();
        let mesh = scene.mesh_ref(1);
        let material = scene.material_ref(2);
        let caster =
            scene.create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)));
        scene.add_renderable(caster, mesh, material).unwrap();
        scene
            .set_renderable_casts_contact_shadow(caster, true)
            .unwrap();
        // An invisible renderable: still gets a render object (and so needs assets)
        // but contributes no draw, so it never reaches the per-draw caster list.
        let hidden = scene.create_node();
        scene.add_renderable(hidden, mesh, material).unwrap();
        scene.set_renderable_visibility(hidden, false).unwrap();

        let camera =
            scene.create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)));
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
        scene.update_world_transforms();

        let report = api.submit(
            &frame_with_assets(&api),
            &mut scene,
            &mut WebGpuApi::new_recording(),
        );
        assert_eq!(api.report_draw_count(&report), 1);
        assert_eq!(api.report_draw_casts_shadow(&report, 0), Some(true));
    }

    #[test]
    fn renders_a_scene_to_a_recording_submission() {
        let mut api = RenderPipelineApi::new();
        let mut scene = cube_scene();
        let frame = frame_with_assets(&api);
        let mut webgpu = WebGpuApi::new_recording();

        let report = api.submit(&frame, &mut scene, &mut webgpu);

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
        assert_eq!(api.report_draw_mesh_id(&report, 0), Some(1));
        assert!(api.report_draw_mesh_id(&report, 9).is_none());
        assert_eq!(api.report_draw_material_id(&report, 0), Some(2));
        assert!(api.report_draw_material_id(&report, 9).is_none());
        assert_eq!(api.report_draw_casts_shadow(&report, 0), Some(false));
        assert!(api.report_draw_casts_shadow(&report, 9).is_none());
        // The frame resolves its one directional light: kind 0, to-light dir is
        // the negated frame sun direction (0.3, -1.0, 0.4).
        assert_eq!(api.report_light_count(&report), 1);
        let (kind, vec, _color, _intensity) = api.report_light_at(&report, 0).unwrap();
        assert_eq!(kind, 0);
        assert_eq!(vec, [-0.3, 1.0, -0.4]);
        assert!(api.report_light_at(&report, 9).is_none());
        assert_ne!(api.report_view_projection(&report), Mat4::IDENTITY);
        assert_ne!(
            api.report_light_view_proj(&report),
            Mat4::IDENTITY.as_cols_array()
        );
        assert!(api.report_recorded(&report));
        assert!(!api.report_presented(&report));
    }


    #[test]
    fn point_light_resolves_to_its_node_world_position() {
        // A point light on a translated node resolves to kind 1 with that node's
        // world position as its geometry vector (not the sun direction).
        let mut api = RenderPipelineApi::new();
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
        let mut webgpu = WebGpuApi::new_recording();
        let report = api.submit(&frame, &mut scene, &mut webgpu);

        assert_eq!(api.report_light_count(&report), 1);
        let (kind, vec, color, intensity) = api.report_light_at(&report, 0).unwrap();
        assert_eq!(kind, 1);
        assert_eq!(vec, [2.0, 3.0, -4.0]);
        assert_eq!(color, [1.0, 0.0, 0.0]);
        assert_eq!(intensity, 2.5);
    }

    #[test]
    fn frame_add_lit_material_threads_opacity_into_the_report_draw_alpha() {
        // SPEC-11 §3.4: a fully-specified lit material (emissive + roughness +
        // opacity) authored through `frame_add_lit_material` reaches the report's
        // per-draw colour with its opacity folded into the alpha — the boundary the
        // umbrella `Material → asset` path now threads instead of dropping.
        let mut api = RenderPipelineApi::new();
        let mut scene = cube_scene();
        let mut webgpu = WebGpuApi::new_recording();
        let mut frame = api.new_frame(800, 600, [0.0, 0.0, 0.0, 1.0], Vec3::new(0.3, -1.0, 0.4));
        api.frame_add_mesh(&mut frame, 1, 36);
        // The scene's material id 2, authored translucent (opacity 0.5) + emissive
        // + rough. Base alpha 1.0 × opacity 0.5 ⇒ a report draw alpha of 0.5.
        api.frame_add_lit_material(
            &mut frame,
            2,
            [0.2, 0.4, 0.8, 1.0],
            [0.5, 0.0, 0.0],
            Ratio::finite_or_zero(0.25),
            Ratio::finite_or_zero(0.5),
            0,
            0,
        );
        let report = api.submit(&frame, &mut scene, &mut webgpu);
        assert_eq!(
            api.report_draw_color(&report, 0),
            Some([0.2, 0.4, 0.8, 0.5])
        );
        // …and its emissive reaches the report as its OWN term, unmultiplied by
        // the colour and unclamped by the alpha fold — the second half of the same
        // catalog surface, which used to die here.
        assert_eq!(api.report_draw_emissive(&report, 0), Some([0.5, 0.0, 0.0]));
        assert_eq!(api.report_draw_emissive(&report, 9), None);
        // …and so does the third per-draw term. This frame already authored a
        // roughness of 0.25 and nothing ever read it back, which is how the
        // accessor reached `main` uncovered: the material carried the value, the
        // report carried it, and no test asked for it. Specular is the roughness
        // inverted, so 0.25 rough is 0.75 smooth.
        assert_eq!(api.report_draw_specular(&report, 0), Some(0.75));
        assert_eq!(api.report_draw_specular(&report, 9), None);
    }

    /// **Seam 3 of 4 — the composition tier.** The appearance program has to
    /// survive `MaterialAsset` → `MaterialSlot` → the report's per-draw record.
    /// It is a separate seam from the colour because the slot table is refilled
    /// per frame from a *fallback*, and a field added to the asset but forgotten
    /// on the fallback would read as "built-in" for the first frame only.
    #[test]
    fn frame_add_lit_material_threads_the_surface_program_to_the_report_draw() {
        const PROGRAM: u64 = 0x00C0_FFEE_0BAD_F00D;
        let mut api = RenderPipelineApi::new();
        let mut scene = cube_scene();
        let mut webgpu = WebGpuApi::new_recording();
        let mut frame = api.new_frame(800, 600, [0.0; 4], Vec3::new(0.3, -1.0, 0.4));
        api.frame_add_mesh(&mut frame, 1, 36);
        let one = Ratio::finite_or_zero(1.0);
        api.frame_add_lit_material(&mut frame, 2, [1.0; 4], [0.0; 3], one, one, 0, PROGRAM);
        let report = api.submit(&frame, &mut scene, &mut webgpu);
        assert_eq!(api.report_draw_surface_program(&report, 0), Some(PROGRAM));
        assert_eq!(api.report_draw_surface_program(&report, 9), None);
        // The same pipeline reused for a frame whose material names no program
        // reports the built-in path, so the retained slot table cannot leak the
        // previous frame's program.
        let plain = frame_with_assets(&api);
        let report = api.submit(&plain, &mut scene, &mut webgpu);
        assert_eq!(api.report_draw_surface_program(&report, 0), Some(0));
    }

    #[test]
    fn a_material_with_no_authored_emissive_reports_zero_self_illumination() {
        // The default catalog surface is non-emissive, so every pre-existing frame
        // carries `[0, 0, 0]` — an exact no-op through both backends.
        let mut api = RenderPipelineApi::new();
        let mut scene = cube_scene();
        let mut webgpu = WebGpuApi::new_recording();
        let report = api.submit(&frame_with_assets(&api), &mut scene, &mut webgpu);
        assert_eq!(api.report_draw_emissive(&report, 0), Some([0.0; 3]));
    }

    #[test]
    fn deterministic_for_identical_input() {
        let mut api = RenderPipelineApi::new();
        let mut webgpu = WebGpuApi::new_recording();
        let a = api.submit(&frame_with_assets(&api), &mut cube_scene(), &mut webgpu);
        let b = api.submit(&frame_with_assets(&api), &mut cube_scene(), &mut webgpu);
        assert_eq!(a, b);
    }

    #[test]
    fn a_scene_with_no_camera_leaves_view_projection_identity() {
        // Covers the camera-absent branch: no camera command, identity VP, but
        // the renderable still draws.
        let mut api = RenderPipelineApi::new();
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
        let mut webgpu = WebGpuApi::new_recording();
        let report = api.submit(&frame, &mut scene, &mut webgpu);

        assert_eq!(api.report_view_projection(&report), Mat4::IDENTITY);
        assert_eq!(api.report_draw_count(&report), 1);
    }
}
