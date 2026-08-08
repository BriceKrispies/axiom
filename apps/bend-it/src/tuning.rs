//! Every gameplay number in one place.
//!
//! Nothing in this game is allowed to hide a magic constant in a system. A
//! system reads its own sub-table off [`Tuning`], so "the keeper is too good"
//! and "the bend is too weak" are both edits to a value here rather than a hunt
//! through the code. [`Tuning::DEFAULT`] is the shipping feel; the whole tree is
//! `const` so a test can build a variant without allocating.
//!
//! Units are SI: metres, seconds, radians. One world unit is one metre.

/// The fixed simulation step, seconds (60 Hz).
pub const DT: f32 = 1.0 / 60.0;

/// How the authored endpoint is kept off the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalTuning {
    /// Minimum distance an authored endpoint keeps from the inside face of the
    /// posts and the crossbar, metres. The endpoint is valid *by construction*,
    /// so this is what guarantees a normal shot can never clip the frame just
    /// because the renderer and the maths round differently.
    pub inset: f32,
}

/// Bounds and response for one sculpting axis (bend or height).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SculptTuning {
    /// Largest offset the curve may reach away from the straight line, metres.
    pub max_offset: f32,
    /// Smallest offset in the *negative* direction, metres. Bend is symmetric
    /// (`-max_offset`); height is not — a shot may only dip so far before it
    /// would go through the turf.
    pub min_offset: f32,
    /// Metres of curve offset per metre of on-screen drag across the editor
    /// panel. `1.0` is literal 1:1 within the panel's own scale.
    pub drag_gain: f32,
    /// The peak parameter is clamped away from the endpoints by this much, so a
    /// grab right on the ball or right on the goal still yields a smooth curve
    /// instead of a spike with no room to resolve.
    pub peak_margin: f32,
}

/// Ball flight: how hard the ball is hit, and how densely the path is sampled.
///
/// **Speed is the authored quantity here, not time.** Flight time used to be
/// the number the game chose and speed was whatever fell out of it, which is how
/// the ball ended up crossing 11 metres at 35 km/h. Now the shot is given a
/// launch speed and the flight time is derived from it — so "how hard was it
/// hit" is a number a person can read, sanity-check against a real penalty, and
/// tune, and everything downstream is timed against a ball that behaves like one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightTuning {
    /// How fast the ball leaves the boot, metres per second, for the slowest and
    /// the fastest line the reading can produce.
    ///
    /// 27.8 m/s is 100 km/h and 44.4 is 160 — the real range of a struck
    /// penalty. At those speeds the ball is on the keeper in about a third of a
    /// second, which is the fact the whole keeper model is calibrated against.
    pub slow_launch: f32,
    pub fast_launch: f32,
    /// How much of the tempo a heavily-shaped shot gives up, `0..1`. Bending and
    /// lifting a ball costs pace: the boot's energy goes into the movement
    /// instead of into the flight. A fully-shaped shot drawn at full tempo lands
    /// this fraction of the way back down toward the slow end.
    pub shape_cost: f32,
    /// How much pace the ball bleeds over its flight, as the exponent of the
    /// speed profile — the ball keeps `e^-decel` of its launch speed at the line.
    ///
    /// A real ball loses 10–15% of its speed over 11 m to drag, so this belongs
    /// near `0.15`. Larger values are what made a struck shot arrive at walking
    /// pace, floating in as if on a string.
    pub decel: f32,
    /// Number of arc-length-uniform samples in the canonical trajectory.
    pub samples: usize,
    /// Ball radius, metres (a size-5 ball).
    pub ball_radius: f32,
    /// Presentation spin: revolutions per second per metre of lateral curve
    /// offset, and the baseline roll from forward speed.
    pub spin_per_curve: f32,
    pub roll_per_speed: f32,
}

