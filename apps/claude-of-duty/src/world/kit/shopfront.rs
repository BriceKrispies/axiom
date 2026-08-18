//! Ported from Claude-of-Duty `src/world/kit.js:566-675` — `shopfront` (a
//! wide ground-floor opening with a roller shutter part-way down) and its
//! `rollerShutter` sub-part builder.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::geo::WorldGeo;
use crate::world::palette::Surface;

use super::{box_fine_kit, box_kit, box_thin_kit, cloth_geometry, ll, merge_simple, plane_geometry, ClothOpts, WallHole};

/// `shopfront(A, pm, o, rng, opts = {})`'s `opts` (`kit.js:568-569,580,594,612`).
/// Defaults: `t=0.34`, `drop=None` (drawn from `rng.range(0.15, 0.85)` when
/// absent), `counter=true`, `inside=true`.
pub struct ShopfrontOpts {
    pub t: f32,
    pub drop: Option<f32>,
    pub counter: bool,
    pub inside: bool,
}

/// `shopfront(A, pm, o, rng, opts = {})` (`kit.js:568-658`): a lintel beam, a
/// shutter housing, a part-lowered roller shutter (drawn stochastically
/// unless `opts.drop` pins it), a stall counter in the opening, and interior
/// dressing (wall shelving, conduit, an occasional bolt of cloth) beside the
/// opening — the biggest surface in an interior shot, per the source's own
/// comment (`kit.js:607-611`), so bare render there is the fastest way to
/// make an interior look like an empty box.
///
/// The source's `rng?.range ? ... : 0` / `if (rng && ...)` guards
/// (`kit.js:617`, `638`) are defensive against a missing `rng` that no real
/// call site ever passes; `rng` is a plain `&mut Rng` here, so those arms
/// always take their "rng present" branch.
pub fn shopfront<'a>(asm: &mut Assembler, pm: &Mat4, o: &'a WallHole, rng: &mut Rng, opts: ShopfrontOpts) -> &'a WallHole {
    let (x, y, w, h) = (o.x, o.y, o.w, o.h);
    let box_ = box_kit(asm);

    let m = ll(pm, x, y + h / 2.0 + 0.11, opts.t * 0.5, 0.0, w + 0.5, 0.22, opts.t, 0.0, 0.0);
    asm.add("concrete", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.4, 0.55, 0.35]), paint: None }));
    let m = ll(pm, x, y + h / 2.0 - 0.09, 0.06, 0.0, w + 0.12, 0.18, 0.16, 0.0, 0.0);
    asm.add("metal_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.5, 0.1]), paint: None }));

    let drop = opts.drop.unwrap_or_else(|| rng.range(0.15, 0.85) as f32);
    if drop > 0.02 {
        let sh = h * drop;
        let shutter = asm.cache(&format!("roller:{w:.2}"), || roller_shutter(w, 1.0));
        let m = ll(pm, x, y + h / 2.0 - 0.18 - sh / 2.0, 0.05, 0.0, 1.0, sh, 1.0, 0.0, 0.0);
        asm.add("corrugated", &shutter, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.6, 0.15]), paint: None }));
        asm.slab_box(Surface::Metal, pm, x, y + h / 2.0 - 0.18 - sh / 2.0, w, sh, 0.12);
    }

    if opts.counter {
        let m = ll(pm, x, 0.42, opts.t + 0.28, 0.0, w * 0.82, 0.08, 0.7, 0.0, 0.0);
        asm.add("wood_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.5, 0.2]), paint: None }));
        let m = ll(pm, x - w * 0.34, 0.21, opts.t + 0.28, 0.0, 0.09, 0.42, 0.62, 0.0, 0.0);
        asm.add("wood_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.6, 0.3]), paint: None }));
        let m = ll(pm, x + w * 0.34, 0.21, opts.t + 0.28, 0.0, 0.09, 0.42, 0.62, 0.0, 0.0);
        asm.add("wood_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.6, 0.3]), paint: None }));
        asm.slab_box(Surface::Wood, pm, x, 0.25, w * 0.82, 0.5, opts.t + 0.6);
    }

    if opts.inside {
        let thin = box_thin_kit(asm);
        for sx in [-1.0f32, 1.0] {
            let bx = x + sx * (w / 2.0 + 0.75);
            let sy = 1.35 + rng.range(-0.1, 0.25) as f32;
            let fine = box_fine_kit(asm);
            let m = ll(pm, bx, sy, opts.t + 0.17, 0.0, 1.3, 0.045, 0.34, 0.0, 0.0);
            asm.add("wood_prop", &fine, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.4, 0.15]), paint: None }));
            for b in [-1.0f32, 1.0] {
                let m = ll(pm, bx + b * 0.5, sy - 0.12, opts.t + 0.09, 0.0, 0.03, 0.24, 0.18, 0.0, 0.0);
                asm.add("metal_rust", &thin, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.6, 0.2]), paint: None }));
            }
            let m = ll(pm, bx + 0.62, 1.5, opts.t + 0.03, 0.0, 0.045, 2.6, 0.045, 0.0, 0.0);
            asm.add("metal_dark", &thin, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.5, 0.2]), paint: None }));
            let fine2 = box_fine_kit(asm);
            let m = ll(pm, bx + 0.62, 1.42, opts.t + 0.06, 0.0, 0.16, 0.22, 0.09, 0.0, 0.0);
            asm.add("metal_dark", &fine2, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.5, 0.2]), paint: None }));
        }
        if rng.float() < 0.7 {
            let cw = rng.range(0.9, 1.5) as f32;
            let ch = rng.range(1.1, 1.7) as f32;
            let c = cloth_geometry(
                cw,
                ch,
                ClothOpts { seg_x: 7, seg_y: 8, sag: 0.05, wrinkle: 0.055, twist: 0.06, thickness: 0.003, fray: 0.02, ..ClothOpts::default() },
                Some(rng),
            );
            let key = *rng.pick(&["fabric_red", "fabric_teal", "fabric_cream"]);
            let side: f32 = if rng.float() < 0.5 { -1.0 } else { 1.0 };
            let dx = rng.range(0.9, 1.5) as f32;
            let m = ll(pm, x + side * (w / 2.0 + dx), 1.75, opts.t + 0.08, std::f32::consts::PI, 1.0, 1.0, 1.0, 0.0, 0.0);
            asm.add_once(key, &c, Some(&m), Some(AccumAddOpts { masks: Some([0.3, 0.45, 0.2]), paint: None }));
        }
    }

    o
}

