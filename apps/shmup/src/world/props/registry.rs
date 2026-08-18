//! Ported from Claude-of-Duty `src/world/props.js:898-994` — `registerProps`,
//! which registers every instanced prototype this file builds. Prototype ids
//! are the vocabulary `dressing.js`/`interiors.js` (not ported by this
//! slice) draw from.
//!
//! Registration order matches the source exactly: every builder call is a
//! draw against the shared `rng` stream passed in, so reordering any of
//! these ~60 calls — or adding/removing one — would shift every subsequent
//! prototype's geometry. See the port recipe's determinism rules.

use crate::rng::Rng;
use crate::world::assembler::{Assembler, ProtoSpec};
use crate::world::geo::WorldGeo;
use crate::world::kit::{pock_geometry, rock_geometry};

use super::containers::{barrel, bucket, cardboard_box, crate_, gas_bottle, jerry_can};
use super::cover::{concrete_block, jersey_barrier, pallet, sandbag, tyre};
use super::debris::{bottle, brick_chunk, can, litter_paper, plank, rebar_bundle, slab_shard};
use super::furniture::{cabinet, chair, mattress, shelf_unit, stall, table};
use super::mesh::dust_skirt;
use super::services::{ac_unit, lamp_glass, roof_vent, sat_dish, street_lamp, water_tank};
use super::signage::{sign_board, sign_hanging};
use super::vegetation::{palm_frond, palm_tree, planter, shrub, weed_tuft};

/// `registerProps`'s per-prototype `opts` object (`props.js:905`). Defaults
/// match [`ProtoSpec`]'s own documented defaults.
#[derive(Debug, Clone, Copy)]
struct Opts {
    tilt: f32,
    sink: f32,
    skirt: f32,
    max_dist: f32,
    chunk: bool,
    cast_shadow: bool,
    receive_shadow: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { tilt: 0.0, sink: 0.0, skirt: 0.0, max_dist: 0.0, chunk: true, cast_shadow: true, receive_shadow: true }
    }
}

/// `LOOSE(tilt, sink)` (`props.js:914`): a prototype knocked out of true by
/// up to `tilt` radians and sunk `sink` metres so the raised corner doesn't
/// float — see `Assembler::put`'s doc for how these two drive placement.
fn loose(tilt: f32, sink: f32) -> Opts {
    Opts { tilt, sink, ..Opts::default() }
}

/// One registered prototype's full observable shape: what
/// `docs/work-manifests/shmup-port/02-port-recipe.md`'s
/// golden-capture method needs to check ("each prototype's vertex/triangle
/// count and palette-key set, the per-prototype metadata table, and the
/// mask values on a prototype with a real chamfer") without a public getter
/// on [`Assembler`] itself — `Assembler::finalize()` only surfaces a
/// prototype that has at least one *placed* instance
/// (`assembler.rs::finalize`'s `if n == 0 { continue; }`), and
/// `register_props` never places any. [`register_props`] returns one of
/// these per prototype, alongside doing the real `a.proto(...)`
/// registration — this is the one deliberate addition beyond a literal
/// `return A;` translation, and exists purely for testability (see
/// `tests/props_port.rs`).
#[derive(Debug, Clone)]
pub struct RegisteredProto {
    pub id: String,
    pub key: String,
    pub geo: WorldGeo,
    pub tilt: f32,
    pub sink: f32,
    pub skirt: f32,
    pub max_dist: f32,
    pub chunk: bool,
    pub cast_shadow: bool,
    pub receive_shadow: bool,
}

/// `const P = (id, key, geo, opts = {}) => A.proto(id, { geo, key, ...opts });`
/// (`props.js:905`), plus recording a [`RegisteredProto`] summary — see its
/// doc for why.
fn p(a: &mut Assembler, out: &mut Vec<RegisteredProto>, id: &str, key: &str, geo: WorldGeo, o: Opts) -> String {
    out.push(RegisteredProto {
        id: id.to_string(),
        key: key.to_string(),
        geo: geo.clone(),
        tilt: o.tilt,
        sink: o.sink,
        skirt: o.skirt,
        max_dist: o.max_dist,
        chunk: o.chunk,
        cast_shadow: o.cast_shadow,
        receive_shadow: o.receive_shadow,
    });
    a.proto(
        id,
        ProtoSpec {
            geo,
            key: key.to_string(),
            tilt: o.tilt,
            sink: o.sink,
            skirt: o.skirt,
            cast_shadow: o.cast_shadow,
            receive_shadow: o.receive_shadow,
            chunk: o.chunk,
            max_dist: o.max_dist,
            no_prepass: false,
        },
    )
}