/// The goalkeeper. Every value here is a limit on a *physical* attempt: the
/// keeper never teleports and never edits the ball's path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeeperTuning {
    /// How long after the strike the keeper is still frozen, seconds.
    pub reaction: f32,
    /// How long after the first read the keeper takes its ONE correction,
    /// seconds. This is what turns *where* the player puts the peak of a curve
    /// into the central decision of the game: movement that happens before the
    /// correction is seen and answered, movement that happens after it is not.
    pub adjust_delay: f32,
    /// How much of the ball's observed *acceleration* the keeper folds into its
    /// prediction, `0..1`. At `0` the keeper extrapolates a straight line from
    /// the ball's velocity at the moment it reads — so every curve, dip and lift
    /// authored after that instant is movement it did not see coming. At `1` it
    /// is a perfect ballistic reader. This single number is why two shots to the
    /// same corner are not the same shot.
    pub read_fidelity: f32,
    /// The same, for the one mid-flight correction. It is higher than the first
    /// read because by then the keeper has *watched* the ball swerve for a beat
    /// and has some idea it is swerving. The gap between the two numbers is the
    /// whole game: movement the keeper sees before its correction it can answer,
    /// movement after it, it cannot.
    pub adjust_fidelity: f32,
    /// The downward acceleration the keeper's own mental model assumes, m/s².
    ///
    /// Deliberately well below real gravity, because the keeper is modelling
    /// *balls in this game* rather than point masses: a struck ball here follows
    /// an authored arc that holds its line far better than a projectile would. A
    /// keeper using true gravity reads every flat driven shot as dropping into
    /// the turf and dives at its own feet, which turns "aim high and hit it flat"
    /// into a shot that always scores.
    pub read_gravity: f32,
    /// How far the keeper commits to its *vertical* read, `0..1`. Below `1` it
    /// hedges toward its own standing height.
    ///
    /// Judging how high a ball will arrive is much harder than judging which
    /// side, and a keeper that fully trusted a vertical extrapolation would throw
    /// its hands to the crossbar off one glimpse of a rising ball — which makes
    /// "arc it into the bottom corner" an unconditional goal. Hedging keeps the
    /// height read a real edge without making it the only shot in the game.
    pub vertical_trust: f32,
    /// Lateral dive speed, m/s.
    pub dive_speed: f32,
    /// Furthest the keeper's hips can travel sideways in one dive, metres.
    ///
    /// Bounded BELOW the goal by design. The hips reach `dive_distance ×
    /// execution` and the laid-over body plus arm covers a further
    /// `figure::stretch_from_hips()`, and that
    /// total has to stay short of the far corner — otherwise a keeper with a
    /// clean read covers the whole goal and there is nowhere left to shoot.
    /// Standing off-centre (the shading its memory does) is what brings a corner
    /// into range, which is the point: the keeper earns a corner by expecting it.
    pub dive_distance: f32,
    /// How high the keeper's hips can leave the ground, metres.
    pub vertical_reach: f32,
    /// Radius of the swept reach capsule (hand + forearm), metres.
    pub reach_radius: f32,
    /// Radius of the keeper's torso capsule, metres.
    pub body_radius: f32,
    /// How completely the keeper executes what it committed to, `0..1`. Below
    /// `1` it consistently falls a little short of its own read.
    pub execution: f32,
    /// Seconds the dive takes to reach full extension.
    pub extend_time: f32,
    /// The keeper's nerve: how much any one attempt varies from its average
    /// self. Every value here is a *bound* on a seeded roll taken once per
    /// penalty, so a keeper is unpredictable without the game ever being
    /// unrepeatable — the same seed is the same shootout, always.
    ///
    /// A keeper with no nerve at all is a machine you solve once and beat
    /// forever; a keeper who is merely random is a coin toss. These bound the
    /// space between.
    pub reaction_jitter: f32,
    /// How far its judgement of where the ball is going can be out, metres.
    pub read_error_across: f32,
    pub read_error_up: f32,
    /// How much its follow-through varies around `execution`.
    pub execution_spread: f32,
    /// How often it abandons the read entirely and simply picks a side before
    /// the ball is struck, `0..1` — the thing real penalty keepers do.
    pub guess_chance: f32,
    /// How often it gets its one mid-flight correction at all, `0..1`.
    pub correction_chance: f32,
    /// How far the keeper shades its starting position toward where recent shots
    /// have finished, as a fraction of the average, and how far it will go,
    /// metres.
    ///
    /// This is the keeper's own memory, and it is what stops any single authored
    /// shape from being a solved answer: keep putting them in the same corner and
    /// the keeper starts standing nearer to it, which costs the shot the metre it
    /// was winning by. It adds no randomness — a replay of the same shots is the
    /// same shootout.
    pub shade_gain: f32,
    pub shade_limit: f32,
}

