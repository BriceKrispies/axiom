//! The per-frame paint table: registered linear/radial gradients a command's
//! [`crate::Fill2d`] references by [`PaintId`], never inlining stops.

use axiom_kernel::{Meters, Ratio};
use axiom_math::Vec2;

use crate::handles::PaintId;
use crate::rgba::Rgba;

/// One stop in a gradient: an `offset` along the gradient axis and a `color`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: Ratio,
    pub color: Rgba,
}

impl GradientStop {
    /// Construct a stop from its offset and colour.
    pub const fn new(offset: Ratio, color: Rgba) -> Self {
        GradientStop { offset, color }
    }
}

/// The linear-gradient geometry (private payload).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Linear2d {
    from: Vec2,
    to: Vec2,
}

/// The radial-gradient geometry (private payload).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Radial2d {
    center: Vec2,
    radius: Meters,
}

/// One registered paint: a linear **or** radial gradient with its stops. Which
/// arm is `Some` is the paint's kind — no separate discriminant is stored. Kept
/// crate-internal; the public surface names paints only by [`PaintId`] and
/// inspects them through [`crate::Draw2dList`]'s paint accessors.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Paint2d {
    linear: Option<Linear2d>,
    radial: Option<Radial2d>,
    stops: Vec<GradientStop>,
}

impl Paint2d {
    pub(crate) fn linear(from: Vec2, to: Vec2, stops: Vec<GradientStop>) -> Self {
        Paint2d {
            linear: Some(Linear2d { from, to }),
            radial: None,
            stops,
        }
    }

    pub(crate) fn radial(center: Vec2, radius: Meters, stops: Vec<GradientStop>) -> Self {
        Paint2d {
            linear: None,
            radial: Some(Radial2d { center, radius }),
            stops,
        }
    }

    pub(crate) fn as_linear(&self) -> Option<(Vec2, Vec2)> {
        self.linear.map(|l| (l.from, l.to))
    }

    pub(crate) fn as_radial(&self) -> Option<(Vec2, Meters)> {
        self.radial.map(|r| (r.center, r.radius))
    }

    pub(crate) fn stops(&self) -> &[GradientStop] {
        &self.stops
    }

    /// Sample the gradient's colour at parameter `u` (clamped to `[0, 1]`).
    ///
    /// This is the paint's **canonical colour ramp** — the contract's own
    /// definition of what a linear/radial gradient looks like, independent of any
    /// framebuffer. It lives here, in the neutral layer, so both render backends
    /// (Canvas 2D, GPU) sample the *identical* gradient rather than each
    /// re-deriving it (the "shared primitive belongs in a lower layer" rule).
    ///
    /// Evaluated branchlessly as a telescoping sum over the stop list, sorted by
    /// offset: starting from the first stop's colour, each adjacent pair adds
    /// `(next − prev) · clamp((u − prev.offset)/(next.offset − prev.offset), 0, 1)`.
    /// Before the first stop every term is `0` (the first colour); past the last,
    /// every term saturates to `1` and the sum telescopes to the last colour; in
    /// between it is the piecewise-linear interpolation. An empty stop list is
    /// fully transparent; a single stop is that solid colour. A zero-width segment
    /// (duplicate offsets) is floored by [`RAMP_EPS`] so the divide stays finite.
    pub(crate) fn sample(&self, u: f32) -> [f32; 4] {
        let u = u.clamp(0.0, 1.0);
        let base = self
            .stops
            .first()
            .map(|s| s.color.channels())
            .unwrap_or([0.0; 4]);
        self.stops.windows(2).fold(base, |acc, pair| {
            let a = pair[0];
            let b = pair[1];
            let denom = (b.offset.get() - a.offset.get()).max(RAMP_EPS);
            let t = ((u - a.offset.get()) / denom).clamp(0.0, 1.0);
            let ca = a.color.channels();
            let cb = b.color.channels();
            [
                acc[0] + (cb[0] - ca[0]) * t,
                acc[1] + (cb[1] - ca[1]) * t,
                acc[2] + (cb[2] - ca[2]) * t,
                acc[3] + (cb[3] - ca[3]) * t,
            ]
        })
    }

