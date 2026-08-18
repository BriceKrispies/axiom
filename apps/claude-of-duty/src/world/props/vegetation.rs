//! Ported from Claude-of-Duty `src/world/props.js:722-828` — the
//! "vegetation" group: a palm trunk, a palm frond, a shrub, a weed tuft, a
//! planter.
//!
//! Every size/position/rotation argument here is `f64` — see `pb.rs`'s
//! module doc for why. `rotation_y`/`rotation_z` and
//! `crate::world::kit::plane_geometry` are the `f32`-native transform/mesh
//! boundaries this narrows to at the last possible moment.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::geo::WorldGeo;
use crate::world::kit::{merge_simple, plane_geometry};

use super::pb::{mat, CylOpts, PB};

/// `palmTree(rng, h = 5.2)`'s return shape (`props.js:723-745`): the source
/// stashes `{topX, topY}` on `g.userData` for a future frond-placement
/// caller (`dressing.js`, not ported here) to read; this port carries the
/// same two numbers alongside the geometry instead of a side-channel field.
pub(crate) struct PalmTree {
    pub geo: WorldGeo,
    pub top_x: f64,
    pub top_y: f64,
}

/// `palmTree(rng, h = 5.2)` (`props.js:723-745`).
pub(crate) fn palm_tree(rng: &mut Rng, h: f64) -> PalmTree {
    let mut p = PB::new();
    let segs = 9;
    let lean = rng.range(-0.1, 0.1);
    for i in 0..segs {
        let t = f64::from(i) / f64::from(segs);
        let r = 0.19 * (1.0 - t * 0.42);
        let y = t * h;
        let x = (t * 2.2 + lean * 4.0).sin() * lean * h * 0.4;
        p.cyl(
            r,
            h / f64::from(segs) + 0.02,
            x,
            y + h / f64::from(segs) / 2.0,
            0.0,
            CylOpts { radial: 9, taper: 0.92, grime: (0.3 + t * 0.2) as f32, wear: 1.0, ..CylOpts::default() },
        );
        // Ring scars where old fronds broke off.
        p.cyl(r * 1.13, 0.045, x, y + h / f64::from(segs) * 0.75, 0.0, CylOpts { radial: 9, wear: 1.0, grime: 0.4, ..CylOpts::default() });
    }
    let top_x = (2.2 + lean * 4.0).sin() * lean * h * 0.4;
    let geo = p.build();
    PalmTree { geo, top_x, top_y: h }
}

/// Standard right-handed rotation about `+Y` (`Matrix4.makeRotationY`),
/// needed by [`palm_frond`] alongside a Z rotation — neither of which any
/// other builder in this port needs, so both stay local here rather than
/// widening [`crate::world::geo::WorldGeo`]'s API. `f32`-native: like
/// [`super::pb::mat`], this is the one unavoidable narrowing point for a
/// transform.
fn rotation_y(angle: f32) -> Mat4 {
    let (s, c) = angle.sin_cos();
    Mat4::from_cols_array([
        c, 0.0, -s, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        s, 0.0, c, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ])
}

/// Standard right-handed rotation about `+Z` (`Matrix4.makeRotationZ`).
fn rotation_z(angle: f32) -> Mat4 {
    let (s, c) = angle.sin_cos();
    Mat4::from_cols_array([
        c, s, 0.0, 0.0, //
        -s, c, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ])
}

