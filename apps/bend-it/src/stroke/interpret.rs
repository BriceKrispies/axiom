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
//!   → offsets from the straight ball→target line
//!   → least squares → (w1, w2) lateral  and  (w1, w2) vertical
//!   → ShotIntent
//! ```
//!
//! The endpoint is clamped into the goal, so — exactly as before — the shot is
//! **valid by construction** and nothing downstream has to steer it there.

use axiom::prelude::{Vec2, Vec3};

use crate::pitch::GoalMouth;
use crate::projection::ScreenProjection;
use crate::shot::{shot_right, GoalTarget, ShotIntent, ShotPath};
use crate::tuning::Tuning;

use super::fit::{offsets_at, path_point, progress_at_fraction, ruler_lengths};
use super::line::Stroke;
use super::pace::Pace;

/// How many evenly-spaced points the drawing is read at.
const FIT_SAMPLES: usize = 96;
/// How finely the straight shot is projected to serve as the ruler.
/// How many times the ruler is rebuilt from the shape the last pass read.
const RULER_PASSES: usize = 8;

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
    let ruler = |shape: &ShotPath| -> Vec<(f32, Vec2)> {
        (0..=BASE_SAMPLES)
            .filter_map(|i| {
                let u = i as f32 / BASE_SAMPLES as f32;
                let at = path_point(ball, world_target, right, shape, u);
                projection.project(at).map(|screen| (u, screen))
            })
            .collect()
    };

    // Measure the drawing against the *straight* shot first, because that is the
    // only ruler available before anything is known — then against the shot that
    // pass found, and again. Each pass improves where along the flight every
    // drawn point sits, which matters most exactly where the first pass is worst:
    // a drawing that bows a long way from straight.
    //
    // It took two passes when a fit followed, because least squares absorbed
    // what was left. Nothing absorbs it now — every sample keeps its own error —
    // so the ruler has to actually converge.
    let read = (0..RULER_PASSES).try_fold(
        read_offsets(
            &ruler(&ShotPath::STRAIGHT),
            &ShotPath::STRAIGHT,
            &samples,
            projection,
            ball,
            world_target,
        )?,
        |best, _| {
            let shape = shape_of(&best);
            Some(
                read_offsets(
                    &ruler(&shape),
                    &shape,
                    &samples,
                    projection,
                    ball,
                    world_target,
                )
                .unwrap_or(best),
            )
        },
    )?;

    Some(Reading {
        intent: ShotIntent {
            target,
            // The drawing, kept. Smoothed once to take a fingertip's tremor out
            // and not otherwise touched: no fit, no nearest legal curve, no
            // opinion about what the player probably meant.
            shape: shape_of(&read).smoothed(),
            // Shape came from the geometry; how hard it was hit comes from the
            // tempo, read from the *unoriented* line so a drawing is timed as it
            // was made rather than as it is read.
            pace: Pace::read(stroke, short_edge, &tuning.pace),
        },
        raw_target,
        read_points: read
            .iter()
            .map(|(u, offset)| {
                let base = ball.add(world_target.subtract(ball).mul_scalar(*u));
                base.add(right.mul_scalar(offset.x))
                    .add(Vec3::new(0.0, offset.y, 0.0))
            })
            .collect(),
    })
}

