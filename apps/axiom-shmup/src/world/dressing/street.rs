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

    // WHOLE SET-PIECES THE ARENA DRESSING REMOVES (`dressing.js:348-372`).
    //
    // Each of these builds a composite: a suppressible prototype plus raw
    // geometry that has no id — a stall's canopy and valance, a wreck's body
    // slab, a rubble pile's mound. Suppressing only the prototype left the raw
    // half behind, floating where its support used to be.
    // [`Assembler::muted`] runs the builder and swallows everything it emits,
    // so the set-piece disappears whole AND the shared RNG stream advances
    // exactly as it always did — which is what keeps every other set-piece in
    // the same place. See `crate::world::clutter`.
    //
    // `const drop = (fn) => A.muted(() => fn(A, rng));` (`dressing.js:359`),
    // spelled as a macro because a Rust closure taking `&mut Assembler` cannot
    // also hold the `&mut Rng` the pass needs.
    //
    // **Unconditional, as in the source.** `drop()` does not consult
    // `isSuppressed`/`suppresses`, so `?clutter=1` restores the individually
    // suppressed props (the debris scatter, the interiors, the seam stones,
    // the skirts, the road marks) but NOT these eight set-pieces. That is the
    // original's behaviour, transcribed rather than improved: the switch is a
    // side-by-side comparison aid, and the composites are the half of the
    // policy the source chose to make permanent.
    macro_rules! drop_set_piece {
        ($pass:ident) => {
            asm.muted(|a| $pass(a, rng))
        };
    }

    drop_set_piece!(market_stalls);
    drop_set_piece!(barriers);
    drop_set_piece!(sandbag_emplacements);
    drop_set_piece!(wrecks);
    drop_set_piece!(palms);
    street_lamps(asm, rng);
    overhead_lines(asm, rng);
    facade_hangings(asm, rng);
    drop_set_piece!(rubble_piles);
    drop_set_piece!(tyre_stacks);
    drop_set_piece!(cover_clusters);
    street_floor(asm, rng);
    asm.jitter = None;
}
