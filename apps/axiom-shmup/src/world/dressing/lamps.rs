//! Ported from Claude-of-Duty `src/world/dressing.js:1124-1163` —
//! `streetLamps`: the column, the arm and its lens, the anchor a later pass
//! hangs a point light off, and the litter that collects round the base.

use axiom_math::Vec3;

use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::layout::SET_PIECES;
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::{ground_skirt, ground_y, SkirtOpts};

const LITTER: [&str; 4] = ["litter", "brick_b", "can", "weeds"];

/// `streetLamps(A, rng)` (`dressing.js:1125-1163`).
///
/// **`SET_PIECES.lamps` carries a third element (`ry`) that this pass throws
/// away.** The source destructures only `[x, z]` and derives the yaw from
/// the sign of `x` instead — "the arm must reach across the street, so face
/// it inward" (`dressing.js:1128`). Ported as written; the authored `ry`
/// column in the layout data is dead here.
pub fn street_lamps(asm: &mut Assembler, rng: &mut Rng) {
    for [x, z, _authored_ry] in SET_PIECES.lamps.iter().copied() {
        let y = ground_y(x, z);
        // the arm must reach across the street, so face it inward
        let ry: f64 = if x < 0.0 { 0.0 } else { std::f64::consts::PI };
        let grime = rng.range(0.9, 1.2);
        asm.put("lamp_post", x as f32, y as f32, z as f32, ry as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        let arm_x = x + ry.cos() * 0.88;
        let arm_z = z - ry.sin() * 0.88;
        asm.put("lamp_glass", arm_x as f32, (y + 5.33) as f32, arm_z as f32, ry as f32, 1.0, None, 0.0, -0.16);
        asm.collide_box(Surface::Metal, x as f32, (y + 2.7) as f32, z as f32, 0.3, 5.4, 0.3, 0.0);
        // the column stands in a broken square of concrete, not on a clean line
        let pebbles = rng.int(3, 6);
        ground_skirt(asm, rng, x, y, z, 0.34, SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() });
        asm.lamp_anchors.push(Vec3::new(arm_x as f32, (y + 5.3) as f32, arm_z as f32));
        // a hanging sign or a bundle of cable ties at head height
        if rng.float() < 0.5 {
            asm.put(
                "sign_hang",
                (x + ry.cos() * 0.2) as f32,
                (y + 3.4) as f32,
                (z - ry.sin() * 0.2) as f32,
                (ry + std::f64::consts::FRAC_PI_2) as f32,
                1.0,
                Some([1.0, 1.2, 1.0]),
                0.0,
                0.0,
            );
        }
        let mut i = 0;
        while int_loop_continues(rng, i, 2, 5) {
            let a = rng.float() * 6.28;
            let r = rng.range(0.35, 1.1);
            let px = x + a.cos() * r;
            let pz = z + a.sin() * r;
            let id = *rng.pick(&LITTER);
            let py = ground_y(px, pz) + 0.02;
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.7, 1.2);
            asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
            i += 1;
        }
    }
}
