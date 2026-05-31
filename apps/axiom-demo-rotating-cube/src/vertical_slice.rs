//! The headless deterministic vertical slice: the per-tick orchestrator
//! and its single inspectable output, [`VerticalSliceArtifact`].
//!
//! This module owns the cross-module *plumbing*. Each module's contract
//! value (`SceneSnapshot`, `ResolvedResources`, `RenderInput`,
//! `RenderCommandList`, `GpuSubmission`, `GpuSubmissionReport`) is produced
//! and consumed through its facade here, because none of those types are
//! nameable outside their module — every module re-exports exactly one
//! facade. The orchestrator only does two mechanical jobs:
//!
//! 1. read each un-nameable producer value into a plain-data artifact, and
//! 2. replay a plain-data plan back into the next module's facade.
//!
//! Both jobs must live in one function because a helper would have to name
//! the un-nameable contract type in its signature. The *semantic*
//! translation between modules — the part that is nameable and testable —
//! lives in [`crate::scene_to_render_input`] and
//! [`crate::render_to_gpu_submission`].

use axiom_ecs::World;
use axiom_host::HostFrameInput;
use axiom_math::{Quat, Transform, Vec2, Vec3, Vec4};
use axiom_render::RenderApi;

use crate::cube_world::{
    world_to_scene_snapshot, CameraData, CubeComponents, LightData, RenderableData,
    TransformPropagation,
};
use crate::demo_api::{DemoRotatingCubeApi, FIXED_STEP_NANOS};
use crate::render_to_gpu_submission::{
    render_command_list_to_gpu_submission, GpuCommandArtifact, RenderCommandArtifact,
    RenderCommandListArtifact,
};
use crate::scene_to_render_input::{
    scene_to_render_input, ResolvedMaterialArtifact, ResolvedMeshArtifact,
    ResolvedResourcesArtifact, SceneSnapshotArtifact, DEMO_CUBE_BASE_COLOR, DEMO_LIGHT_COLOR,
    DEMO_LIGHT_INTENSITY, VIEWPORT_HEIGHT, VIEWPORT_WIDTH,
};

// Re-exported artifact types are reachable through the facade's return
// value; see `lib.rs`.
pub use crate::render_to_gpu_submission::GpuSubmissionArtifact;
pub use crate::scene_to_render_input::RenderInputArtifact;

/// The stable identity of the cube within the demo scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubeIdentityArtifact {
    /// The scene node id of the renderable cube (the rotating parent's child).
    pub node_id: u64,
    /// The resolved cube mesh resource id.
    pub mesh_id: u64,
    /// The resolved basic-lit material resource id.
    pub material_id: u64,
}

/// The cube's local and world transforms for the tick.
///
/// The cube's local transform is identity; its world transform is driven
/// by the rotating parent node, so it differs from tick to tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubeTransformArtifact {
    pub local: Transform,
    pub world: Transform,
}

/// Plain-data mirror of `axiom_webgpu::GpuSubmissionReport` — the final
/// boundary value.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSubmissionReportArtifact {
    pub target_width: u32,
    pub target_height: u32,
    pub command_count: usize,
    /// Every submitted command's kind code (see `WebGpuApi::KIND_*`).
    pub command_kinds: Vec<u32>,
    pub clear_count: u32,
    pub draw_count: u32,
    pub present_count: u32,
}

/// The single inspectable artifact produced by one headless tick. Every
/// boundary in the vertical slice is captured as a plain-data child
/// artifact, so two runs of the same tick compare equal field-for-field.
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalSliceArtifact {
    /// The demo tick this artifact was produced for.
    pub tick: u64,
    /// The engine frame index (monotonic across ticks).
    pub engine_frame_index: u64,
    /// The host frame sequence (monotonic across ticks).
    pub host_frame_sequence: u64,
    /// Runtime steps executed for this frame (1 at the demo's fixed step).
    pub runtime_step_count: u32,

    /// The cube's stable scene/resource identity.
    pub cube: CubeIdentityArtifact,
    /// The cube's local + world transform for this tick.
    pub cube_transform: CubeTransformArtifact,

    /// Boundary 1 — scene snapshot.
    pub scene_snapshot: SceneSnapshotArtifact,
    /// Boundary 2 — resolved resources.
    pub resolved_resources: ResolvedResourcesArtifact,
    /// Boundary 3 — neutral render input (app-translated).
    pub render_input: RenderInputArtifact,
    /// Boundary 4 — render command list (built by `axiom-render`).
    pub render_command_list: RenderCommandListArtifact,
    /// Boundary 5 — GPU submission (app-translated).
    pub gpu_submission: GpuSubmissionArtifact,
    /// Boundary 6 — GPU submission report (built by `axiom-webgpu`).
    pub gpu_submission_report: GpuSubmissionReportArtifact,
}

