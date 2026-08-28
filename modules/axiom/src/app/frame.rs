//! The per-frame engine drive on [`RunningApp`] — the `tick` family that advances
//! exactly one deterministic frame (step the runtime, advance the scene, and, when
//! rendering is enabled, submit and summarise the draws).

use axiom_host::HostFrameInput;
use axiom_kernel::Radians;

use super::RunningApp;
use crate::controller::FirstPersonInput;
use crate::frame_outcome::{DrawData, FrameOutcome, LightData, SkinnedDraw};
use crate::player::PlayerInput;
use crate::texture::Texture;

impl RunningApp {
    /// Drive one deterministic frame at `tick`: step the runtime, advance the
    /// scene at the tick, and (when rendering is enabled) submit the frame and
    /// summarise the per-object draws. Browser-free and fully replayable — the
    /// outcome is a pure function of `tick`. The caller (the run loop) owns the
    /// monotonic tick and must pass `0, 1, 2, …` in order.
    pub fn tick(&mut self, tick: u64) -> FrameOutcome {
        self.tick_with_controls(tick, &[], &[])
    }

    /// Drive one deterministic frame at `tick`, applying `inputs` (per-player
    /// move deltas) to the simulation before stepping. The input-free
    /// [`Self::tick`] is `tick_with(tick, &[])`. Like `tick`, the outcome is a
    /// pure function of `tick` and `inputs`, so two peers given the same
    /// confirmed inputs produce byte-identical frames.
    pub fn tick_with(&mut self, tick: u64, inputs: &[PlayerInput]) -> FrameOutcome {
        self.tick_with_controls(tick, inputs, &[])
    }

    /// Drive one deterministic frame at `tick`, applying both per-player move
    /// `inputs` and first-person `controls` to the simulation before stepping.
    /// [`Self::tick`] and [`Self::tick_with`] are the empty-`controls` cases. A
    /// `control` yaws and moves its addressed [`crate::prelude::Controller`] node
    /// along its own facing — the first-person camera path — while `inputs`
    /// translate [`crate::prelude::Player`] nodes in world space. The outcome
    /// stays a pure function of `tick`, `inputs`, and `controls`.
    pub fn tick_with_controls(
        &mut self,
        tick: u64,
        inputs: &[PlayerInput],
        controls: &[FirstPersonInput],
    ) -> FrameOutcome {
        self.step(tick, inputs, controls);
        self.render(tick)
    }