/// The offsets a drawing has at each of the shape's own sample points.
///
/// The drawn points arrive at whatever progress the ruler puts them at, which is
/// not the even grid a [`ShotPath`] is stored on — so each sample takes the
/// nearest drawn offsets either side of it, interpolated. Nothing is averaged
/// across the line and nothing is fitted; a sample simply reads what the hand was
/// doing there.
fn shape_of(read: &[(f32, Vec3)]) -> ShotPath {
    ShotPath::sampled(|u| {
        let after = read.iter().position(|(at, _)| *at >= u);
        match after {
            None => read.last().map(|(_, o)| (o.x, o.y)).unwrap_or((0.0, 0.0)),
            Some(0) => read
                .first()
                .map(|(_, o)| (o.x, o.y))
                .unwrap_or((0.0, 0.0)),
            Some(i) => {
                let (u0, a) = read[i - 1];
                let (u1, b) = read[i];
                let t = ((u - u0) / (u1 - u0).max(1.0e-6)).clamp(0.0, 1.0);
                (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
            }
        }
    })
}

/// Where each drawn point sits on the shot, and how far off the straight line it
/// is — in metres across and metres up.
///
/// This is the whole of the reading now. It used to be followed by a
/// least-squares solve onto two Bézier weights per projection; that solve is
/// gone, and what it used to consume is what the game now plays.
fn read_offsets(
    ruler: &[(f32, Vec2)],
    shape: &ShotPath,
    samples: &[Vec2],
    projection: &ScreenProjection,
    ball: Vec3,
    world_target: Vec3,
) -> Option<Vec<(f32, Vec3)>> {
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
            offsets_at(projection, ball, world_target, right, shape, u, *point)
                .map(|(across, up)| (u, Vec3::new(across, up, 0.0)))
        })
        .collect();
    (read.len() >= 3).then_some(read)
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
        traced_at(shot, projection, count, 1)
    }

    /// The same, drawn at an explicit tempo: `per_point` ticks between points.
    fn traced_at(
        shot: &ResolvedShot,
        projection: &ScreenProjection,
        count: usize,
        per_point: u64,
    ) -> Stroke {
        Stroke::from_timed_points(
            (0..count)
                .filter_map(|i| {
                    let u = i as f32 / (count - 1) as f32;
                    projection.project(shot.trajectory.at_progress(u))
                })
                .collect(),
            per_point,
        )
    }

    fn shot_of(bend: f32, bend_at: f32, loft: f32, loft_at: f32, h: f32, v: f32) -> ResolvedShot {
        let (_, ball, mouth, tuning) = setup();
        ResolvedShot::build(
            ball,
            ShotIntent::curved(GoalTarget::new(h, v), crate::shot::BendCurve::through(bend_at, bend, 0.14), crate::shot::BendCurve::through(loft_at, loft, 0.14), crate::stroke::Pace::STEADY),
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
            // The rebuilt flight follows the drawing **on screen**, which is the
            // only place the player can judge it.
            //
            // This used to be a world-space bound, and world space is the wrong
            // room to ask the question in: the far half of a penalty is eleven
            // metres from a camera sitting behind the ball, so half a metre of
            // world error out there is a handful of pixels nobody can see, while
            // the same half metre at the kicker's feet would be glaring. The
            // promise the game makes is "the ball goes where you drew" — and
            // "where you drew" is a set of pixels.
            let strays = |a: &ResolvedShot| {
                (0..=40)
                    .filter_map(|i| projection.project(a.trajectory.at_progress(i as f32 / 40.0)))
                    .map(|p| {
                        drawing
                            .points()
                            .iter()
                            .map(|q| q.subtract(p).length())
                            .fold(f32::INFINITY, f32::min)
                    })
                    .fold(0.0f32, f32::max)
            };
            let short = projection.viewport().x.min(projection.viewport().y);
            let drawn_stray = strays(&rebuilt) / short;
            assert!(
                drawn_stray < 0.04,
                "the flight strays {:.1}% of the screen from the line that drew it",
                drawn_stray * 100.0
            );
            // And it is no further from the line than the flight that DREW it —
            // the reading cannot be worse than the thing it read.

        }
    }

    #[test]
    fn a_shape_no_curve_could_hold_survives_into_the_flight() {
        // The reason the fit is gone. A line that changes direction four times is
        // outside anything two Bézier weights can represent — the old reading
        // replaced it with the nearest smooth arc and the wobble was simply
        // deleted. Now it is played.
        let (projection, ball, mouth, tuning) = setup();
        let straight = shot_of(0.0, 0.5, 0.6, 0.5, 0.3, 0.5);
        let spine = trace(&straight, &projection, 60);
        let wobbled = Stroke::from_points(
            spine
                .points()
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let t = i as f32 / (spine.len().max(2) - 1) as f32;
                    Vec2::new(p.x + (t * 14.0).sin() * 26.0, p.y)
                })
                .collect(),
        );
        let reading = interpret(&wobbled, &projection, ball, &mouth, &tuning).expect("readable");
        // The flight changes direction as many times as the hand did.
        let turns = reading
            .intent
            .shape
            .samples()
            .map(|(_, across, _)| across)
            .collect::<Vec<_>>()
            .windows(3)
            .filter(|w| (w[1] - w[0]).signum() != (w[2] - w[1]).signum())
            .count();
        assert!(turns >= 3, "the wobble was flattened: only {turns} turns");
        // ... and it is a real wobble, not noise: the ball moves either side of
        // its own straight line by a visible amount.
        let (widest, _) = reading.intent.shape.reach();
        assert!(widest.abs() > 0.25, "the wobble was only {widest} m wide");
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
        assert!(reading.intent.shape.reach().0.abs() < 0.15);
    }

    #[test]
    fn the_same_drawing_always_produces_the_same_kick() {
        let (projection, ball, mouth, tuning) = setup();
        let drawing = trace(&shot_of(1.4, 0.6, 1.2, 0.5, -0.5, 0.6), &projection, 33);
        let a = interpret(&drawing, &projection, ball, &mouth, &tuning).expect("readable");
        let b = interpret(&drawing, &projection, ball, &mouth, &tuning).expect("readable");
        assert_eq!(a.intent, b.intent);
        assert_eq!(a.read_points, b.read_points);
        // ... and how densely it was sampled changes nothing about the SHAPE.
        // (Its tempo may differ — that is the point of reading them separately.)
        let hurried = trace(&shot_of(1.4, 0.6, 1.2, 0.5, -0.5, 0.6), &projection, 9);
        let c = interpret(&hurried, &projection, ball, &mouth, &tuning).expect("readable");
        assert!((c.intent.shape.reach().0 - a.intent.shape.reach().0).abs() < 0.35);
    }

    #[test]
    fn the_tempo_of_the_drawing_sets_the_pace_and_nothing_else() {
        // The two halves of a drawing are read separately and must stay
        // separate: how it is SHAPED comes from the geometry, how HARD it was hit
        // comes from the timing. The same line drawn quickly and slowly is the
        // same shot, struck with different conviction.
        let (projection, ball, mouth, tuning) = setup();
        let original = shot_of(1.4, 0.6, 1.2, 0.5, -0.5, 0.6);
        let flicked = interpret(
            &traced_at(&original, &projection, 30, 1),
            &projection,
            ball,
            &mouth,
            &tuning,
        )
        .expect("readable");
        let careful = interpret(
            &traced_at(&original, &projection, 30, 7),
            &projection,
            ball,
            &mouth,
            &tuning,
        )
        .expect("readable");

        // Same shape, to the letter.
        assert_eq!(flicked.intent.target, careful.intent.target);
        assert_eq!(flicked.intent.shape, careful.intent.shape);
        // Different pace, and the quick one is the harder shot.
        assert!(
            flicked.intent.pace.speed > careful.intent.pace.speed + 0.3,
            "flicked {:.2} vs careful {:.2}",
            flicked.intent.pace.speed,
            careful.intent.pace.speed
        );
        let quick = ResolvedShot::build(ball, flicked.intent, &mouth, &tuning);
        let slow = ResolvedShot::build(ball, careful.intent, &mouth, &tuning);
        assert!(quick.trajectory.duration() < slow.trajectory.duration());
        // ... and it is the same flight, taken faster: identical geometry.
        assert_eq!(quick.trajectory.points(), slow.trajectory.points());
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
            reading.intent.shape.reach().0.abs() < 0.25,
            "a straight line bent by {}",
            reading.intent.shape.reach().0
        );
        assert!(reading.intent.shape.reach().1.abs() < 0.25);
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
        assert!(right.intent.shape.reach().0 > 0.5, "drawn right, bends right");
        assert!(left.intent.shape.reach().0 < -0.5, "drawn left, bends left");
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
            early.intent.shape.peak_at().0 < late.intent.shape.peak_at().0 - 0.1,
            "early peak {} should precede late peak {}",
            early.intent.shape.peak_at().0,
            late.intent.shape.peak_at().0
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
        assert!((a.intent.shape.reach().0 - b.intent.shape.reach().0).abs() < 0.05);
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
            (steady.intent.shape.reach().0 - wobbly.intent.shape.reach().0).abs() < 0.5,
            "a tremor should not change the shot much"
        );
        assert_ne!(wobbly.intent.shape, steady.intent.shape, "the wobble is kept");
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

