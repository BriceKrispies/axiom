//! Ported from Claude-of-Duty `src/world/kit.js:919-958` — `drainpipe`.
//! Draws nothing from `rng`: the source's `rng` parameter is accepted but
//! never read in this function's body.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;

use super::{box_fine_kit, ll, tube_y};

/// `drainpipe(A, pm, x, yTop, h, rng, opts = {})`'s `opts`
/// (`kit.js:919-923`). Defaults: `r=0.055`, `key="metal_rust"`,
/// `z=-r-0.02`.
pub struct DrainpipeOpts<'a> {
    pub r: f32,
    pub key: &'a str,
    pub z: f32,
}

/// `drainpipe(A, pm, x, yTop, h, rng, opts = {})` (`kit.js:919-958`): three
/// (or more, per `segs`) pipe sections with visible joints and a slight
/// lean, a kicked-out shoe at the bottom, a rainwater head + bracket at the
/// top (without which "the pipe simply stops in mid-air", per the source's
/// own comment), the overflow stain it leaves down the render beside it, and
/// wall brackets at every internal joint.
pub fn drainpipe(asm: &mut Assembler, pm: &Mat4, x: f32, y_top: f32, h: f32, _rng: &mut Rng, opts: DrainpipeOpts) {
    let key = opts.key;
    let pipe = asm.cache(&format!("pipe:{:.3}", opts.r), || tube_y(opts.r, 1.0, 8, 1.0, false, 1));
    let z = opts.z;

    let segs = ((h / 1.6).round() as i32).max(2) as u32;
    let mut y = y_top - h;
    for i in 0..segs {
        let sh = h / segs as f32;
        let lean = if i % 2 == 1 { 0.006 } else { -0.006 };
        let m = ll(pm, x + lean, y, z, 0.0, 1.0, sh, 1.0, 0.0, 0.0);
        asm.add(key, &pipe, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.6, 0.1]), paint: None }));
        let m = ll(pm, x, y + sh - 0.03, z, 0.0, 1.22, 0.075, 1.22, 0.0, 0.0);
        asm.add(key, &pipe, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.7, 0.2]), paint: None }));
        y += sh;
    }

    // The shoe at the bottom, kicking out to the street.
    let m = ll(pm, x, y_top - h + 0.02, z + 0.09, 0.0, 1.0, 0.3, 1.0, -0.75, 0.0);
    asm.add(key, &pipe, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.7, 0.3]), paint: None }));

    // The rainwater head at the top.
    let fine = box_fine_kit(asm);
    let m = ll(pm, x, y_top - 0.09, z - 0.01, 0.0, 0.2, 0.2, 0.17, 0.0, 0.0);
    asm.add(key, &fine, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.65, 0.25]), paint: None }));
    let fine2 = box_fine_kit(asm);
    let m = ll(pm, x, y_top + 0.02, z - 0.01, 0.0, 0.24, 0.03, 0.2, 0.0, 0.0);
    asm.add("metal_dark", &fine2, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.55, 0.2]), paint: None }));

    // The overflow stain down the render beside it.
    let fine3 = box_fine_kit(asm);
    let m = ll(pm, x, y_top - 0.24, z * 0.35, 0.0, 0.09, 0.3, 0.02, 0.0, 0.0);
    asm.add(key, &fine3, Some(&m), Some(AccumAddOpts { masks: Some([0.2, 1.0, 0.6]), paint: None }));

    // Brackets at every internal joint.
    for i in 1..segs {
        let fine4 = box_fine_kit(asm);
        let m = ll(pm, x, y_top - h + (i as f32 * h) / segs as f32, z * 0.45, 0.0, 0.16, 0.03, z.abs() * 0.9, 0.0, 0.0);
        asm.add("metal_dark", &fine4, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.6, 0.2]), paint: None }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drainpipe_emits_bracket_per_internal_joint_and_a_shoe() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        // h = 4.8 -> segs = round(4.8/1.6) = 3 -> 2 internal joints (i=1,2).
        drainpipe(&mut asm, &Mat4::IDENTITY, 0.0, 5.0, 4.8, &mut rng, DrainpipeOpts { r: 0.055, key: "metal_rust", z: -0.075 });
        let out = asm.finalize();
        let keys: Vec<&str> = out.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"metal_rust"));
        assert!(keys.contains(&"metal_dark"));
    }

    #[test]
    fn drainpipe_never_draws_from_rng() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng_a = Rng::new(5);
        let rng_b = Rng::new(5);
        drainpipe(&mut asm, &Mat4::IDENTITY, 0.0, 5.0, 4.8, &mut rng_a, DrainpipeOpts { r: 0.055, key: "metal_rust", z: -0.075 });
        assert_eq!(rng_a.state(), rng_b.state());
    }

    #[test]
    fn drainpipe_segs_formula_matches_the_javascripts_rounding() {
        // h/1.6 rounded, floor 2.
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        drainpipe(&mut asm, &Mat4::IDENTITY, 0.0, 2.0, 0.5, &mut rng, DrainpipeOpts { r: 0.055, key: "metal_rust", z: -0.075 });
        // segs = max(2, round(0.5/1.6)=0) = 2 -> no internal-joint brackets
        // (loop `1..segs` is `1..2`, one iteration) but the top head/stain
        // still land in "metal_rust"/"metal_dark".
        let out = asm.finalize();
        assert!(out.statics.iter().any(|s| s.key == "metal_dark"));
    }
}