    /// Advance the simulation one deterministic tick **without rendering** — the
    /// step half of a frame. [`Self::tick_with_controls`] calls this then
    /// [`Self::render`]; a host that owns its own fixed-step loop (the `@axiom/game`
    /// TS SDK) calls this once per fixed tick during its `advance` and renders only
    /// once per presented frame, after its per-frame scene mutations. Stepping is
    /// where all simulation state changes; rendering it is a separate, side-effect-
    /// free read (see [`Self::render`]). Splitting them keeps an N-tick catch-up
    /// frame from doing N wasted renders — it does N steps and one render.
    pub fn step(&mut self, tick: u64, inputs: &[PlayerInput], controls: &[FirstPersonInput]) {
        let host_input = HostFrameInput::new(tick + 1, self.step_nanos, self.viewport);
        let host_report = self
            .driver
            .drive(&mut self.runtime, host_input)
            .expect("driver inputs are deterministic and valid");
        let mut commands: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(i, input)| self.scene.move_command(i as u64, input.player, input.delta))
            .collect();
        let scene = &self.scene;
        commands.extend(controls.iter().enumerate().map(|(j, control)| {
            let yaw = Radians::new(control.yaw.as_radians()).expect("authored yaw is finite");
            let pitch = Radians::new(control.pitch.as_radians()).expect("authored pitch is finite");
            scene.controller_command(
                (inputs.len() + j) as u64,
                control.index,
                control.move_local,
                yaw,
                pitch,
                control.seat_y,
            )
        }));
        let engine_frame = self
            .frame_builder
            .build(&host_report, commands)
            .expect("host report sequence is monotone");
        let frame_ctx = self.frame_api.frame_context(&engine_frame);
        // Step the systems only; the snapshot is taken lazily at render time into
        // a retained buffer, so a stepped frame never allocates + discards one.
        self.scene.advance_systems(tick, &frame_ctx);
    }

    /// Render the current scene state at `tick` **without stepping the
    /// simulation** — the present half of a frame. [`Self::tick_with_controls`]
    /// calls this right after it steps; a host that drives the simulation itself
    /// (banking real elapsed time into fixed ticks) instead calls this once per
    /// presented frame, after writing that frame's camera and node transforms, so
    /// the pixels reflect the very latest authored state rather than the state as
    /// of the last fixed tick. Re-rendering the same scene at the same `tick`
    /// twice is a pure function of that state — it submits draws and summarises
    /// them, mutating no simulation state — so it is safe to call standalone and
    /// replayable. When rendering is disabled the outcome is simulation-only.
    pub fn render(&mut self, tick: u64) -> FrameOutcome {
        let width = self.viewport.physical_width();
        let height = self.viewport.physical_height();

        // The app's authored hemisphere ambient rides onto the outcome so both
        // backends light unlit faces from it (captured before the render closure
        // borrows `self`).
        let ambient = self.ambient;
        // The app's authored colour grade rides onto the outcome the same way, so
        // both backends present the same filmic look (captured before the closure
        // borrows `self`).
        let postprocess = self.postprocess;
        // The app's authored atmospheric fog rides onto the outcome the same way, so
        // both backends recede distance into the same colour over the same range
        // (captured before the closure borrows `self`).
        let depth_fog = self.depth_fog;
        // The app's authored sky and bloom ride onto the outcome the same way, for
        // the same reason (captured before the closure borrows `self`).
        let sky = self.sky;
        let indirect = self.indirect;
        let bloom = self.bloom;
        let rendered = self.render.then(|| {
            let mut frame =
                self.pipeline
                    .new_frame(width, height, self.clear_color, self.light_direction);
            let pipeline = &mut self.pipeline;
            // Identity only. The geometry these ids name was uploaded to the
            // backend once at bind (`RunningApp::mesh_set` reads this same
            // registry); re-sending it every frame copied the whole world twice
            // per frame to deliver an id and an index count. See
            // `axiom_render::RenderMesh`.
            self.meshes.iter().for_each(|(id, geometry)| {
                pipeline.frame_add_mesh(&mut frame, *id, geometry.indices.len() as u32)
            });
            self.materials.iter().for_each(|(id, material)| {
                // `0` = untextured; live albedo pixels are uploaded separately via
                // `material_textures`. Opacity is folded into the per-draw alpha so
                // a translucent material blends. An app-authored raw-pixel texture
                // (nonzero `custom_texture`) takes precedence over the built-in one.
                let texture_id = (material.custom_texture() != 0)
                    .then_some(material.custom_texture())
                    .or_else(|| material.texture().map(Texture::id))
                    .unwrap_or(0);
                let emissive = material.emissive().to_array();
                pipeline.frame_add_lit_material(
                    &mut frame,
                    *id,
                    material.base_color().to_array(),
                    [emissive[0], emissive[1], emissive[2]],
                    material.roughness(),
                    material.opacity(),
                    texture_id,
                    // The appearance program the material names (`0` = the
                    // built-in fixed material path, which is every material an
                    // app authored with `Material::lit`).
                    material.surface_program(),
                )
            });
            let report = pipeline.submit(&frame, &mut self.scene, &mut self.webgpu);

            let view_projection = pipeline.report_view_projection(&report);
            // One DrawData per drawn object (submission order): mvp, world,
            // colour, mesh/material ids, and the contact-shadow caster mark.
            let draws: Vec<DrawData> = (0..pipeline.report_draw_count(&report))
                .map(|i| {
                    let world = pipeline
                        .report_draw_world(&report, i)
                        .expect("draw index in range");
                    DrawData::new(
                        view_projection.multiply(world).as_cols_array(),
                        world.as_cols_array(),
                        pipeline
                            .report_draw_color(&report, i)
                            .expect("draw in range"),
                        pipeline
                            .report_draw_mesh_id(&report, i)
                            .expect("draw in range"),
                        pipeline
                            .report_draw_material_id(&report, i)
                            .expect("draw in range"),
                        pipeline
                            .report_draw_casts_shadow(&report, i)
                            .expect("draw in range"),
                    )
                    // The material's self-illumination, carried beside the colour
                    // rather than multiplied into it — `with_emissive` for a
                    // non-emissive material is `[0, 0, 0]`, an exact no-op.
                    .with_emissive(
                        pipeline
                            .report_draw_emissive(&report, i)
                            .expect("draw in range"),
                    )
                    // ...and its specular strength, from the same place and for
                    // the same reason: a fully-rough material yields `0`, which
                    // is an exact no-op in every backend.
                    .with_specular(
                        pipeline
                            .report_draw_specular(&report, i)
                            .expect("draw in range"),
                    )
                    // ...and the appearance program its material names, from the
                    // same place. `0` — every material that never named a
                    // surface — is the built-in fixed material path.
                    .with_surface_program(
                        pipeline
                            .report_draw_surface_program(&report, i)
                            .expect("draw in range"),
                    )
                })
                .collect();

            // Drain this frame's queued skinned draws (bake-once meshes deformed by
            // a joint palette), computing each MVP = view_proj * world so the
            // skinning vertex shader only has to apply `mvp * skin * position`.
            let skinned_draws: Vec<SkinnedDraw> = self
                .pending_skinned
                .drain(..)
                .map(|p| {
                    let mvp = view_projection
                        .multiply(axiom_math::Mat4::from_cols_array(p.world))
                        .as_cols_array();
                    SkinnedDraw::new(mvp, p.world, p.color, p.mesh_id, p.material_id, p.palette)
                })
                .collect();

            // The frame's resolved lights (directional + point), threaded to
            // the live backend's lighting uniform.
            let light_count = pipeline.report_light_count(&report);
            let lights: Vec<LightData> = (0..light_count)
                .map(|i| {
                    let (kind, vec, color, intensity) = pipeline
                        .report_light_at(&report, i)
                        .expect("light index in range");
                    LightData::new(kind, vec, color, intensity)
                })
                .collect();

            FrameOutcome::new(
                tick,
                pipeline.report_command_count(&report),
                pipeline.report_clear_color(&report),
                draws,
                lights,
                pipeline.report_light_view_proj(&report),
                view_projection.as_cols_array(),
                pipeline.report_view(&report).as_cols_array(),
                pipeline.report_projection(&report).as_cols_array(),
                pipeline.report_sdf_scene(&report).cloned(),
                pipeline.report_presented(&report),
                pipeline.report_recorded(&report),
            )
            .with_camera_lens(pipeline.report_camera_lens(&report))
            .with_skinned_draws(skinned_draws)
            .with_ambient(ambient)
            .with_depth_fog(depth_fog)
            .with_postprocess(postprocess)
            .with_sky(sky)
            .with_indirect(indirect)
            .with_bloom(bloom)
        });
        rendered.unwrap_or_else(|| FrameOutcome::simulation_only(tick, self.clear_color))
    }
}

