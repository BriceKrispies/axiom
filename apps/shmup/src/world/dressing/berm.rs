//! Ported from Claude-of-Duty `src/world/util.js:670-721` — `driftBerm`.
//!
//! **Why it lives here and not in `crate::world::kit::primitives`.** Every
//! other `util.js` sub-primitive this port needed so far landed in
//! `world/kit/primitives.rs`, and that is where `driftBerm` structurally
//! belongs — it is a generic `util.js` geometry builder, not a dressing
//! concept. `dressing.js` is simply its only caller today, and this slice is
//! forbidden from editing any pre-existing file under `src/world/`. Move it
//! (and [`super::cable::catenary_tube`]) into `world/kit/primitives.rs` the
//! moment a second caller appears; nothing here depends on the location.

use crate::jsmath;
use crate::rng::Rng;
use crate::world::geo::WorldGeo;
use crate::world::noise::fbm3;

/// `driftBerm(rng, len, w, h, opts = {})` (`util.js:681-721`): a ridge of
/// blown sand or swept rubble piled against a wall.
///
/// Runs along local `+X` for `len`, `w` deep in Z with the TALL edge at
/// `z = 0` (put that edge against the wall) feathering to nothing at
/// `z = w`. The crest wanders and dips along its length and the toe is
/// scalloped, so it never reads as an extruded triangle.
///
/// Draws from `rng` **exactly once** (the `seed`), before any loop — every
/// other irregularity comes from `fbm3` keyed on that seed. `opts.nz`
/// defaults to `4` in the source; callers that pass `{ nz: 3 }` say so.
///
/// The source pushes a placeholder `(0, 1, 0)` normal per vertex and then
/// immediately calls `computeVertexNormals()`, which overwrites all of them.
/// That write is dead, and it is ported anyway (per the port recipe's "dead
/// computation in the source is still part of the source") — costs nothing
/// and keeps the transcription diffable.
pub fn drift_berm(rng: &mut Rng, len: f64, w: f64, h: f64, nz: u32) -> WorldGeo {
    let nx = (jsmath::round(len / 0.55) as i64).max(4) as u32;
    let mut pos: Vec<f32> = Vec::new();
    let mut nrm: Vec<f32> = Vec::new();
    let mut uv: Vec<f32> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let seed = rng.float() * 30.0;

    for i in 0..=nx {
        let u = f64::from(i) / f64::from(nx);
        let x = (u - 0.5) * len;
        // crest height wanders, and the ends taper into the ground
        let taper = 1.0f64.min(u.min(1.0 - u) * 6.0);
        let wob = 0.45 + fbm3(x * 0.7 + seed, 2.1, seed, 3) * 1.1;
        let ch = h * wob * taper;
        let cw = w * (0.6 + fbm3(x * 0.5 + seed + 7.0, 5.3, 1.9, 2) * 0.85);
        for j in 0..=nz {
            let v = f64::from(j) / f64::from(nz);
            // cosine section: steep at the wall, long feathered toe out into
            // the road
            let y = ch * ((v * std::f64::consts::PI) / 2.0).cos().powf(1.7);
            let rip = fbm3(x * 2.3 + seed, v * 3.1, 8.4, 2) - 0.5;
            pos.push(x as f32);
            pos.push((0.0f64).max(y + rip * h * 0.22 * (1.0 - v)) as f32);
            pos.push((v * cw) as f32);
            nrm.push(0.0);
            nrm.push(1.0);
            nrm.push(0.0);
            uv.push((x * 0.5) as f32);
            uv.push((v * cw * 0.5) as f32);
        }
    }

    let row = nz + 1;
    for i in 0..nx {
        for j in 0..nz {
            let a = i * row + j;
            idx.extend_from_slice(&[a, a + 1, a + row, a + 1, a + row + 1, a + row]);
        }
    }

    let mut g = WorldGeo {
        pos,
        normal: nrm,
        uv,
        color: Vec::new(),
        index: idx,
    };
    g.compute_vertex_normals();
    g
}

/// `driftBerm`'s `opts.nz` default (`util.js:682`).
pub const DRIFT_BERM_DEFAULT_NZ: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_berm_draws_exactly_one_rng_value() {
        let mut a = Rng::new(3);
        let mut b = Rng::new(3);
        drift_berm(&mut a, 4.0, 1.0, 0.3, DRIFT_BERM_DEFAULT_NZ);
        b.float();
        assert_eq!(a.state(), b.state());
    }

    #[test]
    fn drift_berm_grid_topology_follows_len_and_nz() {
        let mut rng = Rng::new(1);
        // len/0.55 = 4.0 -> nx = 4, nz = 3 -> (4+1)*(3+1) = 20 verts,
        // 4*3*2 = 24 triangles.
        let g = drift_berm(&mut rng, 2.2, 1.0, 0.3, 3);
        assert_eq!(g.vert_count(), 20);
        assert_eq!(g.tri_count(), 24);
    }

    #[test]
    fn drift_berm_length_clamps_the_segment_count_at_four() {
        let mut rng = Rng::new(1);
        let g = drift_berm(&mut rng, 0.1, 1.0, 0.3, 4);
        assert_eq!(g.vert_count(), 5 * 5);
    }

    #[test]
    fn drift_berm_never_dips_below_the_ground() {
        let mut rng = Rng::new(11);
        let g = drift_berm(&mut rng, 6.0, 1.2, 0.4, 4);
        assert!(g.pos.iter().skip(1).step_by(3).all(|&y| y >= 0.0));
    }
}
