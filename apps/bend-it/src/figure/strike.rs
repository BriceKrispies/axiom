//! The strike, as a physical swing: what the drawing asks the body for, and what
//! the leg actually does about it.
//!
//! The kick used to be a schedule. The boot met the ball on tick 24 because tick
//! 24 was written down, and a harder shot looked exactly like a softer one played
//! at the same speed.
//!
//! Now the leg is a **driven pendulum**. The hip applies a torque, the leg has
//! inertia and damping, and the swing is integrated at the fixed step like
//! anything else in the simulation. What comes out is not a schedule but a
//! consequence: a bigger torque reaches the ball **sooner** and **faster**, so
//! the contact tick is *solved* rather than declared, and the follow-through
//! carries the speed the leg genuinely had when it struck.
//!
//! Two things the drawing controls, and they are different things:
//!
//! * **how hard** — the tempo of the line becomes torque, run-up speed, and how
//!   much the body commits;
//! * **how** — the shape of the line becomes where the plant foot goes, how far
//!   the body leans, and how much the hips turn through the ball. A shot bent
//!   hard is struck across the ball with the body opened up; a lofted one is
//!   struck from further behind it with the body leaning away.

use crate::figure::model::{L_FOOT, L_SHIN, PARTS};
use crate::shot::ShotIntent;
use crate::tuning::{KickTuning, Tuning, DT};

/// What the drawing asks the body for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickDrive {
    /// Hip torque through the ball, newton-metres.
    pub torque: f32,
    /// How fast the run-up arrives, metres per second.
    pub approach: f32,
    /// Where the plant foot goes relative to the ball: metres to the side and
    /// metres behind it.
    pub plant_side: f32,
    pub plant_back: f32,
    /// How far the body leans away from the ball, radians.
    pub lean: f32,
    /// How far the hips turn through the strike, radians.
    pub turn: f32,
    /// How late the knee snaps straight, `0..1` — the whip.
    pub whip: f32,
    /// The signed bend the body is being asked to put on the ball, `-1..1`: which
    /// side of the ball the boot has to come across, and how far.
    pub across: f32,
}

impl KickDrive {
    /// Read the body's instructions off an authored shot.
    pub fn for_shot(intent: &ShotIntent, tuning: &Tuning) -> KickDrive {
        let k = &tuning.kick;
        let effort = intent.pace.speed.clamp(0.0, 1.0);
        let (_, loft_effort) = intent.effort(tuning);
        // Which way the shot bends decides which side of the ball the boot has to
        // come across, and therefore which side the body plants on.
        let across = intent.across(tuning);
        KickDrive {
            torque: hip_torque(intent.launch_speed(tuning), k),
            approach: k.base_approach * (1.0 + k.approach_from_pace * effort),
            // A shot bent hard is struck from wider, so the boot can come round
            // the ball rather than through it.
            plant_side: k.plant_side + k.plant_side_from_bend * across,
            // A lofted shot is struck from further behind, which is what gets a
            // boot under a ball rather than over it.
            plant_back: k.plant_back + k.plant_back_from_loft * loft_effort,
            // Lean away to get under it; open the hips to wrap across it.
            lean: k.lean_from_loft * loft_effort - k.lean_from_pace * effort,
            turn: k.turn_from_bend * across + k.turn_from_pace * effort,
            whip: (k.base_whip + k.whip_from_pace * effort).clamp(0.0, 0.95),
            across,
        }
    }
}

