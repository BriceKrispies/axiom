//! Ported from Claude-of-Duty `src/world/dressing.js:1358-1401` —
//! `coverClusters`: deliberate cover at chest height along the street, so the
//! map plays — something to break contact behind every ~12 m of open ground.

use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::{ground_skirt, ground_y, is_open, SkirtOpts};
use super::sandbags::sandbag_wall;

/// `spots` (`dressing.js:1363-1370`): `[x, z, ry]`.
const SPOTS: [[f64; 3]; 6] = [
    [0.6, 0.9, 0.35],
    [-2.2, 8.6, 1.2],
    [2.6, -6.4, -0.4],
    [-3.0, -21.5, 0.6],
    [2.2, -33.0, 1.9],
    [-2.6, 27.5, 0.2],
];

const BESIDE: [&str; 3] = ["crate_c", "barrel_rust", "block_small"];
const LITTER: [&str; 6] = ["brick_a", "brick_b", "litter", "can", "rock_b", "plank_a"];

/// `coverClusters(A, rng)` (`dressing.js:1362-1401`).
pub fn cover_clusters(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, ry] in SPOTS {
        let y = ground_y(x, z);
        // six squashed courses ~= 0.8 m: cover you can shoot over crouched,
        // not standing. The length draw happens before `sandbagWall` runs.
        let len = rng.range(1.8, 2.8);
        sandbag_wall(asm, rng, x, z, ry, len, 6, None);
        let bx = x + (ry + 1.57).cos() * 1.5;
        let bz = z - (ry + 1.57).sin() * 1.5;
        if is_open(bx, bz, 0.4) {
            let id = *rng.pick(&BESIDE);
            let by = ground_y(bx, bz);
            let bry = rng.float() * 6.28;
            asm.put(id, bx as f32, by as f32, bz as f32, bry as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
            asm.collide_box(Surface::Wood, bx as f32, (y + 0.4) as f32, bz as f32, 0.8, 0.8, 0.8, 0.0);
            let pebbles = rng.int(3, 6);
            ground_skirt(asm, rng, bx, ground_y(bx, bz), bz, 0.5, SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() });
        }
        let mut i = 0;
        while int_loop_continues(rng, i, 3, 6) {
            let px = x + rng.range(-2.0, 2.0);
            let pz = z + rng.range(-2.0, 2.0);
            i += 1;
            if !is_open(px, pz, 0.2) {
                continue;
            }
            let id = *rng.pick(&LITTER);
            let py = ground_y(px, pz) + 0.02;
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.6, 1.2);
            asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
        }
    }
}
