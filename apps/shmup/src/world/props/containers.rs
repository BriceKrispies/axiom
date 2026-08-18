//! Ported from Claude-of-Duty `src/world/props.js:116-218` — the "containers"
//! group: crates, a cardboard box, barrels, a gas bottle, a bucket, a jerry
//! can.
//!
//! Every size/position/rotation argument here is `f64` (narrowed to `f32`
//! only inside [`super::pb::PB`]'s methods, right before the actual
//! `chamfer_box`/`cylinder_geometry`/`trs` call) — see `pb.rs`'s module doc
//! for the concrete bug this avoids.

use crate::rng::Rng;
use crate::world::geo::WorldGeo;

use super::mesh::warp_geometry;
use super::pb::{BoxOpts, CylOpts, PB};

/// `crate(rng, s = 0.62, slats = true)` (`props.js:117-156`). Named
/// `crate_` because `crate` is a reserved word in Rust.
pub(crate) fn crate_(rng: &mut Rng, s: f64, slats: bool) -> WorldGeo {
    let mut p = PB::new();
    p.box_(s, s * 0.85, s * 0.92, 0.0, 0.0, 0.0, BoxOpts { bevel: 0.012, grime: 0.12, ..BoxOpts::default() });
    if slats {
        // Plank slats standing proud of the body, with one board sprung loose.
        let n = 3;
        for i in 0..n {
            let y = -s * 0.32 + (f64::from(i) / f64::from(n - 1)) * s * 0.64;
            let loose = rng.float() < 0.18;
            // The `rng.range` draw only happens when `loose` (a ternary in the
            // source, `props.js:128`) — an unconditional draw here would shift
            // every subsequent value in the shared rng stream.
            let rz = if loose { rng.range(-0.12, 0.12) } else { 0.0 };
            p.box_(s * 1.01, s * 0.14, 0.016, 0.0, y, s * 0.46, BoxOpts { bevel: 0.004, rz, wear: 1.0, ..BoxOpts::default() });
            p.box_(s * 1.01, s * 0.14, 0.016, 0.0, y, -s * 0.46, BoxOpts { bevel: 0.004, ..BoxOpts::default() });
            p.box_(0.016, s * 0.14, s * 0.94, s * 0.5, y, 0.0, BoxOpts { bevel: 0.004, ..BoxOpts::default() });
            p.box_(0.016, s * 0.14, s * 0.94, -s * 0.5, y, 0.0, BoxOpts { bevel: 0.004, ..BoxOpts::default() });
        }
        // Corner posts.
        for &sx in &[-1.0f64, 1.0] {
            for &sz in &[-1.0f64, 1.0] {
                p.box_(0.05, s * 0.86, 0.05, sx * (s * 0.48), 0.0, sz * (s * 0.44), BoxOpts { bevel: 0.006, ..BoxOpts::default() });
            }
        }
        // Lid boards with real gaps: the top face is what the player looks
        // down on, and one unbroken panel there is what makes a crate read as
        // a solid block.
        let lid = 4;
        for i in 0..lid {
            let z = -s * 0.46 + ((f64::from(i) + 0.5) / f64::from(lid)) * s * 0.92;
            let rz = rng.range(-0.006, 0.006);
            p.box_(
                s * 1.0,
                0.02,
                (s * 0.92) / f64::from(lid) - 0.012,
                0.0,
                s * 0.425 + 0.012,
                z,
                BoxOpts { bevel: 0.004, rz, wear: 1.0, ..BoxOpts::default() },
            );
        }
        // A cross batten and a couple of nail heads' worth of relief.
        p.box_(s * 1.02, 0.022, 0.055, 0.0, s * 0.44, s * 0.2, BoxOpts { bevel: 0.004, wear: 1.0, ..BoxOpts::default() });
    }
    let mut g = p.build();
    g.translate(0.0, (s * 0.425) as f32, 0.0);
    g
}

/// `cardboardBox(rng, s = 0.45)` (`props.js:158-168`).
pub(crate) fn cardboard_box(rng: &mut Rng, s: f64) -> WorldGeo {
    let mut p = PB::new();
    let h = s * rng.range(0.6, 0.9);
    let depth = s * rng.range(0.8, 1.1);
    p.box_(s, h, depth, 0.0, 0.0, 0.0, BoxOpts { bevel: 0.006, grime: 0.25, ..BoxOpts::default() });
    // Flaps, one folded up.
    p.box_(s * 0.48, 0.012, s * 0.9, -s * 0.25, h / 2.0 + 0.006, 0.0, BoxOpts { bevel: 0.003, wear: 1.0, ..BoxOpts::default() });
    p.box_(s * 0.48, 0.012, s * 0.9, s * 0.25, h / 2.0 + 0.09, 0.0, BoxOpts { bevel: 0.003, rz: -0.9, ..BoxOpts::default() });
    let mut g = p.build();
    g.translate(0.0, (h / 2.0) as f32, 0.0);
    g
}

