//! 2D drawing (SPEC-10: draw2d) composed into the bridge: the particle, render
//! target, and shape verbs the TS `Frame` surface drives, every one forwarding to
//! the engine's [`axiom_draw2d::Draw2dApi`] builder. The builder owns the
//! transform stack, the layer sort, and the particle field; this module only
//! marshals scalars in and the neutral, layer-sorted command list out — nothing
//! is rasterized or re-implemented here.
//!
//! ## Presentation-only (§17.5)
//! Everything here is display data: the particle field feeds no sim-readable
//! getter, and `advance_particles` steps on the **presentation** delta the host
//! measured, never a fixed sim tick — so a 2D draw can never perturb determinism.
//!
//! ## Boundary convention (the established slice / scalar / handle rule)
//! A point / bounds crosses as a `&[f64]` slice (`bounds = [x, y, w, h]`); a
//! colour as its packed `0xRRGGBBAA` `u32`; an emitter recipe as one flat
//! `&[f64]` config slice (count, lifetime, speed, spread, gravityX, gravityY,
//! size, colorStart, colorEnd, layer) — one slice keeps the call within the
//! engine's argument-count budget. Handles cross as raw `u64` (`f64` at the JS
//! edge). [`Self::draw2d_finish`] returns the sorted command list as a flat
//! `[kind, layer, submission, …]` triple per command — the deterministic shape a
//! `Frame` consumer reads (full per-shape geometry rides the typed `as_*`
//! accessors on the host contract, a follow-up the boundary does not yet flatten).

use axiom_draw2d::{EmitterConfig, EmitterId};
use axiom_host::{Common2d, Fill2d, Rect, RenderTargetId, Rgba};
use axiom_kernel::{Meters, Ratio, Seconds};
use axiom_math::Vec2;

use crate::GameBridge;

/// The `i`-th element of a boundary slice as a scalar (missing ⇒ `0`).
fn at(s: &[f64], i: usize) -> f64 {
    *s.get(i).unwrap_or(&0.0)
}

/// A finite [`Meters`] from a boundary scalar (non-finite ⇒ zero).
fn meters(value: f64) -> Meters {
    Meters::new(value as f32).unwrap_or_else(|_| Meters::new(0.0).expect("0.0 is finite"))
}

/// A finite [`Seconds`] from a boundary scalar (non-finite ⇒ zero).
fn seconds(value: f64) -> Seconds {
    Seconds::new(value as f32).unwrap_or_else(|_| Seconds::new(0.0).expect("0.0 is finite"))
}

/// A `Vec2` from a 2-element boundary slice (missing entries read `0`).
fn vec2(s: &[f64]) -> Vec2 {
    Vec2::new(at(s, 0) as f32, at(s, 1) as f32)
}

/// An [`Rgba`] from a packed `0xRRGGBBAA` value (each channel `0..1`).
fn rgba(packed: u32) -> Rgba {
    let channel = |shift: u32| Ratio::finite_or_zero(((packed >> shift) & 0xFF) as f32 / 255.0);
    Rgba::new(channel(24), channel(16), channel(8), channel(0))
}

/// The per-draw [`Common2d`] (z-layer + alpha) from boundary scalars.
fn common(layer: i32, alpha: f64) -> Common2d {
    Common2d::new(layer, Ratio::finite_or_zero(alpha as f32))
}

impl GameBridge {
    /// Register a particle emitter from a flat config slice (`draw2dCreateEmitter`)
    /// `[count, lifetime, speed, spread, gravityX, gravityY, size, colorStart,
    /// colorEnd, layer]`; returns its raw [`EmitterId`].
    pub fn draw2d_create_emitter(&mut self, config: &[f64]) -> u64 {
        let recipe = EmitterConfig {
            count: at(config, 0) as u32,
            lifetime: seconds(at(config, 1)),
            speed: meters(at(config, 2)),
            spread: Ratio::finite_or_zero(at(config, 3) as f32),
            gravity: Vec2::new(at(config, 4) as f32, at(config, 5) as f32),
            size: meters(at(config, 6)),
            color_start: rgba(at(config, 7) as u32),
            color_end: rgba(at(config, 8) as u32),
            layer: at(config, 9) as i32,
        };
        u64::from(self.draw2d.create_emitter(recipe).raw())
    }

