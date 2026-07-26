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

use crate::ai::{select_defense, variation_key};
use crate::data::prototype::{triple_read, PROTOTYPE_LINE};
use crate::data::PlayDefinition;
use crate::identity::TeamId;
use crate::launch::{resolve_defense, RunConfig};
use crate::state::SimState;

use super::PROTOTYPE_HEAT;

/// The down/distance the defensive selector is asked to answer. Fixed so the
/// coverage mix stays a stable, understandable distribution rather than
/// drifting with a game state the prototype does not have.
const NOMINAL_DOWN: u8 = 2;
const NOMINAL_DISTANCE: f32 = 10.0;

/// Install attempt `index`'s play into `sim` and return which defensive call it
/// drew (for inspection and the debug overlay).
pub fn install(sim: &mut SimState, config: &RunConfig, index: u32) -> usize {
    let offense = triple_read();
    let key = variation_key(config.seed, u64::from(index), NOMINAL_DOWN);
    let selection = select_defense(
        offense.tag,
        NOMINAL_DOWN,
        NOMINAL_DISTANCE,
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
