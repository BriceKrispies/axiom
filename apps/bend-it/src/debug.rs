//! The debug view: entirely out of the player's way, and the only practical way
//! to diagnose a trajectory bug.
//!
//! A wrong trajectory looks, from the outside, exactly like a right one — the
//! ball goes somewhere and either the keeper reaches it or it does not. So this
//! turns the invisible into geometry: every sample of the canonical path, the
//! two projections drawn flat on the turf and up the goal plane, the authored
//! endpoint, the keeper's read and the reach it actually swept, and the state
//! machine's own text.
//!
//! It is off by default, costs one bool to check, and never appears in the
//! shipping frame.

use axiom::prelude::{Transform, Vec3};
use axiom_math::Quat;

use crate::pitch::PENALTY_SPOT_Z;
use crate::play::Session;
use crate::stroke::Reading;

/// One marker to draw. `alternate` picks the second (keeper-coloured) pool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugMarker {
    pub transform: Transform,
    pub alternate: bool,
}

fn dot(at: Vec3, size: f32, alternate: bool) -> DebugMarker {
    DebugMarker {
        transform: Transform::new(at, Quat::IDENTITY, Vec3::new(size, size, size)),
        alternate,
    }
}

/// A capsule axis drawn as a run of beads, so a reach can be *seen* rather than
/// inferred.
fn segment(a: Vec3, b: Vec3, beads: usize, size: f32, alternate: bool, out: &mut Vec<DebugMarker>) {
    (0..=beads).for_each(|i| {
        let t = i as f32 / beads.max(1) as f32;
        out.push(dot(a.add(b.subtract(a).mul_scalar(t)), size, alternate));
    });
}

/// Build this tick's markers.
///
/// Everything the design brief asks a debug mode to show is here, and each piece
/// is drawn where it actually is in the world rather than as a schematic:
/// the sampled 3D path, its horizontal projection flattened onto the turf, its
/// vertical projection pushed out to the goal plane, the authored endpoint, and
/// the keeper's chosen interception point and swept reach.
pub fn markers(session: &Session, reading: Option<&Reading>, out: &mut Vec<DebugMarker>) {
    out.clear();
    let trajectory = &session.shot().trajectory;
    let points = trajectory.points();
    let step = (points.len() / 40).max(1);

    // The sampled 3D path.
    points
        .iter()
        .step_by(step)
        .for_each(|p| out.push(dot(*p, 0.07, false)));

    // The horizontal projection: the same path, flattened onto the turf. Seeing
    // it there is how a lateral bug is told apart from a height bug.
    points.iter().step_by(step).for_each(|p| {
        out.push(dot(Vec3::new(p.x, 0.02, p.z), 0.05, false));
    });

    // The vertical projection: the same path with its lateral offset removed,
    // pushed out to the side of the goal.
    points.iter().step_by(step).for_each(|p| {
        out.push(dot(
            Vec3::new(crate::pitch::PITCH_HALF_WIDTH * 0.22, p.y, p.z),
            0.05,
            false,
        ));
    });

    // The authored endpoint.
    out.push(dot(session.shot().world_target, 0.16, false));

    // What the last drawing was actually read as, in the world: the points the
    // line was understood to pass through. Seeing these next to the fitted path
    // is the only way to tell "the fit is wrong" from "I drew that".
    reading.into_iter().for_each(|r| {
        r.read_points
            .iter()
            .for_each(|p| out.push(dot(*p, 0.09, true)));
        out.push(dot(r.raw_target, 0.18, true));
    });

    // The keeper: where it thinks the ball is going, and the reach it is
    // actually sweeping.
    if let Some(read) = session.keeper().read() {
        out.push(dot(crate::play::keeper::drawable_prediction(read.predicted), 0.20, true));
        out.push(dot(read.aim, 0.13, true));
    }
    let frame = session.keeper().frame(&session.tuning().keeper);
    segment(frame.reach.a, frame.reach.b, 10, 0.07, true, out);
    segment(frame.body.a, frame.body.b, 4, 0.09, true, out);
}

