//! Ported from Claude-of-Duty `src/world/dressing.js:1076-1122` — `palms`:
//! trunk, a crown of 8-11 fronds, three dead fronds hanging under it, and the
//! ring of dirt, weeds and litter at the base.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{ll, patch_geometry};
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::ground_y;

/// `palms(A, rng)` (`dressing.js:1077-1122`).
pub fn palms(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, s] in SET_PIECES.palms.iter().copied() {
        let y = ground_y(x, z);
        let ry = rng.float() * 6.28;
        let grime = rng.range(0.8, 1.2);
        asm.put("palm_trunk", x as f32, y as f32, z as f32, ry as f32, s as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        let top_y = y + 5.4 * s;
        let n = rng.int(8, 11);
        for i in 0..n {
            let a = ry + (f64::from(i) / f64::from(n)) * 6.28 + rng.range(-0.16, 0.16);
            let tilt = rng.range(-0.55, 0.15);
            let drop = rng.range(0.05, 0.3);
            let sx = s * rng.range(0.85, 1.15);
            let sy = s * rng.range(0.85, 1.15);
            let sz = s * rng.range(0.85, 1.15);
            let fg = rng.range(0.7, 1.3);
            asm.put_s(
                "palm_frond",
                x as f32,
                (top_y - drop) as f32,
                z as f32,
                a as f32,
                sx as f32,
                sy as f32,
                sz as f32,
                Some([1.0, fg as f32, 1.0]),
                0.0,
                tilt as f32,
            );
        }
        // dead fronds hanging under the crown
        for _ in 0..3 {
            let a = ry + rng.float() * 6.28;
            asm.put_s(
                "palm_frond",
                x as f32,
                (top_y - 0.35) as f32,
                z as f32,
                a as f32,
                (s * 0.8) as f32,
                (s * 0.8) as f32,
                (s * 0.8) as f32,
                Some([1.0, 1.6, 1.0]),
                0.0,
                -1.35,
            );
        }
        asm.collide_box(Surface::Wood, x as f32, (y + 2.7 * s) as f32, z as f32, (0.42 * s) as f32, (5.4 * s) as f32, (0.42 * s) as f32, 0.0);
        // ring of dirt, weeds and litter at the base
        let radius = rng.range(0.9, 1.4);
        let g = patch_geometry(rng, radius, 10, 0.45, 0.0);
        let dry = rng.float() * 6.28;
        let m = ll(&Mat4::IDENTITY, x as f32, (y + 0.02) as f32, z as f32, dry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.8, 0.5]), paint: None }));
        let mut i = 0;
        while int_loop_continues(rng, i, 3, 7) {
            let a = rng.float() * 6.28;
            let r = rng.range(0.4, 1.2);
            let wry = rng.float() * 6.28;
            let ws = rng.range(0.7, 1.3);
            asm.put(
                "weeds",
                (x + a.cos() * r) as f32,
                (y + 0.02) as f32,
                (z + a.sin() * r) as f32,
                wry as f32,
                ws as f32,
                Some([1.0, 1.2, 1.0]),
                0.0,
                0.0,
            );
            i += 1;
        }
        if rng.float() < 0.5 {
            let px = x + rng.range(-1.4, 1.4);
            let pz = z + rng.range(-1.4, 1.4);
            let pry = rng.float() * 6.28;
            asm.put("planter", px as f32, y as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
        }
    }
}
