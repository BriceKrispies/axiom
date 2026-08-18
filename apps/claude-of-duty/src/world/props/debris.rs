//! Ported from Claude-of-Duty `src/world/props.js:591-720` — the "debris"
//! group: a brick chunk, a slab shard with rebar, a rebar bundle, a plank, a
//! litter scrap, a bottle, a crushed can. (`dust_skirt`/`pock` — also debris
//! by registry grouping — live in [`super::mesh`], since they're hand-rolled
//! low-level builders rather than `PB` assemblies.)

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::geo::WorldGeo;
use crate::world::kit::{chamfer_box, cylinder_geometry, plane_geometry, poly_prism, rock_geometry};
use crate::world::noise::fbm3;

use super::mesh::{auto_edge_wear, warp_geometry};
use super::pb::{CylOpts, GeoOpts, PB};

/// `brickChunk(rng)` (`props.js:591-599`).
pub(crate) fn brick_chunk(rng: &mut Rng) -> WorldGeo {
    let mut g = rock_geometry(rng, 0.22, 0, 0.55);
    g.paint_masks(|x, y, z, _nx, ny, _nz, out, _i| {
        let (x, y, z, ny) = (f64::from(x), f64::from(y), f64::from(z), f64::from(ny));
        out[0] = (0.5 + fbm3(x * 9.0, y * 9.0, z * 9.0, 2) * 0.5) as f32;
        out[1] = (0.4 + (-ny).max(0.0) * 0.4) as f32;
        out[2] = 0.25;
    });
    g
}

/// `slabShard(rng)` (`props.js:601-627`): an irregular concrete fragment
/// (`polyPrism` over an fbm-wobbled outline) with 2-4 bent rebar stubs
/// sticking out.
///
/// **Reuses `crate::world::kit::poly_prism`** for the fragment body — the
/// same `THREE.ExtrudeGeometry`-backed builder `props.js`'s own `polyPrism`
/// import names — rather than a second copy. `poly_prism`'s own `height`/
/// `bevel` parameters are `f32` (an established contract elsewhere in this
/// port); every value stays `f64` right up to that one call.
pub(crate) fn slab_shard(rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let w = rng.range(0.5, 0.95);
    let d = rng.range(0.35, 0.7);
    let n = 7;
    let mut pts: Vec<[f64; 2]> = Vec::with_capacity(n);
    for i in 0..n {
        let t = (i as f64 / n as f64) * std::f64::consts::TAU;
        let rr = 0.5 * (0.6 + fbm3(t.cos() * 3.0 + 2.0, t.sin() * 3.0, 5.0, 2) * 0.8);
        pts.push([t.cos() * rr * w, t.sin() * rr * d]);
    }
    let height = rng.range(0.07, 0.13);
    let mut g = poly_prism(&pts, height as f32, 0.0);
    auto_edge_wear(&mut g, 0.02, 1.0);
    p.geo(g, 0.0, 0.0, 0.0, GeoOpts { auto_wear: false, grime: 0.4, ..GeoOpts::default() });

    // Rebar sticking out, bent.
    let bars = rng.int(2, 4);
    for _ in 0..bars {
        let a = rng.float() * std::f64::consts::TAU;
        // Draw order matches the source's argument-evaluation order exactly:
        // `a`, then the cylinder's own height, then `rz`, then `rx`
        // (`props.js:617-624`) — an extra or reordered draw here would shift
        // every subsequent value in the shared rng stream.
        let bar_h = rng.range(0.3, 0.7);
        let rz = rng.range(-1.4, 1.4);
        let rx = rng.range(-1.2, 1.2);
        p.cyl(0.008, bar_h, a.cos() * w * 0.3, 0.06, a.sin() * d * 0.3, CylOpts { radial: 5, rz, rx, grime: 0.5, ..CylOpts::default() });
    }
    p.build()
}

/// `rebarBundle(rng)` (`props.js:629-641`).
pub(crate) fn rebar_bundle(rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let n = rng.int(4, 7);
    for i in 0..n {
        let bar_h = rng.range(1.4, 2.6);
        let x = rng.range(-0.08, 0.08);
        let z = rng.range(-0.06, 0.06);
        let ry = rng.range(-0.12, 0.12);
        p.cyl(
            0.009,
            bar_h,
            x,
            0.012 + f64::from(i) * 0.019,
            z,
            CylOpts { radial: 5, rx: std::f64::consts::FRAC_PI_2, ry, grime: 0.55, ..CylOpts::default() },
        );
    }
    p.build()
}

