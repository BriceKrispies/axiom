//! The maths behind the reading.
//!
//! Split from [`super::interpret`] because "what a drawing means" and "the solve
//! that extracts it" are different jobs, and the solve is the part that has to be
//! exactly right rather than merely sensible. Everything here is closed form and
//! deterministic: no search, no iteration, no tolerance.

use axiom::prelude::{Vec2, Vec3};

use crate::projection::ScreenProjection;
use crate::shot::BendCurve;

/// A point on a shot: the straight spine, plus the two offsets.
pub(super) fn path_point(
    ball: Vec3,
    target: Vec3,
    right: Vec3,
    bend: &BendCurve,
    loft: &BendCurve,
    u: f32,
) -> Vec3 {
    let base = ball.add(target.subtract(ball).mul_scalar(u));
    base.add(right.mul_scalar(bend.offset(u)))
        .add(Vec3::new(0.0, loft.offset(u), 0.0))
}

/// Read the drawing against one ruler and fit both curves to it.
#[allow(clippy::type_complexity)]
/// How far along the shot the drawing is, at drawn-length fraction `s`.
///
/// **Not** by nearest approach, and this is the subtlest thing in the file. The
/// camera looks almost straight down the shot, so on screen "further away" and
/// "higher" are very nearly the same direction. A ruler that assigns progress by
/// perpendicular foot therefore reads every bit of *lift* in a drawing as
/// *distance* instead, and the height of an arc vanishes entirely — the fit comes
/// back flat and, worse, comes back flat with a perfect residual.
///
/// So progress is taken along the *length of the drawing*, converted through the
/// ruler's own screen length. That is unambiguous — arc length has no
/// perpendicular component to absorb — and it is perspective-correct, because the
/// ruler is a projection of a real path and its screen length is compressed at
/// the far end in exactly the way the drawing's is.
pub(super) fn progress_at_fraction(ruler: &[(f32, Vec2)], lengths: &[f32], s: f32) -> f32 {
    let total = lengths.last().copied().unwrap_or(0.0);
    let want = s.clamp(0.0, 1.0) * total;
    let index = lengths
        .iter()
        .position(|walked| *walked >= want)
        .unwrap_or(lengths.len() - 1)
        .max(1);
    let (before, after) = (lengths[index - 1], lengths[index]);
    let t = ((want - before) / (after - before).max(1.0e-6)).clamp(0.0, 1.0);
    let (u0, _) = ruler[index - 1];
    let (u1, _) = ruler[index];
    u0 + (u1 - u0) * t
}

/// Cumulative screen length along a ruler.
pub(super) fn ruler_lengths(ruler: &[(f32, Vec2)]) -> Vec<f32> {
    ruler
        .windows(2)
        .scan(0.0f32, |walked, pair| {
            *walked += pair[1].1.subtract(pair[0].1).length();
            Some(*walked)
        })
        .fold(vec![0.0f32], |mut out, walked| {
            out.push(walked);
            out
        })
}

/// How far a drawn point sits from the straight shot at progress `u`, in metres
/// **across** and metres **up**.
///
/// This is the other half of the depth problem, and it is why nothing here
/// unprojects. A ray cast from a screen point has to be stopped at *some* depth,
/// and getting that depth slightly wrong turns straight into bent — badly, when
/// the camera is looking down the shot. So the offset is never unprojected at
/// all. Instead the two directions the game actually cares about are projected
/// *forward*: one metre across and one metre up, at this point of the flight,
/// become two screen vectors. The drawn point's offset from the straight shot is
/// then solved in that basis — an exact 2×2 — and comes back as the two numbers a
/// shot is made of, with no depth guess anywhere in it.
pub(super) fn offsets_at(
    projection: &ScreenProjection,
    ball: Vec3,
    target: Vec3,
    right: Vec3,
    u: f32,
    drawn: Vec2,
) -> Option<(f32, f32)> {
    let base = ball.add(target.subtract(ball).mul_scalar(u));
    let origin = projection.project(base)?;
    let across = projection.project(base.add(right))?.subtract(origin);
    let up = projection.project(base.add(Vec3::UNIT_Y))?.subtract(origin);
    let offset = drawn.subtract(origin);
    // Solve `offset = a·across + b·up`.
    let det = across.x * up.y - across.y * up.x;
    (det.abs() > 1.0e-5).then(|| {
        (
            (offset.x * up.y - offset.y * up.x) / det,
            (across.x * offset.y - across.y * offset.x) / det,
        )
    })
}

/// The least-squares curve through `(progress, offset)` samples.
///
/// The offset model is linear in its two weights, so this is a 2×2 normal-
/// equation solve — closed form, no iteration, and identical everywhere. `ridge`
/// pulls the answer toward straight; the caller scales it by how much of the
/// shot the drawing covered, so a line drawn all the way to the goal is taken at
/// its word while a flick over the first fifth is treated as the hint it is.
pub(super) fn fit(samples: &[(f32, f32)], ridge: f32) -> BendCurve {
    let ridge = ridge.max(1.0e-4) * samples.len() as f32;
    let (mut a11, mut a12, mut a22, mut b1, mut b2) = (ridge, 0.0f32, ridge, 0.0f32, 0.0f32);
    samples.iter().for_each(|(u, d)| {
        let v = 1.0 - u;
        let (p1, p2) = (3.0 * v * v * u, 3.0 * v * u * u);
        a11 += p1 * p1;
        a12 += p1 * p2;
        a22 += p2 * p2;
        b1 += p1 * d;
        b2 += p2 * d;
    });
    let det = a11 * a22 - a12 * a12;
    match det.abs() < 1.0e-9 {
        true => BendCurve::STRAIGHT,
        false => BendCurve {
            w1: (b1 * a22 - b2 * a12) / det,
            w2: (a11 * b2 - a12 * b1) / det,
        },
    }
}

