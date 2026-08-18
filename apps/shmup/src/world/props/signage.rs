//! Ported from Claude-of-Duty `src/world/props.js:830-847` — the "signage"
//! group: a freestanding sign board, a hanging shop sign. Neither reads
//! `rng` in the source.

use crate::rng::Rng;
use crate::world::geo::WorldGeo;

use super::pb::{BoxOpts, CylOpts, PB};

/// `signBoard(rng, w = 1.5, h = 0.5)` (`props.js:831-838`).
pub(crate) fn sign_board(_rng: &mut Rng, w: f64, h: f64) -> WorldGeo {
    let mut p = PB::new();
    p.box_(w, h, 0.05, 0.0, 0.0, 0.0, BoxOpts { bevel: 0.008, grime: 0.25, ..BoxOpts::default() });
    p.box_(w + 0.05, 0.045, 0.07, 0.0, h / 2.0, 0.0, BoxOpts { bevel: 0.006, wear: 1.0, ..BoxOpts::default() });
    p.box_(w + 0.05, 0.045, 0.07, 0.0, -h / 2.0, 0.0, BoxOpts { bevel: 0.006, wear: 1.0, ..BoxOpts::default() });
    for &sx in &[-1.0f64, 1.0] {
        p.box_(0.03, 0.24, 0.12, sx * (w / 2.0 - 0.12), 0.0, -0.08, BoxOpts { grime: 0.5, ..BoxOpts::default() });
    }
    p.build()
}

/// `signHanging(rng, w = 0.9, h = 0.62)` (`props.js:840-847`).
pub(crate) fn sign_hanging(_rng: &mut Rng, w: f64, h: f64) -> WorldGeo {
    let mut p = PB::new();
    p.box_(w, h, 0.04, 0.0, -h / 2.0 - 0.12, 0.0, BoxOpts { bevel: 0.006, grime: 0.3, ..BoxOpts::default() });
    p.cyl(0.014, 0.14, -w / 2.0 + 0.08, -0.06, 0.0, CylOpts { radial: 6, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.014, 0.14, w / 2.0 - 0.08, -0.06, 0.0, CylOpts { radial: 6, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.018, w + 0.14, 0.0, 0.0, 0.0, CylOpts { radial: 6, rz: std::f64::consts::FRAC_PI_2, wear: 1.0, grime: 0.4, ..CylOpts::default() });
    p.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_board_and_hanging_sign_build_nonempty() {
        let mut rng = Rng::new(1);
        assert!(sign_board(&mut rng, 1.6, 0.55).vert_count() > 0);
        assert!(sign_hanging(&mut rng, 0.9, 0.62).vert_count() > 0);
    }
}
