//! Ported from Claude-of-Duty `src/world/props.js:220-403` — the "cover"
//! group: sandbags, a jersey barrier, concrete blocks, tyres, a pallet.
//!
//! Every size/position/rotation argument here is `f64` — see `pb.rs`'s
//! module doc for why.

use crate::rng::Rng;
use crate::weapons::geometry::primitives::{extrude, lathe_geometry, ExtrudeOpts};
use crate::world::geo::WorldGeo;
use crate::world::kit::rock_geometry;
use crate::world::noise::fbm3;

use super::mesh::{auto_edge_wear, bounds_axis, sack_geometry, sign3, SackOpts};
use super::pb::{BoxOpts, CylOpts, GeoOpts, PB};

/// `sandbag(rng, i = 0)` (`props.js:226-255`): three genuinely different
/// silhouettes so a wall built from one bag isn't a lattice of identical
/// lozenges.
pub(crate) fn sandbag(rng: &mut Rng, i: u32) -> WorldGeo {
    const DIMS: [[f64; 3]; 3] = [[0.49, 0.175, 0.33], [0.45, 0.16, 0.35], [0.47, 0.15, 0.3]];
    let variant = i % 3;
    let dims = DIMS[variant as usize];
    let mut g = sack_geometry(rng, dims[0], dims[1], dims[2], SackOpts { variant, box_p: 4.6 - f64::from(variant) * 0.5, lump: 1.2 });
    let y_min = f64::from(bounds_axis(&g.pos, 1).0);
    g.paint_masks(|x, y, z, _nx, ny, _nz, out, _i| {
        let (x, y, z, ny) = (f64::from(x), f64::from(y), f64::from(z), f64::from(ny));
        let n = fbm3(x * 12.0, y * 12.0, z * 12.0, 2);
        // Creases and the underside of the bag: where dust and shadow collect.
        let crease = (1.0 - ny.abs() * 3.2).max(0.0);
        let low = (1.0 - (y - y_min) / (dims[1] * 0.55)).max(0.0);
        // The tied ends are the darkest part of a bag, and they draw the seam
        // between one bag and the next in a stack.
        let end = ((x.abs() / (dims[0] * 0.5) - 0.62) / 0.38).max(0.0);
        // Bags weather hard: sun-bleached on top, filthy where they touch.
        out[0] = (0.3 + n * 0.45 + ny.max(0.0) * 0.2) as f32;
        // Keep the hessian pale: bags are only filthy where they touch, and
        // burying the weave under grime is what makes sandbags read as
        // beanbags.
        out[1] = (0.16 + (-ny).max(0.0) * 0.45 + n * 0.14 + low * low * 0.3 + end * 0.25) as f32;
        out[2] = (0.1 + (-ny).max(0.0) * 0.45 + crease * 0.22 + low * low * 0.35 + end * end * 0.5) as f32;
    });
    g.translate(0.0, (dims[1] * 0.5) as f32, 0.0);
    g
}

