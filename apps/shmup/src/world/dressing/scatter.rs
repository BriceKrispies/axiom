//! Ported from Claude-of-Duty `src/world/dressing.js:1725-1859` —
//! `scatterDebris`: the final pass, several hundred small instanced props
//! biased toward wall bases and kerbs, because that is where wind, water and
//! people put things. Empty ground is what makes a level read as a WebGL
//! demo.

use axiom_math::Mat4;

use crate::jsmath;
use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{ll, patch_geometry, rubble_mound, RubbleOpts};
use crate::world::layout::{ALLEYS, STREET};
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::{ground_y, in_building, is_open, jitter_rig, nearest_wall};

/// The road-surface vocabulary (`dressing.js:1789`). `litter` appears twice:
/// a weighted pick, not a typo.
const ROAD: [&str; 7] = ["litter", "can", "rock_b", "brick_b", "litter", "bottle", "weeds"];

/// `scatterDebris(A, rng)` (`dressing.js:1731-1859`).
pub fn scatter_debris(asm: &mut Assembler, rng: &mut Rng) {
    let z_min = STREET.z_min;
    let z_max = STREET.z_max;
    let kerb = STREET.kerb;
    asm.jitter = Some(jitter_rig());

    building_line(asm, rng, kerb, z_min, z_max);
    road_surface(asm, rng, z_min, z_max);
    alleys(asm, rng);
    vegetation(asm, rng, kerb, z_min, z_max);
    litter_drifts(asm, rng, kerb, z_min, z_max);

    asm.jitter = None;
}

/// `dressing.js:1735-1774`: against the building line, both sides of the
/// street.
fn building_line(asm: &mut Assembler, rng: &mut Rng, kerb: f64, z_min: f64, z_max: f64) {
    for _ in 0..340 {
        let side = if rng.float() < 0.5 { -1.0f64 } else { 1.0 };
        let z = rng.range(z_min + 1.0, z_max - 1.0);
        // "exponential falloff away from the wall" (the source's own comment)
        // — the implementation is a HALF-NORMAL, `|gauss()| * 0.75`.
        // Transcribed as written.
        let off = 0.12 + rng.gauss().abs() * 0.75;
        let x = side * (kerb - off);
        if !is_open(x, z, 0.05) {
            continue;
        }
        let y = ground_y(x, z);
        let pick = rng.float();
        let id: &str = if pick < 0.3 {
            "litter"
        } else if pick < 0.46 {
            *rng.pick(&["brick_a", "brick_b"])
        } else if pick < 0.58 {
            *rng.pick(&["rock_a", "rock_b"])
        } else if pick < 0.68 {
            "weeds"
        } else if pick < 0.76 {
            *rng.pick(&["can", "bottle"])
        } else if pick < 0.84 {
            *rng.pick(&["plank_a", "plank_b"])
        } else if pick < 0.9 {
            "cinder"
        } else if pick < 0.95 {
            *rng.pick(&["box_card_a", "box_card_b"])
        } else {
            *rng.pick(&["tyre_small", "bucket", "crate_b", "slab_shard"])
        };
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.65, 1.25);
        let grime = rng.range(1.0, 1.5);
        asm.put(id, x as f32, (y + 0.015) as f32, z as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
    }
}

/// `dressing.js:1777-1791`: the road surface — sparser, and pushed to the
/// gutters.
fn road_surface(asm: &mut Assembler, rng: &mut Rng, z_min: f64, z_max: f64) {
    for _ in 0..180 {
        let x = rng.range(-STREET.half_width + 0.1, STREET.half_width - 0.1) * (0.45 + 0.55 * rng.signed().abs());
        let z = rng.range(z_min + 1.0, z_max - 1.0);
        if !is_open(x, z, 0.05) {
            continue;
        }
        let id = *rng.pick(&ROAD);
        let py = ground_y(x, z) + 0.012;
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.6, 1.15);
        let grime = rng.range(1.0, 1.5);
        asm.put(id, x as f32, py as f32, z as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
    }
}

