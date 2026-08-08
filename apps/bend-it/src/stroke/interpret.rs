//! Reading a drawn line as a shot.
//!
//! This is the whole game now. The player draws one line; the kicker looks at it
//! and takes the closest shot it is actually capable of. Two things make that a
//! mechanic rather than a lottery:
//!
//! **It is a fit, not a parse.** The drawing is projected back into the world and
//! then **least-squares fitted** onto the space of legal shots — the two Bézier
//! weights per projection that `shot::curve` defines. A clean banana gives a
//! banana. A shaky line gives the smooth shot nearest to it. A scribble gives the
//! best single shot that scribble is evidence for. Nothing is rejected, because
//! "do its best with what I drew" is the promise.
//!
//! **It is exact.** The model is linear in the two weights, so the fit is a 2×2
//! normal-equation solve in closed form — no search, no iteration, no tolerance.
//! The same pixels always produce the same kick, on any machine.
//!
//! ```text
//! drawn pixels
//!   → last point  → ray → goal plane → CLAMPED into the mouth   the target
//!   → each point  → how far along the shot it sits (u)
//!                 → ray → the plane at that depth               a world point
//!   → residual from the straight ball→target line
//!   → least squares → (w1, w2) lateral  and  (w1, w2) vertical
//!   → ShotIntent
//! ```
//!
//! The endpoint is clamped into the goal, so — exactly as before — the shot is
//! **valid by construction** and nothing downstream has to steer it there.

use axiom::prelude::{Vec2, Vec3};

use crate::pitch::GoalMouth;
use crate::projection::ScreenProjection;
use crate::shot::{shot_right, BendCurve, GoalTarget, ShotIntent};
use crate::tuning::Tuning;

use super::fit::{fit, offsets_at, path_point, progress_at_fraction, ruler_lengths};
use super::line::Stroke;

/// How many evenly-spaced points the drawing is read at.
const FIT_SAMPLES: usize = 28;
/// How finely the straight shot is projected to serve as the ruler.
const BASE_SAMPLES: usize = 48;

/// One reading of a drawing, kept whole so the debug view can show its working.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The shot the kicker took from it.
    pub intent: ShotIntent,
    /// Where the drawing put the finish, before clamping.
    pub raw_target: Vec3,
    /// The world points the drawing was read as, in shot order.
    pub read_points: Vec<Vec3>,
    /// How far the drawn line strayed from the shot that was fitted to it,
    /// metres — how much the kicker had to "do its best".
    pub residual: f32,
}

