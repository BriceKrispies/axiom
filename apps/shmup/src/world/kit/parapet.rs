//! Ported from Claude-of-Duty `src/world/kit.js:742-774` — `parapet`, a roof
//! edge wall with a coping course.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::palette::Surface;

use super::{box_kit, box_soft_kit, ll};

/// `parapet(A, key, cx, cz, w, d, y, rng, opts = {})`'s `opts`
/// (`kit.js:743-745`). Defaults: `h=0.72`, `t=0.24`, `coping_key="concrete"`.
pub struct ParapetOpts<'a> {
    pub h: f32,
    pub t: f32,
    pub coping_key: &'a str,
}

/// `parapet(A, key, cx, cz, w, d, y, rng, opts = {})` (`kit.js:743-774`): the
/// four sides of a roof-edge wall (each independently jittered in height by
/// `rng.range(-0.05, 0.05)`), each capped with a slightly wider, weathered
/// coping course. Every side is authored in LEVEL space directly — the
/// source's `pmI` is reset to identity every iteration (`kit.js:757`), so
/// [`axiom_math::Mat4::IDENTITY`] stands in for it here. Returns `y + h`,
/// the top of the wall — the source's `return y + h`.
pub fn parapet(asm: &mut Assembler, key: &str, cx: f32, cz: f32, w: f32, d: f32, y: f32, rng: &mut Rng, opts: ParapetOpts) -> f32 {
    let h = opts.h;
    let t = opts.t;
    let box_ = box_kit(asm);
    let sides = [
        (cx, cz - d / 2.0 + t / 2.0, w, t),
        (cx, cz + d / 2.0 - t / 2.0, w, t),
        (cx - w / 2.0 + t / 2.0, cz, t, d),
        (cx + w / 2.0 - t / 2.0, cz, t, d),
    ];
    for (sx, sz, sw, sd) in sides {
        let jitter = rng.range(-0.05, 0.05) as f32;
        let m = ll(&Mat4::IDENTITY, sx, y + (h + jitter) / 2.0, sz, 0.0, sw, h + jitter, sd, 0.0, 0.0);
        asm.add(key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.4, 0.15]), paint: None }));
        let soft = box_soft_kit(asm);
        let m = ll(&Mat4::IDENTITY, sx, y + h + jitter + 0.045, sz, 0.0, sw + 0.09, 0.09, sd + 0.09, 0.0, 0.0);
        asm.add(opts.coping_key, &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.75, 0.3, 0.1]), paint: None }));
        asm.collide_box(Surface::Concrete, sx, y + (h + 0.1) / 2.0, sz, sw, h + 0.1, sd, 0.0);
    }
    y + h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parapet_returns_the_wall_top_and_emits_four_sides() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let top = parapet(&mut asm, "roof_screed", 0.0, 0.0, 6.0, 4.0, 8.0, &mut rng, ParapetOpts { h: 0.72, t: 0.24, coping_key: "concrete" });
        // `y + h` exactly — the per-side jitter only perturbs each side's own
        // wall/coping height, never the returned wall-top value.
        assert_eq!(top, 8.72);
        let out = asm.finalize();
        assert_eq!(out.collision[0].geo.tri_count(), 4 * 12);
    }

    #[test]
    fn parapet_jitters_each_side_deterministically_from_the_same_seed() {
        let mut asm_a = Assembler::new(Rng::new(1));
        let mut rng_a = Rng::new(9);
        parapet(&mut asm_a, "roof_screed", 0.0, 0.0, 6.0, 4.0, 8.0, &mut rng_a, ParapetOpts { h: 0.72, t: 0.24, coping_key: "concrete" });
        let mut asm_b = Assembler::new(Rng::new(1));
        let mut rng_b = Rng::new(9);
        parapet(&mut asm_b, "roof_screed", 0.0, 0.0, 6.0, 4.0, 8.0, &mut rng_b, ParapetOpts { h: 0.72, t: 0.24, coping_key: "concrete" });
        let a = asm_a.finalize();
        let b = asm_b.finalize();
        assert_eq!(a.statics[0].geo.pos, b.statics[0].geo.pos);
    }
}