    /// Bake this paint into its canonical sampling **texture** as
    /// `(width, height, RGBA8 bytes)`: a linear gradient bakes an `n×1` colour
    /// ramp (sampled along the projection parameter), a radial gradient bakes an
    /// `n×n` disc whose texel `(i, j)` is the gradient at the radius of that
    /// texel's centre mapped into `[-1, 1]²` (so a backend samples it with the
    /// affine UV `((p − center)/radius)·0.5 + 0.5`). Both backends upload/sample
    /// the *same* bytes with nearest filtering, so the gradient is byte-identical
    /// across backends. Branchless: the radial-vs-linear height and per-texel
    /// parameter are table/`select` choices, never an `if`.
    pub(crate) fn bake_texture(&self, n: u32) -> (u32, u32, Vec<u8>) {
        let n = n.max(1);
        let is_radial = usize::from(self.radial.is_some());
        let height = [1, n][is_radial];
        let nf = n as f32;
        let bytes: Vec<u8> = (0..(n * height))
            .flat_map(|idx| {
                let i = (idx % n) as f32;
                let j = (idx / n) as f32;
                let linear_u = (i + 0.5) / nf;
                let rx = (i + 0.5) / nf * 2.0 - 1.0;
                let ry = (j + 0.5) / nf * 2.0 - 1.0;
                let radial_u = (rx * rx + ry * ry).sqrt();
                let u = [linear_u, radial_u][is_radial];
                let c = self.sample(u);
                [to_byte(c[0]), to_byte(c[1]), to_byte(c[2]), to_byte(c[3])]
            })
            .collect();
        (n, height, bytes)
    }
}

/// Smallest gradient-segment width used to floor the interpolation divide so a
/// duplicate-offset stop pair never divides by zero (yielding a non-finite that
/// would poison the telescoping sum).
const RAMP_EPS: f32 = 1.0e-6;

/// Linear `0.0..=1.0` channel → clamped, rounded RGBA8 byte — the same rounding
/// the software framebuffer and the GPU's `Rgba8Unorm` quantization apply, so a
/// baked texel matches a directly-composited colour within ±1.
fn to_byte(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// The per-frame collection of registered paints, keyed by a zero-based
/// [`PaintId`]. Built by the facade as gradients are registered; carried on the
/// finished list so a backend can resolve every referenced paint.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintTable {
    paints: Vec<Paint2d>,
}

impl PaintTable {
    /// Register a paint, returning its zero-based id (its index in the table).
    pub(crate) fn register(&mut self, paint: Paint2d) -> PaintId {
        let id = PaintId::from_raw(self.paints.len() as u32);
        self.paints.push(paint);
        id
    }

    pub(crate) fn len(&self) -> usize {
        self.paints.len()
    }

    pub(crate) fn get(&self, id: PaintId) -> Option<&Paint2d> {
        self.paints.get(id.raw() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn stop(offset: f32) -> GradientStop {
        GradientStop::new(
            ratio(offset),
            Rgba::new(ratio(1.0), ratio(1.0), ratio(1.0), ratio(1.0)),
        )
    }

    #[test]
    fn gradient_stop_round_trips() {
        let s = stop(0.25);
        assert_eq!(s.offset, ratio(0.25));
    }

    #[test]
    fn linear_paint_exposes_linear_not_radial() {
        let p = Paint2d::linear(Vec2::ZERO, Vec2::new(1.0, 0.0), vec![stop(0.0), stop(1.0)]);
        assert_eq!(p.as_linear(), Some((Vec2::ZERO, Vec2::new(1.0, 0.0))));
        assert_eq!(p.as_radial(), None);
        assert_eq!(p.stops().len(), 2);
    }

    #[test]
    fn radial_paint_exposes_radial_not_linear() {
        let p = Paint2d::radial(Vec2::new(2.0, 3.0), meters(5.0), vec![stop(0.0)]);
        assert_eq!(p.as_radial(), Some((Vec2::new(2.0, 3.0), meters(5.0))));
        assert_eq!(p.as_linear(), None);
        assert_eq!(p.stops().len(), 1);
    }

    #[test]
    fn register_assigns_sequential_zero_based_ids_and_get_round_trips() {
        let mut table = PaintTable::default();
        let a = table.register(Paint2d::linear(Vec2::ZERO, Vec2::ONE, vec![stop(0.0)]));
        let b = table.register(Paint2d::radial(Vec2::ZERO, meters(1.0), vec![stop(1.0)]));
        assert_eq!(a, PaintId::from_raw(0));
        assert_eq!(b, PaintId::from_raw(1));
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(a).and_then(Paint2d::as_linear), Some((Vec2::ZERO, Vec2::ONE)));
        assert_eq!(
            table.get(b).and_then(Paint2d::as_radial),
            Some((Vec2::ZERO, meters(1.0)))
        );
    }

    #[test]
    fn get_unknown_id_is_none() {
        let table = PaintTable::default();
        assert_eq!(table.get(PaintId::from_raw(5)), None);
    }

    fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
        Rgba::new(ratio(r), ratio(g), ratio(b), ratio(a))
    }