/// Read a drawing as a shot. `None` when there is not enough line to read.
pub fn interpret(
    stroke: &Stroke,
    projection: &ScreenProjection,
    ball: Vec3,
    mouth: &GoalMouth,
    tuning: &Tuning,
) -> Option<Reading> {
    let short_edge = projection.viewport().x.min(projection.viewport().y);
    let enough = stroke.length() >= short_edge * tuning.stroke.min_length;
    let goal_on_screen = projection.project(Vec3::new(0.0, mouth.ceiling() * 0.5, 0.0))?;
    let drawn = stroke.oriented(goal_on_screen);
    let samples = drawn.resampled(FIT_SAMPLES);
    let finish = *samples.last()?;
    enough.then_some(())?;

    // Where the line finishes is where the ball finishes — clamped into the
    // mouth, which is what keeps every shot legal however wildly it was drawn.
    let raw_target = projection.goal_plane_hit(finish)?;
    let (h, v) = mouth.to_normalized(raw_target);
    let target = GoalTarget::new(h, v);
    let world_target = mouth.to_world(target.h, target.v);

    // The ruler every deviation is measured against, as a *polyline with known
    // progress at every vertex* rather than two endpoints — because perspective
    // is not linear: the world midpoint of a shot lands far closer to the goal on
    // the screen than halfway. Measuring against a straight screen segment
    // mis-assigns how far along the shot each drawn point is, and the fit
    // inherits that error as a bend and a lift nobody drew.
    let right = shot_right(ball, world_target);
    let ruler = |bend: &BendCurve, loft: &BendCurve| -> Vec<(f32, Vec2)> {
        (0..=BASE_SAMPLES)
            .filter_map(|i| {
                let u = i as f32 / BASE_SAMPLES as f32;
                let at = path_point(ball, world_target, right, bend, loft, u);
                projection.project(at).map(|screen| (u, screen))
            })
            .collect()
    };

    // Two passes. The first measures the drawing against the *straight* shot,
    // which is the only ruler available before anything is known. The second
    // measures it against the shot the first pass found — and that matters most
    // exactly where the first pass is worst, on a drawing that bows a long way
    // from straight. Two passes is enough; the ruler is already the right shape
    // by then, and every pass is the same closed-form solve.
    let first = read_and_fit(&ruler(&BendCurve::STRAIGHT, &BendCurve::STRAIGHT), &samples,
        projection, ball, world_target, tuning)?;
    let (bend, loft, read) =
        read_and_fit(&ruler(&first.0, &first.1), &samples, projection, ball, world_target, tuning)
            .unwrap_or(first);

    // How far the drawing strayed from the shot fitted to it: the leftover the
    // kicker had to "do its best" with, in metres.
    let residual = read
        .iter()
        .map(|(u, offset)| {
            let dx = offset.x - bend.offset(*u);
            let dy = offset.y - loft.offset(*u);
            (dx * dx + dy * dy).sqrt()
        })
        .sum::<f32>()
        / read.len().max(1) as f32;

    Some(Reading {
        intent: ShotIntent {
            target,
            bend,
            loft,
        },
        raw_target,
        read_points: read
            .into_iter()
            .map(|(u, offset)| {
                let base = ball.add(world_target.subtract(ball).mul_scalar(u));
                base.add(right.mul_scalar(offset.x))
                    .add(Vec3::new(0.0, offset.y, 0.0))
            })
            .collect(),
        residual,
    })
}