/// The kick, as a body and a swing rather than a schedule.
///
/// The contact tick is **not** in here: it is solved by integrating the swing,
/// so a harder shot genuinely reaches the ball sooner. What is here is what the
/// hip can do and what the drawing is allowed to ask of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickTuning {
    /// The shortest a run-up may be, in ticks — a floor under the arrival so a
    /// furious drawing still reads as a run-up rather than a teleport.
    pub plant: u32,
    /// How long the follow-through keeps playing after contact, ticks.
    pub follow_through: u32,
    /// Where the run-up starts, relative to the ball: metres back, and metres to
    /// the side at zero bend.
    pub approach_back: f32,
    pub approach_side: f32,
    /// Extra sideways offset of the run-up at full bend, metres.
    pub approach_bend_widen: f32,

    /// The swing: the leg's own physics.
    ///
    /// A rough but honest human leg — about 1.1 kg·m² about the hip, with the
    /// damping soft tissue provides. The numbers matter less than the fact that
    /// the same ones produce every shot, so a harder swing is *earned* rather
    /// than animated.
    pub leg_inertia: f32,
    pub swing_damping: f32,
    /// Where the leg is cocked to before it is released, radians behind.
    pub cock_angle: f32,
    /// How much faster the ball leaves than the boot that hit it.
    ///
    /// A football is light and the collision is near-elastic, so a struck ball
    /// outruns the boot by about a third. It is the number that ties the swing to
    /// the flight: the hip's torque is whatever gets the boot to `launch /
    /// ball_off_boot`, so a harder shot is a visibly harder swing rather than the
    /// same swing with a faster ball leaving it.
    pub ball_off_boot: f32,
    /// The swing travel the torque is sized against — where the boot meets the
    /// ball on a typical stance. The true contact angle is measured off the
    /// planted body per shot; this is only the figure the work-energy sum uses.
    pub nominal_contact: f32,
    /// What fraction of the torque keeps driving after contact.
    pub follow_through_torque: f32,
    /// Where the hip runs out of travel, radians in front. The leg stops there
    /// rather than carrying on over the top — a hip is not a windmill.
    pub follow_through_limit: f32,
    /// How much speed the ball takes off the leg, `0..1`.
    pub impact_loss: f32,

    /// The run-up's speed, and what the tempo adds to it, m/s.
    pub base_approach: f32,
    pub approach_from_pace: f32,
    /// Where the plant foot goes: beside the ball, and behind it.
    pub plant_side: f32,
    pub plant_back: f32,
    /// How much a bent shot widens the plant, and a lofted one drops it back.
    pub plant_side_from_bend: f32,
    pub plant_back_from_loft: f32,
    /// How far the body leans away for a lofted shot, and forward for a hard one.
    pub lean_from_loft: f32,
    pub lean_from_pace: f32,
    /// How far the hips open through the ball.
    pub turn_from_bend: f32,
    pub turn_from_pace: f32,
    /// How late the knee snaps straight, and what the tempo adds.
    pub base_whip: f32,
    pub whip_from_pace: f32,
}

/// How a drawn line is captured and read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeTuning {
    /// The shortest line that counts as a shot, as a fraction of the viewport's
    /// short edge. Below it the drawing is a tap and nothing is kicked.
    pub min_length: f32,
    /// How far apart captured points are kept, as a fraction of the short edge.
    /// A finger reports far more samples than a shape needs.
    pub spacing: f32,
    /// How strongly the fit is pulled toward a plain straight shot when the
    /// drawing does not constrain much. This is the "does its best" knob: at `0`
    /// a two-inch flick can author a wild curve; higher, and only a line that
    /// really says *bend* gets one.
    pub ridge: f32,
    /// How long the drawn line takes to flick away after release, seconds.
    pub fade: f32,
}