/// `plank(rng)` (`props.js:643-651`).
pub(crate) fn plank(rng: &mut Rng) -> WorldGeo {
    let length = rng.range(0.9, 2.1);
    let width = rng.range(0.12, 0.2);
    let mut g = chamfer_box(length as f32, 0.035, width as f32, 0.005);
    auto_edge_wear(&mut g, 0.012, 1.0);
    warp_geometry(&mut g, 0.012, 1.4, rng.float() as f32 * 9.0);
    g.paint_masks(|_x, _y, _z, _nx, ny, _nz, out, _i| {
        out[1] = (out[1] + 0.3 + (-ny).max(0.0) * 0.4).min(1.0);
    });
    g
}

/// `litterPaper(rng)` (`props.js:688-698`).
pub(crate) fn litter_paper(rng: &mut Rng) -> WorldGeo {
    let width = rng.range(0.1, 0.22);
    let height = rng.range(0.1, 0.28);
    let mut g = plane_geometry(width as f32, height as f32, 2, 2);
    for p in g.pos.chunks_exact_mut(3) {
        let (x, y) = (f64::from(p[0]), f64::from(p[1]));
        p[2] = ((fbm3(x * 20.0, y * 20.0, 3.0, 2) - 0.5) * 0.035) as f32;
    }
    g.rotate_x(-std::f32::consts::FRAC_PI_2);
    g.compute_vertex_normals();
    g.fill_masks(0.3, 0.5, 0.2);
    g
}

/// `bottle(rng)` (`props.js:700-705`). The source never reads `rng` here.
pub(crate) fn bottle(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    p.cyl(0.038, 0.17, 0.0, 0.085, 0.0, CylOpts { radial: 10, grime: 0.3, ..CylOpts::default() });
    p.cyl(0.02, 0.08, 0.0, 0.2, 0.0, CylOpts { radial: 8, taper: 0.8, ..CylOpts::default() });
    p.build()
}

/// `Matrix4.makeRotationZ(angle)`: standard right-handed rotation about Z,
/// used only by [`can`] below (the source's `g.rotateZ(1.4)`,
/// `props.js:717`) — no other builder in this port rotates about Z, so this
/// is kept local rather than added to [`crate::world::geo::WorldGeo`]
/// alongside its existing `rotate_x`.
fn rotate_z(g: &mut WorldGeo, angle: f32) {
    let (s, c) = angle.sin_cos();
    let m = Mat4::from_cols_array([
        c, s, 0.0, 0.0, //
        -s, c, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ]);
    g.apply(&m);
}

/// `can(rng)` (`props.js:707-720`): a crushed drink can. The source never
/// reads `rng` here.
pub(crate) fn can(_rng: &mut Rng) -> WorldGeo {
    let mut g = cylinder_geometry(0.033, 0.033, 0.115, 10, 1, false);
    auto_edge_wear(&mut g, 0.01, 1.0);
    // Crushed.
    for p in g.pos.chunks_exact_mut(3) {
        let y = p[1];
        p[0] *= 1.0 - y.abs() * 1.2;
    }
    g.compute_vertex_normals();
    rotate_z(&mut g, 1.4);
    g.translate(0.0, 0.033, 0.0);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brick_chunk_and_bottle_build_nonempty() {
        let mut rng = Rng::new(1);
        assert!(brick_chunk(&mut rng).vert_count() > 0);
        assert!(bottle(&mut rng).vert_count() > 0);
    }

    #[test]
    fn slab_shard_merges_a_body_and_two_to_four_bars() {
        let mut rng = Rng::new(2);
        let g = slab_shard(&mut rng);
        assert!(g.vert_count() > 0);
    }

    #[test]
    fn rebar_bundle_has_four_to_seven_bars() {
        let mut rng = Rng::new(3);
        let g = rebar_bundle(&mut rng);
        assert!(g.vert_count() > 0);
    }

    #[test]
    fn plank_is_warped_and_marked_grimy_on_its_underside() {
        let mut rng = Rng::new(4);
        let g = plank(&mut rng);
        assert!(g.color.iter().any(|&c| c > 0.0));
    }

    #[test]
    fn litter_paper_lies_flat_on_the_ground_plane() {
        let mut rng = Rng::new(5);
        let g = litter_paper(&mut rng);
        let y_max = g.pos.iter().skip(1).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(y_max < 0.05, "litter should lie nearly flat, y_max={y_max}");
    }

    #[test]
    fn can_is_crushed_narrower_at_its_rim_than_a_plain_cylinder() {
        let mut rng = Rng::new(6);
        let g = can(&mut rng);
        assert!(g.vert_count() > 0);
    }
}
