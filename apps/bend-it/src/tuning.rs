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

/// Ball flight: how the shape becomes speed, and how densely the path is
/// sampled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightTuning {
    /// Flight time of a plain, unsculpted shot, seconds.
    pub base_duration: f32,
    /// Extra flight time at full bend, seconds.
    pub bend_duration_gain: f32,
    /// Extra flight time at full loft, seconds.
    pub loft_duration_gain: f32,
    /// Hard bounds on flight time, seconds.
    pub min_duration: f32,
    pub max_duration: f32,
    /// Air-drag shape of the traversal. `0` is exactly uniform speed; larger
    /// values make the ball leave hot and bleed pace, which is what stops an
    /// evenly-parameterised curve from looking like a lift on rails.
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
    pub dive_distance: f32,
    /// How high the keeper's hips can leave the ground, metres.
    pub vertical_reach: f32,
    /// Arm span from the hips to the leading fingertip, metres.
    pub arm_span: f32,
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

/// The kick, in fixed ticks from the start of the run-up. The ball leaves at
/// [`KickTuning::contact`] and not one tick before.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickTuning {
    /// Tick the plant foot lands.
    pub plant: u32,
    /// Tick the boot meets the ball — the launch tick.
    pub contact: u32,
    /// How long the follow-through keeps playing after contact, ticks.
    pub follow_through: u32,
    /// Where the run-up starts, relative to the ball: metres back, and metres to
    /// the side at zero bend.
    pub approach_back: f32,
    pub approach_side: f32,
    /// Extra sideways offset of the run-up at full bend, metres. A shot the
    /// player has bent hard is approached from a wider angle.
    pub approach_bend_widen: f32,
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
            base_duration: 0.92,
            bend_duration_gain: 0.22,
            loft_duration_gain: 0.78,
            min_duration: 0.74,
            max_duration: 1.92,
            decel: 0.55,
            samples: 96,
            ball_radius: 0.11,
            spin_per_curve: 1.35,
            roll_per_speed: 0.30,
        },
        keeper: KeeperTuning {
            reaction: 0.17,
            adjust_delay: 0.30,
            read_fidelity: 0.42,
            adjust_fidelity: 0.80,
            read_gravity: 4.4,
            vertical_trust: 0.58,
            dive_speed: 5.4,
            dive_distance: 2.60,
            vertical_reach: 0.85,
            arm_span: 0.86,
            reach_radius: 0.13,
            body_radius: 0.28,
            execution: 0.90,
            extend_time: 0.44,
            reaction_jitter: 0.055,
            read_error_across: 0.42,
            read_error_up: 0.34,
            execution_spread: 0.10,
            guess_chance: 0.17,
            correction_chance: 0.80,
            shade_gain: 0.62,
            shade_limit: 1.15,
        },
        kick: KickTuning {
            plant: 19,
            contact: 24,
            follow_through: 26,
            approach_back: 2.7,
            approach_side: 0.62,
            approach_bend_widen: 0.80,
        },
        stroke: StrokeTuning {
            min_length: 0.16,
            spacing: 0.008,
            ridge: 0.055,
            fade: 0.22,
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
        assert!(t.flight.min_duration < t.flight.base_duration);
        assert!(t.flight.base_duration < t.flight.max_duration);
        assert!(t.bend.min_offset < 0.0 && t.bend.max_offset > 0.0);
        assert!(t.loft.min_offset < 0.0 && t.loft.max_offset > 0.0);
        assert!(t.kick.plant < t.kick.contact);
        assert!(t.stroke.min_length > 0.0 && t.stroke.ridge > 0.0);
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