#[cfg(test)]
mod tests {
    use crate::angle::Angle;
    use crate::app::App;
    use crate::camera::{Camera, PerspectiveProjection};
    use crate::default_plugins::DefaultPlugins;
    use crate::window::Window;
    use axiom_kernel::Meters;
    use axiom_math::{Transform, Vec3};

    /// A perspective camera looking down -Z.
    fn camera() -> Camera {
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(60.0),
            near: Meters::new(0.1).expect("near plane is finite"),
            far: Meters::new(100.0).expect("far plane is finite"),
        })
    }

    /// A bare rendering app — empty scene, render enabled.
    fn render_app() -> crate::app::RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .build()
    }

    /// The camera's intrinsics reach the frame outcome as the app authored
    /// them, and travel with the pose rather than being frozen at build: the
    /// world matrix follows the camera when it is moved, and a frame rendered
    /// before any camera was set states nothing at all.
    ///
    /// This is the lane a backend fits its own view volumes from, so "60 degrees
    /// went in and 60 degrees came out" is the whole contract — a fov recovered
    /// from the projection instead would be the shortcut it exists to remove.
    #[test]
    fn the_frame_outcome_carries_the_camera_intrinsics_the_app_authored() {
        let mut app = render_app();
        // Before a camera exists there is nothing to state.
        assert_eq!(app.tick(0).camera_lens(), None);

        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
        );
        let lens = app
            .render(1)
            .camera_lens()
            .expect("the frame now has a camera");
        assert!(
            (lens.fovy().get() - 60_f32.to_radians()).abs() < 1.0e-6,
            "authored 60 degrees, reported {} rad",
            lens.fovy().get()
        );
        assert_eq!(lens.near().get(), 0.1);
        assert_eq!(lens.far().get(), 100.0);
        // A 64x64 window is square, so the aspect is exactly 1.
        assert_eq!(lens.aspect().get(), 1.0);
        assert_eq!(
            [lens.world()[12], lens.world()[13], lens.world()[14]],
            [0.0, 0.0, 8.0]
        );

        // The pose travels: move the camera and the stated world matrix moves
        // with it (the volume fit follows the view, it is not fixed at build).
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 40.0)),
        );
        let moved = app.render(2).camera_lens().expect("still has a camera");
        assert_eq!(moved.world()[14], 40.0);
        assert_eq!(moved.fovy(), lens.fovy());
    }

    #[test]
    fn render_reflects_a_scene_mutation_made_after_the_last_step() {
        // A host that steps during its own `advance` and then writes the camera
        // before presenting must see the *new* camera in the rendered frame — not
        // the camera as of the last fixed tick.
        let mut app = render_app();
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
        );
        let near = app.tick(0).camera_view_proj();
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 40.0)),
        );
        let far = app.render(1).camera_view_proj();
        assert_ne!(
            near, far,
            "render() reflects the post-step camera write, not the stale tick state"
        );
    }

    /// The whole point of `Visible`: an invisible renderable is not a small
    /// draw, it is *no* draw. It never reaches the frame's draw list, so it
    /// costs no projection, no shading, and no instance in a mesh batch.
    ///
    /// This is what lets a pooled scene retire its unused slots for free
    /// instead of parking them off-screen and paying full per-triangle cost to
    /// have them culled.
    #[test]
    fn an_invisible_renderable_produces_no_draw_at_all() {
        use crate::color::Color;
        use crate::material::Material;
        use crate::mesh::Mesh;
        use crate::spawn::Spawn;
        use crate::visible::Visible;

        let mut app = render_app();
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
        );
        let mesh = app.add_mesh(Mesh::cube());
        let material = app.add_material(Material::lit(Color::WHITE));
        let entity = app.spawn(Spawn::new(
            Transform::from_translation(Vec3::new(0.0, 0.0, -3.0)),
            mesh,
            material,
        ));

        let shown = app.render(0);
        let shown_draws = shown.draws().len();
        let shown_instances: u32 = shown.mesh_batches().iter().map(|(_, _, _, n)| n).sum();
        assert_eq!(shown_draws, 1, "a visible cube is one draw");
        assert_eq!(shown_instances, 1);

        assert!(app.set::<Visible>(entity, Visible(false)));
        let hidden = app.render(1);
        assert!(
            hidden.draws().is_empty(),
            "an invisible renderable emits no draw"
        );
        assert!(
            hidden.mesh_batches().is_empty(),
            "and therefore no mesh batch instance"
        );
        assert!(
            hidden.mesh_batch_casters().is_empty(),
            "the caster flags stay aligned with the batches"
        );

        // And it comes back, unchanged, when shown again.
        assert!(app.set::<Visible>(entity, Visible(true)));
        let again = app.render(2);
        assert_eq!(again.draws().len(), shown_draws);
        assert_eq!(
            again.mesh_batches().iter().map(|(_, _, _, n)| n).sum::<u32>(),
            shown_instances
        );
    }

    /// **End to end.** An app authors two materials the two available ways —
    /// `Material::lit`, the one-liner every existing app uses, and
    /// `Material::from_surface` — and the appearance program each names has to
    /// arrive on that object's draw. `0` for the plain one is the compatibility
    /// guarantee: it is the built-in fixed material path, i.e. exactly today's
    /// engine.
    #[test]
    fn a_materials_surface_program_reaches_its_draw() {
        use crate::color::Color;
        use crate::material::Material;
        use crate::mesh::Mesh;
        use crate::spawn::Spawn;

        let surface = || {
            axiom_surface::SurfaceBuilder::new()
                .lighting(axiom_surface::LightingModel::Unlit)
                .build()
                .expect("an unlit surface is legal")
        };

        let mut app = render_app();
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
        );
        let mesh = app.add_mesh(Mesh::cube());
        let plain = app.add_material(Material::lit(Color::WHITE));
        let authored = app.add_material(Material::from_surface(surface()));
        app.spawn(Spawn::new(
            Transform::from_translation(Vec3::new(-2.0, 0.0, -3.0)),
            mesh,
            plain,
        ));
        app.spawn(Spawn::new(
            Transform::from_translation(Vec3::new(2.0, 0.0, -3.0)),
            mesh,
            authored,
        ));

        let outcome = app.render(0);
        let programs: Vec<u64> = outcome
            .draws()
            .iter()
            .map(crate::frame_outcome::DrawData::surface_program)
            .collect();
        assert_eq!(
            programs,
            vec![0, surface().digest().raw()],
            "the plain material takes the built-in path; the surface-backed one \
             carries its surface's content digest"
        );
    }

    #[test]
    fn render_only_does_not_advance_the_simulation_and_is_idempotent() {
        let mut app = render_app();
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
        );
        let before = app.snapshot_sim();
        let first = app.render(7);
        let second = app.render(7);
        assert_eq!(
            first, second,
            "render is a pure function of the scene at a tick"
        );
        assert_eq!(
            before,
            app.snapshot_sim(),
            "render must not mutate simulation state"
        );
    }
}