/// `dressing.js:1794-1830`: the alleys — denser, junkier.
fn alleys(asm: &mut Assembler, rng: &mut Rng) {
    for a in ALLEYS {
        let (x0, z0, x1, z1) = (a.x0, a.z0, a.x1, a.z1);
        let area = (x1 - x0) * (z1 - z0);
        let n = jsmath::round(area * 0.85) as i64;
        for _ in 0..n {
            let x = rng.range(x0 + 0.3, x1 - 0.3);
            let z = rng.range(z0 + 0.3, z1 - 0.3);
            if in_building(x, z, 0.25) {
                continue;
            }
            let near = nearest_wall(x, z);
            let wall_bias = if near.d < 1.2 { 1.0 } else { 0.45 };
            if rng.float() > wall_bias {
                continue;
            }
            let pick = rng.float();
            let id: &str = if pick < 0.2 {
                "litter"
            } else if pick < 0.34 {
                *rng.pick(&["brick_a", "brick_b", "cinder"])
            } else if pick < 0.46 {
                *rng.pick(&["rock_a", "rock_b"])
            } else if pick < 0.56 {
                "weeds"
            } else if pick < 0.64 {
                "shrub"
            } else if pick < 0.72 {
                *rng.pick(&["plank_a", "plank_b"])
            } else if pick < 0.8 {
                *rng.pick(&["crate_a", "crate_b", "crate_flat", "pallet"])
            } else if pick < 0.86 {
                *rng.pick(&["barrel_rust", "barrel_blue", "barrel_wood"])
            } else if pick < 0.9 {
                *rng.pick(&["tyre", "tyre_small"])
            } else if pick < 0.95 {
                *rng.pick(&["box_card_a", "box_card_b", "bucket", "jerry_can"])
            } else {
                *rng.pick(&["slab_shard", "rebar", "gas_bottle"])
            };
            let y = ground_y(x, z);
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.7, 1.2);
            let grime = rng.range(1.0, 1.5);
            asm.put(id, x as f32, (y + 0.015) as f32, z as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
            // big items get a collision box; scatter does not
            if id.starts_with("barrel") {
                asm.collide_box(Surface::Metal, x as f32, (y + 0.45) as f32, z as f32, 0.62, 0.9, 0.62, 0.0);
            } else if id.starts_with("crate") {
                asm.collide_box(Surface::Wood, x as f32, (y + 0.3) as f32, z as f32, 0.62, 0.6, 0.62, 0.0);
            }
        }
        // a skip-load of rubble at one end of each alley
        if rng.float() < 0.7 {
            let bx = if rng.float() < 0.5 { x0 + 1.6 } else { x1 - 1.6 };
            let bz = rng.range(z0 + 1.2, z1 - 1.2);
            if !in_building(bx, bz, 0.4) {
                let radius = rng.range(0.9, 1.8);
                let count = rng.int(12, 24);
                rubble_mound(asm, rng, bx as f32, ground_y(bx, bz) as f32, bz as f32, radius as f32, count as u32, RubbleOpts { key: "concrete" });
            }
        }
    }
}

/// `dressing.js:1833-1851`: vegetation in the cracks — kerb line, wall bases,
/// alley corners.
fn vegetation(asm: &mut Assembler, rng: &mut Rng, kerb: f64, z_min: f64, z_max: f64) {
    for _ in 0..220 {
        let side = if rng.float() < 0.5 { -1.0f64 } else { 1.0 };
        let z = rng.range(z_min + 1.0, z_max - 1.0);
        let at_kerb = rng.float() < 0.55;
        let x = if at_kerb {
            side * (STREET.half_width + rng.range(0.02, 0.3))
        } else {
            side * (kerb - rng.range(0.05, 0.35))
        };
        if !is_open(x, z, 0.02) {
            continue;
        }
        let id = if rng.float() < 0.78 { "weeds" } else { "shrub" };
        let py = ground_y(x, z) + 0.01;
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.6, 1.25);
        let grime = rng.range(1.0, 1.4);
        asm.put(id, x as f32, py as f32, z as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
    }
}

/// `dressing.js:1853-1858`: the sun-bleached litter drifts that collect in
/// corners. (Glass under a blown-out window is handled per-building.)
fn litter_drifts(asm: &mut Assembler, rng: &mut Rng, kerb: f64, z_min: f64, z_max: f64) {
    for _ in 0..60 {
        let side = if rng.float() < 0.5 { -1.0f64 } else { 1.0 };
        let z = rng.range(z_min + 2.0, z_max - 2.0);
        let x = side * (kerb - rng.range(0.1, 0.5));
        if !is_open(x, z, 0.05) {
            continue;
        }
        let radius = rng.range(0.3, 0.8);
        let g = patch_geometry(rng, radius, 8, 0.6, 0.0);
        let gry = rng.float() * 6.28;
        let m = ll(&Mat4::IDENTITY, x as f32, (ground_y(x, z) + 0.01) as f32, z as f32, gry as f32, 1.0, 1.0, 0.6, 0.0, 0.0);
        asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.95, 0.7]), paint: None }));
        let mut k = 0;
        while int_loop_continues(rng, k, 2, 6) {
            let px = x + rng.range(-0.5, 0.5);
            let pz = z + rng.range(-0.6, 0.6);
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.7, 1.2);
            asm.put(
                "litter",
                px as f32,
                (ground_y(x, z) + 0.02) as f32,
                pz as f32,
                pry as f32,
                ps as f32,
                Some([1.0, 1.5, 1.0]),
                0.0,
                0.0,
            );
            k += 1;
        }
    }
}
