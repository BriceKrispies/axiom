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
//! ## The normalized-depth ramp, and why it is not enough on its own
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
//! "Author the numbers from that fact" is where this contract used to stop, and
//! it is not author*able* for the case aerial perspective exists to serve: a
//! ground plane running from under the camera to the horizon. NDC depth is
//! hyperbolic in view distance, so with that same frustum `50 m` is `0.976`,
//! `200 m` is `0.9940`, `800 m` is `0.9985` and the horizon is `1.0` — three
//! quarters of the *depth* range is spent on the first few dozen metres and the
//! entire remaining kilometre is squeezed into the last `0.006`. Any `[near, far]`
//! window authored in that unit is therefore not a ramp at all: it is a **switch
//! that flips at one screen row**. Everything beyond it is fully hazed and
//! everything nearer is untouched, with a visible seam between the two and a
//! large, perfectly flat, entirely unmodulated plane below it. Widening the
//! window does not fix it — it moves the seam. The unit itself is the defect,
//! and it cannot be fixed by an app because the app has no knob whose shape is
//! different.
//!
//! ## Extinction: the physical term, in metres
//!
//! [`FrameDepthFog::with_extinction`] adds the term real air has. Haze is
//! Beer–Lambert: a constant *fraction* of the remaining radiance is scattered
//! out per unit of distance travelled, so the density is
//! `1 - 2^(-rate * distance)` — a smooth exponential in **world metres**, with
//! no frustum in it and no seam anywhere. `rate` is the reciprocal of the
//! half-distance: `0.004 /m` means the haze is half-way in at `250 m`, three
//! quarters at `500 m`. That is one authored number with a physical meaning a
//! reader can check against the scene, and it is the same number for a portrait
//! phone frustum and a widescreen one, which the NDC window is not.
//!
//! The two terms **compose** rather than replace ([`FrameDepthFog::mix_fraction`]):
//! they are independent extinction events, so the surviving radiance multiplies
//! and the density is `1 - (1 - screen) * (1 - air)`. A frame that authors no
//! rate has `air = 0` exactly, so it evaluates bit-for-bit as it did before this
//! parameter existed, and the screen-space ramp remains available as the
//! near-field cue it is good at.
//!
//! Evaluating the air term needs the fragment's **world distance from the
//! camera**, which the GPU backends have (the mesh pass interpolates a world
//! position, the SDF pass marches one) and the Canvas 2D software post-pass does
//! not — it reads an NDC z-buffer and nothing else. That is a real capability
//! split, so it is a declared one:
//! [`RenderCapability::AerialPerspective`](crate::RenderCapability), whose
//! degradation is `Substitute` — a backend without it evaluates the
//! normalized-depth ramp alone, which is exactly what it was already doing.
//!
//! Normalized scalars are [`Ratio`] — no naked `f32` on the public surface.