    /// Spawn a burst from emitter `id` at `at_point` flying along `direction`
    /// (`draw2dEmit`); an unknown id is a no-op.
    pub fn draw2d_emit(&mut self, id: u64, at_point: &[f64], direction: &[f64]) {
        self.draw2d
            .emit(EmitterId::from_raw(id as u32), vec2(at_point), vec2(direction));
    }

    /// Step the live particles by the presentation delta `dt` seconds and append
    /// each survivor as a particle-quad command (`draw2dAdvanceParticles`).
    pub fn draw2d_advance_particles(&mut self, dt: f64) {
        self.draw2d.advance_particles(seconds(dt));
    }

    /// Create an off-screen render target (`draw2dCreateRenderTarget`), returning
    /// its raw [`RenderTargetId`].
    pub fn draw2d_create_render_target(&mut self, width: u32, height: u32) -> u64 {
        u64::from(self.draw2d.create_render_target(width, height).raw())
    }

    /// Route subsequent draws into `target` (`draw2dBeginTarget`).
    pub fn draw2d_begin_target(&mut self, target: u64) {
        self.draw2d.begin_target(RenderTargetId::from_raw(target as u32));
    }

    /// Stop routing into a render target (`draw2dEndTarget`).
    pub fn draw2d_end_target(&mut self) {
        self.draw2d.end_target();
    }

    /// The texture handle naming `target`'s off-screen surface (`draw2dTargetTexture`).
    pub fn draw2d_target_texture(&self, target: u64) -> u64 {
        self.draw2d.target_texture(RenderTargetId::from_raw(target as u32)).raw()
    }

    /// Draw a filled rectangle (`draw2dRect`); `bounds = [x, y, w, h]`.
    pub fn draw2d_rect(&mut self, bounds: &[f64], fill: u32, layer: i32, alpha: f64) {
        let rect = Rect::new(
            Vec2::new(at(bounds, 0) as f32, at(bounds, 1) as f32),
            Vec2::new(at(bounds, 2) as f32, at(bounds, 3) as f32),
        );
        self.draw2d
            .rect(rect, Fill2d::color(rgba(fill)), common(layer, alpha));
    }

    /// Draw a filled circle (`draw2dCircle`); `center = [x, y]`.
    pub fn draw2d_circle(&mut self, center: &[f64], radius: f64, fill: u32, layer: i32, alpha: f64) {
        self.draw2d.circle(
            vec2(center),
            meters(radius),
            Fill2d::color(rgba(fill)),
            common(layer, alpha),
        );
    }

