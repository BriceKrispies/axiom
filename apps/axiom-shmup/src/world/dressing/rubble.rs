//! Ported from Claude-of-Duty `src/world/dressing.js:1278-1307` —
//! `rubblePiles`, the hand-placed masonry heaps from `SET_PIECES.rubble`.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{ll, patch_geometry, rubble_mound, RubbleOpts};
use crate::world::layout::SET_PIECES;

use super::int_loop_continues;
use super::occupancy::ground_y;

/// `rubblePiles(A, rng)` (`dressing.js:1279-1307`).
pub fn rubble_piles(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, radius, count] in SET_PIECES.rubble.iter().copied() {
        let y = ground_y(x, z);
        rubble_mound(asm, rng, x as f32, y as f32, z as f32, radius as f32, count as u32, RubbleOpts { key: "concrete" });
        // dust ring
        let g = patch_geometry(rng, radius * 1.5, 12, 0.4, 0.0);
        let ry = rng.float() * 6.28;
        let m = ll(&Mat4::IDENTITY, x as f32, (y + 0.012) as f32, z as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.9, 0.6]), paint: None }));

        let mut i = 0;
        while int_loop_continues(rng, i, 2, 5) {
            let px = x + rng.range(-radius, radius);
            let pz = z + rng.range(-radius, radius);
            let pry = rng.float() * 6.28;
            asm.put("slab_shard", px as f32, (y + 0.06) as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
            i += 1;
        }
        let mut i = 0;
        while int_loop_continues(rng, i, 1, 3) {
            let px = x + rng.range(-radius, radius);
            let pz = z + rng.range(-radius, radius);
            let pry = rng.float() * 6.28;
            asm.put("rebar", px as f32, (y + 0.05) as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
            i += 1;
        }
        let mut i = 0;
        while int_loop_continues(rng, i, 3, 7) {
            let px = x + rng.range(-radius * 1.4, radius * 1.4);
            let pz = z + rng.range(-radius * 1.4, radius * 1.4);
            let pry = rng.float() * 6.28;
            let prx = rng.range(-0.2, 0.2);
            let prz = rng.range(-0.2, 0.2);
            asm.put("cinder", px as f32, (y + 0.02) as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.3, 1.0]), prx as f32, prz as f32);
            i += 1;
        }
    }
}
