//! Ported from Claude-of-Duty `src/world/dressing.js:177-184` —
//! `dressStreet`, the ordered run of every street-level dressing pass.

use crate::rng::Rng;
use crate::world::assembler::Assembler;

use super::barriers::barriers;
use super::cover::cover_clusters;
use super::lamps::street_lamps;
use super::lines::{facade_hangings, overhead_lines};
use super::occupancy::jitter_rig;
use super::palms::palms;
use super::rubble::rubble_piles;
use super::sandbags::sandbag_emplacements;
use super::stalls::market_stalls;
use super::street_floor::street_floor;
use super::tyres::tyre_stacks;
use super::wrecks::wrecks;

/// `dressStreet(A, rng)` (`dressing.js:177-192`).
///
/// The pass order is the determinism contract: every one of these draws from
/// the same shared `rng`, so moving a call moves every subsequent placement
/// in the level.
pub fn dress_street(asm: &mut Assembler, rng: &mut Rng) {
    // A FORK, not `rng` itself: drawing the jitter from the placement stream
    // would shift every subsequent position in the level and walk props into
    // the shot cameras' keepout zones.
    asm.jitter = Some(jitter_rig());
    market_stalls(asm, rng);
    barriers(asm, rng);
    sandbag_emplacements(asm, rng);
    wrecks(asm, rng);
    palms(asm, rng);
    street_lamps(asm, rng);
    overhead_lines(asm, rng);
    facade_hangings(asm, rng);
    rubble_piles(asm, rng);
    tyre_stacks(asm, rng);
    cover_clusters(asm, rng);
    street_floor(asm, rng);
    asm.jitter = None;
}
