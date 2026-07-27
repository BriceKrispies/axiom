//! Building one attempt: the same offensive concept every time, against a
//! deterministically varied defensive answer, spotted at the fixed prototype
//! line.
//!
//! Two things are deliberately constant. The **offense** never changes, because
//! the prototype is testing a read, not a playbook. The **aggression** never
//! escalates, because it is testing a decision, not a difficulty curve. What
//! varies is the coverage the three reads have to beat — drawn from the app's
//! existing deterministic defensive selector, keyed only on the run seed and
//! the attempt number, so a session replays exactly and no two attempts in a
//! row present the same picture.

use axiom::prelude::Vec3;

use crate::ai::offense::SET_RANGE;
use crate::ai::assignment::offense_player;
use crate::ai::{select_defense, variation_key};
use crate::data::prototype::{concept_play, PROTOTYPE_LINE};
use crate::data::PlayDefinition;
use crate::identity::TeamId;
use crate::launch::{resolve_defense, RunConfig};
use crate::state::SimState;

use super::{ATTEMPT_DISTANCE, PROTOTYPE_HEAT};

/// The down the defensive selector is asked to answer. Fixed so the coverage
/// mix stays a stable, understandable distribution rather than drifting with a
/// game state the prototype does not have. Its distance partner is
/// [`super::ATTEMPT_DISTANCE`], shared with the field paint so the line drawn
/// on the turf is the line the defense was called against.
const NOMINAL_DOWN: u8 = 2;

/// Install attempt `index`'s play into `sim` and return which defensive call it
/// drew (for inspection and the debug overlay).
pub fn install(sim: &mut SimState, config: &RunConfig, index: u32, concept: usize) -> usize {
    let offense = concept_play(concept);
    let key = variation_key(config.seed, u64::from(index), NOMINAL_DOWN);
    let selection = select_defense(
        offense.tag,
        NOMINAL_DOWN,
        ATTEMPT_DISTANCE,
        PROTOTYPE_HEAT,
        key,
    );
    let play = PlayDefinition::compose(
        &offense,
        &selection.call,
        TeamId(0),
        sim.frame.direction,
        PROTOTYPE_LINE,
    );
    sim.install_play(play);
    let (defense, tuning) = resolve_defense(config, PROTOTYPE_HEAT);
    sim.reload_defense(defense, tuning);
    sim.respot(PROTOTYPE_LINE);
    selection.index
}

/// Whether the OFFENSE has reached the alignment its play wants.
///
/// This is the snap cue once a play has been called: the ball goes as soon as
/// the offense's shift is finished, so calling a play IS the snap count rather
/// than something you do while a timer you cannot influence runs down. The
/// pre-snap deadline stays as the fallback for the player who calls nothing.
///
/// Deliberately the offense only. The defense is shifting too — it has to
/// re-align to a formation that just moved — but a defender chasing a receiver
/// clear across the field would otherwise gate the snap on the SLOWEST player
/// on the field, and a call that took two seconds to answer is not a snap
/// count. Snapping while the defense is still sorting itself out is the reward
/// for calling early, and it is what a real offense does.
pub(super) fn offense_is_set(sim: &SimState) -> bool {
    sim.play
        .offense_assignments
        .iter()
        .enumerate()
        .map(|(slot, _)| offense_player(&sim.play, slot).index())
        .all(|index| {
            let player = &sim.players[index];
            let align = sim.assignments[index].align;
            Vec3::new(player.pos.x, align.y, player.pos.z).distance(align) <= SET_RANGE
        })
}