/// The hip torque that will send the ball off at `launch` metres per second.
///
/// This is the join between the animation and the flight, and it is deliberately
/// a *derivation* rather than two numbers tuned until they looked alike. The ball
/// leaves at [`KickTuning::ball_off_boot`] times the boot's speed, the boot is the
/// end of a leg swinging about the hip, and the work the hip does over the swing's
/// travel is the leg's kinetic energy at the ball:
///
/// ```text
/// v_boot = launch / ball_off_boot        ω = v_boot / leg_length
/// τ·Δθ  = ½·I·ω²              ⇒          τ = I·ω² / (2·Δθ)
/// ```
///
/// So a shot authored at 160 km/h is *visibly* a harder swing than one at 100:
/// more torque, a leg that reaches the ball sooner, and a follow-through carrying
/// the speed it actually had. Nothing has to be kept in step by hand, because
/// there is only one number.
fn hip_torque(launch: f32, tuning: &KickTuning) -> f32 {
    let boot = launch / tuning.ball_off_boot.max(1.0);
    let omega = boot / leg_length();
    let travel = (tuning.cock_angle - tuning.nominal_contact).max(0.1);
    tuning.leg_inertia * omega * omega / (2.0 * travel)
}

/// Hip to boot, metres — read off the figure rather than written down twice.
fn leg_length() -> f32 {
    PARTS[L_SHIN].offset.y.abs() + PARTS[L_FOOT].offset.y.abs()
}

/// How fast the boot is travelling at swing rate `rate` radians per second.
pub fn boot_speed(rate: f32) -> f32 {
    rate.abs() * leg_length()
}

/// How many integration substeps the swing takes per simulation tick.
const SUBSTEPS: usize = 8;

/// The striking leg, mid-swing.
///
/// The angle is measured at the hip in the body's own sagittal plane: **positive
/// is behind the body**, so the swing runs from a cocked positive angle down
/// through the ball and out to a negative follow-through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swing {
    angle: f32,
    rate: f32,
    /// The tick the boot reached the ball, once it has.
    struck: Option<u32>,
    ticks: u32,
    /// How fast the leg was travelling at contact, radians per second — what the
    /// follow-through is carrying.
    impact_rate: f32,
}

impl Swing {
    /// A leg cocked and about to be thrown.
    pub fn cocked(tuning: &KickTuning) -> Swing {
        Swing {
            angle: tuning.cock_angle,
            rate: 0.0,
            struck: None,
            ticks: 0,
            impact_rate: 0.0,
        }
    }

    pub fn angle(&self) -> f32 {
        self.angle
    }
    pub fn rate(&self) -> f32 {
        self.rate
    }
    /// The tick the ball was struck on, if it has been.
    pub fn struck_at(&self) -> Option<u32> {
        self.struck
    }
    pub fn impact_rate(&self) -> f32 {
        self.impact_rate
    }
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// How far through the swing the leg is, `0` cocked to `1` at the ball.
    pub fn progress(&self, tuning: &KickTuning, contact_angle: f32) -> f32 {
        let span = (tuning.cock_angle - contact_angle).abs().max(1.0e-3);
        ((tuning.cock_angle - self.angle) / span).clamp(0.0, 1.4)
    }

    /// Advance the swing one fixed step.
    ///
    /// A plain driven pendulum: the hip's torque, less the damping the leg's own
    /// tissue provides, over the leg's inertia. The only event in it is the ball:
    /// when the boot reaches `contact_angle` the ball takes a share of the leg's
    /// energy, which is why a follow-through is slower than the swing that
    /// produced it.
    ///
    /// A real penalty's downswing is about a tenth of a second, which is six
    /// ticks. Integrating that at the tick would resolve the contact to ±0.4
    /// radians — a boot that swings *through* where the ball was — so the swing,
    /// and only the swing, runs at a finer step inside its own tick.
    pub fn step(&mut self, drive: &KickDrive, contact_angle: f32, tuning: &KickTuning) {
        let before = self.struck;
        (0..SUBSTEPS).for_each(|_| self.substep(drive, contact_angle, tuning));
        self.ticks += 1;
        // The leg rests on the ball for the frame it strikes it.
        //
        // Without this the substeps that follow the contact carry the boot on
        // past — a fifth of a swing in one tick, at these speeds — and the only
        // frame anyone actually SEES of the strike has the boot already through
        // the ball by a hand's width. A real contact lasts about ten
        // milliseconds, which is about a tick, so holding it is not a fudge: the
        // ball is genuinely in the way for exactly this long.
        (before.is_none() & self.hit()).then(|| self.angle = contact_angle);
    }

