//! Ported from Claude-of-Duty `src/world/dressing.js:710-837` —
//! `marketStalls`.
//!
//! The same module four times down one street is only a repeat if nothing
//! about it changes. Yaw, depth, canopy tension, colour, whether the roof has
//! a torn-out band, the side drape and the clutter all differ per stall.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::kit::{ll, striped_cloth, StripedClothOpts};
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::occupancy::{ground_skirt, ground_y, is_open, SkirtOpts};
use super::{int_loop_continues, striped_cloth_defaults};

const CANOPY: [&str; 3] = ["fabric_red", "fabric_teal", "fabric_cream"];
/// The second key is picked from a *differently ordered* copy of the same
/// three (`dressing.js:737`) — a different pick for the same roll, so the
/// order is part of the data.
const CANOPY_ALT: [&str; 3] = ["fabric_cream", "fabric_teal", "fabric_red"];

const ON_TABLE: [&str; 4] = ["box_card_a", "box_card_b", "crate_b", "bucket"];
const UNDER_TABLE: [&str; 5] = ["crate_a", "crate_b", "crate_flat", "sandbag_a", "tray"];

/// `marketStalls(A, rng)` (`dressing.js:711-837`).
pub fn market_stalls(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, ry0, w] in SET_PIECES.stalls.iter().copied() {
        let y = ground_y(x, z);
        let s = w / 2.3;
        let ry = ry0 + rng.range(-0.07, 0.07);
        // NOTE (source shape, not a port bug): only `sx` carries the
        // width scale `s`; `sy`/`sz` are the ~1.0 per-instance variation
        // multipliers. `dressing.js:719-723` really does read
        // `putS('stall', x, y, z, ry, s, rng.range(0.94, 1.05),
        // rng.range(0.95, 1.06), …)`.
        let sy = rng.range(0.94, 1.05);
        let sz = rng.range(0.95, 1.06);
        let grime = rng.range(0.8, 1.35);
        asm.put_s("stall", x as f32, y as f32, z as f32, ry as f32, s as f32, sy as f32, sz as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        // collision: the table volume plus the two post lines
        asm.collide_box(Surface::Wood, x as f32, (y + 0.45) as f32, z as f32, w as f32, 0.9, 1.05, ry as f32);
        // the legs stand IN something: dust and swept grit at each post line
        for t in [-0.42f64, 0.42] {
            let pebbles = rng.int(2, 5);
            ground_skirt(
                asm,
                rng,
                x + ry.cos() * w * t,
                y,
                z - ry.sin() * w * t,
                0.4,
                SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() },
            );
        }

        // canopy: striped cloth draped over the crossbars, sagging between
        // posts. Tension varies per stall.
        let cw = w * rng.range(1.02, 1.16);
        let cd = rng.range(1.32, 1.6);
        let keys = [*rng.pick(&CANOPY), *rng.pick(&CANOPY_ALT)];
        let slack = rng.range(0.8, 1.5);

        let roof_rz = rng.range(-0.05, 0.05);
        let roof_m = ll(
            &Mat4::IDENTITY,
            x as f32,
            (y + 2.02) as f32,
            z as f32,
            ry as f32,
            1.0,
            1.0,
            1.0,
            -std::f32::consts::FRAC_PI_2,
            roof_rz as f32,
        );
        // one band torn out or flapped back on the older stalls
        let skip_band = if rng.float() < 0.3 { rng.int(0, 5) } else { -1 };
        let roof_grime = rng.range(0.4, 0.7);
        let (bands, seg_x) = striped_cloth_defaults(cw);
        striped_cloth(
            asm,
            &keys,
            &roof_m,
            cw as f32,
            cd as f32,
            StripedClothOpts {
                bands,
                seg_x,
                seg_y: 7,
                sag: (0.19 * slack) as f32,
                wrinkle: (0.028 * slack) as f32,
                bulge: (0.05 * slack) as f32,
                thickness: 0.0028,
                fray: 0.012,
                skip_band,
                masks: [0.35, roof_grime as f32, 0.15],
                ..StripedClothOpts::default()
            },
            Some(rng),
        );

        // a valance hanging off the front edge, which is what reads as a market
        let val_m = ll(&Mat4::IDENTITY, x as f32, (y + 1.86) as f32, z as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
        let val_h = rng.range(0.24, 0.4);
        let val_grime = rng.range(0.45, 0.75);
        striped_cloth(
            asm,
            &keys,
            &val_m,
            cw as f32,
            val_h as f32,
            StripedClothOpts {
                bands,
                seg_x,
                seg_y: 3,
                sag: (0.06 * slack) as f32,
                wrinkle: (0.028 * slack) as f32,
                bulge: 0.0,
                thickness: 0.0026,
                fray: 0.016,
                masks: [0.4, val_grime as f32, 0.2],
                ..StripedClothOpts::default()
            },
            Some(rng),
        );

        // a drape closing one end of the stall on about half of them
        if rng.float() < 0.55 {
            let sd = if rng.float() < 0.5 { -1.0f64 } else { 1.0 };
            let ki = rng.int(0, 1) as usize;
            let drape_keys = [keys[ki]];
            let m = ll(
                &Mat4::IDENTITY,
                (x + ry.cos() * (cw / 2.0) * sd) as f32,
                (y + 1.42) as f32,
                (z - ry.sin() * (cw / 2.0) * sd) as f32,
                (ry + std::f64::consts::FRAC_PI_2) as f32,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            );
            let dw = cd * 0.9;
            let dh = rng.range(0.9, 1.3);
            let d_grime = rng.range(0.5, 0.8);
            // `segX` is given explicitly here (7); `bands` still defaults off
            // the cloth's own width.
            let (d_bands, _) = striped_cloth_defaults(dw);
            striped_cloth(
                asm,
                &drape_keys,
                &m,
                dw as f32,
                dh as f32,
                StripedClothOpts {
                    bands: d_bands,
                    seg_x: 7,
                    seg_y: 8,
                    sag: 0.09,
                    wrinkle: 0.042,
                    twist: 0.07,
                    thickness: 0.0026,
                    fray: 0.02,
                    masks: [0.35, d_grime as f32, 0.25],
                    ..StripedClothOpts::default()
                },
                Some(rng),
            );
        }

        // goods on the table
        let n = rng.int(3, 6);
        for _ in 0..n {
            let lx = rng.range(-w / 2.0 + 0.3, w / 2.0 - 0.3);
            let lz = rng.range(-0.35, 0.35);
            let px = x + ry.cos() * lx + ry.sin() * lz;
            let pz = z - ry.sin() * lx + ry.cos() * lz;
            if rng.float() < 0.5 {
                let tray_ry = ry + rng.range(-0.3, 0.3);
                asm.put("tray", px as f32, (y + 0.87) as f32, pz as f32, tray_ry as f32, 1.0, Some([1.0, 1.1, 1.0]), 0.0, 0.0);
                if rng.float() < 0.8 {
                    let pry = rng.float() * 6.28;
                    asm.put("produce", px as f32, (y + 0.89) as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
                }
            } else {
                let id = *rng.pick(&ON_TABLE);
                let pry = rng.float() * 6.28;
                let ps = rng.range(0.7, 1.0);
                asm.put(id, px as f32, (y + 0.87) as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
            }
        }

        // crates and sacks stuffed underneath and alongside
        let mut i = 0;
        while int_loop_continues(rng, i, 2, 5) {
            let lx = rng.range(-w / 2.0, w / 2.0);
            let lz = rng.range(-0.3, 0.3);
            let id = *rng.pick(&UNDER_TABLE);
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.85, 1.05);
            let g = rng.range(1.0, 1.4);
            asm.put(
                id,
                (x + ry.cos() * lx + ry.sin() * lz) as f32,
                (y + 0.02) as f32,
                (z - ry.sin() * lx + ry.cos() * lz) as f32,
                pry as f32,
                ps as f32,
                Some([1.0, g as f32, 1.0]),
                0.0,
                0.0,
            );
            i += 1;
        }

        let side_x = x + ry.cos() * (w / 2.0 + 0.5);
        let side_z = z - ry.sin() * (w / 2.0 + 0.5);
        if is_open(side_x, side_z, 0.4) {
            let bry = rng.float() * 6.28;
            asm.put("barrel_wood", side_x as f32, ground_y(side_x, side_z) as f32, side_z as f32, bry as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
            asm.collide_box(Surface::Wood, side_x as f32, (y + 0.4) as f32, side_z as f32, 0.66, 0.8, 0.66, 0.0);
        }
        let sry = rng.float() * 6.28;
        asm.put("stool", (x - ry.sin() * 0.95) as f32, y as f32, (z - ry.cos() * 0.95) as f32, sry as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
    }
}