/// `rollerShutter(w, h)` (`kit.js:661-675`): a corrugated roller shutter, 1 m
/// tall (`h = 1` at the only call site), scaled by the caller. Sine-corrugated
/// front plane, plus a second copy rotated `PI` about Y for the back face —
/// see [`crate::world::geo::WorldGeo::rotate_y`]'s doc.
fn roller_shutter(w: f32, h: f32) -> WorldGeo {
    let mut g = plane_geometry(w, h, 2, 14);
    for i in 0..g.vert_count() {
        let y = g.pos[i * 3 + 1];
        g.pos[i * 3 + 2] = (y * 90.0).sin() * 0.008;
    }
    g.compute_vertex_normals();
    let mut g2 = g.clone();
    g2.rotate_y(std::f32::consts::PI);
    merge_simple(&[g, g2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shopfront_pinned_drop_always_emits_a_shutter_and_metal_collision() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let o = WallHole { x: 0.0, y: 1.1, w: 3.0, h: 2.2, arch: 0.0, ragged: 0.0 };
        shopfront(&mut asm, &Mat4::IDENTITY, &o, &mut rng, ShopfrontOpts { t: 0.34, drop: Some(0.5), counter: true, inside: false });
        let result = asm.finalize();
        let keys: Vec<&str> = result.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"corrugated"));
        assert!(result.collision.iter().any(|c| c.surface == Surface::Metal));
        assert!(result.collision.iter().any(|c| c.surface == Surface::Wood));
    }

    #[test]
    fn shopfront_zero_drop_never_emits_a_shutter() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let o = WallHole { x: 0.0, y: 1.1, w: 3.0, h: 2.2, arch: 0.0, ragged: 0.0 };
        shopfront(&mut asm, &Mat4::IDENTITY, &o, &mut rng, ShopfrontOpts { t: 0.34, drop: Some(0.0), counter: false, inside: false });
        let result = asm.finalize();
        let keys: Vec<&str> = result.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(!keys.contains(&"corrugated"));
        assert!(!result.collision.iter().any(|c| c.surface == Surface::Metal));
    }

    #[test]
    fn roller_shutter_is_two_planes_merged_front_and_back() {
        let g = roller_shutter(2.0, 1.0);
        let one_plane = super::plane_geometry(2.0, 1.0, 2, 14);
        assert_eq!(g.tri_count(), one_plane.tri_count() * 2);
        assert_eq!(g.vert_count(), one_plane.vert_count() * 2);
    }
}