    fn substep(&mut self, drive: &KickDrive, contact_angle: f32, tuning: &KickTuning) {
        let h = DT / SUBSTEPS as f32;
        let torque = drive.torque * [1.0, tuning.follow_through_torque][usize::from(self.hit())];
        let acceleration = (-torque - tuning.swing_damping * self.rate) / tuning.leg_inertia.max(0.01);
        self.rate += acceleration * h;
        // The hip runs out of travel, and the leg stops there. Without this the
        // integration is happy to carry the boot straight over the kicker's head.
        let free = self.angle + self.rate * h;
        self.angle = free.max(tuning.follow_through_limit);
        self.rate *= [1.0, 0.0][usize::from(free < tuning.follow_through_limit)];
        // Contact: the first substep on which the boot has reached the ball.
        let arrived = (self.angle <= contact_angle) & !self.hit();
        arrived.then(|| {
            self.impact_rate = self.rate;
            // Counted from the first tick the leg was released on, so a caller
            // adding it to the release tick lands on the tick the boot is on the
            // ball — which is the tick the ball leaves.
            self.struck = Some(self.ticks);
            // Rest the boot exactly on the ball for the frame it is struck. The
            // ball is genuinely in the way, and it is what makes the drawn
            // contact exact rather than exact-to-within-a-substep.
            self.angle = contact_angle;
            // And it is not free: it takes energy off the leg on the way past.
            self.rate *= 1.0 - tuning.impact_loss.clamp(0.0, 0.95);
        });
    }

