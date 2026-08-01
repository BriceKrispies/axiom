//! Backend-neutral **atmospheric depth fog** for a frame: the colour distance
//! recedes toward, and the normalized-depth range over which it does.
//!
//! Like [`crate::FrameAmbient`] and [`crate::FrameVolumetrics`], this is neutral
//! frame data — the *one* definition of a frame's aerial perspective, read
//! identically by every backend. Before it existed the two backends disagreed:
//! the Canvas 2D software raster has always run a depth-fog post-pass (its
//! `FogCue`, receding toward the frame's clear colour), while the GPU scene
//! renderer had **no** fog at all. The same scene therefore had a soft horizon on
//! Canvas 2D and a hard, un-attenuated one on WebGPU/WebGL2 — a backend
//! divergence, not an art choice. Carrying the fog as frame data removes it:
//! both backends read these numbers, and a frame that carries no fog keeps each
//! backend's prior default.
//!
//! ## Why normalized depth and not metres
//!
//! The range is expressed in **normalized device depth** (`0` at the near plane,
//! `1` at the far plane) because that is the depth both backends already hold:
//! the Canvas 2D post-pass reads its z-buffer, and the GPU fragment stage reads
//! `@builtin(position).z`. Neither has to reconstruct a view distance, so the two
//! fog terms are the same arithmetic on the same quantity — which is what keeps
//! them in parity. NDC depth is **non-linear** (most of a perspective frustum's
//! visible range clusters near `1.0`), so a useful atmospheric range for a long
//! view sits high: with a `1.2 m` near plane and a `1650 m` far plane, `200 m` is
//! already `≈0.995`. Author the numbers from that fact — this is the same unit
//! [`FrameRetro32BitProfile`](crate::FrameRetro32BitProfile) states its `fog_*`
//! fields in.
//!
//! Normalized scalars are [`Ratio`] — no naked `f32` on the public surface.

use axiom_kernel::Ratio;

/// A frame's atmospheric depth fog: pixels are mixed toward `color` by their
/// normalized depth, from `0` at `near` to `strength` at `far` and beyond.
///
/// Presence of a `FrameDepthFog` on a [`FramePacket`](crate::FramePacket) *is*
/// the enable — an absent fog leaves each backend on its own prior default, so
/// no existing frame changes. [`FrameDepthFog::none`] is the explicit "author
/// says: no fog" value, and is an exact no-op on every backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameDepthFog {
    near: Ratio,
    far: Ratio,
    strength: Ratio,
    color: [f32; 3],
}

impl FrameDepthFog {
    /// A depth fog from its normalized-depth range, maximum density, and the
    /// linear-RGB colour distance recedes toward.
    ///
    /// `near`/`far` are normalized device depths (see the module docs: non-linear,
    /// so an atmospheric range sits high); `strength` is the mix fraction reached
    /// at `far` (`1.0` = the far plane is pure `color`). A degenerate or inverted
    /// range is safe — a backend clamps it — and `strength = 0` is a no-op.
    pub const fn new(near: Ratio, far: Ratio, strength: Ratio, color: [f32; 3]) -> Self {
        FrameDepthFog {
            near,
            far,
            strength,
            color,
        }
    }

    /// Fog that does nothing: zero strength over the whole depth range, black.
    /// Every backend's mix fraction is `0` for every pixel, so a frame carrying
    /// this renders exactly as an unfogged one.
    pub const fn none() -> Self {
        FrameDepthFog::new(
            Ratio::finite_or_zero(0.0),
            Ratio::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.0),
            [0.0, 0.0, 0.0],
        )
    }

    /// Normalized depth at which the fog starts (mix fraction `0`).
    pub const fn near(&self) -> Ratio {
        self.near
    }

    /// Normalized depth at which the fog reaches [`Self::strength`].
    pub const fn far(&self) -> Ratio {
        self.far
    }

    /// The maximum mix fraction, reached at (and past) [`Self::far`].
    pub const fn strength(&self) -> Ratio {
        self.strength
    }

    /// The linear-RGB colour distance recedes toward.
    pub const fn color(&self) -> [f32; 3] {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    #[test]
    fn accessors_round_trip_every_field() {
        let fog = FrameDepthFog::new(r(0.98), r(0.999), r(0.85), [0.02, 0.03, 0.08]);
        assert_eq!(fog.near().get(), 0.98);
        assert_eq!(fog.far().get(), 0.999);
        assert_eq!(fog.strength().get(), 0.85);
        assert_eq!(fog.color(), [0.02, 0.03, 0.08]);
        assert!(format!("{fog:?}").contains("FrameDepthFog"));
    }

    #[test]
    fn none_is_a_zero_strength_no_op_and_differs_from_a_real_fog() {
        let off = FrameDepthFog::none();
        assert_eq!(off.strength().get(), 0.0);
        assert_eq!(off.near().get(), 0.0);
        assert_eq!(off.far().get(), 1.0);
        assert_eq!(off.color(), [0.0, 0.0, 0.0]);
        assert_eq!(off, FrameDepthFog::none());
        assert_ne!(off, FrameDepthFog::new(r(0.9), r(1.0), r(0.5), [1.0; 3]));
    }
}