fn read_and_fit(
    ruler: &[(f32, Vec2)],
    samples: &[Vec2],
    projection: &ScreenProjection,
    ball: Vec3,
    world_target: Vec3,
    tuning: &Tuning,
) -> Option<(BendCurve, BendCurve, Vec<(f32, Vec3)>)> {
    (ruler.len() >= 2).then_some(())?;
    let lengths = ruler_lengths(ruler);
    let last = samples.len().saturating_sub(1).max(1) as f32;
    let right = shot_right(ball, world_target);
    let read: Vec<(f32, Vec3)> = samples
        .iter()
        .enumerate()
        .filter_map(|(i, point)| {
            // The samples are evenly spaced along the drawing, so the index IS
            // the drawn-length fraction.
            let u = progress_at_fraction(ruler, &lengths, i as f32 / last);
            offsets_at(projection, ball, world_target, right, u, *point)
                .map(|(across, up)| (u, Vec3::new(across, up, 0.0)))
        })
        .collect();
    (read.len() >= 3).then_some(())?;

    // How much of the shot the drawing actually has an opinion about. A line that
    // runs the whole way is evidence; a flick over the first fifth is a hint, and
    // is regularised as one.
    let spread = read
        .iter()
        .fold((1.0f32, 0.0f32), |(lo, hi), (u, _)| (lo.min(*u), hi.max(*u)));
    let ridge = tuning.stroke.ridge * (1.05 - (spread.1 - spread.0).clamp(0.0, 1.0)).max(0.06);

    let lateral: Vec<(f32, f32)> = read.iter().map(|(u, o)| (*u, o.x)).collect();
    let vertical: Vec<(f32, f32)> = read.iter().map(|(u, o)| (*u, o.y)).collect();
    Some((
        fit(&lateral, ridge).bounded(tuning.bend.min_offset, tuning.bend.max_offset),
        fit(&vertical, ridge).bounded(tuning.loft.min_offset, tuning.loft.max_offset),
        read,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera;
    use crate::pitch::ball_spot;
    use crate::shot::ResolvedShot;

    fn setup() -> (ScreenProjection, Vec3, GoalMouth, Tuning) {
        let tuning = Tuning::DEFAULT;
        let mouth = GoalMouth::new(tuning.goal.inset);
        let ball = ball_spot(tuning.flight.ball_radius);
        let viewport = Vec2::new(390.0, 844.0);
        let pose = camera::frame(
            viewport,
            &mouth,
            ball,
            Vec3::new(-1.04, 0.0, 14.2),
            0.0,
            &tuning.camera,
        );
        (ScreenProjection::new(&pose, viewport), ball, mouth, tuning)
    }

    /// Draw the screen-space picture of an actual shot — the drawing a player
    /// would make if they traced the flight they wanted.
    fn trace(shot: &ResolvedShot, projection: &ScreenProjection, count: usize) -> Stroke {
        Stroke::from_points(
            (0..count)
                .filter_map(|i| {
                    let u = i as f32 / (count - 1) as f32;
                    projection.project(shot.trajectory.at_progress(u))
                })
                .collect(),
        )
    }

    fn shot_of(bend: f32, bend_at: f32, loft: f32, loft_at: f32, h: f32, v: f32) -> ResolvedShot {
        let (_, ball, mouth, tuning) = setup();
        ResolvedShot::build(
            ball,
            ShotIntent {
                target: GoalTarget::new(h, v),
                bend: BendCurve::through(bend_at, bend, 0.14),
                loft: BendCurve::through(loft_at, loft, 0.14),
            },
            &mouth,
            &tuning,
        )
    }

    #[test]
    fn tracing_a_shot_reads_back_as_that_shot() {
        let (projection, ball, mouth, tuning) = setup();
        for (bend, bend_at, loft, loft_at, h, v) in [
            (0.0f32, 0.5f32, 3.0f32, 0.5f32, 0.0f32, 0.40f32),
            (1.7, 0.62, 0.9, 0.5, -0.7, 0.35),
            (-1.5, 0.35, 2.2, 0.55, 0.6, 0.8),
        ] {
            let original = shot_of(bend, bend_at, loft, loft_at, h, v);
            let drawing = trace(&original, &projection, 40);
            let reading = interpret(&drawing, &projection, ball, &mouth, &tuning)
                .expect("a traced flight is a readable drawing");
            let rebuilt = ResolvedShot::build(ball, reading.intent, &mouth, &tuning);
            assert!(
                rebuilt.world_target.subtract(original.world_target).length() < 0.30,
                "aim drifted: {:?} vs {:?}",
                rebuilt.world_target,
                original.world_target
            );
            // The rebuilt path follows the original closely all the way down.
            let worst = (0..=20)
                .map(|i| {
                    let u = i as f32 / 20.0;
                    rebuilt
                        .trajectory
                        .at_progress(u)
                        .subtract(original.trajectory.at_progress(u))
                        .length()
                })
                .fold(0.0f32, f32::max);
            assert!(worst < 0.55, "the read-back path strays by {worst} m");
            assert!(reading.residual < 0.15, "and it fitted cleanly");
        }
    }

    #[test]
    fn a_small_arc_straight_down_the_middle_is_the_one_thing_the_camera_cannot_see() {
        // An honest limitation, recorded so it cannot regress into a mystery.
        //
        // The camera sits on the shot's own centre line, so every world point
        // with `x = 0` projects onto the *same* vertical line on screen. A shot
        // aimed dead centre therefore draws an identical picture whether it is
        // flat or gently arced: there is no drawing a player could make to tell
        // the two apart, and the reading correctly refuses to invent one.
        //
        // It costs nothing in play. A real lob leaves that line — it climbs above
        // the goal and drops back into it, and a line that doubles back *is*
        // readable (see the trace test above, which reads a 3 m centred lob back
        // at better than 90%). Only the difference between "flat" and "slightly
        // arced" down the exact middle is lost, and those two shots arrive in the
        // same place at nearly the same time anyway.
        let (projection, ball, mouth, tuning) = setup();
        let gentle = shot_of(0.0, 0.5, 0.6, 0.5, 0.0, 0.5);
        let drawing = trace(&gentle, &projection, 40);
        // The picture really is a straight vertical line: that is the limitation,
        // stated in pixels.
        let points = drawing.points();
        let spread = points
            .iter()
            .map(|p| (p.x - points[0].x).abs())
            .fold(0.0f32, f32::max);
        assert!(spread < 0.5, "a centred shot draws a vertical line: {spread} px");
        let reading = interpret(&drawing, &projection, ball, &mouth, &tuning).expect("readable");
        // What IS read is exactly what the picture contains: the finish.
        let rebuilt = ResolvedShot::build(ball, reading.intent, &mouth, &tuning);
        assert!(rebuilt.world_target.subtract(gentle.world_target).length() < 0.20);
        assert!(reading.intent.bend.magnitude().abs() < 0.15);
    }

    #[test]
    fn the_same_drawing_always_produces_the_same_kick() {
        let (projection, ball, mouth, tuning) = setup();
        let drawing = trace(&shot_of(1.4, 0.6, 1.2, 0.5, -0.5, 0.6), &projection, 33);
        let a = interpret(&drawing, &projection, ball, &mouth, &tuning).expect("readable");
        let b = interpret(&drawing, &projection, ball, &mouth, &tuning).expect("readable");
        assert_eq!(a.intent, b.intent);
        assert_eq!(a.read_points, b.read_points);
        // ... and how fast it was drawn changes nothing.
        let hurried = trace(&shot_of(1.4, 0.6, 1.2, 0.5, -0.5, 0.6), &projection, 9);
        let c = interpret(&hurried, &projection, ball, &mouth, &tuning).expect("readable");
        assert!((c.intent.bend.magnitude() - a.intent.bend.magnitude()).abs() < 0.35);
    }

    #[test]
    fn a_straight_drawing_is_a_straight_shot() {
        let (projection, ball, mouth, tuning) = setup();
        let corner = mouth.to_world(0.75, 0.55);
        let from = projection.project(ball).expect("on screen");
        let to = projection.project(corner).expect("on screen");
        let straight = Stroke::from_points(
            (0..30)
                .map(|i| {
                    let t = i as f32 / 29.0;
                    from.add(to.subtract(from).mul_scalar(t))
                })
                .collect(),
        );
        let reading = interpret(&straight, &projection, ball, &mouth, &tuning).expect("readable");
        assert!(
            reading.intent.bend.magnitude().abs() < 0.25,
            "a straight line bent by {}",
            reading.intent.bend.magnitude()
        );
        assert!(reading.intent.loft.magnitude().abs() < 0.25);
        assert!(reading.residual < 0.15, "and it fitted cleanly");
    }

    #[test]
    fn which_way_the_line_bows_is_which_way_the_shot_bends() {
        let (projection, ball, mouth, tuning) = setup();
        let bow = |sideways: f32| {
            let from = projection.project(ball).expect("on");
            let to = projection.project(mouth.to_world(0.0, 0.5)).expect("on");
            Stroke::from_points(
                (0..30)
                    .map(|i| {
                        let t = i as f32 / 29.0;
                        let mid = from.add(to.subtract(from).mul_scalar(t));
                        Vec2::new(mid.x + sideways * (t * (1.0 - t) * 4.0), mid.y)
                    })
                    .collect(),
            )
        };
        let right = interpret(&bow(90.0), &projection, ball, &mouth, &tuning).expect("readable");
        let left = interpret(&bow(-90.0), &projection, ball, &mouth, &tuning).expect("readable");
        assert!(right.intent.bend.magnitude() > 0.5, "drawn right, bends right");
        assert!(left.intent.bend.magnitude() < -0.5, "drawn left, bends left");
    }

    #[test]
    fn where_the_line_bows_is_where_the_shot_breaks() {
        let (projection, ball, mouth, tuning) = setup();
        let bow_at = |peak: f32| {
            let from = projection.project(ball).expect("on");
            let to = projection.project(mouth.to_world(0.0, 0.5)).expect("on");
            Stroke::from_points(
                (0..30)
                    .map(|i| {
                        let t = i as f32 / 29.0;
                        let mid = from.add(to.subtract(from).mul_scalar(t));
                        // A bump centred on `peak`.
                        let d = ((t - peak) / 0.30).clamp(-1.0, 1.0);
                        Vec2::new(mid.x + 80.0 * (1.0 - d * d), mid.y)
                    })
                    .collect(),
            )
        };
        let early = interpret(&bow_at(0.3), &projection, ball, &mouth, &tuning).expect("readable");
        let late = interpret(&bow_at(0.7), &projection, ball, &mouth, &tuning).expect("readable");
        assert!(
            early.intent.bend.peak().0 < late.intent.bend.peak().0 - 0.1,
            "early peak {} should precede late peak {}",
            early.intent.bend.peak().0,
            late.intent.bend.peak().0
        );
    }

    #[test]
    fn the_finish_is_always_inside_the_goal_however_wildly_it_is_drawn() {
        let (projection, ball, mouth, tuning) = setup();
        // Lines that finish miles outside the frame, above it, and below it.
        for finish in [
            Vec2::new(-4000.0, 100.0),
            Vec2::new(4000.0, 100.0),
            Vec2::new(195.0, -3000.0),
            Vec2::new(195.0, 300.0),
        ] {
            let from = projection.project(ball).expect("on");
            let wild = Stroke::from_points(
                (0..24)
                    .map(|i| {
                        let t = i as f32 / 23.0;
                        from.add(finish.subtract(from).mul_scalar(t))
                    })
                    .collect(),
            );
            let Some(reading) = interpret(&wild, &projection, ball, &mouth, &tuning) else {
                continue;
            };
            let target = mouth.to_world(reading.intent.target.h, reading.intent.target.v);
            assert!(target.x.abs() <= mouth.half_width() + 1.0e-4, "{target:?}");
            assert!(target.y >= mouth.floor() - 1.0e-4);
            assert!(target.y <= mouth.ceiling() + 1.0e-4);
        }
    }

    #[test]
    fn a_tap_or_a_flick_too_short_to_mean_anything_is_not_a_shot() {
        let (projection, ball, mouth, tuning) = setup();
        assert!(interpret(&Stroke::new(), &projection, ball, &mouth, &tuning).is_none());
        let tap = Stroke::from_points(vec![Vec2::new(200.0, 600.0), Vec2::new(203.0, 598.0)]);
        assert!(interpret(&tap, &projection, ball, &mouth, &tuning).is_none());
    }

    #[test]
    fn a_line_drawn_from_the_goal_back_to_the_ball_reads_the_same() {
        let (projection, ball, mouth, tuning) = setup();
        let original = shot_of(1.4, 0.6, 1.0, 0.5, -0.6, 0.6);
        let forward = trace(&original, &projection, 30);
        let mut backward_points = forward.points().to_vec();
        backward_points.reverse();
        let backward = Stroke::from_points(backward_points);
        let a = interpret(&forward, &projection, ball, &mouth, &tuning).expect("readable");
        let b = interpret(&backward, &projection, ball, &mouth, &tuning).expect("readable");
        assert_eq!(a.intent.target, b.intent.target);
        assert!((a.intent.bend.magnitude() - b.intent.bend.magnitude()).abs() < 0.05);
    }

    #[test]
    fn a_shaky_line_still_yields_a_smooth_shot() {
        let (projection, ball, mouth, tuning) = setup();
        let original = shot_of(1.2, 0.55, 1.0, 0.5, 0.4, 0.5);
        let clean = trace(&original, &projection, 40);
        // The same line with a deterministic tremor on it.
        let shaky = Stroke::from_points(
            clean
                .points()
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let wobble = ((i * 37 % 11) as f32 - 5.0) * 2.4;
                    Vec2::new(p.x + wobble, p.y - wobble * 0.5)
                })
                .collect(),
        );
        let steady = interpret(&clean, &projection, ball, &mouth, &tuning).expect("readable");
        let wobbly = interpret(&shaky, &projection, ball, &mouth, &tuning).expect("readable");
        assert!(
            (steady.intent.bend.magnitude() - wobbly.intent.bend.magnitude()).abs() < 0.5,
            "a tremor should not change the shot much"
        );
        assert!(wobbly.residual > steady.residual, "but it is measurably rougher");
        // Whatever was drawn, the shot that comes out is a legal one.
        let rebuilt = ResolvedShot::build(ball, wobbly.intent, &mouth, &tuning);
        assert_eq!(rebuilt.trajectory.points()[0], ball);
        assert!(rebuilt
            .trajectory
            .points()
            .iter()
            .all(|p| p.y >= tuning.flight.ball_radius - 1.0e-4));
    }
}

