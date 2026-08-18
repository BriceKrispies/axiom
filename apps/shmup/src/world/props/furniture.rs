//! Ported from Claude-of-Duty `src/world/props.js:405-494` — the "furniture"
//! group: a table, a market stall, a shelf unit, a mattress, a chair, a
//! cabinet.
//!
//! None of these read `rng` in the source (grep-verified per function); each
//! keeps the parameter for call-site parity with `registerProps`.

use crate::rng::Rng;
use crate::world::geo::WorldGeo;
use crate::world::kit::chamfer_box;
use crate::world::noise::fbm3;

use super::pb::{BoxOpts, PB};

/// `table(rng, w = 1.5, h = 0.78, d = 0.8)` (`props.js:406-417`).
pub(crate) fn table(_rng: &mut Rng, w: f64, h: f64, d: f64) -> WorldGeo {
    let mut p = PB::new();
    p.box_(w, 0.045, d, 0.0, h - 0.02, 0.0, BoxOpts { bevel: 0.008, wear: 1.0, ..BoxOpts::default() });
    p.box_(w - 0.1, 0.05, d - 0.1, 0.0, h - 0.075, 0.0, BoxOpts { bevel: 0.006, grime: 0.3, ..BoxOpts::default() });
    for &sx in &[-1.0f64, 1.0] {
        for &sz in &[-1.0f64, 1.0] {
            p.box_(
                0.07,
                h - 0.1,
                0.07,
                sx * (w / 2.0 - 0.09),
                (h - 0.1) / 2.0,
                sz * (d / 2.0 - 0.09),
                BoxOpts { bevel: 0.005, grime: 0.25, ..BoxOpts::default() },
            );
        }
    }
    p.build()
}

/// `stall(rng, w = 2.3)` (`props.js:419-438`): trestle table, back board,
/// canopy poles.
pub(crate) fn stall(_rng: &mut Rng, w: f64) -> WorldGeo {
    let mut p = PB::new();
    let h = 0.84;
    let d = 1.05;
    p.box_(w, 0.05, d, 0.0, h, 0.0, BoxOpts { bevel: 0.008, ..BoxOpts::default() });
    p.box_(w - 0.06, 0.09, d - 0.08, 0.0, h - 0.07, 0.0, BoxOpts { bevel: 0.006, grime: 0.35, ..BoxOpts::default() });
    for &sx in &[-1.0f64, 1.0] {
        p.box_(0.08, h - 0.05, 0.08, sx * (w / 2.0 - 0.1), (h - 0.05) / 2.0, d / 2.0 - 0.1, BoxOpts { grime: 0.3, ..BoxOpts::default() });
        p.box_(0.08, h - 0.05, 0.08, sx * (w / 2.0 - 0.1), (h - 0.05) / 2.0, -d / 2.0 + 0.1, BoxOpts { grime: 0.3, ..BoxOpts::default() });
        // Corner posts carrying the canopy.
        p.box_(0.06, 2.0, 0.06, sx * (w / 2.0 - 0.05), 1.0, -d / 2.0 + 0.06, BoxOpts { grime: 0.2, ..BoxOpts::default() });
        p.box_(0.06, 2.0, 0.06, sx * (w / 2.0 - 0.05), 1.0, d / 2.0 - 0.06, BoxOpts { grime: 0.2, ..BoxOpts::default() });
    }
    p.box_(w, 0.06, 0.06, 0.0, 1.98, -d / 2.0 + 0.06, BoxOpts::default());
    p.box_(w, 0.06, 0.06, 0.0, 1.98, d / 2.0 - 0.06, BoxOpts::default());
    // Shelf under the table.
    p.box_(w - 0.3, 0.03, d - 0.3, 0.0, 0.24, 0.0, BoxOpts { bevel: 0.004, grime: 0.45, ..BoxOpts::default() });
    p.build()
}

/// `shelfUnit(rng, w = 1.1, h = 1.9, d = 0.35)` (`props.js:440-450`).
pub(crate) fn shelf_unit(_rng: &mut Rng, w: f64, h: f64, d: f64) -> WorldGeo {
    let mut p = PB::new();
    for &sx in &[-1.0f64, 1.0] {
        p.box_(0.05, h, d, sx * (w / 2.0 - 0.025), h / 2.0, 0.0, BoxOpts { grime: 0.2, ..BoxOpts::default() });
    }
    let n = 4;
    for i in 0..n {
        let y = 0.22 + (f64::from(i) / f64::from(n - 1)) * (h - 0.4);
        p.box_(w - 0.06, 0.03, d, 0.0, y, 0.0, BoxOpts { bevel: 0.005, grime: 0.25, ..BoxOpts::default() });
    }
    p.box_(w, 0.03, 0.02, 0.0, h - 0.02, -d / 2.0 + 0.01, BoxOpts::default());
    p.build()
}