    fn hit(&self) -> bool {
        self.struck.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shot::{BendCurve, GoalTarget};
    use crate::stroke::Pace;

    fn shot(pace: f32, bend: f32, loft: f32) -> ShotIntent {
        ShotIntent::curved(GoalTarget::new(0.0, 0.5), BendCurve::through(0.5, bend, 0.14), BendCurve::through(0.5, loft, 0.14), Pace {
                speed: pace,
                easing: 0.0,
            })
    }

    /// Swing until the ball is struck, and report `(tick, speed at contact)`.
    fn strike(drive: &KickDrive, contact: f32) -> (u32, f32) {
        let tuning = Tuning::DEFAULT;
        let mut swing = Swing::cocked(&tuning.kick);
        (0..400).for_each(|_| swing.step(drive, contact, &tuning.kick));
        (
            swing.struck_at().expect("the leg reaches the ball"),
            swing.impact_rate().abs(),
        )
    }

    #[test]
    fn a_harder_shot_is_struck_sooner_and_faster() {
        let tuning = Tuning::DEFAULT;
        let contact = -0.24;
        let (soft_tick, soft_speed) =
            strike(&KickDrive::for_shot(&shot(0.0, 0.0, 0.6), &tuning), contact);
        let (hard_tick, hard_speed) =
            strike(&KickDrive::for_shot(&shot(1.0, 0.0, 0.6), &tuning), contact);
        assert!(
            hard_tick < soft_tick,
            "a harder swing must arrive first: {hard_tick} vs {soft_tick}"
        );
        assert!(
            hard_speed > soft_speed * 1.15,
            "and arrive faster: {hard_speed:.2} vs {soft_speed:.2}"
        );
    }

    #[test]
    fn the_ball_takes_pace_off_the_leg_on_the_way_past() {
        let tuning = Tuning::DEFAULT;
        let drive = KickDrive::for_shot(&shot(0.6, 0.0, 0.6), &tuning);
        let mut swing = Swing::cocked(&tuning.kick);
        let mut before = 0.0f32;
        while swing.struck_at().is_none() && swing.ticks() < 400 {
            before = swing.rate();
            swing.step(&drive, -0.24, &tuning.kick);
        }
        let _ = before;
        let at_impact = swing.impact_rate().abs();
        assert!(swing.rate().abs() < at_impact, "the ball costs the leg speed");
        assert!(swing.rate().abs() > at_impact * 0.2, "but not all of it");
    }

    #[test]
    fn the_hip_runs_out_of_travel_instead_of_going_over_the_top() {
        let tuning = Tuning::DEFAULT;
        let drive = KickDrive::for_shot(&shot(1.0, 0.0, 0.0), &tuning);
        let mut swing = Swing::cocked(&tuning.kick);
        (0..600).for_each(|_| swing.step(&drive, -0.24, &tuning.kick));
        assert_eq!(swing.angle(), tuning.kick.follow_through_limit);
        assert_eq!(swing.rate(), 0.0, "and it stays stopped there");
    }

    #[test]
    fn the_swing_only_ever_strikes_once() {
        let tuning = Tuning::DEFAULT;
        let drive = KickDrive::for_shot(&shot(1.0, 0.0, 0.0), &tuning);
        let mut swing = Swing::cocked(&tuning.kick);
        (0..400).for_each(|_| swing.step(&drive, -0.24, &tuning.kick));
        let first = swing.struck_at().expect("struck");
        (0..200).for_each(|_| swing.step(&drive, -0.24, &tuning.kick));
        assert_eq!(swing.struck_at(), Some(first), "contact happens once");
    }

    #[test]
    fn the_leg_starts_cocked_behind_the_body_and_ends_in_front_of_it() {
        let tuning = Tuning::DEFAULT;
        let swing = Swing::cocked(&tuning.kick);
        assert!(swing.angle() > 0.0, "cocked is behind");
        assert_eq!(swing.rate(), 0.0);
        assert_eq!(swing.struck_at(), None);
        assert_eq!(swing.progress(&tuning.kick, -0.24), 0.0);
        let drive = KickDrive::for_shot(&shot(0.5, 0.0, 0.6), &tuning);
        let mut swing = swing;
        (0..400).for_each(|_| swing.step(&drive, -0.24, &tuning.kick));
        assert!(swing.angle() < -0.24, "and it follows through past the ball");
        assert!(swing.progress(&tuning.kick, -0.24) >= 1.0);
    }

    #[test]
    fn the_shape_of_the_shot_decides_how_the_body_meets_the_ball() {
        let tuning = Tuning::DEFAULT;
        // Bending it hard plants wider and opens the hips through the ball.
        let straight = KickDrive::for_shot(&shot(0.5, 0.0, 0.6), &tuning);
        let bent = KickDrive::for_shot(&shot(0.5, tuning.bend.max_offset, 0.6), &tuning);
        assert!(bent.plant_side.abs() > straight.plant_side.abs());
        assert!(bent.turn.abs() > straight.turn.abs());
        // Bending the other way mirrors it.
        let other = KickDrive::for_shot(&shot(0.5, -tuning.bend.max_offset, 0.6), &tuning);
        assert!((bent.across + other.across).abs() < 1.0e-4);
        // The hips open the other way by exactly as much: the tempo's own
        // contribution to the turn is what both shots share.
        assert!(((bent.turn + other.turn) - 2.0 * straight.turn).abs() < 1.0e-4);
        // Lofting it plants further back and leans away.
        let lofted = KickDrive::for_shot(&shot(0.5, 0.0, tuning.loft.max_offset), &tuning);
        let flat = KickDrive::for_shot(&shot(0.5, 0.0, 0.0), &tuning);
        assert!(lofted.plant_back > flat.plant_back);
        assert!(lofted.lean > flat.lean);
        // And hitting it hard commits the body forward rather than away.
        let hard = KickDrive::for_shot(&shot(1.0, 0.0, 0.0), &tuning);
        assert!(hard.lean < flat.lean);
        assert!(hard.approach > KickDrive::for_shot(&shot(0.0, 0.0, 0.0), &tuning).approach);
    }
}
