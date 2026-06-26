//! The per-frame engine drive on [`RunningApp`] — the `tick` family that advances
//! exactly one deterministic frame (step the runtime, advance the scene, and, when
//! rendering is enabled, submit and summarise the draws). A child module of `app`
//! so it reaches `RunningApp`'s private per-frame machinery while keeping `app.rs`
//! within the per-file size budget.

use axiom_host::HostFrameInput;
use axiom_kernel::Radians;

use super::RunningApp;
use crate::controller::FirstPersonInput;
use crate::frame_outcome::{DrawData, FrameOutcome, LightData};
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
        let width = self.viewport.physical_width();
        let height = self.viewport.physical_height();

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
            )
        }));
        let engine_frame = self
            .frame_builder
            .build(&host_report, commands)
            .expect("host report sequence is monotone");
        let frame_ctx = self.frame_api.frame_context(&engine_frame);
        self.scene.advance(tick, &frame_ctx);

        // `then` keeps the render path lazy: it runs (with all its side effects)
        // only when rendering is enabled; otherwise the simulation-only outcome is
        // produced. Behaviourally identical to the former `if !self.render` early
        // return, without the branch in source.
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
            self.materials.iter().for_each(|(id, color, texture)| {
                // The pipeline records the material→texture binding (for
                // receipt fidelity); the live albedo pixels are uploaded
                // separately via `material_textures`. `0` = untextured.
                let texture_id = texture.map(Texture::id).unwrap_or(0);
                pipeline.frame_add_textured_material(&mut frame, *id, *color, texture_id)
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
                pipeline.report_presented(&report),
                pipeline.report_recorded(&report),
            )
        });
        rendered.unwrap_or_else(|| FrameOutcome::simulation_only(tick, self.clear_color))
    }
}
