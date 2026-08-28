//! Ported from Claude-of-Duty `src/world/dressing.js:1-2269` — the whole
//! file, split one submodule per prop family plus one for each scatter pass,
//! matching the source's own banner comments and the house style already set
//! by `crate::world::props` and `crate::world::kit`.
//!
//! **WORLD — set dressing.** Geometry makes a level; dressing makes it a
//! *place*. This pass adds the hundreds of instanced props that turn a street
//! of boxes into a market that people evidently live in: stalls under fabric
//! canopies, sandbag emplacements, jersey barriers, wrecked cars, palms,
//! lamps, cables and laundry strung overhead, roof clutter, rubble, and the
//! litter and blown sand that collects against every wall base.
//!
//! Everything is placed in LEVEL space and instanced through the
//! [`crate::world::assembler::Assembler`], so the cost of another two hundred
//! props is a few kilobytes of matrices.
//!
//! ## Where each source section landed
//!
//! | `dressing.js` | module |
//! |---|---|
//! | `inBuilding`/`isOpen`/`groundY`/`groundSkirt`/`nearestWall`/`camClear`/`jitterRig` | [`occupancy`] |
//! | `registerDressingProps` | [`prototypes`] |
//! | `dressStreet` | [`street`] |
//! | `marketStalls` | [`stalls`] |
//! | `barriers` | [`barriers`] |
//! | `sandbagEmplacements`/`sandbagWall` | [`sandbags`] |
//! | `wrecks` | [`wrecks`] |
//! | `palms` | [`palms`] |
//! | `streetLamps` | [`lamps`] |
//! | `overheadLines`/`facadeHangings` | [`lines`] |
//! | `rubblePiles` | [`rubble`] |
//! | `tyreStack`/`tyreStacks` | [`tyres`] |
//! | `coverClusters` | [`cover`] |
//! | `streetFloor` | [`street_floor`] |
//! | `dressBuildings`/`dressBuilding`/`alleyLines` | [`buildings`] |
//! | `scatterDebris` | [`scatter`] |
//! | `gateAperture`/`merlonRun`/`buildGate` | [`gate`] |
//! | `buildPerimeter` | [`perimeter`] |
//!
//! Two `src/world/util.js` primitives this pass is the only current caller of
//! ([`berm::drift_berm`] and [`cable::catenary_tube`]) also live here; see
//! [`berm`]'s module doc for why, and for the note that they belong in
//! `crate::world::kit::primitives` the moment a second caller appears.
//!
//! ## Determinism
//!
//! Every pass draws from one shared `rng`, in a fixed order. Two source
//! idioms in here are easy to lose in translation and both change the draw
//! count if you do:
//!
//! 1. **`for (let i = 0; i < rng.int(a, b); i++)`** re-evaluates its
//!    condition every iteration, so `rng.int` is drawn once per test —
//!    including the final failing one. Sixteen loops in `dressing.js` are
//!    written this way. [`int_loop_continues`] is how this port spells it.
//! 2. **`&&` / `||` / `??` / `?:` short-circuit.** `isOpen(...) && rng.float()
//!    < 0.96`, `i > 0 && rng.float() < lyingP`, `opts.pebbles ?? rng.int(4,
//!    8)`, `lying ? 0 : rng.range(...)` — every one of these skips a draw on
//!    one branch. Each site is commented where it appears.
//!
//! Argument evaluation is left-to-right in JavaScript, so a call like
//! `A.put(rng.pick(ids), x, y, z, rng.float() * 6.28, rng.range(...), [1,
//! rng.range(...), 1])` draws in exactly that order; this port hoists each
//! draw into a `let` in the same order rather than relying on Rust's own
//! argument evaluation order.

pub mod barriers;
pub mod berm;
pub mod buildings;
pub mod cable;
pub mod cover;
pub mod gate;
pub mod lamps;
pub mod lines;
pub mod occupancy;
pub mod palms;
pub mod perimeter;
pub mod prototypes;
pub mod rubble;
pub mod sandbags;
pub mod scatter;
pub mod stalls;
pub mod street;
pub mod street_floor;
pub mod tyres;
pub mod wrecks;

pub use buildings::dress_buildings;
pub use gate::build_gate;
pub use occupancy::{cam_clear, ground_skirt, ground_y, in_building, is_open, jitter_rig, nearest_wall, SkirtOpts};
pub use perimeter::build_perimeter;
pub use prototypes::register_dressing_props;
pub use sandbags::sandbag_wall;
pub use scatter::scatter_debris;
pub use street::dress_street;
pub use tyres::tyre_stack;

use crate::jsmath;
use crate::rng::Rng;

/// One iteration test of the source's `for (let i = 0; i < rng.int(min, max);
/// i++)` idiom.
///
/// A JavaScript `for` loop re-evaluates its condition on **every** pass, so
/// `rng.int(min, max)` is a fresh draw each time — including the final test
/// that ends the loop. Reading it as "draw a count once, then loop" is the
/// single easiest way to desynchronise this whole file's stream: it changes
/// both the iteration count *and* the number of draws consumed. Sixteen
/// loops in `dressing.js` are written this way.
///
/// Spelled as `while int_loop_continues(rng, i, min, max) { …; i += 1; }`,
/// with `i += 1` placed so a `continue` in the body still bumps it (matching
/// the `i++` in the loop header).
pub(crate) fn int_loop_continues(rng: &mut Rng, i: i32, min: i32, max: i32) -> bool {
    i < rng.int(min, max)
}

/// `stripedCloth`'s `bands` / `segX` defaults (`kit.js:837,840`):
/// `bands ?? max(3, round(w / 0.38))` and `segX ?? max(2, round(24 / bands))`.
///
/// `crate::world::kit::striped_cloth_default_bands` /
/// `…_default_seg_x` are the same two formulas, but they take and divide
/// `f32`. Rounding a division is exactly where an `f32` narrowing can flip an
/// integer result, and a different band count is a different mesh — so the
/// dressing pass computes them in `f64`, as the source's JS numbers do.
pub(crate) fn striped_cloth_defaults(w: f64) -> (u32, u32) {
    let bands = (jsmath::round(w / 0.38) as i64).max(3) as u32;
    let seg_x = (jsmath::round(24.0 / f64::from(bands)) as i64).max(2) as u32;
    (bands, seg_x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_loop_draws_once_per_test_including_the_failing_one() {
        // Three body iterations means FOUR draws (0<n0, 1<n1, 2<n2, 3>=n3).
        let mut rng = Rng::new(4);
        let mut counted = Rng::new(4);
        let mut i = 0;
        while int_loop_continues(&mut rng, i, 1, 6) {
            i += 1;
        }
        for _ in 0..=i {
            counted.int(1, 6);
        }
        assert_eq!(rng.state(), counted.state(), "one draw per test, i={i}");
    }

    #[test]
    fn striped_cloth_defaults_match_the_sources_formulas() {
        // w = 2.5 -> round(6.578) = 7 bands -> round(24/7) = 3 segX.
        assert_eq!(striped_cloth_defaults(2.5), (7, 3));
        // Clamped floors.
        assert_eq!(striped_cloth_defaults(0.1), (3, 8));
    }
}
