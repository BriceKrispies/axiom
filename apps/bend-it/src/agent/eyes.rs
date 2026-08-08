//! The striker's eyes: what it can see, reduced to numbers.
//!
//! Perception is the *app's* job, not the agent substrate's. Which corner is
//! open, whether the keeper went the right way last time, how late a curve has to
//! break — every one of those names a Bend It noun, and `axiom-agent` must never
//! learn any of them. What crosses into the agent is five integers.

use crate::play::Session;

use super::{
    Striker, FACT_BEND_DEMAND, FACT_BREAK_LATENESS, FACT_LOFT_DEMAND, FACT_OPEN_HEIGHT,
    FACT_OPEN_SIDE, FACT_PACE_DEMAND,
};

impl Striker {
    /// The striker's eyes: what it can see this tick, as `(fact kind, scalar)`.
    pub fn sightings(&self, session: &Session) -> [(u16, f32); 6] {
        // Which side is open. A keeper that went one way is not going the other,
        // and a shape that scored is worth repeating; anything else, switch. With
        // nothing yet remembered, it takes its cue from where the keeper is
        // standing right now and attacks the side it is further from.
        let standing = session.keeper().motion().hips.x;
        let side = self
            .recall
            .map(|r| match r.scored {
                true => nonzero_sign(r.aimed),
                false => -nonzero_sign(r.keeper_went),
            })
            .unwrap_or_else(|| -nonzero_sign(standing + self.opening_bias()))
            * 0.94;
        // Low, and arced to get there. A keeper reads the first fraction of the
        // flight: a ball still climbing at that moment is read as arriving high,
        // and a keeper that has thrown its hands up cannot get them back down.
        let height = 0.15;
        let loft = 0.52;
        // Bend AWAY from the side being attacked, and only about half of what the
        // game allows — a shot bent to its limit swings wide and then comes back
        // *through* the dive it created.
        let bend = -nonzero_sign(side) * 0.48;
        // Break it late: movement before the keeper's correction is movement the
        // keeper answers.
        let lateness = 0.72;
        // Hit it hard. A keeper that has read you correctly still has to get
        // there, and pace is the one thing that takes time away from it.
        let pace = 0.88;
        [
            (FACT_OPEN_SIDE, side),
            (FACT_OPEN_HEIGHT, height),
            (FACT_BEND_DEMAND, bend),
            (FACT_BREAK_LATENESS, lateness),
            (FACT_LOFT_DEMAND, loft),
            (FACT_PACE_DEMAND, pace),
        ]
    }

    /// The one bit of identity its opening corner depends on.
    pub(super) fn opening_bias(&self) -> f32 {
        [0.01f32, -0.01][(self.seed % 2) as usize]
    }
}

/// `signum`, but never zero — a striker with no preference still picks a side.
pub fn nonzero_sign(v: f32) -> f32 {
    [1.0f32, -1.0][usize::from(v < 0.0)]
}

