//! Ported from Claude-of-Duty `src/world/props.js:497-588` — the "services"
//! group: an AC unit, a satellite dish, a water tank, a roof vent, a street
//! lamp, and its glass diffuser.
//!
//! None of `acUnit`/`satDish`/`waterTank`/`roofVent`/`streetLamp` read `rng`
//! in the source (grep-verified per function; every draw here is
//! deterministic from its loop index); each keeps the parameter for
//! call-site parity with `registerProps`. `lampGlass()` takes no parameters
//! at all in the source.

use axiom_math::{Mat4, Vec3};

use crate::rng::Rng;
use crate::weapons::geometry::primitives::sphere_geometry;
use crate::world::geo::WorldGeo;
use crate::world::kit::chamfer_box;

use super::mesh::auto_edge_wear;
use super::pb::{BoxOpts, CylOpts, GeoOpts, PB};

/// `acUnit(rng)` (`props.js:497-522`).
pub(crate) fn ac_unit(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let w = 0.78;
    let h = 0.55;
    let d = 0.34;
    p.box_(w, h, d, 0.0, 0.0, 0.0, BoxOpts { bevel: 0.012, grime: 0.35, ..BoxOpts::default() });
    // Louvre grille on the face.
    for i in 0..7 {
        p.box_(
            w - 0.1,
            0.035,
            0.02,
            0.0,
            -h / 2.0 + 0.08 + f64::from(i) * 0.06,
            d / 2.0 + 0.005,
            BoxOpts { bevel: 0.003, rx: 0.35, wear: 1.0, ..BoxOpts::default() },
        );
    }
    // Fan ring.
    p.cyl(0.19, 0.03, 0.0, 0.02, d / 2.0 + 0.02, CylOpts { radial: 16, rx: std::f64::consts::FRAC_PI_2, wear: 1.0, ..CylOpts::default() });
    // Wall brackets.
    for &sx in &[-1.0f64, 1.0] {
        p.box_(0.05, 0.05, 0.5, sx * (w / 2.0 - 0.05), -h / 2.0 + 0.03, -d / 2.0 - 0.16, BoxOpts { grime: 0.5, ..BoxOpts::default() });
        p.box_(0.05, 0.34, 0.05, sx * (w / 2.0 - 0.05), -h / 2.0 - 0.14, -d / 2.0 - 0.36, BoxOpts { grime: 0.5, rz: 0.5, ..BoxOpts::default() });
    }
    // Condensate drip stain hanger.
    p.cyl(0.012, 0.5, w / 2.0 - 0.12, -h / 2.0 - 0.24, 0.0, CylOpts { radial: 6, grime: 0.6, ..CylOpts::default() });
    p.build()
}