/// The overlay rows: the state machine, the shot's own parameters, and the
/// keeper's decision, as text.
pub fn rows(session: &Session, reading: Option<&Reading>) -> Vec<(String, String)> {
    let intent = session.intent();
    let (bend_effort, loft_effort) = intent.effort(session.tuning());
    let (bend_size, loft_size) = intent.shape.reach();
    let (bend_at, loft_at) = intent.shape.peak_at();
    let shot = session.shot();
    let mut rows = vec![
        (
            "phase".into(),
            format!("{:?} +{}", session.phase(), session.phase_tick()),
        ),
        (
            "target".into(),
            format!(
                "h {:+.2} v {:.2}  ->  ({:+.2}, {:.2})",
                intent.target.h, intent.target.v, shot.world_target.x, shot.world_target.y
            ),
        ),
        (
            "bend".into(),
            format!("{bend_size:+.2} m at u={bend_at:.2} ({:.0}%)", bend_effort * 100.0),
        ),
        (
            "loft".into(),
            format!("{loft_size:+.2} m at u={loft_at:.2} ({:.0}%)", loft_effort * 100.0),
        ),
        (
            "flight".into(),
            format!(
                "{:.2} s over {:.1} m ({} samples)",
                shot.trajectory.duration(),
                shot.trajectory.length(),
                shot.trajectory.points().len()
            ),
        ),
        (
            "ball".into(),
            format!(
                "{:?} at ({:+.2}, {:.2}, {:.2})",
                session.ball().motion,
                session.ball().position.x,
                session.ball().position.y,
                session.ball().position.z
            ),
        ),
    ];
    rows.push((
        "drawing".into(),
        reading
            .map(|r| {
                format!(
                    "{} points kept, raw finish ({:+.2}, {:.2})",
                    r.read_points.len(),
                    r.raw_target.x,
                    r.raw_target.y
                )
            })
            .unwrap_or_else(|| "none yet".into()),
    ));
    rows.push((
        "nerve".into(),
        session.keeper().nerve().describe(),
    ));
    rows.push((
        "keeper".into(),
        session
            .keeper()
            .read()
            .map(|r| {
                format!(
                    "read ({:+.2}, {:.2}) aim ({:+.2}, {:.2}) bias {:+.2}",
                    r.predicted.x, r.predicted.y, r.aim.x, r.aim.y, r.height_bias
                )
            })
            .unwrap_or_else(|| "set".into()),
    ));
    rows.push((
        "result".into(),
        session
            .result()
            .map(|r| r.banner().to_string())
            .unwrap_or_else(|| "-".into()),
    ));
    rows.push((
        "tally".into(),
        format!(
            "{} / {}",
            session.tally().goals,
            session.tally().attempts
        ),
    ));
    rows
}

/// A sanity check the debug view exists to make cheap: the path really does span
/// the pitch from the spot to the goal line.
pub fn path_spans_the_shot(session: &Session) -> bool {
    let points = session.shot().trajectory.points();
    points
        .first()
        .zip(points.last())
        .map(|(a, b)| ((a.z - PENALTY_SPOT_Z).abs() < 0.01) & (b.z.abs() < 0.01))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{Phase, PlayCommand, Session};
    use crate::shot::{BendCurve, GoalTarget, ShotIntent};
    use crate::tuning::Tuning;

    fn sculpted() -> Session {
        let mut s = Session::new(Tuning::DEFAULT);
        while s.phase() != Phase::Aiming {
            s.step(&[]);
        }
        s.step(&[PlayCommand::Kick(ShotIntent::curved(GoalTarget::new(-0.6, 0.7), BendCurve::through(0.6, 1.6, 0.14), BendCurve::through(0.5, 1.0, 0.14), crate::stroke::Pace::STEADY))]);
        s
    }

    #[test]
    fn the_markers_cover_every_thing_the_brief_asks_to_see() {
        let session = sculpted();
        let mut out = Vec::new();
        markers(&session, None, &mut out);
        assert!(out.len() > 40, "the sampled path is drawn");
        // The 3D path, its flattened shadow on the turf, and its side elevation
        // are three distinct sets of markers.
        assert!(out.iter().any(|m| m.transform.translation.y > 0.3));
        assert!(out
            .iter()
            .any(|m| (m.transform.translation.y - 0.02).abs() < 1.0e-4));
        assert!(out
            .iter()
            .any(|m| m.transform.translation.x > crate::pitch::PITCH_HALF_WIDTH * 0.2));
        // The keeper's own geometry is in the alternate pool.
        assert!(out.iter().any(|m| m.alternate));
        // Rebuilding replaces rather than appends.
        let first = out.len();
        markers(&session, None, &mut out);
        assert_eq!(out.len(), first);
    }

    #[test]
    fn the_rows_report_the_state_machine_and_the_shot_parameters() {
        let session = sculpted();
        let rows = rows(&session, None);
        let names: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        [
            "phase", "target", "bend", "loft", "flight", "ball", "drawing", "nerve",
            "keeper", "result", "tally",
        ]
            .iter()
            .for_each(|k| assert!(names.contains(k), "missing row {k}"));
        assert!(rows.iter().any(|(k, v)| k == "phase" && v.contains("ShotReady")));
        assert!(rows.iter().any(|(k, v)| k == "keeper" && v == "set"));
        assert!(rows.iter().any(|(k, v)| k == "result" && v == "-"));
    }

    #[test]
    fn the_keeper_row_fills_in_once_it_has_committed() {
        let mut session = sculpted();
        let mut n = 0;
        while session.result().is_none() && n < 600 {
            session.step(&[]);
            n += 1;
        }
        assert!(rows(&session, None).iter().any(|(k, v)| k == "keeper" && v.contains("aim")));
        assert!(rows(&session, None).iter().any(|(k, v)| k == "result" && v != "-"));
        assert_eq!(session.phase(), Phase::Resolution);
    }

    #[test]
    fn the_path_always_spans_the_spot_to_the_goal_line() {
        assert!(path_spans_the_shot(&sculpted()));
        assert!(path_spans_the_shot(&Session::new(Tuning::DEFAULT)));
    }
}
