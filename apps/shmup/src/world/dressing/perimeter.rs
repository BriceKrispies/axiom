//! Ported from Claude-of-Duty `src/world/dressing.js:2207-2269` —
//! `buildPerimeter`: the map edge, a continuous wall of compound walls,
//! blocked side streets and distant infill so the playable 120 m reads as
//! part of a bigger town.

use axiom_math::Mat4;

use crate::jsmath;
use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{box_kit, box_soft_kit, ll, rubble_mound, RubbleOpts};
use crate::world::layout::STREET;
use crate::world::palette::Surface;

use super::occupancy::ground_y;

const WALL_KEYS: [&str; 3] = ["plaster_sand", "plaster_cream", "concrete"];
const BARRICADE_LITTER: [&str; 6] = ["brick_a", "brick_b", "cinder", "rock_a", "slab_shard", "rebar"];

/// `buildPerimeter(A, rng)` (`dressing.js:2212-2269`).
pub fn build_perimeter(asm: &mut Assembler, rng: &mut Rng) {
    const R: f64 = 58.0;
    // `[x0, z0, x1, z1]` runs of compound wall.
    let segs: [[f64; 4]; 4] = [[-R, -R, R, -R], [-R, R, R, R], [-R, -R, -R, R], [R, -R, R, R]];
    for [x0, z0, x1, z1] in segs {
        let dx = x1 - x0;
        let dz = z1 - z0;
        // `Math.hypot(dx, dz)` — see `crate::jsmath::hypot` for why this
        // is neither `sqrt(dx*dx + dz*dz)` nor `f64::hypot`.
        let len = jsmath::hypot2(dx, dz);
        let ry = dx.atan2(dz) - std::f64::consts::FRAC_PI_2;
        let n = jsmath::round(len / 4.0) as i64;
        for i in 0..n {
            let t = (i as f64 + 0.5) / n as f64;
            let px = x0 + dx * t;
            let pz = z0 + dz * t;
            let h = rng.range(3.0, 3.8);
            let key = *rng.pick(&WALL_KEYS);
            let bx = box_kit(asm);
            let m = ll(&Mat4::IDENTITY, px as f32, (h / 2.0) as f32, pz as f32, ry as f32, (len / n as f64 + 0.05) as f32, h as f32, 0.4, 0.0, 0.0);
            asm.add(key, &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.7, 0.4]), paint: None }));
            let soft = box_soft_kit(asm);
            let m = ll(&Mat4::IDENTITY, px as f32, (h + 0.06) as f32, pz as f32, ry as f32, (len / n as f64 + 0.14) as f32, 0.12, 0.54, 0.0, 0.0);
            asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.4, 0.15]), paint: None }));
            asm.collide_box(Surface::Concrete, px as f32, (h / 2.0) as f32, pz as f32, (len / n as f64 + 0.05) as f32, h as f32, 0.45, ry as f32);
        }
    }

    // Blocked cross-streets: rubble barricades and stacked barriers rather
    // than an invisible wall, so the boundary is diegetic.
    let blocks: [[f64; 2]; 2] = [[0.0, STREET.z_max + 1.5], [0.0, STREET.z_min - 1.5]];
    for [bx, bz] in blocks {
        for i in -1..=1 {
            let jr = 0.02 + rng.range(-0.05, 0.05);
            asm.put("jersey", (bx + f64::from(i) * 2.1) as f32, 0.02, bz as f32, jr as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
            asm.collide_box(Surface::Concrete, (bx + f64::from(i) * 2.1) as f32, 0.46, bz as f32, 0.62, 0.92, 1.9, 0.0);
        }
        rubble_mound(asm, rng, (bx - 3.4) as f32, 0.0, bz as f32, 2.2, 30, RubbleOpts { key: "concrete" });
        rubble_mound(asm, rng, (bx + 3.6) as f32, 0.0, bz as f32, 2.0, 26, RubbleOpts { key: "concrete" });
        asm.collide_box(Surface::Concrete, bx as f32, 1.4, (bz + if bz > 0.0 { 1.4 } else { -1.4 }) as f32, 16.0, 2.8, 1.2, 0.0);
        for _ in 0..14 {
            let px = bx + rng.range(-7.0, 7.0);
            let pz = bz + rng.range(-2.0, 2.0);
            let id = *rng.pick(&BARRICADE_LITTER);
            let py = ground_y(px, pz) + 0.03;
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.7, 1.3);
            asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
        }
    }
}
