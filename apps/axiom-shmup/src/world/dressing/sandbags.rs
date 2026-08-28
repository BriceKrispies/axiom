//! Ported from Claude-of-Duty `src/world/dressing.js:1200-1276` —
//! `sandbagEmplacements` and the course-laid `sandbagWall` every emplacement,
//! cover cluster and rampart in the level is built from.
//!
//! What makes a stack of bags read as cover rather than as a tray of bread
//! rolls: three different bag silhouettes picked so neighbours rarely match;
//! the bags INTERPENETRATE (1-2 cm of overlap along the run and between
//! courses closes the daylight gaps that turn a wall into a lattice); squash
//! grows with the number of bags above; per-bag yaw/scale/row-pitch jitter;
//! the odd header bag laid across the run.

use crate::jsmath;
use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::{ground_skirt, ground_y, is_open, SkirtOpts};

const BAG_W: f64 = 0.5;
const BAG_H: f64 = 0.17;
const IDS: [&str; 3] = ["sandbag_a", "sandbag_b", "sandbag_c"];

/// What people leave behind the run (`dressing.js:1021`).
const BEHIND: [&str; 4] = ["jerry_can", "crate_b", "box_card_a", "gas_bottle"];

/// `sandbagEmplacements(A, rng)` (`dressing.js:1201-1207`): five courses —
/// interpenetrating, load-squashed bags stack lower than a rigid 15.5 cm
/// pitch did, and this cover has to stay chest-high to a crouch.
pub fn sandbag_emplacements(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, ry, len] in SET_PIECES.sandbag_walls.iter().copied() {
        sandbag_wall(asm, rng, x, z, ry, len, 5, None);
    }
}

/// `sandbagWall(A, rng, x, z, ry, len, courses = 3, baseY = null)`
/// (`dressing.js:1229-1276`).
///
/// `base_y` puts the run on a roof or a rampart walkway instead of the
/// street — and, exactly as in the source, a non-`null` `base_y` returns
/// early before any ground clutter (a rampart run has nothing behind it).
#[allow(clippy::too_many_arguments)]
pub fn sandbag_wall(asm: &mut Assembler, rng: &mut Rng, x: f64, z: f64, ry: f64, len: f64, courses: i32, base_y: Option<f64>) {
    let y = base_y.unwrap_or_else(|| ground_y(x, z));
    let mut cy = y + 0.01;
    let mut prev: i32 = -1;
    for c in 0..courses {
        // load from the bags above: the bottom of a five-high wall carries
        // most of it
        let load = f64::from(courses - 1 - c) / f64::from(1.max(courses - 1));
        let squash = 1.0 - load * 0.19; // vertical
        let spread = 1.0 + load * 0.07; // and it bulges out sideways
        // 2-4 cm of row-pitch jitter, so course seams never stack vertically
        let pitch = BAG_W - rng.range(0.02, 0.04);
        let per = (jsmath::round(len / pitch) as i64).max(2) as i32;
        let stagger = f64::from(c % 2) * pitch * 0.5 + rng.range(-0.03, 0.03);
        let shrink = i32::from(c == courses - 1 && courses > 2);
        let bag_h = BAG_H * squash;
        for i in shrink..(per - shrink) {
            let lx = -len / 2.0 + stagger + (f64::from(i) + 0.5) * pitch;
            if lx.abs() > len / 2.0 {
                continue;
            }
            // never the same silhouette twice in a row
            let mut pick = rng.int(0, 2);
            if pick == prev {
                pick = (pick + 1 + rng.int(0, 1)) % 3;
            }
            prev = pick;
            // Headers: bags turned across the run.
            let header = rng.float() < 0.3;
            let lz = rng.range(-0.03, 0.03) + if header { rng.range(-0.05, 0.05) } else { 0.0 };
            let px = x + ry.cos() * lx + ry.sin() * lz;
            let pz = z - ry.sin() * lx + ry.cos() * lz;
            let byaw = ry + if header { std::f64::consts::FRAC_PI_2 } else { 0.0 } + rng.range(-0.21, 0.21);
            let sx = rng.range(0.9, 1.12) * spread;
            let sy = rng.range(0.9, 1.06) * squash;
            let sz = rng.range(0.94, 1.12) * spread;
            let grime = rng.range(0.7, 1.6);
            let ao = rng.range(0.85, 1.3);
            let brx = rng.range(-0.09, 0.09);
            let brz = rng.range(-0.11, 0.11);
            asm.put_s(
                IDS[pick as usize],
                px as f32,
                // the bag prop's origin is its base, so scale never lifts it
                cy as f32,
                pz as f32,
                byaw as f32,
                sx as f32,
                sy as f32,
                sz as f32,
                Some([1.0, grime as f32, ao as f32]),
                brx as f32,
                brz as f32,
            );
        }
        // the next course beds 1.5-2.5 cm into this one
        cy += bag_h - rng.range(0.015, 0.025);
    }
    let h = (0.2f64).max(cy - y + 0.06);
    asm.collide_box(Surface::Fabric, x as f32, (y + h / 2.0) as f32, z as f32, len as f32, h as f32, 0.46, ry as f32);
    if base_y.is_some() {
        return; // a rampart run: no ground clutter behind it
    }
    // spilled sand and grit along the foot of the run
    let skirts = (jsmath::round(len / 1.1) as i64).max(2) as i32;
    for i in 0..skirts {
        let lx = -len / 2.0 + ((f64::from(i) + 0.5) / f64::from(skirts)) * len;
        let pebbles = rng.int(1, 3);
        ground_skirt(
            asm,
            rng,
            x + ry.cos() * lx,
            y,
            z - ry.sin() * lx,
            0.44,
            SkirtOpts { pebbles: Some(pebbles), key: "sand", grime: 0.7, ..SkirtOpts::default() },
        );
    }
    // ammo tins and a jerry can behind the wall
    let mut i = 0;
    while int_loop_continues(rng, i, 1, 3) {
        let lx = rng.range(-len / 2.0, len / 2.0);
        let px = x + ry.cos() * lx + ry.sin() * 0.7;
        let pz = z - ry.sin() * lx + ry.cos() * 0.7;
        i += 1;
        if !is_open(px, pz, 0.3) {
            continue;
        }
        let id = *rng.pick(&BEHIND);
        let py = ground_y(px, pz);
        let pry = rng.float() * 6.28;
        asm.put(id, px as f32, py as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rampart_run_skips_the_ground_clutter_entirely() {
        // `baseY !== null` returns before the skirts and the ammo tins, so a
        // rampart run consumes strictly fewer draws than a street run.
        let mut asm_a = Assembler::new(Rng::new(1));
        let mut a = Rng::new(77);
        sandbag_wall(&mut asm_a, &mut a, 0.0, 0.0, 0.0, 2.4, 3, Some(4.0));

        let mut asm_b = Assembler::new(Rng::new(1));
        let mut b = Rng::new(77);
        sandbag_wall(&mut asm_b, &mut b, 0.0, 0.0, 0.0, 2.4, 3, None);

        assert_ne!(a.state(), b.state());
        let out = asm_a.finalize();
        // No dirt/sand statics at all on a rampart run.
        assert!(out.statics.iter().all(|s| s.key != "sand" && s.key != "dirt"));
    }
}
