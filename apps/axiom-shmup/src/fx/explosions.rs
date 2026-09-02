//! Ported from Claude-of-Duty `src/fx/explosions.js:1-209` — the whole file.
//!
//! Explosions, ordered the way a real detonation is ordered, because the
//! order is the whole effect: a white-hot core gone in 60 ms plus a real
//! light flash, a fireball expanding on a decelerating curve, a shockwave
//! ring that outruns it, a debris cone and ground dust ring thrown radially,
//! and a smoke column that keeps rising long after the fire has gone out.

use crate::fx::system::FxSystem;


/// `explode(fx, o)`'s parameters, `explosions.js:20-27` (the `o` object).
pub struct ExplosionOpts {
    pub position: (f64, f64, f64),
    pub radius: f64,
    /// `o.up ?? {x:0,y:1,z:0}`.
    pub up: (f64, f64, f64),
}

impl Default for ExplosionOpts {
    fn default() -> Self {
        ExplosionOpts {
            position: (0.0, 0.0, 0.0),
            radius: 5.0,
            up: (0.0, 1.0, 0.0),
        }
    }
}

/// `explode(fx, o)`, `explosions.js:20-209`.
pub fn explode(fx: &mut FxSystem, o: &ExplosionOpts) {
    let r = o.radius.max(0.6);
    let (px, py, pz) = o.position;
    let site = crate::fx::burst::Site::blast(fx, o.position, o.up, r);

    crate::fx::burst::run_all(fx, &crate::fx::recipes::EXPLOSION_FIRE, site);
    fx.haze_ring(px, py, pz, r * 0.25, 9.0, 0.34, 2.2);
    crate::fx::burst::run_all(fx, &crate::fx::recipes::EXPLOSION_BLAST, site);
    // ---- lingering smoke column -----------------------------------------------
    fx.add_smoke_column(
        px,
        py + r * 0.1,
        pz,
        crate::fx::system::SmokeColumnOpts {
            radius: r * 0.35,
            duration: 1.5,
            rate: 9.0,
            rise: 1.6,
            dark: 0.12,
            life: 3.4,
            growth: 3.2,
        },
    );

    // ---- light + ground scorch -------------------------------------------------
    fx.lights.flash(px, py + r * 0.15, pz, 1.0, 0.72, 0.4, 420.0 * (r / 4.0), 0.45, 8.0, r * 8.0, 4.0);
    fx.scorch(px, py, pz, r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::system::FxSystem;

    #[test]
    fn explode_spawns_additive_and_lit_particles_and_a_light() {
        let mut fx = FxSystem::test_instance(1);
        let before_add = fx.add.spawned();
        let before_lit = fx.lit.spawned();
        explode(&mut fx, &ExplosionOpts::default());
        assert!(fx.add.spawned() > before_add);
        assert!(fx.lit.spawned() > before_lit);
        assert!(fx.lights.slots.iter().any(|s| s.intensity > 0.0 || s.priority > 0.0));
    }

    #[test]
    fn radius_is_floored_at_the_minimum() {
        let mut fx = FxSystem::test_instance(2);
        let opts = ExplosionOpts {
            radius: 0.0,
            ..ExplosionOpts::default()
        };
        // must not panic and must still spawn a core flash sized off `r.max(0.6)`.
        explode(&mut fx, &opts);
        assert!(fx.add.spawned() > 0);
    }
}
