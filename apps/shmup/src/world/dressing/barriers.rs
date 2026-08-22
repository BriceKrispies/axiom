//! Ported from Claude-of-Duty `src/world/dressing.js:839-928` — `barriers`:
//! the jersey barriers from `SET_PIECES.jerseys`, plus the heavier concrete
//! blocks laid as chest-high cover at street corners.

use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::{ground_skirt, ground_y, SkirtOpts};

const ON_TOP: [&str; 2] = ["sandbag_a", "sandbag_b"];
const AGAINST: [&str; 4] = ["tyre", "crate_a", "barrel_rust", "block_small"];
const LITTER: [&str; 4] = ["brick_a", "brick_b", "rock_b", "litter"];

/// Heavier concrete blocks as chest-high cover at street corners
/// (`dressing.js:903-911`): `[x, z, ry]`.
const BLOCKS: [[f64; 3]; 7] = [
    [-4.0, 22.0, 0.1],
    [4.2, 14.5, -0.15],
    [-4.3, -1.0, 0.05],
    [4.3, -12.0, 0.2],
    [-4.1, -30.0, -0.1],
    [4.0, -37.5, 0.12],
    [-2.0, -41.0, 1.5],
];

/// `barriers(A, rng)` (`dressing.js:840-928`).
pub fn barriers(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, ry] in SET_PIECES.jerseys.iter().copied() {
        let y = ground_y(x, z);
        let jr = ry + rng.range(-0.05, 0.05);
        let grime = rng.range(0.8, 1.3);
        let jrz = rng.range(-0.02, 0.02);
        asm.put("jersey", x as f32, y as f32, z as f32, jr as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, jrz as f32);
        asm.collide_box(Surface::Concrete, x as f32, (y + 0.46) as f32, z as f32, 0.62, 0.92, 1.9, jr as f32);
        // dragged into place: dust skirt and spalled grit along the splayed foot
        for t in [-0.55f64, 0.55] {
            let pebbles = rng.int(2, 4);
            ground_skirt(
                asm,
                rng,
                x + jr.sin() * t * 1.1,
                y,
                z + jr.cos() * t * 1.1,
                0.52,
                SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() },
            );
        }
        // things people leave on top of / against a barrier
        if rng.float() < 0.4 {
            let id = *rng.pick(&ON_TOP);
            let px = x + rng.range(-0.5, 0.5);
            let pz = z + rng.range(-0.6, 0.6);
            let pry = rng.float() * 6.28;
            asm.put(id, px as f32, (y + 0.92) as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        }
        if rng.float() < 0.45 {
            let ox = x + jr.cos() * rng.range(0.5, 0.9);
            let oz = z - jr.sin() * rng.range(0.5, 0.9);
            let id = *rng.pick(&AGAINST);
            let pry = rng.float() * 6.28;
            asm.put(id, ox as f32, ground_y(ox, oz) as f32, oz as f32, pry as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
        }
        let mut i = 0;
        while int_loop_continues(rng, i, 1, 4) {
            let id = *rng.pick(&LITTER);
            let px = x + rng.range(-1.2, 1.2);
            let pz = z + rng.range(-1.4, 1.4);
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.6, 1.1);
            asm.put(id, px as f32, (y + 0.03) as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
            i += 1;
        }
    }

    for [x, z, ry] in BLOCKS {
        let y = ground_y(x, z);
        let grime = rng.range(0.9, 1.3);
        asm.put("block_big", x as f32, y as f32, z as f32, ry as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        asm.collide_box(Surface::Concrete, x as f32, (y + 0.48) as f32, z as f32, 1.3, 0.96, 0.9, ry as f32);
        // a block this heavy grinds a dirt halo into the deck when it is dropped
        for t in [-0.4f64, 0.4] {
            let pebbles = rng.int(2, 5);
            ground_skirt(asm, rng, x + ry.cos() * t, y, z - ry.sin() * t, 0.62, SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() });
        }
        if rng.float() < 0.6 {
            let px = x + rng.range(-1.0, 1.0);
            let pz = z + rng.range(-1.0, 1.0);
            let pry = rng.float() * 6.28;
            asm.put("block_small", px as f32, y as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        }
    }
}