    /// Finish the frame and return the layer-sorted main command list as a flat
    /// `[kind, layer, submission, …]` triple per command (`draw2dFinish`). Resets
    /// the per-frame surface for the next frame (particles persist).
    pub fn draw2d_finish(&mut self) -> Vec<f64> {
        self.draw2d
            .finish()
            .commands()
            .iter()
            .flat_map(|cmd| {
                [
                    f64::from(cmd.kind_code()),
                    f64::from(cmd.layer()),
                    f64::from(cmd.submission_index()),
                ]
            })
            .collect()
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Register a particle emitter from a flat config slice (`draw2dCreateEmitter`).
        #[wasm_bindgen(js_name = draw2dCreateEmitter)]
        pub fn draw2d_create_emitter(&mut self, config: &[f64]) -> f64 {
            self.bridge.draw2d_create_emitter(config) as f64
        }

        /// Spawn a particle burst (`draw2dEmit`).
        #[wasm_bindgen(js_name = draw2dEmit)]
        pub fn draw2d_emit(&mut self, id: f64, at_point: &[f64], direction: &[f64]) {
            self.bridge.draw2d_emit(id as u64, at_point, direction);
        }

        /// Step the live particles (`draw2dAdvanceParticles`).
        #[wasm_bindgen(js_name = draw2dAdvanceParticles)]
        pub fn draw2d_advance_particles(&mut self, dt: f64) {
            self.bridge.draw2d_advance_particles(dt);
        }

        /// Create an off-screen render target (`draw2dCreateRenderTarget`).
        #[wasm_bindgen(js_name = draw2dCreateRenderTarget)]
        pub fn draw2d_create_render_target(&mut self, width: u32, height: u32) -> f64 {
            self.bridge.draw2d_create_render_target(width, height) as f64
        }

        /// Route subsequent draws into a render target (`draw2dBeginTarget`).
        #[wasm_bindgen(js_name = draw2dBeginTarget)]
        pub fn draw2d_begin_target(&mut self, target: f64) {
            self.bridge.draw2d_begin_target(target as u64);
        }

        /// Stop routing into a render target (`draw2dEndTarget`).
        #[wasm_bindgen(js_name = draw2dEndTarget)]
        pub fn draw2d_end_target(&mut self) {
            self.bridge.draw2d_end_target();
        }

        /// The texture handle naming a render target's surface (`draw2dTargetTexture`).
        #[wasm_bindgen(js_name = draw2dTargetTexture)]
        pub fn draw2d_target_texture(&self, target: f64) -> f64 {
            self.bridge.draw2d_target_texture(target as u64) as f64
        }

        /// Draw a filled rectangle (`draw2dRect`).
        #[wasm_bindgen(js_name = draw2dRect)]
        pub fn draw2d_rect(&mut self, bounds: &[f64], fill: u32, layer: i32, alpha: f64) {
            self.bridge.draw2d_rect(bounds, fill, layer, alpha);
        }

        /// Draw a filled circle (`draw2dCircle`).
        #[wasm_bindgen(js_name = draw2dCircle)]
        pub fn draw2d_circle(
            &mut self,
            center: &[f64],
            radius: f64,
            fill: u32,
            layer: i32,
            alpha: f64,
        ) {
            self.bridge.draw2d_circle(center, radius, fill, layer, alpha);
        }

        /// Finish the frame, returning the flat command list (`draw2dFinish`).
        #[wasm_bindgen(js_name = draw2dFinish)]
        pub fn draw2d_finish(&mut self) -> Vec<f64> {
            self.bridge.draw2d_finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// A particle emitter recipe: 3 particles, layer 5, opaque→clear fade.
    fn emitter() -> [f64; 10] {
        [
            3.0,                  // count
            2.0,                  // lifetime
            10.0,                 // speed
            0.25,                 // spread
            0.0,                  // gravityX
            -4.0,                 // gravityY
            0.5,                  // size
            f64::from(u32::MAX),  // colorStart 0xffffffff
            0.0,                  // colorEnd   0x00000000
            5.0,                  // layer
        ]
    }

    /// Build one frame: a render-target-routed rect, a main-list circle (layer 1),
    /// and a 3-particle burst (layer 5). Returns the flat finished command list.
    fn frame() -> Vec<f64> {
        let mut b = bridge();
        let target = b.draw2d_create_render_target(64, 32);
        b.draw2d_begin_target(target);
        b.draw2d_rect(&[0.0, 0.0, 10.0, 10.0], 0xff00_00ff, 0, 1.0);
        b.draw2d_end_target();
        b.draw2d_circle(&[1.0, 1.0], 2.0, 0x00ff_00ff, 1, 1.0);
        let e = b.draw2d_create_emitter(&emitter());
        b.draw2d_emit(e, &[0.0, 0.0], &[1.0, 0.0]);
        b.draw2d_advance_particles(0.5);
        b.draw2d_finish()
    }

    #[test]
    fn a_frame_builds_a_layer_sorted_command_list_and_replays() {
        let list = frame();
        // The render-target rect routes off the main list; the main list holds the
        // circle + 3 particle quads = 4 commands × 3 columns.
        assert_eq!(list.len(), 12);
        // Layer-sorted: the layer-1 circle (KIND_CIRCLE = 2) precedes the layer-5
        // particle quads (KIND_PARTICLE_QUAD = 8).
        assert_eq!(list[0], 2.0); // first command kind = circle
        assert_eq!(list[1], 1.0); // its layer
        assert_eq!(list[3], 8.0); // next command kind = particle quad
        assert_eq!(list[4], 5.0); // particle layer
        // Same facade calls + same dt ⇒ byte-identical command list.
        assert_eq!(frame(), list);
    }

    #[test]
    fn render_target_handles_are_stable_and_distinct() {
        let mut b = bridge();
        let a = b.draw2d_create_render_target(16, 16);
        let c = b.draw2d_create_render_target(32, 32);
        assert_ne!(a, c);
        // The target's surface texture is stable for a given target.
        assert_eq!(b.draw2d_target_texture(a), b.draw2d_target_texture(a));
    }
}