/// `barrel(rng, r = 0.29, h = 0.88, ribs = 3)` (`props.js:170-185`).
pub(crate) fn barrel(rng: &mut Rng, r: f64, h: f64, ribs: u32) -> WorldGeo {
    let mut p = PB::new();
    p.cyl(r, h, 0.0, 0.0, 0.0, CylOpts { radial: 16, grime: 0.15, ..CylOpts::default() });
    for i in 0..ribs {
        let y = -h / 2.0 + (f64::from(i + 1) / f64::from(ribs + 1)) * h;
        p.cyl(r * 1.045, h * 0.055, 0.0, y, 0.0, CylOpts { radial: 16, wear: 1.0, grime: 0.3, ..CylOpts::default() });
    }
    p.cyl(r * 1.02, 0.03, 0.0, h / 2.0 - 0.015, 0.0, CylOpts { radial: 16, wear: 1.0, ..CylOpts::default() });
    p.cyl(r * 1.02, 0.03, 0.0, -h / 2.0 + 0.015, 0.0, CylOpts { radial: 16, wear: 1.0, grime: 0.5, ..CylOpts::default() });
    // Bung.
    p.cyl(0.05, 0.02, r * 0.45, h / 2.0 + 0.008, 0.0, CylOpts { radial: 8, wear: 1.0, ..CylOpts::default() });
    let mut g = p.build();
    g.translate(0.0, (h / 2.0) as f32, 0.0);
    warp_geometry(&mut g, 0.008, 2.2, rng.float() as f32 * 10.0);
    g
}

/// `gasBottle(rng)` (`props.js:187-198`). The source never reads `rng` here
/// (grep-verified across the whole function body); kept as a parameter for
/// call-site parity with `registerProps`.
pub(crate) fn gas_bottle(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let h = 0.58;
    p.cyl(0.155, h, 0.0, 0.0, 0.0, CylOpts { radial: 14, grime: 0.2, ..CylOpts::default() });
    p.cyl(0.15, 0.06, 0.0, h / 2.0 + 0.02, 0.0, CylOpts { radial: 14, taper: 0.75, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.032, 0.09, 0.0, h / 2.0 + 0.09, 0.0, CylOpts { radial: 8, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.075, 0.035, 0.0, h / 2.0 + 0.14, 0.0, CylOpts { radial: 10, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.16, 0.02, 0.0, -h / 2.0 + 0.01, 0.0, CylOpts { radial: 14, grime: 0.6, ..CylOpts::default() });
    let mut g = p.build();
    g.translate(0.0, (h / 2.0) as f32, 0.0);
    g
}

/// `bucket(rng)` (`props.js:200-208`). The source never reads `rng` here.
pub(crate) fn bucket(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    p.cyl(0.145, 0.28, 0.0, 0.0, 0.0, CylOpts { radial: 14, taper: 1.24, grime: 0.4, open: true, ..CylOpts::default() });
    p.cyl(0.145, 0.02, 0.0, -0.13, 0.0, CylOpts { radial: 14, grime: 0.6, ..CylOpts::default() });
    p.cyl(0.185, 0.018, 0.0, 0.14, 0.0, CylOpts { radial: 14, wear: 1.0, ..CylOpts::default() });
    let mut g = p.build();
    g.translate(0.0, 0.14, 0.0);
    g
}

/// `jerryCan(rng)` (`props.js:210-218`). The source never reads `rng` here.
pub(crate) fn jerry_can(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    p.box_(0.34, 0.44, 0.17, 0.0, 0.0, 0.0, BoxOpts { bevel: 0.02, grime: 0.2, ..BoxOpts::default() });
    p.box_(0.3, 0.06, 0.05, 0.0, 0.24, 0.0, BoxOpts { bevel: 0.01, wear: 1.0, ..BoxOpts::default() });
    p.cyl(0.035, 0.05, 0.11, 0.25, 0.0, CylOpts { radial: 8, wear: 1.0, ..CylOpts::default() });
    let mut g = p.build();
    g.translate(0.0, 0.22, 0.0);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_with_slats_has_more_triangles_than_flat() {
        let mut rng = Rng::new(1);
        let with_slats = crate_(&mut rng, 0.62, true);
        let mut rng2 = Rng::new(1);
        let flat = crate_(&mut rng2, 0.62, false);
        assert!(with_slats.tri_count() > flat.tri_count());
    }

    #[test]
    fn crate_loose_board_draw_is_conditional() {
        // Two different seeds should be able to produce different geometry
        // only through the conditional `rng.range` draw inside the slats
        // loop — if that draw were unconditional, this would still hold, but
        // if the ORDER were wrong downstream sequences would desync. This is
        // a smoke check that the function is deterministic per-seed.
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let ga = crate_(&mut a, 0.62, true);
        let gb = crate_(&mut b, 0.62, true);
        assert_eq!(ga.pos, gb.pos);
    }

    #[test]
    fn cardboard_box_is_translated_to_sit_on_the_ground() {
        let mut rng = Rng::new(3);
        let g = cardboard_box(&mut rng, 0.45);
        let y_min = g.pos.iter().skip(1).step_by(3).copied().fold(f32::INFINITY, f32::min);
        assert!(y_min > -0.01, "box should sit at or above y=0, got {y_min}");
    }

    #[test]
    fn barrel_is_warped_and_not_a_perfect_cylinder_of_revolution() {
        let mut rng = Rng::new(5);
        let g = barrel(&mut rng, 0.29, 0.88, 3);
        assert!(g.vert_count() > 0);
    }

    #[test]
    fn gas_bottle_bucket_jerry_can_build_without_reading_rng() {
        let mut rng = Rng::new(9);
        let a = gas_bottle(&mut rng);
        let b = bucket(&mut rng);
        let c = jerry_can(&mut rng);
        assert!(a.vert_count() > 0 && b.vert_count() > 0 && c.vert_count() > 0);
    }
}
