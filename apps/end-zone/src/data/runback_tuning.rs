//! Every number the running back's three moves read. Kept as plain data in the
//! authoring layer, like all the other tuning, so making a juke sharper or a
//! charge harder to win is a data edit and never a change to the mechanic.
//!
//! The three moves are tuned to have *different jobs*, and the numbers are what
//! enforce that: a juke buys lateral yards and nothing else, a charge converts
//! momentum into a defender on the ground, and a leap buys height at the cost of
//! being unable to do anything else for the better part of a second and of a
//! three-second wait afterwards. None of them is a strictly better answer to an
//! encounter than the other two, which is the whole design.

/// The running back's move tuning. Units: yards, yd/s, yd/s², ticks (60 Hz).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunbackTuning {
    // --- the juke -----------------------------------------------------------
    /// Lateral speed the plant-and-cut carries the back sideways at, yd/s.
    ///
    /// Sized against `tackle_range`: over [`Self::juke_ticks`] it must move him
    /// further sideways than a defender can reach, or the move is decoration.
    pub juke_speed: f32,
    /// How long the cut carries him, ticks.
    pub juke_ticks: u32,
    /// Ticks after a juke before any move may begin again. Long enough that a
    /// mashed juke is worse than a timed one, short enough to string two cuts
    /// through traffic.
    pub juke_recovery_ticks: u32,
    /// The fraction of forward speed the cut costs, applied once at the plant.
    /// Below 1.0 because a real cut scrubs speed; near 1.0 because the arcade
    /// promise is that forward momentum is *retained*.
    pub juke_forward_keep: f32,

    // --- what makes a dodge a dodge ----------------------------------------
    /// How near a defender must be, at the moment of the cut, to count as a
    /// credible threat at all, yd.
    pub dodge_threat_range: f32,
    /// How far ahead the pre-juke trajectories are projected when deciding
    /// whether a defender's tackle was genuinely *imminent*, ticks.
    pub dodge_lookahead_ticks: u32,
    /// How long a threat stays pending before it is dropped unresolved, ticks.
    /// Past this, whatever happened was not the juke's doing.
    pub dodge_resolve_ticks: u32,
    /// How far downfield of the beaten defender the back must get before the
    /// dodge is credited, yd.
    pub dodge_clear_yards: f32,

    // --- the shoulder charge ------------------------------------------------
    /// How long a lowered shoulder stays armed looking for contact, ticks.
    ///
    /// Must be longer than the time it takes to actually *reach* the man you
    /// dropped your pads for. At 24 ticks it was not: a back committing at the
    /// ideal gap of ~3.4 yd with ~6 yd/s of closing needs about 35 ticks to
    /// arrive, so eight charges in ten expired having touched nobody — and each
    /// one still cost a full move lockout, which made the down button a trap
    /// that measurably doubled the tackled rate.
    pub shoulder_ticks: u32,
    /// Ticks after a charge RESOLVES before another move may begin. Contact
    /// costs something either way.
    pub shoulder_recovery_ticks: u32,
    /// Ticks after a charge EXPIRES untouched. Small on purpose: standing a man
    /// up who never arrived is a misread, and a misread should cost you the
    /// beat you spent on it, not the next three.
    pub shoulder_expire_ticks: u32,
    /// Extra reach beyond the two body radii at which the charge finds contact.
    pub shoulder_reach: f32,
    /// The speed-independent **drive** a lowered shoulder is worth, yd/s of
    /// equivalent impulse, scaled by the back's `block_strength`.
    ///
    /// The charge's counterpart of the tackle's `tackle_grip`, and it exists for
    /// the same measured reason: modelling contact as pure momentum meant a
    /// charge could only be won in a head-on collision, and the encounters this
    /// game actually produces are pursuits. Instrumented against real play, the
    /// charge lost *every* contest it was ever offered — impulse 3.5–5.4 against
    /// a resistance of 5.5–6.5 — which is to say the down button did nothing at
    /// all. Lowering your pads and driving through a man is worth something even
    /// when you are not running at him.
    pub charge_drive: f32,
    /// The gap at which lowering the shoulder is perfectly timed, yd.
    ///
    /// A back drops his shoulder about half a second out, and half a second at
    /// running speed is three and a half yards — not the two and a half this
    /// used to say, which taxed every realistically-timed charge by a third
    /// before the contest even started.
    pub charge_ideal_gap: f32,
    /// How far either side of the ideal gap timing decays to its floor, yd.
    pub charge_timing_span: f32,
    /// How much of the charge is lost at the worst possible timing, `0..1`.
    pub charge_timing_penalty: f32,
    /// The speed (yd/s) a unit-mass, fully braced defender is worth. This is the
    /// one number that sets the *scale* of the contest: raise it and every
    /// charge gets harder, lower it and the back runs through the world.
    pub charge_resist_speed: f32,
    /// How much of the defender's resistance survives being caught unsquared,
    /// `0..1` — the floor of the brace term.
    pub charge_brace_floor: f32,
    /// Speed a beaten defender is knocked back at, yd/s per unit of overload.
    pub charge_knock_speed: f32,
    /// Overload (impulse / resistance) above which the beaten defender is put
    /// clean off his feet rather than merely staggered.
    pub charge_airborne_overload: f32,
    /// The fraction of speed the back keeps through a *won* charge — contact
    /// costs something even when it is won.
    pub charge_win_keep: f32,
    /// The fraction he keeps through a *lost* one. Low enough that the defender
    /// closing behind him lands the tackle, which is what makes a bad charge a
    /// real mistake rather than a free attempt.
    pub charge_loss_keep: f32,

    // --- the leap -----------------------------------------------------------
    /// Vertical launch speed, yd/s. With [`Self::jump_gravity`] this sets the
    /// apex, which must clear a standing player (~2 yd) to do its job.
    pub jump_launch_speed: f32,
    /// The leap's own gravity, yd/s². Deliberately far heavier than the ball's:
    /// under real gravity an apex this high hangs for well over a second, which
    /// is floaty rather than arcade. Heavier gravity buys the same height in a
    /// shorter, snappier arc.
    pub jump_gravity: f32,
    /// Simulation ticks before another leap may begin, measured from launch.
    pub jump_cooldown_ticks: u64,
    /// The height the back's feet must exceed while a defender passes beneath
    /// him for the encounter to count as cleared, yd — a defender's tackling
    /// reach.
    pub hurdle_min_height: f32,
    /// Extra horizontal margin beyond the two body radii inside which a defender
    /// counts as having passed *through* the encounter region, yd.
    pub hurdle_reach: f32,
}

impl Default for RunbackTuning {
    fn default() -> Self {
        RunbackTuning {
            juke_speed: 9.0,
            juke_ticks: 14,
            juke_recovery_ticks: 20,
            juke_forward_keep: 0.9,

            dodge_threat_range: 4.6,
            dodge_lookahead_ticks: 26,
            dodge_resolve_ticks: 60,
            dodge_clear_yards: 0.6,

            shoulder_ticks: 42,
            shoulder_recovery_ticks: 26,
            shoulder_expire_ticks: 8,
            shoulder_reach: 0.4,
            charge_drive: 2.2,
            charge_ideal_gap: 3.4,
            charge_timing_span: 3.0,
            charge_timing_penalty: 0.6,
            charge_resist_speed: 5.6,
            charge_brace_floor: 0.55,
            charge_knock_speed: 5.5,
            charge_airborne_overload: 1.5,
            charge_win_keep: 0.74,
            charge_loss_keep: 0.32,

            jump_launch_speed: 11.2,
            jump_gravity: 26.0,
            jump_cooldown_ticks: 180,
            hurdle_min_height: 1.5,
            hurdle_reach: 0.5,
        }
    }
}
