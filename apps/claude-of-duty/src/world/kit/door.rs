//! Ported from Claude-of-Duty `src/world/kit.js:500-564` — `doorUnit` and
//! its `doorLeaf` sub-part builder.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::geo::WorldGeo;

use super::{box_kit, box_soft_kit, chamfer_box, ll, merge_simple, WallHole};

/// `doorUnit(A, pm, o, rng, opts = {})`'s `opts` (`kit.js:501-502,524-526`).
/// Defaults: `t=0.34`, `frame_key="wood_dark"`, `leaf=true`,
/// `leaf_key="metal_green"`, `open=0.0` (the swing angle, radians).
pub struct DoorOpts<'a> {
    pub t: f32,
    pub frame_key: &'a str,
    pub leaf: bool,
    pub leaf_key: &'a str,
    pub open: f32,
}

/// `doorUnit(A, pm, o, rng, opts = {})` (`kit.js:501-543`): jamb + head
/// casing standing slightly proud of the wall, a threshold, and (unless
/// `opts.leaf === false`) a hinged leaf swung to `opts.open` radians. Draws
/// nothing from `rng` — the source's `rng` parameter is accepted but never
/// read in this function's body.
pub fn door_unit<'a>(asm: &mut Assembler, pm: &Mat4, o: &'a WallHole, _rng: &mut Rng, opts: DoorOpts) -> &'a WallHole {
    let (w, h, x, y) = (o.w, o.h, o.x, o.y);
    let box_ = box_kit(asm);

    let m = ll(pm, x - w / 2.0 - 0.03, y, 0.0, 0.0, 0.09, h + 0.1, opts.t * 0.9, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.4, 0.2]), paint: None }));
    let m = ll(pm, x + w / 2.0 + 0.03, y, 0.0, 0.0, 0.09, h + 0.1, opts.t * 0.9, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.4, 0.2]), paint: None }));
    let m = ll(pm, x, y + h / 2.0 + 0.06, 0.0, 0.0, w + 0.2, 0.11, opts.t * 0.9, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.45, 0.25]), paint: None }));

    let soft = box_soft_kit(asm);
    let m = ll(pm, x, y - h / 2.0 + 0.03, opts.t * 0.5 - 0.02, 0.0, w + 0.1, 0.06, opts.t, 0.0, 0.0);
    asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.5, 0.3]), paint: None }));

    if opts.leaf {
        let ang = opts.open;
        let leaf_w = w - 0.06;
        let leaf = asm.cache(&format!("doorleaf:{leaf_w:.2}:{h:.2}"), || door_leaf(leaf_w, h - 0.06));
        // Hinge at the left jamb: rotate about the hinge, not the centre.
        let hx = x - leaf_w / 2.0;
        let (sn, cs) = ang.sin_cos();
        let m = ll(pm, hx + (leaf_w / 2.0) * cs, y, opts.t * 0.45 + (leaf_w / 2.0) * sn, -ang, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add(opts.leaf_key, &leaf, Some(&m), Some(AccumAddOpts { masks: Some([0.95, 0.5, 0.1]), paint: None }));
    }

    o
}

/// `doorLeaf(w, h)` (`kit.js:545-564`): recessed panels framed by rails, plus
/// a handle. Unlike `sashLeaf`/`shutterLeaf` (which scale a shared unit
/// `plainBox()`), each part here is its own `chamferBox(sx, sy, sz, 0.006)`
/// at real dimensions — a heavier, chamfered surface fitting for the one
/// door leaf in view rather than a thin repeated member.
fn door_leaf(w: f32, h: f32) -> WorldGeo {
    let mut parts = Vec::new();
    let mut add = |sx: f32, sy: f32, sz: f32, x: f32, y: f32, z: f32| {
        let mut g = chamfer_box(sx, sy, sz, 0.006);
        g.translate(x, y, z);
        parts.push(g);
    };
    add(w, h, 0.05, 0.0, 0.0, 0.0);
    add(w, 0.1, 0.062, 0.0, h / 2.0 - 0.07, 0.0);
    add(w, 0.1, 0.062, 0.0, -h / 2.0 + 0.07, 0.0);
    add(w, 0.12, 0.062, 0.0, h * 0.06, 0.0);
    add(0.09, h, 0.062, -w / 2.0 + 0.05, 0.0, 0.0);
    add(0.09, h, 0.062, w / 2.0 - 0.05, 0.0, 0.0);
    // Handle.
    add(0.035, 0.13, 0.035, w / 2.0 - 0.16, -0.02, 0.05);
    merge_simple(&parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn door_unit_with_leaf_produces_frame_threshold_and_leaf_batches() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let o = WallHole { x: 0.0, y: 1.05, w: 1.0, h: 2.1, arch: 0.0, ragged: 0.0 };
        door_unit(
            &mut asm,
            &Mat4::IDENTITY,
            &o,
            &mut rng,
            DoorOpts { t: 0.34, frame_key: "wood_dark", leaf: true, leaf_key: "metal_green", open: 0.0 },
        );
        let result = asm.finalize();
        let keys: Vec<&str> = result.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"wood_dark"));
        assert!(keys.contains(&"concrete"));
        assert!(keys.contains(&"metal_green"));
    }

    #[test]
    fn door_unit_without_leaf_never_emits_the_leaf_key() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let o = WallHole { x: 0.0, y: 1.05, w: 1.0, h: 2.1, arch: 0.0, ragged: 0.0 };
        door_unit(
            &mut asm,
            &Mat4::IDENTITY,
            &o,
            &mut rng,
            DoorOpts { t: 0.34, frame_key: "wood_dark", leaf: false, leaf_key: "metal_green", open: 0.0 },
        );
        let result = asm.finalize();
        let keys: Vec<&str> = result.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(!keys.contains(&"metal_green"));
    }

    #[test]
    fn door_unit_never_draws_from_rng() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng_a = Rng::new(5);
        let rng_b = Rng::new(5);
        let o = WallHole { x: 0.0, y: 1.05, w: 1.0, h: 2.1, arch: 0.0, ragged: 0.0 };
        door_unit(&mut asm, &Mat4::IDENTITY, &o, &mut rng_a, DoorOpts { t: 0.34, frame_key: "wood_dark", leaf: true, leaf_key: "metal_green", open: 0.4 });
        // If door_unit drew from rng, rng_a would now disagree with the
        // untouched rng_b (same seed, never passed to door_unit).
        assert_eq!(rng_a.state(), rng_b.state());
    }

    #[test]
    fn door_leaf_produces_non_empty_geometry() {
        let g = door_leaf(0.9, 2.0);
        assert!(g.vert_count() > 0);
        assert!(g.tri_count() > 0);
    }
}
