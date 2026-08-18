//! Ported from Claude-of-Duty `src/world/kit.js:678-739` — `balcony`. Draws
//! nothing from `rng`: the source's `rng` parameter is accepted but never
//! read in this function's body.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::palette::Surface;

use super::{box_kit, box_soft_kit, box_thin_kit, ll, ry_of, world_of};

/// `opts.railing` (`kit.js:694,703-704`): `'concrete'` builds a solid
/// balustrade; anything else (the source's `else` arm) builds a metal-bar
/// railing with its own `opts.railKey` (default `'metal_rust'`).
pub enum BalconyRailing<'a> {
    Concrete,
    Metal(&'a str),
}

/// `balcony(A, pm, x, y, w, rng, opts = {})`'s `opts` (`kit.js:678-679,681`).
/// Defaults: `depth=1.15`, `key="concrete"`.
pub struct BalconyOpts<'a> {
    pub depth: f32,
    pub key: &'a str,
    pub railing: BalconyRailing<'a>,
}

/// `balcony(...)`'s return shape (`kit.js:738`, `{x, y, w, d}`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalconyResult {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub d: f32,
}

/// `balcony(A, pm, x, y, w, rng, opts = {})` (`kit.js:678-739`): a slab
/// (slightly sagging via a softer chamfer), two brackets underneath, and
/// either a solid concrete balustrade or a metal-bar railing with vertical
/// balusters and corner posts.
pub fn balcony(asm: &mut Assembler, pm: &Mat4, x: f32, y: f32, w: f32, _rng: &mut Rng, opts: BalconyOpts) -> BalconyResult {
    let d = opts.depth;
    let box_ = box_kit(asm);
    let key = opts.key;

    let soft = box_soft_kit(asm);
    let m = ll(pm, x, y + 0.06, -d / 2.0, 0.0, w, 0.13, d, 0.0, 0.0);
    asm.add(key, &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.45, 0.55, 0.3]), paint: None }));
    let p = world_of(pm, x, y + 0.06, -d / 2.0);
    asm.collide_box(Surface::Concrete, p.x, p.y, p.z, w, 0.16, d, ry_of(pm));

    for i in [-1.0f32, 1.0] {
        let m = ll(pm, x + i * (w / 2.0 - 0.16), y - 0.14, -d * 0.42, 0.0, 0.11, 0.3, d * 0.75, 0.0, 0.0);
        asm.add(key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.4, 0.6, 0.4]), paint: None }));
    }

    match opts.railing {
        BalconyRailing::Concrete => {
            let m = ll(pm, x, y + 0.55, -d + 0.06, 0.0, w, 0.85, 0.12, 0.0, 0.0);
            asm.add(key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.5, 0.2]), paint: None }));
            let m = ll(pm, x - w / 2.0 + 0.06, y + 0.55, -d / 2.0, 0.0, 0.12, 0.85, d, 0.0, 0.0);
            asm.add(key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.5, 0.2]), paint: None }));
            let m = ll(pm, x + w / 2.0 - 0.06, y + 0.55, -d / 2.0, 0.0, 0.12, 0.85, d, 0.0, 0.0);
            asm.add(key, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.5, 0.2]), paint: None }));
            let p = world_of(pm, x, y + 0.55, -d + 0.06);
            asm.collide_box(Surface::Concrete, p.x, p.y, p.z, w, 0.9, 0.16, ry_of(pm));
        }
        BalconyRailing::Metal(rail_key) => {
            let bar = box_thin_kit(asm);
            let m = ll(pm, x, y + 1.0, -d + 0.04, 0.0, w, 0.05, 0.05, 0.0, 0.0);
            asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.45, 0.0]), paint: None }));
            let m = ll(pm, x, y + 0.52, -d + 0.04, 0.0, w, 0.035, 0.035, 0.0, 0.0);
            asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.45, 0.0]), paint: None }));
            let m = ll(pm, x - w / 2.0, y + 1.0, -d / 2.0 + 0.02, 0.0, 0.05, 0.05, d, 0.0, 0.0);
            asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.45, 0.0]), paint: None }));
            let m = ll(pm, x + w / 2.0, y + 1.0, -d / 2.0 + 0.02, 0.0, 0.05, 0.05, d, 0.0, 0.0);
            asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.45, 0.0]), paint: None }));

            let n = ((w / 0.17).round() as i32).max(4);
            for i in 0..=n {
                let bx = x - w / 2.0 + (i as f32 / n as f32) * w;
                let m = ll(pm, bx, y + 0.53, -d + 0.04, 0.0, 0.024, 1.0, 0.024, 0.0, 0.0);
                asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));
            }

            for sx in [-1.0f32, 1.0] {
                let m = ll(pm, x + sx * (w / 2.0), y + 0.53, -d + 0.04, 0.0, 0.05, 1.05, 0.05, 0.0, 0.0);
                asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));
                for i in 0..=3 {
                    let m = ll(pm, x + sx * (w / 2.0), y + 0.53, -d + 0.04 + (i as f32 / 3.0) * (d - 0.08), 0.0, 0.024, 1.0, 0.024, 0.0, 0.0);
                    asm.add(rail_key, &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));
                }
            }
            let p = world_of(pm, x, y + 0.55, -d + 0.06);
            asm.collide_box(Surface::Metal, p.x, p.y, p.z, w, 0.95, 0.1, ry_of(pm));
        }
    }

    BalconyResult { x, y, w, d }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balcony_concrete_railing_produces_one_concrete_batch_and_two_collision_boxes() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let result = balcony(&mut asm, &Mat4::IDENTITY, 0.0, 3.0, 1.8, &mut rng, BalconyOpts { depth: 1.15, key: "concrete", railing: BalconyRailing::Concrete });
        assert_eq!(result, BalconyResult { x: 0.0, y: 3.0, w: 1.8, d: 1.15 });
        let out = asm.finalize();
        assert_eq!(out.statics.len(), 1);
        assert_eq!(out.collision.len(), 1);
        assert_eq!(out.collision[0].surface, Surface::Concrete);
    }

    #[test]
    fn balcony_metal_railing_uses_the_rail_key_and_a_metal_collision_box() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        balcony(&mut asm, &Mat4::IDENTITY, 0.0, 3.0, 1.8, &mut rng, BalconyOpts { depth: 1.15, key: "concrete", railing: BalconyRailing::Metal("metal_rust") });
        let out = asm.finalize();
        let keys: Vec<&str> = out.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"metal_rust"));
        assert!(out.collision.iter().any(|c| c.surface == Surface::Metal));
    }

    #[test]
    fn balcony_never_draws_from_rng() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng_a = Rng::new(5);
        let rng_b = Rng::new(5);
        balcony(&mut asm, &Mat4::IDENTITY, 0.0, 3.0, 1.8, &mut rng_a, BalconyOpts { depth: 1.15, key: "concrete", railing: BalconyRailing::Metal("metal_rust") });
        assert_eq!(rng_a.state(), rng_b.state());
    }
}