/// `satDish(rng)` (`props.js:524-536`).
///
/// **Reuses `weapons::geometry::primitives::sphere_geometry`** (the same
/// faithful `THREE.SphereGeometry` port `dome()`/`props::mesh::sack_geometry`
/// build on) for the dish's partial sphere, then applies the source's own
/// `scale`/`rotateX` as [`WorldGeo::apply`]/[`WorldGeo::rotate_x`] calls.
pub(crate) fn sat_dish(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    let raw = sphere_geometry(0.42, 16, 10, 0.0, std::f64::consts::TAU, 0.0, 0.55);
    let mut dish = WorldGeo {
        pos: raw.pos,
        normal: raw.normal,
        uv: raw.uv,
        color: Vec::new(),
        index: raw.index,
    };
    dish.apply(&Mat4::scale(Vec3::new(1.0, 0.42, 1.0)));
    dish.rotate_x(-2.1);
    auto_edge_wear(&mut dish, 0.03, 0.8);
    p.geo(dish, 0.0, 0.55, 0.1, GeoOpts { auto_wear: false, grime: 0.3, ..GeoOpts::default() });
    p.cyl(0.03, 0.5, 0.0, 0.4, -0.12, CylOpts { radial: 8, rx: 0.5, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.045, 0.55, 0.0, 0.27, -0.22, CylOpts { radial: 8, grime: 0.4, ..CylOpts::default() });
    p.box_(0.24, 0.03, 0.24, 0.0, 0.02, -0.22, BoxOpts { bevel: 0.005, grime: 0.6, ..BoxOpts::default() });
    p.cyl(0.028, 0.16, 0.0, 0.62, 0.34, CylOpts { radial: 6, rx: 1.1, wear: 1.0, ..CylOpts::default() });
    p.build()
}

/// `waterTank(rng)` (`props.js:538-547`).
pub(crate) fn water_tank(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    p.cyl(0.55, 1.0, 0.0, 0.5, 0.0, CylOpts { radial: 18, grime: 0.3, ..CylOpts::default() });
    p.cyl(0.56, 0.05, 0.0, 0.99, 0.0, CylOpts { radial: 18, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.18, 0.09, 0.16, 1.05, 0.0, CylOpts { radial: 12, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.03, 0.5, -0.5, 0.2, 0.0, CylOpts { radial: 6, grime: 0.5, rz: 0.3, ..CylOpts::default() });
    // Cradle.
    for &sz in &[-1.0f64, 1.0] {
        p.box_(1.2, 0.09, 0.09, 0.0, 0.045, sz * 0.36, BoxOpts { grime: 0.5, ..BoxOpts::default() });
    }
    p.build()
}

/// `roofVent(rng)` (`props.js:549-556`).
pub(crate) fn roof_vent(_rng: &mut Rng) -> WorldGeo {
    let mut p = PB::new();
    p.box_(0.5, 0.3, 0.5, 0.0, 0.15, 0.0, BoxOpts { bevel: 0.01, grime: 0.4, ..BoxOpts::default() });
    p.cyl(0.17, 0.36, 0.0, 0.48, 0.0, CylOpts { radial: 12, grime: 0.3, ..CylOpts::default() });
    p.cyl(0.24, 0.06, 0.0, 0.68, 0.0, CylOpts { radial: 12, wear: 1.0, ..CylOpts::default() });
    p.cyl(0.2, 0.05, 0.0, 0.74, 0.0, CylOpts { radial: 12, taper: 0.3, wear: 1.0, ..CylOpts::default() });
    p.build()
}

/// `streetLamp(rng, h = 5.4)` (`props.js:558-581`): a curved arm made of
/// short cylinder segments, with a diagonal stay back to the post — without
/// it the head reads as a box floating a metre off the column.
pub(crate) fn street_lamp(_rng: &mut Rng, h: f64) -> WorldGeo {
    let mut p = PB::new();
    p.cyl(0.13, 0.35, 0.0, 0.17, 0.0, CylOpts { radial: 12, grime: 0.6, ..CylOpts::default() });
    p.cyl(0.075, h, 0.0, h / 2.0, 0.0, CylOpts { radial: 10, taper: 0.7, grime: 0.25, ..CylOpts::default() });
    let segs = 5;
    for i in 0..segs {
        let t = f64::from(i) / f64::from(segs - 1);
        let a = t * 1.35;
        p.cyl(
            0.055,
            0.44,
            a.sin() * 0.62 * (0.4 + t),
            h - 0.1 + a.cos() * 0.34 * t,
            0.0,
            CylOpts { radial: 8, rz: -a, grime: 0.3, ..CylOpts::default() },
        );
    }
    p.cyl(0.028, 0.95, 0.32, h - 0.42, 0.0, CylOpts { radial: 6, rz: -0.72, grime: 0.4, ..CylOpts::default() });
    p.box_(0.1, 0.16, 0.1, 0.05, h - 0.72, 0.0, BoxOpts { bevel: 0.01, grime: 0.45, ..BoxOpts::default() });
    p.box_(0.5, 0.13, 0.28, 0.86, h + 0.06, 0.0, BoxOpts { bevel: 0.02, rz: -0.16, grime: 0.35, ..BoxOpts::default() });
    p.box_(0.42, 0.06, 0.22, 0.88, h - 0.02, 0.0, BoxOpts { bevel: 0.01, rz: -0.16, wear: 1.0, ..BoxOpts::default() });
    p.build()
}

/// `lampGlass()` (`props.js:584-588`): the lamp's diffuser, kept separate so
/// it can use a glassy material. Takes no parameters in the source.
pub(crate) fn lamp_glass() -> WorldGeo {
    let mut g = chamfer_box(0.4, 0.05, 0.2, 0.01);
    g.fill_masks(0.2, 0.1, 0.0);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_service_prop_builds_nonempty_geometry() {
        let mut rng = Rng::new(1);
        assert!(ac_unit(&mut rng).vert_count() > 0);
        assert!(sat_dish(&mut rng).vert_count() > 0);
        assert!(water_tank(&mut rng).vert_count() > 0);
        assert!(roof_vent(&mut rng).vert_count() > 0);
        assert!(street_lamp(&mut rng, 5.4).vert_count() > 0);
        assert!(lamp_glass().vert_count() > 0);
    }

    #[test]
    fn street_lamp_head_reaches_up_near_the_pole_height() {
        let mut rng = Rng::new(2);
        let h = 5.4;
        let g = street_lamp(&mut rng, h);
        let y_max = g.pos.iter().skip(1).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(f64::from(y_max) > h - 0.5, "lamp head should reach near the pole top, y_max={y_max}");
    }
}