/// `jerseyBarrier(rng)` (`props.js:257-297`): a proper jersey profile —
/// splayed foot, sloped face, narrow top — extruded, plus lifting eyes and a
/// scuffed reflector.
///
/// **Reuses `weapons::geometry::primitives::extrude`**, exactly the trade
/// `crate::world::kit::poly_prism` documents (bevelled-extrude-with-holes
/// engine rather than a second `THREE.ExtrudeGeometry` copy). The source's
/// raw `ExtrudeGeometry` is never translated (`z` spans `0..depth`), then
/// explicitly translated `-depth/2` (`props.js:283`); `extrude()` always
/// translates `-depth/2 + bevel`, so the correction here is `translate(0, 0,
/// -bevel)` — undo just the extra `+bevel` term, unlike `poly_prism`'s own
/// correction (which also un-does a subsequent axis rotation this shape
/// never performs).
pub(crate) fn jersey_barrier(_rng: &mut Rng) -> WorldGeo {
    let prof: [[f64; 2]; 10] = [
        [-0.3, 0.0],
        [0.3, 0.0],
        [0.3, 0.09],
        [0.16, 0.24],
        [0.09, 0.72],
        [0.09, 0.92],
        [-0.09, 0.92],
        [-0.09, 0.72],
        [-0.16, 0.24],
        [-0.3, 0.09],
    ];
    let depth = 1.9f32;
    let bevel = 0.015f32;
    let raw = extrude(&prof, depth, ExtrudeOpts { bevel, bevel_segments: 1, curve_segments: 6, steps: 1, holes: Vec::new() });
    let mut g = WorldGeo { pos: raw.pos, normal: raw.normal, uv: raw.uv, color: Vec::new(), index: raw.index };
    g.translate(0.0, 0.0, -bevel);
    g.compute_vertex_normals();
    auto_edge_wear(&mut g, 0.035, 1.0);

    let mut p = PB::new();
    p.geo(g, 0.0, 0.0, 0.0, GeoOpts { auto_wear: false, grime: 0.15, ..GeoOpts::default() });
    // Lifting eyes and a scuffed reflector.
    p.cyl(0.035, 0.1, 0.0, 0.95, -0.55, CylOpts { radial: 8, rx: std::f64::consts::FRAC_PI_2, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.035, 0.1, 0.0, 0.95, 0.55, CylOpts { radial: 8, rx: std::f64::consts::FRAC_PI_2, wear: 1.0, ..CylOpts::default() });
    let mut out = p.build();
    out.paint_masks(|_x, y, _z, _nx, ny, _nz, o, _i| {
        let (y, ny) = (f64::from(y), f64::from(ny));
        o[1] = (f64::from(o[1]) + (1.0 - y / 0.35).max(0.0).powi(2) * 0.6 + (-ny).max(0.0) * 0.4).min(1.0) as f32;
        o[2] = (f64::from(o[2]) + (1.0 - y / 0.3).max(0.0).powi(2) * 0.45).min(1.0) as f32;
    });
    out
}

/// `concreteBlock(rng, w = 1.2, h = 0.9, d = 0.8)` (`props.js:299-308`).
pub(crate) fn concrete_block(rng: &mut Rng, w: f64, h: f64, d: f64) -> WorldGeo {
    let mut p = PB::new();
    p.box_(w, h, d, 0.0, 0.0, 0.0, BoxOpts { bevel: 0.03, grime: 0.2, ..BoxOpts::default() });
    // Chipped corner.
    let chip = rock_geometry(rng, 0.34, 0, 0.8);
    p.geo(chip, w / 2.0 - 0.06, h / 2.0 - 0.05, d / 2.0 - 0.08, GeoOpts { grime: 0.4, ..GeoOpts::default() });
    let mut g = p.build();
    g.translate(0.0, (h / 2.0) as f32, 0.0);
    g
}

/// `tyre(rng, r = 0.33)` (`props.js:316-383`): a real tread band (17
/// deliberately low-poly blocks so it resolves at 3 m instead of aliasing),
/// a distinct shoulder/crown profile revolved rather than a plain torus, and
/// sidewall lettering relief.
///
/// **Reuses `weapons::geometry::primitives::lathe_geometry`** directly
/// (rather than [`crate::weapons::geometry::primitives::lathe_z`], which
/// rotates its result onto `+Z` for weapon parts): the tyre revolves around
/// `+Y` exactly as `THREE.LatheGeometry` does natively, with no axis
/// rotation, matching the source.
pub(crate) fn tyre(rng: &mut Rng, r: f64) -> WorldGeo {
    const BLOCKS: f64 = 17.0;
    let radial = (BLOCKS * 5.0) as u32;
    let hw = r * 0.3; // half the section width
    #[rustfmt::skip]
    let profile_raw: [[f64; 2]; 15] = [
        [0.52, 0.45], [0.66, 0.88], [0.82, 1.0], [0.94, 0.92], [0.995, 0.62],
        [1.0, 0.35], [1.0, -0.35], [0.995, -0.62], [0.94, -0.92], [0.82, -1.0],
        [0.66, -0.88], [0.52, -0.45], [0.5, -0.18], [0.505, 0.18], [0.52, 0.45],
    ];
    let profile: Vec<(f64, f64)> = profile_raw.iter().map(|&[pr, py]| (pr * r, py * hw)).collect();
    let raw = lathe_geometry(&profile, radial, 0.0, std::f64::consts::TAU);
    let mut g = WorldGeo { pos: raw.pos, normal: raw.normal, uv: raw.uv, color: Vec::new(), index: raw.index };

    let stagger = rng.float() * 6.28;
    for i in 0..g.vert_count() {
        let x = f64::from(g.pos[i * 3]);
        let y = f64::from(g.pos[i * 3 + 1]);
        let z = f64::from(g.pos[i * 3 + 2]);
        let a = z.atan2(x);
        let rr = x.hypot(z);
        // Tread blocks: a square wave round the crown, split by a centre
        // groove.
        let ph = (a * BLOCKS) / (std::f64::consts::PI * 2.0) + stagger;
        let blk_t = ph.rem_euclid(1.0);
        // A block occupying 62% of the pitch with chamfered leading/trailing
        // edges.
        let blk = (blk_t / 0.075).min((0.62 - blk_t) / 0.075).min(1.0).max(0.0);
        let centre = (-((y / (hw * 0.22)).powi(2)) * 3.0).exp(); // circumferential groove
        let tread_band = ((rr / r - 0.9) / 0.1).max(0.0) * (1.0 - y.abs() / (hw * 0.72)).max(0.0);
        // 9 mm of tread relief: enough to read as blocks at 3 m, not a
        // monster truck.
        let grow = tread_band * (blk * 0.0062 - 0.0018 - centre * 0.0045) * (r / 0.33);
        let f = 1.0 + grow / rr.max(1e-4);
        // Sidewall lettering / brand ring relief, pushed along the sidewall
        // normal.
        let band = (-(((rr / r - 0.76) / 0.11).powi(2))).exp();
        let bump = if (a * 23.0 + stagger * 3.0).sin() > 0.4 { 0.006 } else { 0.0 };
        let letter = band * (bump + 0.0022) * (r / 0.33);
        // `Math.sign(y)`, not `signum` — see `super::mesh::sign3`'s doc: `y`
        // legitimately lands on exactly `0.0` along the tyre's equator.
        g.pos[i * 3] = (x * f) as f32;
        g.pos[i * 3 + 1] = (y * 0.94 + sign3(y) * letter) as f32;
        g.pos[i * 3 + 2] = (z * f) as f32;
    }
    g.compute_vertex_normals();
    g.paint_masks(|x, y, z, _nx, ny, _nz, out, _i| {
        let (x, y, z, ny) = (f64::from(x), f64::from(y), f64::from(z), f64::from(ny));
        let rr = x.hypot(z);
        // The crown is scrubbed clean-ish, the sidewalls and grooves hold
        // dust.
        let crown = ((rr / r - 0.88) / 0.12).min(1.0).max(0.0);
        let hole = (1.0 - (rr / r - 0.5) / 0.12).max(0.0); // inside the bead
        let n = fbm3(x * 9.0, y * 9.0, z * 9.0, 2);
        out[0] = (0.25 + crown * 0.4 + n * 0.25) as f32;
        out[1] = (0.3 + (1.0 - crown) * 0.35 + (-ny).max(0.0) * 0.3) as f32;
        out[2] = (0.12 + (1.0 - crown) * 0.25 + (-ny).max(0.0) * 0.3 + hole * 0.5) as f32;
    });
    g.translate(0.0, (hw * 0.95) as f32, 0.0);
    g
}

/// `pallet(rng)` (`props.js:385-403`).
///
/// **Source quirk, ported not fixed**: unlike every other prototype in this
/// file, `pallet()` never translates its result to sit flush on `y = 0` — it
/// returns `p.build()` directly. Its bottom skid boards (centred at `y =
/// -0.008`, half-height `0.009`) therefore sit ~1.7 cm into the ground. Pinned
/// as-is per the port recipe's "port the behaviour, don't silently fix a
/// defect" rule.
pub(crate) fn pallet(rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let w = 1.16;
    let d = 0.98;
    for i in 0..3 {
        let z = -d / 2.0 + 0.06 + (f64::from(i) / 2.0) * (d - 0.12);
        p.box_(w, 0.075, 0.11, 0.0, 0.04, z, BoxOpts { bevel: 0.006, grime: 0.3, ..BoxOpts::default() });
    }
    let boards = 6;
    for i in 0..boards {
        let z = -d / 2.0 + 0.05 + (f64::from(i) / f64::from(boards - 1)) * (d - 0.1);
        let rz = rng.range(-0.004, 0.004);
        p.box_(w, 0.018, 0.1, 0.0, 0.088, z, BoxOpts { bevel: 0.004, rz, ..BoxOpts::default() });
    }
    for i in 0..3 {
        let z = -d / 2.0 + 0.06 + (f64::from(i) / 2.0) * (d - 0.12);
        p.box_(w, 0.018, 0.1, 0.0, -0.008, z, BoxOpts { bevel: 0.004, ..BoxOpts::default() });
    }
    p.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbag_variants_all_have_positive_volume_and_differ() {
        let mut rng = Rng::new(1);
        let a = sandbag(&mut rng, 0);
        let b = sandbag(&mut rng, 1);
        let c = sandbag(&mut rng, 2);
        assert!(a.vert_count() > 0 && b.vert_count() > 0 && c.vert_count() > 0);
        assert_ne!(a.pos, b.pos);
        assert_ne!(b.pos, c.pos);
    }

    #[test]
    fn jersey_barrier_spans_the_documented_depth() {
        let mut rng = Rng::new(1);
        let g = jersey_barrier(&mut rng);
        let (z0, z1) = bounds_axis(&g.pos, 2);
        assert!((z1 - z0 - 1.9).abs() < 0.05, "z span should be about 1.9 m, got {}", z1 - z0);
    }

    #[test]
    fn concrete_block_has_a_chipped_corner_merged_in() {
        let mut rng = Rng::new(2);
        let plain_tris = {
            let mut p = super::PB::new();
            p.box_(1.2, 0.9, 0.8, 0.0, 0.0, 0.0, BoxOpts::default());
            p.build().tri_count()
        };
        let g = concrete_block(&mut rng, 1.2, 0.9, 0.8);
        assert!(g.tri_count() > plain_tris);
    }

    #[test]
    fn tyre_is_roughly_annular_around_the_y_axis() {
        let mut rng = Rng::new(3);
        let g = tyre(&mut rng, 0.33);
        let (y0, y1) = bounds_axis(&g.pos, 1);
        // hw = r*0.3 = 0.099, translated up by hw*0.95 = 0.09405: the section
        // spans roughly [-hw, hw] before the translate, so [~-0.005, ~0.193]
        // after — the tread's small radial `grow` perturbs this by only a
        // few mm, hence the generous but still tight margin below.
        assert!(y0 > -0.02 && y0 < 0.02, "tyre y_min {y0} should be near 0");
        assert!(y1 > 0.15 && y1 < 0.25, "tyre y_max {y1} should be near 2*hw");
    }

    #[test]
    fn pallet_is_not_translated_off_the_ground_source_quirk() {
        let mut rng = Rng::new(4);
        let g = pallet(&mut rng);
        let (y0, _) = bounds_axis(&g.pos, 1);
        assert!(y0 < 0.0, "pallet's bottom boards should dip below y=0 (source quirk), got y_min={y0}");
    }
}
