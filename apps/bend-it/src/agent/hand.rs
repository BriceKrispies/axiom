//! The striker's hand: turning a decided shape into a line on the glass.
//!
//! This is the whole reason the agent counts as *playing* the game rather than
//! configuring it. The control law upstairs decides a shape; here that shape is
//! drawn — as pixels, with a hand that is not quite steady — and from this point
//! the game has no way to tell it from a thumb.

use axiom::prelude::Vec2;

use crate::play::Session;
use crate::projection::ScreenProjection;
use crate::shot::{BendCurve, GoalTarget, ResolvedShot, ShotIntent};
use crate::stroke::Stroke;

use super::{Striker, HAND_SAMPLES, HAND_TREMOR};

impl Striker {
    /// The line this striker would draw — its entire output, in screen pixels.
    ///
    /// The agent decides a *shape*; this turns that shape into the picture of it a
    /// finger would leave, tremor and all. From here the game cannot tell the
    /// difference between this and a hand.
    pub fn stroke_for(
        &mut self,
        session: &Session,
        projection: &ScreenProjection,
    ) -> Option<Stroke> {
        let wanted = self.wanted_shot(session);
        let points: Vec<Vec2> = (0..HAND_SAMPLES)
            .filter_map(|i| {
                let u = i as f32 / (HAND_SAMPLES - 1) as f32;
                projection
                    .project(wanted.trajectory.at_progress(u))
                    .map(|p| p.add(self.tremor(i)))
            })
            .collect();
        (points.len() >= 3).then(|| Stroke::from_points(points))
    }

    /// A small, deterministic unsteadiness in the drawing hand.
    fn tremor(&self, index: usize) -> Vec2 {
        let a = ((index as u64 * 7 + self.seed * 13) % 11) as f32 - 5.0;
        let b = ((index as u64 * 5 + self.seed * 3) % 9) as f32 - 4.0;
        Vec2::new(a * HAND_TREMOR * 0.2, b * HAND_TREMOR * 0.2)
    }

    /// The shot the agent has decided it wants, as an actual flight it can trace.
    pub(super) fn wanted_shot(&mut self, session: &Session) -> ResolvedShot {
        let axes = self.decide(session);
        let tuning = session.tuning();
        ResolvedShot::build(
            session.shot().origin,
            ShotIntent {
                target: GoalTarget::new(axes.aim_h, axes.aim_v),
                bend: BendCurve::through(
                    axes.break_at,
                    axes.bend * tuning.bend.max_offset,
                    tuning.bend.peak_margin,
                ),
                loft: BendCurve::through(
                    axes.break_at,
                    axes.loft * tuning.loft.max_offset,
                    tuning.loft.peak_margin,
                ),
            },
            session.mouth(),
            tuning,
        )
    }

}