/// Run the full headless vertical slice for one deterministic tick.
///
/// The whole pipeline lives in one function on purpose: the module
/// contract values it threads (`SceneSnapshot`, `ResolvedResources`,
/// `RenderInput`, `RenderCommandList`, `GpuSubmission`,
/// `GpuSubmissionReport`) cannot be named outside their owning module, so
/// they exist here only as type-inferred locals. The nameable, testable
/// translation steps are delegated to `scene_to_render_input` and
/// `render_command_list_to_gpu_submission`.
pub(crate) fn run_vertical_slice(
    api: &mut DemoRotatingCubeApi,
    tick: u64,
) -> VerticalSliceArtifact {
    // ---- 1. Drive one host frame through the runtime (this runs the
    //         cube-spin system) and build the engine frame contract. ----
    let host_input = HostFrameInput::new(tick + 1, FIXED_STEP_NANOS, api.viewport);
    let host_report = api
        .driver
        .drive(&mut api.runtime, host_input)
        .expect("driver inputs are deterministic and valid");
    let engine_frame = api
        .frame_builder
        .build(&host_report, Vec::new())
        .expect("host report sequence is monotone");

    // Record the frame into the introspection surface — every tick's frame
    // becomes a queryable, serializable report.
    api.introspect.observe(&engine_frame);
    let frame_ctx = api.frame_api.frame_context(&engine_frame);

    // ---- 2. The cube's rotation IS the engine's own telemetry: the cube-spin
    //         system computed `cube.angle_rad` this step and it flows through
    //         the frame. The value the cube is built from is the value
    //         introspection reports — a single source of truth. ----
    let angle_rad = engine_frame
        .runtime_step_summaries()
        .iter()
        .flat_map(|summary| summary.metrics())
        .find(|m| m.name() == "cube.angle_rad")
        .and_then(|m| m.value().as_float())
        .expect("the cube-spin system emits cube.angle_rad each step");

    // ---- 3. Build a fresh resource table: built-in cube mesh + basic-lit material. ----
    let mut resources = api.resources_api.empty_table();
    let mesh_id = api.resources_api.register_cube_mesh(&mut resources);
    let base_color = Vec4::new(
        DEMO_CUBE_BASE_COLOR[0],
        DEMO_CUBE_BASE_COLOR[1],
        DEMO_CUBE_BASE_COLOR[2],
        DEMO_CUBE_BASE_COLOR[3],
    );
    let material_id = api
        .resources_api
        .register_basic_lit_material(&mut resources, base_color);

    // ---- 4. Build the world on the ECS substrate: rotating parent + child
    //         cube (renderable) + camera + light. This is the demo's world
    //         model, replacing the former axiom-scene scene graph. ----
    let aspect = VIEWPORT_WIDTH as f32 / VIEWPORT_HEIGHT as f32;
    let mut world: World<CubeComponents> = World::new();
    world.register_system(Box::new(TransformPropagation));

    let cube_root = world.spawn();
    let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, angle_rad)
        .expect("axis is unit and angle is finite");
    world.get_mut(cube_root).expect("just spawned").local = Some(Transform::from_rotation(rotation));

    let cube_child = world.spawn();
    {
        let row = world.get_mut(cube_child).expect("just spawned");
        row.local = Some(Transform::IDENTITY);
        row.parent = Some(cube_root);
        row.renderable = Some(RenderableData {
            mesh_id: mesh_id.raw(),
            material_id: material_id.raw(),
            visible: true,
        });
    }

    let camera_entity = world.spawn();
    {
        let row = world.get_mut(camera_entity).expect("just spawned");
        row.local = Some(Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)));
        row.camera = Some(CameraData {
            fovy_radians: std::f32::consts::FRAC_PI_3,
            aspect,
            near: 0.1,
            far: 100.0,
        });
    }

    let light_entity = world.spawn();
    {
        let row = world.get_mut(light_entity).expect("just spawned");
        row.local = Some(Transform::IDENTITY);
        row.light = Some(LightData {
            color: DEMO_LIGHT_COLOR,
            intensity: DEMO_LIGHT_INTENSITY,
        });
    }

    // ---- 5. Advance the world (frame-gated): runs transform propagation. ----
    world.advance(&frame_ctx);

    // ---- 6. Resolve resources (un-nameable value). ----
    let resolved = api.resources_api.resolve(&resources);

    // ---- 7. Read the world into the same plain-data snapshot the render
    //         pipeline already consumes — nothing downstream changes. ----
    let scene_snapshot_artifact = world_to_scene_snapshot(&world);

    // ---- 9. Read the resolved resources into a plain-data artifact. ----
    let mut meshes = Vec::with_capacity(api.resources_api.resolved_mesh_count(&resolved));
    for i in 0..api.resources_api.resolved_mesh_count(&resolved) {
        let id = api
            .resources_api
            .resolved_mesh_id_at(&resolved, i)
            .expect("mesh index in range");
        let vertex_count = api
            .resources_api
            .resolved_mesh_vertex_count(&resolved, id)
            .expect("mesh is present");
        let mut positions = Vec::with_capacity(vertex_count);
        let mut normals = Vec::with_capacity(vertex_count);
        let mut uvs = Vec::with_capacity(vertex_count);
        for v in 0..vertex_count {
            positions.push(
                api.resources_api
                    .resolved_mesh_position_at(&resolved, id, v)
                    .expect("vertex in range"),
            );
            normals.push(
                api.resources_api
                    .resolved_mesh_normal_at(&resolved, id, v)
                    .expect("vertex in range"),
            );
            uvs.push(
                api.resources_api
                    .resolved_mesh_uv_at(&resolved, id, v)
                    .expect("vertex in range"),
            );
        }
        let indices = api
            .resources_api
            .resolved_mesh_indices(&resolved, id)
            .expect("mesh is present")
            .to_vec();
        meshes.push(ResolvedMeshArtifact {
            id,
            positions,
            normals,
            uvs,
            indices,
        });
    }
    let mut materials =
        Vec::with_capacity(api.resources_api.resolved_material_count(&resolved));
    for i in 0..api.resources_api.resolved_material_count(&resolved) {
        let id = api
            .resources_api
            .resolved_material_id_at(&resolved, i)
            .expect("material index in range");
        let mat_base_color = api
            .resources_api
            .resolved_material_base_color(&resolved, id)
            .expect("material is present");
        materials.push(ResolvedMaterialArtifact {
            id,
            base_color: mat_base_color,
        });
    }
    let resolved_resources_artifact = ResolvedResourcesArtifact { meshes, materials };

    // ---- 10. GLUE: scene snapshot + resolved resources -> render input plan. ----
    let render_input_artifact = scene_to_render_input(
        &api.math,
        &scene_snapshot_artifact,
        &resolved_resources_artifact,
    );

    // ---- 11. Replay the render input plan into the real RenderApi builder. ----
    let mut render_input = api
        .render_api
        .new_input(render_input_artifact.viewport_width, render_input_artifact.viewport_height);
    api.render_api
        .set_input_clear_color(&mut render_input, render_input_artifact.clear_color);
    if let Some(camera) = render_input_artifact.camera {
        api.render_api
            .set_input_camera(&mut render_input, camera.view, camera.projection);
    }
    for light in &render_input_artifact.lights {
        // Every demo light is directional (see scene_to_render_input).
        api.render_api.add_input_directional_light(
            &mut render_input,
            light.vector_world,
            light.color,
            light.intensity,
        );
    }
    for mesh in &render_input_artifact.meshes {
        let positions = mesh
            .positions
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect();
        let normals = mesh
            .normals
            .iter()
            .map(|n| Vec3::new(n[0], n[1], n[2]))
            .collect();
        let uvs = mesh.uvs.iter().map(|u| Vec2::new(u[0], u[1])).collect();
        api.render_api.add_input_mesh(
            &mut render_input,
            mesh.id,
            positions,
            normals,
            uvs,
            mesh.indices.clone(),
        );
    }
    for material in &render_input_artifact.materials {
        let c = material.base_color;
        api.render_api.add_input_basic_lit_material(
            &mut render_input,
            material.id,
            Vec4::new(c[0], c[1], c[2], c[3]),
        );
    }
    for object in &render_input_artifact.objects {
        api.render_api.add_input_object(
            &mut render_input,
            object.world,
            object.mesh_idx,
            object.material_idx,
            object.visible,
        );
    }

    // ---- 12. Compile the render command list (un-nameable value). ----
    let render_commands = api.render_api.build_command_list(&render_input);

    // ---- 13. Read the render command list into a plain-data artifact. ----
    let command_count = api.render_api.command_count(&render_commands);
    let render_command_list_artifact = RenderCommandListArtifact {
        commands: (0..command_count)
            .filter_map(|i| match api.render_api.command_kind_at(&render_commands, i)? {
                RenderApi::KIND_CLEAR_FRAME => api
                    .render_api
                    .command_clear_color_at(&render_commands, i)
                    .map(|color| RenderCommandArtifact::ClearFrame { color }),
                RenderApi::KIND_SET_CAMERA => api
                    .render_api
                    .command_camera_at(&render_commands, i)
                    .map(|(view, projection)| RenderCommandArtifact::SetCamera {
                        view,
                        projection,
                    }),
                RenderApi::KIND_SET_PIPELINE => api
                    .render_api
                    .command_pipeline_at(&render_commands, i)
                    .map(|pipeline_id| RenderCommandArtifact::SetPipeline { pipeline_id }),
                RenderApi::KIND_SET_MESH => api
                    .render_api
                    .command_mesh_id_at(&render_commands, i)
                    .map(|mesh_id| RenderCommandArtifact::SetMesh { mesh_id }),
                RenderApi::KIND_SET_MATERIAL => api
                    .render_api
                    .command_material_id_at(&render_commands, i)
                    .map(|material_id| RenderCommandArtifact::SetMaterial { material_id }),
                RenderApi::KIND_DRAW_INDEXED => api
                    .render_api
                    .command_draw_indexed_at(&render_commands, i)
                    .map(|(index_count, world)| RenderCommandArtifact::DrawIndexed {
                        index_count,
                        world,
                    }),
                _ => None,
            })
            .collect(),
    };

    // ---- 14. GLUE: render command list -> GPU submission plan. ----
    let gpu_submission_artifact = render_command_list_to_gpu_submission(
        &render_command_list_artifact,
        render_input_artifact.viewport_width,
        render_input_artifact.viewport_height,
    );

    // ---- 15. Replay the submission plan into the real WebGpuApi and submit. ----
    let mut submission = api
        .webgpu_api
        .new_submission(gpu_submission_artifact.target_width, gpu_submission_artifact.target_height);
    for command in &gpu_submission_artifact.commands {
        match *command {
            GpuCommandArtifact::ClearFrame { color } => {
                api.webgpu_api.submission_clear_frame(&mut submission, color)
            }
            GpuCommandArtifact::SetCamera { view, projection } => api
                .webgpu_api
                .submission_set_camera(&mut submission, view, projection),
            GpuCommandArtifact::SetPipeline { pipeline_id } => api
                .webgpu_api
                .submission_set_pipeline(&mut submission, pipeline_id),
            GpuCommandArtifact::SetMesh { mesh_id } => {
                api.webgpu_api.submission_set_mesh(&mut submission, mesh_id)
            }
            GpuCommandArtifact::SetMaterial { material_id } => api
                .webgpu_api
                .submission_set_material(&mut submission, material_id),
            GpuCommandArtifact::DrawIndexed { index_count, world } => api
                .webgpu_api
                .submission_draw_indexed(&mut submission, index_count, world),
            GpuCommandArtifact::Present => {
                api.webgpu_api.submission_present(&mut submission)
            }
        }
    }
    let gpu_report = api.webgpu_api.submit(submission);

    // ---- 16. Read the GPU submission report into a plain-data artifact. ----
    let report_command_count = api.webgpu_api.report_command_count(&gpu_report);
    let gpu_submission_report_artifact = GpuSubmissionReportArtifact {
        target_width: gpu_submission_artifact.target_width,
        target_height: gpu_submission_artifact.target_height,
        command_count: report_command_count,
        command_kinds: (0..report_command_count)
            .filter_map(|i| api.webgpu_api.report_kind_at(&gpu_report, i))
            .collect(),
        clear_count: api.webgpu_api.report_clear_count(&gpu_report),
        draw_count: api.webgpu_api.report_draw_count(&gpu_report),
        present_count: api.webgpu_api.report_present_count(&gpu_report),
    };

    // ---- 17. Cube identity + transform from the snapshot artifact. ----
    let cube_node = scene_snapshot_artifact
        .nodes
        .iter()
        .find(|n| n.id == cube_child.raw())
        .expect("the cube child node is present in the snapshot");
    let cube = CubeIdentityArtifact {
        node_id: cube_child.raw(),
        mesh_id: mesh_id.raw(),
        material_id: material_id.raw(),
    };
    let cube_transform = CubeTransformArtifact {
        local: cube_node.local,
        world: cube_node.world,
    };

    VerticalSliceArtifact {
        tick,
        engine_frame_index: engine_frame.engine_frame_index(),
        host_frame_sequence: engine_frame.host_frame_sequence(),
        runtime_step_count: engine_frame.runtime_step_count(),
        cube,
        cube_transform,
        scene_snapshot: scene_snapshot_artifact,
        resolved_resources: resolved_resources_artifact,
        render_input: render_input_artifact,
        render_command_list: render_command_list_artifact,
        gpu_submission: gpu_submission_artifact,
        gpu_submission_report: gpu_submission_report_artifact,
    }
}
