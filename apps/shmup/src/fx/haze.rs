//! Ported from Claude-of-Duty `src/fx/haze.js:1-247`.
//!
//! Screen-space refraction for hot gas, shockwaves and heat shimmer.
//!
//! ## What is ported, what is a seam
//!
//! [`HazeSystem::emit`] (`haze.js:174-196`) writes into the same
//! [`crate::fx::particles::ParticleLayer`] every other particle pool uses —
//! real, testable spawn logic. [`HazeSystem::resize`]'s half-resolution
//! target-size math (`haze.js:150-160`) is pure arithmetic and is ported
//! too. Everything else in the source is GPU presentation with no CPU
//! analogue: the `DISTORT_FRAG`/`WARP_FRAG`/`WARP_VERT` shader strings, the
//! half-float `WebGLRenderTarget`, `prewarm`'s `renderer.compile` calls, and
//! `render`'s two-pass draw. Those stay unported, the same seam
//! [`crate::fx::particles`]'s module doc draws around `PARTICLE_VERT`/
//! `PARTICLE_FRAG`.

use crate::fx::particles::{reset_spawn, ParticleLayer, ParticleMode};

/// `class HazeSystem`'s CPU-relevant state, `haze.js:76-135`.
pub struct HazeSystem {
    pub layer: ParticleLayer,
    pub enabled: bool,
    /// Half-resolution refraction target size, `(width, height)` — `resize`'s
    /// `this.size`, `haze.js:136`.
    pub size: (u32, u32),
}

impl HazeSystem {
    /// `constructor(o)`, `haze.js:76-135` — minus the private distortion
    /// scene, the swapped-in `DISTORT_FRAG` material, and the warp-pass
    /// quad/material (all GPU presentation).
    pub fn new(capacity: usize) -> Self {
        HazeSystem {
            layer: ParticleLayer::new(capacity, ParticleMode::Additive),
            enabled: true,
            size: (1, 1),
        }
    }

    /// `resize(w, h)`, `haze.js:150-160` — minus the render-target
    /// (re)allocation, which only fires on this same size-change guard.
    /// Returns `true` when the size actually changed (i.e. the source would
    /// have reallocated its render target).
    pub fn resize(&mut self, w: u32, h: u32) -> bool {
        let rw = ((w as f64) * 0.5).floor().max(1.0) as u32;
        let rh = ((h as f64) * 0.5).floor().max(1.0) as u32;
        if self.size == (rw, rh) {
            return false;
        }
        self.size = (rw, rh);
        true
    }

    /// Add one distortion sprite. `strength` is a screen-space offset in UV.
    /// `emit(now, x, y, z, radius, grow, life, strength, tile, seed)`,
    /// `haze.js:174-196`. `tile` defaults to `P.SMOKE_A` at the call sites
    /// that omit it (the JS default parameter) — callers here pass it
    /// explicitly since Rust has no default arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        now: f64,
        x: f64,
        y: f64,
        z: f64,
        radius: f64,
        grow: f64,
        life: f64,
        strength: f64,
        tile: usize,
        seed: f64,
    ) -> usize {
        let mut s = reset_spawn();
        s.x = x;
        s.y = y;
        s.z = z;
        s.size0 = radius;
        s.size1 = radius * grow;
        s.size_curve = 0.5;
        s.life = life;
        s.drag = 2.0;
        s.tile = tile as f64;
        s.soft = 0.6;
        s.alpha = 1.0;
        s.alpha_curve = 1.2;
        s.r0 = strength;
        s.g0 = strength;
        s.b0 = strength;
        s.i0 = 1.0;
        s.r1 = strength;
        s.g1 = strength;
        s.b1 = strength;
        s.i1 = 1.0;
        s.seed = seed;
        self.layer.emit(&s, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::atlas::p;

    #[test]
    fn resize_halves_the_target_and_floors() {
        let mut haze = HazeSystem::new(16);
        assert!(haze.resize(1921, 1081));
        assert_eq!(haze.size, (960, 540));
    }

    #[test]
    fn resize_is_a_no_op_at_the_same_size() {
        let mut haze = HazeSystem::new(16);
        haze.resize(1920, 1080);
        assert!(!haze.resize(1920, 1080));
    }

    #[test]
    fn resize_never_goes_below_one_pixel() {
        let mut haze = HazeSystem::new(16);
        haze.resize(1, 1);
        assert_eq!(haze.size, (1, 1));
    }

    #[test]
    fn emit_writes_a_particle_into_the_layer() {
        let mut haze = HazeSystem::new(16);
        let slot = haze.emit(0.0, 1.0, 2.0, 3.0, 0.1, 3.0, 0.2, 0.7, p::SMOKE_A, 0.5);
        let raw = haze.layer.raw();
        assert_eq!(raw[slot * crate::fx::particles::STRIDE], 1.0);
    }
}
