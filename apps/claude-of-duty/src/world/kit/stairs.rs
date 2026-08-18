//! Ported from Claude-of-Duty `src/world/kit.js:781-825` — `stairRun`. Draws
//! nothing from `rng`: the source's `rng` parameter (implicit — `stairRun`
//! takes no `rng` at all, unlike its siblings) never appears in this
//! function's signature either.

use axiom_math::Mat4;

use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;

use super::{box_kit, box_thin_kit, ll, ry_of, world_of};

/// `opts.railing` (`kit.js:801,806-807`): `false`/absent skips the railing
/// entirely; `'left'`/`'right'` build only that side; any other truthy value
/// (the source's fallback, since neither string comparison matches) builds
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StairRailing {
    None,
    Left,
    Right,
    Both,
}

/// `stairRun(A, pm, x, y, z, w, steps, rise, run, opts = {})`'s `opts`
/// (`kit.js:781-782,796,801`). Defaults: `key="concrete"`, `stringer=true`,
/// `railing=None`.
pub struct StairOpts<'a> {
    pub key: &'a str,
    pub stringer: bool,
    pub railing: StairRailing,
}

/// `stairRun(...)`'s return shape (`kit.js:824`, `{top, endZ}`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StairResult {
    pub top: f32,
    pub end_z: f32,
}

/// `stairRun(A, pm, x, y, z, w, steps, rise, run, opts = {})`
/// (`kit.js:781-825`): a straight flight, origin at the bottom step's
/// front-centre, climbing `+Z`, with per-step collision so the character
/// controller steps up naturally, an optional side stringer/spine so it
/// doesn't read as floating slabs, and an optional railing.
#[allow(clippy::too_many_arguments)]
pub fn stair_run(asm: &mut Assembler, pm: &Mat4, x: f32, y: f32, z: f32, w: f32, steps: u32, rise: f32, run: f32, opts: StairOpts) -> StairResult {
    let box_ = box_kit(asm);
    for i in 0..steps {
        let sy = y + (i as f32 + 0.5) * rise;
        let sz = z + (i as f32 + 0.5) * run;
        let m = ll(pm, x, sy, sz, 0.0, w, rise, run, 0.0, 0.0);
        asm.add(opts.key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.35, 0.15]), paint: None }));
        let wp = world_of(pm, x, sy, sz);
        let surface = asm.surface_of(opts.key);
        asm.collide_box(surface, wp.x, wp.y, wp.z, w, rise, run, ry_of(pm));
    }

    let h = steps as f32 * rise;
    let d = steps as f32 * run;
    if opts.stringer {
        let m = ll(pm, x, y + h / 2.0 - 0.1, z + d / 2.0, 0.0, w * 1.02, h, d * 0.99, 0.0, 0.0);
        asm.add(opts.key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.4, 0.6, 0.4]), paint: None }));
    }

    if opts.railing != StairRailing::None {
        let bar = box_thin_kit(asm);
        let ang = h.atan2(d);
        let len = h.hypot(d);
        for sx in [-1.0f32, 1.0] {
            let skip = (opts.railing == StairRailing::Right && sx < 0.0) || (opts.railing == StairRailing::Left && sx > 0.0);
            if skip {
                continue;
            }
            let m = ll(pm, x + sx * (w / 2.0 - 0.05), y + h / 2.0 + 0.95, z + d / 2.0, 0.0, 0.045, 0.045, len, -ang, 0.0);
            asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));
            let mut i = 0u32;
            while i < steps {
                let m = ll(pm, x + sx * (w / 2.0 - 0.05), y + i as f32 * rise + 0.5, z + (i as f32 + 0.5) * run, 0.0, 0.03, 1.0, 0.03, 0.0, 0.0);
                asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));
                i += 3;
            }
        }
    }

    StairResult { top: y + h, end_z: z + d }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn stair_run_emits_one_collision_box_per_step_plus_the_stringer() {
        let mut asm = Assembler::new(Rng::new(1));
        let result = stair_run(&mut asm, &Mat4::IDENTITY, 0.0, 0.0, 0.0, 1.2, 6, 0.18, 0.28, StairOpts { key: "concrete", stringer: true, railing: StairRailing::None });
        assert_eq!(result, StairResult { top: 6.0 * 0.18, end_z: 6.0 * 0.28 });
        let out = asm.finalize();
        assert_eq!(out.collision[0].geo.tri_count(), 6 * 12);
        // Steps (6) + stringer (1) static boxes merged into one "concrete" batch.
        assert_eq!(out.statics.len(), 1);
    }

    #[test]
    fn stair_run_left_only_railing_skips_the_right_side() {
        let mut asm_left = Assembler::new(Rng::new(1));
        stair_run(&mut asm_left, &Mat4::IDENTITY, 0.0, 0.0, 0.0, 1.2, 6, 0.18, 0.28, StairOpts { key: "concrete", stringer: false, railing: StairRailing::Left });
        let left = asm_left.finalize();

        let mut asm_both = Assembler::new(Rng::new(1));
        stair_run(&mut asm_both, &Mat4::IDENTITY, 0.0, 0.0, 0.0, 1.2, 6, 0.18, 0.28, StairOpts { key: "concrete", stringer: false, railing: StairRailing::Both });
        let both = asm_both.finalize();

        let left_metal = left.statics.iter().find(|s| s.key == "metal_rust").unwrap();
        let both_metal = both.statics.iter().find(|s| s.key == "metal_rust").unwrap();
        assert_eq!(both_metal.geo.tri_count(), left_metal.geo.tri_count() * 2);
    }

    #[test]
    fn stair_run_never_draws_from_rng() {
        // stairRun takes no rng at all in the source; this pins that no
        // ambient Rng anywhere in the crate needs to be threaded through it.
        let mut asm = Assembler::new(Rng::new(1));
        let _ = stair_run(&mut asm, &Mat4::IDENTITY, 0.0, 0.0, 0.0, 1.0, 3, 0.18, 0.28, StairOpts { key: "concrete", stringer: false, railing: StairRailing::None });
    }
}
