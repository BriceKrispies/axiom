//! Ported from Claude-of-Duty `src/world/dressing.js:1309-1356` —
//! `tyreStack` and `tyreStacks`.
//!
//! Nobody stacks tyres concentrically: each one is dropped on the last, so
//! the stack walks 2-4 cm sideways per tyre, leans, and every tyre is turned
//! a few degrees off its neighbour. A coaxial pile of toruses is the most
//! obvious "instanced prop" tell in the level.

use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::occupancy::{ground_skirt, ground_y, SkirtOpts};

/// `tyreStack(A, rng, x, y, z, n)` (`dressing.js:1315-1343`).
pub fn tyre_stack(asm: &mut Assembler, rng: &mut Rng, x: f64, y: f64, z: f64, n: i32) {
    let walk_a = rng.float() * 6.28;
    let lean = rng.range(-0.05, 0.05);
    let mut ox = 0.0f64;
    let mut oz = 0.0f64;
    let mut yaw = rng.float() * 6.28;
    for i in 0..n {
        let a = walk_a + rng.range(-1.1, 1.1);
        let step = rng.range(0.02, 0.04);
        ox += a.cos() * step;
        oz += a.sin() * step;
        // 5-15 degrees of relative rotation, so the tread blocks never line up
        yaw += (if rng.float() < 0.5 { -1.0 } else { 1.0 }) * rng.range(0.087, 0.262);
        let id = if i % 2 != 0 { "tyre_small" } else { "tyre" };
        let sx = rng.range(0.97, 1.04);
        let sy = rng.range(0.9, 1.05);
        let sz = rng.range(0.97, 1.04);
        let grime = rng.range(0.88, 1.35);
        let ao = rng.range(0.9, 1.2);
        let rx = lean * rng.range(0.5, 1.5);
        let rz = rng.range(-0.05, 0.05);
        asm.put_s(
            id,
            (x + ox) as f32,
            (y + f64::from(i) * 0.168) as f32,
            (z + oz) as f32,
            yaw as f32,
            sx as f32,
            sy as f32,
            sz as f32,
            Some([1.0, grime as f32, ao as f32]),
            rx as f32,
            rz as f32,
        );
    }
}

/// `tyreStacks(A, rng)` (`dressing.js:1345-1356`).
pub fn tyre_stacks(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, n] in SET_PIECES.tyres.iter().copied() {
        let n = n as i32;
        let y = ground_y(x, z);
        tyre_stack(asm, rng, x, y, z, n);
        ground_skirt(asm, rng, x, y, z, 0.42, SkirtOpts::default());
        asm.collide_box(Surface::Rubber, x as f32, (y + (f64::from(n) * 0.175) / 2.0) as f32, z as f32, 0.68, (f64::from(n) * 0.175) as f32, 0.68, 0.0);
        if rng.float() < 0.6 {
            // on its side, leaning: no fillet, it is not standing on the ground
            asm.skirts = false;
            let px = x + rng.range(0.7, 1.1);
            let pz = z + rng.range(-0.6, 0.6);
            let pry = rng.float() * 6.28;
            asm.put("tyre", px as f32, y as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.3, 1.0]), 1.4, 0.0);
            asm.skirts = true;
        }
    }
}