/// How the tempo of a drawing becomes the pace of a shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaceTuning {
    /// The drawing speed that counts as a full-blooded flick, as a fraction of
    /// the viewport's short edge per fixed tick. A swipe at this rate or faster
    /// reads as `1`.
    pub reference: f32,
    /// How much a hand that sped up (or trailed off) through the stroke changes
    /// how sharply the ball bleeds pace.
    pub easing_gain: f32,
    /// Hard bounds on that decay. **Both are above zero**, which is the whole
    /// normalisation: whatever was drawn, the ball's speed only ever falls, so it
    /// can never dawdle through the air and then hurry.
    pub min_decay: f32,
    pub max_decay: f32,
}

/// Phase timings, in fixed ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionTuning {
    /// The settle before the editor accepts an aim.
    pub ready: u32,
    /// The commit beat between the last edit and the run-up: the editor fades,
    /// the preview brightens.
    pub commit: u32,
    /// How long the result banner holds.
    pub resolution: u32,
    /// The wipe back to a fresh attempt.
    pub reset: u32,
}

/// Camera framing. The pose is derived from the viewport every frame, so the
/// same numbers compose a 9:19.5 phone and a 16:9 desktop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraTuning {
    /// Eye height and how far behind the ball the eye sits, metres.
    pub eye_height: f32,
    pub eye_back: f32,
    /// The aim point: height, and how far in front of the goal line it sits.
    pub look_height: f32,
    pub look_depth: f32,
    /// Slack around the must-see points when the field of view is fitted.
    pub fit_padding: f32,
    /// The near-field allowance: metres of turf kept in frame in front of the
    /// ball, and how far to each side of it. This is what reserves room for the
    /// kicker without letting the *top* of a run-up — which is very close to the
    /// camera and therefore very wide-angled — dictate the whole frustum and
    /// shrink the goal to a stamp. The kicker enters frame during its approach,
    /// which is also how a broadcast frames one.
    pub near_depth: f32,
    pub near_margin: f32,
    /// Hard bounds on the fitted vertical field of view, degrees.
    pub min_fov: f32,
    pub max_fov: f32,
    /// How far the camera creeps in over the flight, metres.
    pub flight_dolly: f32,
    /// How far forward the eye comes on a wide screen, metres.
    pub landscape_close: f32,
}

/// The whole tuning tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    pub goal: GoalTuning,
    pub bend: SculptTuning,
    pub loft: SculptTuning,
    pub flight: FlightTuning,
    pub keeper: KeeperTuning,
    pub kick: KickTuning,
    pub stroke: StrokeTuning,
    pub pace: PaceTuning,
    pub transitions: TransitionTuning,
    pub camera: CameraTuning,
}