    fn black_white() -> Paint2d {
        Paint2d::linear(
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            vec![
                GradientStop::new(ratio(0.0), rgba(0.0, 0.0, 0.0, 1.0)),
                GradientStop::new(ratio(1.0), rgba(1.0, 1.0, 1.0, 1.0)),
            ],
        )
    }

    #[test]
    fn sample_interpolates_endpoints_and_midpoint_and_clamps() {
        let p = black_white();
        assert_eq!(p.sample(0.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(p.sample(1.0), [1.0, 1.0, 1.0, 1.0]);
        // Midpoint is the halfway grey.
        assert_eq!(p.sample(0.5), [0.5, 0.5, 0.5, 1.0]);
        // Out-of-range u clamps to the endpoints (before-first / past-last arms).
        assert_eq!(p.sample(-2.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(p.sample(3.0), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn sample_of_empty_stops_is_transparent_and_single_stop_is_solid() {
        let empty = Paint2d::linear(Vec2::ZERO, Vec2::ONE, vec![]);
        assert_eq!(empty.sample(0.5), [0.0, 0.0, 0.0, 0.0]);
        let one = Paint2d::radial(
            Vec2::ZERO,
            meters(1.0),
            vec![GradientStop::new(ratio(0.25), rgba(0.2, 0.4, 0.6, 1.0))],
        );
        assert_eq!(one.sample(0.0), [0.2, 0.4, 0.6, 1.0]);
        assert_eq!(one.sample(1.0), [0.2, 0.4, 0.6, 1.0]);
    }

    #[test]
    fn sample_duplicate_offsets_stay_finite() {
        // Two stops at the same offset: the EPS-floored divide must not yield NaN.
        let p = Paint2d::linear(
            Vec2::ZERO,
            Vec2::ONE,
            vec![
                GradientStop::new(ratio(0.5), rgba(1.0, 0.0, 0.0, 1.0)),
                GradientStop::new(ratio(0.5), rgba(0.0, 1.0, 0.0, 1.0)),
            ],
        );
        assert!(p.sample(0.5).iter().all(|c| c.is_finite()));
    }

    #[test]
    fn bake_texture_linear_is_a_ramp_row_and_radial_is_a_disc() {
        // Linear → n×1 ramp; first texel is near-black, last near-white.
        let (lw, lh, lbytes) = black_white().bake_texture(8);
        assert_eq!((lw, lh), (8, 1));
        assert_eq!(lbytes.len(), 8 * 1 * 4);
        assert!(lbytes[0] < lbytes[(7 * 4)], "ramp brightens left→right");
        // Radial → n×n disc.
        let radial = Paint2d::radial(
            Vec2::ZERO,
            meters(1.0),
            vec![
                GradientStop::new(ratio(0.0), rgba(1.0, 1.0, 1.0, 1.0)),
                GradientStop::new(ratio(1.0), rgba(0.0, 0.0, 0.0, 1.0)),
            ],
        );
        let (rw, rh, rbytes) = radial.bake_texture(4);
        assert_eq!((rw, rh), (4, 4));
        assert_eq!(rbytes.len(), 4 * 4 * 4);
    }

    #[test]
    fn bake_texture_clamps_n_to_at_least_one_and_clamps_hdr_channels() {
        // n = 0 floors to 1; an HDR (>1) channel clamps to 255 in to_byte.
        let p = Paint2d::linear(
            Vec2::ZERO,
            Vec2::ONE,
            vec![GradientStop::new(ratio(0.0), rgba(2.5, 0.0, 0.0, 1.0))],
        );
        let (w, h, bytes) = p.bake_texture(0);
        assert_eq!((w, h), (1, 1));
        assert_eq!(bytes, vec![255, 0, 0, 255]);
    }
}