/// `mattress(rng)` (`props.js:452-471`): a chamfered box with an analytic
/// fabric-grime mask and a cosine sag in the middle.
pub(crate) fn mattress(_rng: &mut Rng) -> WorldGeo {
    let mut g = chamfer_box(1.85, 0.16, 0.85, 0.05);
    g.paint_masks(|x, y, z, _nx, ny, _nz, out, _i| {
        let n = fbm3(f64::from(x) * 3.0 + 4.0, f64::from(y) * 3.0, f64::from(z) * 3.0, 2);
        out[0] = 0.2;
        out[1] = (0.45 + n * 0.4) as f32;
        out[2] = ((-f64::from(ny)).max(0.0) * 0.4) as f32;
    });
    // Sag in the middle.
    for p in g.pos.chunks_exact_mut(3) {
        let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        if y > 0.0 {
            p[1] = (y - 0.035 * ((x / 1.85) * std::f64::consts::PI).cos() * ((z / 0.85) * std::f64::consts::PI).cos()) as f32;
        }
    }
    g.compute_vertex_normals();
    g.translate(0.0, 0.08, 0.0);
    g
}

/// `chair(rng)` (`props.js:473-483`).
pub(crate) fn chair(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let sh = 0.46;
    p.box_(0.42, 0.04, 0.4, 0.0, sh, 0.0, BoxOpts { bevel: 0.006, wear: 1.0, ..BoxOpts::default() });
    for &sx in &[-1.0f64, 1.0] {
        for &sz in &[-1.0f64, 1.0] {
            p.box_(0.04, sh, 0.04, sx * 0.18, sh / 2.0, sz * 0.17, BoxOpts { grime: 0.2, ..BoxOpts::default() });
        }
    }
    p.box_(0.42, 0.5, 0.035, 0.0, sh + 0.27, -0.18, BoxOpts { bevel: 0.005, rx: -0.08, ..BoxOpts::default() });
    p.box_(0.42, 0.06, 0.05, 0.0, sh + 0.48, -0.2, BoxOpts { bevel: 0.005, ..BoxOpts::default() });
    p.build()
}

/// `cabinet(rng, w = 0.9, h = 1.15, d = 0.44)` (`props.js:485-494`).
pub(crate) fn cabinet(_rng: &mut Rng, w: f64, h: f64, d: f64) -> WorldGeo {
    let mut p = PB::new();
    p.box_(w, h, d, 0.0, h / 2.0, 0.0, BoxOpts { bevel: 0.01, grime: 0.2, ..BoxOpts::default() });
    for &sx in &[-1.0f64, 1.0] {
        p.box_(w / 2.0 - 0.03, h - 0.12, 0.03, sx * (w / 4.0), h / 2.0, d / 2.0 + 0.01, BoxOpts { bevel: 0.005, wear: 1.0, ..BoxOpts::default() });
        p.box_(0.03, 0.1, 0.03, sx * 0.06, h / 2.0, d / 2.0 + 0.03, BoxOpts { wear: 1.0, ..BoxOpts::default() });
    }
    p.box_(w + 0.04, 0.04, d + 0.04, 0.0, h + 0.02, 0.0, BoxOpts { bevel: 0.008, wear: 1.0, grime: 0.3, ..BoxOpts::default() });
    p.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_furniture_piece_builds_nonempty_geometry() {
        let mut rng = Rng::new(1);
        assert!(table(&mut rng, 1.5, 0.78, 0.8).vert_count() > 0);
        assert!(stall(&mut rng, 2.3).vert_count() > 0);
        assert!(shelf_unit(&mut rng, 1.1, 1.9, 0.35).vert_count() > 0);
        assert!(mattress(&mut rng).vert_count() > 0);
        assert!(chair(&mut rng).vert_count() > 0);
        assert!(cabinet(&mut rng, 0.9, 1.15, 0.44).vert_count() > 0);
    }

    #[test]
    fn mattress_sags_downward_at_its_centre_but_not_at_its_underside() {
        let mut rng = Rng::new(1);
        let g = mattress(&mut rng);
        // Only the TOP face (y > 0 before the sag) is displaced (`if (y > 0)
        // pa.setY(...)`, `props.js:465`); the sag is a cosine bowl so it is
        // strongest at the centre and vanishes at the edges — top-face
        // vertices should therefore span a real range of y values, not sit
        // at one flat height.
        let top_ys: Vec<f32> = g.pos.chunks_exact(3).map(|p| p[1]).filter(|&y| y > 0.1).collect();
        assert!(!top_ys.is_empty(), "expected some top-face vertices above y=0.1");
        let min = top_ys.iter().copied().fold(f32::INFINITY, f32::min);
        let max = top_ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(max - min > 0.01, "expected the sag to spread top-face y values, got range [{min}, {max}]");
    }
}
