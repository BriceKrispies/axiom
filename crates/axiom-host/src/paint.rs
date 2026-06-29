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
}
