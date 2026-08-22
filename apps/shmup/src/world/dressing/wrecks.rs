//! Ported from Claude-of-Duty `src/world/dressing.js:1030-1074` — `wrecks`:
//! the burnt-out saloons from `SET_PIECES.wrecks`, their wheels (two flat,
//! one missing, the hub resting on a block), the scorch patch and the debris
//! field around each one.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{ll, patch_geometry};
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::occupancy::{ground_y, is_open};

const DEBRIS: [&str; 6] = ["brick_b", "rock_b", "litter", "plank_b", "can", "glass_shards"];

/// `wheelPos` (`dressing.js:1037-1042`).
const WHEEL_POS: [[f64; 2]; 4] = [[0.86, 1.35], [-0.86, 1.35], [0.86, -1.35], [-0.86, -1.35]];

/// `wrecks(A, rng)` (`dressing.js:1031-1074`).
pub fn wrecks(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, ry, roll] in SET_PIECES.wrecks.iter().copied() {
        let y = ground_y(x, z);
        asm.put(
            "wreck",
            x as f32,
            (y + 0.02) as f32,
            z as f32,
            ry as f32,
            1.0,
            Some([1.0, 1.0, 1.0]),
            0.0,
            ((roll * std::f64::consts::PI) / 180.0) as f32,
        );
        asm.collide_box(Surface::Metal, x as f32, (y + 0.75) as f32, z as f32, 1.85, 1.5, 4.4, ry as f32);
        // wheels: two flat, one missing, the hub resting on a block
        for (i, [lx, lz]) in WHEEL_POS.iter().copied().enumerate() {
            if i == 3 {
                continue;
            }
            let px = x + ry.cos() * lx + ry.sin() * lz;
            let pz = z - ry.sin() * lx + ry.cos() * lz;
            asm.put("wheel_flat", px as f32, (y + 0.2) as f32, pz as f32, ry as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        }
        asm.put(
            "block_small",
            (x + ry.cos() * -0.86 + ry.sin() * -1.35) as f32,
            y as f32,
            (z - ry.sin() * -0.86 + ry.cos() * -1.35) as f32,
            ry as f32,
            1.0,
            Some([1.0, 1.4, 1.0]),
            0.0,
            0.0,
        );
        // scorch and debris field
        let radius = rng.range(2.6, 3.4);
        let scorch = patch_geometry(rng, radius, 11, 0.5, 0.0);
        let sry = rng.float() * 6.28;
        let m = ll(&Mat4::IDENTITY, x as f32, (y + 0.008) as f32, z as f32, sry as f32, 1.0, 1.0, 0.7, 0.0, 0.0);
        asm.add_once("asphalt", &scorch, Some(&m), Some(AccumAddOpts { masks: Some([0.05, 1.0, 0.9]), paint: None }));
        for _ in 0..18 {
            let a = rng.float() * 6.28;
            let r = rng.range(1.2, 4.5);
            let px = x + a.cos() * r;
            let pz = z + a.sin() * r;
            if !is_open(px, pz, 0.2) {
                continue;
            }
            let id = *rng.pick(&DEBRIS);
            let py = ground_y(px, pz) + 0.02;
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.6, 1.2);
            asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
        }
        let try_ = rng.float() * 6.28;
        asm.put("tyre", (x + ry.cos() * 1.6) as f32, y as f32, (z - ry.sin() * 1.6) as f32, try_ as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
    }
}
