//! Named tuning data: the behaviour (steering/contact) and camera numbers.
//! Every knob the systems read lives here as plain data — nothing is buried in
//! system code. The other authoring concerns have their own files next to this
//! one: [`super::juice_tuning`], [`super::runback_tuning`],
//! [`super::locomotion_tuning`], [`super::biomech_tuning`].

/// Steering + contact tuning shared by the generic player systems. Units:
/// yards, seconds, radians, ticks (60 Hz), normalized strengths `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BehaviorTuning {
    /// Teammates begin to separate inside this range, yd.
    pub separation_radius: f32,
    /// Separation steering weight.
    pub separation_strength: f32,
    /// Arrival slow-down radius, yd.
    pub arrival_radius: f32,
    /// Range at which a blocker latches a defender, yd.
    pub block_engage_range: f32,
    /// How strongly a won block slows the defender, 0..=1.
    pub block_resist: f32,
    /// Range at which a tackle attempt can land, yd.
    pub tackle_range: f32,
    /// How high off the turf a carrier's feet may be and still be tackled by a
    /// player standing on it, yd — a defender's tackling reach.
    ///
    /// The standing-tackle gate used to be purely horizontal, which is a bug
    /// with two faces: a whiffed dive sailing *over* the carrier still registered
    /// a tackle (the "phantom dive"), and — once the back could leap — a
    /// defender could bring down a man clean over his head. One height is the
    /// fix for both, and it is the number that makes the leap a real answer to
    /// an encounter rather than an animation.
    pub tackle_reach_height: f32,
    /// Minimum closing speed for a tackle to even be ATTEMPTED, yd/s. Below it
    /// a defender is jogging alongside, not hitting anybody.
    pub tackle_min_closing_speed: f32,
    /// The speed-independent **grip** a tackler brings, yd/s of equivalent
    /// impulse per unit mass. A tackle is two things at once — a hit and a wrap
    /// — and this is the wrap: getting hold of a man and dragging him down, which
    /// works at a dead run alongside him where a hit does not.
    ///
    /// It exists because modelling only the hit made chase-down tackles
    /// impossible: the pursuit AI converges on the carrier and *matches his
    /// pace*, so closing speed at contact is near zero, and a measured run had
    /// six of nine carries walking into the end zone untouched. Real football
    /// has both, and so does this.
    pub tackle_grip: f32,
    /// The speed (yd/s) one unit of carrier mass is worth resisting with — the
    /// scale of the **tackle contest** (see [`crate::player::tackle`]).
    ///
    /// This is the single number that decides how often a hit is shed. Raise it
    /// and the carrier stays up more; lower it and contact becomes the
    /// guaranteed takedown it used to be, which is the thing this exists to stop.
    pub tackle_break_speed: f32,
    /// How much balance a shed tackle costs the carrier, `0..=1`. Sheds are
    /// meant to be survivable but **cumulative**: the second man through gets a
    /// runner who is already off balance, which is what stops a good carrier
    /// running through the whole defense.
    pub tackle_shed_balance_cost: f32,
    /// How much harder a committed DIVE is to shed than a standing wrap. A diver
    /// has thrown his whole body at you; he has also given up his feet to do it.
    pub tackle_dive_bonus: f32,
    /// Ticks a defender who was shed spends bounced off and out of the play.
    /// Without this he simply re-attempts next tick and the shed means nothing.
    pub hit_reaction_ticks: u32,
    /// Relative speed mapped to impact strength 1.0, yd/s.
    pub tackle_full_strength_speed: f32,
    /// Deep-pursuit cushion: how many yards a rallying deep defender stays
    /// goal-side of a perceived pass landing point (over-the-top leverage
    /// instead of camping the catch).
    pub pursuit_cushion: f32,
    /// Impact strength above which the target goes airborne.
    pub airborne_threshold: f32,
    /// Diving-tackle commit window, as a multiple of `tackle_range`: a chaser
    /// leaves their feet when the carrier is beyond standing range but within
    /// `tackle_range * dive_window`.
    pub dive_window: f32,
    /// Minimum closing speed (yd/s) required to commit a dive.
    pub dive_min_closing_speed: f32,
    /// The carrier must be moving at least this fast (yd/s) to be worth diving
    /// at — you don't dive at a stationary target you can just run down.
    pub dive_carrier_min_speed: f32,
    /// Forward launch speed of a dive (yd/s).
    pub dive_launch_forward: f32,
    /// Upward launch speed of a dive (yd/s) — the arc height.
    pub dive_launch_up: f32,
    /// Impact strength recorded for a whiffed dive's own landing (drives the
    /// dust puff when a diver hits the turf without a tackle).
    pub dive_whiff_impact: f32,
    /// Upward launch speed for an airborne knockdown, yd/s.
    pub launch_up_speed: f32,
    /// Ticks a grounded fall lasts before recovery starts.
    pub fall_ticks: u32,
    /// Ticks of the recovery animation/state.
    pub recovery_ticks: u32,
    /// Ticks the snap takes to reach the quarterback.
    pub snap_ticks: u32,
    /// Ticks the **exchange** takes: how long the ball is visibly travelling
    /// from the quarterback's hands into the back's. Longer than the snap on
    /// purpose — the snap is a blur behind the line, the handoff is the beat the
    /// player has to be able to read, because it is the moment they take over.
    pub handoff_ticks: u32,
    /// How close the back must be to the quarterback for the exchange to be
    /// legal, yd. The mesh gate: an order to hand off from further away than
    /// this is simply refused, so possession can never jump across the field.
    pub handoff_range: f32,
    /// The speed a pass actually leaves the hand at, yd/s (~58 mph) — a hard,
    /// realistic NFL throw.
    ///
    /// It is the REAL flight speed, which it did not used to be: the launch
    /// speed was once derived from the range instead (the minimum that just
    /// reached the target at a fixed 12° angle, ~19 yd/s at 15 yards), while
    /// this value only fed the intercept solve. The pass was therefore aimed
    /// for one flight time and thrown with another, which floated it and landed
    /// it behind a running receiver. Now `flight::aim_and_velocity` leads and
    /// launches at the same speed, and the *elevation* is what varies with
    /// range.
    ///
    /// Re-tuned from 34 when that changed: 34 was never exercised as a flight
    /// speed, and as one it is ~76 mph — beyond any real throw, and it completed
    /// 100% of passes, which leaves the read with nothing to decide.
    pub pass_speed: f32,
    /// Floor on the launch speed, yd/s. Only reached on a very short throw,
    /// where `min_flight_ticks` would otherwise slow the ball below a real
    /// pass. A floor, not a punishment: a five-yard route still gets a crisp
    /// ball, never a lob.
    pub pass_speed_min: f32,
    /// Minimum pass flight time, ticks. This is a floor on the CATCH pipeline —
    /// the ball needs a few ticks airborne to be contested and resolved — and
    /// NOT a stylistic hang time. At 24 it forced a five-yard slant that should
    /// travel for 0.15 s to hang for 0.40 s, which is most of why short throws
    /// felt weak and kept getting jumped.
    pub min_flight_ticks: u32,
    /// Ticks of quarterback throw wind-up before release.
    pub throw_windup_ticks: u32,
    /// Half-angle of the quarterback's throwing cone, radians. A receiver must
    /// be within this much of the quarterback's facing to be throwable — this
    /// is what makes the stick aim the pass.
    pub throw_cone_half_angle: f32,
    /// How far off straight-downfield a STEERED quarterback may turn, radians.
    /// His facing is clamped to this forward arc, so pushing the stick sideways
    /// strafes him instead of spinning him: he keeps his eyes downfield and can
    /// never end up facing his own end zone. It also bounds how far he can swing
    /// the throwing cone, which is how the stick aims the pass.
    pub qb_aim_max_yaw: f32,
    /// Nearest a receiver may be and still be throwable, yd (a man standing on
    /// top of the quarterback is not a pass).
    pub throw_min_range: f32,
    /// Furthest a receiver may be and still be throwable, yd.
    pub throw_max_range: f32,
    /// Gravity, yd/s² (9.8 m/s² in yards).
    pub gravity: f32,
    /// Boundary clamp margin, yd.
    pub bounds_margin: f32,
    /// Half-width of the protected pocket box, yd.
    pub pocket_half_width: f32,
    /// How far behind the line of scrimmage the pocket extends, yd.
    pub pocket_depth: f32,
    /// How far past the line of scrimmage still counts as in the pocket, yd.
    pub pocket_lip: f32,
    /// Downfield speed (yd/s) a ball-holding quarterback must show outside the
    /// pocket to register as running.
    pub scramble_speed: f32,
    /// Consecutive run-showing ticks before the quarterback is deemed committed
    /// to running (the scramble becomes a defensive event).
    pub scramble_commit_ticks: u32,
    /// A defender within this range of a live pass's catch point makes it a
    /// contested ball, yd.
    pub contest_radius: f32,
    /// Slack (ticks) a defender may arrive after the ball and still be counted
    /// able to contest an interception.
    pub contest_window_ticks: u32,
    /// Per-tick rate the engagement advantage moves, scaled by the strength edge.
    pub engage_advantage_rate: f32,
    /// The base advantage gain per tick at strength parity, so a held block
    /// eventually yields (the pass rush wins if the quarterback holds forever).
    pub engage_base_gain: f32,
    /// Advantage at which the rusher sheds the block and breaks free, `0..=1`.
    pub shed_threshold: f32,
    /// Displacement speed a winning blocker drives the rusher off his lane, yd/s.
    pub block_drive: f32,
    /// Ticks a blocker spends squaring up before the contest counts as set.
    pub engage_square_ticks: u32,
    /// Ticks a fresh ball carrier is securing the ball and cannot be tackled —
    /// a caught pass gets a beat before the hit, so a contested catch is a
    /// catch-and-step, not an instant swarm.
    pub catch_secure_ticks: u32,
    /// Interception difficulty: as the ball arrives, a defender must be within
    /// this fraction of his catch radius of it to pick it off. Further out — but
    /// still in the catch volume — he can only get a hand on it and swat it down.
    /// Tighter than a reception, so an interception is a genuine play on the ball.
    pub interception_radius_scale: f32,
}

