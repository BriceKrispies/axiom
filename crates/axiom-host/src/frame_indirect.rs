//! Backend-neutral **two-band indirect fill** for a frame: a cool skylight band
//! from above, a warm bounce band from the ground below, and the budget the
//! image-based diffuse term is scaled by.
//!
//! This is the term that stops geometry the key light does not reach from
//! collapsing to black. A hemisphere ambient ([`crate::FrameAmbient`]) already
//! lights unlit faces, but it is a single `mix` between two colours by the
//! normal's up-component — it cannot express *"a vertical wall genuinely sees
//! half the sky dome"*, and it carries no warm ground bounce at all. Those are
//! two different bands with two different gates, and they are what a shaded
//! facade is actually lit by outdoors.
//!
//! Carried as neutral frame data, like [`crate::FrameAmbient`] and
//! [`crate::FrameVolumetrics`], so a backend that can evaluate the bands does
//! and one that cannot degrades to the hemisphere alone rather than each
//! hardcoding a fill of its own.
//!
//! # Why the bands are frame data and not derived from the ambient
//!
//! It is tempting to compute the fill from the hemisphere colours the frame
//! already carries. It would be wrong: the two are authored from different
//! quantities. The reference this is ported from (`render/index.js:1133-1147`)
//! takes the cool band from the **sky's own published irradiance** — so that a
//! night frame lit by a 0.05 moon is not scaled to nothing — and the warm band
//! from the **key light's colour through a ground albedo**. One is the sky, the
//! other is the sun off the road. A single hemisphere pair cannot say both.

/// The two-band indirect fill: a cool upper band, a warm lower band, and the
/// scale applied to image-based diffuse.
///
/// The band colours are **level-folded** — the authored tint times its level —
/// exactly as [`crate::FrameAmbient`]'s are strength-folded, so a backend
/// applies them directly with no extra multiply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameIndirect {
    sky_fill: [f32; 3],
    ground_fill: [f32; 3],
    fill_gain: [f32; 2],
    ibl_diffuse: f32,
    interior_floor: f32,
}

impl FrameIndirect {
    /// A fill from its level-folded band colours and the two gains.
    ///
    /// `fill_gain` is `(band gain, bounce gain)`: the first scales both bands
    /// together, the second scales the warm sun-bounce wrap on top of the lower
    /// band. `ibl_diffuse` scales whatever image-based diffuse the backend has
    /// (an exact zero where it has none, which multiplies out). `interior_floor`
    /// is the indirect level inside a closed interior volume — skylight does not
    /// reach the middle of a room, and without a floor a doorway reads as a hole
    /// cut in a card.
    pub const fn new(
        sky_fill: [f32; 3],
        ground_fill: [f32; 3],
        fill_gain: [f32; 2],
        ibl_diffuse: f32,
        interior_floor: f32,
    ) -> Self {
        FrameIndirect {
            sky_fill,
            ground_fill,
            fill_gain,
            ibl_diffuse,
            interior_floor,
        }
    }

    /// **No fill at all** — every band black, both gains zero, no IBL budget.
    ///
    /// The identity: a frame carrying this renders exactly as a frame carrying
    /// no fill did before this type existed, because every term it contributes
    /// is a multiply by or an add of zero. This is what a backend uses for a
    /// frame that authors none, so the feature costs nothing until it is asked
    /// for.
    pub const fn none() -> Self {
        FrameIndirect::new([0.0; 3], [0.0; 3], [0.0; 2], 0.0, 0.0)
    }

    /// The cool upper band (level-folded linear RGB).
    pub const fn sky_fill(&self) -> [f32; 3] {
        self.sky_fill
    }

    /// The warm lower band (level-folded linear RGB).
    pub const fn ground_fill(&self) -> [f32; 3] {
        self.ground_fill
    }

    /// `(band gain, bounce gain)`.
    pub const fn fill_gain(&self) -> [f32; 2] {
        self.fill_gain
    }

    /// The scale on image-based diffuse.
    pub const fn ibl_diffuse(&self) -> f32 {
        self.ibl_diffuse
    }

    /// The indirect level inside a closed interior volume.
    pub const fn interior_floor(&self) -> f32 {
        self.interior_floor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let f = FrameIndirect::new([0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [1.0, 0.62], 0.03, 0.035);
        assert_eq!(f.sky_fill(), [0.1, 0.2, 0.3]);
        assert_eq!(f.ground_fill(), [0.4, 0.5, 0.6]);
        assert_eq!(f.fill_gain(), [1.0, 0.62]);
        assert_eq!(f.ibl_diffuse(), 0.03);
        assert_eq!(f.interior_floor(), 0.035);
    }

    /// **The identity, stated as an identity.** Every lane of `none()` is the
    /// value that makes its term vanish: the bands are added, so they are zero;
    /// the gains multiply the sum, so they are zero too; the IBL budget scales a
    /// term, so it is zero. A frame carrying this cannot move a pixel, which is
    /// what makes the fill free for every app that does not author one.
    #[test]
    fn none_is_the_no_op_in_every_lane() {
        let n = FrameIndirect::none();
        assert_eq!(n.sky_fill(), [0.0; 3]);
        assert_eq!(n.ground_fill(), [0.0; 3]);
        assert_eq!(n.fill_gain(), [0.0; 2]);
        assert_eq!(n.ibl_diffuse(), 0.0);
        assert_eq!(n.interior_floor(), 0.0);
    }

    #[test]
    fn is_copy_and_comparable() {
        let a = FrameIndirect::none();
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, FrameIndirect::new([1.0; 3], [0.0; 3], [0.0; 2], 0.0, 0.0));
    }
}
