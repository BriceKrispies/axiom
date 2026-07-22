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
        self.scene.advance(tick, &frame_ctx);
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
        let rendered = self.render.then(|| {
            let mut frame =
                self.pipeline
                    .new_frame(width, height, self.clear_color, self.light_direction);
            let pipeline = &mut self.pipeline;
            self.meshes.iter().for_each(|(id, geometry)| {
                pipeline.frame_add_mesh(
                    &mut frame,
                    *id,
                    geometry.positions.clone(),
                    geometry.normals.clone(),
                    geometry.uvs.clone(),
                    geometry.indices.clone(),
                )
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
                )
            });
            let report = pipeline.submit(&frame, &self.scene, &self.webgpu);

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
                pipeline.report_sdf_scene(&report).cloned(),
                pipeline.report_presented(&report),
                pipeline.report_recorded(&report),
            )
            .with_skinned_draws(skinned_draws)
            .with_ambient(ambient)
            .with_postprocess(postprocess)
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