impl Default for BehaviorTuning {
    fn default() -> Self {
        BehaviorTuning {
            separation_radius: 1.6,
            separation_strength: 6.0,
            arrival_radius: 2.2,
            block_engage_range: 1.4,
            block_resist: 0.8,
            tackle_range: 1.3,
            tackle_reach_height: 1.5,
            tackle_min_closing_speed: 2.0,
            tackle_grip: 3.7,
            tackle_break_speed: 3.7,
            tackle_shed_balance_cost: 0.34,
            tackle_dive_bonus: 1.25,
            hit_reaction_ticks: 26,
            tackle_full_strength_speed: 14.0,
            pursuit_cushion: 6.0,
            airborne_threshold: 0.55,
            dive_window: 2.4,
            dive_min_closing_speed: 6.0,
            dive_carrier_min_speed: 4.0,
            dive_launch_forward: 9.5,
            dive_launch_up: 3.2,
            dive_whiff_impact: 0.25,
            launch_up_speed: 4.6,
            fall_ticks: 26,
            recovery_ticks: 40,
            snap_ticks: 7,
            handoff_ticks: 12,
            handoff_range: 1.9,
            pass_speed: 26.0,
            pass_speed_min: 17.0,
            min_flight_ticks: 16,
            throw_windup_ticks: 9,
            throw_cone_half_angle: 0.95,
            qb_aim_max_yaw: 1.05,
            throw_min_range: 2.0,
            throw_max_range: 34.0,
            gravity: 10.72,
            bounds_margin: 0.6,
            pocket_half_width: 5.0,
            pocket_depth: 9.0,
            pocket_lip: 1.5,
            scramble_speed: 2.2,
            scramble_commit_ticks: 12,
            contest_radius: 3.0,
            contest_window_ticks: 10,
            engage_advantage_rate: 0.05,
            engage_base_gain: 0.75,
            shed_threshold: 0.85,
            block_drive: 2.5,
            engage_square_ticks: 8,
            catch_secure_ticks: 22,
            interception_radius_scale: 0.62,
        }
    }
}
