//! The evaluation context — every external input a field may read, supplied
//! explicitly.

use axiom_kernel::Seconds;
use axiom_math::{Vec2, Vec3};

/// What the four context-source operators ([`crate::FieldOp::Point`],
/// [`crate::FieldOp::Uv`], [`crate::FieldOp::Normal`], [`crate::FieldOp::Time`])
/// read.
///
/// **There is no ambient anything.** Every external input is handed in by the
/// caller, which is what makes a field a pure function and its evaluation
/// replayable. `time` is a kernel [`Seconds`] the caller supplies — never a wall
/// clock, which the Determinism Rules forbid and `engine_no_time_in_sim` would
/// catch inside a `#[sim]` zone anyway.
///
/// **Coordinate spaces are not typed; they are contextual.** `point` is whatever
/// space the caller supplies, and the caller documents it. Moving between spaces
/// is an explicit [`crate::FieldOp::Transform`] node. Adding a *space type*
/// would put scene semantics into the primitive, which is exactly the
/// contamination this layer exists to avoid.
///
/// **There is no randomness here.** The only stochastic-looking operators are
/// `Noise` and `Fbm`, which are pure functions of `(seed, point)` where the seed
/// is a graph parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvalContext {
    point: Vec3,
    uv: Vec2,
    normal: Vec3,
    time: Seconds,
}

impl EvalContext {
    /// The context at the origin: a zero point, a zero uv, a `+Y` normal and
    /// zero time. The neutral starting point a caller overrides.
    pub const ORIGIN: EvalContext = EvalContext {
        point: Vec3::ZERO,
        uv: Vec2::ZERO,
        normal: Vec3::UNIT_Y,
        time: Seconds::finite_or_zero(0.0),
    };

    /// A context from its four explicit inputs.
    pub const fn new(point: Vec3, uv: Vec2, normal: Vec3, time: Seconds) -> Self {
        EvalContext {
            point,
            uv,
            normal,
            time,
        }
    }

    /// The sample position, in whatever space the caller supplied.
    pub const fn point(self) -> Vec3 {
        self.point
    }

    /// The surface parameterisation. Origin `(0, 0)` is the lower-left.
    pub const fn uv(self) -> Vec2 {
        self.uv
    }

    /// The surface normal, expected to be unit length.
    pub const fn normal(self) -> Vec3 {
        self.normal
    }

    /// The presentation time the caller supplied.
    pub const fn time(self) -> Seconds {
        self.time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_reports_every_input_it_was_given() {
        let context = EvalContext::new(
            Vec3::new(1.0, 2.0, 3.0),
            Vec2::new(0.25, 0.75),
            Vec3::UNIT_X,
            Seconds::finite_or_zero(1.5),
        );
        assert_eq!(context.point(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(context.uv(), Vec2::new(0.25, 0.75));
        assert_eq!(context.normal(), Vec3::UNIT_X);
        assert_eq!(context.time().get(), 1.5);
    }

    #[test]
    fn the_origin_context_is_the_neutral_starting_point() {
        assert_eq!(EvalContext::ORIGIN.point(), Vec3::ZERO);
        assert_eq!(EvalContext::ORIGIN.uv(), Vec2::ZERO);
        assert_eq!(EvalContext::ORIGIN.normal(), Vec3::UNIT_Y);
        assert_eq!(EvalContext::ORIGIN.time().get(), 0.0);
        assert_eq!(
            EvalContext::ORIGIN,
            EvalContext::new(
                Vec3::ZERO,
                Vec2::ZERO,
                Vec3::UNIT_Y,
                Seconds::finite_or_zero(0.0)
            )
        );
    }

    #[test]
    fn contexts_differing_in_any_input_are_different() {
        let moved = EvalContext::new(
            Vec3::UNIT_Z,
            Vec2::ZERO,
            Vec3::UNIT_Y,
            Seconds::finite_or_zero(0.0),
        );
        assert_ne!(EvalContext::ORIGIN, moved);
    }
}
