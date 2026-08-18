//! Ported from Claude-of-Duty `src/world/kit.js:960-1094` — `pockGeometry`
//! (bullet pocks), `spallPatch` (a crumbled corner exposing the substrate),
//! and `rubbleMound` (a scattered pile of masonry chunks).

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::geo::WorldGeo;
use crate::world::noise::fbm3;

use super::{ll, poly_prism, rock_geometry};

/// `pockGeometry(rng, r = 0.05)` (`kit.js:982-1041`): a shallow crater with a
/// chipped rim — see the source's own doc comment for why this is a crater
/// (floor near-flush, rim raised) rather than the convex spike a naive
/// `ConeGeometry` would produce. Draws from `rng` **exactly 16 times** (8
/// rim-radius jitters, then 8 rim-crest-height jitters) — the source's own
/// comment notes `registerProps` shares one stream with the whole level
/// build, so changing this draw count would silently re-roll everything
/// downstream.
pub fn pock_geometry(rng: &mut Rng, r: f32) -> WorldGeo {
    const SEG: usize = 8;
    // (radius factor, height factor) — floor, bowl wall, rim crest, outer skirt.
    const RINGS: [(f64, f64); 4] = [(0.0, 0.010), (0.42, 0.024), (0.8, 0.075), (1.0, 0.004)];

    // Chipped rim: per-segment radius and crest-height jitter. 8 + 8 = 16 draws.
    let jr: Vec<f64> = (0..SEG).map(|_| 1.0 + (rng.float() - 0.5) * 0.42).collect();
    let jz: Vec<f64> = (0..SEG).map(|_| 0.62 + rng.float() * 0.76).collect();

    let r64 = f64::from(r);
    let mut pos: Vec<f64> = vec![0.0, 0.0, RINGS[0].1 * r64];
    // Ring 0 is the single centre vertex; rings 1..3 are full circles.
    for k in 1..RINGS.len() {
        let (rf, zf) = RINGS[k];
        for s in 0..SEG {
            let a = (s as f64 / SEG as f64) * std::f64::consts::TAU;
            // Only the two outer rings are chipped; the bowl stays smooth
            // so the floor does not poke through the wall.
            let rj = if k >= 2 { jr[s] } else { 1.0 };
            let zj = if k == 2 { jz[s] } else { 1.0 };
            pos.push(a.cos() * rf * r64 * rj);
            pos.push(a.sin() * rf * r64 * rj);
            pos.push(zf * r64 * zj);
        }
    }

    let ring_start = |k: usize| -> usize { 1 + (k - 1) * SEG };
    let mut idx: Vec<u32> = Vec::new();
    for s in 0..SEG {
        let n = (s + 1) % SEG;
        idx.push(0);
        idx.push((ring_start(1) + s) as u32);
        idx.push((ring_start(1) + n) as u32);
    }
    for k in 1..RINGS.len() - 1 {
        let a0 = ring_start(k);
        let b0 = ring_start(k + 1);
        for s in 0..SEG {
            let n = (s + 1) % SEG;
            idx.extend_from_slice(&[(a0 + s) as u32, (b0 + s) as u32, (b0 + n) as u32, (a0 + s) as u32, (b0 + n) as u32, (a0 + n) as u32]);
        }
    }

    let mut g = WorldGeo {
        pos: pos.iter().map(|&v| v as f32).collect(),
        normal: Vec::new(),
        uv: Vec::new(),
        color: Vec::new(),
        index: idx,
    };
    g.compute_vertex_normals();
    // Wear (exposed substrate) and AO are strongest in the crater floor; the
    // raised rim is cleaner and catches light, which is what sells the depth.
    g.paint_masks(|x, y, _z, _nx, _ny, _nz, out, _i| {
        let t = (x.hypot(y) / (r * 0.8)).min(1.0);
        out[0] = 0.9 - 0.35 * t;
        out[1] = 0.62 - 0.3 * t;
        out[2] = 0.9 - 0.55 * t;
    });
    g
}

/// `spallPatch(rng, w, h, depth = 0.03)` (`kit.js:1044-1065`): a crumbled
/// corner / spalled patch exposing the substrate under the render. Draws
/// nothing from `rng` — the polygon's irregularity comes from `fbm3` keyed
/// on the angle `t` alone, not from `rng`, exactly matching the source's
/// unused `rng` parameter.
pub fn spall_patch(_rng: &mut Rng, w: f32, h: f32, depth: f32) -> WorldGeo {
    let n = 11;
    let pts: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let t = (f64::from(i) / f64::from(n)) * std::f64::consts::TAU;
            let rr = 0.5 * (1.0 + (fbm3(t.cos() * 2.0 + 9.0, t.sin() * 2.0 + 3.0, 1.7, 2) - 0.5) * 0.9);
            [t.cos() * rr * f64::from(w), t.sin() * rr * f64::from(h)]
        })
        .collect();
    let mut g = poly_prism(&pts, depth, 0.0);
    // Undoes `polyPrism`'s own internal `rotateX(-PI/2)` — see
    // `crate::world::kit::poly_prism`'s doc for the net effect: this leaves
    // the raw (never-rotated) extrusion, with the polygon's own XY plane
    // becoming this geometry's XZ (ground) plane after the SECOND rotation
    // below composes with the first.
    g.rotate_x(std::f32::consts::FRAC_PI_2);
    g.paint_masks(|_x, _y, _z, _nx, _ny, nz, out, _i| {
        out[0] = 0.3;
        out[1] = 0.55 + 0.3 * (1.0 - nz.abs());
        out[2] = 0.5 * (1.0 - nz.abs()) + 0.2;
    });
    g
}