impl Tuning {
    /// The shipping feel.
    pub const DEFAULT: Tuning = Tuning {
        goal: GoalTuning { inset: 0.30 },
        bend: SculptTuning {
            max_offset: 2.0,
            min_offset: -2.0,
            drag_gain: 1.0,
            peak_margin: 0.14,
        },
        loft: SculptTuning {
            // A lob clears a two-and-a-half metre keeper by a comfortable margin;
            // a dip pulls the middle of the flight *below* the straight line to
            // the target, which is how a shot aimed at the top corner still
            // arrives under a keeper's hands.
            max_offset: 3.4,
            min_offset: -1.5,
            drag_gain: 1.0,
            peak_margin: 0.14,
        },
        flight: FlightTuning {
            slow_launch: 27.8,
            fast_launch: 44.4,
            shape_cost: 0.42,
            decel: 0.16,
            samples: 96,
            ball_radius: 0.11,
            spin_per_curve: 0.42,
            roll_per_speed: 0.085,
        },
        keeper: KeeperTuning {
            reaction: 0.09,
            adjust_delay: 0.13,
            read_fidelity: 0.30,
            adjust_fidelity: 0.60,
            read_gravity: 4.4,
            vertical_trust: 0.95,
            dive_speed: 9.0,
            dive_distance: 2.80,
            vertical_reach: 0.85,
            reach_radius: 0.19,
            body_radius: 0.38,
            execution: 0.97,
            extend_time: 0.06,
            reaction_jitter: 0.030,
            read_error_across: 0.26,
            read_error_up: 0.21,
            execution_spread: 0.10,
            guess_chance: 0.10,
            correction_chance: 0.75,
            shade_gain: 0.62,
            shade_limit: 1.15,
        },
        kick: KickTuning {
            plant: 14,
            follow_through: 14,
            approach_back: 2.2,
            approach_side: 0.62,
            approach_bend_widen: 0.80,

            leg_inertia: 0.35,
            swing_damping: 0.22,
            cock_angle: 0.95,
            ball_off_boot: 1.35,
            nominal_contact: -0.40,
            follow_through_torque: 0.22,
            follow_through_limit: -1.85,
            impact_loss: 0.35,

            base_approach: 3.6,
            approach_from_pace: 0.60,
            plant_side: -0.26,
            plant_back: 0.10,
            plant_side_from_bend: -0.16,
            plant_back_from_loft: 0.16,
            lean_from_loft: 0.26,
            lean_from_pace: 0.14,
            turn_from_bend: 0.42,
            turn_from_pace: 0.18,
            base_whip: 0.45,
            whip_from_pace: 0.35,
        },
        stroke: StrokeTuning {
            min_length: 0.16,
            spacing: 0.008,
            ridge: 0.055,
            fade: 0.22,
        },
        pace: PaceTuning {
            reference: 0.070,
            easing_gain: 0.55,
            min_decay: 0.09,
            max_decay: 0.30,
        },
        transitions: TransitionTuning {
            ready: 6,
            commit: 7,
            resolution: 74,
            reset: 14,
        },
        camera: CameraTuning {
            eye_height: 4.15,
            eye_back: 8.8,
            look_height: 1.55,
            look_depth: 2.8,
            fit_padding: 1.10,
            near_depth: 1.7,
            near_margin: 1.5,
            min_fov: 30.0,
            max_fov: 86.0,
            flight_dolly: 1.1,
            landscape_close: 2.6,
        },
    };
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_tuning_is_internally_consistent() {
        let t = Tuning::DEFAULT;
        assert!(t.flight.slow_launch < t.flight.fast_launch);
        assert!(t.flight.decel > 0.0 && t.flight.decel < 0.5);
        assert!(t.bend.min_offset < 0.0 && t.bend.max_offset > 0.0);
        assert!(t.loft.min_offset < 0.0 && t.loft.max_offset > 0.0);
        assert!(t.kick.leg_inertia > 0.0 && t.kick.swing_damping > 0.0);
        assert!(t.kick.cock_angle > t.kick.nominal_contact);
        assert!(t.kick.ball_off_boot > 1.0);
        assert!((0.0..1.0).contains(&t.kick.impact_loss));
        assert!(t.kick.follow_through_limit < 0.0);
        assert!(t.stroke.min_length > 0.0 && t.stroke.ridge > 0.0);
        assert!(t.pace.min_decay < t.pace.max_decay);
        assert!(t.pace.min_decay > 0.0 && t.pace.max_decay > t.pace.min_decay);
        assert!(t.camera.min_fov < t.camera.max_fov);
        assert!((0.0..=1.0).contains(&t.keeper.read_fidelity));
        assert!(t.keeper.adjust_fidelity > t.keeper.read_fidelity);
        assert!((0.0..=1.0).contains(&t.keeper.execution));
        assert!((0.0..=1.0).contains(&t.keeper.guess_chance));
        assert!((0.0..=1.0).contains(&t.keeper.correction_chance));
        assert!(t.keeper.reaction_jitter < t.keeper.reaction);
        assert_eq!(Tuning::default(), t);
    }
}