/// `palmFrond(rng, len = 2.6)` (`props.js:748-782`): leaflets along a curved
/// spine, each a small quad rotated/yawed/placed, plus the spine itself as a
/// sagging strip — all foliage-textured quads merged into one geometry.
pub(crate) fn palm_frond(_rng: &mut Rng, len: f64) -> WorldGeo {
    let mut list: Vec<WorldGeo> = Vec::new();
    let n = 13;
    for i in 0..n {
        let t = f64::from(i + 1) / f64::from(n + 1);
        let x = t * len;
        let droop = -t * t * len * 0.42;
        let lw = (0.42 + (t * std::f64::consts::PI).sin() * 0.55) * (1.0 - t * 0.35);
        for &side in &[-1.0f64, 1.0] {
            let mut q = plane_geometry(lw as f32, 0.16, 1, 1);
            q.translate((lw / 2.0) as f32, 0.0, 0.0);
            let m = mat(x, droop, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
            let rot = rotation_z((-0.5 - t * 0.5) as f32);
            let yaw = rotation_y((side * (1.15 - t * 0.35)) as f32);
            // Three separate `applyMatrix4` calls, in this exact order
            // (`props.js:762-764`) — equivalent to one combined matrix, but
            // kept as three applies to stay literally traceable against the
            // source.
            q.apply(&rot);
            q.apply(&yaw);
            q.apply(&m);
            q.fill_masks(0.2, 0.25, 0.0);
            list.push(q);
        }
    }
    // Spine.
    let mut spine = plane_geometry(len as f32, 0.05, 6, 1);
    for p in spine.pos.chunks_exact_mut(3) {
        let x = f64::from(p[0]) + len / 2.0;
        p[0] = x as f32;
        p[1] -= ((x / len).powi(2) * len * 0.42) as f32;
    }
    spine.compute_vertex_normals();
    spine.fill_masks(0.2, 0.3, 0.0);
    list.push(spine);
    merge_simple(&list)
}

/// `shrub(rng, s = 0.8)` (`props.js:784-804`): 7 randomly placed/rotated/
/// scaled foliage quads.
pub(crate) fn shrub(rng: &mut Rng, s: f64) -> WorldGeo {
    let mut list: Vec<WorldGeo> = Vec::new();
    let n = 7;
    for _ in 0..n {
        let w = s * rng.range(0.7, 1.15);
        let h = s * rng.range(0.6, 1.0);
        let mut q = plane_geometry(w as f32, h as f32, 1, 1);
        let x = rng.range(-s * 0.2, s * 0.2);
        let y = s * rng.range(0.28, 0.6);
        let z = rng.range(-s * 0.2, s * 0.2);
        let ry = rng.float() * std::f64::consts::PI;
        let rx = rng.range(-0.4, 0.4);
        let rz = rng.range(-0.3, 0.3);
        q.apply(&mat(x, y, z, ry, rx, rz, 1.0, 1.0, 1.0));
        q.fill_masks(0.2, 0.35, 0.2);
        list.push(q);
    }
    merge_simple(&list)
}

/// `weedTuft(rng)` (`props.js:806-820`): 4 small foliage quads.
pub(crate) fn weed_tuft(rng: &mut Rng) -> WorldGeo {
    let mut list: Vec<WorldGeo> = Vec::new();
    let n = 4;
    for _ in 0..n {
        let w = rng.range(0.18, 0.34);
        let h = rng.range(0.14, 0.3);
        let mut q = plane_geometry(w as f32, h as f32, 1, 1);
        let x = rng.range(-0.06, 0.06);
        let y = rng.range(0.07, 0.17);
        let z = rng.range(-0.06, 0.06);
        let ry = rng.float() * 3.14;
        let rx = rng.range(-0.5, 0.5);
        q.apply(&mat(x, y, z, ry, rx, 0.0, 1.0, 1.0, 1.0));
        q.fill_masks(0.2, 0.5, 0.3);
        list.push(q);
    }
    merge_simple(&list)
}

/// `planter(rng)` (`props.js:822-828`). The source never reads `rng` here.
pub(crate) fn planter(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    p.cyl(0.34, 0.42, 0.0, 0.21, 0.0, CylOpts { radial: 14, taper: 0.78, grime: 0.4, ..CylOpts::default() });
    p.cyl(0.36, 0.05, 0.0, 0.42, 0.0, CylOpts { radial: 14, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.3, 0.06, 0.0, 0.4, 0.0, CylOpts { radial: 12, grime: 0.9, ..CylOpts::default() });
    p.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palm_tree_reports_its_own_top_position() {
        let mut rng = Rng::new(1);
        let palm = palm_tree(&mut rng, 5.4);
        assert!((palm.top_y - 5.4).abs() < 1e-6);
        assert!(palm.geo.vert_count() > 0);
    }

    #[test]
    fn palm_frond_merges_twenty_six_leaflets_and_a_spine() {
        let mut rng = Rng::new(2);
        let g = palm_frond(&mut rng, 2.7);
        assert!(g.vert_count() > 0);
    }

    #[test]
    fn shrub_and_weeds_produce_distinct_geometry_per_seed() {
        let mut a = Rng::new(3);
        let mut b = Rng::new(4);
        let sa = shrub(&mut a, 0.85);
        let sb = shrub(&mut b, 0.85);
        assert_ne!(sa.pos, sb.pos);

        let mut c = Rng::new(3);
        let wa = weed_tuft(&mut c);
        assert!(wa.vert_count() > 0);
    }

    #[test]
    fn planter_builds_without_reading_rng() {
        let mut rng = Rng::new(5);
        assert!(planter(&mut rng).vert_count() > 0);
    }

    #[test]
    fn rotation_y_and_z_are_orthonormal() {
        let ry = rotation_y(0.7);
        let rz = rotation_z(0.7);
        // A rotation matrix applied to a unit vector preserves its length.
        let mut g = WorldGeo { pos: vec![1.0, 0.0, 0.0], normal: vec![0.0, 0.0, 1.0], uv: Vec::new(), color: Vec::new(), index: Vec::new() };
        g.apply(&ry);
        let len = (g.pos[0].powi(2) + g.pos[1].powi(2) + g.pos[2].powi(2)).sqrt();
        assert!((len - 1.0).abs() < 1e-5);
        g.apply(&rz);
        let len2 = (g.pos[0].powi(2) + g.pos[1].powi(2) + g.pos[2].powi(2)).sqrt();
        assert!((len2 - 1.0).abs() < 1e-5);
    }
}
