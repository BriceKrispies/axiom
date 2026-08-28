//! Ported from Claude-of-Duty `src/world/props.js:850-896` — `burntCar`, a
//! burnt-out saloon built as one merged geometry: sagging roof, blown glass,
//! missing wheels, doors hanging. Exported (`export function burntCar`) in
//! the source; called from `dressing.js`'s `registerDressingProps` (not
//! ported by this slice), never from `registerProps` itself — kept public
//! here for the same reason.

use crate::rng::Rng;
use crate::world::geo::WorldGeo;

use super::pb::{BoxOpts, CylOpts, PB};

/// `burntCar(rng)` (`props.js:855-896`). The source never reads `rng` here.
pub(crate) fn burnt_car(_rng: &mut Rng) -> WorldGeo {
    let mut body = PB::new();
    let l = 4.35;
    let w = 1.78;
    // Main body tub.
    body.box_(w, 0.5, l, 0.0, 0.62, 0.0, BoxOpts { bevel: 0.05, grime: 0.5, ..BoxOpts::default() });
    body.box_(w * 0.99, 0.34, l * 0.62, 0.0, 0.95, -0.15, BoxOpts { bevel: 0.06, grime: 0.5, ..BoxOpts::default() });
    // Bonnet + boot.
    body.box_(w * 0.94, 0.13, l * 0.3, 0.0, 0.94, l * 0.33, BoxOpts { bevel: 0.03, rx: 0.06, wear: 1.0, ..BoxOpts::default() });
    body.box_(w * 0.94, 0.13, l * 0.22, 0.0, 0.95, -l * 0.38, BoxOpts { bevel: 0.03, rx: -0.08, wear: 1.0, ..BoxOpts::default() });
    // Cabin: A/B/C pillars and a sagging roof.
    let rh = 1.42;
    for &sx in &[-1.0f64, 1.0] {
        body.box_(0.09, 0.55, 0.1, sx * (w / 2.0 - 0.08), 1.2, l * 0.14, BoxOpts { rx: 0.35, grime: 0.4, ..BoxOpts::default() });
        body.box_(0.09, 0.5, 0.1, sx * (w / 2.0 - 0.08), 1.22, -l * 0.02, BoxOpts { grime: 0.4, ..BoxOpts::default() });
        body.box_(0.11, 0.52, 0.12, sx * (w / 2.0 - 0.08), 1.2, -l * 0.2, BoxOpts { rx: -0.3, grime: 0.4, ..BoxOpts::default() });
        // Sills and door skins.
        body.box_(0.07, 0.42, l * 0.42, sx * (w / 2.0 - 0.03), 0.68, 0.05, BoxOpts { bevel: 0.02, wear: 1.0, grime: 0.5, ..BoxOpts::default() });
    }
    body.box_(w * 0.86, 0.07, l * 0.36, 0.0, rh - 0.04, -l * 0.04, BoxOpts { bevel: 0.04, wear: 1.0, grime: 0.6, ..BoxOpts::default() });
    // Wheel arches.
    for &sx in &[-1.0f64, 1.0] {
        for &sz in &[-1.0f64, 1.0] {
            body.cyl(
                0.42,
                0.1,
                sx * (w / 2.0 - 0.04),
                0.5,
                sz * l * 0.31,
                CylOpts { radial: 12, rz: std::f64::consts::FRAC_PI_2, open: true, grime: 0.5, ..CylOpts::default() },
            );
        }
    }
    // Bumpers.
    body.box_(w * 0.98, 0.22, 0.16, 0.0, 0.5, l / 2.0 - 0.05, BoxOpts { bevel: 0.03, wear: 1.0, grime: 0.5, ..BoxOpts::default() });
    body.box_(w * 0.98, 0.22, 0.16, 0.0, 0.5, -l / 2.0 + 0.05, BoxOpts { bevel: 0.03, wear: 1.0, grime: 0.5, rz: 0.05, ..BoxOpts::default() });

    let mut g = body.build();
    g.paint_masks(|_x, y, z, _nx, ny, _nz, out, _i| {
        // Soot: heaviest around the cabin and upward faces.
        let soot = 0.45 + 0.5 * ny.max(0.0) + 0.3 * (1.0 - z.abs() / 1.6).max(0.0);
        out[1] = (out[1] + soot * 0.8).min(1.0);
        out[0] = (out[0] * 0.8).min(1.0);
        let _ = y;
    });
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burnt_car_builds_a_single_merged_body() {
        let mut rng = Rng::new(1);
        let g = burnt_car(&mut rng);
        assert!(g.vert_count() > 0);
        assert!(g.color.iter().any(|&c| c > 0.0));
    }
}