/// `rubbleMound(A, rng, x, y, z, radius, count, opts = {})`'s `opts`
/// (`kit.js:1069`). Default: `key="concrete"`.
pub struct RubbleOpts<'a> {
    pub key: &'a str,
}

/// `rubbleMound(A, rng, x, y, z, radius, count, opts = {})`
/// (`kit.js:1068-1094`): a low pile of masonry chunks and dust — `count`
/// noise-deformed rocks scattered within `radius` (denser and larger toward
/// the centre), plus one flattened collision box for the whole mound. Every
/// rock is authored in LEVEL space directly via `IDENT`
/// ([`axiom_math::Mat4::IDENTITY`]), matching `kit.js:1079`'s `IDENT`
/// argument to `LL`.
#[allow(clippy::too_many_arguments)]
pub fn rubble_mound(asm: &mut Assembler, rng: &mut Rng, x: f32, y: f32, z: f32, radius: f32, count: u32, opts: RubbleOpts) {
    for _ in 0..count {
        let a = rng.float() as f32 * std::f32::consts::PI * 2.0;
        let rr = (rng.float() as f32).sqrt() * radius;
        let s = rng.range(0.09, 0.3) as f32 * (1.0 - rr / radius / 1.6);
        let g = rock_geometry(rng, s, 0, 0.75);
        let ry = rng.float() as f32 * 6.28;
        let rx = rng.range(-0.4, 0.4) as f32;
        let rz = rng.range(-0.4, 0.4) as f32;
        let m = ll(
            &Mat4::IDENTITY,
            x + a.cos() * rr,
            y + s * 0.3 + (0.0f32).max((1.0 - rr / radius) * radius * 0.3),
            z + a.sin() * rr,
            ry,
            1.0,
            1.0,
            1.0,
            rx,
            rz,
        );
        asm.add_once(opts.key, &g, Some(&m), Some(AccumAddOpts { masks: Some([0.3, 0.75, 0.45]), paint: None }));
    }
    let surface = asm.surface_of(opts.key);
    asm.collide_box(surface, x, y + radius * 0.14, z, radius * 1.5, radius * 0.34, radius * 1.5, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pock_geometry_draws_exactly_sixteen_rng_values() {
        let mut rng_a = Rng::new(1);
        let mut rng_b = Rng::new(1);
        pock_geometry(&mut rng_a, 0.05);
        for _ in 0..16 {
            rng_b.float();
        }
        assert_eq!(rng_a.state(), rng_b.state());
    }

    #[test]
    fn pock_geometry_has_the_expected_ring_topology() {
        let mut rng = Rng::new(1);
        let g = pock_geometry(&mut rng, 0.05);
        // 1 centre + 3 rings of 8 = 25 vertices.
        assert_eq!(g.vert_count(), 25);
        // Floor fan (8) + 2 middle bands x 8 quads x 2 tris (32) = 40 tris.
        assert_eq!(g.tri_count(), 8 + 2 * 8 * 2);
    }

    #[test]
    fn spall_patch_never_draws_from_rng() {
        let mut rng_a = Rng::new(5);
        let rng_b = Rng::new(5);
        spall_patch(&mut rng_a, 1.0, 0.8, 0.03);
        assert_eq!(rng_a.state(), rng_b.state());
    }

    #[test]
    fn spall_patch_produces_non_empty_geometry() {
        let mut rng = Rng::new(1);
        let g = spall_patch(&mut rng, 1.0, 0.8, 0.03);
        assert!(g.vert_count() > 0);
        assert!(g.tri_count() > 0);
    }

    #[test]
    fn rubble_mound_emits_one_rock_batch_and_one_collision_box() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        rubble_mound(&mut asm, &mut rng, 0.0, 0.0, 0.0, 2.0, 12, RubbleOpts { key: "concrete" });
        let out = asm.finalize();
        assert_eq!(out.statics.len(), 1);
        assert_eq!(out.statics[0].key, "concrete");
        assert_eq!(out.collision.len(), 1);
        assert_eq!(out.collision[0].geo.tri_count(), 12);
    }

    #[test]
    fn rubble_mound_deterministic_from_the_same_seed() {
        let mut asm_a = Assembler::new(Rng::new(1));
        let mut rng_a = Rng::new(9);
        rubble_mound(&mut asm_a, &mut rng_a, 0.0, 0.0, 0.0, 2.0, 5, RubbleOpts { key: "concrete" });
        let mut asm_b = Assembler::new(Rng::new(1));
        let mut rng_b = Rng::new(9);
        rubble_mound(&mut asm_b, &mut rng_b, 0.0, 0.0, 0.0, 2.0, 5, RubbleOpts { key: "concrete" });
        let a = asm_a.finalize();
        let b = asm_b.finalize();
        assert_eq!(a.statics[0].geo.pos, b.statics[0].geo.pos);
    }
}
