//! The maths behind the reading.
//!
//! Split from [`super::interpret`] because "what a drawing means" and "the solve
//! that extracts it" are different jobs, and the solve is the part that has to be
//! exactly right rather than merely sensible. Everything here is closed form and
//! deterministic: no search, no iteration, no tolerance.

use axiom::prelude::{Vec2, Vec3};

use crate::projection::ScreenProjection;
use crate::shot::ShotPath;

/// A point on a shot: the straight spine, plus the two offsets.
pub(super) fn path_point(
    ball: Vec3,
    target: Vec3,
    right: Vec3,
    shape: &ShotPath,
    u: f32,
) -> Vec3 {
    let base = ball.add(target.subtract(ball).mul_scalar(u));
    let (across, up) = shape.at(u);
    base.add(right.mul_scalar(across)).add(Vec3::new(0.0, up, 0.0))
}

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
    shape: &ShotPath,
    u: f32,
    drawn: Vec2,
) -> Option<(f32, f32)> {
    // The basis is built where the flight currently *is*, not on the straight
    // line — which matters because this is a linearisation. One metre across at
    // the spine and one metre across three metres above it do not project to the
    // same screen vector, so anchoring the solve at the spine and then asking it
    // about a point three metres away is asking it to extrapolate exactly where
    // it is least entitled to. A least-squares fit used to absorb the error; now
    // that the drawing is kept, nothing does, so the anchor has to be right.
    //
    // Each pass re-anchors on the shape the last one read, which makes this a
    // Newton step: the offsets it returns are relative to that shape, and adding
    // them back gives the next estimate.
    let base = ball.add(target.subtract(ball).mul_scalar(u));
    let (was_across, was_up) = shape.at(u);
    let anchor = base
        .add(right.mul_scalar(was_across))
        .add(Vec3::new(0.0, was_up, 0.0));
    let origin = projection.project(anchor)?;
    let across = projection.project(anchor.add(right))?.subtract(origin);
    let up = projection.project(anchor.add(Vec3::UNIT_Y))?.subtract(origin);
    let offset = drawn.subtract(origin);
    // Solve `offset = a·across + b·up`.
    let det = across.x * up.y - across.y * up.x;
    (det.abs() > 1.0e-5).then(|| {
        (
            was_across + (offset.x * up.y - offset.y * up.x) / det,
            was_up + (across.x * offset.y - across.y * offset.x) / det,
        )
    })
}