/// `registerProps(A, rngIn)` (`props.js:903-994`). Returns one
/// [`RegisteredProto`] per registered prototype — see that struct's doc.
pub fn register_props(a: &mut Assembler, rng: &mut Rng) -> Vec<RegisteredProto> {
    let mut out = Vec::new();
    macro_rules! p {
        ($id:expr, $key:expr, $geo:expr, $opts:expr) => {
            p(a, &mut out, $id, $key, $geo, $opts)
        };
    }

    // ------------------------------------------------------------ containers --
    p!("crate_a", "wood_prop", crate_(rng, 0.64, true), Opts { skirt: 0.37, ..loose(0.09, 0.022) });
    p!("crate_b", "wood_prop", crate_(rng, 0.48, true), loose(0.10, 0.018));
    p!("crate_c", "wood_prop_dark", crate_(rng, 0.82, true), Opts { skirt: 0.45, ..loose(0.075, 0.026) });
    p!("crate_flat", "wood_prop", crate_(rng, 0.55, false), loose(0.10, 0.02));
    p!("box_card_a", "wood_pale", cardboard_box(rng, 0.46), loose(0.10, 0.016));
    p!("box_card_b", "wood_pale", cardboard_box(rng, 0.34), loose(0.11, 0.012));
    p!("barrel_rust", "metal_rust_prop", barrel(rng, 0.29, 0.88, 3), Opts { skirt: 0.28, ..loose(0.085, 0.014) });
    p!("barrel_blue", "metal_blue", barrel(rng, 0.28, 0.9, 2), Opts { skirt: 0.26, ..loose(0.085, 0.014) });
    p!("barrel_wood", "wood_prop_dark", barrel(rng, 0.31, 0.78, 4), Opts { skirt: 0.28, ..loose(0.09, 0.015) });
    p!("gas_bottle", "metal_green", gas_bottle(rng), Opts { skirt: 0.18, ..loose(0.07, 0.008) });
    p!("bucket", "metal_rust_prop", bucket(rng), loose(0.12, 0.008));
    p!("jerry_can", "metal_green", jerry_can(rng), loose(0.10, 0.01));

    // ------------------------------------------------------------------ cover --
    p!("sandbag_a", "burlap", sandbag(rng, 0), loose(0.085, 0.006));
    p!("sandbag_b", "burlap", sandbag(rng, 1), loose(0.09, 0.006));
    p!("sandbag_c", "burlap", sandbag(rng, 2), loose(0.095, 0.006));
    p!("jersey", "concrete_prop", jersey_barrier(rng), Opts { skirt: 0.69, max_dist: 0.0, ..Opts::default() });
    p!("block_big", "concrete_prop", concrete_block(rng, 1.25, 0.95, 0.85), Opts { skirt: 0.63, ..loose(0.05, 0.03) });
    p!("block_small", "concrete_dark", concrete_block(rng, 0.55, 0.42, 0.4), Opts { skirt: 0.31, ..loose(0.09, 0.018) });
    p!("tyre", "rubber", tyre(rng, 0.33), Opts { skirt: 0.33, ..loose(0.10, 0.008) });
    p!("tyre_small", "rubber", tyre(rng, 0.26), loose(0.11, 0.006));
    p!("pallet", "wood_prop", pallet(rng), Opts { skirt: 0.51, ..loose(0.055, 0.02) });

    // -------------------------------------------------------------- furniture --
    p!("table", "wood_prop_dark", table(rng, 1.5, 0.78, 0.8), Opts { skirt: 0.57, ..Opts::default() });
    p!("table_small", "wood_prop", table(rng, 0.9, 0.72, 0.7), Opts::default());
    p!("stall", "wood_prop_dark", stall(rng, 2.3), Opts { skirt: 0.90, max_dist: 0.0, ..Opts::default() });
    p!("shelf", "wood_prop_dark", shelf_unit(rng, 1.1, 1.9, 0.35), Opts { skirt: 0.42, ..Opts::default() });
    p!("mattress", "fabric_cream", mattress(rng), loose(0.06, 0.01));
    p!("chair", "wood_prop", chair(rng), loose(0.05, 0.012));
    p!("cabinet", "wood_prop_dark", cabinet(rng, 0.9, 1.15, 0.44), Opts { skirt: 0.42, ..Opts::default() });

    // --------------------------------------------------------------- services --
    p!("ac_unit", "metal_dark", ac_unit(rng), Opts::default());
    p!("sat_dish", "metal_dark", sat_dish(rng), Opts::default());
    p!("water_tank", "metal_blue", water_tank(rng), Opts { skirt: 0.48, ..Opts::default() });
    p!("roof_vent", "metal_rust", roof_vent(rng), Opts::default());
    p!("lamp_post", "metal_dark", street_lamp(rng, 5.4), Opts { skirt: 0.25, chunk: false, ..Opts::default() });
    p!("lamp_glass", "lamp_lens", lamp_glass(), Opts { chunk: false, cast_shadow: false, ..Opts::default() });

    // ----------------------------------------------------------------- debris --
    p!("brick_a", "brick", brick_chunk(rng), loose(0.16, 0.006));
    p!("brick_b", "brick", brick_chunk(rng), loose(0.16, 0.006));
    p!("rock_a", "concrete_prop", rock_geometry(rng, 0.26, 0, 0.7), Opts { max_dist: 90.0, ..Opts::default() });
    p!("rock_b", "concrete_dark", rock_geometry(rng, 0.17, 0, 0.8), Opts { max_dist: 70.0, cast_shadow: false, ..Opts::default() });
    p!("slab_shard", "concrete_prop", slab_shard(rng), loose(0.14, 0.01));
    p!("rebar", "metal_rust", rebar_bundle(rng), loose(0.10, 0.004));
    p!("plank_a", "wood_prop", plank(rng), Opts { max_dist: 90.0, ..loose(0.06, 0.004) });
    p!("plank_b", "wood_prop_dark", plank(rng), Opts { max_dist: 90.0, ..loose(0.06, 0.004) });
    p!("litter", "wood_pale", litter_paper(rng), Opts { max_dist: 45.0, cast_shadow: false, ..Opts::default() });
    // Contact fillets. Registered last so `put()` can find them, and never
    // given a skirt of their own.
    p!("dust_skirt", "dust_skirt", dust_skirt(rng), Opts { max_dist: 42.0, cast_shadow: false, ..Opts::default() });
    p!("bottle", "glass", bottle(rng), Opts { max_dist: 55.0, cast_shadow: false, ..Opts::default() });
    p!("can", "steel", can(rng), Opts { max_dist: 45.0, cast_shadow: false, ..Opts::default() });

    // -------------------------------------------------------------- vegetation --
    let palm = palm_tree(rng, 5.4);
    p!("palm_trunk", "wood_dark", palm.geo, Opts { skirt: 0.57, chunk: false, ..Opts::default() });
    p!("palm_frond", "foliage", palm_frond(rng, 2.7), Opts { chunk: false, receive_shadow: true, ..Opts::default() });
    p!("shrub", "foliage", shrub(rng, 0.85), Opts::default());
    p!("weeds", "foliage", weed_tuft(rng), Opts { max_dist: 40.0, ..Opts::default() });
    p!("planter", "concrete_prop", planter(rng), Opts { skirt: 0.33, ..loose(0.07, 0.014) });

    // ----------------------------------------------------------------- signage --
    p!("sign_board", "metal_blue", sign_board(rng, 1.6, 0.55), Opts { skirt: 0.18, ..Opts::default() });
    p!("sign_hang", "metal_green", sign_hanging(rng, 0.9, 0.62), Opts::default());

    // ------------------------------------------------------------------ damage --
    // 3.2 cm base radius: callers scale it 0.5-1.5x, so pocks land at 3-10 cm
    // across. At the old 5.5 cm base a single rifle strike was 16 cm wide.
    p!("pock", "concrete_dark", pock_geometry(rng, 0.032), Opts { max_dist: 65.0, cast_shadow: false, ..Opts::default() });

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_props_registers_every_prototype_exactly_once() {
        let mut a = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(20260818);
        register_props(&mut a, &mut rng);
        for id in [
            "crate_a", "crate_b", "crate_c", "crate_flat", "box_card_a", "box_card_b", "barrel_rust", "barrel_blue",
            "barrel_wood", "gas_bottle", "bucket", "jerry_can", "sandbag_a", "sandbag_b", "sandbag_c", "jersey",
            "block_big", "block_small", "tyre", "tyre_small", "pallet", "table", "table_small", "stall", "shelf",
            "mattress", "chair", "cabinet", "ac_unit", "sat_dish", "water_tank", "roof_vent", "lamp_post",
            "lamp_glass", "brick_a", "brick_b", "rock_a", "rock_b", "slab_shard", "rebar", "plank_a", "plank_b",
            "litter", "dust_skirt", "bottle", "can", "palm_trunk", "palm_frond", "shrub", "weeds", "planter",
            "sign_board", "sign_hang", "pock",
        ] {
            assert!(a.has(id), "missing prototype {id}");
        }
    }

    #[test]
    fn register_props_is_deterministic_for_a_fixed_seed() {
        // Same rng state in => same rng state out: proves the exact same
        // number and order of draws happened both times, which is the
        // property every builder above depends on (see this module's doc).
        let mut a1 = Assembler::new(Rng::new(1));
        let mut r1 = Rng::new(42);
        register_props(&mut a1, &mut r1);
        let next1 = r1.float();

        let mut a2 = Assembler::new(Rng::new(1));
        let mut r2 = Rng::new(42);
        register_props(&mut a2, &mut r2);
        let next2 = r2.float();

        assert_eq!(next1, next2);
    }
}