use axiom_kernel::{Meters, Ratio};

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
    extinction: Ratio,
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
            // Zero extinction is exactly no air: `1 - 2^0 = 0` at every distance,
            // so a fog that never mentions the parameter evaluates bit-for-bit as
            // it did before the parameter existed. Same posture
            // `FrameSky::gradient` takes for its body and its cloud.
            extinction: Ratio::finite_or_zero(0.0),
        }
    }

    /// Give the fog the **physical** term: an extinction `rate` per world metre,
    /// so density is `1 - 2^(-rate * distance)` — smooth, frustum-independent,
    /// and seamless across a ground plane running to the horizon, which the
    /// normalized-depth range structurally cannot be (see the module docs).
    ///
    /// The rate is the reciprocal of the half-distance: `0.004 /m` puts the haze
    /// half-way in at `250 m`. `0` is the default and an exact no-op.
    ///
    /// This term composes with — it does not replace — the `[near, far]` ramp;
    /// see [`Self::mix_fraction`]. It requires the fragment's world distance, so
    /// it is gated by [`RenderCapability::AerialPerspective`](crate::RenderCapability),
    /// which degrades by *substituting* the ramp alone.
    pub const fn with_extinction(mut self, rate: Ratio) -> Self {
        self.extinction = rate;
        self
    }

    /// The per-metre extinction rate. See [`Self::with_extinction`].
    pub const fn extinction(&self) -> Ratio {
        self.extinction
    }

    /// **The** definition of this frame's fog density at a fragment: the mix
    /// fraction toward [`Self::color`], given the fragment's normalized device
    /// depth and its world distance from the camera. Every backend mirrors this
    /// arithmetic; keeping it here rather than only in WGSL is what makes it
    /// testable without a GPU and what stops the two backends drifting.
    ///
    /// The screen-space ramp runs `0` at [`Self::near`] to `1` at [`Self::far`]
    /// (a degenerate or inverted range is floored, never divided by zero). The
    /// air term is Beer–Lambert on the distance. They are independent extinction
    /// events, so the surviving radiance multiplies and the densities compose as
    /// `1 - (1 - screen) * (1 - air)`. [`Self::strength`] is the ceiling on the
    /// result, exactly as it was when the ramp was the only term.
    ///
    /// A backend without
    /// [`RenderCapability::AerialPerspective`](crate::RenderCapability) passes
    /// `0.0` for `view_distance`, which zeroes the air term and leaves the ramp —
    /// the declared substitute, expressed as an argument rather than as a second
    /// code path.
    /// `ndc_depth` is a normalized device depth and `view_distance` is a world
    /// distance, so they are a [`Ratio`] and [`Meters`] rather than two bare
    /// `f32`s: at this boundary a naked float says nothing about which of the two
    /// it is, and swapping them silently produces a plausible-looking fog instead
    /// of an error.
    pub fn mix_fraction(&self, ndc_depth: Ratio, view_distance: Meters) -> Ratio {
        let span = (self.far.get() - self.near.get()).abs().max(1.0e-6);
        let screen = ((ndc_depth.get() - self.near.get()) / span).clamp(0.0, 1.0);
        let air =
            1.0 - (-self.extinction.get().max(0.0) * view_distance.get().max(0.0)).exp2();
        let combined = 1.0 - (1.0 - screen) * (1.0 - air);
        Ratio::finite_or_zero(combined * self.strength.get().clamp(0.0, 1.0))
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

    /// A normalized device depth as the `Ratio` the API now takes.
    fn nd(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    /// A world distance as the `Meters` the API now takes.
    fn me(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

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
        // Extinction defaults to zero — the no-air default.
        assert_eq!(fog.extinction().get(), 0.0);
        assert_eq!(fog.with_extinction(r(0.004)).extinction().get(), 0.004);
        assert!(format!("{fog:?}").contains("FrameDepthFog"));
    }

    #[test]
    fn none_is_a_zero_strength_no_op_and_differs_from_a_real_fog() {
        let off = FrameDepthFog::none();
        assert_eq!(off.strength().get(), 0.0);
        assert_eq!(off.near().get(), 0.0);
        assert_eq!(off.far().get(), 1.0);
        assert_eq!(off.color(), [0.0, 0.0, 0.0]);
        assert_eq!(off.extinction().get(), 0.0);
        assert_eq!(off, FrameDepthFog::none());
        assert_ne!(off, FrameDepthFog::new(r(0.9), r(1.0), r(0.5), [1.0; 3]));
        // A rate is part of a fog's identity: two fogs differing only in air are
        // different fogs.
        assert_ne!(off, off.with_extinction(r(0.01)));
        // Zero strength is a no-op whatever else is authored, air included.
        assert_eq!(off.with_extinction(r(1.0)).mix_fraction(nd(1.0), me(900.0)).get(), 0.0);
    }

    /// The screen-space ramp on its own, unchanged by the arrival of the air
    /// term: `0` at and below `near`, `strength` at and beyond `far`, linear
    /// between, and safe for a degenerate range.
    #[test]
    fn the_normalized_depth_ramp_alone_is_what_it_always_was() {
        let fog = FrameDepthFog::new(r(0.5), r(1.0), r(0.8), [0.5; 3]);
        assert_eq!(fog.mix_fraction(nd(0.0), me(0.0)).get(), 0.0, "before near: clear");
        assert_eq!(fog.mix_fraction(nd(0.5), me(0.0)).get(), 0.0, "at near: clear");
        assert!((fog.mix_fraction(nd(0.75), me(0.0)).get() - 0.4).abs() < 1.0e-6, "halfway");
        assert!((fog.mix_fraction(nd(1.0), me(0.0)).get() - 0.8).abs() < 1.0e-6, "at far");
        assert!((fog.mix_fraction(nd(9.0), me(0.0)).get() - 0.8).abs() < 1.0e-6, "past far");
        // An inverted (and a zero-width) range is floored, not divided by zero.
        let inverted = FrameDepthFog::new(r(1.0), r(0.5), r(1.0), [0.0; 3]);
        assert!(inverted.mix_fraction(nd(0.75), me(0.0)).get().is_finite());
        let degenerate = FrameDepthFog::new(r(0.5), r(0.5), r(1.0), [0.0; 3]);
        assert_eq!(degenerate.mix_fraction(nd(0.75), me(0.0)).get(), 1.0);
        assert_eq!(degenerate.mix_fraction(nd(0.25), me(0.0)).get(), 0.0);
        // A strength outside 0..1 is a ceiling, clamped rather than trusted.
        let hot = FrameDepthFog::new(r(0.0), r(1.0), r(4.0), [0.0; 3]);
        assert_eq!(hot.mix_fraction(nd(1.0), me(0.0)).get(), 1.0);
        let cold = FrameDepthFog::new(r(0.0), r(1.0), r(-2.0), [0.0; 3]);
        assert_eq!(cold.mix_fraction(nd(1.0), me(0.0)).get(), 0.0);
    }

    /// The defect this parameter exists to remove, measured rather than asserted
    /// in prose. NDC depth is hyperbolic in world distance, so a ramp that is
    /// linear in it front-loads its whole range into the near field however the
    /// window is chosen. The depths below are the real ones for a `1.2 m` near /
    /// `1650 m` far frustum — `z = f*(d-n) / (d*(f-n))` — and the statistic is
    /// the share of the total haze range delivered inside the first `100 m` of a
    /// ground plane that runs to `1200 m`.
    #[test]
    fn the_depth_window_front_loads_its_whole_range_and_extinction_does_not() {
        let ndc = |metres: f32| {
            let (n, f) = (1.2_f32, 1650.0_f32);
            f * (metres - n) / (metres * (f - n))
        };
        let share = |f: &FrameDepthFog, air: bool| {
            let at = |d: f32| f.mix_fraction(nd(ndc(d)), me(d * f32::from(air))).get();
            (at(100.0) - at(25.0)) / (at(1200.0) - at(25.0))
        };
        // The widest window that still reaches full density inside the scene —
        // the most generous shape this unit can be given.
        let window = FrameDepthFog::new(r(0.95), r(0.9999), r(1.0), [0.7; 3]);
        let front_loaded = share(&window, false);
        assert!(
            front_loaded > 0.7,
            "the window spends {front_loaded} of its range in the first 100 m of 1200"
        );
        // Beer-Lambert over the same span, at a 250 m half-distance: the haze is
        // laid down at a constant fraction per metre, so the near field takes its
        // proportionate share and no more.
        // `near = far = 1.0` parks the screen ramp at zero for every depth in
        // front of the far plane, isolating the air term.
        let air = FrameDepthFog::new(r(1.0), r(1.0), r(1.0), [0.7; 3]).with_extinction(r(0.004));
        let spread = share(&air, true);
        assert!(
            spread < 0.3,
            "extinction spends {spread} of its range in the first 100 m"
        );
        // ...and it separates every pair of distances the window collapses.
        [25.0_f32, 100.0, 400.0, 1200.0]
            .windows(2)
            .for_each(|w| {
                let (a, b) = (
                    air.mix_fraction(nd(0.0), me(w[0])).get(),
                    air.mix_fraction(nd(0.0), me(w[1])).get(),
                );
                assert!(b - a > 0.05, "{a} -> {b} at {w:?} is not a ramp");
            });
        // Checked against Beer-Lambert's own definition: half in at the
        // half-distance, three quarters at twice it.
        assert!((air.mix_fraction(nd(0.0), me(250.0)).get() - 0.5).abs() < 1.0e-5);
        assert!((air.mix_fraction(nd(0.0), me(500.0)).get() - 0.75).abs() < 1.0e-5);
        // Behind the camera is not negative air, and neither is a negative rate.
        assert_eq!(air.mix_fraction(nd(0.0), me(-40.0)).get(), 0.0);
        let backwards =
            FrameDepthFog::new(r(1.0), r(1.0), r(1.0), [0.7; 3]).with_extinction(r(-0.004));
        assert_eq!(backwards.mix_fraction(nd(0.0), me(400.0)).get(), 0.0);
    }

    /// The two terms compose as independent extinction, and a backend that
    /// cannot evaluate the air term (Canvas 2D — it passes distance `0`) gets
    /// exactly the ramp it had before, bit for bit.
    #[test]
    fn the_two_terms_compose_and_the_substitute_is_the_ramp_alone() {
        let fog = FrameDepthFog::new(r(0.5), r(1.0), r(1.0), [0.5; 3])
            .with_extinction(r(0.004));
        // screen = 0.5 at ndc 0.75; air = 1 - 2^-1 = 0.5 at 250 m.
        // combined = 1 - 0.5*0.5 = 0.75.
        assert!((fog.mix_fraction(nd(0.75), me(250.0)).get() - 0.75).abs() < 1.0e-5);
        // Either term alone reaches the ceiling on its own.
        assert!((fog.mix_fraction(nd(1.0), me(0.0)).get() - 1.0).abs() < 1.0e-6);
        // The declared substitute: distance 0 kills the air term and leaves the
        // ramp identical to the same fog with no rate authored at all.
        let ramp_only = FrameDepthFog::new(r(0.5), r(1.0), r(1.0), [0.5; 3]);
        [0.0_f32, 0.4, 0.6, 0.9, 1.0].into_iter().for_each(|z| {
            assert_eq!(
                fog.mix_fraction(nd(z), me(0.0)).get(),
                ramp_only.mix_fraction(nd(z), me(0.0)).get(),
                "the substitute must be bit-identical at ndc {z}"
            );
        });
    }
}
